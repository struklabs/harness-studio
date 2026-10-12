//! The PDF extraction child of the app binary (release builds abort on panic, so PDFs are
//! parsed in `harness-studio --harness-extract-pdf <file>`; see harness_core::rag::extract).

use std::path::Path;

use harness_core::rag::extract::pdf_pages_with;

fn app() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_harness-studio"))
}

#[test]
fn app_binary_extracts_pdf_pages_in_a_child_process() {
    let pdf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/harness-core/tests/data/rag/state-db-schema.pdf");
    let pages = pdf_pages_with(app(), &pdf).unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].number, Some(1));
    assert!(pages[0].text.contains("folders(id, name, parent_id"));
    assert!(pages[1].text.contains("vram_measurements(model_key"));
}

#[test]
fn a_broken_pdf_fails_only_that_document() {
    let path = std::env::temp_dir().join(format!("hs-broken-{}.pdf", std::process::id()));
    std::fs::write(
        &path,
        b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 9 0 R >>\n%%EOF",
    )
    .unwrap();
    let e = pdf_pages_with(app(), &path).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(e.starts_with("rag.pdf_failed"), "{e}");
}
