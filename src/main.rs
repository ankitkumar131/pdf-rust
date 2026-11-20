//! `pdf-rust` — a fast, lightweight, native PDF reader.
//!
//! Usage:
//! ```text
//! pdf-rust [options] [document.pdf ...]
//!
//! Options:
//!   -h, --help             Show this help
//!   -V, --version          Print the version
//!       --db <path>        Use a specific library database
//!       --no-db            Do not read or write the library database
//!       --page <n>         Open the first document at page n
//!       --stats            Print startup/memory statistics at exit
//!       --selftest [pdf]   Headless smoke test and performance measurement
//!       --screenshot <png> Render one frame headlessly and write it as a PNG
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use pdf_rust::app::App;
use pdf_rust::db::{default_database_path, Database};
use pdf_rust::pdf::Engine;
use pdf_rust::ui::Font;

const USAGE: &str = "\
pdf-rust — a fast, lightweight, native PDF reader

Usage: pdf-rust [options] [document.pdf ...]

Options:
  -h, --help             Show this help
  -V, --version          Print the version
      --db <path>        Use a specific library database
      --no-db            Do not read or write the library database
      --page <n>         Open the first document at page n
      --stats            Print startup/memory statistics at exit
      --selftest [pdf]   Headless smoke test and performance measurement
      --screenshot <png> Render one frame headlessly and write it as a PNG
      --dark             Start in the dark theme
      --size <WxH>       Window size for --screenshot (default 1180x860)
";

struct Options {
    db: Option<PathBuf>,
    no_db: bool,
    page: Option<u32>,
    stats: bool,
    selftest: bool,
    screenshot: Option<PathBuf>,
    dark: bool,
    size: Option<(u32, u32)>,
    docs: Vec<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options {
        db: None,
        no_db: false,
        page: None,
        stats: false,
        selftest: false,
        screenshot: None,
        dark: false,
        size: None,
        docs: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-h" | "--help" => {
                print!("{}", USAGE);
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("{} {}", pdf_rust::APP_NAME, pdf_rust::APP_VERSION);
                return Ok(None);
            }
            "--no-db" => options.no_db = true,
            "--stats" => options.stats = true,
            "--selftest" => options.selftest = true,
            "--dark" => options.dark = true,
            "--screenshot" => {
                i += 1;
                let value = args.get(i).ok_or("--screenshot needs a path")?;
                options.screenshot = Some(PathBuf::from(value));
            }
            "--size" => {
                i += 1;
                let value = args.get(i).ok_or("--size needs WxH")?;
                let (w, h) = value
                    .split_once(['x', 'X'])
                    .ok_or_else(|| format!("invalid size: {} (expected 1180x860)", value))?;
                let w: u32 = w.parse().map_err(|_| format!("invalid width: {}", w))?;
                let h: u32 = h.parse().map_err(|_| format!("invalid height: {}", h))?;
                if w < 320 || h < 240 || w > 4096 || h > 4096 {
                    return Err(format!("size out of range: {}x{}", w, h));
                }
                options.size = Some((w, h));
            }
            "--db" => {
                i += 1;
                let value = args.get(i).ok_or("--db needs a path")?;
                options.db = Some(PathBuf::from(value));
            }
            "--page" => {
                i += 1;
                let value = args.get(i).ok_or("--page needs a number")?;
                options.page = Some(value.parse().map_err(|_| format!("invalid page: {}", value))?);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {}", other));
            }
            other => options.docs.push(PathBuf::from(other)),
        }
        i += 1;
    }
    Ok(Some(options))
}

fn open_database(options: &Options) -> (Option<Database>, Option<String>) {
    if options.no_db {
        return (None, None);
    }
    let path = options
        .db
        .clone()
        .or_else(default_database_path)
        .unwrap_or_else(|| PathBuf::from("library.db"));
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                return (
                    None,
                    Some(format!("could not create {}: {}", dir.display(), e)),
                );
            }
        }
    }
    match Database::open(&path) {
        Ok(db) => {
            if let Err(e) = db.migrate() {
                return (Some(db), Some(format!("schema migration failed: {}", e)));
            }
            (Some(db), None)
        }
        Err(e) => (None, Some(format!("{} ({})", e, path.display()))),
    }
}

/// Headless smoke test: drives the application without a window and reports the
/// timings the README quotes. Used by CI and by `scripts/bench.sh`.
fn selftest(options: &Options) -> Result<(), String> {
    use pdf_rust::ui::{Bitmap, Font, Input};

    let started = Instant::now();
    if !pdf_rust::pdf::ffi::available() {
        let tried: Vec<String> = pdf_rust::pdf::ffi::search_paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        return Err(format!(
            "PDFium was not found. Put libpdfium.so / pdfium.dll next to the executable, \
             in ./runtime/pdfium, or set PDFRUST_PDFIUM.\n  tried: {}",
            tried.join(", ")
        ));
    }
    let engine = Engine::start();
    let (db, db_error) = open_database(options);
    let mut app = App::new(engine, db, db_error);
    app.window = pdf_rust::geom::Size::new(1180.0, 860.0);
    let font = Font::ui();
    let mut bmp = Bitmap::new(1180, 860);
    let input = Input::default();

    // Warm the UI up with one frame of the empty window.
    app.frame(&input, &mut bmp, &font);
    let first_frame_ms = started.elapsed().as_secs_f32() * 1000.0;

    let mut rendered_ms = f32::NAN;
    if let Some(path) = options.docs.first() {
        app.open_path(path.clone(), None);
        let open_started = Instant::now();
        let deadline = Instant::now() + std::time::Duration::from_secs(20);
        while Instant::now() < deadline {
            app.tick(app.now());
            app.frame(&input, &mut bmp, &font);
            let ready = app.active_doc().map(|d| d.is_ready()).unwrap_or(false);
            let drawn = app.active_doc().map(|d| d.cache.len() > 0).unwrap_or(false);
            if ready && drawn {
                rendered_ms = open_started.elapsed().as_secs_f32() * 1000.0;
                break;
            }
            if app.active_doc().map(|d| d.error.is_some()).unwrap_or(false) {
                return Err(format!(
                    "the document could not be opened: {}",
                    app.active_doc().and_then(|d| d.error.clone()).unwrap_or_default()
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if rendered_ms.is_nan() {
            let status = app
                .active_doc()
                .map(|d| format!("ready={} pages={}", d.is_ready(), d.page_count()))
                .unwrap_or_else(|| "no document".to_string());
            return Err(format!(
                "timed out waiting for the first page ({status}); \
                 is {} readable and a valid PDF?",
                path.display()
            ));
        }
        // Give the worker a moment to finish the visible pages, then measure a
        // steady-state frame.
        for _ in 0..40 {
            app.tick(app.now());
            app.frame(&input, &mut bmp, &font);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    } else {
        app.frame(&input, &mut bmp, &font);
    }

    let doc = app.active_doc();
    println!("pdf-rust selftest");
    println!("  first frame        {:.1} ms", first_frame_ms);
    if !rendered_ms.is_nan() {
        println!("  document -> pixels {:.1} ms", rendered_ms);
    }
    println!("  frame (steady)     {:.2} ms", app.perf.last_frame_ms);
    println!("  page render        {:.1} ms", app.perf.last_render_ms);
    println!("  page cache         {}", pdf_rust::util::stats::human_bytes(app.perf.cache_bytes as u64));
    println!("  peak memory        {}", pdf_rust::util::stats::human_bytes(pdf_rust::util::stats::peak_rss_bytes()));
    if let Some(doc) = doc {
        println!("  document           {} pages", doc.page_count());
        println!("  text selection     {}", if doc.texts.is_empty() { "not loaded" } else { "ready" });
    }
    app.save_all();
    Ok(())
}

/// Renders one frame of the viewer headlessly and writes it as a PNG.
///
/// This is how documentation screenshots are produced (and how a visual check
/// can run without a display server); it exercises exactly the paint path the
/// windowed shells use.
fn screenshot(options: &Options, out: &std::path::Path) -> Result<(), String> {
    use pdf_rust::ui::{Bitmap, Font, Input};

    let (w, h) = options.size.unwrap_or((1180, 860));
    let engine = Engine::start();
    if let Some(err) = engine.init_error.clone() {
        return Err(err);
    }
    let (db, db_error) = open_database(options);
    let mut app = App::new(engine, db, db_error);
    app.window = pdf_rust::geom::Size::new(w as f32, h as f32);
    app.settings.dark = options.dark;
    let font = Font::ui();
    let mut bmp = Bitmap::new(w, h);
    let input = Input::default();

    if let Some(page) = options.page {
        app.pending_initial_page = Some(page);
    }
    if let Some(path) = options.docs.first() {
        app.open_path(path.clone(), None);
        let deadline = Instant::now() + std::time::Duration::from_secs(20);
        while Instant::now() < deadline {
            app.tick(app.now());
            app.frame(&input, &mut bmp, &font);
            let drawn = app
                .active_doc()
                .map(|d| d.is_ready() && d.cache.len() > 0 && !d.thumbs.is_empty())
                .unwrap_or(false);
            if drawn {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if let Some(doc) = app.active_doc() {
            if let Some(err) = &doc.error {
                return Err(format!("the document could not be opened: {}", err));
            }
        }
        // Let thumbnails and prefetched pages settle, then paint a final frame.
        for _ in 0..60 {
            app.tick(app.now());
            app.frame(&input, &mut bmp, &font);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    // One more frame so the recorded timing is a steady-state one.
    app.frame(&input, &mut bmp, &font);
    let png = pdf_rust::util::png::encode_rgb(w, h, &bmp.px);
    if let Some(dir) = out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("could not create {}: {}", dir.display(), e))?;
        }
    }
    std::fs::write(out, &png).map_err(|e| format!("could not write {}: {}", out.display(), e))?;
    println!(
        "wrote {} ({}x{}, {})",
        out.display(),
        w,
        h,
        pdf_rust::util::stats::human_bytes(png.len() as u64)
    );
    Ok(())
}

fn run(options: Options) -> Result<(), String> {
    if options.selftest {
        return selftest(&options);
    }
    if let Some(out) = options.screenshot.clone() {
        return screenshot(&options, &out);
    }
    let started = Instant::now();
    if !pdf_rust::pdf::ffi::available() {
        let tried: Vec<String> = pdf_rust::pdf::ffi::search_paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        return Err(format!(
            "PDFium was not found. Put libpdfium.so / pdfium.dll next to the executable, \
             in ./runtime/pdfium, or set PDFRUST_PDFIUM.\n  tried: {}",
            tried.join(", ")
        ));
    }
    let engine = Engine::start();
    if let Some(err) = &engine.init_error {
        eprintln!("pdf-rust: warning: {}", err);
    }
    let (db, db_error) = open_database(&options);
    if let Some(err) = &db_error {
        eprintln!("pdf-rust: warning: reading history is unavailable: {}", err);
    }
    let mut app = App::new(engine, db, db_error);
    for doc in &options.docs {
        app.open_path(doc.clone(), None);
        if app.dialogs.error.is_some() {
            // Keep going: other documents may still open. The error is shown in
            // the window, which is friendlier than exiting with a message.
        }
    }
    if let Some(page) = options.page {
        app.pending_initial_page = Some(page);
    }
    let startup_ms = started.elapsed().as_secs_f32() * 1000.0;
    if std::env::var("PDFRUST_STATS").is_ok() {
        eprintln!("pdf-rust: startup {:.1} ms", startup_ms);
    }
    let font = Font::ui();

    #[cfg(target_os = "linux")]
    {
        pdf_rust::platform::x11::run(app, font)?;
    }
    #[cfg(windows)]
    {
        pdf_rust::platform::win32::run(app, font)?;
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (app, font);
        return Err("this build has no window system shell".to_string());
    }

    if options.stats || std::env::var("PDFRUST_STATS").is_ok() {
        let rss = pdf_rust::util::stats::peak_rss_bytes();
        eprintln!(
            "pdf-rust: startup {:.1} ms, peak memory {:.1} MB",
            startup_ms,
            rss as f64 / (1024.0 * 1024.0)
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args).and_then(|options| match options {
        Some(options) => run(options),
        None => Ok(()),
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("pdf-rust: {}", message);
            eprintln!("Try `pdf-rust --help`.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_paths() {
        let options = parse_args(&args(&["--page", "12", "--no-db", "a.pdf", "b.pdf"]))
            .unwrap()
            .unwrap();
        assert_eq!(options.page, Some(12));
        assert!(options.no_db);
        assert_eq!(options.docs.len(), 2);
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(parse_args(&args(&["--wat"])).is_err());
        assert!(parse_args(&args(&["--page", "abc"])).is_err());
        assert!(parse_args(&args(&["--db"])).is_err());
    }

    #[test]
    fn help_short_circuits() {
        assert!(parse_args(&args(&["--help"])).unwrap().is_none());
        assert!(parse_args(&args(&["-V"])).unwrap().is_none());
    }
}
