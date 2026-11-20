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
        let row = app.db.as_ref().unwrap().find_by_path(&path.to_string_lossy()).unwrap();
        assert!(row.is_some(), "the document must be recorded");
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

/// Settings dialog geometry, mirroring `chrome::paint_settings_dialog`.
fn settings_rects(window: Rect) -> (Rect, Rect, Rect, [Rect; 3]) {
    let frame = Rect::new(
        window.x + (window.w - 560.0) * 0.5,
        window.y + (window.h - 470.0) * 0.5,
        560.0,
        470.0,
    );
    let inner = Rect::new(frame.x + 20.0, frame.y + 54.0, frame.w - 40.0, frame.h - 70.0);
    let dark = Rect::new(inner.x, inner.y + 20.0, inner.w, 26.0);
    let smooth = Rect::new(inner.x, inner.y + 54.0, inner.w, 26.0);
    let mut policies = [Rect::ZERO; 3];
    let mut y = inner.y + 76.0 + 22.0;
    for r in policies.iter_mut() {
        *r = Rect::new(inner.x, y, inner.w, 26.0);
        y += 30.0;
    }
    let done = Rect::new(frame.right() - 104.0, frame.bottom() - 44.0, 86.0, 32.0);
    (dark, smooth, done, policies)
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
    let (dark, smooth, done, policies) = settings_rects(window);

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
