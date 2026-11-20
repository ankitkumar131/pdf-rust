//! End-to-end tests for the viewer: application state, chrome rendering and
//! persistence, all without a display server.
//!
//! The platform shells only pump events in and blit the frame out, so the whole
//! application can be driven headlessly — which is exactly what happens here.

mod common;

use common::{PdfBuilder, TempDir, TestPage};
use pdf_rust::app::commands::Command;
use pdf_rust::app::docview::DocState;
use pdf_rust::app::{Action, App};
use pdf_rust::db::{Database, ReadingMode, ZoomMode};
use pdf_rust::geom::{Point, Rect, Size};
use pdf_rust::pdf::Engine;
use pdf_rust::ui::{Bitmap, Font, Input, Key, KeyEvent};
use std::sync::Arc;

fn font() -> Font {
    Font::ui()
}

fn build_pdf(dir: &TempDir, pages: u32) -> std::path::PathBuf {
    let pages: Vec<TestPage> = (0..pages)
        .map(|i| TestPage::a4(0).with_text(&[&format!("Page {}", i + 1), "The quick brown fox"]))
        .collect();
    PdfBuilder::new(pages).write_to(&dir.join("viewer.pdf"))
}

/// Makes an app that persists into `db_path`.
fn app_with_db(db_path: &std::path::Path) -> App {
    let db = Database::open(db_path).expect("database");
    db.migrate().expect("migration");
    App::new(Engine::start(), Some(db), None)
}

fn input_none() -> Input {
    Input::default()
}

fn press(key: Key, ctrl: bool, shift: bool) -> Input {
    let mut input = Input {
        ctrl,
        shift,
        ..Default::default()
    };
    input.keys.push(KeyEvent { key, repeat: false });
    input
}

/// Runs frames until `done` is true (or the deadline expires).
fn pump(app: &mut App, bmp: &mut Bitmap, font: &Font, mut done: impl FnMut(&App) -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        app.tick(app.now());
        app.frame(&input_none(), bmp, font);
        if done(app) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn opens_a_document_and_draws_it() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium is not installed");
        return;
    }
    let dir = TempDir::new("viewer-open");
    let path = build_pdf(&dir, 3);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(1000.0, 780.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(1000, 780);

    let ready = pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    });
    assert!(ready, "document never became ready: {:?}", app.active_doc().map(|d| d.error.clone()));

    // Wait for the first page bitmap.
    let rendered = pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc()
            .map(|d| d.cache.len() > 0)
            .unwrap_or(false)
    });
    assert!(rendered, "no page was rendered");

    // The toolbar and status bar must have been painted with the theme colours.
    let theme = pdf_rust::ui::Theme::from_dark(false);
    assert_eq!(bmp.get(4, 8), theme.toolbar_bg, "toolbar background");
    assert_eq!(
        bmp.get(4, bmp.height as u32 - 4),
        theme.status_bg,
        "status bar background"
    );
    // Some ink must exist on the page area (the page has text).
    let mut dark = 0;
    let content = app.content_rect();
    for y in (content.y as u32 + 20)..(content.bottom() as u32 - 20) {
        for x in (content.x as u32 + 20)..(content.right() as u32 - 20) {
            let c = bmp.get(x, y);
            if c.r() < 120 && c.g() < 120 && c.b() < 120 {
                dark += 1;
            }
        }
    }
    assert!(dark > 200, "page content should contain text pixels, found {dark}");

    let doc = app.active_doc().unwrap();
    assert_eq!(doc.page_count(), 3);
    assert_eq!(doc.title, "viewer.pdf");
    assert!(app.perf.startup_ms > 0.0);
}

#[test]
fn keyboard_navigation_updates_the_reading_position() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-nav");
    let path = build_pdf(&dir, 12);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(900.0, 700.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));

    // Go to page 7 with Ctrl+G then typing.
    let mut input = press(Key::Char('g'), true, false);
    app.frame(&input, &mut bmp, &f);
    assert!(app.dialogs.goto.is_some(), "Ctrl+G opens the go-to dialog");
    input = Input {
        text: "7".into(),
        ..Default::default()
    };
    input.keys.push(KeyEvent { key: Key::Enter, repeat: false });
    app.frame(&input, &mut bmp, &f);
    assert!(app.dialogs.goto.is_none(), "dialog closes after Enter");
    assert_eq!(app.active_doc().unwrap().current_page(), 6);

    // Page down moves forward one page.
    app.frame(&press(Key::PageDown, false, false), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().current_page(), 7);
    app.frame(&press(Key::End, false, false), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().current_page(), 11);
    app.frame(&press(Key::Home, false, false), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().current_page(), 0);
    assert_eq!(app.active_doc().unwrap().session.last_page, 1);
}

#[test]
fn zoom_and_rotation_are_commands_with_visible_effect() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-zoom");
    let path = build_pdf(&dir, 4);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(900.0, 700.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));
    let page = app.active_doc().unwrap().current_page();
    app.frame(&press(Key::Char('2'), true, false), &mut bmp, &f);
    assert!((app.active_doc().unwrap().layout.zoom - 1.0).abs() < 1e-3, "Ctrl+2 = actual size");
    app.frame(&press(Key::Char('+'), true, false), &mut bmp, &f);
    assert!(app.active_doc().unwrap().layout.zoom > 1.0);
    assert_eq!(app.active_doc().unwrap().current_page(), page, "zoom keeps the page");

    let width_before = app.active_doc().unwrap().layout.pages[page as usize].w;
    app.frame(&press(Key::Char('r'), true, false), &mut bmp, &f);
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.session.rotation, 90);
    let width_after = doc.layout.pages[page as usize].w;
    assert!(width_after > width_before, "landscape page is wider");

    app.frame(&press(Key::Char('0'), true, false), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().session.zoom_mode, ZoomMode::FitPage);
}

#[test]
fn reading_position_survives_restart() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-persist");
    let path = build_pdf(&dir, 20);
    let db_path = dir.join("library.db");
    let f = font();
    let mut bmp = Bitmap::new(900.0 as u32, 700);

    {
        let mut app = app_with_db(&db_path);
        app.window = Size::new(900.0, 700.0);
        app.settings.resume_policy = pdf_rust::app::dialogs::ResumePolicy::Always;
        app.open_path(path.clone(), None);
        assert!(pump(&mut app, &mut bmp, &f, |app| {
            app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
        }));
        app.frame(&press(Key::End, false, false), &mut bmp, &f);
        assert_eq!(app.active_doc().unwrap().current_page(), 19);
        app.save_all();
        // The database is keyed by the path the app records (absolute, and without
        // the Windows verbatim prefix), never by the string that was passed in.
        let stored = app.active_doc().unwrap().identity.path_string();
        assert!(!stored.starts_with("\\\\?\\"), "no verbatim prefix in {stored}");
        assert!(std::path::Path::new(&stored).is_absolute());
        let row = app.db.as_ref().unwrap().find_by_path(&stored).unwrap();
        assert!(row.is_some(), "the document must be recorded as {stored}");
        assert_eq!(row.unwrap().session.last_page, 20);
    }

    // A brand new process would look like this: fresh engine, same database.
    let mut app = app_with_db(&db_path);
    app.window = Size::new(900.0, 700.0);
    app.settings.resume_policy = pdf_rust::app::dialogs::ResumePolicy::Always;
    app.open_path(path, None);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.session.last_page, 20, "restored page");
    assert_eq!(doc.current_page(), 19, "restored scroll position");
    assert_eq!(app.recent.len(), 1);
    assert!(doc.row_id.is_some());
}

#[test]
fn moving_a_file_keeps_its_reading_position() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-move");
    let path = build_pdf(&dir, 8);
    let db_path = dir.join("library.db");
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    {
        let mut app = app_with_db(&db_path);
        app.window = Size::new(900.0, 700.0);
        app.settings.resume_policy = pdf_rust::app::dialogs::ResumePolicy::Always;
        app.open_path(path.clone(), None);
        assert!(pump(&mut app, &mut bmp, &f, |app| {
            app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
        }));
        app.frame(&press(Key::End, false, false), &mut bmp, &f);
        app.save_all();
    }
    let moved = dir.join("moved-elsewhere.pdf");
    std::fs::rename(&path, &moved).unwrap();
    let mut app = app_with_db(&db_path);
    app.window = Size::new(900.0, 700.0);
    app.settings.resume_policy = pdf_rust::app::dialogs::ResumePolicy::Always;
    app.open_path(moved, None);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));
    assert_eq!(app.active_doc().unwrap().session.last_page, 8, "fingerprint matched");
    assert_eq!(app.recent.len(), 1, "no duplicate entry after the move");
}

#[test]
fn resume_prompt_can_start_over() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-resume");
    let path = build_pdf(&dir, 10);
    let db_path = dir.join("library.db");
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    {
        let mut app = app_with_db(&db_path);
        app.window = Size::new(900.0, 700.0);
        app.settings.resume_policy = pdf_rust::app::dialogs::ResumePolicy::Always;
        app.open_path(path.clone(), None);
        assert!(pump(&mut app, &mut bmp, &f, |app| {
            app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
        }));
        app.frame(&press(Key::End, false, false), &mut bmp, &f);
        app.save_all();
    }
    let mut app = app_with_db(&db_path);
    app.window = Size::new(900.0, 700.0);
    app.settings.resume_policy = pdf_rust::app::dialogs::ResumePolicy::Ask;
    app.open_path(path, None);
    assert!(pump(&mut app, &mut bmp, &f, |app| app.dialogs.resume.is_some()));
    // Clicking "Start from the beginning" is the same action the button emits.
    app.apply_action(Action::ResumeAnswer { resume: false, remember: false });
    app.frame(&input_none(), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().current_page(), 0);
}

#[test]
fn tabs_and_single_page_mode() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-tabs");
    let a = build_pdf(&dir, 3);
    let b = {
        let pages: Vec<TestPage> = (0..2)
            .map(|i| TestPage::a4(0).with_text(&[&format!("Other {}", i + 1)]))
            .collect();
        PdfBuilder::new(pages).write_to(&dir.join("second.pdf"))
    };
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(1000.0, 800.0);
    app.open_path(a, None);
    app.open_path(b, None);
    let f = font();
    let mut bmp = Bitmap::new(1000, 800);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.docs.len() == 2 && app.docs.iter().all(|d| d.is_ready())
    }));
    assert_eq!(app.active, 1);
    let regions = app.regions();
    assert!(regions.tabbar.h > 0.0, "two documents show a tab bar");

    app.frame(&press(Key::Tab, true, false), &mut bmp, &f);
    assert_eq!(app.active, 0, "Ctrl+Tab cycles documents");

    app.frame(&press(Key::Char('m'), true, false), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().session.reading_mode, ReadingMode::Single);
    assert_eq!(app.active_doc().unwrap().layout.pages.len(), 1);
    app.frame(&press(Key::Char('m'), true, false), &mut bmp, &f);
    assert_eq!(app.active_doc().unwrap().session.reading_mode, ReadingMode::Continuous);
    assert_eq!(app.active_doc().unwrap().layout.pages.len(), 3);

    app.frame(&press(Key::Char('w'), true, false), &mut bmp, &f);
    assert_eq!(app.docs.len(), 1, "Ctrl+W closes the active document");
}

#[test]
fn clicking_the_page_starts_a_selection() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-select");
    let pages = vec![TestPage::a4(0).with_text(&["Hello selectable world", "second line of text"])];
    let path = PdfBuilder::new(pages).write_to(&dir.join("text.pdf"));
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(900.0, 760.0);
    app.settings.sidebar_visible = false;
    app.show_sidebar = false;
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(900, 760);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc()
            .map(|d| d.is_ready() && d.texts.contains_key(&0))
            .unwrap_or(false)
    }));
    // Find a line of text on the page and drag across it.
    let content = app.content_rect();
    let (text, geom, zoom, page_h_pt, scroll) = {
        let doc = app.active_doc().unwrap();
        (
            doc.text(0).unwrap().clone(),
            doc.layout.pages[0],
            doc.layout.zoom,
            doc.info().unwrap().display_size(0, 0).h,
            doc.layout.scroll,
        )
    };
    assert!(!text.chars.is_empty(), "the page must have extractable text");
    let first = text.chars[0];
    let to_screen = |x: f32, y: f32| {
        Point::new(
            content.x + geom.x + x * zoom,
            content.y + geom.y - scroll + (page_h_pt - y) * zoom,
        )
    };
    let start = to_screen(first.left + 1.0, (first.bottom + first.top) * 0.5);
    let mut down = Input {
        mouse: start,
        down: true,
        pressed: true,
        ..Default::default()
    };
    down.time = app.now();
    app.frame(&down, &mut bmp, &f);
    let last = text.chars.last().unwrap();
    let end = to_screen(last.right - 1.0, (last.bottom + last.top) * 0.5);
    let mut drag = Input {
        mouse: end,
        down: true,
        ..Default::default()
    };
    drag.time = app.now();
    app.frame(&drag, &mut bmp, &f);
    let mut up = Input {
        mouse: end,
        down: false,
        released: true,
        ..Default::default()
    };
    up.time = app.now();
    app.frame(&up, &mut bmp, &f);
    let selection = app.active_doc().unwrap().selection;
    assert!(selection.is_some(), "dragging selects text");
    app.apply_action(Action::CopySelection);
    let copied = app.clipboard_out.clone().unwrap_or_default();
    assert!(copied.contains("Hello"), "selection copies the text: {copied:?}");
}

#[test]
fn search_finds_text_across_pages() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-search");
    let pages: Vec<TestPage> = (0..6)
        .map(|i| {
            let line = if i == 4 { "needle in a haystack" } else { "nothing here" };
            TestPage::a4(0).with_text(&[line])
        })
        .collect();
    let path = PdfBuilder::new(pages).write_to(&dir.join("search.pdf"));
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(900.0, 700.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));
    app.search_start("needle".to_string());
    let found = pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| !d.search.hits.is_empty()).unwrap_or(false)
    });
    assert!(found, "the search should find the needle");
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.search.hits.len(), 1);
    assert_eq!(doc.search.hits[0].page, 4);
    assert_eq!(doc.current_page(), 4, "jumps to the matching page");

    // A query that does not exist terminates with "no more pages".
    app.search_start("absent".to_string());
    let done = pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc()
            .map(|d| d.search.hits.is_empty() && d.search.no_more_pages)
            .unwrap_or(false)
    });
    assert!(done, "an unsuccessful search must finish");
}

#[test]
fn highlight_round_trips_through_the_database() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-highlight");
    let pages = vec![TestPage::a4(0).with_text(&["Highlight this sentence please"])];
    let path = PdfBuilder::new(pages).write_to(&dir.join("hl.pdf"));
    let db_path = dir.join("library.db");
    let mut app = app_with_db(&db_path);
    app.window = Size::new(900.0, 760.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(900, 760);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc()
            .map(|d| d.is_ready() && d.texts.contains_key(&0) && d.row_id.is_some())
            .unwrap_or(false)
    }));
    let index = app.active;
    let count = app.docs[index].text(0).unwrap().chars.len();
    app.docs[index].selection = Some(pdf_rust::app::docview::Selection {
        page: 0,
        anchor: 0,
        focus: count.min(9),
        dragging: false,
    });
    app.add_highlight();
    assert_eq!(app.docs[index].highlights.len(), 1);
    let doc_row = app.docs[index].row_id.unwrap();
    let stored = app.db.as_ref().unwrap().highlights(doc_row).unwrap();
    assert_eq!(stored.len(), 1);
    assert!(!stored[0].rects.is_empty());
    let id = stored[0].id;
    app.delete_highlight(id);
    assert!(app.docs[index].highlights.is_empty());
}

#[test]
fn no_document_shows_the_welcome_screen() {
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(800.0, 600.0);
    let f = font();
    let mut bmp = Bitmap::new(800, 600);
    app.frame(&input_none(), &mut bmp, &f);
    // The welcome screen paints the product name; look for ink in the content area.
    let content = app.content_rect();
    let mut ink = 0;
    for y in content.y as u32..content.bottom() as u32 {
        for x in content.x as u32..content.right() as u32 {
            let c = bmp.get(x, y);
            if c != bmp.get(content.x as u32, content.y as u32) {
                ink += 1;
            }
        }
    }
    assert!(ink > 100, "the welcome screen should draw something, found {ink}");
    assert!(app.docs.is_empty());
    assert!(app.active_doc().is_none());
}

#[test]
fn missing_pdf_reports_an_error_and_keeps_running() {
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(700.0, 560.0);
    app.open_path(std::path::PathBuf::from("/definitely/not/here.pdf"), None);
    assert!(app.docs.is_empty());
    assert!(app.dialogs.error.is_some());
    let f = font();
    let mut bmp = Bitmap::new(700, 560);
    app.frame(&input_none(), &mut bmp, &f);
    assert!(!app.quit);
    app.frame(&press(Key::Escape, false, false), &mut bmp, &f);
    assert!(app.dialogs.error.is_none(), "Esc dismisses the dialog");
}

#[test]
fn open_dialog_lists_pdfs() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-dialog");
    let path = build_pdf(&dir, 2);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(900.0, 700.0);
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    app.handle_command(Command::OpenDialog);
    assert!(app.dialogs.open.is_some());
    let dialog = app.dialogs.open.as_mut().unwrap();
    dialog.navigate_to(path.parent().unwrap());
    let entry = dialog
        .entries
        .iter()
        .position(|e| e.path == path)
        .expect("the PDF is listed");
    dialog.selected = Some(entry);
    app.frame(&input_none(), &mut bmp, &f);
    // "Open" is a real button: activate the same path through the action channel.
    let chosen = app.dialogs.open.as_mut().unwrap().activate_selected();
    assert_eq!(chosen.as_deref(), Some(path.as_path()));
    app.apply_action(Action::OpenPath(chosen.unwrap()));
    assert!(app.dialogs.open.is_none() || app.docs.len() == 1);
    assert_eq!(app.docs.len(), 1);
}

#[test]
fn theme_toggle_changes_the_frame_background() {
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(600.0, 480.0);
    let f = font();
    let mut bmp = Bitmap::new(600, 480);
    app.frame(&input_none(), &mut bmp, &f);
    let light = bmp.get(2, 2);
    app.frame(&press(Key::Char('d'), true, false), &mut bmp, &f);
    assert!(app.settings.dark);
    let dark = bmp.get(2, 2);
    assert_ne!(light, dark, "Ctrl+D repaints with the dark theme");
    let theme = pdf_rust::ui::Theme::from_dark(true);
    assert_eq!(dark, theme.toolbar_bg);
}

#[test]
fn corrupt_pdf_is_reported_as_a_failed_document() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-corrupt");
    let path = dir.join("broken.pdf");
    std::fs::write(&path, b"this is not a PDF at all, just bytes").unwrap();
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(800.0, 600.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(800, 600);
    let failed = pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc()
            .map(|d| matches!(d.state, DocState::Failed(_)))
            .unwrap_or(false)
    });
    assert!(failed, "a corrupt file must end in the Failed state");
    assert!(app.dialogs.error.is_some());
}

/// Reading for a long time must not grow memory: the page cache is bounded by
/// bytes and evicts what is no longer visible.
#[test]
fn extended_reading_keeps_the_cache_bounded() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-memory");
    let path = build_pdf(&dir, 30);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(1200.0, 900.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(1200, 900);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));
    // Zoom in so each page is a few megabytes: three of them already exceed the
    // budget the test pins the cache to.
    {
        let viewport = app.content_rect().size();
        let doc = app.active_doc_mut().unwrap();
        doc.set_zoom(ZoomMode::Custom, 1.5, viewport);
    }
    app.active_doc_mut().unwrap().cache.set_limit(8 * 1024 * 1024);

    let mut rendered = 0;
    for _ in 0..30 {
        app.frame(&press(Key::PageDown, false, false), &mut bmp, &f);
        for _ in 0..6 {
            app.tick(app.now());
            app.frame(&input_none(), &mut bmp, &f);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let doc = app.active_doc().unwrap();
        rendered = rendered.max(doc.cache.len());
        assert!(
            doc.cache.memory_bytes() <= doc.cache.limit_bytes(),
            "cache grew to {} bytes with a {} byte budget",
            doc.cache.memory_bytes(),
            doc.cache.limit_bytes()
        );
    }
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.current_page(), 29, "read to the last page");
    assert!(!doc.cache.is_empty(), "the visible page stays cached");
    assert!(
        rendered < 30,
        "old pages were evicted instead of piling up ({rendered} cached at once)"
    );
    // Going back re-renders instead of failing.
    app.frame(&press(Key::Home, false, false), &mut bmp, &f);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| !d.cache.is_empty()).unwrap_or(false)
    }));
    assert_eq!(app.active_doc().unwrap().current_page(), 0);
}

/// Closing the app must remember the tabs (order and which one was active), the
/// sidebar selection and its width, and a crash must be noticed on the next start.
#[test]
fn the_whole_session_comes_back_after_a_restart() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-session");
    let db_path = dir.join("library.db");
    let font = font();
    let mut bmp = Bitmap::new(1100, 820);
    let mut names = Vec::new();
    for name in ["alpha.pdf", "beta.pdf", "gamma.pdf"] {
        let pages: Vec<TestPage> = (0..3)
            .map(|i| TestPage::a4(0).with_text(&[&format!("{} page {}", name, i + 1)]))
            .collect();
        names.push(PdfBuilder::new(pages).write_to(&dir.join(name)));
    }

    {
        let mut app = app_with_db(&db_path);
        app.window = Size::new(1100.0, 820.0);
        for path in &names {
            app.open_path(path.clone(), None);
        }
        assert!(pump(&mut app, &mut bmp, &font, |a| a.docs.len() == 3
            && a.docs.iter().all(|d| d.is_ready())));
        // Pretend the user picked the second tab, the outline panel and a wider
        // sidebar, then closed the window properly.
        app.apply_action(Action::ActivateDoc(1));
        app.apply_action(Action::SetSidebarTab(
            pdf_rust::app::SidebarTab::Outline,
        ));
        app.apply_action(Action::SetSidebarWidth(320.0));
        app.save_all();
        assert!(!app.recovered_session);
    }

    // A fresh process sees the same tabs in the same order with the same tab
    // selected, and the sidebar the user left behind.
    {
        let mut app = app_with_db(&db_path);
        app.window = Size::new(1100.0, 820.0);
        assert!(!app.recovered_session, "the last run shut down cleanly");
        assert_eq!(app.restore_session(), 3, "every tab comes back");
        assert!(pump(&mut app, &mut bmp, &font, |a| {
            a.docs.len() == 3 && a.docs.iter().all(|d| d.is_ready())
        }));
        let titles: Vec<&str> = app.docs.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, ["alpha.pdf", "beta.pdf", "gamma.pdf"], "tab order is kept");
        assert_eq!(app.active, 1, "the active tab is kept");
        assert_eq!(app.sidebar.tab, pdf_rust::app::SidebarTab::Outline);
        assert!((app.settings.sidebar_width - 320.0).abs() < 0.5);
        assert!((app.window.w - 1100.0).abs() < 0.5);
        // Simulate an unexpected exit: only `save_all` marks the session clean,
        // so simply dropping the app leaves the flag set to "crashed".
        drop(app);
    }

    // The next start knows the previous run did not shut down cleanly …
    let mut previous = app_with_db(&db_path);
    assert!(previous.recovered_session, "a crash is detected on the next start");
    // … and the tabs are still there (they were saved as they changed).
    assert_eq!(previous.restore_session(), 3);
    // Close every handle before touching the files: Windows keeps a document
    // locked while it is open, so the file can only be removed afterwards.
    drop(previous);
    // A document that has disappeared is skipped instead of failing the restore.
    std::fs::remove_file(&names[1]).unwrap();
    let mut app = app_with_db(&db_path);
    assert_eq!(app.restore_session(), 2, "the missing document is skipped");
    let titles: Vec<&str> = app.docs.iter().map(|d| d.title.as_str()).collect();
    assert_eq!(titles, ["alpha.pdf", "gamma.pdf"]);
}

/// The page cache is shared by every open document: reading two documents must
/// not double the memory footprint, and the background document gives up its
/// pages before the one on screen does.
#[test]
fn one_global_budget_covers_every_document() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-global-memory");
    let a = build_pdf(&dir, 12);
    let b = {
        let pages: Vec<TestPage> = (0..12)
            .map(|i| TestPage::a4(0).with_text(&[&format!("Second document {}", i + 1)]))
            .collect();
        PdfBuilder::new(pages).write_to(&dir.join("second.pdf"))
    };
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(1200.0, 900.0);
    app.open_path(a, None);
    app.open_path(b, None);
    let f = font();
    let mut bmp = Bitmap::new(1200, 900);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.docs.len() == 2 && app.docs.iter().all(|d| d.is_ready())
    }));
    // Zoom both documents so a handful of pages costs tens of megabytes.
    let viewport = app.content_rect().size();
    for doc in &mut app.docs {
        doc.set_zoom(ZoomMode::Custom, 2.5, viewport);
    }
    // The smallest budget the app offers is still far too small for both
    // documents at this zoom level.
    let budget = pdf_rust::app::CACHE_BUDGET_MIN;
    app.set_cache_budget(budget);

    // Read through the first document, then swap and read the second.
    for _ in 0..10 {
        app.frame(&press(Key::PageDown, false, false), &mut bmp, &f);
        app.tick(app.now());
        app.frame(&input_none(), &mut bmp, &f);
    }
    app.frame(&press(Key::Tab, true, false), &mut bmp, &f);
    assert_eq!(app.active, 0, "the first document is active again");
    for _ in 0..10 {
        app.frame(&press(Key::PageDown, false, false), &mut bmp, &f);
        app.tick(app.now());
        app.frame(&input_none(), &mut bmp, &f);
        // Rows are ordered largest first, so find the active one by flag.
        let stats = app.memory_stats();
        assert_eq!(stats.documents.len(), 2);
        assert_eq!(stats.documents.iter().filter(|d| d.active).count(), 1);
        assert!(stats.documents.iter().any(|d| d.active && d.pages > 0));
    }

    // The policy has to be exercised deterministically: rendering is
    // asynchronous, so fill both caches by hand and then let the app enforce the
    // global budget. 40 pages of 4 MB each is 160 MB in a 64 MiB budget.
    for doc in &mut app.docs {
        doc.cache.clear();
    }
    for doc in app.docs.iter_mut() {
        for page in 0..20u32 {
            let key = pdf_rust::pdf::RenderKey::new(page, 1.0, 0);
            doc.cache.insert(key, Arc::new(Bitmap::new(1000, 1000)));
        }
    }
    let before = app.memory_stats();
    assert!(before.used > budget, "the test needs to start over budget");
    app.enforce_cache_budget();
    let stats = app.memory_stats();
    assert_eq!(stats.budget, budget);
    assert!(
        stats.used <= budget,
        "{} bytes cached with a {} byte global budget",
        stats.used,
        budget
    );
    assert!(stats.pages > 0, "the visible page is still cached");
    assert!(stats.evictions > 0, "pages were evicted to stay inside it");
    // The background document was trimmed first: only the page it was left on
    // and the most recent one can survive (`trim_to` protects both).
    assert!(
        app.docs[1].cache.len() <= 2,
        "background document kept {} pages",
        app.docs[1].cache.len()
    );
    assert!(
        app.docs[1].cache.memory_bytes() <= 2 * 4 * 1024 * 1024,
        "background document kept {} bytes",
        app.docs[1].cache.memory_bytes()
    );
    // The page the user is looking at is never thrown away.
    assert!(!app.docs[0].cache.is_empty());
    assert!(
        app.docs[0].cache.len() >= app.docs[1].cache.len(),
        "active kept {} pages, background {}",
        app.docs[0].cache.len(),
        app.docs[1].cache.len()
    );
    // Switching back to the background document still renders.
    app.frame(&press(Key::Tab, true, false), &mut bmp, &f);
    app.docs[1].cache.clear();
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active == 1 && !app.docs[1].cache.is_empty()
    }));
    // Raising the budget again is possible at runtime.
    app.set_cache_budget(512 * 1024 * 1024);
    assert_eq!(app.memory_stats().pressure, pdf_rust::app::MemoryPressure::Ok);
}

/// The "smooth scrolling" preference must actually change behaviour: with it on
/// a wheel step eases towards the target over several frames, with it off the
/// offset jumps immediately.
#[test]
fn smooth_scrolling_eases_towards_the_target() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-smooth");
    let path = build_pdf(&dir, 6);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(900.0, 700.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(900, 700);
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));

    let mut wheel = input_none();
    wheel.wheel = -1.0; // scroll down
    wheel.mouse = Point::new(500.0, 400.0);

    // Smooth: the wheel only records the target, the following frames ease
    // towards it, and the motion is monotonic.
    app.settings.smooth_scroll = true;
    let before = app.active_doc().unwrap().scroll;
    app.frame(&wheel, &mut bmp, &f);
    assert!(app.active_doc().unwrap().is_scrolling(), "a smooth scroll is queued");
    let mut last = app.active_doc().unwrap().scroll;
    let mut steps = 0;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        app.tick(app.now());
        app.frame(&input_none(), &mut bmp, &f);
        let now = app.active_doc().unwrap().scroll;
        assert!(now >= last - 0.001, "the offset never jumps backwards");
        if now > last + 0.01 {
            steps += 1;
        }
        last = now;
    }
    assert!(steps >= 3, "the motion is spread over several frames (got {steps})");
    assert!(!app.active_doc().unwrap().is_scrolling(), "the animation settles");
    let travelled = last - before;
    assert!(
        (travelled - 60.0).abs() < 2.0,
        "the wheel step lands 60 px down (moved {travelled:.1})"
    );

    // Without smoothing the same wheel step lands immediately.
    app.settings.smooth_scroll = false;
    let before = app.active_doc().unwrap().scroll;
    app.frame(&wheel, &mut bmp, &f);
    let after = app.active_doc().unwrap().scroll;
    assert!(!app.active_doc().unwrap().is_scrolling());
    assert!(after - before > 30.0, "an unsmoothed wheel step is ~60 px");
}

// ---------------------------------------------------------------- reported bugs

fn click_at(app: &mut App, bmp: &mut Bitmap, f: &Font, at: Point) {
    let mut down = input_none();
    down.mouse = at;
    down.down = true;
    down.pressed = true;
    app.frame(&down, bmp, f);
    let mut up = input_none();
    up.mouse = at;
    up.down = false;
    up.released = true;
    app.frame(&up, bmp, f);
}

/// Settings dialog geometry, straight from the painter's layout.
fn settings_layout(window: Rect) -> pdf_rust::ui::chrome::SettingsLayout {
    pdf_rust::ui::chrome::settings_layout(window)
}

/// Every control in the Settings dialog must work and survive a restart.
#[test]
fn settings_dialog_changes_and_saves_preferences() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-settings");
    let db_path = dir.join("library.db");
    let font = font();
    let mut bmp = Bitmap::new(1000, 800);
    let window = Rect::new(0.0, 0.0, 1000.0, 800.0);
    let layout = settings_layout(window);
    let (dark, smooth, done) = (layout.dark, layout.smooth, layout.done);
    let policies = layout.policies;

    {
        let mut app = app_with_db(&db_path);
        app.window = Size::new(window.w, window.h);
        app.dialogs.settings = true;
        let was_dark = app.settings.dark;
        let was_smooth = app.settings.smooth_scroll;

        app.frame(&input_none(), &mut bmp, &font);
        click_at(&mut app, &mut bmp, &font, dark.center());
        eprintln!("DEBUG after dark: dark={} smooth={}", app.settings.dark, app.settings.smooth_scroll);
        assert_eq!(app.settings.dark, !was_dark, "dark theme checkbox toggles the theme");
        assert!(app.dialogs.settings, "the dialog stays open until Done");

        app.frame(&input_none(), &mut bmp, &font);
        eprintln!("DEBUG smooth rect {:?} settings {:?}", smooth, app.settings.smooth_scroll);
        click_at(&mut app, &mut bmp, &font, smooth.center());
        assert_eq!(
            app.settings.smooth_scroll, !was_smooth,
            "smooth scrolling checkbox toggles"
        );

        app.frame(&input_none(), &mut bmp, &font);
        click_at(&mut app, &mut bmp, &font, policies[1].center());
        assert_eq!(
            app.settings.resume_policy,
            pdf_rust::app::dialogs::ResumePolicy::Always,
            "resume policy buttons select a policy"
        );

        // The smallest cache budget offered must apply to the app right away.
        app.frame(&input_none(), &mut bmp, &font);
        click_at(&mut app, &mut bmp, &font, layout.budget_buttons[0].center());
        assert_eq!(
            app.cache_budget,
            pdf_rust::app::CACHE_BUDGET_MIN,
            "a budget button updates the live cache budget"
        );
        assert_eq!(app.settings.cache_mb, pdf_rust::app::CACHE_BUDGET_MIN / (1024 * 1024));

        app.frame(&input_none(), &mut bmp, &font);
        let restore = app.settings.restore_session;
        click_at(&mut app, &mut bmp, &font, layout.restore.center());
        assert_eq!(
            app.settings.restore_session, !restore,
            "the session checkbox toggles"
        );

        app.frame(&input_none(), &mut bmp, &font);
        click_at(&mut app, &mut bmp, &font, layout.colors[2].center());
        assert_eq!(app.highlight_color, 0xFF9A_D0F5, "colour swatches select a colour");

        app.frame(&input_none(), &mut bmp, &font);
        click_at(&mut app, &mut bmp, &font, done.center());
        assert!(!app.dialogs.settings, "Done closes the dialog");
    }

    let db = Database::open(&db_path).expect("database");
    db.migrate().expect("migration");
    let saved = pdf_rust::app::Settings::load(&db);
    assert_eq!(saved.dark, !pdf_rust::app::Settings::default().dark, "theme persisted");
    assert_eq!(
        saved.resume_policy,
        pdf_rust::app::dialogs::ResumePolicy::Always,
        "resume policy persisted"
    );
    assert_eq!(
        saved.cache_mb,
        pdf_rust::app::CACHE_BUDGET_MIN / (1024 * 1024),
        "cache budget persisted"
    );
    assert!(!saved.restore_session, "session restore preference persisted");
    assert_eq!(saved.highlight_color, 0xFF9A_D0F5, "highlight colour persisted");
}

/// Tab and Shift+Tab must walk the controls on screen, so the app is usable
/// without a mouse, and Enter must press whatever has the focus.
#[test]
fn keyboard_focus_visits_every_control_and_can_activate_one() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-focus");
    let path = build_pdf(&dir, 2);
    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(1100.0, 800.0);
    app.open_path(path, None);
    let f = font();
    let mut bmp = Bitmap::new(1100, 800);
    assert!(pump(&mut app, &mut bmp, &f, |a| {
        a.active_doc().map(|d| d.is_ready()).unwrap_or(false)
    }));
    assert_eq!(app.ui.focus, None, "nothing is focused after opening");
    app.frame(&input_none(), &mut bmp, &f);

    let tab = |app: &mut App, bmp: &mut Bitmap, f: &Font, shift: bool| {
        app.frame(&press(Key::Tab, false, shift), bmp, f);
        app.frame(&input_none(), bmp, f);
        app.ui.focus
    };
    let first = tab(&mut app, &mut bmp, &f, false);
    assert!(first.is_some(), "Tab focuses the first control");
    let mut seen = vec![first];
    for _ in 0..12 {
        let next = tab(&mut app, &mut bmp, &f, false);
        assert!(next.is_some());
        if seen.last() != Some(&next) {
            seen.push(next);
        }
    }
    assert!(seen.len() >= 3, "several controls are reachable ({seen:?})");
    // Shift+Tab steps back to the control that was focused before.
    let before = app.ui.focus;
    let forward = tab(&mut app, &mut bmp, &f, false);
    assert_ne!(forward, before, "Tab moved on");
    let back = tab(&mut app, &mut bmp, &f, true);
    assert_eq!(back, before, "Shift+Tab returns to the previous control");
    // Enter presses the focused control: with the settings dialog open, park the
    // focus on the theme checkbox and press Enter.
    app.dialogs.settings = true;
    app.frame(&input_none(), &mut bmp, &f);
    app.ui.focus = Some(pdf_rust::ui::id("set-dark"));
    app.frame(&input_none(), &mut bmp, &f);
    let dark = app.settings.dark;
    app.frame(&press(Key::Enter, false, false), &mut bmp, &f);
    assert_eq!(app.settings.dark, !dark, "Enter presses the focused control");
    app.dialogs.settings = false;
    app.frame(&input_none(), &mut bmp, &f);

    // Ctrl+Tab still switches documents instead of moving focus.
    app.open_path(build_pdf(&dir, 1), None);
    app.frame(&press(Key::Tab, true, false), &mut bmp, &f);
    assert_eq!(app.active, 0, "Ctrl+Tab keeps switching documents");
}

/// A password-protected document must prompt, keep prompting after a wrong
/// password (without turning it into a fatal error dialog) and open once the
/// right one is typed. Cancelling must not leave a dead tab behind.
#[test]
fn password_protected_documents_prompt_and_retry() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-password");
    let mut builder = PdfBuilder::new(vec![
        TestPage::a4(0).with_text(&["Locked page"]),
        TestPage::a4(0).with_text(&["Second locked page"]),
    ]);
    builder.encrypt = Some(("s3cret".into(), "owner".into()));
    let path = builder.write_to(&dir.join("locked.pdf"));

    let mut app = App::new(Engine::start(), None, None);
    app.window = Size::new(1000.0, 800.0);
    app.open_path(path.clone(), None);
    let f = font();
    let mut bmp = Bitmap::new(1000, 800);
    assert!(pump(&mut app, &mut bmp, &f, |app| app.dialogs.password.is_some()));
    let dialog = app.dialogs.password.as_ref().unwrap();
    assert_eq!(dialog.document, "locked.pdf");
    assert!(dialog.error.is_none(), "no error on the first prompt");
    assert!(app.dialogs.error.is_none(), "a password is not a fatal error");

    // Wrong password: the dialog comes back with an explanation.
    app.apply_action(Action::PasswordSubmit("nope".into()));
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.dialogs.password.as_ref().map(|d| d.error.is_some()).unwrap_or(false)
    }), "a rejected password reopens the prompt");
    assert!(app.dialogs.error.is_none(), "still not a fatal error");
    assert_eq!(app.docs.len(), 1, "the document keeps its tab");

    // Correct password: the document opens.
    app.apply_action(Action::PasswordSubmit("s3cret".into()));
    assert!(pump(&mut app, &mut bmp, &f, |app| {
        app.docs.first().map(|d| d.is_ready()).unwrap_or(false)
    }), "the right password opens the document");
    assert!(app.dialogs.password.is_none());
    app.tick(app.now());
    app.frame(&input_none(), &mut bmp, &f);

    // Cancelling the prompt drops the tab instead of leaving it unusable.
    app.open_path(dir.join("locked.pdf"), None);
    let second = PdfBuilder::new(vec![TestPage::a4(0).with_text(&["Another"])]);
    let mut second = second;
    second.encrypt = Some(("other".into(), "owner".into()));
    let other = second.write_to(&dir.join("other-locked.pdf"));
    app.open_path(other, None);
    assert!(pump(&mut app, &mut bmp, &f, |app| app.dialogs.password.is_some()));
    let index = app.dialogs.password.as_ref().unwrap().doc_index;
    let before = app.docs.len();
    app.apply_action(Action::PasswordCancel(index));
    assert!(app.dialogs.password.is_none());
    assert_eq!(app.docs.len(), before - 1, "cancelling closes the tab");
    assert_eq!(app.active_doc().map(|d| d.title.as_str()), Some("locked.pdf"));
}

/// Closing one tab must leave the other documents open.
#[test]
fn closing_one_document_keeps_the_others_open() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-close");
    let font = font();
    let mut bmp = Bitmap::new(1000, 800);
    let mut app = app_with_db(&dir.join("library.db"));
    app.window = Size::new(1000.0, 800.0);
    for name in ["a.pdf", "b.pdf", "c.pdf"] {
        let pages: Vec<TestPage> = (0..3)
            .map(|i| TestPage::a4(0).with_text(&[&format!("{} page {}", name, i + 1)]))
            .collect();
        let path = PdfBuilder::new(pages).write_to(&dir.join(name));
        app.open_path(path, None);
    }
    assert!(pump(&mut app, &mut bmp, &font, |a| a.docs.iter().all(|d| d.is_ready())));
    app.active = 1;
    app.frame(&input_none(), &mut bmp, &font);

    // The close button sits at the right edge of each tab.
    let tabbar = app.regions().tabbar;
    assert!(tabbar.h > 0.0, "two or more documents show a tab bar");
    let title_w = font.text_width(&app.docs[0].title, pdf_rust::ui::Metrics::TEXT_SIZE) + 62.0;
    let tab_w = title_w.clamp(96.0, 240.0);
    let close = Point::new(6.0 + tab_w - 22.0 + 8.0, tabbar.y + 3.0 + (tabbar.h - 4.0) * 0.5);
    click_at(&mut app, &mut bmp, &font, close);

    assert_eq!(app.docs.len(), 2, "only the clicked document closes");
    assert_eq!(app.docs[0].title, "b.pdf");
    assert_eq!(app.docs[1].title, "c.pdf");
    assert_eq!(app.active, 0, "the neighbour becomes active");
}

/// Clicking a thumbnail jumps to that page.
#[test]
fn clicking_a_thumbnail_navigates_to_the_page() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-thumbs");
    let path = build_pdf(&dir, 12);
    let font = font();
    let mut bmp = Bitmap::new(1400, 900);
    let mut app = app_with_db(&dir.join("library.db"));
    app.window = Size::new(1400.0, 900.0);
    app.show_sidebar = true;
    app.sidebar.tab = pdf_rust::app::SidebarTab::Thumbnails;
    app.open_path(path, None);
    assert!(pump(&mut app, &mut bmp, &font, |a| a.docs.iter().all(|d| d.is_ready())));
    app.frame(&input_none(), &mut bmp, &font);

    let body = pdf_rust::app::layout::sidebar_body(app.regions().sidebar);
    let cells = app.thumbnail_cells().cells;
    assert!(cells.len() >= 6, "one cell per page");
    let visible = cells
        .iter()
        .filter(|c| c.y + 20.0 < body.bottom())
        .count();
    assert!(visible >= 3, "thumbnails are listed for many pages, got {}", visible);
    let target = cells[2];
    click_at(
        &mut app,
        &mut bmp,
        &font,
        Point::new(target.center().x, target.y + 24.0),
    );
    assert_eq!(app.active_doc().unwrap().current_page(), 2, "thumbnail click navigates");
}

/// Search covers the whole document, not just the pages around the current one.
#[test]
fn search_scans_every_page_of_the_document() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-search");
    let pages: Vec<TestPage> = (0..12)
        .map(|i| {
            let line = if i == 9 { "needle on a late page" } else { "filler text" };
            TestPage::a4(0).with_text(&[line])
        })
        .collect();
    let path = PdfBuilder::new(pages).write_to(&dir.join("search.pdf"));
    let font = font();
    let mut bmp = Bitmap::new(1000, 800);
    let mut app = app_with_db(&dir.join("library.db"));
    app.window = Size::new(1000.0, 800.0);
    app.open_path(path, None);
    assert!(pump(&mut app, &mut bmp, &font, |a| a.docs.iter().all(|d| d.is_ready())));

    app.search_start("needle".to_string());
    let found = pump(&mut app, &mut bmp, &font, |a| {
        a.active_doc()
            .map(|d| d.search.hits.iter().any(|h| h.page == 9))
            .unwrap_or(false)
    });
    assert!(found, "the match on page 10 is found from page 1");
}

/// Scrolling must not paint pages over the toolbar or the sidebar.
#[test]
fn scrolled_pages_stay_inside_the_reading_area() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-clip");
    let path = build_pdf(&dir, 6);
    let font = font();
    let mut bmp = Bitmap::new(1000, 800);
    for dark in [false, true] {
        let mut app = app_with_db(&dir.join("library.db"));
        app.window = Size::new(1000.0, 800.0);
        app.settings.dark = dark;
        app.open_path(path.clone(), None);
        assert!(pump(&mut app, &mut bmp, &font, |a| a.docs.iter().all(|d| d.is_ready())));
        // Halfway through the second page: the page above it straddles the top
        // edge of the reading area.
        let scroll = app.regions().content.h * 1.5;
        let viewport = app.regions().content.size();
        if let Some(doc) = app.active_doc_mut() {
            doc.jump_to(scroll);
            doc.recompute_layout(viewport);
        }
        app.frame(&input_none(), &mut bmp, &font);
        let content = app.regions().content;
        // The two rows directly above the reading area belong to the toolbar and
        // must stay uniform whatever the document scrolls to.
        for dy in [1u32, 2] {
            let y = content.y as u32 - dy;
            let expected = bmp.get(content.x as u32 + 1, y);
            for x in (content.x as u32 + 1)..(content.right() as u32 - 1) {
                assert_eq!(
                    bmp.get(x, y),
                    expected,
                    "dark={} page painted over the toolbar at ({}, {})",
                    dark,
                    x,
                    y
                );
            }
        }
        // The reading area itself is unaffected: the page is still painted.
        let mut page_pixels = 0;
        for y in (content.y as u32 + 4)..(content.bottom() as u32 - 4) {
            for x in (content.x as u32 + 4)..(content.right() as u32 - 12) {
                if bmp.get(x, y) == pdf_rust::ui::canvas::Color::rgb(255, 255, 255) {
                    page_pixels += 1;
                }
            }
        }
        assert!(page_pixels > 1000, "dark={} the page is still visible", dark);
    }
}

/// Rotating a document must refresh its thumbnails (they are drawn rotated).
#[test]
fn rotating_redraws_thumbnails() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-thumb-rotate");
    let path = build_pdf(&dir, 4);
    let font = font();
    let mut bmp = Bitmap::new(1200, 900);
    let mut app = app_with_db(&dir.join("library.db"));
    app.window = Size::new(1200.0, 900.0);
    app.show_sidebar = true;
    app.sidebar.tab = pdf_rust::app::SidebarTab::Thumbnails;
    app.open_path(path, None);
    assert!(pump(&mut app, &mut bmp, &font, |a| a
        .active_doc()
        .map(|d| d.thumbs.len() >= 2)
        .unwrap_or(false)));
    let before = app.active_doc().unwrap().thumbs.len();
    assert!(before >= 2);
    app.rotate(90);
    assert!(
        app.active_doc().unwrap().thumbs.is_empty(),
        "rotating clears the stale thumbnails"
    );
    assert!(pump(&mut app, &mut bmp, &font, |a| a
        .active_doc()
        .map(|d| d.thumbs.len() >= 2)
        .unwrap_or(false)));
    // The thumbnail is now landscape: wider than tall for a portrait page.
    let page = app.thumbnail_cells().cells[1];
    let size = app.active_doc().unwrap().info().unwrap().display_size(0, 90);
    assert!(
        page.w > page.h && size.w > size.h,
        "cells follow the rotated page size"
    );
}

/// Thumbnails must render for the pages the user scrolled to, not just the ones
/// around the reading position.
#[test]
fn thumbnails_load_for_the_visible_pages() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-thumb-scroll");
    let path = build_pdf(&dir, 40);
    let font = font();
    let mut bmp = Bitmap::new(1200, 900);
    let mut app = app_with_db(&dir.join("library.db"));
    app.window = Size::new(1200.0, 900.0);
    app.show_sidebar = true;
    app.sidebar.tab = pdf_rust::app::SidebarTab::Thumbnails;
    app.open_path(path, None);
    assert!(pump(&mut app, &mut bmp, &font, |a| !a.active_doc().unwrap().thumbs.is_empty()));
    // Scroll the thumbnail strip to the end of the document.
    app.sidebar.thumb_focus = 0;
    app.sidebar.thumb_scroll = 1_000_000.0;
    assert!(pump(&mut app, &mut bmp, &font, |a| {
        let visible = a.sidebar.thumb_visible.unwrap_or((0, 0));
        visible.1 > 30
            && (visible.0..visible.1).all(|page| a
                .active_doc()
                .map(|d| d.thumbs.get(&page).is_some())
                .unwrap_or(false))
    }));
}

/// The toolbar close button closes the document, never the whole application.
#[test]
fn toolbar_close_button_closes_only_the_document() {
    if !common::pdfium_available() {
        return;
    }
    let dir = TempDir::new("viewer-tb-close");
    let font = font();
    let mut bmp = Bitmap::new(1100, 800);
    let mut app = app_with_db(&dir.join("library.db"));
    app.window = Size::new(1100.0, 800.0);
    for name in ["one.pdf", "two.pdf"] {
        let pages: Vec<TestPage> = (0..2)
            .map(|i| TestPage::a4(0).with_text(&[&format!("{} {}", name, i + 1)]))
            .collect();
        let path = PdfBuilder::new(pages).write_to(&dir.join(name));
        app.open_path(path, None);
    }
    assert!(pump(&mut app, &mut bmp, &font, |a| a.docs.iter().all(|d| d.is_ready())));
    app.active = 0;
    app.frame(&input_none(), &mut bmp, &font);
    // The toolbar's right-hand cluster is laid out from the right edge: search,
    // print, full screen, presentation, theme, settings, info, close.
    let regions = app.regions();
    const BTN: f32 = 30.0;
    const GAP: f32 = 4.0;
    let close_slot_x = regions.toolbar.right() - 8.0 - BTN - 7.0 * (BTN + GAP);
    let close = Point::new(close_slot_x + BTN * 0.5, regions.toolbar.center().y);
    click_at(&mut app, &mut bmp, &font, close);
    assert!(!app.quit, "closing a document does not quit the application");
    assert_eq!(app.docs.len(), 1, "exactly one document closed");
    assert_eq!(app.docs[0].title, "two.pdf");
}


/// The checkbox widget must report a *click*, not the value it was given: with
/// the old contract every checked box fired its action on every frame, which made
/// the settings dialog (and the search options) look broken.
#[test]
fn checkbox_reports_clicks_not_values() {
    use pdf_rust::ui::widgets;
    let font = font();
    let mut bmp = Bitmap::new(200, 120);
    let mut state = pdf_rust::ui::UiState::default();
    let r = Rect::new(10.0, 10.0, 180.0, 24.0);
    let key = pdf_rust::ui::id("test-checkbox");

    // Hovering a checked box without pressing must not report a click.
    let hover = Input {
        mouse: r.center(),
        ..Default::default()
    };
    let mut ui = pdf_rust::ui::Ui::new(&mut bmp, &font, pdf_rust::ui::Theme::light(), &hover, &mut state);
    assert!(!widgets::checkbox(&mut ui, key, r, "Checked", true));
    let idle = Input::default();
    let mut ui = pdf_rust::ui::Ui::new(&mut bmp, &font, pdf_rust::ui::Theme::light(), &idle, &mut state);
    assert!(!widgets::checkbox(&mut ui, key, r, "Checked", true));

    // Press then release inside reports exactly one click, even when checked.
    let press = Input {
        mouse: r.center(),
        down: true,
        pressed: true,
        ..Default::default()
    };
    let mut ui = pdf_rust::ui::Ui::new(&mut bmp, &font, pdf_rust::ui::Theme::light(), &press, &mut state);
    assert!(!widgets::checkbox(&mut ui, key, r, "Checked", true), "press is not a click");
    let release = Input {
        mouse: r.center(),
        down: false,
        released: true,
        ..Default::default()
    };
    let mut ui = pdf_rust::ui::Ui::new(&mut bmp, &font, pdf_rust::ui::Theme::light(), &release, &mut state);
    assert!(widgets::checkbox(&mut ui, key, r, "Checked", true), "release reports the click");
    // ... and a second release on the following frame is not another click.
    let mut ui = pdf_rust::ui::Ui::new(&mut bmp, &font, pdf_rust::ui::Theme::light(), &idle, &mut state);
    assert!(!widgets::checkbox(&mut ui, key, r, "Checked", true));
}
