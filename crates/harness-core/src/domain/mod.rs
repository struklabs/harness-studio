//! Domain model (ARCHITECTURE §13.3): four separate concepts that are never used in place of
//! each other.
//!
//! * [`LocalModel`] (`models::LocalModel`): a physical artifact on disk, derived by scanning.
//!   Not persisted. [`ModelKind`] says what the file is.
//! * [`ModelDefinition`]: a logical model: unique alias, base file, adapters, projector,
//!   draft, chat template and default load settings. `model_definitions.json`.
//! * [`ServerProfile`]: how a definition is served (host, port, autostart, API key reference,
//!   serve-only overrides). `server_profiles.json`.
//! * [`InferencePreset`]: request-side settings (system prompt, sampling, reasoning).
//!   `inference_presets.json`; built-ins live in code and are read-only.
//!
//! Effective launch settings are layered by [`launch::resolve_launch`]:
//! defaults ← `ModelDefinition.load` ← `ServerProfile.serve_overrides` ← transient overrides.
//!
//! "Draft" is a usage, not a file kind: any LLM file can be the draft of another definition
//! (`ModelDefinition.draft`), so [`ModelKind`] has no `Draft` variant.

use crate::error_key::err;

pub mod launch;
pub mod portable;
pub mod presets;
pub mod status;
pub mod store;

use crate::gguf::GgufMetadata;
use crate::params::ParamValues;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use crate::models::LocalModel;
pub use presets::{InferencePreset, ReasoningSettings, SamplingParams};

// --- LocalModel kind ---

/// What a GGUF file is. Decided from its header by [`detect_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ModelKind {
    /// Text generation model (possibly paired with an mmproj for vision).
    Llm,
    /// Embedding model (`<arch>.pooling_type` mean/cls/last).
    Embedding,
    /// Reranker (`pooling_type` rank or a classification head).
    Reranker,
    /// A Hugging Face `pipeline` task model served through `/v1/hf` (text classification,
    /// NER, fill-mask, question answering...); the task is in [`crate::models::LocalModel::task`].
    Pipeline,
    /// Speech recognition model: whisper.cpp's ggml `.bin` (not a GGUF; detected by the ggml magic).
    Asr,
    /// Image / video generation model run by stable-diffusion.cpp (a checkpoint or standalone
    /// diffusion model in safetensors / `.ckpt` / GGUF; found by its tensors, see
    /// [`crate::engine::is_image_gguf`]).
    Image,
    /// Text-to-speech model: a Kokoro or Piper folder run by sherpa-onnx (found by its files,
    /// see [`crate::tts::detect`]).
    Tts,
    /// Multimodal projector (`general.type = mmproj`, `clip.*`): vision and/or audio.
    Mmproj,
    /// LoRA / adapter file (`general.type = adapter`, `adapter.type`).
    Adapter,
    /// Not a model llama-server can serve on its own (e.g. imatrix data) or unreadable header.
    #[default]
    Unknown,
}

/// llama.cpp `enum llama_pooling_type`: NONE=0, MEAN=1, CLS=2, LAST=3, RANK=4.
const POOLING_RANK: u64 = 4;

/// Kind of a GGUF file from its parsed header.
///
/// Verified on real files (see `tests/data/gguf-kind-hints.json`):
/// * LLMs: `general.type = "model"`, no `<arch>.pooling_type` (qwen2, qwen3, gpt-oss, and
///   `mistral3` which has no `general.type` at all).
/// * Embeddings: `general.type = "model"` + `<arch>.pooling_type` 1/2/3 (bert, qwen3,
///   gemma-embedding).
///   nomic-embed-text-v1.5 (`nomic-bert`) has no `general.type` but `pooling_type` 1.
/// * Rerankers: bge-reranker-v2-m3 (`bert`, gpustack GGUF) has no `pooling_type` and no
///   `classifier.output_labels`; only its `cls.weight` / `cls.output.weight` tensors (the
///   classification head llama.cpp uses for RANK pooling under `--rerank`) tell it apart.
/// * Projectors: `general.type = "mmproj"`, `general.architecture = "clip"`,
///   `clip.has_vision_encoder` / `clip.has_audio_encoder`.
///
/// From llama.cpp's writers (no local file to verify): newer reranker conversions write
/// pooling type RANK (4) and/or a `<arch>.classifier.output_labels` head; LoRA files have
/// `general.type = "adapter"` and `adapter.type = "lora"`.
pub fn detect_kind(meta: Option<&GgufMetadata>) -> ModelKind {
    let Some(m) = meta else {
        return ModelKind::Unknown;
    };
    if m.architecture.as_deref() == Some("audiocpp") {
        return if m
            .audiocpp_family
            .as_deref()
            .and_then(crate::engine::audio_cpp::task)
            .is_some()
        {
            ModelKind::Tts
        } else {
            ModelKind::Unknown
        };
    }
    if m.architecture
        .as_deref()
        .is_some_and(crate::engine::crispasr_tts::is_tts_arch)
    {
        return ModelKind::Tts;
    }
    let general_type = m.general_type.as_deref();
    if m.architecture
        .as_deref()
        .is_some_and(crate::engine::is_image_arch)
    {
        return ModelKind::Image;
    }
    if m.is_mmproj
        || general_type == Some("mmproj")
        || m.architecture.as_deref() == Some("clip")
        || m.has_vision_encoder
        || m.has_audio_encoder
    {
        return ModelKind::Mmproj;
    }
    if general_type == Some("adapter") || m.adapter_type.is_some() {
        return ModelKind::Adapter;
    }
    if general_type.is_some_and(|t| t != "model") || m.architecture.is_none() {
        return ModelKind::Unknown;
    }
    match m.pooling_type {
        Some(POOLING_RANK) => ModelKind::Reranker,
        _ if m.has_classifier_labels => ModelKind::Reranker,
        Some(1..=3) => ModelKind::Embedding,
        _ if m.has_cls_head => ModelKind::Reranker,
        _ => ModelKind::Llm,
    }
}

/// Where a scanned model lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ModelOrigin {
    /// Inside one of the library directories (ours to move/delete).
    #[default]
    Library,
    /// Linked in place from another app (Ollama, LM Studio); never moved or deleted.
    Linked,
}

// --- ModelDefinition ---

/// Base weights of a definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ModelRef {
    /// A local GGUF (`LocalModel.id`: canonical launch path; shard 1 for split models).
    Local {
        #[cfg_attr(feature = "openapi", schema(value_type = String))]
        path: PathBuf,
    },
    /// A Hugging Face file not downloaded yet (`hf:<repo>:<file>`).
    Hf { repo: String, file: String },
}

impl ModelRef {
    /// Parses `hf:<owner>/<repo>:<file>`; anything else is a local path.
    pub fn parse(s: &str) -> Self {
        if let Some(rest) = s.strip_prefix("hf:") {
            if let Some((repo, file)) = rest.split_once(':') {
                if !repo.is_empty() && !file.is_empty() {
                    return ModelRef::Hf {
                        repo: repo.to_string(),
                        file: file.to_string(),
                    };
                }
            }
        }
        ModelRef::Local {
            path: PathBuf::from(s),
        }
    }

    pub fn local_path(&self) -> Option<&std::path::Path> {
        match self {
            ModelRef::Local { path } => Some(path),
            ModelRef::Hf { .. } => None,
        }
    }
}

impl std::fmt::Display for ModelRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelRef::Local { path } => write!(f, "{}", path.display()),
            ModelRef::Hf { repo, file } => write!(f, "hf:{}:{}", repo, file),
        }
    }
}

/// A LoRA adapter applied at load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdapterRef {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub path: PathBuf,
    /// `None` = llama-server's default scale (1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

/// Speculative decoding with a draft model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DraftSpec {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub model: PathBuf,
    /// Draft-only catalog params (`draft_gpu_layers`, ...).
    #[serde(default, skip_serializing_if = "ParamValues::is_empty")]
    pub params: ParamValues,
}

/// A logical model: what clients ask for by `alias`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelDefinition {
    pub id: String,
    /// API `model` name; unique (case-insensitive), `[A-Za-z0-9._:-]`, see [`validate_alias`].
    pub alias: String,
    pub base: ModelRef,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub mmproj: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<AdapterRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<DraftSpec>,
    /// Built-in template name or template file (`--chat-template`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_template: Option<String>,
    /// JSON object text for `--chat-template-kwargs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<String>,
    /// Extra command-line arguments (same rules as the Dashboard's custom args field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_args: Option<String>,
    /// Default load settings: catalog ids (`params.toml`) -> value. The typed fields above are
    /// never duplicated here.
    #[serde(default)]
    pub load: ParamValues,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_preset_id: Option<String>,
    /// Reserved for the gateway (idle unload after N seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
    /// Reserved for the gateway (never evicted).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// Store the request and response content of gateway requests to this definition in
    /// `analytics.db`. Off by default: only metadata is recorded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub capture_content: bool,
    /// Days captured content is kept (default [`crate::analytics::DEFAULT_CONTENT_RETENTION_DAYS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_retention_days: Option<u32>,
    /// Created by Harness Studio itself (v2 migration, a profile saved without a definition,
    /// the Dashboard session). Deleted again when the profile that owned it goes away and
    /// nothing else references it. Cleared as soon as the user edits the definition.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_created: bool,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl ModelDefinition {
    /// An empty definition (no id yet: `Domain::save_definition` assigns one).
    pub fn new(alias: impl Into<String>, base: ModelRef) -> Self {
        let mut load = ParamValues::new();
        if base.local_path().is_some_and(crate::asr::is_onnx_ctc_file) {
            load.insert(
                "task".into(),
                crate::params::ParamValue::Text("asr_ctc".into()),
            );
        }
        if base
            .local_path()
            .is_some_and(crate::engine::crispasr::is_model)
        {
            load.insert(
                "task".into(),
                crate::params::ParamValue::Text("crispasr".into()),
            );
        }
        Self {
            id: String::new(),
            alias: alias.into(),
            base,
            description: String::new(),
            mmproj: None,
            adapters: Vec::new(),
            draft: None,
            chat_template: None,
            chat_template_kwargs: None,
            extra_args: None,
            load,
            default_preset_id: None,
            ttl_seconds: None,
            pinned: false,
            capture_content: false,
            content_retention_days: None,
            auto_created: false,
            created_at: 0,
            updated_at: 0,
        }
    }
}

/// Router mode: one llama-server serving every model of a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RouterSpec {
    /// Directory the router serves (`None` = the primary library directory at launch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub models_dir: Option<PathBuf>,
}

/// How a definition is served.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema), schema(as = DomainServerProfile))]
pub struct ServerProfile {
    /// Also the server instance id. `"default"` is the Dashboard's own server (its last
    /// session); it is not listed among the user's profiles.
    pub id: String,
    pub name: String,
    /// Short free-text purpose ("OpenCode", "Open Design"...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    /// The served definition (`None` only in router mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router: Option<RouterSpec>,
    pub host: String,
    /// With `port_auto` the last automatic pick (kept while free), see
    /// `LaunchConfig::port_auto`.
    pub port: u16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub port_auto: bool,
    #[serde(default)]
    pub auto_start: bool,
    /// Keychain reference of the API key (`profile:<id>`, `session`); never the key itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
    /// Layer over the definition's `load` (catalog ids plus `custom_args` / `lora_scale`, see
    /// [`launch`]); an empty text value means "unset".
    #[serde(default)]
    pub serve_overrides: ParamValues,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

// --- alias rules ---

pub const ALIAS_MAX_LEN: usize = 128;

fn alias_char_ok(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-')
}

/// `[A-Za-z0-9._:-]{1,128}`.
pub fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty() || alias.len() > ALIAS_MAX_LEN || !alias.chars().all(alias_char_ok) {
        return Err(err("definitions.invalid_alias")
            .with("alias", alias)
            .num("max", ALIAS_MAX_LEN as u64)
            .into());
    }
    Ok(())
}

/// Turns any text (a file stem) into a valid alias: invalid characters become `-`.
pub fn sanitize_alias(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        let c = if alias_char_ok(c) { c } else { '-' };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let out: String = out
        .trim_matches(|c| c == '-' || c == '.')
        .chars()
        .take(ALIAS_MAX_LEN)
        .collect();
    if out.is_empty() {
        "model".to_string()
    } else {
        out
    }
}

/// `base`, or `base-2`, `base-3`... the first one not in `taken` (case-insensitive).
pub fn unique_alias<'a>(base: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let base = sanitize_alias(base);
    let is_taken = |a: &str| taken.clone().into_iter().any(|t| t.eq_ignore_ascii_case(a));
    if !is_taken(&base) {
        return base;
    }
    (2..)
        .map(|n| {
            let suffix = format!("-{}", n);
            let head: String = base.chars().take(ALIAS_MAX_LEN - suffix.len()).collect();
            format!("{}{}", head, suffix)
        })
        .find(|a| !is_taken(a))
        .expect("unbounded")
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A new unique id with a prefix (`md-1a2b...`).
pub(crate) fn new_id(prefix: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seed = format!(
        "{}:{}:{}",
        nanos,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let digest = Sha256::digest(seed.as_bytes());
    let hex: String = digest
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect();
    format!("{}-{}", prefix, hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_validation() {
        for ok in [
            "qwen3-4b",
            "Qwen3-4B-Instruct-2507-Q4_K_M",
            "gpt-oss:20b",
            "a.b_c",
        ] {
            assert!(validate_alias(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a b", "ü", "x/y", "a\\b", &"a".repeat(129)] {
            assert!(validate_alias(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn alias_sanitize_and_uniqueness_are_case_insensitive() {
        assert_eq!(sanitize_alias("qwen2.5 coder (14B)"), "qwen2.5-coder-14B");
        assert_eq!(sanitize_alias("  ***  "), "model");
        assert_eq!(sanitize_alias("Türkçe model"), "T-rk-e-model");
        let taken = ["Qwen", "qwen-2"];
        assert_eq!(unique_alias("qwen", taken.iter().copied()), "qwen-3");
        assert_eq!(unique_alias("other", taken.iter().copied()), "other");
        let long = "x".repeat(200);
        let a = unique_alias(&long, [long[..128].to_string()].iter().map(|s| s.as_str()));
        assert_eq!(a.len(), 128);
        assert!(a.ends_with("-2"));
        assert!(validate_alias(&a).is_ok());
    }

    #[test]
    fn model_ref_parse_and_display() {
        let hf = ModelRef::parse("hf:unsloth/Qwen3-4B-GGUF:Qwen3-4B-Q4_K_M.gguf");
        assert_eq!(
            hf,
            ModelRef::Hf {
                repo: "unsloth/Qwen3-4B-GGUF".into(),
                file: "Qwen3-4B-Q4_K_M.gguf".into()
            }
        );
        assert_eq!(
            hf.to_string(),
            "hf:unsloth/Qwen3-4B-GGUF:Qwen3-4B-Q4_K_M.gguf"
        );
        let local = ModelRef::parse(r"D:\Models\x.gguf");
        assert_eq!(
            local.local_path().unwrap(),
            std::path::Path::new(r"D:\Models\x.gguf")
        );
        // Round trip through JSON uses a tagged object.
        let json = serde_json::to_value(&hf).unwrap();
        assert_eq!(json["kind"], "hf");
    }

    /// Header facts of real files (extracted with `gguf::read_gguf_metadata` by
    /// `kind_hints_dump` below) and the kind each must get.
    #[test]
    fn kind_detection_on_real_header_metadata() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/gguf-kind-hints.json");
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(rows.len() >= 10);
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            let mut full = serde_json::to_value(GgufMetadata::default()).unwrap();
            for (k, v) in row["meta"].as_object().unwrap() {
                full[k] = v.clone();
            }
            let meta: GgufMetadata = serde_json::from_value(full).unwrap();
            let expected: ModelKind = serde_json::from_value(row["expected"].clone()).unwrap();
            assert_eq!(detect_kind(Some(&meta)), expected, "{}", row["file"]);
            seen.insert(format!("{:?}", expected));
        }
        for kind in ["Llm", "Embedding", "Reranker", "Mmproj"] {
            assert!(seen.contains(kind), "no real sample for {kind}");
        }
    }

    /// Decision rules for kinds without a local sample file (keys as written by llama.cpp's
    /// `convert_lora_to_gguf.py` and rerank conversions).
    #[test]
    fn kind_rules_for_adapter_reranker_and_unknown() {
        let base = GgufMetadata {
            architecture: Some("qwen3".into()),
            general_type: Some("model".into()),
            ..Default::default()
        };
        let with = |f: &dyn Fn(&mut GgufMetadata)| {
            let mut m = base.clone();
            f(&mut m);
            detect_kind(Some(&m))
        };
        assert_eq!(with(&|_| {}), ModelKind::Llm);
        assert_eq!(with(&|m| m.pooling_type = Some(0)), ModelKind::Llm);
        assert_eq!(with(&|m| m.pooling_type = Some(4)), ModelKind::Reranker);
        assert_eq!(
            with(&|m| m.has_classifier_labels = true),
            ModelKind::Reranker
        );
        // A classification head next to a real embedding pooling stays an embedding model.
        assert_eq!(
            with(&|m| {
                m.has_cls_head = true;
                m.pooling_type = Some(2);
            }),
            ModelKind::Embedding
        );
        assert_eq!(
            with(&|m| {
                m.general_type = Some("adapter".into());
                m.adapter_type = Some("lora".into());
            }),
            ModelKind::Adapter
        );
        assert_eq!(
            with(&|m| m.general_type = Some("imatrix".into())),
            ModelKind::Unknown
        );
        // Ollama's mistral3 blob has no `general.type` at all (seen on a real file).
        assert_eq!(with(&|m| m.general_type = None), ModelKind::Llm);
        assert_eq!(with(&|m| m.architecture = None), ModelKind::Unknown);
        assert_eq!(detect_kind(None), ModelKind::Unknown);
    }

    /// Prints the header facts used by `kind_detection_on_real_header_metadata`.
    /// HS_MODELS_DIR="D:\Models;C:\Users\me\.lmstudio\models" cargo test -p harness-core
    ///   kind_hints_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn kind_hints_dump() {
        let Ok(dirs) = std::env::var("HS_MODELS_DIR") else {
            eprintln!("HS_MODELS_DIR not set, skipping");
            return;
        };
        let mut files = Vec::new();
        for dir in dirs.split(';').filter(|d| !d.is_empty()) {
            crate::importer::collect_files(std::path::Path::new(dir), &mut files, 6);
        }
        let mut rows = Vec::new();
        for f in files.iter().filter(|f| {
            f.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        }) {
            let meta = crate::gguf::read_gguf_metadata(f).unwrap();
            let kind = detect_kind(Some(&meta));
            rows.push(serde_json::json!({
                "file": f.file_name().unwrap().to_string_lossy(),
                "meta": {
                    "architecture": meta.architecture,
                    "general_type": meta.general_type,
                    "adapter_type": meta.adapter_type,
                    "pooling_type": meta.pooling_type,
                    "is_mmproj": meta.is_mmproj,
                    "has_vision_encoder": meta.has_vision_encoder,
                    "has_audio_encoder": meta.has_audio_encoder,
                    "has_classifier_labels": meta.has_classifier_labels,
                    "has_cls_head": meta.has_cls_head,
                    "has_chat_template": meta.has_chat_template,
                    "quantization": meta.quantization,
                },
                "expected": kind,
            }));
        }
        println!("{}", serde_json::to_string_pretty(&rows).unwrap());
    }
}
