//! The PDF engine: a single background worker thread that owns PDFium.
//!
//! PDFium is not thread-safe, so *every* call into it happens on this one
//! thread. The UI talks to it through a command channel and receives results
//! through an event channel; it never blocks on rendering. This gives us:
//!
//! * a responsive UI at any document size,
//! * no locks around the engine, no data races, no `unsafe` in the UI,
//! * a natural place to bound memory (the worker's page/text caches) and to
//!   implement cancellation (stale results are simply dropped by the UI).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

use super::ffi::{self, Api, FPDF_DOCUMENT, FPDF_FILEWRITE};
use crate::geom::Size;
use crate::ui::Bitmap;

/// Message used when a document needs a password before it can be opened.
pub const ERR_PASSWORD_REQUIRED: &str = "This document is password protected.";
/// Message used when the supplied password was rejected.
pub const ERR_PASSWORD_WRONG: &str = "The password is incorrect.";

/// Why an open attempt failed, as far as the user interface cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFailure {
    /// The document is encrypted and no password was supplied.
    PasswordRequired,
    /// A password was supplied, but it did not unlock the document.
    WrongPassword,
    /// Anything else (missing file, corrupt data, unsupported encryption …).
    Other,
}

/// Classifies an open error without guessing from arbitrary text: only the two
/// messages this module produces are treated as password related.
pub fn classify_open_error(message: &str) -> OpenFailure {
    let message = message.trim();
    if message == ERR_PASSWORD_REQUIRED {
        OpenFailure::PasswordRequired
    } else if message == ERR_PASSWORD_WRONG {
        OpenFailure::WrongPassword
    } else {
        OpenFailure::Other
    }
}

/// Identifies an open document inside the worker.
pub type DocId = u64;

/// Upper bound on pages per document.
///
/// A page tree that claims millions of pages would allocate gigabytes of size
/// and rotation bookkeeping before a single page is drawn, so such documents are
/// rejected instead of being trusted.
pub const MAX_PAGES: usize = 200_000;

/// Render request identity: the cache key and the coalescing key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderKey {
    pub page: u32,
    /// Device scale in thousandths of a pixel per PDF point.
    pub scale_milli: u32,
    /// Clockwise rotation in degrees (0/90/180/270), on top of the page's own.
    pub rotation: u16,
}

impl RenderKey {
    pub fn new(page: u32, scale: f32, rotation: u16) -> RenderKey {
        RenderKey {
            page,
            scale_milli: (scale * 1000.0).round().clamp(1.0, 64_000.0) as u32,
            rotation: rotation % 360,
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale_milli as f32 / 1000.0
    }
}

/// Metadata gathered when a document is opened.
#[derive(Debug, Clone, Default)]
pub struct DocInfo {
    pub page_count: u32,
    /// Page sizes in PDF points, ignoring the user's rotation.
    pub page_sizes: Vec<Size>,
    /// Intrinsic `/Rotate` of every page (degrees clockwise).
    pub page_rotations: Vec<u16>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub file_version: i32,
    pub encrypted: bool,
    pub security_revision: i32,
    pub permissions: u32,
    /// PDF page labels ("iv", "A-1", ...) when the document defines them.
    pub page_labels: Vec<String>,
}

impl DocInfo {
    /// Size of `page` (0-based) as it should be displayed for `user_rotation`.
    pub fn display_size(&self, page: u32, user_rotation: u16) -> Size {
        let base = self
            .page_sizes
            .get(page as usize)
            .copied()
            .unwrap_or(Size::new(612.0, 792.0));
        let intrinsic = self
            .page_rotations
            .get(page as usize)
            .copied()
            .unwrap_or(0);
        rotated(base, intrinsic.wrapping_add(user_rotation) % 360)
    }
}

/// Applies a multiple-of-90 rotation to a size.
pub fn rotated(size: Size, rotation: u16) -> Size {
    match rotation % 360 {
        90 | 270 => Size::new(size.h, size.w),
        _ => size,
    }
}

/// A single text character with its bounding box in PDF points (y up).
#[derive(Debug, Clone, Copy)]
pub struct CharBox {
    pub ch: char,
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
}

impl CharBox {
    pub fn width(&self) -> f32 {
        (self.right - self.left).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.top - self.bottom).max(0.0)
    }
}

/// Extracted text of one page plus per-character boxes for selection.
#[derive(Debug, Clone, Default)]
pub struct PageText {
    pub text: String,
    pub chars: Vec<CharBox>,
}

impl PageText {
    /// Character index at a point in PDF coordinates, or `None`.
    pub fn char_at(&self, x: f32, y: f32, tolerance: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.chars.iter().enumerate() {
            if x >= c.left - tolerance
                && x <= c.right + tolerance
                && y >= c.bottom - tolerance
                && y <= c.top + tolerance
            {
                let cx = (c.left + c.right) * 0.5;
                let d = (cx - x).abs();
                if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((i, d));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// Text covered by a normalised PDF-space rectangle, with spaces where the
    /// original had line breaks.
    pub fn text_in_rect(&self, left: f32, bottom: f32, right: f32, top: f32) -> String {
        let mut out = String::new();
        let mut last_line = f32::NAN;
        for c in &self.chars {
            let cx = (c.left + c.right) * 0.5;
            let cy = (c.bottom + c.top) * 0.5;
            if cx < left || cx > right || cy < bottom || cy > top {
                continue;
            }
            if last_line.is_nan() {
                last_line = cy;
            } else if (cy - last_line).abs() > c.height() * 0.5 {
                out.push('\n');
                last_line = cy;
            }
            out.push(c.ch);
        }
        out
    }

    /// Index of the character closest to a point (used for click-to-caret).
    pub fn nearest_char(&self, x: f32, y: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.chars.iter().enumerate() {
            let cx = if x < c.left {
                c.left
            } else if x > c.right {
                c.right
            } else {
                x
            };
            let cy = (c.bottom + c.top) * 0.5;
            let d = (cx - x) * (cx - x) + (cy - y) * (cy - y);
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }
}

/// A search hit inside a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchHit {
    pub page: u32,
    pub char_index: u32,
    pub char_count: u32,
    /// Union rectangle of the hit in PDF coordinates (y up).
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

/// One entry of the document outline (table of contents).
#[derive(Debug, Clone, Default)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<u32>,
    pub uri: Option<String>,
    pub children: Vec<OutlineItem>,
}

/// Where a link points.
#[derive(Debug, Clone)]
pub enum LinkTarget {
    Page(u32),
    Uri(String),
}

/// A clickable region on a page, in PDF coordinates (y up).
#[derive(Debug, Clone)]
pub struct LinkRect {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
    pub target: LinkTarget,
}

/// Commands sent from the UI to the worker.
#[derive(Debug)]
pub enum Command {
    Open {
        id: DocId,
        path: PathBuf,
        password: Option<String>,
    },
    Close {
        id: DocId,
    },
    Render {
        id: DocId,
        key: RenderKey,
    },
    PageText {
        id: DocId,
        page: u32,
        token: u64,
    },
    Search {
        id: DocId,
        page: u32,
        query: String,
        token: u64,
        /// Character index to start from, or -1 to start at the beginning.
        start_index: i32,
        backwards: bool,
        match_case: bool,
    },
    Outline {
        id: DocId,
        token: u64,
    },
    Links {
        id: DocId,
        page: u32,
        token: u64,
    },
    /// Writes a copy of the document with `rotations` applied to pages.
    ExportRotated {
        id: DocId,
        rotations: Vec<(u32, u16)>,
        dest: PathBuf,
        token: u64,
    },
    Shutdown,
}

/// Events produced by the worker.
pub enum Event {
    Opened {
        id: DocId,
        result: Result<Arc<DocInfo>, String>,
        path: PathBuf,
    },
    Rendered {
        id: DocId,
        key: RenderKey,
        bitmap: Arc<Bitmap>,
        millis: f32,
    },
    Text {
        id: DocId,
        page: u32,
        token: u64,
        text: Arc<PageText>,
    },
    Searched {
        id: DocId,
        page: u32,
        token: u64,
        hits: Vec<SearchHit>,
    },
    Outline {
        id: DocId,
        token: u64,
        items: Vec<OutlineItem>,
    },
    Links {
        id: DocId,
        page: u32,
        token: u64,
        links: Vec<LinkRect>,
    },
    Exported {
        token: u64,
        result: Result<String, String>,
    },
    /// A command referenced a document that is not open any more.
    UnknownDoc { id: DocId },
    /// The engine itself failed to initialise (e.g. PDFium missing).
    EngineError { message: String },
}

struct OpenDoc {
    doc: FPDF_DOCUMENT,
    #[allow(dead_code)]
    info: Arc<DocInfo>,
    text_cache: HashMap<u32, Arc<PageText>>,
    text_order: Vec<u32>,
}

impl Drop for OpenDoc {
    fn drop(&mut self) {
        // Closed explicitly by the worker; nothing to do here.
    }
}

/// Handle used by the UI to talk to the engine thread.
pub struct Engine {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    joined: Option<std::thread::JoinHandle<()>>,
    pub pdfium_path: Option<PathBuf>,
    pub init_error: Option<String>,
}

impl Engine {
    /// Starts the worker thread. Engine availability is reported asynchronously
    /// so startup is never blocked by loading the shared library.
    pub fn start() -> Engine {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (evt_tx, evt_rx) = channel::<Event>();
        let init_rx = {
            let (tx, rx) = channel::<Result<PathBuf, String>>();
            let handle = std::thread::Builder::new()
                .name("pdfium-worker".into())
                .spawn(move || {
                    worker_main(cmd_rx, evt_tx, tx);
                })
                .expect("failed to spawn PDF worker thread");
            (handle, rx)
        };
        let (handle, init) = init_rx;
        let (pdfium_path, init_error) = match init.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(path)) => (Some(path), None),
            Ok(Err(message)) => (None, Some(message)),
            Err(_) => (
                None,
                Some("the PDF engine did not finish initialising in time".to_string()),
            ),
        };
        Engine {
            tx: cmd_tx,
            rx: evt_rx,
            joined: Some(handle),
            pdfium_path,
            init_error,
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(evt) => Some(evt),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Drains all pending events (bounded to keep a frame responsive).
    pub fn drain(&self, max: usize) -> Vec<Event> {
        let mut out = Vec::new();
        while out.len() < max {
            match self.try_recv() {
                Some(evt) => out.push(evt),
                None => break,
            }
        }
        out
    }

    pub fn is_ready(&self) -> bool {
        self.pdfium_path.is_some()
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(handle) = self.joined.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(handle) = self.joined.take() {
            let _ = handle.join();
        }
    }
}

struct Worker {
    api: &'static Api,
    docs: HashMap<DocId, OpenDoc>,
    /// Composite glyph counts / people meters are irrelevant; we only track
    /// memory indirectly through the page caches below.
    text_cache_limit: usize,
}

fn worker_main(
    cmd_rx: Receiver<Command>,
    evt_tx: Sender<Event>,
    init_tx: Sender<Result<PathBuf, String>>,
) {
    let api = match ffi::global() {
        Ok(api) => api,
        Err(message) => {
            let _ = init_tx.send(Err(message.clone()));
            // Keep draining commands so the UI never blocks on a dead channel.
            while let Ok(cmd) = cmd_rx.recv() {
                if let Command::Shutdown = cmd {
                    break;
                }
                let _ = evt_tx.send(Event::EngineError {
                    message: message.clone(),
                });
            }
            return;
        }
    };
    let path = Box::leak(Box::new(ffi::search_paths()))
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .unwrap_or_default();
    let _ = init_tx.send(Ok(path));
    let mut worker = Worker {
        api,
        docs: HashMap::new(),
        text_cache_limit: 12,
    };
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Command::Shutdown => break,
            cmd => worker.handle(cmd, &evt_tx),
        }
    }
    // Close every document this worker opened; the library itself stays loaded
    // for the lifetime of the process.
    let mut docs = std::mem::take(&mut worker.docs);
    for (_, d) in docs.drain() {
        let _guard = ffi::CALLS.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: serialised by CALLS, document handle still valid.
        unsafe { (worker.api.FPDF_CloseDocument)(d.doc) };
    }
}

impl Worker {
    fn handle(&mut self, cmd: Command, evt: &Sender<Event>) {
        // PDFium is not thread-safe: hold the process-wide lock for the whole
        // command so that concurrent workers (e.g. parallel tests) serialise.
        let _guard = ffi::CALLS.lock().unwrap_or_else(|e| e.into_inner());
        match cmd {
            Command::Open { id, path, password } => {
                let result = self.open_document(path.clone(), password.as_deref());
                match result {
                    Ok((doc, info)) => {
                        if let Some(previous) = self.docs.insert(
                            id,
                            OpenDoc {
                                doc,
                                info: info.clone(),
                                text_cache: HashMap::new(),
                                text_order: Vec::new(),
                            },
                        ) {
                            unsafe { (self.api.FPDF_CloseDocument)(previous.doc) };
                        }
                        let _ = evt.send(Event::Opened {
                            id,
                            result: Ok(info),
                            path,
                        });
                    }
                    Err(e) => {
                        let _ = evt.send(Event::Opened {
                            id,
                            result: Err(e),
                            path,
                        });
                    }
                }
            }
            Command::Close { id } => {
                if let Some(doc) = self.docs.remove(&id) {
                    unsafe { (self.api.FPDF_CloseDocument)(doc.doc) };
                }
            }
            Command::Render { id, key } => {
                let Some(doc) = self.docs.get(&id) else {
                    let _ = evt.send(Event::UnknownDoc { id });
                    return;
                };
                let started = Instant::now();
                match self.render(doc.doc, key) {
                    Ok(bitmap) => {
                        let _ = evt.send(Event::Rendered {
                            id,
                            key,
                            bitmap: Arc::new(bitmap),
                            millis: started.elapsed().as_secs_f32() * 1000.0,
                        });
                    }
                    Err(_) => {
                        // Rendering failures are non-fatal: the UI keeps the
                        // placeholder and retries when the view changes.
                    }
                }
            }
            Command::PageText { id, page, token } => {
                let Some(handle) = self.docs.get(&id).map(|d| d.doc) else {
                    let _ = evt.send(Event::UnknownDoc { id });
                    return;
                };
                if let Some(cached) = self
                    .docs
                    .get(&id)
                    .and_then(|d| d.text_cache.get(&page))
                    .cloned()
                {
                    let _ = evt.send(Event::Text {
                        id,
                        page,
                        token,
                        text: cached,
                    });
                    return;
                }
                let text = Arc::new(self.load_text(handle, page));
                if let Some(doc) = self.docs.get_mut(&id) {
                    doc.text_cache.insert(page, text.clone());
                    doc.text_order.retain(|p| *p != page);
                    doc.text_order.push(page);
                    while doc.text_order.len() > self.text_cache_limit {
                        let evicted = doc.text_order.remove(0);
                        doc.text_cache.remove(&evicted);
                    }
                }
                let _ = evt.send(Event::Text {
                    id,
                    page,
                    token,
                    text,
                });
            }
            Command::Search {
                id,
                page,
                query,
                token,
                start_index,
                backwards,
                match_case,
            } => {
                let Some(doc) = self.docs.get(&id) else {
                    let _ = evt.send(Event::UnknownDoc { id });
                    return;
                };
                let hits = self.search(doc.doc, page, &query, start_index, backwards, match_case);
                let _ = evt.send(Event::Searched {
                    id,
                    page,
                    token,
                    hits,
                });
            }
            Command::Outline { id, token } => {
                let Some(doc) = self.docs.get(&id) else {
                    let _ = evt.send(Event::UnknownDoc { id });
                    return;
                };
                let items = self.outline(doc.doc);
                let _ = evt.send(Event::Outline { id, token, items });
            }
            Command::Links { id, page, token } => {
                let Some(doc) = self.docs.get(&id) else {
                    let _ = evt.send(Event::UnknownDoc { id });
                    return;
                };
                let links = self.links(doc.doc, page);
                let _ = evt.send(Event::Links {
                    id,
                    page,
                    token,
                    links,
                });
            }
            Command::ExportRotated {
                id,
                rotations,
                dest,
                token,
            } => {
                let result = match self.docs.get(&id) {
                    None => Err("the document is no longer open".to_string()),
                    Some(doc) => self.export_rotated(doc.doc, &rotations, &dest),
                };
                let _ = evt.send(Event::Exported { token, result });
            }
            Command::Shutdown => {}
        }
    }

    fn open_document(
        &self,
        path: PathBuf,
        password: Option<&str>,
    ) -> Result<(FPDF_DOCUMENT, Arc<DocInfo>), String> {
        let c_path = ffi::cstring(&path.to_string_lossy());
        let mut doc = unsafe {
            (self.api.FPDF_LoadDocument)(c_path.as_ptr(), std::ptr::null())
        };
        if doc.is_null() {
            let code = unsafe { (self.api.FPDF_GetLastError)() };
            if code == ffi::FPDF_ERR_PASSWORD {
                if let Some(pw) = password {
                    let c_pw = ffi::cstring(pw);
                    doc = unsafe { (self.api.FPDF_LoadDocument)(c_path.as_ptr(), c_pw.as_ptr()) };
                    if doc.is_null() {
                        let code = unsafe { (self.api.FPDF_GetLastError)() };
                        return Err(if code == ffi::FPDF_ERR_PASSWORD {
                            ERR_PASSWORD_WRONG.to_string()
                        } else {
                            format!("This document could not be opened: {}", ffi::last_error_message(code))
                        });
                    }
                } else {
                    return Err(ERR_PASSWORD_REQUIRED.to_string());
                }
            } else {
                return Err(format!(
                    "{} ({})",
                    ffi::last_error_message(code),
                    if path.exists() {
                        "the file exists but could not be parsed"
                    } else {
                        "the file does not exist"
                    }
                ));
            }
        }

        let page_count = unsafe { (self.api.FPDF_GetPageCount)(doc) };
        if page_count <= 0 {
            unsafe { (self.api.FPDF_CloseDocument)(doc) };
            return Err("The document contains no pages.".to_string());
        }
        if page_count as usize > MAX_PAGES {
            unsafe { (self.api.FPDF_CloseDocument)(doc) };
            return Err(format!(
                "The document claims {} pages, which is more than this reader supports.",
                page_count
            ));
        }
        let mut info = DocInfo {
            page_count: page_count as u32,
            ..Default::default()
        };
        info.page_sizes.reserve(page_count as usize);
        info.page_rotations.reserve(page_count as usize);
        for i in 0..page_count {
            let mut size = ffi::FS_SIZEF::default();
            let ok = unsafe { (self.api.FPDF_GetPageSizeByIndexF)(doc, i, &mut size) };
            let (w, h) = if ok != 0 && size.width > 0.0 && size.height > 0.0 {
                (size.width, size.height)
            } else {
                (612.0, 792.0)
            };
            info.page_sizes.push(Size::new(w, h));
            // Intrinsic /Rotate is needed so exported/printed copies can bake in
            // the user's rotation without double-applying the page's own.
            let page = unsafe { (self.api.FPDF_LoadPage)(doc, i) };
            let rot = if page.is_null() {
                0
            } else {
                let r = unsafe { (self.api.FPDFPage_GetRotation)(page) };
                unsafe { (self.api.FPDF_ClosePage)(page) };
                (r as u16 % 4) * 90
            };
            info.page_rotations.push(rot);
        }
        let mut version = 0;
        unsafe { (self.api.FPDF_GetFileVersion)(doc, &mut version) };
        info.file_version = version;
        info.encrypted = unsafe { (self.api.FPDF_GetSecurityHandlerRevision)(doc) } > 0;
        info.security_revision = unsafe { (self.api.FPDF_GetSecurityHandlerRevision)(doc) };
        info.permissions = unsafe { (self.api.FPDF_GetDocPermissions)(doc) } as u32;
        info.title = self.meta(doc, "Title");
        info.author = self.meta(doc, "Author");
        info.subject = self.meta(doc, "Subject");
        info.creator = self.meta(doc, "Creator");
        // Page labels are optional; cheap to query per page.
        if page_count <= 5000 {
            for i in 0..page_count {
                let label = self.page_label(doc, i);
                info.page_labels.push(label);
            }
        }
        Ok((doc, Arc::new(info)))
    }

    fn meta(&self, doc: FPDF_DOCUMENT, tag: &str) -> Option<String> {
        let c_tag = ffi::cstring(tag);
        let s = ffi::wide_string_from_len_query(|buf, len| unsafe {
            (self.api.FPDF_GetMetaText)(doc, c_tag.as_ptr(), buf, len)
        });
        let trimmed = s.trim_matches('\0').trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    fn page_label(&self, doc: FPDF_DOCUMENT, index: i32) -> String {
        ffi::wide_string_from_len_query(|buf, len| unsafe {
            (self.api.FPDF_GetPageLabel)(doc, index, buf, len)
        })
    }

    fn render(&self, doc: FPDF_DOCUMENT, key: RenderKey) -> Result<Bitmap, String> {
        let page = unsafe { (self.api.FPDF_LoadPage)(doc, key.page as i32) };
        if page.is_null() {
            return Err("the page could not be loaded".to_string());
        }
        let result = self.render_loaded(page, key);
        unsafe { (self.api.FPDF_ClosePage)(page) };
        result
    }

    fn render_loaded(&self, page: *mut std::ffi::c_void, key: RenderKey) -> Result<Bitmap, String> {
        let scale = key.scale();
        let (pw, ph) = (
            unsafe { (self.api.FPDF_GetPageWidthF)(page) },
            unsafe { (self.api.FPDF_GetPageHeightF)(page) },
        );
        if !(pw > 0.0 && ph > 0.0) {
            return Err("the page has an invalid size".to_string());
        }
        // The page's own /Rotate is applied by PDFium when rendering, so the
        // rendered size follows the rotated dimensions.
        let display = rotated(Size::new(pw, ph), key.rotation);
        let mut width = (display.w * scale).round().max(1.0) as u32;
        let mut height = (display.h * scale).round().max(1.0) as u32;
        // Guard rails against absurd zoom levels / malformed pages.
        const MAX_DIM: u32 = 8192;
        const MAX_PIXELS: u32 = 40_000_000;
        if width > MAX_DIM || height > MAX_DIM || width.saturating_mul(height) > MAX_PIXELS {
            let factor = (MAX_DIM as f32 / width as f32).min(MAX_DIM as f32 / height as f32);
            width = ((width as f32 * factor).max(1.0)) as u32;
            height = ((height as f32 * factor).max(1.0)) as u32;
        }
        let mut bitmap = Bitmap::new(width, height);
        let stride = (width * 4) as i32;
        let fpdf_bitmap = unsafe {
            (self.api.FPDFBitmap_CreateEx)(
                width as i32,
                height as i32,
                ffi::FPDFBitmap_BGRA,
                bitmap.px.as_mut_ptr() as *mut std::ffi::c_void,
                stride,
            )
        };
        if fpdf_bitmap.is_null() {
            return Err("the renderer could not allocate a bitmap".to_string());
        }
        unsafe {
            (self.api.FPDFBitmap_FillRect)(fpdf_bitmap, 0, 0, width as i32, height as i32, 0xFFFF_FFFF);
            (self.api.FPDF_RenderPageBitmap)(
                fpdf_bitmap,
                page,
                0,
                0,
                width as i32,
                height as i32,
                (key.rotation / 90) as i32,
                ffi::FPDF_ANNOT | ffi::FPDF_LCD_TEXT,
            );
            (self.api.FPDFBitmap_Destroy)(fpdf_bitmap);
        }
        // PDFium leaves the alpha channel undefined for BGRA bitmaps.
        for px in bitmap.px.iter_mut() {
            *px |= 0xFF00_0000;
        }
        Ok(bitmap)
    }

    fn load_text(&self, doc: FPDF_DOCUMENT, page_index: u32) -> PageText {
        let page = unsafe { (self.api.FPDF_LoadPage)(doc, page_index as i32) };
        if page.is_null() {
            return PageText::default();
        }
        let text_page = unsafe { (self.api.FPDFText_LoadPage)(page) };
        if text_page.is_null() {
            unsafe { (self.api.FPDF_ClosePage)(page) };
            return PageText::default();
        }
        let count = unsafe { (self.api.FPDFText_CountChars)(text_page) };
        let mut out = PageText::default();
        if count > 0 {
            let mut buf = vec![0u16; count as usize + 1];
            let written = unsafe {
                (self.api.FPDFText_GetText)(text_page, 0, count, buf.as_mut_ptr())
            };
            // `written` counts UTF-16 code units including the trailing NUL.
            let units = if written > 0 {
                (written as usize - 1).min(count as usize)
            } else {
                0
            };
            out.text = String::from_utf16_lossy(&buf[..units]);
            out.chars.reserve(units);
            let mut i = 0i32;
            while i < count {
                let mut left = 0f64;
                let mut right = 0f64;
                let mut bottom = 0f64;
                let mut top = 0f64;
                let ok = unsafe {
                    (self.api.FPDFText_GetCharBox)(text_page, i, &mut left, &mut right, &mut bottom, &mut top)
                };
                let unit = buf.get(i as usize).copied().unwrap_or(0);
                let mut ch = char::from_u32(unit as u32).unwrap_or('\u{FFFD}');
                let mut advance = 1;
                if (0xD800..0xDC00).contains(&unit) {
                    if let Some(low) = buf.get(i as usize + 1).copied() {
                        if (0xDC00..0xE000).contains(&low) {
                            let cp = 0x10000
                                + (((unit as u32) - 0xD800) << 10)
                                + ((low as u32) - 0xDC00);
                            ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            advance = 2;
                        }
                    }
                }
                if ok != 0 {
                    out.chars.push(CharBox {
                        ch,
                        left: left as f32,
                        right: right as f32,
                        bottom: bottom as f32,
                        top: top as f32,
                    });
                }
                i += advance;
            }
        }
        unsafe { (self.api.FPDFText_ClosePage)(text_page) };
        unsafe { (self.api.FPDF_ClosePage)(page) };
        out
    }

    fn search(
        &self,
        doc: FPDF_DOCUMENT,
        page_index: u32,
        query: &str,
        start_index: i32,
        backwards: bool,
        match_case: bool,
    ) -> Vec<SearchHit> {
        if query.is_empty() {
            return Vec::new();
        }
        let page = unsafe { (self.api.FPDF_LoadPage)(doc, page_index as i32) };
        if page.is_null() {
            return Vec::new();
        }
        let text_page = unsafe { (self.api.FPDFText_LoadPage)(page) };
        if text_page.is_null() {
            unsafe { (self.api.FPDF_ClosePage)(page) };
            return Vec::new();
        }
        let mut needle: Vec<u16> = query.encode_utf16().collect();
        needle.push(0);
        let flags = if match_case { ffi::FPDF_MATCHCASE } else { 0 };
        let mut hits = Vec::new();
        let handle = unsafe {
            (self.api.FPDFText_FindStart)(text_page, needle.as_ptr(), flags, start_index.max(0))
        };
        if !handle.is_null() {
            const MAX_HITS: usize = 500;
            loop {
                let found = if backwards {
                    unsafe { (self.api.FPDFText_FindPrev)(handle) }
                } else {
                    unsafe { (self.api.FPDFText_FindNext)(handle) }
                };
                if found == 0 || hits.len() >= MAX_HITS {
                    break;
                }
                let index = unsafe { (self.api.FPDFText_GetSchResultIndex)(handle) };
                let count = unsafe { (self.api.FPDFText_GetSchCount)(handle) };
                if index < 0 || count <= 0 {
                    break;
                }
                // Union rectangle of the match.
                let rect_count = unsafe {
                    (self.api.FPDFText_CountRects)(text_page, index, count)
                };
                let mut l = f32::MAX;
                let mut b = f32::MAX;
                let mut r = f32::MIN;
                let mut t = f32::MIN;
                for k in 0..rect_count.max(0) {
                    let mut left = 0f64;
                    let mut top = 0f64;
                    let mut right = 0f64;
                    let mut bottom = 0f64;
                    let ok = unsafe {
                        (self.api.FPDFText_GetRect)(
                            text_page, k, &mut left, &mut top, &mut right, &mut bottom,
                        )
                    };
                    if ok != 0 {
                        l = l.min(left as f32);
                        r = r.max(right as f32);
                        t = t.max(top as f32);
                        b = b.min(bottom as f32);
                    }
                }
                if l != f32::MAX {
                    hits.push(SearchHit {
                        page: page_index,
                        char_index: index as u32,
                        char_count: count as u32,
                        left: l,
                        bottom: b,
                        right: r,
                        top: t,
                    });
                }
            }
            unsafe { (self.api.FPDFText_FindClose)(handle) };
        }
        unsafe { (self.api.FPDFText_ClosePage)(text_page) };
        unsafe { (self.api.FPDF_ClosePage)(page) };
        hits
    }

    fn outline(&self, doc: FPDF_DOCUMENT) -> Vec<OutlineItem> {
        fn collect(
            api: &Api,
            doc: FPDF_DOCUMENT,
            bookmark: ffi::FPDF_BOOKMARK,
            depth: u8,
            budget: &mut usize,
        ) -> Vec<OutlineItem> {
            let mut items = Vec::new();
            if depth > 8 {
                return items;
            }
            let mut current = bookmark;
            while !current.is_null() && *budget > 0 {
                *budget -= 1;
                let title = ffi::wide_string_from_len_query(|buf, len| unsafe {
                    (api.FPDFBookmark_GetTitle)(current, buf, len)
                });
                let dest = unsafe { (api.FPDFBookmark_GetDest)(doc, current) };
                let mut page = None;
                if !dest.is_null() {
                    let idx = unsafe { (api.FPDFDest_GetDestPageIndex)(doc, dest) };
                    if idx >= 0 {
                        page = Some(idx as u32);
                    }
                }
                let mut uri = None;
                let action = unsafe { (api.FPDFBookmark_GetAction)(current) };
                if !action.is_null() {
                    // ACTION_URI == 3
                    if unsafe { (api.FPDFAction_GetType)(action) } == 3 {
                        let s = ffi::byte_string_from_len_query(|buf, len| unsafe {
                            (api.FPDFAction_GetURIPath)(doc, action, buf, len)
                        });
                        if !s.is_empty() {
                            uri = Some(s);
                        }
                    }
                }
                let child = unsafe { (api.FPDFBookmark_GetFirstChild)(doc, current) };
                let children = if child.is_null() {
                    Vec::new()
                } else {
                    collect(api, doc, child, depth + 1, budget)
                };
                items.push(OutlineItem {
                    title: if title.is_empty() {
                        "(untitled)".to_string()
                    } else {
                        title
                    },
                    page,
                    uri,
                    children,
                });
                current = unsafe { (api.FPDFBookmark_GetNextSibling)(doc, current) };
            }
            items
        }
        let mut budget = 5000usize;
        let first = unsafe { (self.api.FPDFBookmark_GetFirstChild)(doc, std::ptr::null_mut()) };
        if first.is_null() {
            return Vec::new();
        }
        collect(&self.api, doc, first, 0, &mut budget)
    }

    fn links(&self, doc: FPDF_DOCUMENT, page_index: u32) -> Vec<LinkRect> {
        let page = unsafe { (self.api.FPDF_LoadPage)(doc, page_index as i32) };
        if page.is_null() {
            return Vec::new();
        }
        let mut out = Vec::new();
        // 1. Link annotations (/Annots with subtype /Link).
        let mut start_pos: i32 = 0;
        loop {
            let mut annot: ffi::FPDF_LINK = std::ptr::null_mut();
            let ok = unsafe { (self.api.FPDFLink_Enumerate)(page, &mut start_pos, &mut annot) };
            if ok == 0 || annot.is_null() {
                break;
            }
            let mut rect = ffi::FS_RECTF::default();
            if unsafe { (self.api.FPDFLink_GetAnnotRect)(annot, &mut rect) } == 0 {
                continue;
            }
            let mut target = None;
            let dest = unsafe { (self.api.FPDFLink_GetDest)(doc, annot) };
            if !dest.is_null() {
                let idx = unsafe { (self.api.FPDFDest_GetDestPageIndex)(doc, dest) };
                if idx >= 0 {
                    target = Some(LinkTarget::Page(idx as u32));
                }
            }
            if target.is_none() {
                let action = unsafe { (self.api.FPDFLink_GetAction)(annot) };
                if !action.is_null()
                    && unsafe { (self.api.FPDFAction_GetType)(action) } == 3
                {
                    let uri = ffi::byte_string_from_len_query(|buf, len| unsafe {
                        (self.api.FPDFAction_GetURIPath)(doc, action, buf, len)
                    });
                    if !uri.is_empty() {
                        target = Some(LinkTarget::Uri(uri));
                    }
                }
            }
            if let Some(target) = target {
                out.push(LinkRect {
                    left: rect.left,
                    bottom: rect.bottom,
                    right: rect.right,
                    top: rect.top,
                    target,
                });
            }
        }
        // 2. Auto-detected web links inside the text.
        let text_page = unsafe { (self.api.FPDFText_LoadPage)(page) };
        if !text_page.is_null() {
            let link_page = unsafe { (self.api.FPDFLink_LoadWebLinks)(text_page) };
            if !link_page.is_null() {
                let count = unsafe { (self.api.FPDFLink_CountWebLinks)(link_page) };
                for i in 0..count.max(0) {
                    let len = unsafe { (self.api.FPDFLink_GetURL)(link_page, i, std::ptr::null_mut(), 0) };
                    if len <= 0 {
                        continue;
                    }
                    let mut buf = vec![0u16; len as usize];
                    let written =
                        unsafe { (self.api.FPDFLink_GetURL)(link_page, i, buf.as_mut_ptr(), len) };
                    if written <= 0 {
                        continue;
                    }
                    let url = ffi::utf16_to_string(&buf);
                    let rect_count = unsafe { (self.api.FPDFLink_CountRects)(link_page, i) };
                    for r in 0..rect_count.max(0) {
                        let mut left = 0f64;
                        let mut top = 0f64;
                        let mut right = 0f64;
                        let mut bottom = 0f64;
                        let ok = unsafe {
                            (self.api.FPDFLink_GetRect)(
                                link_page, i, r, &mut left, &mut top, &mut right, &mut bottom,
                            )
                        };
                        if ok != 0 {
                            out.push(LinkRect {
                                left: left as f32,
                                bottom: bottom as f32,
                                right: right as f32,
                                top: top as f32,
                                target: LinkTarget::Uri(url.clone()),
                            });
                        }
                    }
                }
                unsafe { (self.api.FPDFLink_CloseWebLinks)(link_page) };
            }
            unsafe { (self.api.FPDFText_ClosePage)(text_page) };
        }
        unsafe { (self.api.FPDF_ClosePage)(page) };
        out
    }

    fn export_rotated(
        &self,
        doc: FPDF_DOCUMENT,
        rotations: &[(u32, u16)],
        dest: &std::path::Path,
    ) -> Result<String, String> {
        let info_page_count = unsafe { (self.api.FPDF_GetPageCount)(doc) };
        let mut original = Vec::new();
        // Apply the requested rotation on top of each page's own /Rotate.
        for (page_index, rotation) in rotations {
            if (*page_index as i32) >= info_page_count {
                continue;
            }
            let page = unsafe { (self.api.FPDF_LoadPage)(doc, *page_index as i32) };
            if page.is_null() {
                continue;
            }
            let current = unsafe { (self.api.FPDFPage_GetRotation)(page) };
            original.push((*page_index as i32, current));
            let combined = ((*rotation / 90) as i32 + current).rem_euclid(4);
            unsafe { (self.api.FPDFPage_SetRotation)(page, combined) };
            unsafe { (self.api.FPDF_ClosePage)(page) };
        }
        let result = self.save_copy(doc, dest);
        // Restore the in-memory document so later renders are unaffected.
        for (page_index, rotation) in original {
            let page = unsafe { (self.api.FPDF_LoadPage)(doc, page_index) };
            if !page.is_null() {
                unsafe { (self.api.FPDFPage_SetRotation)(page, rotation) };
                unsafe { (self.api.FPDF_ClosePage)(page) };
            }
        }
        result
    }

    fn save_copy(&self, doc: FPDF_DOCUMENT, dest: &std::path::Path) -> Result<String, String> {
        #[repr(C)]
        struct Writer {
            base: FPDF_FILEWRITE,
            data: Vec<u8>,
        }
        unsafe extern "C" fn write_block(
            this: *mut FPDF_FILEWRITE,
            buffer: *const std::ffi::c_void,
            size: std::os::raw::c_ulong,
        ) -> ffi::Bool {
            // SAFETY: PDFium passes back the pointer we gave it, which is the
            // first field of `Writer`, so the cast is valid.
            let writer = &mut *(this as *mut Writer);
            if buffer.is_null() {
                return 0;
            }
            let slice = std::slice::from_raw_parts(buffer as *const u8, size as usize);
            writer.data.extend_from_slice(slice);
            1
        }
        let mut writer = Box::new(Writer {
            base: FPDF_FILEWRITE {
                version: 1,
                write_block: Some(write_block),
            },
            data: Vec::new(),
        });
        let ok = unsafe {
            (self.api.FPDF_SaveAsCopy)(
                doc,
                &mut writer.base as *mut FPDF_FILEWRITE,
                ffi::FPDF_NO_INCREMENTAL as u32,
            )
        };
        if ok == 0 {
            return Err("PDFium could not write the document".to_string());
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, &writer.data).map_err(|e| format!("could not write {}: {}", dest.display(), e))?;
        Ok(dest.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_keys_normalise() {
        let k = RenderKey::new(3, 1.5, 450);
        assert_eq!(k.rotation, 90);
        assert!((k.scale() - 1.5).abs() < 0.001);
    }

    #[test]
    fn rotation_swaps_dimensions() {
        let s = Size::new(200.0, 100.0);
        assert_eq!(rotated(s, 0), s);
        assert_eq!(rotated(s, 90), Size::new(100.0, 200.0));
        assert_eq!(rotated(s, 270), Size::new(100.0, 200.0));
        assert_eq!(rotated(s, 180), s);
    }

    #[test]
    fn display_size_uses_intrinsic_rotation() {
        let info = DocInfo {
            page_count: 1,
            page_sizes: vec![Size::new(300.0, 400.0)],
            page_rotations: vec![90],
            ..Default::default()
        };
        assert_eq!(info.display_size(0, 0), Size::new(400.0, 300.0));
        assert_eq!(info.display_size(0, 90), Size::new(300.0, 400.0));
        assert_eq!(info.display_size(0, 180), Size::new(400.0, 300.0));
    }

    #[test]
    fn text_in_rect_extracts_lines() {
        let text = PageText {
            text: "ab\ncd".into(),
            chars: vec![
                CharBox { ch: 'a', left: 0.0, right: 10.0, bottom: 20.0, top: 30.0 },
                CharBox { ch: 'b', left: 10.0, right: 20.0, bottom: 20.0, top: 30.0 },
                CharBox { ch: 'c', left: 0.0, right: 10.0, bottom: 40.0, top: 50.0 },
                CharBox { ch: 'd', left: 10.0, right: 20.0, bottom: 40.0, top: 50.0 },
            ],
        };
        assert_eq!(text.text_in_rect(-1.0, 15.0, 25.0, 35.0), "ab");
        assert_eq!(text.text_in_rect(-1.0, 15.0, 25.0, 55.0), "ab\ncd");
        assert_eq!(text.text_in_rect(100.0, 15.0, 200.0, 55.0), "");
        assert_eq!(text.char_at(11.0, 25.0, 2.0), Some(1));
        assert_eq!(text.nearest_char(0.0, 30.0), Some(0));
    }

    #[test]
    fn engine_reports_missing_pdfium_gracefully() {
        // In a sandbox without PDFium this must not panic; it must produce an
        // EngineError event (or a ready engine when the library is present).
        let engine = Engine::start();
        let ready = engine.is_ready();
        assert!(ready || engine.init_error.is_some());
    }
}
