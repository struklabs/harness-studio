//! Knowledge bases with an OCR model: scanned and mixed PDFs are indexed through the same
//! code path as the gateway's `/v1/ocr` page recognition, with the rasterizer and the model
//! faked (the real ones are covered by the manual end-to-end run).

use super::kb::*;
use super::ocr::{OcrModel, RasterPage};
use super::*;
use crate::state::{NewConversation, StateDb};
use std::path::Path;
use std::sync::Mutex;

/// A PDF with one page per entry; an empty entry is a page without a text layer.
fn pdf(texts: &[&str]) -> Vec<u8> {
    let kids = (0..texts.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", texts.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    for (i, text) in texts.iter().enumerate() {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + i * 2));
        let stream = if text.is_empty() {
            String::new()
        } else {
            format!("BT /F1 20 Tf 20 100 Td ({text}) Tj ET")
        };
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }
    let mut out = "%PDF-1.4\n".to_string();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{obj}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

fn setup() -> (tempfile::TempDir, Arc<StateDb>) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateDb::open(&dir.path().join("state.db")).unwrap());
    (dir, db)
}

/// A model that "reads" `Zebra crossing on page N` from the PNG the fake rasterizer made
/// (its first byte is the page number); `calls` collects what it was given.
fn model(calls: Arc<Mutex<Vec<u8>>>) -> OcrModel {
    OcrModel::new("ocr-fake", move |png| {
        calls.lock().unwrap().push(png[0]);
        Box::pin(async move { Ok(format!("Zebra crossing on page {}", png[0])) })
    })
    .with_rasterizer(Arc::new(|_: &Path, pages: &[u32], _| {
        Ok(pages
            .iter()
            .map(|&n| RasterPage {
                number: n,
                png: vec![n as u8],
            })
            .collect())
    }))
}

async fn index(
    db: &Arc<StateDb>,
    kb_id: &str,
    dir: &Path,
    ocr: Option<OcrModel>,
) -> Vec<crate::state::RagDocument> {
    let flag = kb_jobs().register(kb_id);
    index_kb(
        db.clone(),
        KbIndexRequest {
            kb_id: kb_id.to_string(),
            files: scan_folder(dir).unwrap().files,
            root: Some(dir.to_path_buf()),
            embed: None,
            ocr,
            rebuild: false,
        },
        flag,
        |_| {},
    )
    .await
    .unwrap();
    db.rag_documents(kb_id).unwrap()
}

#[test]
fn the_ocr_model_of_a_base_persists_and_clears() {
    let (_d, db) = setup();
    let kb = db.kb_create("Scans", None).unwrap();
    assert_eq!(kb.ocr_model, None);
    let kb = db.kb_set_ocr_model(&kb.id, Some(" paddleocr-vl ")).unwrap();
    assert_eq!(kb.ocr_model.as_deref(), Some("paddleocr-vl"));
    assert_eq!(db.kb_get(&kb.id).unwrap().ocr_model, kb.ocr_model);
    assert_eq!(db.kb_list().unwrap()[0].ocr_model, kb.ocr_model);
    assert_eq!(
        db.kb_set_ocr_model(&kb.id, Some("  ")).unwrap().ocr_model,
        None
    );
    assert!(db.kb_set_ocr_model("missing", Some("x")).is_err());
}

#[tokio::test]
async fn a_scanned_pdf_is_recognized_indexed_and_found() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scan.pdf"), pdf(&["", ""])).unwrap();
    let kb = db.kb_create("Scans", None).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let docs = index(&db, &kb.id, dir.path(), Some(model(calls.clone()))).await;
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].status, "ready", "{:?}", docs[0].error);
    assert_eq!(docs[0].ocr_pages, vec![1, 2]);
    assert_eq!(*calls.lock().unwrap(), vec![1, 2]);

    let chat = db.create_conversation(&NewConversation::default()).unwrap();
    db.kb_attach(&chat.id, &kb.id).unwrap();
    let found = prepare(
        &db,
        &chat.id,
        &RetrieveOptions {
            query: "zebra crossing page 2".into(),
            top_k: 2,
            context_tokens: Some(8192),
            embed: None,
            rerank: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(found.retrieval.as_deref(), Some("keyword"));
    assert!(found.sources[0].text.contains("Zebra crossing"));
}

#[tokio::test]
async fn a_mixed_pdf_uses_the_text_layer_and_recognizes_only_the_blank_pages() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("mixed.pdf"),
        pdf(&["Quarterly report", "", "Closing remarks"]),
    )
    .unwrap();
    let kb = db.kb_create("Mixed", None).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let docs = index(&db, &kb.id, dir.path(), Some(model(calls.clone()))).await;
    assert_eq!(docs[0].status, "ready", "{:?}", docs[0].error);
    assert_eq!(docs[0].ocr_pages, vec![2]);
    assert_eq!(*calls.lock().unwrap(), vec![2]);
    assert_eq!(docs[0].pages, Some(3));
}

#[tokio::test]
async fn text_pdfs_never_start_the_ocr_model() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plain.pdf"), pdf(&["All text here"])).unwrap();
    let kb = db.kb_create("Text", None).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let docs = index(&db, &kb.id, dir.path(), Some(model(calls.clone()))).await;
    assert_eq!(docs[0].status, "ready");
    assert!(docs[0].ocr_pages.is_empty());
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_an_ocr_model_a_scanned_pdf_fails_as_before() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scan.pdf"), pdf(&[""])).unwrap();
    let kb = db.kb_create("Scans", None).unwrap();
    let docs = index(&db, &kb.id, dir.path(), None).await;
    assert_eq!(docs[0].status, "failed");
    assert!(docs[0]
        .error
        .as_deref()
        .unwrap()
        .starts_with("rag.pdf_no_text"));
}

#[tokio::test]
async fn an_ocr_model_without_the_python_engine_fails_with_a_clear_error() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("scan.pdf"), pdf(&[""])).unwrap();
    let kb = db.kb_create("Scans", None).unwrap();
    // The real rasterizer, in a test data folder without the environment.
    let ocr = OcrModel::new("ocr-fake", |_| Box::pin(async { Ok("text".to_string()) }));
    let docs = index(&db, &kb.id, dir.path(), Some(ocr)).await;
    assert_eq!(docs[0].status, "failed");
    assert_eq!(docs[0].error.as_deref(), Some("ocr.rasterizer_unavailable"));
}
