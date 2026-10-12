//! Serving mode of a launch and the launch-time adjustments that follow from the model file.
//!
//! * **Mode:** a llama-server run serves chat/completions (default), embeddings
//!   (`--embeddings`) or reranking (`--rerank`, RANK pooling). The mode is chosen with the
//!   Harness-level setting [`SERVE_MODE`] (`"chat"`, `"embedding"`, `"rerank"`; absent =
//!   automatic). Automatic follows explicitly set `embeddings`/`rerank` catalog values, else
//!   the file kind from its GGUF header (`domain::detect_kind`). The key lives in
//!   `LaunchConfig.params` / `ModelDefinition.load` like a catalog value, so it is layered,
//!   saved and shown like one, but it is never rendered as a flag.
//! * **Pooling** is not sent: llama-server reads `<arch>.pooling_type` from the GGUF itself
//!   (nomic-embed: mean) and `--rerank` selects RANK pooling. A `pooling` value set by the
//!   user is still sent.
//! * **Batch size** for embedding/rerank runs: llama-server clamps `n_batch` to `n_ubatch`
//!   (default 512) for embeddings (checked on b11236: "setting n_batch = n_ubatch = 512"), so
//!   longer inputs would fail (a non-causal model needs the whole input in one ubatch; RAG
//!   chunks with dense markdown tables exceed 512 tokens and the rerank request got a 500).
//!   When the user set no ubatch, both are raised to the model's training context (at most
//!   the launch context and [`MAX_EMBED_BATCH`]), and to at least [`MIN_EMBED_BATCH`]
//!   (also when the header has no context length).
//! * **Draft model:** b11236 loads `-md` but drafts nothing until `--spec-type draft-simple`
//!   is given (checked: `draft_n` appears in `timings` only with it). When a draft model is
//!   set, `spec_type` is unset and the runtime lists `draft-simple`, it is added.

use crate::capabilities::RuntimeCapabilities;
use crate::domain::{detect_kind, ModelKind};
use crate::gguf::GgufMetadata;
use crate::params::{ParamValue, ParamValues};
use crate::process::LaunchConfig;
use serde::{Deserialize, Serialize};

/// Harness-level launch setting (not a llama-server flag).
pub const SERVE_MODE: &str = "serve_mode";
/// Upper bound of the automatic embedding/rerank batch size.
pub const MAX_EMBED_BATCH: u32 = 8192;
/// Lower bound of the automatic embedding/rerank batch size (llama-server's default
/// `n_ubatch` is 512, too small for a chunk with a heading).
pub const MIN_EMBED_BATCH: u32 = 2048;
/// `--spec-type` value of classic draft-model speculative decoding.
const SPEC_DRAFT_SIMPLE: &str = "draft-simple";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServeMode {
    #[default]
    Chat,
    Embedding,
    Rerank,
    /// whisper-server: audio file in, text out (`/inference`; the gateway's
    /// `/v1/audio/transcriptions`).
    Transcription,
    /// sd-server: image (and video) generation (`/sdcpp/v1/img_gen`; the gateway's
    /// `/v1/images/generations`).
    Image,
}

impl ServeMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "chat" => Some(Self::Chat),
            "embedding" => Some(Self::Embedding),
            "rerank" => Some(Self::Rerank),
            "transcription" => Some(Self::Transcription),
            "image" => Some(Self::Image),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Embedding => "embedding",
            Self::Rerank => "rerank",
            Self::Transcription => "transcription",
            Self::Image => "image",
        }
    }

    /// The mode a file of this kind is served in.
    pub fn for_kind(kind: ModelKind) -> Self {
        match kind {
            ModelKind::Embedding => Self::Embedding,
            ModelKind::Reranker => Self::Rerank,
            ModelKind::Asr => Self::Transcription,
            ModelKind::Image => Self::Image,
            _ => Self::Chat,
        }
    }
}

/// The mode chosen in `params` (`None` = automatic, also for unknown values).
pub fn requested(params: &ParamValues) -> Option<ServeMode> {
    match params.get(SERVE_MODE) {
        Some(ParamValue::Text(s)) => ServeMode::parse(s),
        _ => None,
    }
}

fn flag_on(params: &ParamValues, id: &str) -> bool {
    matches!(params.get(id), Some(ParamValue::Bool(true)))
}

/// The mode a launch runs in, without changing it.
pub fn effective(params: &ParamValues, kind: ModelKind, router_mode: bool) -> ServeMode {
    if router_mode {
        return ServeMode::Chat;
    }
    requested(params).unwrap_or_else(|| {
        if flag_on(params, "rerank") {
            ServeMode::Rerank
        } else if flag_on(params, "embeddings") {
            ServeMode::Embedding
        } else {
            ServeMode::for_kind(kind)
        }
    })
}

/// One adjustment [`prepare`] made (written to the server log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// Mode set from the file kind or an explicit choice.
    Mode { mode: ServeMode, automatic: bool },
    /// `-b`/`-ub` raised for an embedding/rerank run.
    Batch(u32),
    /// `--spec-type draft-simple` added for the draft model.
    SpecType,
}

impl Note {
    /// Log line in the UI language (`tr` or anything else = English).
    pub fn message(&self, lang: &str) -> String {
        let tr = lang == "tr";
        match (self, tr) {
            (Note::Mode { mode, automatic }, true) => {
                let flag = match mode {
                    ServeMode::Embedding => "--embeddings",
                    ServeMode::Rerank => "--rerank",
                    ServeMode::Chat => "sohbet",
                    ServeMode::Transcription => "whisper-server",
                    ServeMode::Image => "sd-server",
                };
                if *automatic {
                    format!("[Harness Studio] Model türüne göre sunucu modu: {} ({}). Sunucu modu ayarından değiştirilebilir.", mode.as_str(), flag)
                } else {
                    format!("[Harness Studio] Sunucu modu: {} ({}).", mode.as_str(), flag)
                }
            }
            (Note::Mode { mode, automatic }, false) => {
                let flag = match mode {
                    ServeMode::Embedding => "--embeddings",
                    ServeMode::Rerank => "--rerank",
                    ServeMode::Chat => "chat",
                    ServeMode::Transcription => "whisper-server",
                    ServeMode::Image => "sd-server",
                };
                if *automatic {
                    format!("[Harness Studio] Server mode from the model kind: {} ({}). Change it with the Server mode setting.", mode.as_str(), flag)
                } else {
                    format!("[Harness Studio] Server mode: {} ({}).", mode.as_str(), flag)
                }
            }
            (Note::Batch(n), true) => format!(
                "[Harness Studio] Embedding/rerank için -b/-ub {} yapıldı (llama-server aksi halde 512 belirteçten uzun girdileri reddeder).",
                n
            ),
            (Note::Batch(n), false) => format!(
                "[Harness Studio] -b/-ub set to {} for embeddings/rerank (llama-server otherwise rejects inputs longer than 512 tokens).",
                n
            ),
            (Note::SpecType, true) => "[Harness Studio] Taslak model için --spec-type draft-simple eklendi (bu sürüm onsuz taslak üretmez).".to_string(),
            (Note::SpecType, false) => "[Harness Studio] Added --spec-type draft-simple for the draft model (this build drafts nothing without it).".to_string(),
        }
    }
}

/// Result of [`prepare`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prepared {
    pub mode: ServeMode,
    pub notes: Vec<Note>,
}

/// Turns the stored settings of `config` into what is launched: removes [`SERVE_MODE`],
/// sets `embeddings`/`rerank` for the mode, sizes the batch of embedding runs and enables
/// draft-model speculation. `meta` is the model's GGUF header; `runtime` the binary's
/// capabilities (the spec-type fix needs them).
pub fn prepare(
    config: &mut LaunchConfig,
    meta: Option<&GgufMetadata>,
    runtime: Option<&RuntimeCapabilities>,
) -> Prepared {
    let chosen = requested(&config.params);
    let kind = detect_kind(meta);
    let mode = effective(&config.params, kind, config.router_mode);
    config.params.remove(SERVE_MODE);
    let mut notes = Vec::new();
    if !config.router_mode {
        let automatic = chosen.is_none();
        match mode {
            ServeMode::Chat => {
                if chosen == Some(ServeMode::Chat) {
                    config.params.remove("embeddings");
                    config.params.remove("rerank");
                }
            }
            ServeMode::Embedding => {
                config.params.remove("rerank");
                config
                    .params
                    .insert("embeddings".into(), ParamValue::Bool(true));
            }
            ServeMode::Rerank => {
                config.params.remove("embeddings");
                config
                    .params
                    .insert("rerank".into(), ParamValue::Bool(true));
            }
            // whisper-server launches skip this function (`process::run_claimed`).
            ServeMode::Transcription | ServeMode::Image => {}
        }
        // Only worth a log line when it changes anything or the kind decided it.
        if mode != ServeMode::Chat || chosen.is_some() {
            notes.push(Note::Mode { mode, automatic });
        }
        if mode != ServeMode::Chat && config.physical_batch.is_none() {
            let train = meta
                .and_then(|m| m.context_length)
                .map(|n| n.min(u64::from(MAX_EMBED_BATCH)) as u32);
            let limit = match config.context_size {
                0 => MAX_EMBED_BATCH,
                c => c.min(MAX_EMBED_BATCH),
            };
            let n = train.map_or(0, |t| t.min(limit)).max(MIN_EMBED_BATCH);
            config.physical_batch = Some(n);
            if config.logical_batch.is_none_or(|b| b < n) {
                config.logical_batch = Some(n);
            }
            notes.push(Note::Batch(n));
        }
    }
    let has_draft = config
        .draft_model_path
        .as_ref()
        .is_some_and(|p| !p.as_os_str().is_empty());
    let spec_type_set = config.params.get("spec_type").is_some_and(|v| match v {
        ParamValue::Text(s) => !s.trim().is_empty(),
        ParamValue::List(l) => l.iter().any(|s| !s.trim().is_empty()),
        _ => true,
    });
    if has_draft && !spec_type_set && !config.router_mode {
        let offers_draft = runtime
            .and_then(|r| r.flag("--spec-type"))
            .is_some_and(|f| {
                f.enum_values.iter().any(|v| v == SPEC_DRAFT_SIMPLE)
                    || f.value_hint
                        .as_deref()
                        .is_some_and(|h| h.split(',').any(|v| v.trim() == SPEC_DRAFT_SIMPLE))
            });
        if offers_draft {
            config.params.insert(
                "spec_type".into(),
                ParamValue::List(vec![SPEC_DRAFT_SIMPLE.to_string()]),
            );
            notes.push(Note::SpecType);
        }
    }
    Prepared { mode, notes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::testdata;
    use crate::params::{catalog, render_params, RenderContext};

    /// Header facts of the real files (`tests/data/gguf-kind-hints.json`).
    fn hint(file: &str) -> GgufMetadata {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/gguf-kind-hints.json");
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let row = rows.iter().find(|r| r["file"] == file).unwrap();
        let mut full = serde_json::to_value(GgufMetadata::default()).unwrap();
        for (k, v) in row["meta"].as_object().unwrap() {
            full[k] = v.clone();
        }
        serde_json::from_value(full).unwrap()
    }

    fn config() -> LaunchConfig {
        LaunchConfig {
            context_size: 4096,
            host: "127.0.0.1".into(),
            port: 18090,
            ..Default::default()
        }
    }

    fn rendered(c: &LaunchConfig) -> Vec<String> {
        let caps = testdata::caps("b11236");
        render_params(
            catalog(),
            Some(&caps),
            &c.params,
            &RenderContext {
                host: c.host.clone(),
                router_mode: false,
                legacy_set: Default::default(),
            },
        )
        .unwrap()
        .args
    }

    #[test]
    fn embedding_file_launches_with_embeddings() {
        let mut meta = hint("nomic-embed-text-v1.5.Q8_0.gguf");
        // The hints file has no context length; nomic-embed-text-v1.5 is trained on 2048.
        meta.context_length = Some(2048);
        let mut c = config();
        let p = prepare(&mut c, Some(&meta), None);
        assert_eq!(p.mode, ServeMode::Embedding);
        assert_eq!(rendered(&c), vec!["--embeddings"]);
        assert_eq!(c.physical_batch, Some(2048));
        assert_eq!(c.logical_batch, Some(2048));
        assert!(p.notes.contains(&Note::Mode {
            mode: ServeMode::Embedding,
            automatic: true
        }));
    }

    #[test]
    fn reranker_file_launches_with_rerank() {
        let meta = hint("bge-reranker-v2-m3-Q4_K_M.gguf");
        let mut c = config();
        let p = prepare(&mut c, Some(&meta), None);
        assert_eq!(p.mode, ServeMode::Rerank);
        assert_eq!(rendered(&c), vec!["--rerank"]);
    }

    #[test]
    fn llm_stays_chat_and_explicit_choice_wins() {
        let llm = hint("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let mut c = config();
        let p = prepare(&mut c, Some(&llm), None);
        assert_eq!(p.mode, ServeMode::Chat);
        assert!(p.notes.is_empty());
        assert!(c.params.is_empty());

        // Overriding an embedding file back to chat drops the automatic flag.
        let embed = hint("nomic-embed-text-v1.5.Q8_0.gguf");
        let mut c = config();
        c.params
            .insert(SERVE_MODE.into(), ParamValue::Text("chat".into()));
        c.params.insert("embeddings".into(), ParamValue::Bool(true));
        let p = prepare(&mut c, Some(&embed), None);
        assert_eq!(p.mode, ServeMode::Chat);
        assert!(c.params.is_empty(), "{:?}", c.params);

        // An LLM can be served for embeddings on request; the key never reaches the args.
        let mut c = config();
        c.params
            .insert(SERVE_MODE.into(), ParamValue::Text("embedding".into()));
        let p = prepare(&mut c, Some(&llm), None);
        assert_eq!(p.mode, ServeMode::Embedding);
        assert_eq!(rendered(&c), vec!["--embeddings"]);
        assert!(!c.params.contains_key(SERVE_MODE));

        // Without a context length in the header (or a small one) the floor applies.
        let mut c = config();
        let p = prepare(&mut c, None, None);
        assert_eq!(p.mode, ServeMode::Chat);
        assert_eq!(c.physical_batch, None);
        let mut c = config();
        c.params
            .insert(SERVE_MODE.into(), ParamValue::Text("rerank".into()));
        let rerank = hint("bge-reranker-v2-m3-Q4_K_M.gguf");
        let p = prepare(&mut c, Some(&rerank), None);
        assert_eq!(p.notes.last(), Some(&Note::Batch(MIN_EMBED_BATCH)));
        assert_eq!(c.physical_batch, Some(MIN_EMBED_BATCH));
        assert_eq!(c.logical_batch, Some(MIN_EMBED_BATCH));

        // A user batch size is kept; router mode is left alone.
        let mut c = config();
        c.physical_batch = Some(1024);
        prepare(&mut c, Some(&embed), None);
        assert_eq!(c.physical_batch, Some(1024));
        let mut c = config();
        c.router_mode = true;
        assert_eq!(prepare(&mut c, Some(&embed), None).mode, ServeMode::Chat);
        assert!(c.params.is_empty());
    }

    #[test]
    fn draft_model_gets_spec_type_on_b11236() {
        let caps = testdata::caps("b11236");
        let llm = hint("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let mut c = config();
        c.draft_model_path = Some("draft.gguf".into());
        let p = prepare(&mut c, Some(&llm), Some(&caps));
        assert_eq!(p.notes, vec![Note::SpecType]);
        assert_eq!(rendered(&c), vec!["--spec-type", "draft-simple"]);

        // A chosen spec type is kept; older builds without the flag get nothing.
        let mut c = config();
        c.draft_model_path = Some("draft.gguf".into());
        c.params
            .insert("spec_type".into(), ParamValue::Text("ngram-mod".into()));
        assert!(prepare(&mut c, Some(&llm), Some(&caps)).notes.is_empty());
        let old = testdata::caps("b11026");
        let mut c = config();
        c.draft_model_path = Some("draft.gguf".into());
        if old.flag("--spec-type").is_none() {
            assert!(prepare(&mut c, Some(&llm), Some(&old)).notes.is_empty());
        }
    }
}

/// Real llama-server runs (no mocks). Each test starts its own server on a free port from
/// 18190 on and stops it. Run with:
/// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<chat gguf> HS_TEST_EMBED_MODEL=<embedding gguf>
/// HS_TEST_RERANK_MODEL=<reranker gguf> cargo test -p harness-core serve_mode::e2e -- --ignored
/// --nocapture --test-threads=1
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::process::{ProcessManager, ServerStatus};
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    fn env(name: &str) -> Option<String> {
        let v = std::env::var(name).ok().filter(|v| !v.is_empty());
        if v.is_none() {
            eprintln!("{name} not set, skipping");
        }
        v
    }

    /// Starts `model` in the automatic mode and waits until the post-start verification ran.
    async fn start(
        id: &str,
        bin: &str,
        model: &str,
        from: u16,
        draft: Option<&str>,
    ) -> (ProcessManager, u16) {
        let port = crate::servers::ServerRegistry::default()
            .suggest_port("127.0.0.1", from, &[])
            .unwrap();
        let config = LaunchConfig {
            binary_path: bin.into(),
            model_path: model.into(),
            gpu_layers: 99,
            context_size: 4096,
            host: "127.0.0.1".into(),
            port,
            draft_model_path: draft.map(Into::into),
            draft_gpu_layers: draft.map(|_| 99),
            ..Default::default()
        };
        let pm = ProcessManager::with_id(id);
        pm.start(config).await.unwrap();
        let t = Instant::now();
        while pm.effective_values().is_none() {
            let failed = matches!(
                pm.get_status(),
                ServerStatus::Error(_) | ServerStatus::Stopped
            );
            if failed || t.elapsed() > Duration::from_secs(180) {
                let logs = pm.get_logs(Some(40)).join("\n");
                let _ = pm.stop().await;
                panic!("{:?}\n{}", pm.get_status(), logs);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        (pm, port)
    }

    async fn post(port: u16, path: &str, body: Value) -> Result<(u16, Value), String> {
        let res = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}{path}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let code = res.status().as_u16();
        Ok((code, res.json().await.map_err(|e| e.to_string())?))
    }

    fn cosine(a: &[f64], b: &[f64]) -> f64 {
        let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        dot / (na * nb)
    }

    fn verification(bin: &str, feature: &str) -> Option<crate::capabilities::Verification> {
        crate::capabilities::registry()
            .cached(std::path::Path::new(bin))
            .and_then(|c| c.verification(feature).cloned())
    }

    #[tokio::test]
    #[ignore]
    async fn embedding_model_serves_embeddings() {
        let (Some(bin), Some(model)) = (env("HS_LLAMA_SERVER"), env("HS_TEST_EMBED_MODEL")) else {
            return;
        };
        let (pm, port) = start("e2e-embed", &bin, &model, 18190, None).await;
        let mode = pm.active_server().map(|a| a.serve_mode);
        let logs = pm.get_logs(None);
        let texts = [
            "search_query: What is a panda?",
            "search_document: The giant panda is a bear species endemic to China.",
            "search_document: Paris is the capital of France.",
        ];
        let emb = post(port, "/v1/embeddings", json!({ "input": texts })).await;
        let chat = post(
            port,
            "/v1/chat/completions",
            json!({"messages": [{"role": "user", "content": "hi"}], "max_tokens": 2}),
        )
        .await;
        pm.stop().await.unwrap();
        assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());

        assert_eq!(mode, Some(ServeMode::Embedding));
        assert!(logs.iter().any(|l| l.contains("--embeddings")), "{logs:#?}");
        let (code, body) = emb.unwrap();
        assert_eq!(code, 200, "{body}");
        let vecs: Vec<Vec<f64>> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                d["embedding"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(vecs.len(), 3);
        eprintln!("dims {}", vecs[0].len());
        let (panda, paris) = (cosine(&vecs[0], &vecs[1]), cosine(&vecs[0], &vecs[2]));
        eprintln!("cos(query, panda) = {panda:.4}, cos(query, paris) = {paris:.4}");
        assert!(panda > paris);
        // Chat on an embedding server fails (b11236: 500 "does not logits computation").
        let (code, body) = chat.unwrap();
        eprintln!("chat on embedding server: {code} {body}");
        assert!(code >= 400);
        let v = verification(&bin, "embeddings_endpoint").expect("probe recorded");
        eprintln!("embeddings_endpoint: {v:?}");
        assert!(v.ok);
    }

    #[tokio::test]
    #[ignore]
    async fn reranker_model_serves_rerank() {
        let (Some(bin), Some(model)) = (env("HS_LLAMA_SERVER"), env("HS_TEST_RERANK_MODEL")) else {
            return;
        };
        let (pm, port) = start("e2e-rerank", &bin, &model, 18195, None).await;
        let mode = pm.active_server().map(|a| a.serve_mode);
        let logs = pm.get_logs(None);
        let res = post(
            port,
            "/v1/rerank",
            json!({
                "query": "What is a panda?",
                "documents": [
                    "hi",
                    "The giant panda is a bear species endemic to China.",
                    "Paris is the capital of France."
                ]
            }),
        )
        .await;
        // The native `/rerank` route (what the capability probe calls) with `top_n`.
        let native = post(
            port,
            "/rerank",
            json!({
                "query": "Which city is the capital of France?",
                "documents": [
                    "The giant panda is a bear species endemic to China.",
                    "Paris is the capital of France.",
                    "Bananas are rich in potassium."
                ],
                "top_n": 2
            }),
        )
        .await;
        pm.stop().await.unwrap();
        assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());

        assert_eq!(mode, Some(ServeMode::Rerank));
        assert!(logs.iter().any(|l| l.contains("--rerank")), "{logs:#?}");
        let (code, body) = res.unwrap();
        assert_eq!(code, 200, "{body}");
        eprintln!("rerank: {body}");
        let results = body["results"].as_array().unwrap();
        let best = results
            .iter()
            .max_by(|a, b| {
                a["relevance_score"]
                    .as_f64()
                    .partial_cmp(&b["relevance_score"].as_f64())
                    .unwrap()
            })
            .unwrap();
        assert_eq!(best["index"], 1);
        let (code, body) = native.unwrap();
        assert_eq!(code, 200, "{body}");
        eprintln!("/rerank top_n 2: {body}");
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 2, "top_n limits the results");
        // Sorted by score: Paris first.
        assert_eq!(results[0]["index"], 1, "{body}");
        let v = verification(&bin, "rerank_endpoint").expect("probe recorded");
        eprintln!("rerank_endpoint: {v:?}");
        assert!(v.ok);
    }

    /// The GPU layer count shown at the default log level (derived from `-ngl` and the
    /// model's `block_count`) matches what llama-server prints at `-lv 4`
    /// (`offloaded N/M layers to GPU`). With a CPU-only runtime both say 0.
    #[tokio::test]
    #[ignore]
    async fn derived_gpu_layers_match_the_verbose_log() {
        let (Some(bin), Some(model)) = (env("HS_LLAMA_SERVER"), env("HS_TEST_MODEL")) else {
            return;
        };
        let mut seen = Vec::new();
        for (i, custom) in [None, Some("-lv 4")].into_iter().enumerate() {
            let port = crate::servers::ServerRegistry::default()
                .suggest_port("127.0.0.1", 18210, &[])
                .unwrap();
            let config = LaunchConfig {
                binary_path: bin.clone().into(),
                model_path: model.clone().into(),
                gpu_layers: 20,
                context_size: 2048,
                host: "127.0.0.1".into(),
                port,
                custom_args: custom.map(str::to_string),
                ..Default::default()
            };
            let pm = ProcessManager::with_id(&format!("e2e-ngl-{i}"));
            pm.start(config).await.unwrap();
            let t = Instant::now();
            let values = loop {
                if let Some(v) = pm.effective_values() {
                    break v;
                }
                if matches!(
                    pm.get_status(),
                    ServerStatus::Error(_) | ServerStatus::Stopped
                ) || t.elapsed() > Duration::from_secs(180)
                {
                    let logs = pm.get_logs(Some(40)).join("\n");
                    let _ = pm.stop().await;
                    panic!("{:?}\n{}", pm.get_status(), logs);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            };
            let logged = pm
                .get_logs(None)
                .iter()
                .any(|l| l.contains("layers to GPU"));
            pm.stop().await.unwrap();
            eprintln!(
                "{custom:?}: {:?}/{:?} source {:?}, offload line in log: {logged}",
                values.gpu_layers_offloaded, values.gpu_layers_total, values.gpu_layers_source
            );
            seen.push((values, logged));
        }
        let (quiet, verbose) = (&seen[0].0, &seen[1].0);
        assert!(
            !seen[0].1,
            "b11299 prints no offload line at the default level"
        );
        assert!(seen[1].1, "-lv 4 prints it");
        let gpus = crate::capabilities::registry()
            .cached(std::path::Path::new(&bin))
            .map(|c| c.devices.len())
            .unwrap();
        assert_eq!(quiet.gpu_layers_source.as_deref(), Some("derived"));
        assert_eq!(quiet.gpu_layers_total, verbose.gpu_layers_total);
        assert_eq!(quiet.gpu_layers_offloaded, verbose.gpu_layers_offloaded);
        if gpus == 0 {
            assert_eq!(quiet.gpu_layers_offloaded, Some(0));
        } else {
            assert_eq!(verbose.gpu_layers_source.as_deref(), Some("log"));
            assert_eq!(quiet.gpu_layers_offloaded, Some(20));
        }
    }

    /// Anthropic Messages on a chat server with real inference (the capability probe only
    /// calls `/v1/messages/count_tokens`): a plain answer with usage, and the streamed event
    /// sequence.
    #[tokio::test]
    #[ignore]
    async fn chat_model_answers_anthropic_messages() {
        let (Some(bin), Some(model)) = (env("HS_LLAMA_SERVER"), env("HS_TEST_MODEL")) else {
            return;
        };
        let (pm, port) = start("e2e-messages", &bin, &model, 18205, None).await;
        let request = json!({
            "model": "any",
            "max_tokens": 32,
            "system": "You answer with one word.",
            "messages": [{"role": "user", "content": "What is the capital of France?"}]
        });
        let plain = post(port, "/v1/messages", request.clone()).await;
        let mut streamed = request;
        streamed["stream"] = json!(true);
        let sse = async {
            let res = reqwest::Client::new()
                .post(format!("http://127.0.0.1:{port}/v1/messages"))
                .json(&streamed)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let code = res.status().as_u16();
            Ok::<_, String>((code, res.text().await.map_err(|e| e.to_string())?))
        }
        .await;
        pm.stop().await.unwrap();
        assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());

        let (code, body) = plain.unwrap();
        assert_eq!(code, 200, "{body}");
        eprintln!("/v1/messages: {body}");
        assert_eq!(body["type"], "message");
        assert_eq!(body["role"], "assistant");
        let text: String = body["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        assert!(text.to_lowercase().contains("paris"), "{body}");
        assert!(body["usage"]["input_tokens"].as_u64().unwrap() > 0);
        assert!(body["usage"]["output_tokens"].as_u64().unwrap() > 0);
        assert!(
            matches!(
                body["stop_reason"].as_str(),
                Some("end_turn" | "max_tokens" | "stop_sequence")
            ),
            "{body}"
        );

        let (code, text) = sse.unwrap();
        assert_eq!(code, 200, "{text}");
        let events: Vec<&str> = text
            .lines()
            .filter_map(|l| l.strip_prefix("event: "))
            .collect();
        eprintln!("stream events: {events:?}");
        assert_eq!(events.first(), Some(&"message_start"));
        assert!(events.contains(&"content_block_start"));
        assert!(events.contains(&"content_block_delta"));
        assert!(events.contains(&"message_delta"));
        assert_eq!(events.last(), Some(&"message_stop"));
        assert!(text.to_lowercase().contains("paris"), "{text}");
    }

    /// The chat model drafting for itself: `--spec-type draft-simple` is added, `timings`
    /// report `draft_n` / `draft_n_accepted`, and the `/v1/responses` transport probe
    /// produces neither an exception nor an explanation line in the server log.
    #[tokio::test]
    #[ignore]
    async fn draft_model_drafts_without_probe_errors() {
        let (Some(bin), Some(model)) = (env("HS_LLAMA_SERVER"), env("HS_TEST_MODEL")) else {
            return;
        };
        // A real draft of the same family (`HS_TEST_DRAFT_MODEL`), else the model drafts for itself.
        let draft = env("HS_TEST_DRAFT_MODEL").unwrap_or_else(|| model.clone());
        let (pm, port) = start("e2e-draft", &bin, &model, 18200, Some(&draft)).await;
        let res = post(
            port,
            "/v1/chat/completions",
            json!({
                "messages": [{"role": "user", "content": "Count from 1 to 10."}],
                "max_tokens": 30
            }),
        )
        .await;
        // Give the log reader a moment to catch up.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let logs = pm.get_logs(None);
        pm.stop().await.unwrap();
        assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());

        assert!(
            logs.iter().any(|l| l.contains("--spec-type draft-simple")),
            "{logs:#?}"
        );
        let (code, body) = res.unwrap();
        assert_eq!(code, 200, "{body}");
        let timings = &body["timings"];
        eprintln!("timings: {timings}");
        assert!(timings["draft_n"].as_u64().unwrap() > 0);
        assert!(timings["draft_n_accepted"].as_u64().is_some());
        let exception = logs.iter().position(|l| l.contains("'input' is required"));
        let note = logs
            .iter()
            .position(|l| l.starts_with("[Harness Studio]") && l.contains("/v1/responses 400"));
        assert!(exception.is_none(), "{logs:#?}");
        assert!(note.is_none(), "{logs:#?}");
    }
}
