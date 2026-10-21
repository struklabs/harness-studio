//! Storage, planning and keyword retrieval against a real `state.db` and the real fixture
//! documents (no model needed). Dense retrieval and reranking with real models are covered
//! by `crates/harness-gateway/tests/rag_e2e.rs`.

use super::*;
use crate::state::{NewConversation, StateDb};
use std::path::Path;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/rag")
        .join(name)
}

fn setup() -> (tempfile::TempDir, Arc<StateDb>, String) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateDb::open(&dir.path().join("state.db")).unwrap());
    let conv = db
        .create_conversation(&NewConversation {
            title: Some("belgeler".into()),
            ..Default::default()
        })
        .unwrap();
    (dir, db, conv.id)
}

async fn attach(db: &Arc<StateDb>, conv: &str, path: &Path) -> Result<RagDocument, String> {
    let doc = db
        .rag_create_document(
            conv,
            &extract::file_name(path),
            Some(&path.to_string_lossy()),
        )
        .unwrap();
    let flag = jobs().register(&doc.id);
    run_job(
        db.clone(),
        IndexJob {
            document_id: doc.id.clone(),
            source: Some(path.to_path_buf()),
            embed: None,
            ocr: None,
            asr: None,
        },
        flag,
        |_| {},
    )
    .await
}

#[tokio::test]
async fn attach_extracts_chunks_and_searches_keywords() {
    let (_d, db, conv) = setup();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let doc = db
        .rag_create_document(&conv, "state-db-schema.pdf", None)
        .unwrap();
    assert_eq!(doc.status, "pending");
    let flag = jobs().register(&doc.id);
    assert!(jobs().is_running(&doc.id));
    let s = seen.clone();
    let doc = run_job(
        db.clone(),
        IndexJob {
            document_id: doc.id.clone(),
            source: Some(fixture("state-db-schema.pdf")),
            embed: None,
            ocr: None,
            asr: None,
        },
        flag,
        move |p| s.lock().unwrap().push(p.status),
    )
    .await
    .unwrap();
    assert!(!jobs().is_running(&doc.id));
    assert_eq!(*seen.lock().unwrap(), vec!["extracting", "ready"]);
    assert_eq!(doc.status, "ready");
    assert_eq!(doc.kind, "pdf");
    assert_eq!(doc.pages, Some(2));
    assert!(doc.chunks >= 4);
    assert_eq!(doc.embedded, 0);
    assert_eq!(doc.sha256.len(), 64);
    let content = db.rag_content(&doc.id).unwrap();
    assert!(content.starts_with("[Page 1]\nHarness Studio state.db"));

    // Too big for "auto" in a 1k context → searched by keywords, excerpt numbered [1].
    let prepared = prepare(
        &db,
        &conv,
        &RetrieveOptions {
            query: "vram_measurements birincil anahtar".into(),
            top_k: 2,
            context_tokens: Some(1024),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(prepared.system, None);
    assert_eq!(prepared.retrieval.as_deref(), Some("keyword"));
    assert_eq!(
        prepared.notices,
        vec!["rag.notice_keyword_only".to_string()]
    );
    assert!((1..=2).contains(&prepared.sources.len()));
    let top = &prepared.sources[0];
    assert_eq!(top.n, 1);
    assert!(
        top.text.contains("vram_measurements(model_key"),
        "{}",
        top.text
    );
    assert_eq!(top.page_start, Some(2));
    let user = prepared.user.unwrap();
    assert!(user.contains("[1] state-db-schema.pdf, p. 2"), "{user}");
    assert!(user.ends_with("Question: vram_measurements birincil anahtar"));

    // In a big context the same document goes in whole.
    let whole = prepare(
        &db,
        &conv,
        &RetrieveOptions {
            query: "v3 hangi tabloyu ekler?".into(),
            top_k: 3,
            context_tokens: Some(32768),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(whole.user, None);
    assert_eq!(whole.retrieval, None);
    let sys = whole.system.unwrap();
    assert!(sys.contains("<document name=\"state-db-schema.pdf\" pages=\"2\">\n[Page 1]"));
    assert!(sys.contains("[Page 2]\nSonraki migration"));
    assert_eq!(whole.sources.len(), 1);
    assert!(whole.sources[0].full);
    assert_eq!(whole.tokens as i64, doc.tokens);

    // Forced retrieval wins over a big context.
    db.rag_set_mode(&doc.id, "retrieval").unwrap();
    let forced = prepare(
        &db,
        &conv,
        &RetrieveOptions {
            query: "folders tablosu".into(),
            top_k: 1,
            context_tokens: Some(32768),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(forced.system, None);
    assert!(forced.sources[0].text.contains("folders(id, name"));
    assert_eq!(forced.sources[0].page_start, Some(1));
}

#[tokio::test]
async fn duplicates_failures_cancel_and_delete() {
    let (_d, db, conv) = setup();
    attach(&db, &conv, &fixture("state-db-schema.docx"))
        .await
        .unwrap();
    // Same file again → keyed error, document marked failed.
    let e = attach(&db, &conv, &fixture("state-db-schema.docx"))
        .await
        .unwrap_err();
    assert!(e.starts_with("rag.duplicate"), "{e}");
    let docs = db.rag_documents(&conv).unwrap();
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[1].status, "failed");
    assert!(docs[1]
        .error
        .as_deref()
        .unwrap()
        .starts_with("rag.duplicate"));

    // Unsupported file.
    let e = attach(
        &db,
        &conv,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../static/favicon.png"),
    )
    .await
    .unwrap_err();
    assert!(
        e.starts_with("rag.unsupported_file") || e.starts_with("fs.read_failed"),
        "{e}"
    );

    // Cancelled before it starts.
    let doc = db.rag_create_document(&conv, "extract.rs", None).unwrap();
    let flag = jobs().register(&doc.id);
    assert!(jobs().cancel(&doc.id));
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/rag/extract.rs");
    let r = run_job(
        db.clone(),
        IndexJob {
            document_id: doc.id.clone(),
            source: Some(src),
            embed: None,
            ocr: None,
            asr: None,
        },
        flag,
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(r.status, "cancelled");
    assert_eq!(r.chunks, 0);
    assert!(!jobs().cancel(&doc.id));

    // An interrupted job (no longer running) is reconciled to cancelled.
    let stale = db.rag_create_document(&conv, "x.md", None).unwrap();
    db.rag_set_status(&stale.id, "embedding", None).unwrap();
    assert_eq!(db.rag_reconcile(&conv, |_| false).unwrap(), 1);
    assert_eq!(db.rag_document(&stale.id).unwrap().status, "cancelled");

    // Deleting the conversation deletes its documents, chunks and keyword index rows.
    db.delete_conversation(&conv).unwrap();
    db.read(|c| {
        for table in ["rag_documents", "rag_chunks"] {
            let n: i64 = c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            assert_eq!(n, 0, "{table}");
        }
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM rag_fts WHERE rag_fts MATCH 'vram_measurements'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(n, 0);
        Ok(())
    })
    .unwrap();
}

#[test]
fn embeddings_are_stored_per_model() {
    let (_d, db, conv) = setup();
    let doc = db.rag_create_document(&conv, "a.md", None).unwrap();
    let chunks: Vec<chunk::Chunk> = ["birinci", "ikinci", "üçüncü"]
        .iter()
        .map(|t| chunk::Chunk {
            text: t.to_string(),
            page_start: None,
            page_end: None,
            heading: None,
            ..Default::default()
        })
        .collect();
    db.rag_store_extraction(
        &doc.id,
        "markdown",
        10,
        "abc",
        None,
        "birinci ikinci üçüncü",
        &chunks,
        100,
    )
    .unwrap();
    db.rag_begin_embedding(&doc.id, "emb-a").unwrap();
    let pending = db.rag_pending_chunks(&doc.id, 2).unwrap();
    assert_eq!(pending.len(), 2);
    let rows: Vec<(i64, Vec<f32>)> = pending
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                c.chunk_id,
                if i == 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                },
            )
        })
        .collect();
    db.rag_store_embeddings(&doc.id, &rows).unwrap();
    let d = db.rag_document(&doc.id).unwrap();
    assert_eq!((d.embedded, d.chunks, d.embed_dims), (2, 3, Some(2)));
    // Another dimension for the same document is refused.
    let last = db.rag_pending_chunks(&doc.id, 10).unwrap();
    let e = db
        .rag_store_embeddings(&doc.id, &[(last[0].chunk_id, vec![1.0, 0.0, 0.0])])
        .unwrap_err();
    assert!(e.to_string().starts_with("rag.dims_mismatch"), "{e}");
    let ids = vec![doc.id.clone()];
    let v = db.rag_vectors(&ids, "emb-a").unwrap();
    assert_eq!(v.len(), 2);
    let best = search::nearest(&[0.0, 1.0], &v, 1);
    assert_eq!(best[0].0, pending[1].chunk_id);
    assert!(db.rag_vectors(&ids, "emb-b").unwrap().is_empty());
    // Same model: resume keeps vectors; another model drops them (re-index).
    db.rag_begin_embedding(&doc.id, "emb-a").unwrap();
    assert_eq!(db.rag_document(&doc.id).unwrap().embedded, 2);
    let d = db.rag_begin_embedding(&doc.id, "emb-b").unwrap();
    assert_eq!(
        (d.embedded, d.embed_model.as_deref(), d.embed_dims),
        (0, Some("emb-b"), None)
    );
    // Chunk cap per conversation.
    let other = db.rag_create_document(&conv, "b.md", None).unwrap();
    let e = db
        .rag_store_extraction(&other.id, "markdown", 1, "def", None, "x", &chunks, 5)
        .unwrap_err();
    assert!(e.to_string().starts_with("rag.too_many_chunks"), "{e}");
    assert!(db.rag_set_mode(&doc.id, "sometimes").is_err());
}

#[test]
fn plan_splits_full_text_and_search() {
    let doc = |id: &str, tokens: i64, mode: &str, status: &str| RagDocument {
        id: id.into(),
        conversation_id: "c".into(),
        name: id.into(),
        kind: "text".into(),
        source_path: None,
        size: 0,
        sha256: String::new(),
        pages: None,
        chars: tokens * 3,
        tokens,
        chunks: 1,
        embedded: 0,
        status: status.into(),
        error: None,
        mode: mode.into(),
        embed_model: None,
        embed_dims: None,
        ocr_pages: Vec::new(),
        mtime_ms: None,
        rel_path: None,
        meta: None,
        created_at: 0,
        updated_at: 0,
    };
    let names = |v: Vec<&RagDocument>| v.iter().map(|d| d.id.clone()).collect::<Vec<_>>();
    let docs = vec![
        doc("a", 1000, "auto", "ready"),
        doc("b", 1500, "auto", "ready"),
        doc("f", 500, "full", "ready"),
        doc("r", 10, "retrieval", "ready"),
        doc("x", 10, "auto", "embedding"),
    ];
    // 8k context → budget 4k: a + b + f = 3000 fit.
    let (full, search) = plan(&docs, Some(8192));
    assert_eq!(names(full), vec!["f", "a", "b"]);
    assert_eq!(names(search), vec!["r"]);
    // 4k default → budget 2k: autos go to search, the forced full one stays.
    let (full, search) = plan(&docs, None);
    assert_eq!(names(full), vec!["f"]);
    assert_eq!(names(search), vec!["r", "a", "b"]);
}

#[test]
fn prompts_number_excerpts_and_name_documents() {
    let src = |n, page: Option<i64>, heading: Option<&str>| Source {
        n,
        document_id: "d".into(),
        document_name: "kılavuz.pdf".into(),
        chunk_id: Some(n as i64),
        page_start: page,
        page_end: page.map(|p| p + (n as i64 - 1)),
        t_start_ms: None,
        t_end_ms: None,
        heading: heading.map(str::to_string),
        text: format!("parça {n}\n"),
        similarity: None,
        rerank_score: None,
        full: false,
    };
    let p = excerpts_prompt(
        " Soru? ",
        &[
            src(1, Some(3), Some("Kurulum")),
            src(2, Some(4), None),
            src(3, None, None),
        ],
    );
    assert!(p.contains("<excerpts>\n[1] kılavuz.pdf, p. 3 — Kurulum\nparça 1\n\n[2] kılavuz.pdf, pp. 4-5\nparça 2\n\n[3] kılavuz.pdf\nparça 3\n\n</excerpts>\n\nQuestion: Soru?"), "{p}");
    let s = full_text_block(&[("a \"b\".txt".into(), None, "metin\n".into())]);
    assert!(
        s.ends_with("<document name=\"a 'b'.txt\">\nmetin\n</document>\n"),
        "{s}"
    );
}

// ---- cancelling while the file is being read ----

#[tokio::test]
async fn cancel_takes_effect_while_a_blocking_read_is_still_running() {
    let flag = Arc::new(AtomicBool::new(false));
    let setter = flag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        setter.store(true, Ordering::SeqCst);
    });
    let started = std::time::Instant::now();
    // A parse that takes 5 s (a large PDF): the job must not wait for it.
    let r: Result<(), String> = blocking_or_cancel(&flag, || {
        std::thread::sleep(Duration::from_secs(5));
        Ok(())
    })
    .await;
    assert_eq!(r.unwrap_err(), "rag.cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cancel waited for the read: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_finished_read_is_returned_when_not_cancelled() {
    let flag = AtomicBool::new(false);
    let r = blocking_or_cancel(&flag, || Ok::<_, String>(7)).await;
    assert_eq!(r, Ok(7));
    let e = blocking_or_cancel(&flag, || Err::<(), _>("boom".to_string()))
        .await
        .unwrap_err();
    assert_eq!(e, "boom");
}

// ---- documents follow conversations ----

/// Stores synthetic vectors (one per chunk) the way an embedding job would.
fn embed_all(db: &StateDb, doc_id: &str, model: &str) {
    db.rag_begin_embedding(doc_id, model).unwrap();
    let pending = db.rag_pending_chunks(doc_id, 10_000).unwrap();
    let rows: Vec<(i64, Vec<f32>)> = pending
        .iter()
        .enumerate()
        .map(|(i, c)| (c.chunk_id, vec![i as f32 + 1.0, 1.0]))
        .collect();
    db.rag_store_embeddings(doc_id, &rows).unwrap();
    db.rag_set_status(doc_id, "ready", None).unwrap();
}

fn same_chunks(db: &StateDb, a: &RagDocument, b: &RagDocument) {
    assert_eq!(a.name, b.name);
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(a.pages, b.pages);
    assert_eq!(a.chunks, b.chunks);
    assert_eq!(
        db.rag_content(&a.id).unwrap(),
        db.rag_content(&b.id).unwrap()
    );
}

#[tokio::test]
async fn duplicate_and_fork_carry_documents_with_their_vectors() {
    let (_d, db, conv) = setup();
    let pdf = attach(&db, &conv, &fixture("state-db-schema.pdf"))
        .await
        .unwrap();
    attach(&db, &conv, &fixture("state-db-schema.docx"))
        .await
        .unwrap();
    embed_all(&db, &pdf.id, "emb-a");
    db.rag_set_mode(&pdf.id, "retrieval").unwrap();
    let u = db
        .append_message(crate::state::AppendMessage {
            conversation_id: conv.clone(),
            parent_id: None,
            role: "user".into(),
            variant: crate::state::VariantInput {
                content: "q".into(),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();

    let dup = db.duplicate_conversation(&conv, None).unwrap();
    let fork = db.fork_conversation(&u.id, None).unwrap();
    let originals = db.rag_documents(&conv).unwrap();
    for copy in [&dup.id, &fork.id] {
        let docs = db.rag_documents(copy).unwrap();
        assert_eq!(docs.len(), 2);
        for (o, c) in originals.iter().zip(&docs) {
            same_chunks(&db, o, c);
            assert_ne!(o.id, c.id);
            assert_eq!(c.status, "ready");
            assert_eq!(c.mode, o.mode);
            assert_eq!(c.embed_model, o.embed_model);
            assert_eq!(c.embedded, o.embedded);
        }
        // The copy searches by keywords and has its own vectors.
        let ids: Vec<String> = docs.iter().map(|d| d.id.clone()).collect();
        assert!(!db
            .rag_keyword(&ids, "\"vram_measurements\"", 5)
            .unwrap()
            .is_empty());
        assert_eq!(
            db.rag_vectors(&ids[..1], "emb-a").unwrap().len() as i64,
            docs[0].chunks
        );
    }
    // Deleting the original leaves the copies' documents alone.
    db.delete_conversation(&conv).unwrap();
    assert_eq!(db.rag_documents(&dup.id).unwrap().len(), 2);
}

#[tokio::test]
async fn documents_that_are_not_ready_are_not_copied() {
    let (_d, db, conv) = setup();
    attach(&db, &conv, &fixture("state-db-schema.docx"))
        .await
        .unwrap();
    // Still indexing (e.g. interrupted) or failed: transient states stay with the original.
    let stale = db.rag_create_document(&conv, "x.md", None).unwrap();
    db.rag_set_status(&stale.id, "embedding", None).unwrap();
    let failed = db.rag_create_document(&conv, "y.md", None).unwrap();
    db.rag_set_status(&failed.id, "failed", Some("rag.empty_document"))
        .unwrap();
    let dup = db.duplicate_conversation(&conv, None).unwrap();
    let docs = db.rag_documents(&dup.id).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].name, "state-db-schema.docx");
}

#[tokio::test]
async fn json_export_carries_documents_without_vectors_or_paths() {
    let (_d, db, conv) = setup();
    let pdf = attach(&db, &conv, &fixture("state-db-schema.pdf"))
        .await
        .unwrap();
    embed_all(&db, &pdf.id, "emb-a");
    let export = db.export_json(&conv).unwrap();
    assert_eq!(export.documents.len(), 1);
    let text = serde_json::to_string(&export).unwrap();
    // Text and chunks travel; vectors and the local file path do not.
    assert!(text.contains("vram_measurements"));
    assert!(!text.contains("source_path") && !text.contains("embedding"));
    assert!(export.documents[0].embed_model.is_none());

    let imported = db.import_json_str(&text).unwrap();
    let docs = db.rag_documents(&imported.id).unwrap();
    assert_eq!(docs.len(), 1);
    same_chunks(&db, &pdf, &docs[0]);
    assert_eq!(docs[0].status, "ready");
    assert_eq!(
        (docs[0].embedded, docs[0].embed_model.as_deref()),
        (0, None)
    );
    assert!(docs[0].source_path.is_none());
    // Searchable at once (keywords), and indexable again with an embedding model.
    let ids = vec![docs[0].id.clone()];
    assert!(!db
        .rag_keyword(&ids, "\"vram_measurements\"", 5)
        .unwrap()
        .is_empty());
    db.rag_begin_embedding(&docs[0].id, "emb-b").unwrap();
    assert_eq!(
        db.rag_pending_chunks(&docs[0].id, 10_000).unwrap().len() as i64,
        docs[0].chunks
    );

    // An older file without a `documents` key still imports.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v.as_object_mut().unwrap().remove("documents");
    let old = db.import_json_str(&v.to_string()).unwrap();
    assert!(db.rag_documents(&old.id).unwrap().is_empty());
}
