//! Application state and behaviour: open documents, reading sessions, the PDF
//! worker protocol, persistence and command dispatch.
//!
//! Nothing in this module talks to a window system: the platform shells feed
//! [`App::input`] into [`App::frame`] and blit [`App::frame`]'s result. That is
//! what makes the whole application testable headlessly.

pub mod cache;
pub mod commands;
pub mod dialogs;
pub mod docview;
pub mod layout;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::db::{Database, DocumentRow, ReadingMode, SessionRow, ZoomMode};
use crate::doc::identity;
use crate::geom::{Rect, Size};
use crate::pdf::{Command as PdfCommand, DocId, Engine, Event as PdfEvent, RenderKey};
use crate::ui::theme::Metrics;
use crate::ui::{Bitmap, Font, Input, Key, Theme, Ui, UiState};

use commands::Command;
use dialogs::{Dialogs, FileDialog, GotoDialog, PasswordDialog, ResumeDialog, ResumePolicy};
use docview::{DocState, DocView, RotationScope, Selection};

/// Bitmap cache budget per document (bytes). Total memory is bounded by the
/// number of open documents times this value.
pub const CACHE_LIMIT_BYTES: usize = 192 * 1024 * 1024;
/// Thumbnails are rendered at this scale (milli-pixels per PDF point).
pub const THUMB_SCALE_MILLI: u32 = 260;
/// Maximum number of render requests in flight.
const MAX_INFLIGHT_RENDERS: usize = 6;
/// How often dirty sessions are written (seconds).
const AUTOSAVE_INTERVAL: f64 = 2.0;
/// How long a toast stays on screen (seconds).
const TOAST_LIFETIME: f64 = 4.5;

/// User preferences, persisted in the `settings` table.
#[derive(Debug, Clone)]
pub struct Settings {
    pub dark: bool,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub resume_policy: ResumePolicy,
    pub window: Rect,
    pub window_maximized: bool,
    pub smooth_scroll: bool,
    pub page_fit_mode: ZoomMode,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: false,
            sidebar_visible: true,
            sidebar_width: Metrics::SIDEBAR_DEFAULT_W,
            resume_policy: ResumePolicy::Ask,
            window: Rect::new(80.0, 60.0, 1180.0, 860.0),
            window_maximized: false,
            smooth_scroll: true,
            page_fit_mode: ZoomMode::FitPage,
        }
    }
}

impl Settings {
    pub fn load(db: &Database) -> Settings {
        let mut s = Settings::default();
        s.dark = db.get_setting("theme").ok().flatten().as_deref() == Some("dark");
        s.sidebar_visible = db.setting_bool("sidebar.visible", s.sidebar_visible);
        s.sidebar_width = db
            .setting_f64("sidebar.width", s.sidebar_width as f64)
            .clamp(Metrics::SIDEBAR_MIN_W as f64, Metrics::SIDEBAR_MAX_W as f64)
            as f32;
        s.resume_policy = ResumePolicy::parse(
            &db.get_setting("resume.policy").ok().flatten().unwrap_or_default(),
        );
        let w = db.setting_f64("window.w", s.window.w as f64) as f32;
        let h = db.setting_f64("window.h", s.window.h as f64) as f32;
        s.window = Rect::new(
            db.setting_f64("window.x", s.window.x as f64) as f32,
            db.setting_f64("window.y", s.window.y as f64) as f32,
            w.max(480.0),
            h.max(360.0),
        );
        s.window_maximized = db.setting_bool("window.maximized", false);
        s.smooth_scroll = db.setting_bool("scroll.smooth", true);
        s.page_fit_mode = ZoomMode::parse(&db.get_setting("fit.mode").ok().flatten().unwrap_or_default());
        s
    }

    pub fn save(&self, db: &Database) {
        let _ = db.set_setting("theme", if self.dark { "dark" } else { "light" });
        let _ = db.set_setting("sidebar.visible", if self.sidebar_visible { "1" } else { "0" });
        let _ = db.set_setting("sidebar.width", &format!("{:.1}", self.sidebar_width));
        let _ = db.set_setting("resume.policy", self.resume_policy.as_str());
        let _ = db.set_setting("window.x", &format!("{:.0}", self.window.x));
        let _ = db.set_setting("window.y", &format!("{:.0}", self.window.y));
        let _ = db.set_setting("window.w", &format!("{:.0}", self.window.w));
        let _ = db.set_setting("window.h", &format!("{:.0}", self.window.h));
        let _ = db.set_setting("window.maximized", if self.window_maximized { "1" } else { "0" });
        let _ = db.set_setting("scroll.smooth", if self.smooth_scroll { "1" } else { "0" });
        let _ = db.set_setting("fit.mode", self.page_fit_mode.as_str());
    }
}

/// Which sidebar panel is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Thumbnails,
    Outline,
    Search,
    Recent,
}

impl Default for SidebarTab {
    fn default() -> Self {
        SidebarTab::Thumbnails
    }
}

impl SidebarTab {
    pub fn index(self) -> usize {
        match self {
            SidebarTab::Thumbnails => 0,
            SidebarTab::Outline => 1,
            SidebarTab::Search => 2,
            SidebarTab::Recent => 3,
        }
    }

    pub fn from_index(i: usize) -> SidebarTab {
        match i {
            1 => SidebarTab::Outline,
            2 => SidebarTab::Search,
            3 => SidebarTab::Recent,
            _ => SidebarTab::Thumbnails,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SidebarTab::Thumbnails => "Thumbnails",
            SidebarTab::Outline => "Contents",
            SidebarTab::Search => "Search",
            SidebarTab::Recent => "Recent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: ToastLevel,
    pub created: f64,
}

/// Sidebar interaction state that must survive frames.
#[derive(Debug, Default)]
pub struct SidebarState {
    pub tab: SidebarTab,
    pub scroll: f32,
    pub thumb_scroll: f32,
    pub outline_scroll: f32,
    pub recent_scroll: f32,
    pub search_field: crate::ui::widgets::TextField,
    pub search_scroll: f32,
    pub drag_width: bool,
    /// Page whose thumbnail should be scrolled into view.
    pub thumb_focus: u32,
}

/// Deferred actions produced by the UI layer (widgets never mutate app state
/// directly, which keeps borrow checking trivial and makes the UI testable).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Command(Command),
    OpenPath(PathBuf),
    CloseDoc(usize),
    ActivateDoc(usize),
    SetSidebarTab(SidebarTab),
    ToggleSidebar,
    ToggleTheme,
    GoToPage(u32),
    ZoomTo(f32),
    SetFit(ZoomMode),
    SetRotation(i32),
    SetRotationScope(RotationScope),
    SetReadingMode(ReadingMode),
    SetSidebarWidth(f32),
    SetFullscreen(bool),
    SetPresentation(bool),
    SearchQuery(String),
    SearchNext,
    SearchPrev,
    ToggleMatchCase,
    CopySelection,
    CopyText(String),
    AddHighlight,
    DeleteHighlight(i64),
    OpenUrl(String),
    ResumeAnswer { resume: bool, remember: bool },
    PasswordSubmit(String),
    DismissError,
    CloseDialog,
    ShowShortcuts(bool),
    ForgetDocument(i64),
    ShowDialog(DialogKind),
    SetResumePolicy(ResumePolicy),
    SetSmoothScroll(bool),
    SetHighlightColor(u32),
    GotoSearchHit(usize),
    ToggleOutline(String),
    PrintDocument,
    ExportRotated,
    SaveSettings,
    ScrollBy(f32),
    SetPageScroll { page: u32, frac: f32 },
    Quit,
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Open,
    Goto,
    Settings,
    Info,
}

/// A print job in progress: pages are rendered by the worker and handed to the
/// platform print job one by one.
pub struct PrintState {
    pub job: crate::platform::print::PrintJob,
    pub doc: DocId,
    pub pages: Vec<u32>,
    pub next: usize,
    pub awaiting: Option<u32>,
    pub scale_milli: u32,
    pub caption: String,
}

/// Window regions computed from the current window size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Regions {
    pub window: Rect,
    pub toolbar: Rect,
    pub tabbar: Rect,
    pub sidebar: Rect,
    pub content: Rect,
    pub status: Rect,
}

/// Application-wide performance counters (shown in the status bar).
#[derive(Debug, Clone, Copy, Default)]
pub struct PerfStats {
    pub startup_ms: f32,
    pub last_frame_ms: f32,
    pub last_render_ms: f32,
    pub cache_bytes: usize,
    pub pages_cached: usize,
}

pub struct App {
    pub engine: Engine,
    pub db: Option<Database>,
    pub db_error: Option<String>,
    pub settings: Settings,
    pub docs: Vec<DocView>,
    pub active: usize,
    pub ui: UiState,
    pub sidebar: SidebarState,
    pub dialogs: Dialogs,
    pub input: Input,
    pub window: Size,
    pub frame_count: u64,
    pub quit: bool,
    pub clipboard_out: Option<String>,
    pub toasts: Vec<Toast>,
    pub note: Option<String>,
    pub next_doc_id: DocId,
    pub next_token: u64,
    pub fullscreen: bool,
    pub presentation: bool,
    pub show_sidebar: bool,
    pub recent: Vec<DocumentRow>,
    pub perf: PerfStats,
    pub started: Instant,
    pub last_time: f64,
    pub redraw: bool,
    /// Highlight colour picked in the settings dialog.
    pub highlight_color: u32,
    /// True when the engine failed to load, so the UI can explain why.
    pub engine_error: Option<String>,
    /// Page number field in the toolbar (kept between frames).
    pub page_field: crate::ui::widgets::TextField,
    /// `--page N` from the command line, applied when the first document opens.
    pub pending_initial_page: Option<u32>,
    /// Set while a print/export job is running.
    pub busy: Option<String>,
    /// Active print job (Windows renders pages; Linux prints the file).
    pub print: Option<PrintState>,
    /// Rotated copy waiting to be printed once the export finishes.
    pub pending_print: Option<PathBuf>,
    pub mouse_in_content: bool,
}

impl App {
    pub fn new(engine: Engine, db: Option<Database>, db_error: Option<String>) -> App {
        let started = Instant::now();
        let settings = db
            .as_ref()
            .map(Settings::load)
            .unwrap_or_default();
        let engine_error = engine.init_error.clone();
        let recent = db
            .as_ref()
            .and_then(|db| db.recent(50).ok())
            .unwrap_or_default();
        let show_sidebar = settings.sidebar_visible;
        let window = Size::new(settings.window.w, settings.window.h);
        App {
            engine,
            db,
            db_error,
            settings,
            docs: Vec::new(),
            active: 0,
            ui: UiState::default(),
            sidebar: SidebarState::default(),
            dialogs: Dialogs::default(),
            input: Input::default(),
            window,
            frame_count: 0,
            quit: false,
            clipboard_out: None,
            toasts: Vec::new(),
            note: None,
            next_doc_id: 1,
            next_token: 1,
            fullscreen: false,
            presentation: false,
            show_sidebar,
            recent,
            perf: PerfStats::default(),
            started,
            last_time: 0.0,
            redraw: true,
            highlight_color: 0xFFFFE066,
            engine_error,
            page_field: Default::default(),
            pending_initial_page: None,
            busy: None,
            print: None,
            pending_print: None,
            mouse_in_content: false,
        }
    }

    // ---------------------------------------------------------------- documents

    pub fn active_doc(&self) -> Option<&DocView> {
        self.docs.get(self.active)
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut DocView> {
        let index = self.active;
        self.docs.get_mut(index)
    }

    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn unix_now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Opens a document by path. Failures are reported as toasts/dialogs.
    pub fn open_path(&mut self, path: PathBuf, password: Option<String>) {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map(|c| c.join(&path))
                .unwrap_or(path)
        };
        // Already open? Just switch to it.
        if let Some(index) = self.docs.iter().position(|d| d.identity.path == path) {
            self.active = index;
            self.redraw = true;
            return;
        }
        let identity = match identity::identify(&path) {
            Ok(id) => id,
            Err(e) => {
                self.error_dialog("Cannot open document", e);
                return;
            }
        };
        if let Some(index) = self
            .docs
            .iter()
            .position(|d| d.identity.fingerprint == identity.fingerprint)
        {
            self.active = index;
            self.redraw = true;
            return;
        }
        let now_unix = Self::unix_now();
        let now = self.now();
        // Look up saved progress by fingerprint so moved/renamed files resume.
        let row = self
            .db
            .as_ref()
            .and_then(|db| db.find_by_fingerprint(&identity.fingerprint).ok().flatten());
        let session = row.as_ref().map(|r| r.session).unwrap_or(SessionRow {
            zoom_mode: self.settings.page_fit_mode,
            ..Default::default()
        });
        let row_id = row.as_ref().map(|r| r.id);
        if let Some(row) = &row {
            if row.path != identity.path_string() {
                if let Some(db) = &self.db {
                    let _ = db.upsert_document(
                        &identity.fingerprint,
                        &identity.path_string(),
                        None,
                        0,
                        now_unix,
                    );
                }
            }
        }
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        let mut view = DocView::new(id, identity.clone(), row_id, session, CACHE_LIMIT_BYTES, now);
        if let Some(row) = &row {
            view.highlights = self
                .db
                .as_ref()
                .and_then(|db| db.highlights(row.id).ok())
                .unwrap_or_default();
        }
        self.engine.send(PdfCommand::Open {
            id,
            path: identity.path.clone(),
            password,
        });
        self.docs.push(view);
        self.active = self.docs.len() - 1;
        self.redraw = true;
    }

    pub fn close_doc(&mut self, index: usize) {
        if index >= self.docs.len() {
            return;
        }
        self.save_doc_session(index);
        let doc = self.docs.remove(index);
        self.engine.send(PdfCommand::Close { id: doc.id });
        if self.active >= self.docs.len() && !self.docs.is_empty() {
            self.active = self.docs.len() - 1;
        }
        self.redraw = true;
    }

    fn save_doc_session(&mut self, index: usize) {
        let now_mono = self.now();
        let (Some(db), Some(doc)) = (self.db.as_ref(), self.docs.get_mut(index)) else {
            return;
        };
        let Some(row_id) = doc.row_id else {
            return;
        };
        let mut session = doc.session;
        session.last_page = doc.layout.current_page() + 1;
        session.scroll_frac = doc.layout.scroll_fraction(doc.layout.current_page());
        session.rotation = doc.session.rotation;
        session.clamp_values();
        doc.session = session;
        let now = Self::unix_now();
        if let Err(e) = db.save_session(row_id, &session, now) {
            self.note = Some(format!("Could not save reading position: {}", e));
        }
        doc.dirty = false;
        doc.last_saved = now_mono;
        self.refresh_recent();
    }

    /// Writes every dirty session (called periodically, on close and on quit).
    pub fn save_all(&mut self) {
        for i in 0..self.docs.len() {
            if self.docs[i].dirty || self.docs[i].row_id.is_some() {
                self.save_doc_session(i);
            }
        }
        if let Some(db) = &self.db {
            self.settings.save(db);
        }
    }

    fn refresh_recent(&mut self) {
        if let Some(db) = &self.db {
            if let Ok(recent) = db.recent(50) {
                self.recent = recent;
            }
        }
    }

    // ------------------------------------------------------------------ engine

    /// Handles events produced by the PDF worker.
    pub fn pump_engine(&mut self) {
        for event in self.engine.drain(64) {
            match event {
                PdfEvent::Opened { id, result, path } => self.on_opened(id, result, path),
                PdfEvent::Rendered {
                    id,
                    key,
                    bitmap,
                    millis,
                } => {
                    if self.print_consumed(id, &key, &bitmap) {
                        // Page went to the printer; keep the cache warm too.
                    }
                    if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                        doc.pending_renders.remove(&key);
                        doc.last_render_ms = millis;
                        self.perf.last_render_ms = millis;
                        if key.rotation == 0 && key.scale_milli == doc.thumb_scale_milli {
                            doc.thumbs.insert(key.page, Some(bitmap));
                            doc.pending_thumbs.remove(&key.page);
                        } else {
                            doc.cache.insert(key, bitmap);
                        }
                    }
                    self.redraw = true;
                }
                PdfEvent::Text {
                    id, page, text, ..
                } => {
                    if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                        doc.pending_text.remove(&page);
                        doc.texts.insert(page, text);
                    }
                    self.redraw = true;
                }
                PdfEvent::Searched {
                    id,
                    page,
                    hits,
                    token,
                } => {
                    let viewport = self.content_rect().size();
                    let index = self.docs.iter().position(|d| d.id == id);
                    if let Some(index) = index {
                        if token != self.docs[index].search.token {
                            continue;
                        }
                        let found = self.docs[index].apply_search_hits(page, hits);
                        if found {
                            let doc = &mut self.docs[index];
                            doc.search.active = true;
                            doc.search.current = 0;
                            if let Some(hit) = doc.search.hits.first().copied() {
                                let count = doc.search.hits.len();
                                Self::jump_to_hit(doc, hit, viewport);
                                self.set_note(format!("{} match(es) so far", count));
                            }
                        } else {
                            // Keep scanning the following pages.
                            self.search_continue(index, page, 1);
                        }
                    }
                    self.redraw = true;
                }
                PdfEvent::Outline { id, items, .. } => {
                    if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                        doc.outline = items;
                        doc.outline_requested = false;
                    }
                    self.redraw = true;
                }
                PdfEvent::Links {
                    id, page, links, ..
                } => {
                    if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                        doc.pending_links.remove(&page);
                        doc.links.insert(page, links);
                    }
                }
                PdfEvent::Exported { result, .. } => {
                    self.busy = None;
                    match result {
                        Ok(path) => {
                            let printing = self
                                .pending_print
                                .as_ref()
                                .map(|p| p.to_string_lossy() == path)
                                .unwrap_or(false);
                            if printing {
                                let title = self
                                    .active_doc()
                                    .map(|d| d.title.clone())
                                    .unwrap_or_else(|| "document".to_string());
                                self.start_print(std::path::Path::new(&path), &title);
                            } else {
                                self.toast(ToastLevel::Info, format!("Saved a copy to {}", path));
                            }
                        }
                        Err(e) => {
                            self.pending_print = None;
                            self.toast(ToastLevel::Error, format!("Export failed: {}", e));
                        }
                    }
                    self.redraw = true;
                }
                PdfEvent::UnknownDoc { .. } => {}
                PdfEvent::EngineError { message } => {
                    self.engine_error = Some(message.clone());
                    self.toast(ToastLevel::Error, message);
                }
            }
        }
    }

    fn on_opened(
        &mut self,
        id: DocId,
        result: Result<Arc<crate::pdf::DocInfo>, String>,
        path: PathBuf,
    ) {
        let viewport = self.content_rect().size();
        let Some(index) = self.docs.iter().position(|d| d.id == id) else {
            return;
        };
        match result {
            Ok(info) => {
                let now_unix = Self::unix_now();
                let (fingerprint, path_string, title) = {
                    let doc = &self.docs[index];
                    (
                        doc.identity.fingerprint.clone(),
                        doc.identity.path_string(),
                        info.title.clone(),
                    )
                };
                if let Some(db) = &self.db {
                    match db.upsert_document(
                        &fingerprint,
                        &path_string,
                        title.as_deref(),
                        info.page_count,
                        now_unix,
                    ) {
                        Ok(row_id) => {
                            self.docs[index].row_id = Some(row_id);
                            let _ = db.touch(row_id, now_unix);
                            self.docs[index].highlights =
                                db.highlights(row_id).unwrap_or_default();
                        }
                        Err(e) => {
                            self.note = Some(format!("Could not record the document: {}", e));
                        }
                    }
                }
                let doc = &mut self.docs[index];
                doc.set_info(info.clone(), viewport);
                doc.state = DocState::Ready(info.clone());
                // Offer to resume when there is meaningful saved progress.
                let policy = self.settings.resume_policy;
                let saved_page = doc.session.last_page;
                if policy == ResumePolicy::Ask && saved_page > 1 {
                    self.dialogs.resume = Some(ResumeDialog {
                        doc_index: index,
                        page: saved_page,
                        total: info.page_count,
                        remember: false,
                    });
                } else if policy == ResumePolicy::Never {
                    doc.session.last_page = 1;
                    doc.session.scroll_frac = 0.0;
                    doc.goto_page(0, viewport);
                    doc.dirty = true;
                }
                if let Some(page) = self.pending_initial_page.take() {
                    let viewport = self.content_rect().size();
                    if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                        doc.goto_page(page.saturating_sub(1), viewport);
                        doc.dirty = true;
                    }
                }
                // Kick off the metadata requests for the first visible pages.
                self.engine.send(PdfCommand::Outline { id, token: 1 });
                self.docs[index].outline_requested = true;
                self.request_visible_text();
                self.refresh_recent();
            }
            Err(message) => {
                let needs_password = message.to_lowercase().contains("password");
                let doc = &mut self.docs[index];
                doc.error = Some(message.clone());
                if needs_password {
                    doc.state = DocState::NeedsPassword;
                    self.dialogs.password = Some(PasswordDialog {
                        doc_index: index,
                        document: doc.identity.display_name.clone(),
                        field: Default::default(),
                        error: None,
                    });
                } else {
                    doc.state = DocState::Failed(message.clone());
                    self.error_dialog("Cannot open document", message);
                }
            }
        }
        let _ = path;
        self.redraw = true;
    }

    /// Sends render requests for every page that should be on screen.
    pub fn ensure_renders(&mut self) {
        let viewport = self.content_rect().size();
        let mut requests: Vec<(DocId, RenderKey)> = Vec::new();
        for doc in self.docs.iter_mut() {
            if !doc.is_ready() {
                continue;
            }
            for key in doc.wanted_keys() {
                if doc.cache.contains(&key) || doc.pending_renders.contains(&key) {
                    continue;
                }
                if doc.pending_renders.len() >= MAX_INFLIGHT_RENDERS {
                    break;
                }
                doc.pending_renders.insert(key);
                requests.push((doc.id, key));
            }
        }
        for (id, key) in requests {
            self.engine.send(PdfCommand::Render { id, key });
        }
        let _ = viewport;
    }

    /// Requests thumbnails for the pages around the current one (sidebar open).
    ///
    /// Thumbnails are rendered at the width they are displayed, so drawing them
    /// costs a row copy instead of a resample on every frame.
    pub fn ensure_thumbnails(&mut self) {
        if !self.show_sidebar || self.sidebar.tab != SidebarTab::Thumbnails {
            return;
        }
        let regions = self.regions();
        let available = (regions.sidebar.w - 24.0 - Metrics::SCROLLBAR_W).max(64.0);
        let mut requests = Vec::new();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        if !doc.is_ready() {
            return;
        }
        let Some(info) = doc.info().cloned() else {
            return;
        };
        let current = doc.current_page();
        let total = doc.page_count();
        // Match the thumbnail width to the sidebar cell width.
        let reference_w = info
            .page_sizes
            .get(current as usize)
            .map(|s| crate::pdf::rotated(*s, doc.effective_rotation(current)).w)
            .unwrap_or(595.0)
            .max(1.0);
        let wanted_milli = ((available / reference_w) * 1000.0).round().clamp(60.0, 3000.0) as u32;
        if wanted_milli.abs_diff(doc.thumb_scale_milli) > doc.thumb_scale_milli / 20 {
            doc.thumb_scale_milli = wanted_milli;
            doc.thumbs.clear();
            doc.pending_thumbs.clear();
        }
        let scale = doc.thumb_scale_milli as f32 / 1000.0;
        let from = current.saturating_sub(6);
        let to = (current + 12).min(total);
        for page in from..to {
            if doc.thumbs.contains_key(&page) || doc.pending_thumbs.contains(&page) {
                continue;
            }
            doc.pending_thumbs.insert(page);
            requests.push((doc.id, RenderKey::new(page, scale, 0)));
            if requests.len() >= 4 {
                break;
            }
        }
        for (id, key) in requests {
            self.engine.send(PdfCommand::Render { id, key });
        }
    }

    pub fn request_text(&mut self, page: u32) {
        let token = self.take_token();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        if doc.pending_text.contains(&page) || doc.texts.contains_key(&page) {
            return;
        }
        doc.pending_text.insert(page);
        let id = doc.id;
        self.engine.send(PdfCommand::PageText { id, page, token });
    }

    /// Monotonic request id used to discard stale worker replies.
    fn take_token(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        token
    }

    fn request_visible_text(&mut self) {
        let pages: Vec<u32> = self
            .active_doc()
            .map(|d| d.layout.visible_pages())
            .unwrap_or_default();
        for page in pages {
            self.request_text(page);
        }
    }

    pub fn request_links(&mut self, page: u32) {
        let token = self.take_token();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        if doc.pending_links.contains(&page) || doc.links.contains_key(&page) {
            return;
        }
        doc.pending_links.insert(page);
        let id = doc.id;
        self.engine.send(PdfCommand::Links { id, page, token });
    }

    // ------------------------------------------------------------------- search

    pub fn search_start(&mut self, query: String) {
        let token = self.take_token();
        let Some(doc) = self.active_doc_mut() else { return };
        doc.search = docview::SearchState {
            query: query.clone(),
            token,
            active: true,
            ..Default::default()
        };
        if query.trim().is_empty() || !doc.is_ready() {
            return;
        }
        let page = doc.current_page();
        doc.search.scanning_page = Some(page);
        let id = doc.id;
        let match_case = doc.search.match_case;
        self.engine.send(PdfCommand::Search {
            id,
            page,
            query,
            token,
            start_index: -1,
            backwards: false,
            match_case,
        });
    }

    /// Requests the next page to scan for the active search.
    fn search_continue(&mut self, index: usize, from_page: u32, dir: i32) {
        let Some(doc) = self.docs.get_mut(index) else {
            return;
        };
        if !doc.search.active {
            return;
        }
        let total = doc.page_count();
        let next = from_page as i32 + dir;
        if next < 0 || next >= total as i32 {
            doc.search.no_more_pages = true;
            doc.search.scanning_page = None;
            return;
        }
        doc.search.scanning_page = Some(next as u32);
        let token = doc.search.token;
        let query = doc.search.query.clone();
        let match_case = doc.search.match_case;
        let id = doc.id;
        self.engine.send(PdfCommand::Search {
            id,
            page: next as u32,
            query,
            token,
            start_index: -1,
            backwards: dir < 0,
            match_case,
        });
    }

    /// Moves to the next/previous match, continuing the search if needed.
    pub fn search_step(&mut self, dir: i32) {
        let viewport = self.content_rect().size();
        let index = self.active;
        let Some(doc) = self.docs.get(index) else { return };
        if !doc.search.active || doc.search.query.trim().is_empty() {
            return;
        }
        if doc.search.hits.is_empty() {
            if !doc.search.no_more_pages {
                let page = doc.search.scanning_page.unwrap_or_else(|| doc.current_page());
                self.search_continue(index, page, -1);
            } else {
                self.set_note("No matches");
            }
            return;
        }
        let count = doc.search.hits.len();
        let current = doc.search.current.min(count - 1) as i32;
        let next = (current + dir).rem_euclid(count as i32) as usize;
        let hit = doc.search.hits.get(next).copied();
        if let Some(doc) = self.docs.get_mut(index) {
            doc.search.current = next;
            if let Some(hit) = hit {
                Self::jump_to_hit(doc, hit, viewport);
            }
        }
    }

    fn jump_to_hit(doc: &mut DocView, hit: crate::pdf::SearchHit, viewport: Size) {
        doc.goto_page(hit.page, viewport);
        doc.recompute_layout(viewport);
        let Some(geom) = doc.layout.pages.iter().find(|p| p.page == hit.page) else {
            return;
        };
        let page_h = doc
            .info()
            .map(|i| i.display_size(hit.page, doc.session.rotation).h)
            .unwrap_or(geom.h / doc.layout.zoom);
        // PDF y grows upwards: convert to a top-down page coordinate.
        let top_pt = page_h - hit.top;
        let target_top = geom.y + top_pt * doc.layout.zoom;
        let desired = target_top - viewport.h * 0.35;
        doc.jump_to(desired.clamp(0.0, doc.layout.max_scroll()));
        doc.recompute_layout(viewport);
    }

    // --------------------------------------------------------------- commands

    pub fn handle_command(&mut self, command: Command) {
        let viewport = self.content_rect().size();
        match command {
            Command::OpenDialog => {
                let start = self
                    .active_doc()
                    .map(|d| d.identity.path.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_default();
                self.dialogs.open = Some(FileDialog::new(start));
            }
            Command::CloseTab => {
                if !self.docs.is_empty() {
                    self.close_doc(self.active);
                } else {
                    self.quit = true;
                }
            }
            Command::Quit => {
                self.save_all();
                self.quit = true;
            }
            Command::NextPage => {
                if let Some(doc) = self.active_doc_mut() {
                    let page = doc.current_page();
                    if page + 1 < doc.page_count() {
                        doc.goto_page(page + 1, viewport);
                    }
                }
            }
            Command::PrevPage => {
                if let Some(doc) = self.active_doc_mut() {
                    let page = doc.current_page();
                    doc.goto_page(page.saturating_sub(1), viewport);
                }
            }
            Command::FirstPage => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.goto_page(0, viewport);
                }
            }
            Command::LastPage => {
                if let Some(doc) = self.active_doc_mut() {
                    let last = doc.page_count().saturating_sub(1);
                    doc.goto_page(last, viewport);
                }
            }
            Command::NextDocument => {
                if !self.docs.is_empty() {
                    self.active = (self.active + 1) % self.docs.len();
                }
            }
            Command::PrevDocument => {
                if !self.docs.is_empty() {
                    self.active = (self.active + self.docs.len() - 1) % self.docs.len();
                }
            }
            Command::GoToPageDialog => {
                let mut dialog = GotoDialog::default();
                if let Some(doc) = self.active_doc() {
                    dialog.field =
                        crate::ui::widgets::TextField::with_text(&(doc.current_page() + 1).to_string());
                    dialog.field.select_all();
                }
                self.dialogs.goto = Some(dialog);
            }
            Command::ZoomIn => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.zoom_step(true, viewport);
                    self.settings.page_fit_mode = ZoomMode::Custom;
                }
            }
            Command::ZoomOut => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.zoom_step(false, viewport);
                    self.settings.page_fit_mode = ZoomMode::Custom;
                }
            }
            Command::FitPage => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_zoom(ZoomMode::FitPage, doc.session.zoom, viewport);
                }
            }
            Command::FitWidth => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_zoom(ZoomMode::FitWidth, doc.session.zoom, viewport);
                }
            }
            Command::ZoomActual => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_zoom(ZoomMode::Actual, 1.0, viewport);
                }
            }
            Command::RotateCw => self.rotate(90),
            Command::RotateCcw => self.rotate(-90),
            Command::Rotate180 => self.rotate(180),
            Command::ToggleRotationScope => {
                let mut note = None;
                if let Some(doc) = self.active_doc_mut() {
                    doc.rotation_scope = match doc.rotation_scope {
                        RotationScope::All => RotationScope::Page,
                        RotationScope::Page => RotationScope::All,
                    };
                    note = Some(match doc.rotation_scope {
                        RotationScope::All => "Rotation applies to all pages".to_string(),
                        RotationScope::Page => "Rotation applies to the current page".to_string(),
                    });
                }
                if let Some(note) = note {
                    self.set_note(note);
                }
            }
            Command::ToggleReadingMode => {
                if let Some(doc) = self.active_doc_mut() {
                    let next = match doc.session.reading_mode {
                        ReadingMode::Continuous => ReadingMode::Single,
                        ReadingMode::Single => ReadingMode::Continuous,
                    };
                    doc.set_reading_mode(next, viewport);
                }
            }
            Command::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                self.settings.sidebar_visible = self.show_sidebar;

            }
            Command::ShowThumbnails => self.show_sidebar_tab(SidebarTab::Thumbnails),
            Command::ShowOutline => self.show_sidebar_tab(SidebarTab::Outline),
            Command::ShowSearch => {
                self.show_sidebar_tab(SidebarTab::Search);
                self.ui.focus = Some(crate::ui::id("search-field"));
            }
            Command::ShowRecent => self.show_sidebar_tab(SidebarTab::Recent),
            Command::ToggleTheme => {
                self.settings.dark = !self.settings.dark;
                if let Some(db) = &self.db {
                    self.settings.save(db);
                }
            }
            Command::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                self.settings.window_maximized = self.fullscreen;
            }
            Command::Presentation => {
                self.presentation = !self.presentation;
                self.fullscreen = self.presentation;
                if self.presentation {
                    self.show_sidebar = false;
                }
            }
            Command::FocusSearch => {
                self.show_sidebar_tab(SidebarTab::Search);
                self.ui.focus = Some(crate::ui::id("search-field"));
            }
            Command::FindNext => self.search_step(1),
            Command::FindPrev => self.search_step(-1),
            Command::ToggleMatchCase => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.search.match_case = !doc.search.match_case;
                    let query = doc.search.query.clone();
                    if !query.is_empty() {
                        self.search_start(query);
                    }
                }
            }
            Command::Copy => self.copy_selection(),
            Command::Highlight => self.add_highlight(),
            Command::Print => self.print_document(),
            Command::ExportRotated => self.export_rotated_copy(),
            Command::OpenSettings => self.dialogs.settings = true,
            Command::DocumentInfo => self.dialogs.info = true,
        }
        self.redraw = true;
    }

    fn show_sidebar_tab(&mut self, tab: SidebarTab) {
        if self.show_sidebar && self.sidebar.tab == tab {
            self.show_sidebar = false;
        } else {
            self.show_sidebar = true;
            self.sidebar.tab = tab;
        }
        self.settings.sidebar_visible = self.show_sidebar;
    }

    pub fn rotate(&mut self, delta: i32) {
        let viewport = self.content_rect().size();
        if let Some(doc) = self.active_doc_mut() {
            doc.rotate(delta, viewport);
        }
    }

    pub fn copy_selection(&mut self) {
        if let Some(text) = self.active_doc().and_then(|d| d.selected_text()) {
            self.clipboard_out = Some(text.clone());
            self.toast(ToastLevel::Info, format!("Copied {} character(s)", text.chars().count()));
        }
    }

    pub fn add_highlight(&mut self) {
        let Some(doc) = self.active_doc() else { return };
        let Some(sel) = doc.selection else { return };
        if sel.is_empty() {
            self.toast(ToastLevel::Warn, "Select some text first");
            return;
        }
        let rects = doc.selection_rects(sel.page);
        if rects.is_empty() {
            return;
        }
        let Some(row_id) = doc.row_id else {
            self.toast(ToastLevel::Warn, "Highlights need the local database");
            return;
        };
        let Some(db) = &self.db else { return };
        let color = self.highlight_color;
        let now = Self::unix_now();
        let saved = db.add_highlight(row_id, sel.page, &rects, color, None, now);
        match saved {
            Ok(_) => {
                let all = db.highlights(row_id).unwrap_or_default();
                if let Some(doc) = self.active_doc_mut() {
                    doc.highlights = all;
                    doc.clear_selection();
                }
                self.toast(ToastLevel::Info, "Highlight added");
            }
            Err(e) => self.toast(ToastLevel::Error, format!("Could not save highlight: {}", e)),
        }
    }

    pub fn delete_highlight(&mut self, id: i64) {
        let row_id = self.active_doc().and_then(|d| d.row_id);
        let deleted = self
            .db
            .as_ref()
            .map(|db| db.delete_highlight(id).is_ok())
            .unwrap_or(false);
        if !deleted {
            return;
        }
        let all = row_id.and_then(|rid| self.db.as_ref().and_then(|db| db.highlights(rid).ok()));
        if let (Some(all), Some(doc)) = (all, self.active_doc_mut()) {
            doc.highlights = all;
        }
        self.toast(ToastLevel::Info, "Highlight removed");
    }

    /// Writes a rotated copy next to the original (never modifying it).
    pub fn export_rotated_copy(&mut self) {
        let Some(doc) = self.active_doc() else { return };
        if !doc.is_ready() {
            return;
        }
        let stem = doc
            .identity
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".into());
        let dir = doc
            .identity
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let dest = dir.join(format!("{}-rotated-{}.pdf", stem, doc.session.rotation));
        let rotations: Vec<(u32, u16)> = if doc.rotation_scope == RotationScope::All {
            (0..doc.page_count()).map(|p| (p, doc.session.rotation)).collect()
        } else {
            vec![(doc.current_page(), doc.session.rotation)]
        };
        let id = doc.id;
        let token = self.take_token();
        self.engine.send(PdfCommand::ExportRotated {
            id,
            rotations,
            dest: dest.clone(),
            token,
        });
        self.busy = Some(format!("Exporting {}", dest.display()));
        self.toast(ToastLevel::Info, "Exporting a rotated copy…");
    }

    /// Prints the document. A user rotation is applied by exporting a rotated
    /// copy first (the original file is never touched), then the copy is printed.
    pub fn print_document(&mut self) {
        let viewport = self.content_rect().size();
        let _ = viewport;
        let Some(doc) = self.active_doc() else {
            self.toast(ToastLevel::Warn, "Open a document first");
            return;
        };
        if !doc.is_ready() {
            self.toast(ToastLevel::Warn, "The document is still loading");
            return;
        }
        let rotation = doc.session.rotation;
        let all_pages = doc.rotation_scope == RotationScope::All;
        let page_count = doc.page_count();
        let current = doc.current_page();
        let title = doc.title.clone();
        let id = doc.id;
        let path = doc.identity.path.clone();
        if rotation != 0 {
            let dest = std::env::temp_dir().join(format!(
                "pdf-rust-print-{}-{}.pdf",
                std::process::id(),
                self.next_token
            ));
            let rotations: Vec<(u32, u16)> = if all_pages {
                (0..page_count).map(|p| (p, rotation)).collect()
            } else {
                vec![(current, rotation)]
            };
            let token = self.take_token();
            self.engine.send(PdfCommand::ExportRotated {
                id,
                rotations,
                dest: dest.clone(),
                token,
            });
            self.pending_print = Some(dest);
            self.busy = Some("Preparing the print copy…".to_string());
            self.toast(ToastLevel::Info, "Preparing a rotated copy for printing…");
            return;
        }
        self.start_print(&path, &title);
    }

    /// Starts a print job for `path` (already in the right orientation).
    fn start_print(&mut self, path: &std::path::Path, title: &str) {
        self.pending_print = None;
        let job = match crate::platform::print::PrintJob::start(path, title) {
            Ok(job) => job,
            Err(e) => {
                self.busy = None;
                self.toast(ToastLevel::Error, format!("Printing failed: {}", e));
                return;
            }
        };
        if !job.needs_pages() {
            match job.finish() {
                Ok(message) => self.toast(ToastLevel::Info, message),
                Err(e) => self.toast(ToastLevel::Error, format!("Printing failed: {}", e)),
            }
            self.busy = None;
            return;
        }
        let (Some(doc), scale) = (
            self.docs.iter().find(|d| d.title == title),
            job.scale().clamp(0.1, 8.0),
        ) else {
            self.busy = None;
            self.toast(ToastLevel::Error, "The document was closed while printing");
            return;
        };
        let doc_id = doc.id;
        let pages: Vec<u32> = (0..doc.page_count()).collect();
        let scale_milli = RenderKey::new(0, scale, 0).scale_milli;
        self.print = Some(PrintState {
            job,
            doc: doc_id,
            pages,
            next: 0,
            awaiting: None,
            scale_milli,
            caption: title.to_string(),
        });
        self.busy = Some(format!("Printing {}…", title));
        self.print_pump();
    }

    /// Requests the next page image for the active print job.
    fn print_pump(&mut self) {
        let Some(print) = &mut self.print else { return };
        if print.awaiting.is_some() {
            return;
        }
        let Some(page) = print.pages.get(print.next).copied() else {
            return;
        };
        let key = RenderKey {
            page,
            scale_milli: print.scale_milli,
            rotation: 0,
        };
        print.awaiting = Some(page);
        print.next += 1;
        let id = print.doc;
        self.engine.send(PdfCommand::Render { id, key });
    }

    /// Feeds a rendered page to the print job (returns true when consumed).
    fn print_consumed(&mut self, id: DocId, key: &RenderKey, bitmap: &Arc<Bitmap>) -> bool {
        let Some(print) = &mut self.print else {
            return false;
        };
        if id != print.doc || key.scale_milli != print.scale_milli || print.awaiting != Some(key.page) {
            return false;
        }
        print.awaiting = None;
        if let Err(e) = print.job.page(bitmap) {
            let print = self.print.take();
            self.busy = None;
            let caption = print.map(|p| p.caption).unwrap_or_default();
            self.toast(ToastLevel::Error, format!("Printing {} failed: {}", caption, e));
            return true;
        }
        if print.next >= print.pages.len() {
            let print = self.print.take().expect("print job present");
            match print.job.finish() {
                Ok(message) => self.toast(ToastLevel::Info, message),
                Err(e) => self.toast(ToastLevel::Error, format!("Printing failed: {}", e)),
            }
            self.busy = None;
        } else {
            self.print_pump();
        }
        true
    }

    /// Cancels an in-flight print job.
    pub fn cancel_print(&mut self) {
        if let Some(mut print) = self.print.take() {
            print.awaiting = None;
            self.toast(ToastLevel::Info, format!("Printing {} cancelled", print.caption));
            self.busy = None;
        }
    }

    // -------------------------------------------------------------------- misc

    pub fn toast(&mut self, level: ToastLevel, text: impl Into<String>) {
        let created = self.now();
        self.toasts.push(Toast {
            text: text.into(),
            level,
            created,
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        self.redraw = true;
    }

    pub fn set_note(&mut self, text: impl Into<String>) {
        self.note = Some(text.into());
    }

    pub fn error_dialog(&mut self, title: &str, message: impl Into<String>) {
        self.dialogs.error(title, message);
    }

    /// Computes the window regions for the current window size.
    pub fn regions(&self) -> Regions {
        let mut window = Rect::new(0.0, 0.0, self.window.w.max(320.0), self.window.h.max(240.0));
        let status = window.split_bottom(Metrics::STATUS_H);
        let toolbar_h = if self.presentation { 0.0 } else { Metrics::TOOLBAR_H };
        let mut toolbar = Rect::ZERO;
        if toolbar_h > 0.0 {
            toolbar = window.split_top(toolbar_h);
        }
        let mut tabbar = Rect::ZERO;
        if self.docs.len() > 1 && !self.presentation {
            tabbar = window.split_top(Metrics::TABBAR_H);
        }
        let mut sidebar = Rect::ZERO;
        if self.show_sidebar && !self.presentation {
            sidebar = window.split_left(
                self.settings
                    .sidebar_width
                    .clamp(Metrics::SIDEBAR_MIN_W, window.w * 0.6),
            );
        }
        Regions {
            window: Rect::new(0.0, 0.0, self.window.w, self.window.h),
            toolbar,
            tabbar,
            sidebar,
            content: window,
            status,
        }
    }

    pub fn content_rect(&self) -> Rect {
        self.regions().content
    }

    /// Applies a deferred UI action.
    pub fn apply_action(&mut self, action: Action) {
        let viewport = self.content_rect().size();
        match action {
            Action::Command(cmd) => self.handle_command(cmd),
            Action::OpenPath(path) => {
                self.dialogs.close_all();
                self.open_path(path, None);
            }
            Action::CloseDoc(index) => self.close_doc(index),
            Action::ActivateDoc(index) => {
                if index < self.docs.len() {
                    self.active = index;
                }
            }
            Action::SetSidebarTab(tab) => {
                self.sidebar.tab = tab;
                self.sidebar.scroll = 0.0;
                if !self.show_sidebar {
                    self.show_sidebar = true;
                    self.settings.sidebar_visible = true;
                }
            }
            Action::ToggleSidebar => self.handle_command(Command::ToggleSidebar),
            Action::ToggleTheme => self.handle_command(Command::ToggleTheme),
            Action::GoToPage(page) => {
                if let Some(doc) = self.active_doc_mut() {
                    let page = page.saturating_sub(1).min(doc.page_count().saturating_sub(1));
                    doc.goto_page(page, viewport);
                }
                self.dialogs.goto = None;
            }
            Action::ZoomTo(zoom) => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_zoom(ZoomMode::Custom, zoom, viewport);
                }
            }
            Action::SetFit(mode) => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_zoom(mode, doc.session.zoom, viewport);
                }
            }
            Action::SetRotation(delta) => self.rotate(delta),
            Action::SetRotationScope(scope) => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.rotation_scope = scope;
                }
            }
            Action::SetReadingMode(mode) => {
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_reading_mode(mode, viewport);
                }
            }
            Action::SetSidebarWidth(width) => {
                self.settings.sidebar_width =
                    width.clamp(Metrics::SIDEBAR_MIN_W, Metrics::SIDEBAR_MAX_W);
            }
            Action::SetFullscreen(value) => {
                self.fullscreen = value;
                self.settings.window_maximized = value;
            }
            Action::SetPresentation(value) => {
                self.presentation = value;
                self.fullscreen = value;
                if value {
                    self.show_sidebar = false;
                }
            }
            Action::SearchQuery(query) => {
                self.sidebar.search_field.text = query.clone();
                self.search_start(query);
            }
            Action::SearchNext => self.search_step(1),
            Action::SearchPrev => self.search_step(-1),
            Action::ToggleMatchCase => self.handle_command(Command::ToggleMatchCase),
            Action::CopySelection => self.copy_selection(),
            Action::CopyText(text) => {
                self.clipboard_out = Some(text);
                self.toast(ToastLevel::Info, "Copied to clipboard");
            }
            Action::AddHighlight => self.add_highlight(),
            Action::DeleteHighlight(id) => self.delete_highlight(id),
            Action::OpenUrl(url) => self.open_url(&url),
            Action::ResumeAnswer { resume, remember } => {
                if let Some(dialog) = self.dialogs.resume.take() {
                    if remember {
                        self.settings.resume_policy = if resume {
                            ResumePolicy::Always
                        } else {
                            ResumePolicy::Never
                        };
                        if let Some(db) = &self.db {
                            self.settings.save(db);
                        }
                    }
                    if !resume {
                        if let Some(doc) = self.docs.get_mut(dialog.doc_index) {
                            doc.session.last_page = 1;
                            doc.session.scroll_frac = 0.0;
                            doc.goto_page(0, viewport);
                            doc.dirty = true;
                        }
                    }
                }
            }
            Action::PasswordSubmit(password) => {
                if let Some(dialog) = self.dialogs.password.take() {
                    if let Some(doc) = self.docs.get(dialog.doc_index) {
                        let id = doc.id;
                        let path = doc.identity.path.clone();
                        self.engine.send(PdfCommand::Open {
                            id,
                            path,
                            password: Some(password),
                        });
                    }
                }
            }
            Action::DismissError | Action::Escape => {
                if self.dialogs.error.is_some() {
                    self.dialogs.error = None;
                } else if self.dialogs.any() {
                    self.dialogs.close_all();
                } else if self.presentation {
                    self.presentation = false;
                    self.fullscreen = false;
                }
            }
            Action::CloseDialog => self.dialogs.close_all(),
            Action::ShowShortcuts(show) => self.dialogs.shortcuts = show,
            Action::ForgetDocument(row_id) => {
                if let Some(db) = &self.db {
                    let _ = db.forget_document(row_id);
                }
                self.refresh_recent();
            }
            Action::ShowDialog(kind) => match kind {
                DialogKind::Open => self.handle_command(Command::OpenDialog),
                DialogKind::Goto => self.handle_command(Command::GoToPageDialog),
                DialogKind::Settings => self.dialogs.settings = true,
                DialogKind::Info => self.dialogs.info = true,
            },
            Action::SetResumePolicy(policy) => {
                self.settings.resume_policy = policy;
                if let Some(db) = &self.db {
                    self.settings.save(db);
                }
            }
            Action::SetHighlightColor(color) => {
                self.highlight_color = color;
                if let Some(db) = &self.db {
                    let _ = db.set_setting("highlight.color", &format!("{:08X}", color));
                }
            }
            Action::GotoSearchHit(index) => {
                let viewport = self.content_rect().size();
                if let Some(doc) = self.active_doc_mut() {
                    if let Some(hit) = doc.search.hits.get(index).copied() {
                        doc.search.current = index;
                        Self::jump_to_hit(doc, hit, viewport);
                    }
                }
            }
            Action::ToggleOutline(title) => {
                if let Some(doc) = self.active_doc_mut() {
                    if !doc.expanded.remove(&title) {
                        doc.expanded.insert(title);
                    }
                }
            }
            Action::SetSmoothScroll(on) => {
                self.settings.smooth_scroll = on;
                if !on {
                    // Finish any animation in flight so nothing keeps moving.
                    for doc in &mut self.docs {
                        doc.animate_scroll(1.0, viewport);
                    }
                }
                if let Some(db) = &self.db {
                    self.settings.save(db);
                }
            }
            Action::PrintDocument => self.print_document(),
            Action::ExportRotated => self.export_rotated_copy(),
            Action::SaveSettings => {
                if let Some(db) = &self.db {
                    self.settings.save(db);
                }
            }
            Action::ScrollBy(delta) => {
                let smooth = self.settings.smooth_scroll;
                if let Some(doc) = self.active_doc_mut() {
                    doc.scroll_to(doc.scroll + delta, viewport, smooth);
                }
            }
            Action::SetPageScroll { page, frac } => {
                if let Some(doc) = self.active_doc_mut() {
                    let scroll = doc.layout.scroll_for(page, frac);
                    doc.jump_to(scroll);
                    doc.recompute_layout(viewport);
                    doc.dirty = true;
                }
            }
            Action::Quit => {
                self.save_all();
                self.quit = true;
            }
        }
        self.redraw = true;
    }

    /// Opens a URL in the user's browser after validating the scheme.
    pub fn open_url(&mut self, url: &str) {
        match crate::platform::shell_open(url) {
            Ok(()) => self.toast(ToastLevel::Info, format!("Opened {}", url)),
            Err(e) => self.toast(ToastLevel::Error, format!("Could not open link: {}", e)),
        }
    }

    /// Periodic work: engine events, autosave, expiry of transient messages.
    pub fn tick(&mut self, now: f64) {
        let dt = if self.last_time > 0.0 {
            (now - self.last_time).clamp(0.0, 0.1) as f32
        } else {
            0.0
        };
        self.last_time = now;
        if self.settings.smooth_scroll && dt > 0.0 {
            let viewport = self.content_rect().size();
            if let Some(doc) = self.active_doc_mut() {
                if doc.is_scrolling() {
                    doc.animate_scroll(dt, viewport);
                }
            }
        }
        self.pump_engine();
        if self.engine_error.is_none() && !self.engine.is_ready() {
            self.engine_error = self.engine.init_error.clone();
        }
        // Autosave dirty sessions.
        for i in 0..self.docs.len() {
            if self.docs[i].dirty && now - self.docs[i].last_saved > AUTOSAVE_INTERVAL {
                self.save_doc_session(i);
            }
        }
        // Requests for the pages that are on screen.
        self.ensure_renders();
        self.ensure_thumbnails();
        self.request_visible_text();
        let visible: Vec<u32> = self
            .active_doc()
            .map(|d| d.layout.visible_pages())
            .unwrap_or_default();
        for page in visible {
            self.request_links(page);
        }
        self.toasts.retain(|t| now - t.created < TOAST_LIFETIME);
        self.perf.cache_bytes = self.docs.iter().map(|d| d.memory_bytes()).sum();
        self.perf.pages_cached = self.docs.iter().map(|d| d.cache.len()).sum();
    }

    /// True while something on screen is animating (spinners) so the shell keeps
    /// repainting at animation rate instead of only on input.
    pub fn wants_frames(&self) -> bool {
        if !self.toasts.is_empty() {
            return true;
        }
        if self.docs.iter().any(|d| d.is_scrolling()) {
            return true;
        }
        self.docs.iter().any(|d| {
            !d.pending_renders.is_empty() || !d.pending_thumbs.is_empty() || !d.pending_text.is_empty()
        })
    }

    /// Processes one frame of input and draws the UI into `bmp`.
    pub fn frame(&mut self, input: &Input, bmp: &mut Bitmap, font: &Font) {
        let frame_started = Instant::now();
        self.frame_count += 1;
        self.input = input.clone();
        self.ui.begin_frame();
        self.ui.modal = self.dialogs.any();
        self.pump_engine();

        // Focus must not stay on a dialog field that no longer exists.
        if !self.dialogs.any() {
            if let Some(focus) = self.ui.focus {
                if focus == crate::ui::id("goto-field")
                    || focus == crate::ui::id("open-path")
                    || focus == crate::ui::id("password-field")
                {
                    self.ui.focus = None;
                }
            }
        }

        if input.key_pressed(Key::Escape) && self.print.is_some() {
            self.cancel_print();
        }

        // Escape always closes the top-most modal first.
        if input.key_pressed(Key::Escape) && self.dialogs.any() {
            self.dialogs.close_all();
            self.redraw = true;
        }

        // Global shortcuts first (unless a text field owns the keyboard focus).
        let typing = self.ui.focus.map(is_text_focus).unwrap_or(false);
        if !typing {
            if let Some(cmd) = commands::command_for_input(input) {
                self.handle_command(cmd);
            }
            if input.key_pressed(Key::Escape) && !self.dialogs.any() {
                if self.presentation {
                    self.presentation = false;
                    self.fullscreen = false;
                } else if self.docs.is_empty() {
                    self.quit = true;
                }
            }
        }

        let regions = self.regions();

        // Wheel: scroll the page area, Ctrl+wheel zooms around the pointer.
        if regions.content.contains(input.mouse) && input.wheel != 0.0 && !self.dialogs.any() {
            if input.ctrl {
                let zoom = self
                    .active_doc()
                    .map(|d| d.layout.zoom)
                    .unwrap_or(1.0);
                let next = if input.wheel > 0.0 {
                    layout::zoom_in(zoom)
                } else {
                    layout::zoom_out(zoom)
                };
                if let Some(doc) = self.active_doc_mut() {
                    doc.set_zoom(ZoomMode::Custom, next, regions.content.size());
                    self.settings.page_fit_mode = ZoomMode::Custom;
                }
            } else {
                let smooth = self.settings.smooth_scroll;
                let viewport = regions.content.size();
                if let Some(doc) = self.active_doc_mut() {
                    doc.scroll_to(doc.scroll - input.wheel * 60.0, viewport, smooth);
                }
            }
        }

        self.mouse_in_content = regions.content.contains(input.mouse);
        let theme = Theme::from_dark(self.settings.dark);
        bmp.resize(self.window.w.max(1.0) as u32, self.window.h.max(1.0) as u32);
        bmp.clear(theme.window_bg);

        let mut actions: Vec<Action> = Vec::new();
        {
            let mut ui = Ui::new(bmp, font, theme, input, &mut self.ui);
            let mut chrome = crate::ui::chrome::Chrome {
                docs: &mut self.docs,
                active: &mut self.active,
                settings: &self.settings,
                sidebar: &mut self.sidebar,
                    dialogs: &mut self.dialogs,
                toasts: &self.toasts,
                recent: &self.recent,
                fullscreen: self.fullscreen,
                presentation: self.presentation,
                show_sidebar: self.show_sidebar,
                perf: &self.perf,
                busy: &self.busy,
                note: &self.note,
                engine_error: &self.engine_error,
                db_error: &self.db_error,
                highlight_color: self.highlight_color,
                regions,
                actions: &mut actions,
                page_field: &mut self.page_field,
            };
            crate::ui::chrome::paint(&mut ui, &mut chrome);
            ui.draw_tooltip();
        }
        if let Some(text) = self.ui.clipboard_set.take() {
            actions.push(Action::CopyText(text));
        }
        for action in actions {
            self.apply_action(action);
        }

        // Pointer handling inside the page area (selection, links, highlights).
        let regions = self.regions();
        self.handle_content_pointer(input, &regions);

        self.perf.last_frame_ms = frame_started.elapsed().as_secs_f32() * 1000.0;
        if self.frame_count == 1 {
            self.perf.startup_ms = self.started.elapsed().as_secs_f32() * 1000.0;
        }
        self.redraw = false;
    }

    /// Mouse handling inside the page area: text selection, link activation and
    /// right-click removal of highlights.
    fn handle_content_pointer(&mut self, input: &Input, regions: &Regions) {
        if self.dialogs.any() || !regions.content.contains(input.mouse) {
            return;
        }
        let content = regions.content;
        let mut open_link: Option<String> = None;
        let mut delete_highlight: Option<i64> = None;
        // Clicking on a page always takes keyboard focus away from the sidebar.
        if input.pressed {
            self.ui.focus = None;
        }
        {
            let Some(doc) = self.docs.get_mut(self.active) else {
                return;
            };
            if !doc.is_ready() {
                return;
            }
            let hit = doc
                .layout
                .hit_test(content, input.mouse)
                .and_then(|(page, local)| doc.screen_to_pdf(page, local).map(|pdf| (page, pdf)));
            if input.pressed {
                if let Some((page, local)) = hit {
                    if let Some(target) = doc.link_at(page, local).cloned() {
                        match target {
                            crate::pdf::LinkTarget::Uri(uri) => open_link = Some(uri),
                            crate::pdf::LinkTarget::Page(p) => doc.goto_page(p, content.size()),
                        }
                    } else if let Some(index) = doc
                        .texts
                        .get(&page)
                        .and_then(|t| t.nearest_char(local.x, local.y))
                    {
                        doc.selection = Some(Selection {
                            page,
                            anchor: index,
                            focus: index,
                            dragging: true,
                        });
                    }
                } else {
                    doc.clear_selection();
                }
            }
            if input.double_click {
                if let Some((page, local)) = hit {
                    if let Some(text) = doc.texts.get(&page) {
                        let index = text.nearest_char(local.x, local.y).unwrap_or(0);
                        let (start, end) = word_bounds(&text.text, index);
                        doc.selection = Some(Selection {
                            page,
                            anchor: start,
                            focus: end,
                            dragging: false,
                        });
                    }
                }
            }
            if input.right_pressed {
                if let Some((page, local)) = hit {
                    delete_highlight = doc.highlight_at(page, local);
                }
            }
            if let Some(sel) = &mut doc.selection {
                if sel.dragging {
                    if input.down {
                        if let Some((page, local)) = hit {
                            if page == sel.page {
                                if let Some(index) =
                                    doc.texts.get(&sel.page).and_then(|t| t.nearest_char(local.x, local.y))
                                {
                                    sel.focus = index;
                                }
                            }
                        }
                    } else {
                        sel.dragging = false;
                        if sel.is_empty() {
                            doc.selection = None;
                        }
                    }
                }
            }
        }
        if let Some(uri) = open_link {
            self.open_url(&uri);
        }
        if let Some(id) = delete_highlight {
            self.delete_highlight(id);
        }
    }
}

fn is_text_focus(id: crate::ui::Id) -> bool {
    id == crate::ui::id("search-field")
        || id == crate::ui::id("page-field")
        || id == crate::ui::id("goto-field")
        || id == crate::ui::id("open-path")
        || id == crate::ui::id("password-field")
}

/// Selects the word around `index`.
fn word_bounds(text: &str, index: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut start = index.min(chars.len());
    let mut end = start;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_bounds_finds_the_word() {
        assert_eq!(word_bounds("hello brave world", 7), (6, 11));
        assert_eq!(word_bounds("hello", 2), (0, 5));
        assert_eq!(word_bounds("", 0), (0, 0));
    }

    #[test]
    fn regions_shrink_with_sidebar_and_status() {
        let mut app = App::new(Engine::start(), None, None);
        app.window = Size::new(1000.0, 800.0);
        app.docs.clear();
        let regions = app.regions();
        assert_eq!(regions.status.h, Metrics::STATUS_H);
        assert_eq!(regions.toolbar.h, Metrics::TOOLBAR_H);
        assert!(regions.sidebar.w > 100.0);
        assert_eq!(regions.content.w, 1000.0 - regions.sidebar.w);
        assert_eq!(regions.tabbar.h, 0.0, "single document shows no tab bar");
        app.show_sidebar = false;
        let regions = app.regions();
        assert_eq!(regions.sidebar.w, 0.0);
        assert_eq!(regions.content.w, 1000.0);
        app.presentation = true;
        let regions = app.regions();
        assert_eq!(regions.toolbar.h, 0.0);
        assert_eq!(regions.sidebar.w, 0.0);
    }

    #[test]
    fn settings_round_trip_through_the_database() {
        let db = Database::open_in_memory().unwrap();
        let mut settings = Settings::default();
        settings.dark = true;
        settings.sidebar_width = 300.0;
        settings.resume_policy = ResumePolicy::Never;
        settings.window = Rect::new(10.0, 20.0, 900.0, 700.0);
        settings.save(&db);
        let loaded = Settings::load(&db);
        assert!(loaded.dark);
        assert!((loaded.sidebar_width - 300.0).abs() < 0.1);
        assert_eq!(loaded.resume_policy, ResumePolicy::Never);
        assert!((loaded.window.w - 900.0).abs() < 1.0);
    }

    #[test]
    fn opening_a_missing_file_reports_an_error_dialog() {
        let mut app = App::new(Engine::start(), None, None);
        app.open_path(PathBuf::from("/nope/missing.pdf"), None);
        assert!(app.docs.is_empty());
        assert!(app.dialogs.error.is_some());
    }
}
