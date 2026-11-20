//! Integration tests for the PDF engine (PDFium bindings + worker thread).
//!
//! These run against real PDFium when the shared library is available (see
//! `scripts/fetch-pdfium.sh`); otherwise they are skipped with a message so the
//! rest of the suite still runs on machines without the engine.

mod common;

use common::{PdfBuilder, TempDir, TestPage};
use pdf_rust::pdf::{engine::Engine, Command, DocInfo, Event, RenderKey};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn open_doc(
    engine: &Engine,
    id: u64,
    path: &std::path::Path,
    password: Option<&str>,
) -> Result<Arc<DocInfo>, String> {
    engine.send(Command::Open {
        id,
        path: path.to_path_buf(),
        password: password.map(|s| s.to_string()),
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        for evt in engine.drain(32) {
            if let Event::Opened { id: got, result, .. } = evt {
                if got == id {
                    return result;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("timed out waiting for the document to open".into())
}

fn render(
    engine: &Engine,
    id: u64,
    key: RenderKey,
) -> Result<Arc<pdf_rust::ui::Bitmap>, String> {
    engine.send(Command::Render { id, key });
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        for evt in engine.drain(32) {
            if let Event::Rendered { id: got, key: k, bitmap, .. } = evt {
                if got == id && k == key {
                    return Ok(bitmap);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("timed out waiting for a rendered page".into())
}

fn non_white_pixels(bmp: &pdf_rust::ui::Bitmap) -> usize {
    bmp.px
        .iter()
        .filter(|p| {
            let r = (**p >> 16) & 0xFF;
            let g = (**p >> 8) & 0xFF;
            let b = **p & 0xFF;
            r < 200 || g < 200 || b < 200
        })
        .count()
}

#[test]
fn opens_a_document_and_reports_pages() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("open");
    let mut builder = PdfBuilder::with_pages(20);
    builder.title = Some("Integration Test Document".into());
    builder.author = Some("pdf-rust".into());
    let path = builder.write_to(&dir.join("twenty.pdf"));

    let engine = Engine::start();
    assert!(engine.is_ready(), "engine should be ready: {:?}", engine.init_error);
    let info = open_doc(&engine, 1, &path, None).expect("document should open");
    assert_eq!(info.page_count, 20);
    assert_eq!(info.page_sizes.len(), 20);
    assert!((info.page_sizes[0].w - 595.0).abs() < 1.0);
    assert!((info.page_sizes[0].h - 842.0).abs() < 1.0);
    assert_eq!(info.title.as_deref(), Some("Integration Test Document"));
    assert_eq!(info.author.as_deref(), Some("pdf-rust"));
    assert!(!info.encrypted);
}

#[test]
fn renders_pages_at_the_requested_scale_and_rotation() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("render");
    let path = PdfBuilder::with_pages(3).write_to(&dir.join("three.pdf"));
    let engine = Engine::start();
    let _info = open_doc(&engine, 1, &path, None).expect("open");

    let key = RenderKey::new(0, 1.0, 0);
    let bitmap = render(&engine, 1, key).expect("render");
    assert_eq!(bitmap.width, 595);
    assert_eq!(bitmap.height, 842);
    assert!(non_white_pixels(&bitmap) > 500, "page looks blank");
    assert!(bitmap.px.iter().all(|p| (p >> 24) == 0xFF), "alpha not opaque");

    // 2x zoom doubles the bitmap.
    let key2 = RenderKey::new(0, 2.0, 0);
    let bitmap2 = render(&engine, 1, key2).expect("render 2x");
    assert_eq!(bitmap2.width, 1190);
    assert_eq!(bitmap2.height, 1684);

    // 90 degree rotation swaps the dimensions.
    let key3 = RenderKey::new(0, 1.0, 90);
    let bitmap3 = render(&engine, 1, key3).expect("render rotated");
    assert_eq!(bitmap3.width, 842, "rotated width");
    assert_eq!(bitmap3.height, 595, "rotated height");
    assert!(non_white_pixels(&bitmap3) > 500, "rotated page looks blank");
}

#[test]
fn extracts_text_and_finds_search_hits() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("text");
    let pages = vec![
        TestPage::a4(0).with_text(&["Alpha page", "needle in a haystack"]),
        TestPage::a4(0).with_text(&["Beta page", "another needle here"]),
    ];
    let path = PdfBuilder::new(pages).write_to(&dir.join("text.pdf"));
    let engine = Engine::start();
    let _ = open_doc(&engine, 7, &path, None).expect("open");

    engine.send(Command::PageText {
        id: 7,
        page: 0,
        token: 1,
    });
    let mut text = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && text.is_none() {
        for evt in engine.drain(16) {
            if let Event::Text { page, text: t, .. } = evt {
                assert_eq!(page, 0);
                text = Some(t);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let text = text.expect("text event");
    assert!(text.text.contains("Alpha page"), "got {:?}", text.text);
    assert!(text.text.contains("needle"), "got {:?}", text.text);
    assert!(!text.chars.is_empty());
    let first = text.chars[0];
    assert!(first.width() > 0.0 && first.height() > 0.0);

    engine.send(Command::Search {
        id: 7,
        page: 1,
        query: "needle".into(),
        token: 2,
        start_index: -1,
        backwards: false,
        match_case: false,
    });
    let mut hits = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && hits.is_none() {
        for evt in engine.drain(16) {
            if let Event::Searched { hits: h, page, .. } = evt {
                assert_eq!(page, 1);
                hits = Some(h);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let hits = hits.expect("search event");
    assert_eq!(hits.len(), 1, "expected exactly one hit: {hits:?}");
    assert_eq!(hits[0].char_count, 6);
    assert!(hits[0].right > hits[0].left);
    assert!(hits[0].top > hits[0].bottom);
}

#[test]
fn reads_outline_and_links() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("outline");
    let mut link_page = TestPage::a4(0).with_text(&["Page with a link"]);
    link_page
        .links
        .push((72.0, 700.0, 300.0, 730.0, "https://example.com/docs".into()));
    let pages = vec![link_page, TestPage::a4(0).with_text(&["Second"])];
    let mut builder = PdfBuilder::new(pages);
    builder.outline = vec![("Introduction".into(), 1), ("Chapter Two".into(), 2)];
    let path = builder.write_to(&dir.join("outline.pdf"));

    let engine = Engine::start();
    let _ = open_doc(&engine, 3, &path, None).expect("open");

    engine.send(Command::Outline { id: 3, token: 9 });
    let mut items = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && items.is_none() {
        for evt in engine.drain(16) {
            if let Event::Outline { items: it, .. } = evt {
                items = Some(it);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let items = items.expect("outline event");
    assert_eq!(items.len(), 2, "outline: {items:?}");
    assert_eq!(items[0].title, "Introduction");
    assert_eq!(items[0].page, Some(0));
    assert_eq!(items[1].page, Some(1));

    engine.send(Command::Links {
        id: 3,
        page: 0,
        token: 10,
    });
    let mut links = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && links.is_none() {
        for evt in engine.drain(16) {
            if let Event::Links { links: l, .. } = evt {
                links = Some(l);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let links = links.expect("links event");
    assert_eq!(links.len(), 1, "links: {links:?}");
    match &links[0].target {
        pdf_rust::pdf::LinkTarget::Uri(uri) => assert!(uri.contains("example.com")),
        other => panic!("unexpected link target {other:?}"),
    }
}

#[test]
fn handles_encrypted_and_corrupt_documents() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("broken");
    let mut builder = PdfBuilder::with_pages(2);
    builder.encrypt = Some(("s3cret".into(), "owner".into()));
    let encrypted = builder.write_to(&dir.join("locked.pdf"));

    let corrupt = dir.join("corrupt.pdf");
    std::fs::write(&corrupt, b"this is definitely not a PDF file").unwrap();

    let truncated = dir.join("truncated.pdf");
    let full = PdfBuilder::with_pages(5).build();
    std::fs::write(&truncated, &full[..full.len() / 3]).unwrap();

    let engine = Engine::start();

    let result = open_doc(&engine, 1, &encrypted, None);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_lowercase().contains("password"),
        "expected a password error"
    );

    let result = open_doc(&engine, 2, &encrypted, Some("wrong"));
    assert!(result.is_err());

    let info = open_doc(&engine, 3, &encrypted, Some("s3cret")).expect("correct password opens");
    assert_eq!(info.page_count, 2);
    assert!(info.encrypted);

    let result = open_doc(&engine, 4, &corrupt, None);
    assert!(result.is_err());

    // A truncated file must not crash the engine: either it opens with fewer
    // usable pages or it reports an error.
    let _ = open_doc(&engine, 5, &truncated, None);

    // The engine is still healthy after the failures.
    let path = PdfBuilder::with_pages(1).write_to(&dir.join("ok.pdf"));
    let info = open_doc(&engine, 6, &path, None).expect("engine still works");
    assert_eq!(info.page_count, 1);
}

#[test]
fn handles_missing_file() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let engine = Engine::start();
    let result = open_doc(&engine, 1, std::path::Path::new("/nonexistent/nothing.pdf"), None);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("file"), "error should mention the file");
}

#[test]
fn opens_large_documents_quickly() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("large");
    let path = PdfBuilder::with_pages(1000).write_to(&dir.join("thousand.pdf"));
    let engine = Engine::start();
    let started = Instant::now();
    let info = open_doc(&engine, 1, &path, None).expect("open large document");
    let elapsed = started.elapsed();
    assert_eq!(info.page_count, 1000);
    assert!(
        elapsed < Duration::from_secs(5),
        "opening 1000 pages took {:?}",
        elapsed
    );
    // Only the requested page is rasterized, so this stays cheap.
    let bitmap = render(&engine, 1, RenderKey::new(999, 1.0, 0)).expect("render last page");
    assert_eq!(bitmap.height, 842);
}

#[test]
fn exports_a_rotated_copy_without_touching_the_original() {
    if !common::pdfium_available() {
        eprintln!("skipping: PDFium not installed");
        return;
    }
    let dir = TempDir::new("export");
    let original = PdfBuilder::with_pages(2).write_to(&dir.join("original.pdf"));
    let original_bytes = std::fs::read(&original).unwrap();
    let dest = dir.join("rotated.pdf");

    let engine = Engine::start();
    let _ = open_doc(&engine, 1, &original, None).expect("open");
    engine.send(Command::ExportRotated {
        id: 1,
        rotations: vec![(0, 90), (1, 180)],
        dest: dest.clone(),
        token: 5,
    });
    let mut result = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && result.is_none() {
        for evt in engine.drain(16) {
            if let Event::Exported { result: r, .. } = evt {
                result = Some(r);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let written = result.expect("export event").expect("export succeeded");
    assert!(std::path::Path::new(&written).exists());
    assert_eq!(
        std::fs::read(&original).unwrap(),
        original_bytes,
        "the original document must never be modified"
    );

    // The exported copy really has the rotations baked in.
    let info = open_doc(&engine, 2, &dest, None).expect("reopen exported copy");
    assert_eq!(info.page_rotations[0], 90);
    assert_eq!(info.page_rotations[1], 180);
}
