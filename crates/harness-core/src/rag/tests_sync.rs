//! Change detection, portable sources, media and questions of knowledge bases against a real
//! `state.db`, a fake embedding server and a fake gateway that answers speech recognition.

use super::kb::*;
use super::media::AsrModel;
use super::*;
use crate::state::StateDb;
use axum::{body::Bytes, http::header, routing::post, Router};
use std::path::Path;

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

const CATS: &str =
    "# Cats\n\nCats purr, groom their whiskers and hunt mice at night while the kitten plays.";
const RUST: &str =
    "# Rust\n\nRust enforces ownership and the borrow checker at compile time without a collector.";

/// The fake gateway: `/v1/embeddings` (hashed words) and `/v1/audio/transcriptions`.
async fn fake_gateway() -> (Endpoint, Arc<Mutex<Vec<String>>>) {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    let app = Router::new()
        .route(
            "/v1/embeddings",
            post(|body: Bytes| async move {
                let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let data: Vec<serde_json::Value> = req["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let mut v = vec![0.0f32; 64];
                        for w in t.as_str().unwrap().to_lowercase().split_whitespace() {
                            let h = w.bytes().fold(7u64, |h, b| h.wrapping_mul(31) + b as u64);
                            v[(h % 64) as usize] += 1.0;
                        }
                        serde_json::json!({"index": i, "embedding": v})
                    })
                    .collect();
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    serde_json::json!({ "data": data }).to_string(),
                )
            }),
        )
        .route(
            "/v1/audio/transcriptions",
            post(move |headers: axum::http::HeaderMap, body: Bytes| {
                let log = log.clone();
                async move {
                    let text = String::from_utf8_lossy(&body).to_string();
                    let priority = headers
                        .get("x-harness-priority")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    log.lock().unwrap().push(format!(
                        "priority={priority} verbose={} model={}",
                        text.contains("verbose_json"),
                        text.contains("whisper-fake")
                    ));
                    (
                        [(header::CONTENT_TYPE, "application/json")],
                        serde_json::json!({
                            "text": "Bugün hava çok güzel. The quarterly budget meeting starts soon.",
                            "language": "turkish",
                            "duration": 12.5,
                            "segments": [
                                {"start": 0.0, "end": 4.0, "text": " Bugün hava çok güzel."},
                                {"start": 65.0, "end": 72.5, "text": " The quarterly budget meeting starts soon."}
                            ]
                        })
                        .to_string(),
                    )
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Endpoint {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            token: None,
        },
        seen,
    )
}

fn embedder(ep: &Endpoint, alias: &str) -> EmbedModel {
    EmbedModel {
        endpoint: ep.clone(),
        alias: alias.into(),
        query_prefix: String::new(),
        doc_prefix: String::new(),
        max_chars: None,
    }
}

async fn run_index(
    db: &Arc<StateDb>,
    kb_id: &str,
    embed: Option<EmbedModel>,
    rebuild: bool,
) -> KbRun {
    let mut req = build_request(db, kb_id, None, Vec::new()).unwrap();
    req.embed = embed;
    req.rebuild = rebuild;
    let flag = kb_jobs().register(kb_id);
    index_kb(db.clone(), req, flag, |_| {}).await.unwrap()
}

#[tokio::test]
async fn only_changed_files_are_read_again_and_vanished_ones_are_dropped() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    let cats = write(dir.path(), "cats.md", CATS);
    write(dir.path(), "notes/rust.md", RUST);
    let kb = db
        .kb_create("Corpus", Some(&dir.path().to_string_lossy()))
        .unwrap();

    let first = run_index(&db, &kb.id, None, false).await;
    assert_eq!(
        (first.added, first.changed, first.unchanged, first.removed),
        (2, 0, 0, 0)
    );
    assert_eq!(first.processed, 2);
    let docs = db.rag_documents(&kb.id).unwrap();
    assert!(docs
        .iter()
        .all(|d| d.mtime_ms.is_some() && d.rel_path.is_some()));
    assert!(docs
        .iter()
        .any(|d| d.rel_path.as_deref() == Some("notes/rust.md")));

    // Nothing changed: nothing is read.
    let second = run_index(&db, &kb.id, None, false).await;
    assert_eq!((second.added, second.changed, second.unchanged), (0, 0, 2));
    assert_eq!(second.processed, 0);

    // Same bytes, newer modification time: the hash keeps the document.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options()
        .write(true)
        .open(&cats)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let third = run_index(&db, &kb.id, None, false).await;
    assert_eq!((third.changed, third.unchanged, third.processed), (0, 2, 0));
    let touched = db
        .rag_documents(&kb.id)
        .unwrap()
        .into_iter()
        .find(|d| d.name == "cats.md")
        .unwrap();
    assert_eq!(
        touched.mtime_ms,
        mtime_ms(&cats),
        "the new time is remembered"
    );

    // New content: that document is read again, the other is not.
    write(
        dir.path(),
        "cats.md",
        "# Cats\n\nA completely different text about dogs and bones.",
    );
    let fourth = run_index(&db, &kb.id, None, false).await;
    assert_eq!(
        (fourth.changed, fourth.unchanged, fourth.processed),
        (1, 1, 1)
    );
    let text = db
        .rag_content(
            &db.rag_documents(&kb.id)
                .unwrap()
                .iter()
                .find(|d| d.name == "cats.md")
                .unwrap()
                .id,
        )
        .unwrap();
    assert!(text.contains("dogs and bones") && !text.contains("whiskers"));

    // A file that is gone drops its document and its chunks.
    std::fs::remove_file(dir.path().join("notes/rust.md")).unwrap();
    let fifth = run_index(&db, &kb.id, None, false).await;
    assert_eq!((fifth.removed, fifth.unchanged, fifth.changed), (1, 1, 0));
    let names: Vec<String> = db
        .rag_documents(&kb.id)
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["cats.md".to_string()]);
    let stats = db.kb_get(&kb.id).unwrap().stats.last_run.unwrap();
    assert_eq!((stats.removed, stats.unchanged), (1, 1));
}

#[tokio::test]
async fn the_index_survives_moving_the_folder() {
    let (_d, db) = setup();
    let (ep, _) = fake_gateway().await;
    let old = tempfile::tempdir().unwrap();
    write(old.path(), "a/cats.md", CATS);
    write(old.path(), "rust.md", RUST);
    let kb = db
        .kb_create("Moved", Some(&old.path().to_string_lossy()))
        .unwrap();
    let embed = embedder(&ep, "e5-test");
    let built = run_index(&db, &kb.id, Some(embed.clone()), false).await;
    assert_eq!(built.embedded, built.chunks);
    assert_eq!(
        db.kb_get(&kb.id).unwrap().embed_model.as_deref(),
        Some("e5-test")
    );

    // Copy the folder elsewhere (same bytes; copies get a new modification time) and point the
    // base at it: relative paths still match, so nothing is read or embedded again.
    let new = tempfile::tempdir().unwrap();
    for rel in ["a/cats.md", "rust.md"] {
        let from = old.path().join(rel);
        write(new.path(), rel, &std::fs::read_to_string(from).unwrap());
    }
    drop(old);
    db.kb_update(
        &kb.id,
        &crate::state::KbUpdate {
            folder: Some(new.path().to_string_lossy().to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    let again = run_index(&db, &kb.id, Some(embed), false).await;
    assert_eq!((again.added, again.changed, again.removed), (0, 0, 0));
    assert_eq!(again.unchanged, 2);
    assert_eq!(again.processed, 0);
    let docs = db.rag_documents(&kb.id).unwrap();
    assert!(docs.iter().all(|d| d
        .source_path
        .as_deref()
        .is_some_and(|p| Path::new(p).starts_with(new.path()))));
}

#[tokio::test]
async fn a_new_embedding_model_makes_the_index_stale_and_reembeds_only_vectors() {
    let (_d, db) = setup();
    let (ep, _) = fake_gateway().await;
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "cats.md", CATS);
    let kb = db
        .kb_create("S", Some(&dir.path().to_string_lossy()))
        .unwrap();
    run_index(&db, &kb.id, Some(embedder(&ep, "model-a")), false).await;
    let k = kb::get(&db, &kb.id, Some("model-a")).unwrap();
    assert_eq!(k.state, KbState::Ready);
    let k = kb::get(&db, &kb.id, Some("model-b")).unwrap();
    assert_eq!(
        (k.state, k.stale_reason.as_deref()),
        (KbState::Stale, Some("model"))
    );
    assert_eq!(
        k.row.embed_model.as_deref(),
        Some("model-a"),
        "the model id is stored"
    );

    let run = run_index(&db, &kb.id, Some(embedder(&ep, "model-b")), false).await;
    // The text is kept (nothing changed on disk); only the vectors are made again.
    assert_eq!((run.changed, run.unchanged), (0, 1));
    assert!(run.embed_ms > 0 || run.embedded == run.chunks);
    let k = kb::get(&db, &kb.id, Some("model-b")).unwrap();
    assert_eq!(k.state, KbState::Ready);
}

#[tokio::test]
async fn audio_is_transcribed_with_times_and_cited_by_time() {
    let (_d, db) = setup();
    let (ep, seen) = fake_gateway().await;
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "notes.md", RUST);
    // The bytes are not decoded here: the fake gateway answers for any file.
    std::fs::write(
        dir.path().join("meeting.wav"),
        b"RIFF....WAVEfake audio bytes",
    )
    .unwrap();
    let kb = db
        .kb_create("Media", Some(&dir.path().to_string_lossy()))
        .unwrap();

    // Without a speech model the audio file is left out of the scan.
    let scan = scan_folder_media(dir.path(), MediaScan::default()).unwrap();
    assert_eq!(scan.files.len(), 1);
    assert_eq!(scan.skipped, 0);

    db.kb_update(
        &kb.id,
        &crate::state::KbUpdate {
            asr_model: Some("whisper-fake".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let mut req = build_request(&db, &kb.id, None, Vec::new()).unwrap();
    assert_eq!(req.files.len(), 2);
    req.embed = Some(embedder(&ep, "e5-test"));
    req.asr = Some(AsrModel {
        endpoint: ep.clone(),
        alias: "whisper-fake".into(),
    });
    let flag = kb_jobs().register(&kb.id);
    let run = index_kb(db.clone(), req, flag, |_| {}).await.unwrap();
    assert_eq!((run.added, run.failed), (2, 0));
    assert_eq!(run.embedded, run.chunks);
    // Background priority, verbose_json, the model alias.
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["priority=background verbose=true model=true"]
    );

    let docs = db.rag_documents(&kb.id).unwrap();
    let wav = docs.iter().find(|d| d.name == "meeting.wav").unwrap();
    assert_eq!(wav.kind, "audio");
    let meta = wav.meta.clone().unwrap();
    assert_eq!(
        (meta.duration_ms, meta.language.as_deref(), meta.segments),
        (Some(12_500), Some("turkish"), Some(2))
    );

    // A question finds the segment and the citation shows its time span.
    let answer = kb::ask(
        &db,
        &kb.id,
        &RetrieveOptions {
            query: "quarterly budget meeting".into(),
            top_k: 3,
            embed: Some(embedder(&ep, "e5-test")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let hit = answer
        .sources
        .iter()
        .find(|s| s.document_name == "meeting.wav")
        .expect("the transcript is found");
    assert_eq!(hit.t_start_ms, Some(0));
    assert_eq!(hit.t_end_ms, Some(72_500));
    assert_eq!(place_label(hit).as_deref(), Some("0:00-1:12"));
    assert!(hit.text.contains("quarterly budget"));
    assert!(answer.latency_ms < 5000);
    let k = db.kb_get(&kb.id).unwrap();
    assert_eq!(k.stats.query_count, 1);
    assert_eq!(k.stats.query_ms_last, Some(answer.latency_ms));
}

#[tokio::test]
async fn the_rerank_step_follows_the_bases_setting() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "cats.md", CATS);
    write(dir.path(), "rust.md", RUST);
    let kb = db
        .kb_create("R", Some(&dir.path().to_string_lossy()))
        .unwrap();
    run_index(&db, &kb.id, None, false).await;

    // A fake reranker that prefers the longest text.
    let app = Router::new().route(
        "/v1/rerank",
        post(|body: Bytes| async move {
            let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let docs = req["documents"].as_array().unwrap();
            let results: Vec<serde_json::Value> = docs
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    serde_json::json!({"index": i, "relevance_score": d.as_str().unwrap().len() as f64 / 1000.0})
                })
                .collect();
            (
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::json!({ "results": results }).to_string(),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let ep = Endpoint {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        token: None,
    };
    let mut opts = RetrieveOptions {
        query: "cats rust ownership whiskers".into(),
        top_k: 2,
        rerank: Some((ep.clone(), "rerank-fake".into())),
        ..Default::default()
    };
    let a = kb::ask(&db, &kb.id, &opts).await.unwrap();
    assert_eq!(a.reranked.as_deref(), Some("rerank-fake"));
    assert!(a.sources.iter().all(|s| s.rerank_score.is_some()));
    opts.rerank = None;
    let a = kb::ask(&db, &kb.id, &opts).await.unwrap();
    assert_eq!(a.reranked, None, "off unless the caller or the base asks");
}

#[test]
fn sources_are_added_and_removed_by_name_id_or_path() {
    let (_d, db) = setup();
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    write(dir.path(), "a.md", CATS);
    let b = write(other.path(), "b.md", RUST);
    let kb = db.kb_create("Src", None).unwrap();

    // The first folder becomes the base's folder; a file is added on its own.
    let extra = add_sources(
        &db,
        &kb.id,
        Some(&dir.path().to_string_lossy()),
        &[b.to_string_lossy().to_string()],
    )
    .unwrap();
    assert_eq!(extra.len(), 1);
    assert_eq!(
        db.kb_get(&kb.id).unwrap().folder.as_deref(),
        Some(dir.path().to_string_lossy().as_ref())
    );
    let req = build_request(&db, &kb.id, None, extra).unwrap();
    assert_eq!(req.files.len(), 2);
    let synced = sync_sources(&db, &req).unwrap();
    assert_eq!(synced.added, 2);
    let docs = db.rag_documents(&kb.id).unwrap();
    assert_eq!(docs.iter().filter(|d| d.rel_path.is_some()).count(), 1);

    assert_eq!(remove_sources(&db, &kb.id, &["a.md".into()]).unwrap(), 1);
    assert_eq!(
        remove_sources(&db, &kb.id, &[b.to_string_lossy().to_string()]).unwrap(),
        1
    );
    assert!(db.rag_documents(&kb.id).unwrap().is_empty());
}
