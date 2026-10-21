//! A knowledge base over a mixed folder (PDF, Markdown, text, a short WAV with speech) through
//! the real gateway with the real recommended embedding model (multilingual-e5-small, CPU) and a
//! real whisper-server: the audio is transcribed with timestamps and cited by time, questions in
//! Turkish and English find the right document, and a second run reads only what changed.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_EMBED_MODEL=<multilingual-e5-small-q8_0.gguf>
//! HS_WHISPER_SERVER=<whisper-server.exe> HS_WHISPER_MODEL=<ggml-*.bin> HS_WHISPER_WAV=<speech.wav>
//! HS_WHISPER_TEXT=<a word the wav says, lower case>
//! cargo test -p harness-gateway --test kb_media_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, gateway on 12470 and its model on
//! the port right after it. The test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::rag::kb::{self, KbState};
use harness_core::rag::media::AsrModel;
use harness_core::rag::{self, EmbedModel, Endpoint, RetrieveOptions};
use harness_core::servers::ServerRegistry;
use harness_core::state::StateDb;
use harness_gateway::{GatewayContext, GatewayServer};

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../harness-core/tests/data/rag")
        .join(rel)
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[tokio::test]
#[ignore]
async fn e2e_mixed_folder_with_audio_change_detection_and_citations() {
    let (Some(bin), Some(model), Some(wserver), Some(wmodel), Some(wav)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_EMBED_MODEL"),
        env_path("HS_WHISPER_SERVER"),
        env_path("HS_WHISPER_MODEL"),
        env_path("HS_WHISPER_WAV"),
    ) else {
        return;
    };
    let word = std::env::var("HS_WHISPER_TEXT").unwrap_or_else(|_| "budget".into());

    harness_core::secrets::use_memory_store();
    let root = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("kb-media-e2e"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(root.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.runtime_mode = harness_core::config::RuntimeMode::Custom;
    s.custom_binary.binary_path = Some(bin.clone());
    s.custom_binary.is_valid = true;
    s.server.port = 12480;
    s.control_api.port = 12489;
    s.save().unwrap();

    // The whisper runtime, registered the way a user would (a local folder).
    harness_core::runtimes::RuntimeRegistry::global()
        .add_local(&harness_core::runtimes::AddLocal {
            path: wserver.parent().unwrap().to_path_buf(),
            name: "whisper kb e2e".into(),
            id: None,
            copy: false,
            family: "whisper.cpp".into(),
        })
        .unwrap();
    let mut asr_def = ModelDefinition::new("kb-whisper", ModelRef::Local { path: wmodel });
    asr_def.ttl_seconds = Some(20);
    harness_core::domain::store::Domain::global()
        .save_definition(asr_def)
        .unwrap();
    let mut def = ModelDefinition::new("e5-small", ModelRef::Local { path: model });
    assert!(rag::embedder::apply_cpu_default(&mut def));
    let def = harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: root.clone(),
        downloads: None,
    };
    let server = GatewayServer::new();
    let status = server
        .start(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: 12470,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let endpoint = Endpoint {
        base_url: status.endpoint.unwrap().base_url,
        token: None,
    };
    let embed = EmbedModel::for_definition(endpoint.clone(), &def);

    // The mixed folder.
    let folder = tempfile::tempdir().unwrap();
    let f = folder.path();
    std::fs::create_dir_all(f.join("docs")).unwrap();
    std::fs::copy(
        fixture("state-db-schema.pdf"),
        f.join("docs/state-db-schema.pdf"),
    )
    .unwrap();
    std::fs::create_dir_all(f.join("talk")).unwrap();
    std::fs::copy(&wav, f.join("talk/meeting.wav")).unwrap();
    write(
        f,
        "notes/photosynthesis.md",
        "# Photosynthesis\n\nPlants capture sunlight with chlorophyll in their leaves. They use the light energy to turn carbon dioxide and water into glucose and release oxygen. This process is the base of almost every food chain on Earth.",
    );
    write(
        f,
        "notes/yemek.txt",
        "Mercimek çorbası için önce soğanı ve havucu yağda kavurun, sonra kırmızı mercimeği ekleyip su ile pişirin. Çorbayı blenderdan geçirip limon ve pul biberle servis edin.",
    );

    let db = Arc::new(StateDb::open(&root.join("state.db")).unwrap());
    let row = db
        .kb_create("Mixed E2E", Some(&f.to_string_lossy()))
        .unwrap();
    db.kb_update(
        &row.id,
        &harness_core::state::KbUpdate {
            asr_model: Some("kb-whisper".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let asr = AsrModel {
        endpoint: endpoint.clone(),
        alias: "kb-whisper".into(),
    };

    let run_once = |rebuild: bool| {
        let (db, embed, asr, id) = (db.clone(), embed.clone(), asr.clone(), row.id.clone());
        async move {
            let mut req = kb::build_request(&db, &id, None, Vec::new()).unwrap();
            req.embed = Some(embed);
            req.asr = Some(asr);
            req.rebuild = rebuild;
            let flag = kb::kb_jobs().register(&id);
            let events = Arc::new(Mutex::new(Vec::new()));
            let ev = events.clone();
            let run = kb::index_kb(db, req, flag, move |p| ev.lock().unwrap().push(p))
                .await
                .unwrap();
            let stages: Vec<String> = events.lock().unwrap().iter().map(|p| p.stage.clone()).fold(
                Vec::new(),
                |mut v, s| {
                    if v.last() != Some(&s) {
                        v.push(s);
                    }
                    v
                },
            );
            (run, stages)
        }
    };

    let (first, stages) = run_once(false).await;
    eprintln!("RUN 1: {first:?}\nstages: {stages:?}");
    assert_eq!((first.added, first.failed, first.documents), (4, 0, 4));
    assert_eq!(first.chunks, first.embedded);
    assert_eq!(first.model.as_deref(), Some("e5-small"));
    // The stages ran apart: reading (incl. the transcript) before embedding.
    assert!(stages.iter().position(|s| s == "extract") < stages.iter().position(|s| s == "embed"));
    let docs = db.rag_documents(&row.id).unwrap();
    for d in &docs {
        eprintln!(
            "doc {}: kind {} {} chunks, mtime {:?}, rel {:?}, meta {:?}",
            d.name, d.kind, d.chunks, d.mtime_ms, d.rel_path, d.meta
        );
    }
    let wav_doc = docs.iter().find(|d| d.name == "talk/meeting.wav").unwrap();
    assert_eq!(wav_doc.kind, "audio");
    let meta = wav_doc.meta.clone().unwrap();
    assert!(meta.duration_ms.unwrap_or(0) > 500, "{meta:?}");
    let k = kb::get(&db, &row.id, Some("e5-small")).unwrap();
    assert_eq!(k.state, KbState::Ready);
    let run = k.row.stats.last_run.clone().unwrap();
    eprintln!(
        "STATS: read {} ms, embed {} ms, cpu avg {:.0}% peak {:.0}%, rss peak {:.0} MiB, disk text {} B vectors {} B",
        run.extract_ms,
        run.embed_ms,
        run.cpu_avg_pct,
        run.cpu_peak_pct,
        run.rss_peak_bytes as f64 / 1048576.0,
        k.disk.text_bytes,
        k.disk.vector_bytes
    );
    assert!(k.disk.vector_bytes > 0 && k.disk.text_bytes > 0);

    // Questions in both languages, through the same retrieval a chat uses.
    let ask = |q: &'static str| {
        let (db, embed, id) = (db.clone(), embed.clone(), row.id.clone());
        async move {
            let a = kb::ask(
                &db,
                &id,
                &RetrieveOptions {
                    query: q.into(),
                    top_k: 4,
                    embed: Some(embed),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            eprintln!(
                "Q ({} ms): {q}\n  -> {:?}",
                a.latency_ms,
                a.sources
                    .iter()
                    .map(|s| (s.document_name.clone(), rag::place_label(s), s.similarity))
                    .collect::<Vec<_>>()
            );
            assert_eq!(a.retrieval.as_deref(), Some("hybrid"));
            a
        }
    };
    let tr = ask("Bitkiler güneş ışığını nasıl enerjiye dönüştürür?").await;
    assert_eq!(best(&tr), "notes/photosynthesis.md");
    let en = ask("How do I cook red lentil soup?").await;
    assert_eq!(best(&en), "notes/yemek.txt");
    // The audio: found by something it says, cited with its time span.
    let q: &'static str =
        Box::leak(format!("What was said about {word} in the recording?").into_boxed_str());
    let audio = ask(q).await;
    let hit = audio
        .sources
        .iter()
        .find(|s| s.document_name == "talk/meeting.wav")
        .expect("the transcript is among the excerpts");
    assert!(hit.t_start_ms.is_some() && hit.t_end_ms.is_some());
    eprintln!("audio citation: {:?} {}", rag::place_label(hit), hit.text);
    assert!(hit.text.to_lowercase().contains(&word), "{}", hit.text);
    let k = db.kb_get(&row.id).unwrap();
    assert_eq!(k.stats.query_count, 3);

    // A second run reads only what changed: one note edited, one deleted, the PDF and the audio
    // are not touched (the speech model is not asked again).
    write(
        f,
        "notes/yemek.txt",
        "Domates çorbası için domatesleri rendeleyin, tereyağında kavurun ve un ekleyin; sıcak süt ile pişirin.",
    );
    std::fs::remove_file(f.join("notes/photosynthesis.md")).unwrap();
    let (second, _) = run_once(false).await;
    eprintln!("RUN 2: {second:?}");
    assert_eq!(
        (
            second.added,
            second.changed,
            second.unchanged,
            second.removed
        ),
        (0, 1, 2, 1)
    );
    assert_eq!(second.processed, 1);
    assert_eq!(second.failed, 0);
    assert!(
        second.elapsed_ms < first.elapsed_ms,
        "{} !< {}",
        second.elapsed_ms,
        first.elapsed_ms
    );
    let tomato = ask("Nasıl domates çorbası yapılır?").await;
    assert_eq!(best(&tomato), "notes/yemek.txt");
    assert!(tomato
        .sources
        .iter()
        .all(|s| s.document_name != "notes/photosynthesis.md"));

    // Another embedding model in use: stale, with the model id stored in the index.
    let stale = kb::get(&db, &row.id, Some("other-embedder")).unwrap();
    assert_eq!(stale.state, KbState::Stale);
    assert_eq!(stale.row.embed_model.as_deref(), Some("e5-small"));

    server.stop().await;
    registry.stop_all().await;
}

/// The document of the excerpt with the best dense similarity.
fn best(a: &kb::KbAnswer) -> String {
    a.sources
        .iter()
        .filter(|s| s.similarity.is_some())
        .max_by(|x, y| x.similarity.partial_cmp(&y.similarity).unwrap())
        .unwrap()
        .document_name
        .clone()
}
