//! Generates benchmark fixtures (not run by default): `cargo test --test gen_fixtures -- --ignored`.
mod common;
use common::{PdfBuilder, TestPage};

#[test]
#[ignore]
fn generate_benchmark_pdfs() {
    let dir = std::env::temp_dir().join("pdf-rust-bench");
    std::fs::create_dir_all(&dir).unwrap();
    let pages: Vec<TestPage> = (0..200)
        .map(|i| {
            TestPage::a4(0).with_text(&[
                &format!("Chapter {}", i / 10 + 1),
                &format!("Page {} of the benchmark document", i + 1),
                "The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet.",
                "Searchable text appears on every page of this document, including the word needle.",
            ])
        })
        .collect();
    let path = PdfBuilder::new(pages).write_to(&dir.join("benchmark-200.pdf"));
    println!("wrote {}", path.display());
}
