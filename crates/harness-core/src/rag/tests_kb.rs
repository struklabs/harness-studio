//! Knowledge bases against a real `state.db` and a fake embedding server (a bag of hashed words
//! behind `/v1/embeddings`, so ranking is deterministic and needs no model). Real models are
//! covered by `crates/harness-gateway/tests/rag_e2e.rs` and the manual end-to-end run.

use super::kb::*;
use super::*;
use crate::state::{KnowledgeBaseRow, NewConversation, StateDb};
use axum::{body::Bytes, http::header, routing::post, Router};
use std::path::Path;
use std::sync::atomic::AtomicUsize;

const DIMS: usize = 256;

fn word_vector(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; DIMS];
    for w in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
    {
        // FNV-1a
        let mut h: u64 = 0xcbf29ce484222325;
        for b in w.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        v[(h % DIMS as u64) as usize] += 1.0;
    }
    v
}

/// A fake `/v1/embeddings` server; returns the embed model and a request counter.
async fn fake_embedder(alias: &str) -> (EmbedModel, Arc<AtomicUsize>) {
    fake_embedder_limited(alias, usize::MAX).await
}

/// Like [`fake_embedder`], but answers HTTP 400 like llama-server when an input is longer than
/// `limit` characters.
async fn fake_embedder_limited(alias: &str, limit: usize) -> (EmbedModel, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let app = Router::new().route(
        "/v1/embeddings",
        post(move |body: Bytes| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
                if req["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t.as_str().unwrap().chars().count() > limit)
                {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        [(header::CONTENT_TYPE, "application/json")],
                        r#"{"error":{"message":"input (516 tokens) is larger than the max context size (511 tokens). skipping"}}"#.to_string(),
                    );
                }
                let data: Vec<serde_json::Value> = req["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        serde_json::json!({
                            "index": i,
                            "embedding": word_vector(t.as_str().unwrap())
                        })
                    })
                    .collect();
                (
                    axum::http::StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    serde_json::json!({ "data": data }).to_string(),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        EmbedModel {
            endpoint: Endpoint {
                base_url: format!("http://127.0.0.1:{port}/v1"),
                token: None,
            },
            alias: alias.to_string(),
            query_prefix: String::new(),
            doc_prefix: String::new(),
            max_chars: None,
        },
        calls,
    )
}

fn setup() -> (tempfile::TempDir, Arc<StateDb>) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateDb::open(&dir.path().join("state.db")).unwrap());
    (dir, db)
}

fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

const CATS: &str = "# Cats\n\nCats are small carnivorous mammals that purr, groom their whiskers and hunt mice at night. A cat sleeps most of the day and a kitten learns to hunt by playing with its siblings.";
const RUST: &str = "# Rust\n\nRust enforces ownership and the borrow checker at compile time. Each value has one owner, references borrow it, and cargo builds crates without a garbage collector.";
const PASTA: &str = "# Pasta\n\nBoil water with salt, cook the spaghetti until firm, then toss it with tomato sauce, fresh basil and olive oil. Parmesan goes on top of the pasta.";

/// A folder with three topics and some files a scan must leave out.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "cats.md", CATS);
    write(dir.path(), "notes/rust.md", RUST);
    write(dir.path(), "notes/deep/pasta.txt", PASTA);
    dir
}

async fn build(
    db: &Arc<StateDb>,
    dir: &Path,
    embed: Option<EmbedModel>,
    rebuild: bool,
) -> (KnowledgeBaseRow, KbRun, Vec<KbProgress>) {
    let kb = db
        .kb_create("Corpus", Some(&dir.to_string_lossy()))
        .unwrap();
    let run = run(db, &kb.id, dir, embed, rebuild).await;
    (db.kb_get(&kb.id).unwrap(), run.0, run.1)
}

async fn run(
    db: &Arc<StateDb>,
    kb_id: &str,
    dir: &Path,
    embed: Option<EmbedModel>,
    rebuild: bool,
) -> (KbRun, Vec<KbProgress>) {
    let scan = scan_folder(dir).unwrap();
    let flag = kb_jobs().register(kb_id);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    let out = index_kb(
        db.clone(),
        KbIndexRequest {
            kb_id: kb_id.to_string(),
            files: scan.files,
            root: Some(dir.to_path_buf()),
            embed,
            rebuild,
        },
        flag,
        move |p| s.lock().unwrap().push(p),
    )
    .await
    .unwrap();
    let events = seen.lock().unwrap().clone();
    (out, events)
}

// ---- chunking ----

#[test]
fn chunks_overlap_and_stay_below_the_limit() {
    let sentence = "The quick brown fox jumps over the lazy dog near the river bank. ";
    let text = sentence.repeat(60);
    let pages = vec![extract::PageText { number: None, text }];
    let chunks = chunk::chunk_pages(&pages);
    assert!(chunks.len() >= 3, "{} chunks", chunks.len());
    for c in &chunks {
        // A chunk is at most the packing limit plus the overlap it carries over.
        assert!(c.text.chars().count() <= chunk::MAX_CHARS + chunk::OVERLAP_CHARS);
    }
    // The next chunk starts with the tail of the previous one.
    for pair in chunks.windows(2) {
        let head: String = pair[1].text.chars().take(40).collect();
        assert!(
            pair[0].text.contains(head.trim()),
            "no overlap between {:?} and {:?}",
            pair[0].text,
            pair[1].text
        );
    }
}

// ---- folders ----

#[test]
fn scan_takes_supported_files_and_skips_build_and_hidden_folders() {
    let dir = corpus();
    write(dir.path(), "node_modules/dep/readme.md", "# dep");
    write(dir.path(), ".git/notes.md", "# git");
    write(dir.path(), "target/out.txt", "build");
    write(dir.path(), "logo.png", "png");
    write(dir.path(), "src/main.rs", "fn main() {}");
    write(dir.path(), "package-lock.json", "{}");
    write(dir.path(), "empty.md", "");
    let scan = scan_folder(dir.path()).unwrap();
    let names: Vec<String> = scan
        .files
        .iter()
        .map(|f| doc_name(Some(dir.path()), f))
        .collect();
    assert_eq!(
        names,
        vec![
            "cats.md",
            "notes/deep/pasta.txt",
            "notes/rust.md",
            "src/main.rs"
        ]
    );
    // The lock file and the empty file are counted, not indexed.
    assert_eq!(scan.skipped, 2);
    assert!(!scan.truncated);
    assert!(scan_folder(&dir.path().join("cats.md")).is_err());
    assert!(scan_folder(&dir.path().join("missing")).is_err());
}

// ---- store ----

#[test]
fn bases_are_hidden_attachable_and_deleted_with_their_documents() {
    let (_d, db) = setup();
    let chat = db
        .create_conversation(&NewConversation {
            title: Some("chat".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(db.kb_create("  ", None).is_err());
    let kb = db.kb_create(" Notes ", Some("/tmp/notes")).unwrap();
    assert_eq!(kb.name, "Notes");
    assert_eq!(kb.folder.as_deref(), Some("/tmp/notes"));
    assert!(db.rag_is_kb(&kb.id) && !db.rag_is_kb(&chat.id));
    // The hidden conversation never shows in the conversation list.
    let listed = db.list_conversations(&Default::default()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, chat.id);
    assert_eq!(db.kb_list().unwrap().len(), 1);
    assert_eq!(db.kb_rename(&kb.id, "Docs").unwrap().name, "Docs");

    db.kb_attach(&chat.id, &kb.id).unwrap();
    db.kb_attach(&chat.id, &kb.id).unwrap();
    assert_eq!(db.kb_attached(&chat.id).unwrap().len(), 1);
    assert!(db.kb_attach("missing", &kb.id).is_err());
    assert!(db.kb_attach(&chat.id, "missing").is_err());
    let doc = db
        .rag_create_document_limited(&kb.id, "a.md", Some("/tmp/notes/a.md"), 2)
        .unwrap();
    db.rag_create_document_limited(&kb.id, "b.md", None, 2)
        .unwrap();
    assert!(db
        .rag_create_document_limited(&kb.id, "c.md", None, 2)
        .is_err());
    assert!(db
        .kb_source_paths(&kb.id)
        .unwrap()
        .contains("/tmp/notes/a.md"));

    db.kb_detach(&chat.id, &kb.id).unwrap();
    assert!(db.kb_attached(&chat.id).unwrap().is_empty());
    db.kb_attach(&chat.id, &kb.id).unwrap();
    db.kb_delete(&kb.id).unwrap();
    assert!(db.kb_get(&kb.id).is_err());
    assert!(db.rag_document(&doc.id).is_err());
    assert!(db.kb_attached(&chat.id).unwrap().is_empty());
    // The chat itself stays.
    assert!(db.get_conversation(&chat.id).is_ok());
}

// ---- indexing ----

#[tokio::test]
async fn index_run_embeds_records_model_dims_and_time() {
    let (_d, db) = setup();
    let dir = corpus();
    let (embed, calls) = fake_embedder("fake-a").await;
    let (kb, run, events) = build(&db, dir.path(), Some(embed), false).await;
    assert_eq!(run.processed, 3);
    assert_eq!(run.failed, 0);
    assert!(!run.cancelled);
    assert_eq!(run.documents, 3);
    assert_eq!(run.chunks, run.embedded);
    assert!(run.chunks >= 3);
    assert_eq!(run.model.as_deref(), Some("fake-a"));
    assert_eq!(run.dims, Some(DIMS as i64));
    assert!(calls.load(Ordering::SeqCst) >= 1);
    // Model, size and time are stored in the base.
    assert_eq!(kb.embed_model.as_deref(), Some("fake-a"));
    assert_eq!(kb.embed_dims, Some(DIMS as i64));
    assert!(kb.indexed_at.is_some());
    assert!(kb.index_ms >= 0);
    let docs = db.rag_documents(&kb.id).unwrap();
    assert!(docs.iter().all(|d| d.status == "ready"));
    assert!(docs.iter().any(|d| d.name == "notes/deep/pasta.txt"));
    // Progress ends with `done` and counts every document.
    let last = events.last().unwrap();
    assert_eq!(last.status, "done");
    assert_eq!((last.docs_done, last.docs_total), (3, 3));
    assert!(events
        .iter()
        .any(|e| e.chunk_total > 0 && e.current.is_some()));
    assert!(!kb_jobs().is_running(&kb.id));
    let (state, _) = kb_state(&kb, &docs, Some("fake-a"), false);
    assert_eq!(state, KbState::Ready);

    // A second run with nothing new does no work and keeps the model.
    let again = run_again(&db, &kb.id, dir.path()).await;
    assert_eq!(again.processed, 0);
    assert_eq!(
        db.kb_get(&kb.id).unwrap().embed_model.as_deref(),
        Some("fake-a")
    );
}

async fn run_again(db: &Arc<StateDb>, kb_id: &str, dir: &Path) -> KbRun {
    let (embed, _) = fake_embedder("fake-a").await;
    run(db, kb_id, dir, Some(embed), false).await.0
}

#[tokio::test]
async fn new_files_are_added_to_an_existing_base() {
    let (_d, db) = setup();
    let dir = corpus();
    let (embed, _) = fake_embedder("fake-a").await;
    let (kb, _, _) = build(&db, dir.path(), Some(embed.clone()), false).await;
    write(
        dir.path(),
        "extra.md",
        "# Extra\n\nA new note about volcanoes and lava flows.",
    );
    let (second, _) = run(&db, &kb.id, dir.path(), Some(embed), false).await;
    assert_eq!(second.processed, 1);
    assert_eq!(second.documents, 4);
}

#[tokio::test]
async fn cancel_leaves_documents_resumable() {
    let (_d, db) = setup();
    let dir = corpus();
    let (embed, _) = fake_embedder("fake-a").await;
    let kb = db.kb_create("Corpus", None).unwrap();
    let scan = scan_folder(dir.path()).unwrap();
    let flag = kb_jobs().register(&kb.id);
    flag.store(true, Ordering::SeqCst);
    let out = index_kb(
        db.clone(),
        KbIndexRequest {
            kb_id: kb.id.clone(),
            files: scan.files,
            root: Some(dir.path().to_path_buf()),
            embed: Some(embed.clone()),
            rebuild: false,
        },
        flag,
        |_| {},
    )
    .await
    .unwrap();
    assert!(out.cancelled);
    assert_eq!(out.processed, 0);
    let docs = db.rag_documents(&kb.id).unwrap();
    assert_eq!(docs.len(), 3);
    assert!(docs.iter().all(|d| d.status == "cancelled"));
    let row = db.kb_get(&kb.id).unwrap();
    let (state, _) = kb_state(&row, &docs, Some("fake-a"), false);
    assert_eq!(state, KbState::Incomplete);

    // Resuming (no files given, the documents keep their source paths) finishes the work.
    let flag = kb_jobs().register(&kb.id);
    let out = index_kb(
        db.clone(),
        KbIndexRequest {
            kb_id: kb.id.clone(),
            files: Vec::new(),
            root: None,
            embed: Some(embed),
            rebuild: false,
        },
        flag,
        |_| {},
    )
    .await
    .unwrap();
    assert!(!out.cancelled);
    assert_eq!(out.processed, 3);
    let docs = db.rag_documents(&kb.id).unwrap();
    assert!(docs
        .iter()
        .all(|d| d.status == "ready" && d.embedded == d.chunks));
}

// ---- staleness ----

fn row(model: Option<&str>, dims: Option<i64>, indexed: bool) -> KnowledgeBaseRow {
    KnowledgeBaseRow {
        id: "kb".into(),
        name: "kb".into(),
        folder: None,
        embed_model: model.map(String::from),
        embed_dims: dims,
        index_ms: 5,
        indexed_at: indexed.then_some(1),
        created_at: 0,
        updated_at: 0,
    }
}

fn doc(
    status: &str,
    model: Option<&str>,
    dims: Option<i64>,
    chunks: i64,
    embedded: i64,
) -> RagDocument {
    RagDocument {
        id: format!("d-{status}-{chunks}-{embedded}"),
        conversation_id: "kb".into(),
        name: "a.md".into(),
        kind: "markdown".into(),
        source_path: None,
        size: 1,
        sha256: String::new(),
        pages: None,
        chars: 10,
        tokens: 4,
        chunks,
        embedded,
        status: status.into(),
        error: None,
        mode: "auto".into(),
        embed_model: model.map(String::from),
        embed_dims: dims,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn state_follows_the_embedding_model() {
    let ok = doc("ready", Some("a"), Some(8), 4, 4);
    let kb = row(Some("a"), Some(8), true);
    assert_eq!(kb_state(&kb, &[], Some("a"), false).0, KbState::Empty);
    assert_eq!(
        kb_state(&kb, std::slice::from_ref(&ok), Some("a"), false),
        (KbState::Ready, None)
    );
    // No embedding model in use right now: the index is not judged by it.
    assert_eq!(
        kb_state(&kb, std::slice::from_ref(&ok), None, false).0,
        KbState::Ready
    );
    // Another model: stale, rebuild offered.
    assert_eq!(
        kb_state(&kb, std::slice::from_ref(&ok), Some("b"), false),
        (KbState::Stale, Some("model"))
    );
    // A document with another vector size: stale by size.
    let odd = doc("ready", Some("a"), Some(16), 4, 4);
    assert_eq!(
        kb_state(&kb, &[ok.clone(), odd], Some("a"), false),
        (KbState::Stale, Some("dims"))
    );
    // Built with keywords only, an embedding model exists now: stale.
    let kw = row(None, None, true);
    let kw_doc = doc("ready", None, None, 4, 0);
    assert_eq!(
        kb_state(&kw, std::slice::from_ref(&kw_doc), Some("a"), false),
        (KbState::Stale, Some("model"))
    );
    assert_eq!(
        kb_state(&kw, std::slice::from_ref(&kw_doc), None, false).0,
        KbState::Ready
    );
    // Failed, cancelled and half-embedded documents make it incomplete.
    for d in [
        doc("failed", None, None, 0, 0),
        doc("cancelled", Some("a"), Some(8), 4, 2),
        doc("ready", Some("a"), Some(8), 4, 2),
    ] {
        assert_eq!(kb_state(&kb, &[d], Some("a"), false).0, KbState::Incomplete);
    }
    // A running job or an active document means indexing, before anything else.
    assert_eq!(
        kb_state(&kb, std::slice::from_ref(&ok), Some("b"), true).0,
        KbState::Indexing
    );
    assert_eq!(
        kb_state(
            &kb,
            &[doc("embedding", Some("a"), Some(8), 4, 1)],
            Some("a"),
            false
        )
        .0,
        KbState::Indexing
    );
}

#[tokio::test]
async fn changing_the_model_marks_the_base_stale_and_a_rebuild_fixes_it() {
    let (_d, db) = setup();
    let dir = corpus();
    let (a, _) = fake_embedder("fake-a").await;
    let (kb, _, _) = build(&db, dir.path(), Some(a), false).await;
    assert_eq!(
        get(&db, &kb.id, Some("fake-a")).unwrap().state,
        KbState::Ready
    );
    let stale = get(&db, &kb.id, Some("fake-b")).unwrap();
    assert_eq!(stale.state, KbState::Stale);
    assert_eq!(stale.stale_reason.as_deref(), Some("model"));

    let (b, _) = fake_embedder("fake-b").await;
    let (out, _) = run(&db, &kb.id, dir.path(), Some(b), true).await;
    assert_eq!(out.processed, 3);
    assert_eq!(out.model.as_deref(), Some("fake-b"));
    let fresh = get(&db, &kb.id, Some("fake-b")).unwrap();
    assert_eq!(fresh.state, KbState::Ready);
    assert_eq!(fresh.row.embed_model.as_deref(), Some("fake-b"));
    assert_eq!(fresh.embedded, fresh.chunks);
    // The same base under the old model is stale the other way round.
    assert_eq!(
        get(&db, &kb.id, Some("fake-a")).unwrap().state,
        KbState::Stale
    );
}

// ---- retrieval ----

async fn chat_with(db: &Arc<StateDb>, kb: &str) -> String {
    let chat = db.create_conversation(&NewConversation::default()).unwrap();
    db.kb_attach(&chat.id, kb).unwrap();
    chat.id
}

#[tokio::test]
async fn retrieval_ranks_the_matching_document_first_with_cited_sources() {
    let (_d, db) = setup();
    let dir = corpus();
    let (embed, _) = fake_embedder("fake-a").await;
    let (kb, _, _) = build(&db, dir.path(), Some(embed.clone()), false).await;
    let chat = chat_with(&db, &kb.id).await;
    for (question, expect) in [
        (
            "How does the borrow checker and ownership work?",
            "notes/rust.md",
        ),
        ("What do cats do with whiskers and mice?", "cats.md"),
        (
            "Cook spaghetti with tomato sauce and basil",
            "notes/deep/pasta.txt",
        ),
    ] {
        let prepared = prepare(
            &db,
            &chat,
            &RetrieveOptions {
                query: question.into(),
                top_k: 3,
                context_tokens: Some(8192),
                embed: Some(embed.clone()),
                rerank: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(prepared.retrieval.as_deref(), Some("hybrid"), "{question}");
        assert!(!prepared.sources.is_empty());
        assert_eq!(prepared.sources[0].document_name, expect, "{question}");
        assert_eq!(prepared.sources[0].n, 1);
        assert!(prepared.sources[0].similarity.unwrap() > 0.0);
        assert!(prepared.sources.iter().all(|s| !s.full));
        // Knowledge base documents are never put in whole, however small.
        assert!(prepared.system.is_none());
        let user = prepared.user.unwrap();
        assert!(user.contains("[1]") && user.contains(question));
        assert!(prepared.notices.is_empty(), "{:?}", prepared.notices);
    }
}

#[tokio::test]
async fn retrieval_without_a_model_uses_keywords_and_a_stale_base_says_so() {
    let (_d, db) = setup();
    let dir = corpus();
    let (embed, _) = fake_embedder("fake-a").await;
    let (kb, _, _) = build(&db, dir.path(), Some(embed), false).await;
    let chat = chat_with(&db, &kb.id).await;
    let keyword = prepare(
        &db,
        &chat,
        &RetrieveOptions {
            query: "spaghetti".into(),
            top_k: 2,
            context_tokens: Some(8192),
            embed: None,
            rerank: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(keyword.retrieval.as_deref(), Some("keyword"));
    assert_eq!(keyword.sources[0].document_name, "notes/deep/pasta.txt");

    // Another embedding model than the base was built with: a notice names the base.
    let (other, _) = fake_embedder("fake-b").await;
    let stale = prepare(
        &db,
        &chat,
        &RetrieveOptions {
            query: "spaghetti".into(),
            top_k: 2,
            context_tokens: Some(8192),
            embed: Some(other),
            rerank: None,
        },
    )
    .await
    .unwrap();
    assert!(stale
        .notices
        .iter()
        .any(|n| n.contains("rag.notice_kb_stale")));
    assert_eq!(stale.sources[0].document_name, "notes/deep/pasta.txt");
}

#[tokio::test]
async fn excerpts_of_a_base_stay_within_the_token_budget() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    // Many long, similar chunks: the question matches all of them.
    let para = "The reactor coolant pump circulates water through the primary loop. ".repeat(14);
    let text = (0..12)
        .map(|i| format!("## Section {i}\n\n{para}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    write(dir.path(), "plant.md", &text);
    let (embed, _) = fake_embedder("fake-a").await;
    let (kb, _, _) = build(&db, dir.path(), Some(embed.clone()), false).await;
    let chat = chat_with(&db, &kb.id).await;
    let ask = |ctx: usize| {
        let db = db.clone();
        let chat = chat.clone();
        let embed = embed.clone();
        async move {
            prepare(
                &db,
                &chat,
                &RetrieveOptions {
                    query: "reactor coolant pump primary loop".into(),
                    top_k: 12,
                    context_tokens: Some(ctx),
                    embed: Some(embed),
                    rerank: None,
                },
            )
            .await
            .unwrap()
        }
    };
    let small = ask(4096).await;
    let large = ask(65536).await;
    assert!(!small.sources.is_empty());
    assert!(
        small.tokens <= kb::token_budget(4096) + 400,
        "{}",
        small.tokens
    );
    assert!(large.sources.len() > small.sources.len());
    assert!(large.tokens <= KB_MAX_TOKENS + 400, "{}", large.tokens);
}

#[test]
fn budget_keeps_the_best_and_at_least_one() {
    assert_eq!(token_budget(1000), 300);
    assert_eq!(token_budget(4096), 1024);
    assert_eq!(token_budget(1_000_000), KB_MAX_TOKENS);
    let items = vec![400usize, 400, 400, 400];
    assert_eq!(fit_budget(items.clone(), |t| *t, 1000).len(), 2);
    assert_eq!(fit_budget(items.clone(), |t| *t, 10).len(), 1);
    assert_eq!(fit_budget(Vec::<usize>::new(), |t| *t, 10).len(), 0);
    assert_eq!(fit_budget(items, |t| *t, 10_000).len(), 4);
}

#[test]
fn small_context_models_cut_long_inputs() {
    // multilingual-e5-small has 512 tokens: 1024 characters.
    assert_eq!(max_chars_for_context(Some(512)), Some(1024));
    assert_eq!(max_chars_for_context(Some(8192)), None);
    assert_eq!(max_chars_for_context(None), None);
    let m = EmbedModel {
        endpoint: Endpoint {
            base_url: String::new(),
            token: None,
        },
        alias: "x".into(),
        query_prefix: String::new(),
        doc_prefix: String::new(),
        max_chars: Some(5),
    };
    assert_eq!(m.clip("héllo wörld"), "héllo");
    assert_eq!(m.clip("abc"), "abc");
}

#[tokio::test]
async fn a_text_longer_than_the_models_context_is_cut_instead_of_failing_the_document() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    // One long dense paragraph: its chunk is far above the fake server's 600 characters.
    let dense = "alpha=beta;gamma(delta)[epsilon]{zeta}<eta>#theta ".repeat(40);
    write(
        dir.path(),
        "dense.md",
        &format!(
            "# Dense

{dense}"
        ),
    );
    let (embed, calls) = fake_embedder_limited("fake-a", 600).await;
    let (kb, run, _) = build(&db, dir.path(), Some(embed), false).await;
    assert_eq!(run.failed, 0, "{:?}", db.rag_documents(&kb.id).unwrap());
    assert!(run.chunks >= 1 && run.chunks == run.embedded);
    // The first request was refused, the retries cut the texts until the server took them.
    assert!(calls.load(Ordering::SeqCst) >= 2);
}
