//! What converting a repo would take, before anything is downloaded: can the converter handle
//! its architecture, which files come over the wire, how big is the result and how much disk
//! does the whole run need.

use super::arch::Architectures;
use super::{bits_per_weight, env, is_direct, CHOICES, DEFAULT_QUANT};
use crate::downloader::{hf_refusal, DownloadManager};
use crate::error_key::err;
use crate::storage::StorageManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const HF_API: &str = "https://huggingface.co/api";
/// Hub tasks of repos worth converting: text and vision-language chat models.
pub const TASKS: [&str; 2] = ["text-generation", "image-text-to-text"];
/// Size cap of a non-weight file that is downloaded (tokenizers, configs, chat templates).
const AUX_FILE_CAP: u64 = 128 * 1024 * 1024;
/// Margin kept free besides the files themselves (temporary files, filesystem overhead).
const DISK_MARGIN: f64 = 1.05;
/// Extra bytes a run needs next to the estimate (vocabulary tables, metadata, small files).
const DISK_FLOOR: u64 = 256 * 1024 * 1024;

/// One file of the repo that is downloaded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PlanFile {
    pub name: String,
    pub size: u64,
    /// SHA-256 Hugging Face publishes for the file (LFS), checked after the download.
    pub sha256: Option<String>,
}

/// The result size and disk need of one target type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QuantEstimate {
    pub quant: String,
    /// Size of the finished GGUF.
    pub size_bytes: u64,
    /// Disk the whole run needs at its peak (download + 16-bit GGUF + result).
    pub disk_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConvertPlan {
    pub repo_id: String,
    /// `architectures[0]` of the repo's config.json.
    pub architecture: Option<String>,
    pub task: Option<String>,
    pub license: Option<String>,
    pub gated: bool,
    /// The converter takes this repo.
    pub convertible: bool,
    /// Why it does not: a keyed error (`convert.unsupported_architecture`...).
    pub reason: Option<String>,
    /// The converter also writes a vision projector for it (image-text-to-text).
    pub vision: bool,
    pub files: Vec<PlanFile>,
    pub download_bytes: u64,
    /// Parameter count (from the Hub's safetensors header summary, else estimated).
    pub parameters: u64,
    /// Parameters of the token embedding (`vocab_size * hidden_size` of the config; 0 unknown):
    /// kept at a higher precision than the rest, so a small model's file is bigger than its
    /// parameter count says.
    #[serde(default)]
    pub embedding_parameters: u64,
    pub quants: Vec<QuantEstimate>,
    pub default_quant: String,
    /// Free bytes on the drive of the model library.
    pub free_bytes: u64,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub work_root: std::path::PathBuf,
    /// The conversion environment matches the runtime and is ready.
    pub env_ready: bool,
    /// Rough size a first build of the environment adds (0 when it is ready).
    pub env_estimated_bytes: u64,
    /// llama.cpp tag of the managed runtime the GGUF is made for.
    pub runtime_tag: Option<String>,
    /// The architecture list of the converter was available (false: only the task was checked).
    pub architectures_checked: bool,
}

impl ConvertPlan {
    /// Estimate of a target type (computed for types the Hub does not list).
    pub fn estimate(&self, quant: &str) -> QuantEstimate {
        self.quants
            .iter()
            .find(|q| q.quant == quant)
            .cloned()
            .unwrap_or_else(|| {
                estimate(
                    self.parameters,
                    self.embedding_parameters,
                    self.download_bytes,
                    quant,
                )
            })
    }

    /// Refuses a run that cannot work: the repo is not convertible or the disk is too small.
    pub fn check(&self, quant: &str) -> Result<(), String> {
        if !self.convertible {
            return Err(self
                .reason
                .clone()
                .unwrap_or_else(|| err("convert.unsupported_architecture").into()));
        }
        let need = self.estimate(quant).disk_bytes;
        if self.free_bytes < need {
            return Err(err("convert.not_enough_disk")
                .with("need", crate::doctor::fmt_bytes(need))
                .with("free", crate::doctor::fmt_bytes(self.free_bytes))
                .with("path", self.work_root.display())
                .into());
        }
        Ok(())
    }
}

/// Bits per weight the token embedding (and a tied output matrix) gets in a quantized file:
/// llama.cpp keeps it above the body's type (Q6_K / Q8_0 at the 4-5 bit types). Counted at 8.5 bits (Q8_0);
/// calibrated on SmolLM2-135M Q4_K_M (100.6 MiB real, 28.3 M embedding weights of 134.5 M).
const EMBEDDING_BITS: f64 = 8.5;
/// Metadata and tokenizer a GGUF carries (vocabulary text, scores, merges: about 1.5 MB at 49k
/// entries, several MB at 150k).
const METADATA_BYTES: f64 = 2.0 * 1024.0 * 1024.0;

/// Estimated result size and peak disk need of a run. `embedding_parameters`: the token
/// embedding's weights (0 when the config does not say), which stay at a higher precision when
/// the target is quantized.
pub fn estimate(
    parameters: u64,
    embedding_parameters: u64,
    download_bytes: u64,
    quant: &str,
) -> QuantEstimate {
    let params = parameters as f64;
    let f16 = params * 2.0;
    let bpw = bits_per_weight(quant);
    let result = if is_direct(quant) {
        params * bpw / 8.0
    } else {
        let embedding = (embedding_parameters as f64).min(params);
        (params - embedding) * bpw / 8.0
            + embedding * bpw.max(EMBEDDING_BITS) / 8.0
            + METADATA_BYTES
    };
    // Peak: the downloaded weights, the 16-bit GGUF, and the result next to it (a 16-bit
    // target is the 16-bit GGUF itself).
    let peak = download_bytes as f64 + f16 + if is_direct(quant) { 0.0 } else { result };
    QuantEstimate {
        quant: quant.to_string(),
        size_bytes: result as u64,
        disk_bytes: (peak * DISK_MARGIN) as u64 + DISK_FLOOR,
    }
}

/// Whether `name` (a path in the repo) is a file the converter reads: root level only.
fn wanted_name(name: &str) -> bool {
    if name.contains('/') || name.starts_with('.') {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    // Training logs and result dumps are json too, but of no use to a converter.
    if matches!(
        lower.as_str(),
        "trainer_state.json"
            | "all_results.json"
            | "eval_results.json"
            | "train_results.json"
            | "training_args.json"
    ) {
        return false;
    }
    [
        ".safetensors",
        ".json",
        ".txt",
        ".model",
        ".jinja",
        ".tiktoken",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// The files to download out of a Hub `siblings` array (`?blobs=true`: with sizes). Weights
/// come as `model*.safetensors`; a repo that also ships Mistral's `consolidated*.safetensors`
/// is downloaded once, in the transformers layout.
pub fn select_files(siblings: &[Value]) -> Vec<PlanFile> {
    let mut files: Vec<PlanFile> = siblings
        .iter()
        .filter_map(|s| {
            let name = s.get("rfilename")?.as_str()?;
            if !wanted_name(name) {
                return None;
            }
            let lfs = s.get("lfs");
            let size = lfs
                .and_then(|l| l.get("size"))
                .and_then(Value::as_u64)
                .or_else(|| s.get("size").and_then(Value::as_u64))
                .unwrap_or(0);
            let weights = name.to_ascii_lowercase().ends_with(".safetensors");
            if !weights && size > AUX_FILE_CAP {
                return None;
            }
            Some(PlanFile {
                name: name.to_string(),
                size,
                sha256: lfs
                    .and_then(|l| l.get("sha256"))
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase)
                    .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())),
            })
        })
        .collect();
    let has_hf_layout = files.iter().any(|f| {
        let n = f.name.to_ascii_lowercase();
        n.ends_with(".safetensors") && !n.starts_with("consolidated")
    });
    if has_hf_layout {
        files.retain(|f| {
            let n = f.name.to_ascii_lowercase();
            !(n.starts_with("consolidated") || n == "params.json")
        });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

/// Parameter count of a Hub model record: the safetensors summary, else two bytes per
/// weight of the downloaded files.
fn parameters_of(info: &Value, files: &[PlanFile]) -> u64 {
    info.get("safetensors")
        .and_then(|s| s.get("total"))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            files
                .iter()
                .filter(|f| f.name.to_ascii_lowercase().ends_with(".safetensors"))
                .map(|f| f.size)
                .sum::<u64>()
                / 2
        })
}

/// `vocab_size * hidden_size` of the repo's config (the text config of a multimodal one); 0 when
/// the Hub record has no config.
fn embedding_parameters_of(info: &Value) -> u64 {
    let Some(config) = info.get("config") else {
        return 0;
    };
    let number = |key: &str| {
        config
            .get(key)
            .or_else(|| config.get("text_config").and_then(|t| t.get(key)))
            .and_then(Value::as_u64)
    };
    number("vocab_size")
        .zip(number("hidden_size"))
        .map_or(0, |(v, h)| v * h)
}

/// Adds the fields of the repo's `config.json` that the Hub record lacks to `info["config"]`.
/// The record's `config` carries only `architectures`, `model_type` and the tokenizer config:
/// without `vocab_size` / `hidden_size` the token embedding is not counted and a small model's
/// quantized size comes out about a quarter too small. Fields the record has stay.
pub fn merge_config(info: &mut Value, full: Value) {
    let (Some(record), Value::Object(full)) = (info.as_object_mut(), full) else {
        return;
    };
    let config = record
        .entry("config")
        .or_insert_with(|| Value::Object(Default::default()));
    if let Some(config) = config.as_object_mut() {
        for (key, value) in full {
            config.entry(key).or_insert(value);
        }
    }
}

/// The repo's `config.json` at the record's revision, for [`merge_config`]. Best effort: any
/// failure leaves the record as it is (the estimate then ignores the embedding).
async fn fetch_config(repo_id: &str, info: &Value, token: Option<&str>) -> Option<Value> {
    let revision = info.get("sha").and_then(Value::as_str).unwrap_or("main");
    let url = format!("https://huggingface.co/{repo_id}/resolve/{revision}/config.json");
    let res = DownloadManager::apply_hf_auth(client().ok()?.get(&url), &url, token)
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    res.json().await.ok()
}

fn architecture_of(info: &Value) -> Option<String> {
    info.get("config")
        .and_then(|c| c.get("architectures"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(String::from)
}

fn license_of(info: &Value) -> Option<String> {
    info.get("cardData")
        .and_then(|c| c.get("license"))
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            info.get("tags")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(Value::as_str)
                .find_map(|t| t.strip_prefix("license:").map(String::from))
        })
}

/// A plan from a Hub model record (`GET /api/models/<repo>?blobs=true`). `architectures` is
/// the converter's list (`None` when it is not available yet).
pub fn build_plan(
    repo_id: &str,
    info: &Value,
    archs: Option<&Architectures>,
    free_bytes: u64,
    work_root: std::path::PathBuf,
    env_ready: bool,
    runtime_tag: Option<String>,
) -> ConvertPlan {
    let siblings: Vec<Value> = info
        .get("siblings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let files = select_files(&siblings);
    let download_bytes: u64 = files.iter().map(|f| f.size).sum();
    let parameters = parameters_of(info, &files);
    let embedding_parameters = embedding_parameters_of(info);
    let architecture = architecture_of(info);
    let task = info
        .get("pipeline_tag")
        .and_then(Value::as_str)
        .map(String::from);
    let gated = info
        .get("gated")
        .is_some_and(|g| g.as_bool().unwrap_or(true) && !g.is_null());

    let has_weights = files
        .iter()
        .any(|f| f.name.to_ascii_lowercase().ends_with(".safetensors"));
    let has_config = files.iter().any(|f| f.name == "config.json");
    let is_adapter = !has_config && files.iter().any(|f| f.name == "adapter_config.json");

    let mut vision = false;
    let reason: Option<String> = if is_adapter {
        Some(err("convert.adapter_only").with("repo", repo_id).into())
    } else if !has_weights {
        Some(err("convert.no_safetensors").with("repo", repo_id).into())
    } else if !has_config {
        Some(err("convert.no_config").with("repo", repo_id).into())
    } else if let Some(t) = task.as_deref().filter(|t| !TASKS.contains(t)) {
        Some(err("convert.not_llm").with("task", t).into())
    } else {
        match (architecture.as_deref(), archs) {
            (None, _) => Some(err("convert.no_architecture").with("repo", repo_id).into()),
            (Some(a), Some(list)) => {
                vision = task.as_deref() == Some("image-text-to-text") && list.supports_vision(a);
                (!list.supports_text(a)).then(|| {
                    String::from(
                        err("convert.unsupported_architecture")
                            .with("architecture", a)
                            .with("repo", repo_id),
                    )
                })
            }
            (Some(_), None) => None,
        }
    };

    let quants = CHOICES
        .iter()
        .map(|c| estimate(parameters, embedding_parameters, download_bytes, c.name))
        .collect();
    ConvertPlan {
        repo_id: repo_id.to_string(),
        architecture,
        task,
        license: license_of(info),
        gated,
        convertible: reason.is_none(),
        reason,
        vision,
        files,
        download_bytes,
        parameters,
        embedding_parameters,
        quants,
        default_quant: DEFAULT_QUANT.to_string(),
        free_bytes,
        work_root,
        env_ready,
        env_estimated_bytes: if env_ready {
            0
        } else {
            env::ESTIMATED_SIZE_BYTES
        },
        runtime_tag,
        architectures_checked: archs.is_some(),
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("HarnessStudio/0.1.0")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
}

/// The Hub record of a repo, with file sizes. A gated or private repo without a usable token
/// is the matching keyed error.
pub async fn fetch_info(repo_id: &str, token: Option<&str>) -> Result<Value, String> {
    DownloadManager::validate_repo_id(repo_id)?;
    let url = format!("{HF_API}/models/{repo_id}?blobs=true");
    let res = DownloadManager::apply_hf_auth(client()?.get(&url), &url, token)
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, &url))?;
    let token_present = token.is_some_and(|t| !t.trim().is_empty());
    if let Some(reason) = hf_refusal(res.status(), res.headers(), repo_id, token_present) {
        return Err(reason);
    }
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(err("hf.repo_not_accessible").with("repo", repo_id).into());
    }
    if !res.status().is_success() {
        return Err(err("http.status")
            .with("status", res.status())
            .with("url", &url)
            .into());
    }
    res.json()
        .await
        .map_err(|e| crate::http_error::transport(&e, &url))
}

/// Architecture list of the converter source for the managed runtime's tag, fetching the
/// source (about 40 MB, once per tag) when it is not there yet.
pub async fn converter_architectures(tag: &str) -> Result<Architectures, String> {
    let root = env::root();
    let cancel = super::Cancel::default();
    let report = super::Progress::new(|_| {});
    let dir = env::ensure_source(&root, tag, &cancel, &report).await?;
    tokio::task::spawn_blocking(move || {
        super::arch::scan_dir(&dir).map_err(|e| {
            String::from(
                err("fs.read_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Plans the conversion of `repo_id`. `quant` only decides nothing here (every type is
/// estimated); the parameter exists so callers read like [`super::start`].
pub async fn plan(repo_id: &str, _quant: Option<&str>) -> Result<ConvertPlan, String> {
    let token = crate::hf_auth::current_token().await;
    let mut info = fetch_info(repo_id, token.as_deref()).await?;
    if embedding_parameters_of(&info) == 0 {
        if let Some(full) = fetch_config(repo_id, &info, token.as_deref()).await {
            merge_config(&mut info, full);
        }
    }
    let tag = super::runtime_tag();
    let archs = match &tag {
        Some(tag) => converter_architectures(tag).await.ok(),
        None => None,
    };
    let models = crate::config::AppSettings::load().primary_models_dir();
    let (free, _) = StorageManager::get_directory_disk_space(&models);
    let env_ready = env::status().ready;
    let mut plan = build_plan(
        repo_id,
        &info,
        archs.as_ref(),
        free,
        models.join(".cache").join("convert"),
        env_ready,
        tag.clone(),
    );
    if plan.convertible && tag.is_none() {
        plan.convertible = false;
        plan.reason = Some(err("convert.runtime_missing").into());
    }
    Ok(plan)
}
