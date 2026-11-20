//! Hand-written PDFium C API bindings, loaded dynamically at runtime.
//!
//! PDFium is not linked at build time: the shared library (`libpdfium.so` on
//! Linux, `pdfium.dll` on Windows) is opened with `dlopen`/`LoadLibrary` the
//! first time a document is opened. That keeps the executable small, lets the
//! engine live in a separate file that users can update independently, and
//! means a missing engine degrades into a clean error message instead of a
//! failure to start (or to link).
//!
//! Only the subset of the API the reader needs is declared here, each function
//! exactly as documented in `public/fpdf_*.h`.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_uint, c_ulong, c_ushort, c_void};
use std::path::PathBuf;

pub type Bool = c_int;
pub type FPDF_DOCUMENT = *mut c_void;
pub type FPDF_PAGE = *mut c_void;
pub type FPDF_BITMAP = *mut c_void;
pub type FPDF_TEXTPAGE = *mut c_void;
pub type FPDF_SCHHANDLE = *mut c_void;
pub type FPDF_BOOKMARK = *mut c_void;
pub type FPDF_DEST = *mut c_void;
pub type FPDF_ACTION = *mut c_void;
pub type FPDF_LINK = *mut c_void;
pub type FPDF_ANNOTATION = *mut c_void;

pub const FPDF_PAGEOBJ_UNKNOWN: c_int = 0;

/// `FPDF_BITMAP_FORMAT_BGRA`
#[allow(non_upper_case_globals)]
pub const FPDFBitmap_BGRA: c_int = 4;

// Render flags (fpdf_render.h).
pub const FPDF_ANNOT: c_int = 0x01;
pub const FPDF_LCD_TEXT: c_int = 0x02;
pub const FPDF_NO_NATIVETEXT: c_int = 0x04;
pub const FPDF_GRAYSCALE: c_int = 0x08;
pub const FPDF_REVERSE_BYTE_ORDER: c_int = 0x10;

// Error codes (fpdfview.h).
pub const FPDF_ERR_SUCCESS: c_ulong = 0;
pub const FPDF_ERR_UNKNOWN: c_ulong = 1;
pub const FPDF_ERR_FILE: c_ulong = 2;
pub const FPDF_ERR_FORMAT: c_ulong = 3;
pub const FPDF_ERR_PASSWORD: c_ulong = 4;
pub const FPDF_ERR_SECURITY: c_ulong = 5;
pub const FPDF_ERR_PAGE: c_ulong = 6;

// Save flags (fpdf_save.h).
pub const FPDF_NO_INCREMENTAL: c_int = 1 << 1;
pub const FPDF_REMOVE_SECURITY: c_int = 1 << 3;

// Search flags (fpdf_text.h).
pub const FPDF_MATCHCASE: c_ulong = 0x0000_0001;
pub const FPDF_MATCHWHOLEWORD: c_ulong = 0x0000_0002;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FS_RECTF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// The file-write callback structure required by `FPDF_SaveAsCopy`.
#[repr(C)]
pub struct FPDF_FILEWRITE {
    pub version: c_int,
    pub write_block: Option<unsafe extern "C" fn(*mut FPDF_FILEWRITE, *const c_void, c_ulong) -> Bool>,
}

macro_rules! declare_api {
    ($( fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty ; )*) => {
        /// Resolved PDFium entry points. Field names mirror the exported C
        /// symbols so they can be looked up verbatim.
        #[allow(non_snake_case)]
        pub struct Api {
            $( pub $name: unsafe extern "C" fn($($ty),*) -> $ret, )*
        }

        impl Api {
            fn load(get: &dyn Fn(&str) -> Option<*mut c_void>) -> Result<Api, String> {
                Ok(Api {
                    $( $name: {
                        let sym = concat!(stringify!($name), "\0");
                        let ptr = get(sym).ok_or_else(|| {
                            format!("the PDFium build is missing the symbol {}", stringify!($name))
                        })?;
                        // SAFETY: the symbol comes from a PDFium library and its
                        // declared signature mirrors the public header.
                        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($ty),*) -> $ret>(ptr) }
                    }, )*
                })
            }
        }
    };
}

declare_api! {
    // Library / document lifecycle.
    fn FPDF_InitLibrary() -> ();
    fn FPDF_DestroyLibrary() -> ();
    fn FPDF_GetLastError() -> c_ulong;
    fn FPDF_LoadMemDocument(data: *const c_void, size: c_int, password: *const c_char) -> FPDF_DOCUMENT;
    fn FPDF_LoadDocument(path: *const c_char, password: *const c_char) -> FPDF_DOCUMENT;
    fn FPDF_CloseDocument(doc: FPDF_DOCUMENT) -> ();
    fn FPDF_GetPageCount(doc: FPDF_DOCUMENT) -> c_int;
    fn FPDF_GetFileVersion(doc: FPDF_DOCUMENT, version: *mut c_int) -> Bool;
    fn FPDF_GetDocPermissions(doc: FPDF_DOCUMENT) -> c_ulong;
    fn FPDF_GetSecurityHandlerRevision(doc: FPDF_DOCUMENT) -> c_int;
    fn FPDF_GetPageSizeByIndexF(doc: FPDF_DOCUMENT, index: c_int, size: *mut FS_SIZEF) -> Bool;
    fn FPDF_GetMetaText(doc: FPDF_DOCUMENT, tag: *const c_char, buffer: *mut c_void, buflen: c_ulong) -> c_ulong;
    fn FPDF_GetPageLabel(doc: FPDF_DOCUMENT, index: c_int, buffer: *mut c_void, buflen: c_ulong) -> c_ulong;

    // Pages and rendering.
    fn FPDF_LoadPage(doc: FPDF_DOCUMENT, index: c_int) -> FPDF_PAGE;
    fn FPDF_ClosePage(page: FPDF_PAGE) -> ();
    fn FPDF_GetPageWidthF(page: FPDF_PAGE) -> f32;
    fn FPDF_GetPageHeightF(page: FPDF_PAGE) -> f32;
    fn FPDFPage_GetRotation(page: FPDF_PAGE) -> c_int;
    fn FPDFPage_SetRotation(page: FPDF_PAGE, rotation: c_int) -> ();
    fn FPDF_RenderPageBitmap(bitmap: FPDF_BITMAP, page: FPDF_PAGE, start_x: c_int, start_y: c_int, size_x: c_int, size_y: c_int, rotate: c_int, flags: c_int) -> ();
    fn FPDF_GetPageBoundingBox(page: FPDF_PAGE, rect: *mut FS_RECTF) -> Bool;
    fn FPDFBitmap_CreateEx(width: c_int, height: c_int, format: c_int, first_scan: *mut c_void, stride: c_int) -> FPDF_BITMAP;
    fn FPDFBitmap_Destroy(bitmap: FPDF_BITMAP) -> ();
    fn FPDFBitmap_FillRect(bitmap: FPDF_BITMAP, left: c_int, top: c_int, width: c_int, height: c_int, color: c_ulong) -> ();
    fn FPDFBitmap_GetBuffer(bitmap: FPDF_BITMAP) -> *mut c_void;

    // Text extraction, selection and search.
    fn FPDFText_LoadPage(page: FPDF_PAGE) -> FPDF_TEXTPAGE;
    fn FPDFText_ClosePage(text_page: FPDF_TEXTPAGE) -> ();
    fn FPDFText_CountChars(text_page: FPDF_TEXTPAGE) -> c_int;
    fn FPDFText_GetText(text_page: FPDF_TEXTPAGE, start_index: c_int, count: c_int, result: *mut c_ushort) -> c_int;
    fn FPDFText_GetCharBox(text_page: FPDF_TEXTPAGE, index: c_int, left: *mut f64, right: *mut f64, bottom: *mut f64, top: *mut f64) -> Bool;
    fn FPDFText_GetCharIndexAtPos(text_page: FPDF_TEXTPAGE, x: f64, y: f64, x_tolerance: f64, y_tolerance: f64) -> c_int;
    fn FPDFText_CountRects(text_page: FPDF_TEXTPAGE, start_index: c_int, count: c_int) -> c_int;
    fn FPDFText_GetRect(text_page: FPDF_TEXTPAGE, rect_index: c_int, left: *mut f64, top: *mut f64, right: *mut f64, bottom: *mut f64) -> Bool;
    fn FPDFText_GetBoundedText(text_page: FPDF_TEXTPAGE, left: f64, top: f64, right: f64, bottom: f64, buffer: *mut c_ushort, buflen: c_int) -> c_int;
    fn FPDFText_FindStart(text_page: FPDF_TEXTPAGE, findwhat: *const c_ushort, flags: c_ulong, start_index: c_int) -> FPDF_SCHHANDLE;
    fn FPDFText_FindNext(handle: FPDF_SCHHANDLE) -> Bool;
    fn FPDFText_FindPrev(handle: FPDF_SCHHANDLE) -> Bool;
    fn FPDFText_GetSchResultIndex(handle: FPDF_SCHHANDLE) -> c_int;
    fn FPDFText_GetSchCount(handle: FPDF_SCHHANDLE) -> c_int;
    fn FPDFText_FindClose(handle: FPDF_SCHHANDLE) -> ();

    // Bookmarks / outline.
    fn FPDFBookmark_GetFirstChild(doc: FPDF_DOCUMENT, bookmark: FPDF_BOOKMARK) -> FPDF_BOOKMARK;
    fn FPDFBookmark_GetNextSibling(doc: FPDF_DOCUMENT, bookmark: FPDF_BOOKMARK) -> FPDF_BOOKMARK;
    fn FPDFBookmark_GetTitle(bookmark: FPDF_BOOKMARK, buffer: *mut c_void, buflen: c_ulong) -> c_ulong;
    fn FPDFBookmark_GetDest(doc: FPDF_DOCUMENT, bookmark: FPDF_BOOKMARK) -> FPDF_DEST;
    fn FPDFBookmark_GetAction(bookmark: FPDF_BOOKMARK) -> FPDF_ACTION;
    fn FPDFDest_GetDestPageIndex(doc: FPDF_DOCUMENT, dest: FPDF_DEST) -> c_int;
    fn FPDFAction_GetType(action: FPDF_ACTION) -> c_ulong;
    fn FPDFAction_GetURIPath(doc: FPDF_DOCUMENT, action: FPDF_ACTION, buffer: *mut c_void, buflen: c_ulong) -> c_ulong;

    // Links (annotations of subtype /Link).
    fn FPDFLink_LoadWebLinks(text_page: FPDF_TEXTPAGE) -> *mut c_void;
    fn FPDFLink_CountWebLinks(link_page: *mut c_void) -> c_int;
    fn FPDFLink_GetURL(link_page: *mut c_void, link_index: c_int, buffer: *mut c_ushort, buflen: c_int) -> c_int;
    fn FPDFLink_CountRects(link_page: *mut c_void, link_index: c_int) -> c_int;
    fn FPDFLink_GetRect(link_page: *mut c_void, link_index: c_int, rect_index: c_int, left: *mut f64, top: *mut f64, right: *mut f64, bottom: *mut f64) -> Bool;
    fn FPDFLink_CloseWebLinks(link_page: *mut c_void) -> ();
    fn FPDFLink_GetLinkAtPoint(page: FPDF_PAGE, x: f64, y: f64) -> FPDF_LINK;
    fn FPDFLink_GetAnnotRect(link: FPDF_LINK, rect: *mut FS_RECTF) -> Bool;
    fn FPDFLink_GetAction(link: FPDF_LINK) -> FPDF_ACTION;
    fn FPDFLink_Enumerate(page: FPDF_PAGE, start_pos: *mut c_int, link_annot: *mut FPDF_LINK) -> Bool;
    fn FPDFLink_GetDest(doc: FPDF_DOCUMENT, link: FPDF_LINK) -> FPDF_DEST;

    // Saving / exporting.
    fn FPDF_SaveAsCopy(doc: FPDF_DOCUMENT, file_write: *mut FPDF_FILEWRITE, flags: c_uint) -> Bool;
}

#[cfg(unix)]
mod dynlib {
    use core::ffi::{c_char, c_int, c_void};

    #[link(name = "dl")]
    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }

    pub const RTLD_NOW: c_int = 2;

    pub fn open(path: &str) -> Result<*mut c_void, String> {
        let c = std::ffi::CString::new(path).map_err(|e| e.to_string())?;
        // SAFETY: valid NUL-terminated path.
        let handle = unsafe { dlopen(c.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            let msg = unsafe {
                let err = dlerror();
                if err.is_null() {
                    "unknown dynamic loader error".to_string()
                } else {
                    std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
                }
            };
            return Err(msg);
        }
        Ok(handle)
    }

    pub fn sym(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c = std::ffi::CString::new(name.trim_end_matches('\0')).ok()?;
        // SAFETY: handle came from dlopen.
        let p = unsafe { dlsym(handle, c.as_ptr()) };
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }
}

#[cfg(windows)]
mod dynlib {
    use core::ffi::{c_void, c_int};
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    pub fn open(path: &str) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: NUL-terminated UTF-16 path.
        let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "LoadLibraryW failed for {} (error {})",
                path,
                std::io::Error::last_os_error()
            ));
        }
        Ok(handle)
    }

    pub fn sym(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let mut c = Vec::with_capacity(name.len() + 1);
        c.extend_from_slice(name.as_bytes());
        c.push(0);
        // SAFETY: valid NUL-terminated ASCII symbol name.
        let p = unsafe { GetProcAddress(handle, c.as_ptr()) };
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }

    #[allow(dead_code)]
    fn _unused(_: c_int) {}
}

/// A loaded PDFium library plus its resolved entry points.
pub struct PdfiumLib {
    handle: *mut c_void,
    pub api: Api,
    pub path: PathBuf,
}

// SAFETY: the library handle is only ever used from the single PDF worker
// thread; PDFium itself is not thread-safe, which the design enforces by
// funnelling every call through that worker.
unsafe impl Send for PdfiumLib {}

impl PdfiumLib {
    /// Looks for the engine in the well-known locations and loads it.
    pub fn open() -> Result<PdfiumLib, String> {
        let mut tried: Vec<String> = Vec::new();
        for candidate in search_paths() {
            match dynlib::open(&candidate.to_string_lossy()) {
                Ok(handle) => {
                    let api = Api::load(&|sym_name| dynlib::sym(handle, sym_name))
                        .map_err(|e| format!("{}: {}", candidate.display(), e))?;
                    return Ok(PdfiumLib {
                        handle,
                        api,
                        path: candidate,
                    });
                }
                Err(e) => tried.push(format!("{} ({})", candidate.display(), e)),
            }
        }
        Err(format!(
            "PDFium could not be loaded. Install libpdfium.so / pdfium.dll next to the executable or in ./runtime/pdfium, or set PDFRUST_PDFIUM. Tried: {}",
            tried.join("; ")
        ))
    }

    /// True when the given library is still open (the handle is never closed,
    /// because PDFium has no safe unload path once documents were created).
    pub fn is_loaded(&self) -> bool {
        !self.handle.is_null()
    }
}

/// Process-wide PDFium instance.
///
/// PDFium keeps global state and is not safe for concurrent use, so the library
/// is initialised exactly once per process and every call is serialised through
/// [`CALLS`]. The application runs a single engine worker, but tests (and any
/// future multi-window design) may create more than one; this keeps them safe.
static API: std::sync::OnceLock<Result<&'static Api, String>> = std::sync::OnceLock::new();

/// Serialises every call into PDFium across the whole process.
pub static CALLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Returns the shared, initialised PDFium API (loading it on first use).
pub fn global() -> Result<&'static Api, String> {
    API.get_or_init(|| {
        let lib: &'static PdfiumLib = Box::leak(Box::new(PdfiumLib::open()?));
        // SAFETY: PDFium's documented initialisation entry point; called once.
        unsafe { (lib.api.FPDF_InitLibrary)() };
        Ok(&lib.api)
    })
    .clone()
}

/// True when a PDFium shared library can be found in the usual locations.
pub fn available() -> bool {
    search_paths().iter().any(|p| p.is_file())
}

/// Candidate library paths, most specific first.
pub fn search_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(explicit) = std::env::var("PDFRUST_PDFIUM") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    let file_names: &[&str] = if cfg!(windows) {
        &["pdfium.dll", "libpdfium.dll"]
    } else {
        &["libpdfium.so", "libpdfium.so.1", "libpdfium.dylib"]
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in file_names {
                out.push(dir.join(name));
                out.push(dir.join("runtime").join("pdfium").join(name));
                out.push(dir.join("lib").join(name));
                // Packaged layout: /usr/bin/pdf-rust + /usr/lib/pdf-rust/libpdfium.so
                out.push(dir.join("..").join("lib").join("pdf-rust").join(name));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for name in file_names {
            out.push(cwd.join("runtime").join("pdfium").join(name));
            out.push(cwd.join(name));
        }
    }
    if cfg!(windows) {
        for name in file_names {
            out.push(PathBuf::from(format!("C:\\Program Files\\pdf-rust\\{}", name)));
        }
    } else {
        for name in file_names {
            out.push(PathBuf::from("/usr/lib/pdf-rust").join(name));
            out.push(PathBuf::from("/usr/lib").join(name));
            out.push(PathBuf::from("/usr/lib/x86_64-linux-gnu").join(name));
            out.push(PathBuf::from("/usr/local/lib").join(name));
        }
    }
    out
}

/// Converts a Rust string into a NUL-terminated `CString` for APIs that take
/// `const char*` (PDFium treats them as UTF-8).
pub fn cstring(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Encodes a Rust string as UTF-16LE for the wide-character PDFium APIs.
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Decodes a UTF-16LE buffer returned by PDFium (length includes the NUL).
pub fn utf16_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Convenience wrapper for the "query length, then fill" PDFium pattern.
pub fn wide_string_from_len_query(
    mut query: impl FnMut(*mut c_void, c_ulong) -> c_ulong,
) -> String {
    // SAFETY: passing a null buffer with length 0 is the documented way to ask
    // for the required byte length.
    let len = query(std::ptr::null_mut(), 0);
    if len == 0 {
        return String::new();
    }
    let units = (len as usize + 1) / 2;
    let mut buf = vec![0u16; units];
    let written = query(buf.as_mut_ptr() as *mut c_void, len);
    if written == 0 {
        return String::new();
    }
    let units = (written as usize + 1) / 2;
    utf16_to_string(&buf[..units.min(buf.len())])
}

/// Same as [`wide_string_from_len_query`] but for APIs that return a
/// NUL-terminated single-byte (UTF-8/Latin-1) string, such as
/// `FPDFAction_GetURIPath`.
pub fn byte_string_from_len_query(
    mut query: impl FnMut(*mut c_void, c_ulong) -> c_ulong,
) -> String {
    let len = query(std::ptr::null_mut(), 0);
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u8; len as usize + 1];
    let written = query(buf.as_mut_ptr() as *mut c_void, buf.len() as c_ulong);
    if written == 0 {
        return String::new();
    }
    let end = buf
        .iter()
        .position(|b| *b == 0)
        .unwrap_or_else(|| (written as usize).min(buf.len()));
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

pub fn last_error_message(code: c_ulong) -> &'static str {
    match code {
        FPDF_ERR_SUCCESS => "no error",
        FPDF_ERR_FILE => "the file could not be opened",
        FPDF_ERR_FORMAT => "the file is not a valid PDF document",
        FPDF_ERR_PASSWORD => "the document is password protected",
        FPDF_ERR_SECURITY => "the document uses an unsupported security scheme",
        FPDF_ERR_PAGE => "the requested page does not exist",
        _ => "an unknown PDF error occurred",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_paths_are_ordered_and_nonempty() {
        let paths = search_paths();
        assert!(!paths.is_empty());
        assert!(paths.iter().all(|p| !p.as_os_str().is_empty()));
    }

    #[test]
    fn utf16_round_trip() {
        let s = "Hello, PDF — Unicode ✓";
        assert_eq!(utf16_to_string(&utf16(s)), s);
    }

    #[test]
    fn last_error_strings_exist() {
        assert!(!last_error_message(FPDF_ERR_PASSWORD).is_empty());
        assert!(!last_error_message(999).is_empty());
    }
}
