use crate::config::AppSettings;
use crate::error_key::err;
use crate::paths;
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::io::{AsyncWriteExt, BufWriter};

const USER_AGENT: &str = "HarnessStudio/0.1.2";
#[path = "downloader_task_listing.rs"]
mod task_listing;

const HF_API: &str = "https://huggingface.co/api";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfModelSearchResult {
    pub id: String,
    pub author: Option<String>,
    pub downloads: u64,
    pub likes: u64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pipeline_tag: Option<String>,
    /// Task guessed from tags / file names when `pipeline_tag` is empty
    /// ([`crate::hub_tasks::infer_task`]); never set together with `pipeline_tag`.
    #[serde(default)]
    pub inferred_task: Option<String>,
    /// Where `inferred_task` came from (tags, name, files, library, GGUF header).
    #[serde(default)]
    pub inferred_source: Option<crate::hub_tasks::TaskSource>,
    /// Set when llama-server cannot run the repo at all (no GGUF file, adapter, image model).
    #[serde(default)]
    pub not_runnable: Option<crate::hub_tasks::NotRunnable>,
    /// A LoRA adapter repo: loads on top of a base model. A GGUF LLM adapter stays
    /// downloadable; a diffusion one is not runnable.
    #[serde(default)]
    pub adapter: bool,
    /// The adapter's base model (`base_model:adapter:` tag).
    #[serde(default)]
    pub adapter_base: Option<String>,
    /// Model file format read from the listing (tags, library, file names).
    pub format: crate::hub_compat::ModelFormat,
    /// What Harness can do with the repo; the Tauri command settles runtime availability
    /// ([`crate::hub_compat::resolve_installed`]).
    pub compatibility: crate::hub_compat::Compatibility,
    /// What the repo can do, from its task and format ([`crate::model_registry::hub_capabilities`]).
    #[serde(default)]
    pub capabilities: Vec<crate::model_registry::Capability>,
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub gated: Option<String>,
    /// From the `license:<id>` tag.
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub quants: Vec<String>,
    /// Repo creation time (ISO 8601).
    #[serde(default)]
    pub created_at: Option<String>,
    /// A diffusers pipeline of the Python engine whose download includes PyTorch `.bin` weights
    /// (a component without safetensors): they load as weights only, never as code.
    #[serde(default)]
    pub pickle_weights: bool,
    /// A safetensors chat repo the Python engine runs with transformers when mistral.rs is not
    /// registered ([`HfModelSearchResult::resolve_installed`] switches `compatibility`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub python_chat: bool,
}

impl HfModelSearchResult {
    /// Settles which installed runtime family runs the repo ([`crate::hub_compat::resolve_installed`]).
    /// Order: GGUF (llama.cpp), then mistral.rs, then the Python engine: a safetensors chat repo
    /// whose mistral.rs verdict has no registered runtime falls back to the Python engine.
    pub fn resolve_installed(&mut self, installed: &[String]) {
        if self.python_chat
            && matches!(
                &self.compatibility,
                crate::hub_compat::Compatibility::Runnable { runtime }
                    if runtime == crate::hub_compat::MISTRALRS_FAMILY
            )
            && !installed
                .iter()
                .any(|f| f == crate::hub_compat::MISTRALRS_FAMILY)
        {
            self.compatibility = crate::hub_compat::Compatibility::Runnable {
                runtime: crate::hub_compat::PYTHON_HF_FAMILY.to_string(),
            };
        }
        crate::hub_compat::resolve_installed(&mut self.compatibility, installed);
    }
}

/// Hugging Face search filters. Empty/None fields are not sent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HfSearchOptions {
    pub query: String,
    /// e.g. text-generation, image-text-to-text, feature-extraction
    pub pipeline_tag: Option<String>,
    /// License id, e.g. apache-2.0, mit, llama3.1
    pub license: Option<String>,
    pub author: Option<String>,
    /// With a task filter, also list repos that have no `pipeline_tag` but whose task is
    /// inferred (default true; see [`DownloadManager::search_models_page`]).
    pub include_untagged: Option<bool>,
    /// downloads | likes | trendingScore | lastModified | createdAt
    pub sort: Option<String>,
    pub limit: Option<u32>,
    /// Also list repos that are not GGUF (safetensors, ONNX, whisper ggml...). Default false:
    /// the listing is `filter=gguf`.
    pub include_non_gguf: Option<bool>,
    /// Opaque page cursor from a previous [`HfSearchPage::next_cursor`].
    pub cursor: Option<String>,
}

/// One page of [`DownloadManager::search_models_page`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HfSearchPage {
    pub items: Vec<HfModelSearchResult>,
    /// Cursor of the next page (`Link: <...&cursor=...>; rel="next"`); None on the last page.
    pub next_cursor: Option<String>,
}

/// Model card + repo metadata for the details view.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HfModelCard {
    pub repo_id: String,
    /// README.md without its YAML front matter (capped).
    pub readme: Option<String>,
    pub license: Option<String>,
    /// "auto" | "manual" when gated.
    pub gated: Option<String>,
    pub base_model: Vec<String>,
    pub tags: Vec<String>,
    pub pipeline_tag: Option<String>,
    /// Guessed task when the repo has no `pipeline_tag`.
    #[serde(default)]
    pub inferred_task: Option<String>,
    #[serde(default)]
    pub inferred_source: Option<crate::hub_tasks::TaskSource>,
    #[serde(default)]
    pub not_runnable: Option<crate::hub_tasks::NotRunnable>,
    /// The base model of a LoRA adapter repo.
    #[serde(default)]
    pub adapter_base: Option<String>,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
}

/// Whether the current HF token can download from a (gated) repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfAccess {
    pub gated: Option<String>,
    pub token_present: bool,
    /// None when it couldn't be determined (e.g. no GGUF file / network error).
    pub has_access: Option<bool>,
    /// License id from the card / tags (shown next to "license acceptance required").
    #[serde(default)]
    pub license: Option<String>,
    /// Keyed error explaining a missing access (`hub.gated_no_token`, `hub.gated_no_access`,
    /// `hf.repo_not_accessible`, `hf.rate_limited`).
    #[serde(default)]
    pub reason: Option<String>,
}

/// Owner/repo of a huggingface.co file URL (`https://huggingface.co/<o>/<r>/resolve/...`).
pub fn repo_from_hf_url(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    if u.scheme() != "https" || !matches!(host.as_str(), "huggingface.co" | "hf.co") {
        return None;
    }
    let mut seg = u.path_segments()?;
    let owner = seg.next().filter(|s| !s.is_empty())?;
    let repo = seg.next().filter(|s| !s.is_empty())?;
    let repo_id = format!("{owner}/{repo}");
    DownloadManager::validate_repo_id(&repo_id).ok()?;
    Some(repo_id)
}

/// Keyed error for a Hugging Face refusal, or None when `status` is not one: gated repos
/// (`X-Error-Code: GatedRepo`) -> `hub.gated_no_token` / `hub.gated_no_access`, other 401/403
/// (private or missing repo) -> `hf.repo_not_accessible`, 429 -> `hf.rate_limited`.
pub fn hf_refusal(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    repo: &str,
    token_present: bool,
) -> Option<String> {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Some(crate::hf_auth::rate_limited(headers));
    }
    if status != reqwest::StatusCode::UNAUTHORIZED && status != reqwest::StatusCode::FORBIDDEN {
        return None;
    }
    let gated = headers
        .get("x-error-code")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.eq_ignore_ascii_case("GatedRepo"));
    let key = match (gated, token_present) {
        (true, false) => "hub.gated_no_token",
        (true, true) => "hub.gated_no_access",
        (false, _) => "hf.repo_not_accessible",
    };
    Some(err(key).with("repo", repo).into())
}

const SORT_KEYS: &[&str] = &[
    "downloads",
    "likes",
    "trendingScore",
    "lastModified",
    "createdAt",
];
const README_MAX_BYTES: usize = 200 * 1024;

fn gated_value(v: Option<&serde_json::Value>) -> Option<String> {
    let v = v?;
    match v.as_str() {
        Some(s) => Some(s.to_string()),
        None if v.as_bool().unwrap_or(false) => Some("true".to_string()),
        None => None,
    }
}

fn string_list(v: Option<&serde_json::Value>) -> Vec<String> {
    match v {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|t| t.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

fn license_from_tags(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|t| t.strip_prefix("license:").map(String::from))
}

/// File names of a listing item's `siblings`.
fn sibling_files(item: &serde_json::Value) -> Vec<String> {
    item.get("siblings")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|f| f.get("rfilename").and_then(|v| v.as_str()))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn quants_from_siblings(item: &serde_json::Value) -> Vec<String> {
    let siblings = match item.get("siblings").and_then(|s| s.as_array()) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut set = std::collections::BTreeSet::new();
    for sib in siblings {
        if let Some(rfilename) = sib.get("rfilename").and_then(|v| v.as_str()) {
            if rfilename.to_lowercase().ends_with(".gguf") {
                let q = crate::gguf::extract_quant_from_filename(rfilename);
                if !q.is_empty() && q != "Unknown" {
                    set.insert(q);
                }
            } else if is_ggml_bin_name(rfilename) {
                // whisper.cpp models: no quant in the name means f16.
                set.insert(
                    match crate::gguf::extract_quant_from_filename(rfilename).as_str() {
                        "Unknown" | "unknown" | "" => "F16".to_string(),
                        q => q.to_string(),
                    },
                );
            }
        }
    }
    set.into_iter().collect()
}

/// Removes a leading `---\n...\n---` YAML block.
pub fn strip_front_matter(readme: &str) -> &str {
    let trimmed = readme.trim_start_matches('\u{feff}');
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.trim_start_matches(['\r', '\n']);
        }
    }
    trimmed
}

/// Fields one listing item carries (see [`search_params`]).
const LISTING_FIELDS: &[&str] = &[
    "author",
    "downloads",
    "likes",
    "trendingScore",
    "lastModified",
    "createdAt",
    "gated",
    "tags",
    "siblings",
    "pipeline_tag",
    "library_name",
    "gguf",
    "config",
];

/// Query parameters of a `GET /api/models` listing. GGUF (`filter=gguf`) unless
/// `include_non_gguf` is set, and descending;
/// `search` is omitted for an empty query, so filters + sort alone list models. Values that are
/// not plain identifiers are dropped rather than sent.
pub fn search_params(options: &HfSearchOptions) -> Vec<(&'static str, String)> {
    let limit = options.limit.unwrap_or(30).clamp(1, 100).to_string();
    let mut params: Vec<(&'static str, String)> = Vec::new();
    let query = options.query.trim();
    if !query.is_empty() {
        params.push(("search", query.to_string()));
    }
    if !options.include_non_gguf.unwrap_or(false) {
        params.push(("filter", "gguf".to_string()));
    }
    let sort = options
        .sort
        .as_deref()
        .filter(|s| SORT_KEYS.contains(s))
        .unwrap_or("downloads");
    params.push(("sort", sort.to_string()));
    params.push(("direction", "-1".to_string()));
    params.push(("limit", limit));
    // `expand[]` instead of `full=true`: the full listing has no `gguf` header block or
    // `library_name`, which the task inference reads (`hub_tasks::analyze_repo`).
    for field in LISTING_FIELDS {
        params.push(("expand[]", (*field).to_string()));
    }
    match options.pipeline_tag.as_deref().filter(|v| simple_token(v)) {
        // Decision models are found by their tag, not a `pipeline_tag`.
        Some(crate::hub_tasks::DECISION_TASK) => {
            params.push(("filter", crate::hub_tasks::DECISION_TASK.to_string()))
        }
        Some(tag) => params.push(("pipeline_tag", tag.to_string())),
        None => {}
    }
    if let Some(license) = options.license.as_deref().filter(|v| simple_token(v)) {
        params.push(("filter", format!("license:{}", license)));
    }
    if let Some(author) = options
        .author
        .as_deref()
        .map(str::trim)
        .filter(|v| simple_token(v))
    {
        params.push(("author", author.to_string()));
    }
    if let Some(cursor) = options.cursor.as_deref().filter(|c| cursor_token(c)) {
        params.push(("cursor", cursor.to_string()));
    }
    params
}

/// A page cursor as Hugging Face issues it (base64 of a JSON object).
fn cursor_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 2048
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
}

/// The `cursor` of the `rel="next"` entry of a `Link` header value.
fn next_cursor_from_link(link: &str) -> Option<String> {
    link.split(',').find_map(|entry| {
        let (target, rest) = entry.trim().split_once(';')?;
        if !rest.split(';').any(|p| {
            let p = p.trim();
            p == "rel=\"next\"" || p == "rel=next"
        }) {
            return None;
        }
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        let url = reqwest::Url::parse(target).ok()?;
        url.query_pairs()
            .find(|(k, _)| k == "cursor")
            .map(|(_, v)| v.into_owned())
            .filter(|c| cursor_token(c))
    })
}

/// `<tagged>.<untagged>` cursor of a merged page (see `search_models_page`); a plain HF
/// cursor (no `.`) belongs to the tagged listing alone. A missing half is `None`.
fn split_cursor(cursor: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(c) = cursor else {
        return (None, None);
    };
    match c.split_once('.') {
        Some((a, b)) => (
            Some(a).filter(|a| cursor_token(a)).map(String::from),
            Some(b).filter(|b| cursor_token(b)).map(String::from),
        ),
        None => (Some(c.to_string()), None),
    }
}

/// Sort value of a listing item as a string that orders like the sort key (descending sorts
/// compare these): numbers zero-padded, ISO dates as they are.
fn sort_key(item: &serde_json::Value, sort: &str) -> String {
    let field = match sort {
        "likes" => "likes",
        "trendingScore" => "trendingScore",
        "lastModified" => "lastModified",
        "createdAt" => "createdAt",
        _ => "downloads",
    };
    match item.get(field) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) => format!("{:020.3}", v.as_f64().unwrap_or(0.0).max(0.0)),
        None => String::new(),
    }
}

/// Whether stable-diffusion.cpp runs a text-to-image / video repo: its weights are a GGUF or a
/// single `.safetensors` / `.ckpt` at the repo root (a diffusers repo's folders are not
/// loadable). When it does, the repo is not "not runnable": llama-server cannot run it, the
/// image runtime can.
fn image_route(item: &serde_json::Value, analysis: &mut crate::hub_tasks::RepoAnalysis) -> bool {
    use crate::hub_tasks::NotRunnable;
    // An untagged repo whose task was inferred as text-to-image (a diffusion GGUF) takes the
    // image route too.
    let image_task = matches!(
        analysis.pipeline_tag.as_deref().or(analysis.inferred_task),
        Some("text-to-image" | "image-to-image" | "text-to-video" | "image-to-video")
    );
    if !image_task || analysis.not_runnable == Some(NotRunnable::Adapter) {
        return false;
    }
    let files = sibling_files(item);
    if crate::hub_compat::is_sdxl_refiner(item["id"].as_str().unwrap_or_default(), &files) {
        return false;
    }
    let loadable = crate::image_bundles::repo_has_image_files(&files)
        || files
            .iter()
            .any(|f| f.to_ascii_lowercase().ends_with(".gguf"));
    if loadable {
        analysis.not_runnable = None;
    }
    loadable
}

/// Adds `extras` (runnable non-GGUF repos) to a first page, skipping ids it already lists, and
/// puts the page back in the requested order (extras with the same key as listed ones sort
/// after them).
fn merge_runnable_extras(
    items: &mut Vec<HfModelSearchResult>,
    extras: Vec<HfModelSearchResult>,
    sort: Option<&str>,
) {
    let before = items.len();
    for e in extras {
        if !items.iter().any(|m| m.id == e.id) {
            items.push(e);
        }
    }
    if items.len() == before {
        return;
    }
    match sort
        .filter(|s| SORT_KEYS.contains(s))
        .unwrap_or("downloads")
    {
        "downloads" => items.sort_by_key(|a| std::cmp::Reverse(a.downloads)),
        "likes" => items.sort_by_key(|a| std::cmp::Reverse(a.likes)),
        "lastModified" => items.sort_by(|a, b| b.last_modified.cmp(&a.last_modified)),
        "createdAt" => items.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        _ => {}
    }
}

fn exact_repo_missing(options: &HfSearchOptions, items: &[HfModelSearchResult]) -> bool {
    options.cursor.is_none()
        && options.query.trim().split('/').count() == 2
        && DownloadManager::validate_repo_id(options.query.trim()).is_ok()
        && !items
            .iter()
            .any(|m| m.id.eq_ignore_ascii_case(options.query.trim()))
}

fn merge_search_extras(
    page: &mut HfSearchPage,
    extras: Vec<HfModelSearchResult>,
    whisper: Option<HfModelSearchResult>,
    sort: Option<&str>,
) {
    merge_runnable_extras(&mut page.items, extras, sort);
    // The whisper.cpp repo has no GGUF (the listing is `filter=gguf`): it is looked up by
    // name and put first when the search is for speech models.
    if let Some(repo) = whisper {
        if !page.items.iter().any(|m| m.id == repo.id) {
            page.items.insert(0, repo);
        }
    }
}

fn prioritize_exact_repo(items: &mut [HfModelSearchResult], query: &str) {
    let query = query.trim();
    if query.split('/').count() == 2 {
        items.sort_by_key(|item| !item.id.eq_ignore_ascii_case(query));
    }
}

/// The details card shows the listing's verdict: "no GGUF file" is llama-server's reason and
/// does not apply when the listing found another engine (ONNX, Python, mistral.rs) for the repo.
fn card_not_runnable(
    info: &serde_json::Value,
    fallback: Option<crate::hub_tasks::NotRunnable>,
) -> Option<crate::hub_tasks::NotRunnable> {
    parse_search_item(info).map_or(fallback, |listed| listed.not_runnable)
}

fn parse_search_item(item: &serde_json::Value) -> Option<HfModelSearchResult> {
    let str_field = |k: &str| item.get(k).and_then(|v| v.as_str()).map(String::from);
    let id = str_field("id")?;
    let tags = string_list(item.get("tags"));
    let mut analysis = crate::hub_tasks::analyze_repo(item);
    let image = image_route(item, &mut analysis);
    let ctc = item
        .get("config")
        .is_some_and(crate::hub_compat::is_ctc_config);
    let task = analysis.pipeline_tag.as_deref().or(analysis.inferred_task);
    let ctc_export = ctc && crate::hub_compat::has_ctc_files(&sibling_files(item));
    let refiner = crate::hub_compat::is_sdxl_refiner(&id, &sibling_files(item));
    let native_speech = crate::hub_compat::native_speech_format(item, &sibling_files(item));
    let format = if refiner {
        crate::hub_compat::ModelFormat::Diffusers
    } else if let Some(format) = native_speech {
        format
    } else if ctc_export {
        crate::hub_compat::ModelFormat::Onnx
    } else {
        crate::hub_compat::detect_hub_format(
            &tags,
            item.get("library_name").and_then(|v| v.as_str()),
            &sibling_files(item),
            task,
        )
    };
    // An MLX speech recognition repo without a task (the card shows what was inferred).
    let mlx_stt = crate::hub_compat::mlx_stt_task(&id, format, task, &tags);
    if mlx_stt.is_some() {
        analysis.inferred_task = mlx_stt;
        analysis.source = Some(crate::hub_tasks::TaskSource::Tags);
    }
    let task = task.or(mlx_stt);
    // A GGUF decision model runs on llama-server whatever its `pipeline_tag` says
    // (text-classification, zero-shot-classification...).
    let task = if format == crate::hub_compat::ModelFormat::Gguf
        && crate::hub_tasks::is_decision_repo(&tags)
    {
        Some(crate::hub_tasks::DECISION_TASK)
    } else {
        task
    };
    let mut capabilities = crate::model_registry::hub_capabilities(task, format);
    let adapter = analysis.not_runnable == Some(crate::hub_tasks::NotRunnable::Adapter);
    // A diffusion LoRA adapter generates nothing on its own and no runtime loads it; an LLM
    // GGUF LoRA stays downloadable (llama-server loads it next to its base model).
    let diffusion_adapter = analysis.diffusion_adapter;
    if diffusion_adapter {
        capabilities.clear();
    }
    let genai = crate::hub_compat::is_genai_repo(&tags, &sibling_files(item));
    let compatibility = if crate::hub_compat::is_audio_cpp_repo(item) {
        crate::hub_compat::audio_cpp_compatibility(item)
    } else if format == crate::hub_compat::ModelFormat::Gguf && crate::tts::llama::is_hub_repo(item)
    {
        // Qwen3-TTS GGUF: the runtime's `llama-tts` speaks with it.
        crate::hub_compat::Compatibility::Runnable {
            runtime: crate::hub_compat::LLAMA_FAMILY.into(),
        }
    } else if format == crate::hub_compat::ModelFormat::Gguf
        && crate::engine::transcribe_cpp::is_hub_repo(item)
    {
        // Before CrispASR: handy-computer's exports are transcribe.cpp's.
        crate::hub_compat::Compatibility::Runnable {
            runtime: crate::engine::TRANSCRIBE_CPP.into(),
        }
    } else if crate::engine::python_hf::is_liquid_audio_repo(item) {
        // LFM2.5-Audio in Liquid's own format: speech output through `liquid-audio`.
        crate::hub_compat::Compatibility::Runnable {
            runtime: crate::engine::python_hf::LIQUID_AUDIO.into(),
        }
    } else if format == crate::hub_compat::ModelFormat::Gguf
        && (crate::engine::crispasr::supports_repo(&id, task)
            || crate::engine::crispasr_tts::supports_repo(&id, task))
    {
        crate::hub_compat::Compatibility::Runnable {
            runtime: crate::engine::CRISPASR.into(),
        }
    } else if native_speech.is_some() {
        crate::hub_compat::Compatibility::Runnable {
            runtime: crate::engine::PYTHON_HF.into(),
        }
    } else if ctc_export {
        format.compatibility()
    } else if diffusion_adapter {
        // sd.cpp LoRA usage is not wired in Harness.
        crate::hub_compat::Compatibility::Unsupported { format }
    } else if let Some(mflux) =
        crate::hub_compat::mflux_compatibility(task, &sibling_files(item), &tags)
    {
        // A FLUX.2 [klein] conversion in mflux's format (Apple Silicon).
        mflux
    } else if image {
        crate::model_registry::hub_compatibility(format, &capabilities)
    } else {
        // A safetensors chat model runs on mistral.rs (when the user registered it).
        // A transformers task model (classifier, NER, ASR...) runs on the Python engine.
        // An MLX chat model runs on the MLX engine (Apple Silicon).
        crate::hub_compat::mistralrs_compatibility(
            format,
            task,
            &sibling_files(item),
            item.get("config"),
        )
        .or_else(|| {
            crate::hub_compat::mlx_compatibility(&id, format, task, &sibling_files(item), &tags)
        })
        .or_else(|| {
            crate::hub_compat::python_hf_compatibility(
                format,
                item.get("library_name").and_then(|v| v.as_str()),
                task,
                &tags,
                &sibling_files(item),
                item.get("config"),
            )
        })
        .unwrap_or_else(|| crate::hub_compat::hub_compatibility_for(format, task, genai))
    };
    // Keep native-folder precedence, but refuse processors whose required package is not
    // shipped by the Python engine. This precise reason supersedes llama-server's NoGguf.
    let missing_dependency = matches!(
        &compatibility,
        crate::hub_compat::Compatibility::Runnable { runtime }
            | crate::hub_compat::Compatibility::NeedsRuntime { runtime }
            if runtime == crate::engine::PYTHON_HF
    )
    .then(|| crate::engine::python_hf::missing_dependency(item["config"]["model_type"].as_str()))
    .flatten();
    let compatibility = if missing_dependency.is_some() {
        analysis.not_runnable = Some(crate::hub_tasks::NotRunnable::NeedsLibrosa);
        crate::hub_compat::Compatibility::Unsupported { format }
    } else {
        compatibility
    };
    // A speech repo made for its own package (Qwen3-TTS, VibeVoice streaming...) would show as
    // runnable on the Python engine and fail at load.
    let compatibility = match &compatibility {
        crate::hub_compat::Compatibility::Runnable { runtime }
            if runtime == crate::engine::PYTHON_HF
                && !crate::engine::python_hf::serves_model_type(
                    task,
                    item.get("config")
                        .and_then(|c| c.get("model_type"))
                        .and_then(|v| v.as_str()),
                ) =>
        {
            crate::hub_compat::Compatibility::Unsupported { format }
        }
        _ => compatibility,
    };
    // An ONNX translator / captioner of an architecture the sidecar does not serve (NLLB, BLIP)
    // would show as runnable and fail at launch.
    let compatibility = if format == crate::hub_compat::ModelFormat::Onnx
        && !genai
        && !crate::hub_compat::onnx_config_served(task, item.get("config"))
    {
        crate::hub_compat::Compatibility::Unsupported { format }
    } else {
        compatibility
    };
    // "No GGUF file" is llama-server's verdict; an ONNX encoder or an image model runs elsewhere.
    let package_reason = if crate::hub_compat::unsupported_vibevoice(
        item.get("config"),
        &sibling_files(item),
    ) {
        Some(crate::hub_tasks::NotRunnable::VibevoiceLayout)
    } else if crate::hub_compat::unsupported_auk_package(&id) {
        Some(crate::hub_tasks::NotRunnable::AudioCppIncompletePackage)
    } else if refiner
        && (!sibling_files(item).iter().any(|f| f == "model_index.json")
            || !matches!(compatibility, crate::hub_compat::Compatibility::Runnable { runtime: ref r } if r == crate::engine::PYTHON_HF))
    {
        Some(crate::hub_tasks::NotRunnable::SdxlRefinerFolderRequired)
    } else {
        None
    };
    let compatibility = if package_reason.is_some() {
        crate::hub_compat::Compatibility::Unsupported { format }
    } else {
        compatibility
    };
    let not_runnable = package_reason.or(analysis.not_runnable.filter(|_| {
        diffusion_adapter
            || !matches!(
                compatibility,
                crate::hub_compat::Compatibility::Runnable { .. }
            )
    }));
    // "No GGUF" is the catch-all; name the precise reason when the listing shows one.
    let not_runnable = match not_runnable {
        Some(crate::hub_tasks::NotRunnable::NoGguf) => {
            let files = sibling_files(item);
            Some(crate::hub_tasks::refine_no_gguf(
                &crate::hub_tasks::RefusalFacts {
                    repo_id: &id,
                    tags: &tags,
                    files: &files,
                    library: item.get("library_name").and_then(|v| v.as_str()),
                    task,
                    format,
                    config: item.get("config"),
                    apple: cfg!(target_os = "macos"),
                },
            ))
        }
        other => other,
    };
    // An untagged genai repo (microsoft/Phi-4-mini-instruct-onnx) is a chat model all the same.
    if genai
        && capabilities.is_empty()
        && matches!(
            compatibility,
            crate::hub_compat::Compatibility::Runnable { .. }
        )
    {
        capabilities.push(crate::model_registry::Capability::Chat);
    }
    let pickle_weights = format == crate::hub_compat::ModelFormat::Diffusers
        && matches!(
            &compatibility,
            crate::hub_compat::Compatibility::Runnable { runtime }
                | crate::hub_compat::Compatibility::NeedsRuntime { runtime }
                if runtime == crate::hub_compat::PYTHON_HF_FAMILY
        )
        && crate::hub_compat::diffusers_pickle_weights(&sibling_files(item));
    let python_chat = format == crate::hub_compat::ModelFormat::Safetensors
        && task.is_some_and(|t| crate::engine::python_hf::CHAT_TASKS.contains(&t))
        && crate::hub_compat::python_hf_compatibility(
            format,
            item.get("library_name").and_then(|v| v.as_str()),
            task,
            &tags,
            &sibling_files(item),
            item.get("config"),
        )
        .is_some();
    Some(HfModelSearchResult {
        id,
        author: str_field("author"),
        downloads: item.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
        likes: item.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
        license: license_from_tags(&tags),
        tags,
        pipeline_tag: analysis.pipeline_tag,
        inferred_task: analysis.inferred_task.map(String::from),
        inferred_source: analysis.source,
        not_runnable,
        adapter,
        adapter_base: analysis.adapter_base,
        format,
        compatibility,
        capabilities,
        last_modified: str_field("lastModified"),
        gated: gated_value(item.get("gated")),
        quants: quants_from_siblings(item),
        created_at: str_field("createdAt"),
        pickle_weights,
        python_chat,
    })
}

/// The Hugging Face repo of whisper.cpp's ggml models (`ggml-base.bin`, ...).
pub const WHISPER_CPP_REPO: &str = "ggerganov/whisper.cpp";

/// Whether a listing request should show [`WHISPER_CPP_REPO`] on its first page: the speech
/// task without a query, or a query that names whisper / ggml, in both cases only when the
/// task, publisher and license filters do not exclude the repo (an MIT repo by `ggerganov`).
pub fn wants_whisper_repo(options: &HfSearchOptions) -> bool {
    if options.cursor.is_some() {
        return false;
    }
    let task_ok = options
        .pipeline_tag
        .as_deref()
        .is_none_or(|t| t == "automatic-speech-recognition");
    let author_ok = options
        .author
        .as_deref()
        .map(str::trim)
        .is_none_or(|a| a.is_empty() || a.eq_ignore_ascii_case("ggerganov"));
    let license_ok = options
        .license
        .as_deref()
        .map(str::trim)
        .is_none_or(|l| l.is_empty() || l.eq_ignore_ascii_case("mit"));
    if !(task_ok && author_ok && license_ok) {
        return false;
    }
    let query = options.query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return options.pipeline_tag.as_deref() == Some("automatic-speech-recognition");
    }
    query.contains("whisper")
        || query
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w == "ggml")
}

fn simple_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn clean_hf_search_query(raw: &str) -> String {
    let re_parens = match Regex::new(r"[\(\[\{][^\)\]\}]*[\)\]\}]") {
        Ok(r) => r,
        Err(_) => return raw.trim().to_string(),
    };
    let without_parens = re_parens.replace_all(raw, " ");

    let re_quant = match Regex::new(
        r"(?i)\b(q[0-9]_[a-z0-9_]+|iq[0-9]_[a-z0-9_]+|tq[0-9]_[a-z0-9_]+|mxfp4(?:_moe)?|bf16|fp16|f16|f32)\b",
    ) {
        Ok(r) => r,
        Err(_) => return without_parens.trim().to_string(),
    };
    let without_quant = re_quant.replace_all(&without_parens, " ");

    let re_gguf = match Regex::new(r"(?i)\.gguf\b|\bgguf\b") {
        Ok(r) => r,
        Err(_) => return without_quant.trim().to_string(),
    };
    let without_gguf = re_gguf.replace_all(&without_quant, " ");

    without_gguf
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelArtifactUpdate {
    pub repo_id: String,
    pub filename: String,
    pub has_update: bool,
    pub remote_size: u64,
    pub local_size: u64,
    pub remote_oid: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HfShardInfo {
    pub filename: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub shard_index: u32,
    pub total_shards: u32,
    /// SHA-256 of the file as published by Hugging Face (LFS oid), when known.
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HfModelVariant {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub not_runnable: Option<crate::hub_tasks::NotRunnable>,
    pub quantization: String,
    pub total_size_bytes: u64,
    pub is_split: bool,
    pub shards: Vec<HfShardInfo>,
}

/// The single-file variant of exactly `file` in a repo listing (`items`), or `None` when the
/// repo has no such file.
fn companion_variant_from(
    repo: &str,
    file: &str,
    items: &[serde_json::Value],
) -> Result<Option<HfModelVariant>, String> {
    let Some(item) = items
        .iter()
        .find(|i| i.get("path").and_then(|v| v.as_str()) == Some(file))
    else {
        return Ok(None);
    };
    let size_bytes = item
        .get("lfs")
        .and_then(|l| l.get("size"))
        .and_then(|v| v.as_u64())
        .or_else(|| item.get("size").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let sha256 = item
        .get("lfs")
        .and_then(|l| l.get("oid"))
        .and_then(|v| v.as_str())
        .and_then(valid_sha256);
    let shard = HfShardInfo {
        filename: file.to_string(),
        download_url: DownloadManager::hf_file_url(repo, "resolve", file)?.to_string(),
        size_bytes,
        shard_index: 1,
        total_shards: 1,
        sha256,
    };
    let lower = file.to_ascii_lowercase();
    let quantization = if lower.ends_with(".gguf") {
        crate::gguf::extract_quant_from_filename(file)
    } else {
        crate::image_bundles::variant_quant(file)
    };
    Ok(Some(HfModelVariant {
        name: file
            .trim_end_matches(".gguf")
            .trim_end_matches(".safetensors")
            .trim_end_matches(".ckpt")
            .to_string(),
        not_runnable: None,
        quantization,
        total_size_bytes: size_bytes,
        is_split: false,
        shards: vec![shard],
    }))
}

/// Whether a repo path sits in a folder of model parts (text encoders, VAE, tokenizers...).
fn in_component_folder(lower_path: &str) -> bool {
    let mut parts: Vec<&str> = lower_path.split(['/', '\\']).collect();
    parts.pop();
    parts.iter().any(|d| {
        matches!(
            *d,
            "text_encoders"
                | "text_encoder"
                | "vae"
                | "vision_encoders"
                | "vision_encoder"
                | "audio_encoders"
                | "audio_encoder"
                | "tokenizers"
        )
    })
}

/// The vision projector a repo ships itself (best precision first), from its file listing.
fn own_projector_from(main: &str, items: &[serde_json::Value]) -> Option<String> {
    crate::image_bundles::select_own_projector(
        main,
        items
            .iter()
            .filter(|i| i.get("type").and_then(|v| v.as_str()) != Some("directory"))
            .filter_map(|i| i.get("path").and_then(|v| v.as_str())),
    )
    .map(str::to_string)
}

/// What a model needs besides itself: the curated image bundle of its main file, or the codec
/// of an mlx-audio text-to-speech repo.
pub fn companion_plan(
    main_file: &str,
    repo: Option<&str>,
    libraries: &[PathBuf],
) -> Option<crate::image_bundles::BundlePlan> {
    crate::image_bundles::plan_for(main_file, libraries, None)
        .or_else(|| repo.and_then(|r| crate::engine::mlx::codec_plan_for_repo(r, libraries)))
}

/// [`companion_plan`] with the Hub listing of `repo` checked: a Whisper repo whose root has its
/// own processor ([`crate::engine::mlx::has_whisper_processor`]) needs no processor companion.
/// When the listing cannot be read, the plan stands (the processor is a few MB).
pub async fn companion_plan_checked(
    main_file: &str,
    repo: Option<&str>,
    libraries: &[PathBuf],
    token: Option<&str>,
) -> Option<crate::image_bundles::BundlePlan> {
    let plan = companion_plan(main_file, repo, libraries)?;
    let Some(repo) = repo.filter(|_| plan.bundle_id == crate::engine::mlx::WHISPER_BUNDLE) else {
        return Some(plan);
    };
    checked_whisper_plan(plan, DownloadManager::list_repo_files(repo, token).await)
}

/// [`companion_plan_checked`]'s verdict on a Whisper processor plan from the repo's listing: no
/// plan when its root holds a processor of its own; the plan when the listing failed (offline,
/// rate limited), as a processor too many is harmless and one missing breaks the model.
fn checked_whisper_plan(
    plan: crate::image_bundles::BundlePlan,
    listing: Result<Vec<serde_json::Value>, String>,
) -> Option<crate::image_bundles::BundlePlan> {
    match listing {
        Ok(items) => {
            let root: Vec<&str> = items
                .iter()
                .filter_map(|i| i.get("path").and_then(|v| v.as_str()))
                .filter(|p| !p.contains('/'))
                .collect();
            (!own_whisper_processor(&root)).then_some(plan)
        }
        Err(_) => Some(plan),
    }
}

/// Whether a repo's root files hold a Whisper processor of its own.
fn own_whisper_processor(root: &[&str]) -> bool {
    crate::engine::mlx::has_whisper_processor(|name| root.contains(&name))
}

/// Each audio.cpp GGUF is a model variant with only its own folder's companion
/// files. In the package repo, unrelated families and languages stay separate.
fn audio_cpp_variants(
    repo: &str,
    items: &[serde_json::Value],
    not_runnable: Option<crate::hub_tasks::NotRunnable>,
) -> Result<Vec<HfModelVariant>, String> {
    let mut variants = Vec::new();
    for model in items
        .iter()
        .filter_map(|i| i["path"].as_str())
        .filter(|p| p.ends_with(".gguf"))
    {
        let prefix = model
            .rsplit_once('/')
            .map(|(dir, _)| format!("{dir}/"))
            .unwrap_or_default();
        let mut shards = Vec::new();
        for item in items {
            let Some(path) = item["path"].as_str() else {
                continue;
            };
            if item["type"] == "directory"
                || !path.starts_with(&prefix)
                || (path.ends_with(".gguf") && path != model)
            {
                continue;
            }
            // Nested GGUF folders are other variants, not this model's companions.
            if items
                .iter()
                .filter_map(|i| i["path"].as_str())
                .filter(|p| *p != model && p.ends_with(".gguf"))
                .filter_map(|p| p.rsplit_once('/').map(|(d, _)| format!("{d}/")))
                .any(|p| p != prefix && p.starts_with(&prefix) && path.starts_with(&p))
            {
                continue;
            }
            shards.push(HfShardInfo {
                filename: path.into(),
                download_url: DownloadManager::hf_file_url(repo, "resolve", path)?.to_string(),
                size_bytes: item["lfs"]["size"]
                    .as_u64()
                    .or_else(|| item["size"].as_u64())
                    .unwrap_or(0),
                shard_index: 1,
                total_shards: 1,
                sha256: item["lfs"]["oid"].as_str().and_then(valid_sha256),
            });
        }
        shards.sort_by(|a, b| {
            (a.filename != model)
                .cmp(&(b.filename != model))
                .then_with(|| a.filename.cmp(&b.filename))
        });
        // Refused frontend families expose no downloadable files or voice companions.
        if not_runnable.is_some() {
            shards.clear();
        }
        variants.push(HfModelVariant {
            not_runnable: not_runnable.or_else(|| {
                (crate::engine::audio_cpp::family_from_package_path(model).is_none()
                    && !(crate::engine::audio_cpp::family_of_repo(repo).is_some()
                        && !model.contains('/')))
                .then_some(crate::hub_tasks::NotRunnable::AudioCppUnsupported)
            }),
            name: model.trim_end_matches(".gguf").into(),
            quantization: crate::gguf::extract_quant_from_filename(model),
            total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
            is_split: false,
            shards,
        });
    }
    for variant in &mut variants {
        if crate::engine::audio_cpp::family_from_package_path(&variant.name)
            .is_some_and(crate::engine::audio_cpp::espeak_unavailable)
        {
            variant.not_runnable = Some(crate::hub_tasks::NotRunnable::AudioCppEspeakMissing);
        }
    }
    variants.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(variants)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DownloadStatus {
    Idle,
    /// Waiting in the queue.
    Queued,
    Downloading,
    Verifying,
    /// Cancelled by the user; partial data is kept so it can resume.
    Paused,
    Completed,
    Error(String),
}

impl DownloadStatus {
    pub fn is_pending(&self) -> bool {
        matches!(
            self,
            DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Verifying
        )
    }

    fn is_active(&self) -> bool {
        matches!(
            self,
            DownloadStatus::Downloading | DownloadStatus::Verifying
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DownloadProgress {
    /// Queue job id.
    #[serde(default)]
    pub id: u64,
    pub repo_id: String,
    pub variant_name: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub current_shard_name: String,
    pub current_shard_index: u32,
    pub total_shards: u32,
    pub speed_bytes_per_sec: u64,
    pub status: DownloadStatus,
    #[serde(default)]
    pub created_at: u64,
}

/// One file of a Hugging Face repository at a pinned revision (see
/// [`DownloadManager::start_pinned_download`]).
#[derive(Debug, Clone, Copy)]
pub struct PinnedFile<'a> {
    pub repo: &'a str,
    pub rev: &'a str,
    pub filename: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

/// A single file to fetch as part of a download job.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadItem {
    url: String,
    filename: String,
    size: u64,
    shard_index: u32,
    sha256: Option<String>,
    #[serde(default)]
    custom_url: bool,
}

/// A queued download. The HF token is never stored with it: it is read from the
/// settings (keychain) when the job runs.
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    progress: DownloadProgress,
    items: Vec<DownloadItem>,
    target_dir: PathBuf,
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
}

/// Download queue: jobs run one at a time in order; the list (minus live speed)
/// is persisted so paused/queued downloads survive an app restart.
#[derive(Clone, Default)]
pub struct DownloadManager {
    jobs: Arc<Mutex<Vec<Job>>>,
    next_id: Arc<AtomicU64>,
    worker_active: Arc<AtomicBool>,
    /// Runtime the queue worker is spawned on; `None` uses the caller's tokio context.
    runtime: Option<tokio::runtime::Handle>,
    /// When each job last published a progress event (throttling, see [`Self::emit`]).
    last_event: Arc<Mutex<HashMap<u64, Instant>>>,
}

/// Minimum gap between two progress-only events of one job (status changes always go out).
const EVENT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Internal sentinel: a cancelled job becomes `Paused`, never shown as an error.
const CANCELLED: &str = "downloads.cancelled";
const MAX_FINISHED_KEPT: usize = 50;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `downloads.json`: v1 was a bare array of jobs; v2 is `{schema_version, jobs: [...]}`.
pub const QUEUE_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "downloads.json",
    current: 2,
    envelope: crate::store::Envelope::Wrapped("jobs"),
    migrations: &[queue_v1_to_v2],
};

fn queue_v1_to_v2(doc: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(crate::store::wrap_legacy(doc, "jobs"))
}

/// `model_hashes.json`: v1 was a bare `{path: sha256}` map; v2 is
/// `{schema_version, hashes: {...}}`.
pub const HASHES_SCHEMA: crate::store::Schema = crate::store::Schema {
    file: "model_hashes.json",
    current: 2,
    envelope: crate::store::Envelope::Wrapped("hashes"),
    migrations: &[hashes_v1_to_v2],
};

fn hashes_v1_to_v2(doc: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(crate::store::wrap_legacy(doc, "hashes"))
}

/// Bytes a resume would keep for one file: a complete target counts in full, otherwise the
/// `.part` length (the same offset `fetch_to_part` resumes from; an oversized part is reset).
fn resumable_bytes(item: &DownloadItem, target_dir: &Path, cache: &Path) -> u64 {
    let len = |path: &Path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if item.size > 0 && len(&target_dir.join(&item.filename)) == item.size {
        return item.size;
    }
    let part = len(&cache.join(part_name(&item.url, &item.filename)));
    if item.size > 0 && part > item.size {
        0
    } else {
        part
    }
}

/// A job restored as paused shows what is already on disk instead of 0 B.
fn restore_part_progress(jobs: &mut [Job], cache: &Path) {
    for job in jobs
        .iter_mut()
        .filter(|j| j.progress.status == DownloadStatus::Paused)
    {
        let on_disk: u64 = job
            .items
            .iter()
            .map(|item| resumable_bytes(item, &job.target_dir, cache))
            .sum();
        job.progress.downloaded_bytes = on_disk.max(job.progress.downloaded_bytes);
    }
}

fn queue_file() -> PathBuf {
    paths::config_dir().join("downloads.json")
}

impl DownloadManager {
    /// Manager whose queue worker runs on `runtime`, so downloads can be started
    /// from threads outside a tokio context (sync commands, app setup).
    pub fn with_runtime(runtime: tokio::runtime::Handle) -> Self {
        Self {
            runtime: Some(runtime),
            ..Self::default()
        }
    }

    /// Downloads always go to the currently configured primary library directory.
    pub fn models_dir() -> PathBuf {
        AppSettings::load().primary_models_dir()
    }

    pub fn cache_dir() -> PathBuf {
        Self::models_dir().join(".cache")
    }

    /// Loads the persisted queue (call once the data dir is known). Jobs that were
    /// running or queued when the app closed come back paused.
    pub fn restore(&self) {
        let mut jobs = match crate::store::JsonStore::sensitive(queue_file())
            .load_versioned::<Vec<Job>>(&QUEUE_SCHEMA)
        {
            Ok(Some(loaded)) => loaded.data,
            Ok(None) => return,
            Err(error) => {
                eprintln!("İndirme kuyruğu okunamadı: {}", error);
                return;
            }
        };
        for job in &mut jobs {
            job.cancel = Arc::new(AtomicBool::new(false));
            job.progress.speed_bytes_per_sec = 0;
            if job.progress.status.is_pending() {
                job.progress.status = DownloadStatus::Paused;
            }
        }
        restore_part_progress(&mut jobs, &Self::cache_dir());
        let max_id = jobs.iter().map(|j| j.progress.id).max().unwrap_or(0);
        self.next_id.store(max_id + 1, Ordering::SeqCst);
        *self.jobs.lock().unwrap() = jobs;
    }

    fn persist(&self) {
        let jobs = self.jobs.lock().unwrap().clone();
        let result =
            crate::store::JsonStore::sensitive(queue_file()).save_versioned(&QUEUE_SCHEMA, &jobs);
        if let Err(error) = result {
            eprintln!("İndirme kuyruğu kaydedilemedi: {}", error);
        }
    }

    async fn persist_async(&self) {
        let manager = self.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || manager.persist()).await {
            eprintln!("İndirme kuyruğu kaydedilemedi: {}", error);
        }
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut DownloadProgress)) {
        let changed = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.iter_mut().find(|j| j.progress.id == id) else {
                return;
            };
            let before = job.progress.status.clone();
            f(&mut job.progress);
            (job.progress.status != before, job.progress.clone())
        };
        self.emit(changed.1, changed.0);
    }

    /// Publishes a job's progress on the event bus: always when `status_changed`, else at most
    /// once per [`EVENT_INTERVAL`].
    fn emit(&self, progress: DownloadProgress, status_changed: bool) {
        let now = Instant::now();
        {
            let mut last = self.last_event.lock().unwrap();
            if !status_changed
                && last
                    .get(&progress.id)
                    .is_some_and(|t| now.duration_since(*t) < EVENT_INTERVAL)
            {
                return;
            }
            last.insert(progress.id, now);
        }
        crate::events::publish(crate::events::CoreEvent::Download { progress });
    }

    fn emit_job(&self, id: u64) {
        let progress = self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.progress.id == id)
            .map(|j| j.progress.clone());
        if let Some(progress) = progress {
            self.emit(progress, true);
        }
    }

    /// Job with this id.
    pub fn get(&self, id: u64) -> Option<DownloadProgress> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.progress.id == id)
            .map(|j| j.progress.clone())
    }

    fn set_status(&self, id: u64, status: DownloadStatus) {
        self.update(id, |p| p.status = status);
    }

    /// All jobs in queue order.
    pub fn list(&self) -> Vec<DownloadProgress> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .map(|j| j.progress.clone())
            .collect()
    }

    /// The running job, else the most recently added one (legacy single-download view).
    pub fn get_progress(&self) -> Option<DownloadProgress> {
        let jobs = self.jobs.lock().unwrap();
        jobs.iter()
            .find(|j| j.progress.status.is_active())
            .or_else(|| jobs.iter().max_by_key(|j| j.progress.id))
            .map(|j| j.progress.clone())
    }

    /// Cancels a job (None = the running one). Queued jobs are paused without running.
    pub fn cancel_download(&self, id: Option<u64>) {
        let mut jobs = self.jobs.lock().unwrap();
        let job = match id {
            Some(id) => jobs.iter_mut().find(|j| j.progress.id == id),
            None => jobs.iter_mut().find(|j| j.progress.status.is_active()),
        };
        let mut paused = None;
        if let Some(job) = job {
            match job.progress.status {
                DownloadStatus::Queued => {
                    job.progress.status = DownloadStatus::Paused;
                    paused = Some(job.progress.id);
                }
                DownloadStatus::Downloading | DownloadStatus::Verifying => {
                    job.cancel.store(true, Ordering::SeqCst)
                }
                _ => {}
            }
        }
        drop(jobs);
        if let Some(id) = paused {
            self.emit_job(id);
        }
        self.persist();
    }

    /// Re-queues a paused or failed job (partial data resumes).
    pub fn resume_download(&self, id: u64) -> Result<(), String> {
        {
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter_mut()
                .find(|j| j.progress.id == id)
                .ok_or_else(|| String::from(err("downloads.not_found").num("id", id)))?;
            match job.progress.status {
                DownloadStatus::Paused | DownloadStatus::Error(_) => {
                    job.progress.status = DownloadStatus::Queued;
                    job.cancel.store(false, Ordering::SeqCst);
                }
                _ => return Err(err("downloads.not_resumable").into()),
            }
        }
        self.emit_job(id);
        self.persist();
        self.kick();
        Ok(())
    }

    /// Removes a job that isn't running. With `delete_partial`, its `.part` files go too.
    pub fn remove_download(&self, id: u64, delete_partial: bool) -> Result<(), String> {
        let removed = {
            let mut jobs = self.jobs.lock().unwrap();
            let idx = jobs
                .iter()
                .position(|j| j.progress.id == id)
                .ok_or_else(|| String::from(err("downloads.not_found").num("id", id)))?;
            if jobs[idx].progress.status.is_active() {
                return Err(err("downloads.stop_first").into());
            }
            jobs.remove(idx)
        };
        self.last_event.lock().unwrap().remove(&id);
        crate::events::publish(crate::events::CoreEvent::DownloadRemoved { id });
        if delete_partial && removed.progress.status != DownloadStatus::Completed {
            let cache = Self::cache_dir();
            for item in &removed.items {
                let part = cache.join(part_name(&item.url, &item.filename));
                if let Err(error) = fs::remove_file(&part) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        eprintln!("Kısmi indirme silinemedi ({}): {}", part.display(), error);
                    }
                }
                let metadata = part_metadata_path(&part);
                if let Err(error) = fs::remove_file(&metadata) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        eprintln!(
                            "Kısmi indirme bilgisi silinemedi ({}): {}",
                            metadata.display(),
                            error
                        );
                    }
                }
            }
        }
        self.persist();
        Ok(())
    }

    pub fn clear_finished(&self) {
        let removed: Vec<u64> = {
            let mut jobs = self.jobs.lock().unwrap();
            let done = jobs
                .iter()
                .filter(|j| j.progress.status == DownloadStatus::Completed)
                .map(|j| j.progress.id)
                .collect();
            jobs.retain(|j| j.progress.status != DownloadStatus::Completed);
            done
        };
        for id in removed {
            crate::events::publish(crate::events::CoreEvent::DownloadRemoved { id });
        }
        self.persist();
    }

    /// Moves a queued job to the front of the queue.
    pub fn prioritize(&self, id: u64) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(idx) = jobs
            .iter()
            .position(|j| j.progress.id == id && j.progress.status == DownloadStatus::Queued)
        {
            let job = jobs.remove(idx);
            let insert_at = jobs
                .iter()
                .position(|j| j.progress.status == DownloadStatus::Queued)
                .unwrap_or(jobs.len());
            jobs.insert(insert_at, job);
        }
        drop(jobs);
        self.persist();
    }

    fn client() -> Result<reqwest::Client, String> {
        static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
        CLIENT
            .get_or_init(|| {
                reqwest::Client::builder()
                    .user_agent(USER_AGENT)
                    .build()
                    .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
            })
            .clone()
    }

    fn custom_client() -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| crate::http_error::transport(&e, "HTTP client"))
    }

    /// True for HTTPS URLs on huggingface.co / hf.co (and their subdomains).
    pub fn is_hf_url(url: &str) -> bool {
        match reqwest::Url::parse(url) {
            Ok(u) if u.scheme() == "https" => match u.host_str() {
                Some(h) => {
                    let h = h.to_ascii_lowercase();
                    h == "huggingface.co"
                        || h.ends_with(".huggingface.co")
                        || h == "hf.co"
                        || h.ends_with(".hf.co")
                }
                None => false,
            },
            _ => false,
        }
    }

    /// Adds the HF token only for Hugging Face hosts, so it can never leak to
    /// arbitrary third-party URLs. (reqwest also drops it on cross-host redirects.)
    pub fn apply_hf_auth(
        builder: reqwest::RequestBuilder,
        url: &str,
        token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        match token.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) if Self::is_hf_url(url) => {
                builder.header("Authorization", format!("Bearer {}", t))
            }
            _ => builder,
        }
    }

    pub fn validate_repo_id(repo_id: &str) -> Result<(), String> {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*$").unwrap()
        });
        if re.is_match(repo_id) && !repo_id.contains("..") {
            Ok(())
        } else {
            Err(err("hf.invalid_repo_id").with("repo", repo_id).into())
        }
    }

    fn hf_file_url(repo_id: &str, operation: &str, filename: &str) -> Result<reqwest::Url, String> {
        Self::validate_repo_id(repo_id)?;
        let mut url = reqwest::Url::parse("https://huggingface.co/")
            .map_err(|e| String::from(err("hf.url_build_failed").with("error", e)))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| String::from(err("hf.url_build_failed").with("error", repo_id)))?;
            for segment in repo_id.split('/') {
                segments.push(segment);
            }
            segments.push(operation).push("main");
            for segment in filename.split('/') {
                segments.push(segment);
            }
        }
        Ok(url)
    }

    /// One page of a Hugging Face model listing (see [`search_params`]). An empty query lists
    /// by filters + sort alone; `next_cursor` comes from the `Link: <...>; rel="next"` header.
    ///
    /// HF's `pipeline_tag=` filter drops repos that have no tag (many re-quantizations). For the
    /// tasks [`crate::hub_tasks::INFERABLE`] covers, a second listing without the task runs in
    /// parallel and keeps the untagged repos whose task is inferred from their tags / files
    /// (`inferred_task`); both lists are merged by the sort key. The merged page's cursor holds
    /// both listings' cursors, buffered candidates and emitted IDs (opaque versioned state).
    pub async fn search_models_page(
        options: &HfSearchOptions,
        token: Option<&str>,
    ) -> Result<HfSearchPage, String> {
        // Paginated Hub listings must retain HF's order and cursor. First-page extras
        // would select a different subset for each sort and break the next page boundary.
        Self::search_gguf_page(options, token).await
    }

    /// The `ggerganov/whisper.cpp` repo as a listing item; `None` when it cannot be read (the
    /// rest of the listing is still correct).
    async fn fetch_whisper_repo(token: Option<&str>) -> Option<HfModelSearchResult> {
        Self::fetch_exact_repo(WHISPER_CPP_REPO, token).await
    }

    async fn fetch_exact_repo(repo: &str, token: Option<&str>) -> Option<HfModelSearchResult> {
        let client = Self::client().ok()?;
        let url = format!("{HF_API}/models/{repo}");
        let fields: Vec<(&str, &str)> = LISTING_FIELDS.iter().map(|f| ("expand[]", *f)).collect();
        let res = Self::apply_hf_auth(client.get(&url).query(&fields), &url, token)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let item: serde_json::Value = res.json().await.ok()?;
        parse_search_item(&item)
    }

    async fn search_gguf_page(
        options: &HfSearchOptions,
        token: Option<&str>,
    ) -> Result<HfSearchPage, String> {
        let task = options
            .pipeline_tag
            .as_deref()
            .filter(|t| simple_token(t) && crate::hub_tasks::INFERABLE.contains(t))
            .map(str::to_string);
        match task {
            Some(task) if options.include_untagged.unwrap_or(true) => {
                Self::search_with_untagged(options, &task, token).await
            }
            _ => {
                let (items, next_cursor) = Self::fetch_listing(options, token).await?;
                Ok(HfSearchPage {
                    items: items.iter().filter_map(parse_search_item).collect(),
                    next_cursor,
                })
            }
        }
    }

    /// Non-GGUF repos Harness runs (or would run once its runtime family is installed), for a
    /// listing whose "also show non-GGUF models" switch is off: ONNX encoders and image models
    /// (single-file safetensors / `.ckpt`). Only the first page of a search that has a query or a
    /// task, since without either the extra listing would be unrelated noise; a failed request
    /// just adds nothing.
    async fn runnable_non_gguf(
        options: &HfSearchOptions,
        token: Option<&str>,
    ) -> Vec<HfModelSearchResult> {
        use crate::hub_compat::{is_onnx_task, Compatibility, ModelFormat};
        let task = options.pipeline_tag.as_deref().filter(|t| simple_token(t));
        if options.cursor.is_some() || (options.query.trim().is_empty() && task.is_none()) {
            return Vec::new();
        }
        let wide = HfSearchOptions {
            include_non_gguf: Some(true),
            include_untagged: Some(false),
            cursor: None,
            ..options.clone()
        };
        let mut jobs: Vec<Vec<(&'static str, String)>> = Vec::new();
        if task.is_none_or(|t| is_onnx_task(Some(t))) {
            let mut params = search_params(&wide);
            params.push(("filter", "onnx".to_string()));
            jobs.push(params);
        }
        if task.is_none_or(|t| matches!(t, "text-to-image" | "image-to-image")) {
            let image = HfSearchOptions {
                pipeline_tag: Some(task.unwrap_or("text-to-image").to_string()),
                ..wide.clone()
            };
            jobs.push(search_params(&image));
        }
        let lists = futures_util::future::join_all(
            jobs.into_iter()
                .map(|params| Self::fetch_params(params, token)),
        )
        .await;
        lists
            .into_iter()
            .filter_map(Result::ok)
            .flat_map(|(items, _)| items)
            .filter_map(|item| parse_search_item(&item))
            .filter(|m| {
                m.format != ModelFormat::Gguf
                    && !matches!(m.compatibility, Compatibility::Unsupported { .. })
            })
            .collect()
    }

    /// Raw listing items and the next cursor of one HF request.
    async fn fetch_listing(
        options: &HfSearchOptions,
        token: Option<&str>,
    ) -> Result<(Vec<serde_json::Value>, Option<String>), String> {
        Self::fetch_params(search_params(options), token).await
    }

    async fn fetch_params(
        params: Vec<(&'static str, String)>,
        token: Option<&str>,
    ) -> Result<(Vec<serde_json::Value>, Option<String>), String> {
        let client = Self::client()?;
        let url = format!("{}/models", HF_API);
        let req = client.get(&url).query(&params);
        let res = Self::apply_hf_auth(req, &url, token)
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;

        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(crate::hf_auth::rate_limited(res.headers()));
        }
        if !res.status().is_success() {
            return Err(err("http.status")
                .with("status", res.status())
                .with("url", &url)
                .into());
        }
        let next_cursor = res
            .headers()
            .get_all(reqwest::header::LINK)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(next_cursor_from_link);
        let items: Vec<serde_json::Value> = res
            .json()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        Ok((items, next_cursor))
    }

    async fn search_with_untagged(
        options: &HfSearchOptions,
        task: &str,
        token: Option<&str>,
    ) -> Result<HfSearchPage, String> {
        task_listing::search(options, task, |opts| async move {
            Self::fetch_listing(&opts, token).await
        })
        .await
    }

    pub async fn search_models(
        options: &HfSearchOptions,
        token: Option<&str>,
    ) -> Result<Vec<HfModelSearchResult>, String> {
        // Legacy unpaginated search keeps its curated discovery suggestions. The Model Hub
        // uses search_models_page directly, without these first-page additions/promotions.
        let include_extras = !options.include_non_gguf.unwrap_or(false);
        let (page, extras, whisper) = tokio::join!(
            Self::search_models_page(options, token),
            async {
                if include_extras {
                    Self::runnable_non_gguf(options, token).await
                } else {
                    Vec::new()
                }
            },
            async {
                if include_extras && wants_whisper_repo(options) {
                    Self::fetch_whisper_repo(token).await
                } else {
                    None
                }
            }
        );
        let mut page = page?;
        merge_search_extras(&mut page, extras, whisper, options.sort.as_deref());
        if exact_repo_missing(options, &page.items) {
            if let Some(repo) = Self::fetch_exact_repo(options.query.trim(), token).await {
                page.items.push(repo);
            }
        }
        prioritize_exact_repo(&mut page.items, &options.query);
        let results = page.items;
        // If exact user query returned no results, try fallback with sanitized query
        // (strips quant names like Q4_K_M, parentheses, and redundant .gguf)
        if results.is_empty() && options.cursor.is_none() {
            let clean_query = clean_hf_search_query(&options.query);
            if !clean_query.is_empty() && clean_query != options.query.trim() {
                let fallback = HfSearchOptions {
                    query: clean_query,
                    ..options.clone()
                };
                if let Ok(page) = Self::search_models_page(&fallback, token).await {
                    return Ok(page.items);
                }
            }
        }
        Ok(results)
    }

    /// Model card (README) and repo metadata.
    pub async fn get_model_card(repo_id: &str, token: Option<&str>) -> Result<HfModelCard, String> {
        Self::validate_repo_id(repo_id)?;
        let client = Self::client()?;
        let info_url = format!("{}/models/{}", HF_API, repo_id);
        let readme_url = Self::hf_file_url(repo_id, "raw", "README.md")?.to_string();
        let (info_res, readme_res) = tokio::join!(
            Self::apply_hf_auth(client.get(&info_url), &info_url, token).send(),
            Self::apply_hf_auth(client.get(&readme_url), &readme_url, token).send()
        );

        let info_res = info_res.map_err(|e| crate::http_error::transport(&e, &info_url))?;
        if let Some(e) = hf_refusal(
            info_res.status(),
            info_res.headers(),
            repo_id,
            token.is_some_and(|t| !t.trim().is_empty()),
        ) {
            return Err(e);
        }
        if !info_res.status().is_success() {
            return Err(err("http.status")
                .with("status", info_res.status())
                .with("url", &info_url)
                .into());
        }
        let info: serde_json::Value = info_res
            .json()
            .await
            .map_err(|e| crate::http_error::transport(&e, &info_url))?;
        let card = info.get("cardData");
        let tags = string_list(info.get("tags"));
        let mut analysis = crate::hub_tasks::analyze_repo(&info);
        image_route(&info, &mut analysis);

        let readme = match readme_res {
            Ok(r) if r.status().is_success() => r.text().await.ok().map(|text| {
                let body = strip_front_matter(&text);
                if body.len() > README_MAX_BYTES {
                    let mut end = README_MAX_BYTES;
                    while !body.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}\n\n…", &body[..end])
                } else {
                    body.to_string()
                }
            }),
            _ => None,
        };

        Ok(HfModelCard {
            repo_id: repo_id.to_string(),
            readme,
            license: card
                .and_then(|c| c.get("license"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| license_from_tags(&tags)),
            gated: gated_value(info.get("gated")),
            base_model: string_list(card.and_then(|c| c.get("base_model"))),
            pipeline_tag: analysis.pipeline_tag,
            inferred_task: analysis.inferred_task.map(String::from),
            inferred_source: analysis.source,
            not_runnable: card_not_runnable(&info, analysis.not_runnable),
            adapter_base: analysis.adapter_base,
            downloads: info.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
            likes: info.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
            last_modified: info
                .get("lastModified")
                .and_then(|v| v.as_str())
                .map(String::from),
            tags,
        })
    }

    /// For gated repos: probes a GGUF file with the configured token (HEAD request). One
    /// metadata call plus at most one HEAD, so "Retry" after accepting a license stays cheap.
    pub async fn check_access(repo_id: &str, token: Option<&str>) -> Result<HfAccess, String> {
        Self::validate_repo_id(repo_id)?;
        let token_present = token.map(|t| !t.trim().is_empty()).unwrap_or(false);
        let client = Self::client()?;
        let info_url = format!("{}/models/{}", HF_API, repo_id);
        let res = Self::apply_hf_auth(client.get(&info_url), &info_url, token)
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &info_url))?;
        if let Some(reason) = hf_refusal(res.status(), res.headers(), repo_id, token_present) {
            // Private / missing repo, or rate limited: the metadata itself is refused.
            let limited = res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS;
            return Ok(HfAccess {
                gated: None,
                token_present,
                has_access: (!limited).then_some(false),
                license: None,
                reason: Some(reason),
            });
        }
        if !res.status().is_success() {
            return Err(err("http.status")
                .with("status", res.status())
                .with("url", &info_url)
                .into());
        }
        let info: serde_json::Value = res
            .json()
            .await
            .map_err(|e| crate::http_error::transport(&e, &info_url))?;
        let license = info
            .get("cardData")
            .and_then(|c| c.get("license"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| license_from_tags(&string_list(info.get("tags"))));
        let gated = gated_value(info.get("gated"));
        if gated.is_none() {
            return Ok(HfAccess {
                gated,
                token_present,
                has_access: Some(true),
                license,
                reason: None,
            });
        }
        // Weights/config are gated; README/LICENSE files usually stay public.
        let files: Vec<String> = info
            .get("siblings")
            .and_then(|s| s.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|f| f.get("rfilename").and_then(|v| v.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let file = [".gguf", ".safetensors", "config.json"]
            .iter()
            .find_map(|suffix| {
                files
                    .iter()
                    .find(|n| n.to_lowercase().ends_with(suffix))
                    .cloned()
            });
        let (has_access, reason) = match file {
            Some(f) => {
                let url = Self::hf_file_url(repo_id, "resolve", &f)?.to_string();
                match Self::apply_hf_auth(client.head(&url), &url, token)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() || r.status().is_redirection() => {
                        (Some(true), None)
                    }
                    Ok(r) => {
                        let limited = r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS;
                        match hf_refusal(r.status(), r.headers(), repo_id, token_present) {
                            Some(reason) => ((!limited).then_some(false), Some(reason)),
                            None => (None, None),
                        }
                    }
                    Err(_) => (None, None),
                }
            }
            None => (None, None),
        };
        Ok(HfAccess {
            gated,
            token_present,
            has_access,
            license,
            reason,
        })
    }

    /// Lists every file in a repo (recursively, following pagination).
    async fn list_repo_files(
        repo_id: &str,
        token: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, String> {
        Self::list_repo_files_from(repo_id, token, HF_API).await
    }

    async fn list_repo_files_from(
        repo_id: &str,
        token: Option<&str>,
        api: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        Self::validate_repo_id(repo_id)?;
        let client = Self::client()?;
        let branches = ["main", "master"];
        let mut all = Vec::new();
        let mut last_error = String::new();
        let mut succeeded = false;

        for branch in branches {
            let mut url = format!("{}/models/{}/tree/{}?recursive=true", api, repo_id, branch);
            let mut page_count = 0;
            let mut branch_items = Vec::new();
            let mut branch_ok = true;

            loop {
                page_count += 1;
                if page_count > 25 {
                    break;
                }

                let req = client.get(&url).timeout(std::time::Duration::from_secs(8));
                let res = match Self::apply_hf_auth(req, &url, token).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        last_error = crate::http_error::transport(&e, &url);
                        branch_ok = false;
                        break;
                    }
                };

                if res.status() == reqwest::StatusCode::NOT_FOUND && page_count == 1 {
                    // Branch not found, try next branch
                    branch_ok = false;
                    break;
                }

                if !res.status().is_success() {
                    last_error = err("http.status")
                        .with("status", res.status())
                        .with("url", &url)
                        .into();
                    branch_ok = false;
                    break;
                }

                let next = res
                    .headers()
                    .get(reqwest::header::LINK)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_next_link);

                let items: Vec<serde_json::Value> = match res.json().await {
                    Ok(i) => i,
                    Err(e) => {
                        last_error = crate::http_error::transport(&e, &url);
                        branch_ok = false;
                        break;
                    }
                };

                branch_items.extend(
                    items
                        .into_iter()
                        .filter(|i| i.get("type").and_then(|t| t.as_str()) != Some("directory")),
                );

                match next {
                    Some(n) if Self::is_hf_url(&n) && n != url => url = n,
                    _ => break,
                }
            }

            if branch_ok && !branch_items.is_empty() {
                all = branch_items;
                succeeded = true;
                break;
            }
        }

        // Fallback: if tree endpoint failed, attempt to read file list from model info siblings
        if !succeeded {
            let info_url = format!("{}/models/{}", api, repo_id);
            let req = client
                .get(&info_url)
                .timeout(std::time::Duration::from_secs(8));
            if let Ok(res) = Self::apply_hf_auth(req, &info_url, token).send().await {
                if res.status().is_success() {
                    if let Ok(info) = res.json::<serde_json::Value>().await {
                        if let Some(siblings) = info.get("siblings").and_then(|s| s.as_array()) {
                            // An ONNX encoder repo is taken with its tokenizer and configs.
                            let genai_repo = siblings.iter().any(|s| {
                                s.get("rfilename")
                                    .and_then(|v| v.as_str())
                                    .is_some_and(|n| n.ends_with("genai_config.json"))
                            });
                            let onnx_repo = siblings.iter().any(|s| {
                                s.get("rfilename")
                                    .and_then(|v| v.as_str())
                                    .is_some_and(|n| n.to_lowercase().ends_with(".onnx"))
                            });
                            for sib in siblings {
                                if let Some(rfilename) =
                                    sib.get("rfilename").and_then(|v| v.as_str())
                                {
                                    if is_model_file_name(rfilename)
                                        || crate::image_bundles::is_image_candidate_file(rfilename)
                                        || matches!(rfilename, "config.json" | "model_index.json")
                                        || (onnx_repo && is_onnx_repo_file(rfilename))
                                        || (genai_repo && is_genai_repo_file(rfilename))
                                        || is_safetensors_repo_file(rfilename)
                                        || is_diffusers_repo_file(rfilename)
                                        || is_diffusers_component_weight(rfilename)
                                    {
                                        all.push(serde_json::json!({
                                            "type": "file",
                                            "path": rfilename,
                                            "size": 0
                                        }));
                                    }
                                }
                            }
                            if !all.is_empty() {
                                succeeded = true;
                            }
                        }
                    }
                }
            }
        }

        if !succeeded && all.is_empty() {
            return Err(if !last_error.is_empty() {
                last_error
            } else {
                err("hf.list_files_failed").with("repo", repo_id).into()
            });
        }

        Ok(all)
    }

    async fn repo_config(repo_id: &str, token: Option<&str>) -> Result<serde_json::Value, String> {
        let url = Self::hf_file_url(repo_id, "resolve", "config.json")?.to_string();
        let res = Self::apply_hf_auth(Self::client()?.get(&url), &url, token)
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?
            .error_for_status()
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        res.json()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))
    }

    pub async fn get_model_variants(
        repo_id: &str,
        token: Option<&str>,
    ) -> Result<Vec<HfModelVariant>, String> {
        let mut variants = Self::repo_variants(repo_id, token).await?;
        for v in &mut variants {
            if !crate::engine::crispasr_tts::supports_variant(repo_id, &v.name) {
                v.not_runnable = Some(crate::hub_tasks::NotRunnable::CrispasrPocketEnglishOnly);
            }
        }
        Ok(variants)
    }

    async fn repo_variants(
        repo_id: &str,
        token: Option<&str>,
    ) -> Result<Vec<HfModelVariant>, String> {
        let items = Self::list_repo_files(repo_id, token).await?;
        // `.safetensors` / `.ckpt` files are model files only in an image repo (a language
        // model's shards are not variants of their own).
        let paths: Vec<String> = items
            .iter()
            .filter_map(|i| i.get("path").and_then(|v| v.as_str()).map(String::from))
            .collect();
        let info = if paths.iter().any(|p| p.ends_with(".gguf")) {
            let url = format!("{HF_API}/models/{repo_id}");
            Self::apply_hf_auth(Self::client()?.get(&url), &url, token)
                .send()
                .await
                .map_err(|e| crate::http_error::transport(&e, &url))?
                .error_for_status()
                .map_err(|e| crate::http_error::transport(&e, &url))?
                .json::<serde_json::Value>()
                .await
                .map_err(|e| crate::http_error::transport(&e, &url))?
        } else {
            serde_json::Value::Null
        };
        if crate::hub_compat::native_speech_format(&info, &paths).is_some() {
            return safetensors_variants(repo_id, &items);
        }
        if repo_id == "audio-cpp/audio.cpp-gguf" || crate::hub_compat::is_audio_cpp_repo(&info) {
            return audio_cpp_variants(
                repo_id,
                &items,
                crate::hub_compat::audio_cpp_not_runnable(&info),
            );
        }
        if crate::hub_compat::is_sdxl_refiner(repo_id, &paths) {
            return diffusers_variants(repo_id, &items);
        }
        // A FLUX.2 [klein] conversion in mflux's format: its folders, kept as they are.
        if crate::hub_compat::is_mflux_layout(&paths) {
            return mflux_variants(repo_id, &items);
        }
        let image_repo = crate::image_bundles::repo_has_image_files(&paths);
        // A repo without GGUF or ggml files but with an ONNX encoder is served by harness-onnx.
        let llama_or_whisper = image_repo
            || items.iter().any(|i| {
                i.get("path")
                    .and_then(|v| v.as_str())
                    .is_some_and(is_model_file_name)
            });
        if !llama_or_whisper {
            if crate::hub_compat::has_ctc_files(&paths) {
                let config = Self::repo_config(repo_id, token).await?;
                if crate::hub_compat::is_ctc_config(&config) {
                    return onnx_variants(repo_id, &items);
                }
            }
            let genai = genai_variants(repo_id, &items)?;
            if !genai.is_empty() {
                return Ok(genai);
            }
            let onnx = encoder_onnx_variants(repo_id, &items)?;
            if !onnx.is_empty() {
                return Ok(onnx);
            }
            // A diffusers pipeline (the Python engine): `model_index.json` and its component
            // folders, one weight variant.
            let pipeline = diffusers_variants(repo_id, &items)?;
            if !pipeline.is_empty() {
                return Ok(pipeline);
            }
            // A safetensors checkpoint (mistral.rs): the whole repo folder in one download.
            let checkpoint = safetensors_variants(repo_id, &items)?;
            if !checkpoint.is_empty() {
                return Ok(checkpoint);
            }
        }
        Self::file_variants(repo_id, &items, image_repo)
    }

    fn file_variants(
        repo_id: &str,
        items: &[serde_json::Value],
        image_repo: bool,
    ) -> Result<Vec<HfModelVariant>, String> {
        let split_regex = Regex::new(r"(?i)^(.*?)-(\d{5})-of-(\d{5})\.gguf$").unwrap();

        let mut single_files = Vec::new();
        let mut split_groups: HashMap<String, Vec<HfShardInfo>> = HashMap::new();

        for item in items {
            let path_str = match item.get("path").and_then(|v| v.as_str()) {
                Some(p)
                    if is_model_file_name(p)
                        || (image_repo && crate::image_bundles::is_image_candidate_file(p)) =>
                {
                    p.to_string()
                }
                _ => continue,
            };

            let size = item
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("size").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let sha256 = item
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|v| v.as_str())
                .and_then(valid_sha256);
            let download_url = Self::hf_file_url(repo_id, "resolve", &path_str)?.to_string();

            if let Some(caps) = split_regex.captures(&path_str) {
                let base_name = caps.get(1).unwrap().as_str().to_string();
                let shard_index: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(1);
                let total_shards: u32 = caps.get(3).unwrap().as_str().parse().unwrap_or(1);

                split_groups
                    .entry(base_name)
                    .or_default()
                    .push(HfShardInfo {
                        filename: path_str,
                        download_url,
                        size_bytes: size,
                        shard_index,
                        total_shards,
                        sha256,
                    });
            } else {
                single_files.push(HfShardInfo {
                    filename: path_str,
                    download_url,
                    size_bytes: size,
                    shard_index: 1,
                    total_shards: 1,
                    sha256,
                });
            }
        }

        let has_main_gguf = single_files
            .iter()
            .chain(split_groups.values().flatten())
            .any(|s| {
                let lower = s.filename.to_ascii_lowercase();
                lower.ends_with(".gguf")
                    && !lower.contains("mmproj")
                    && !in_component_folder(&lower)
            });
        let mut variants = Vec::new();

        for shard in single_files {
            let lower = shard.filename.to_lowercase();
            // Vision projectors are companions, not standalone models.
            let quantization = if lower.contains("mmproj") {
                "mmproj".to_string()
            } else if has_main_gguf && lower.ends_with(".gguf") && in_component_folder(&lower) {
                // A text encoder / VAE GGUF in its own folder (`split/text_encoders/`) is a
                // part of a model, not one.
                "component".to_string()
            } else if !lower.ends_with(".gguf") && !is_ggml_bin_name(&shard.filename) {
                // An image checkpoint or one of its companions (VAE, text encoder).
                crate::image_bundles::variant_quant(&shard.filename)
            } else if is_ggml_bin_name(&shard.filename) {
                // whisper.cpp models: `ggml-base.bin` is f16, `...-q5_0.bin` quantized.
                match crate::gguf::extract_quant_from_filename(&shard.filename).as_str() {
                    "Unknown" => "F16".to_string(),
                    q => q.to_string(),
                }
            } else {
                crate::gguf::extract_quant_from_filename(&shard.filename)
            };
            variants.push(HfModelVariant {
                not_runnable: if image_repo
                    && crate::image_bundles::is_adapter_file(&shard.filename)
                {
                    Some(crate::hub_tasks::NotRunnable::Adapter)
                } else if image_repo
                    && crate::image_bundles::is_unloadable_scaled_fp8(&shard.filename)
                {
                    Some(crate::hub_tasks::NotRunnable::UnsupportedWeightLayout)
                } else {
                    None
                },
                name: shard
                    .filename
                    .trim_end_matches(".gguf")
                    .trim_end_matches(".bin")
                    .trim_end_matches(".safetensors")
                    .trim_end_matches(".ckpt")
                    .to_string(),
                quantization,
                total_size_bytes: shard.size_bytes,
                is_split: false,
                shards: vec![shard],
            });
        }

        for (base_name, mut shards) in split_groups {
            shards.sort_by_key(|s| s.shard_index);
            variants.push(HfModelVariant {
                not_runnable: None,
                quantization: crate::gguf::extract_quant_from_filename(&base_name),
                total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
                name: base_name,
                is_split: true,
                shards,
            });
        }

        // Deduplicate variant names to prevent Svelte each-block duplicate key collisions
        // and disambiguate split vs single files of the same quantization!
        let mut seen_names = std::collections::HashSet::new();
        for v in &mut variants {
            if seen_names.contains(&v.name) {
                if v.is_split {
                    v.name = format!("{}-split", v.name);
                } else {
                    let mut counter = 2;
                    let base = v.name.clone();
                    while seen_names.contains(&format!("{}-{}", base, counter)) {
                        counter += 1;
                    }
                    v.name = format!("{}-{}", base, counter);
                }
            }
            seen_names.insert(v.name.clone());
        }

        // Real models first (largest first), mmproj companions last.
        let companion =
            |v: &HfModelVariant| matches!(v.quantization.as_str(), "mmproj" | "component");
        variants.sort_by(|a, b| {
            companion(a)
                .cmp(&companion(b))
                .then(b.total_size_bytes.cmp(&a.total_size_bytes))
        });
        Ok(variants)
    }

    /// Queues a variant download. Returns the job id; progress, completion and
    /// errors are reported through [`list`].
    pub fn start_variant_download(
        &self,
        repo_id: String,
        variant: HfModelVariant,
    ) -> Result<u64, String> {
        let target_dir = Self::models_dir().join(&repo_id);
        self.start_variant_download_to(repo_id, variant, target_dir)
    }

    /// Resolves a variant of `repo_id` (see [`pick_variant`]) and queues it: `pull` of the
    /// Control API and the CLI. Returns the job id and the chosen variant.
    pub async fn pull(
        &self,
        repo_id: &str,
        file: Option<&str>,
        quant: Option<&str>,
        token: Option<&str>,
    ) -> Result<(u64, HfModelVariant), String> {
        Self::validate_repo_id(repo_id)?;
        let variants = Self::get_model_variants(repo_id, token).await?;
        let variant = pick_variant(&variants, file, quant)?;
        let id = self.start_variant_download(repo_id.to_string(), variant.clone())?;
        Ok((id, variant))
    }

    /// Queues the companions of a model that are not on disk yet: the VAE and text encoders of
    /// a known image model whose file is `main_file` ([`crate::image_bundles`]), or the codec of
    /// an mlx-audio text-to-speech repo `repo` ([`crate::engine::mlx::codec_plan_for_repo`]).
    /// One download job per file, from the repos the curated list names; a codec several models
    /// use is downloaded once. A model that is not in the lists, or has all its companions,
    /// queues nothing. Returns the job ids.
    pub async fn pull_companions(
        &self,
        main_file: &str,
        repo: Option<&str>,
        token: Option<&str>,
    ) -> Result<Vec<u64>, String> {
        let libraries = AppSettings::load().all_models_dirs();
        let Some(plan) = companion_plan_checked(main_file, repo, &libraries, token).await else {
            return Ok(Vec::new());
        };
        let active: Vec<(String, String)> = self
            .list()
            .into_iter()
            .filter(|j| {
                matches!(
                    j.status,
                    DownloadStatus::Queued
                        | DownloadStatus::Downloading
                        | DownloadStatus::Verifying
                )
            })
            .map(|j| (j.repo_id, j.variant_name))
            .collect();
        let mut ids = Vec::new();
        // A repo that ships its own vision projector (`split/text_encoders/*mmproj*.gguf`) is
        // run with it, not with the curated one (the model's definition looks for it first).
        let own_projector = match repo {
            Some(r) if plan.components.iter().any(|c| c.role == "llm_vision") => {
                match Self::list_repo_files(r, token).await {
                    Ok(items) => own_projector_from(main_file, &items).map(|file| (file, items)),
                    Err(_) => None,
                }
            }
            _ => None,
        };
        if let (Some((file, items)), Some(r)) = (&own_projector, repo) {
            let on_disk = libraries.iter().any(|l| l.join(r).join(file).is_file());
            if !on_disk {
                if let Some(variant) = companion_variant_from(r, file, items)? {
                    if !active.iter().any(|(ar, av)| ar == r && av == &variant.name) {
                        ids.push(self.start_variant_download(r.to_string(), variant)?);
                    }
                }
            }
        }
        for c in plan.missing() {
            if own_projector.is_some() && c.role == "llm_vision" {
                continue;
            }
            // An MLX codec is taken from its pinned commit and checked against its SHA-256.
            if let Some((file, folder)) = crate::engine::mlx::pinned_codec_file(&c.repo, &c.file) {
                if active.iter().any(|(r, v)| r == &c.repo && v == &c.file) {
                    continue;
                }
                let target = Self::models_dir().join(folder);
                ids.push(self.start_pinned_download(&file, c.file.clone(), target)?);
                continue;
            }
            let variant = Self::companion_variant(&c.repo, &c.file, token).await?;
            // Already on its way (the bundle was asked for twice).
            if active
                .iter()
                .any(|(r, v)| r == &c.repo && v == &variant.name)
            {
                continue;
            }
            ids.push(self.start_variant_download(c.repo.clone(), variant)?);
        }
        Ok(ids)
    }

    /// The download of one curated bundle companion: exactly `file` of `repo`. Companions often
    /// live in sub-folders (`split_files/vae/…`, `vae/…`) that the Hub variant list leaves out,
    /// so the file is looked up in the repo listing itself.
    pub async fn companion_variant(
        repo: &str,
        file: &str,
        token: Option<&str>,
    ) -> Result<HfModelVariant, String> {
        let items = Self::list_repo_files(repo, token).await?;
        companion_variant_from(repo, file, &items)?.ok_or_else(|| ERR_VARIANT_NOT_FOUND.to_string())
    }

    /// [`start_variant_download`] into `target_dir` (a repo folder `<library>/<owner>/<repo>`
    /// of any library directory; used to repair a split model where it lives).
    pub fn start_variant_download_to(
        &self,
        repo_id: String,
        variant: HfModelVariant,
        target_dir: PathBuf,
    ) -> Result<u64, String> {
        Self::validate_repo_id(&repo_id)?;
        if !AppSettings::load()
            .all_models_dirs()
            .iter()
            .any(|root| paths::is_within(&target_dir, root))
        {
            return Err(err("downloads.target_outside_library")
                .with("path", target_dir.display())
                .into());
        }
        let mut items = Vec::new();
        for shard in &variant.shards {
            let expected_url = Self::hf_file_url(&repo_id, "resolve", &shard.filename)?;
            let supplied_url = reqwest::Url::parse(&shard.download_url).map_err(|e| {
                String::from(
                    err("downloads.invalid_url")
                        .with("url", &shard.download_url)
                        .with("error", e),
                )
            })?;
            if supplied_url != expected_url {
                return Err(err("downloads.unexpected_url")
                    .with("url", &shard.download_url)
                    .into());
            }
            // Keep the repo's sub-folder structure but reject traversal.
            if shard
                .filename
                .split(['/', '\\'])
                .any(|c| c == ".." || c.is_empty())
            {
                return Err(err("downloads.invalid_file_name")
                    .with("name", &shard.filename)
                    .into());
            }
            items.push(DownloadItem {
                url: shard.download_url.clone(),
                filename: shard.filename.clone(),
                size: shard.size_bytes,
                shard_index: shard.shard_index,
                sha256: shard.sha256.as_deref().and_then(valid_sha256),
                custom_url: false,
            });
        }
        for helper in crate::engine::crispasr_tts::required_helpers(&repo_id, &variant.name) {
            if helper.verified().is_none() {
                crate::engine::crispasr_helpers::start_download(self, helper.id)?;
            }
        }
        self.enqueue(
            repo_id,
            variant.name,
            variant.total_size_bytes,
            items,
            target_dir,
        )
    }

    /// Queues one file of a Hugging Face repository at a pinned revision, checked against the
    /// SHA-256 and size given here (CrispASR's helper models). The file goes to `target_dir`,
    /// which the caller chose (not a library folder); it shows in the queue like any download.
    pub fn start_pinned_download(
        &self,
        file: &PinnedFile<'_>,
        variant_name: String,
        target_dir: PathBuf,
    ) -> Result<u64, String> {
        self.start_pinned_file(file, variant_name, target_dir, false)
    }

    /// Same verified queue for a pinned upstream dictionary on GitHub.
    pub fn start_pinned_github_download(
        &self,
        file: &PinnedFile<'_>,
        variant_name: String,
        target_dir: PathBuf,
    ) -> Result<u64, String> {
        self.start_pinned_file(file, variant_name, target_dir, true)
    }

    fn start_pinned_file(
        &self,
        file: &PinnedFile<'_>,
        variant_name: String,
        target_dir: PathBuf,
        github: bool,
    ) -> Result<u64, String> {
        let PinnedFile {
            repo: repo_id,
            rev,
            filename,
            size,
            sha256,
        } = *file;
        Self::validate_repo_id(repo_id)?;
        let valid_rev = rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit());
        let valid_name = !filename.contains(['\\', ':'])
            && filename
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        let sha256 = valid_sha256(sha256);
        if !valid_rev || !valid_name || sha256.is_none() {
            return Err(err("downloads.invalid_file_name")
                .with("name", filename)
                .into());
        }
        let mut url = Self::hf_file_url(repo_id, "resolve", filename)?;
        url.set_path(&format!("{repo_id}/resolve/{rev}/{filename}"));
        if github {
            url = reqwest::Url::parse(&format!(
                "https://raw.githubusercontent.com/{repo_id}/{rev}/{filename}"
            ))
            .map_err(|e| e.to_string())?;
        }
        let items = vec![DownloadItem {
            url: url.to_string(),
            filename: filename.to_string(),
            size,
            shard_index: 1,
            sha256,
            custom_url: github,
        }];
        self.enqueue(repo_id.to_string(), variant_name, size, items, target_dir)
    }

    /// Queues a GGUF download from an arbitrary HTTP(S) URL.
    pub async fn start_custom_url_download(
        &self,
        url: String,
        custom_filename: Option<String>,
        token: Option<String>,
    ) -> Result<u64, String> {
        let parsed = reqwest::Url::parse(&url).map_err(|e| {
            String::from(
                err("downloads.invalid_url")
                    .with("url", &url)
                    .with("error", e),
            )
        })?;
        validate_custom_url_structure(&parsed)?;
        validate_public_url(&parsed).await?;

        let raw_name = custom_filename
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                parsed
                    .path_segments()
                    .and_then(|mut s| s.next_back())
                    .map(String::from)
            })
            .unwrap_or_default();
        let sanitized = paths::sanitize_file_name(&raw_name);
        let client = Self::custom_client()?;
        // A name that does not say `.gguf` is decided by the file's first bytes: a whisper
        // ggml model keeps its `.bin` name, anything unrecognized is refused before it is
        // downloaded.
        let magic = if sanitized
            .as_deref()
            .is_some_and(|n| n.to_lowercase().ends_with(".gguf"))
        {
            None
        } else {
            sniff_custom_magic(&client, &parsed, token.as_deref()).await
        };
        let filename = custom_download_name(sanitized.as_deref(), magic)?;
        let size = send_custom_request(
            &client,
            reqwest::Method::HEAD,
            parsed.clone(),
            token.as_deref(),
            None,
            None,
        )
        .await
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.content_length())
        .unwrap_or(0);

        let items = vec![DownloadItem {
            url,
            filename: filename.clone(),
            size,
            shard_index: 1,
            sha256: None,
            custom_url: true,
        }];
        self.enqueue(
            "custom_url".to_string(),
            filename,
            size,
            items,
            Self::models_dir(),
        )
    }

    /// Queues the missing parts of a split model from custom URLs (`(url, file name)` pairs):
    /// the repair of a model that did not come from a Hugging Face repo folder. The files go
    /// to `target_dir`, which must be inside a library directory; every URL passes the same
    /// checks as a custom-URL download.
    pub async fn start_custom_url_parts(
        &self,
        parts: Vec<(String, String)>,
        variant_name: String,
        target_dir: PathBuf,
    ) -> Result<u64, String> {
        if parts.is_empty() {
            return Err(err("library.repair_nothing_missing").into());
        }
        if !AppSettings::load()
            .all_models_dirs()
            .iter()
            .any(|root| paths::is_within(&target_dir, root))
        {
            return Err(err("downloads.target_outside_library")
                .with("path", target_dir.display())
                .into());
        }
        let client = Self::custom_client()?;
        let mut items = Vec::new();
        let mut total = 0u64;
        for (url, name) in parts {
            let parsed = reqwest::Url::parse(&url).map_err(|e| {
                String::from(
                    err("downloads.invalid_url")
                        .with("url", &url)
                        .with("error", e),
                )
            })?;
            validate_custom_url_structure(&parsed)?;
            validate_public_url(&parsed).await?;
            let filename = paths::sanitize_file_name(&name)
                .filter(|f| f.to_lowercase().ends_with(".gguf"))
                .ok_or_else(|| {
                    String::from(err("downloads.invalid_file_name").with("name", &name))
                })?;
            let size = send_custom_request(
                &client,
                reqwest::Method::HEAD,
                parsed.clone(),
                None,
                None,
                None,
            )
            .await
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.content_length())
            .unwrap_or(0);
            total += size;
            items.push(DownloadItem {
                url,
                shard_index: crate::library::sources::shard_parts(&filename)
                    .map(|p| p.1)
                    .unwrap_or(1),
                filename,
                size,
                sha256: None,
                custom_url: true,
            });
        }
        self.enqueue(
            "custom_url".to_string(),
            variant_name,
            total,
            items,
            target_dir,
        )
    }

    fn enqueue(
        &self,
        repo_id: String,
        variant_name: String,
        total_bytes: u64,
        items: Vec<DownloadItem>,
        target_dir: PathBuf,
    ) -> Result<u64, String> {
        let id = {
            let mut jobs = self.jobs.lock().unwrap();
            // Same variant already known: re-queue it instead of duplicating.
            if let Some(job) = jobs.iter_mut().find(|j| {
                j.progress.repo_id == repo_id
                    && j.progress.variant_name == variant_name
                    && j.target_dir == target_dir
            }) {
                if job.progress.status.is_pending() {
                    return Err(err("downloads.already_queued").into());
                }
                job.items = items;
                job.progress.total_bytes = total_bytes;
                job.progress.total_shards = job.items.len() as u32;
                job.progress.status = DownloadStatus::Queued;
                job.cancel.store(false, Ordering::SeqCst);
                job.progress.id
            } else {
                let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
                jobs.push(Job {
                    progress: DownloadProgress {
                        id,
                        repo_id,
                        variant_name,
                        total_bytes,
                        downloaded_bytes: 0,
                        current_shard_name: items
                            .first()
                            .map(|i| i.filename.clone())
                            .unwrap_or_default(),
                        current_shard_index: 0,
                        total_shards: items.len() as u32,
                        speed_bytes_per_sec: 0,
                        status: DownloadStatus::Queued,
                        created_at: now_secs(),
                    },
                    items,
                    target_dir,
                    cancel: Arc::new(AtomicBool::new(false)),
                });
                // Keep the list bounded: drop the oldest completed entries.
                while jobs
                    .iter()
                    .filter(|j| j.progress.status == DownloadStatus::Completed)
                    .count()
                    > MAX_FINISHED_KEPT
                {
                    if let Some(i) = jobs
                        .iter()
                        .position(|j| j.progress.status == DownloadStatus::Completed)
                    {
                        jobs.remove(i);
                    }
                }
                id
            }
        };
        self.emit_job(id);
        self.persist();
        self.kick();
        Ok(id)
    }

    /// Starts the queue worker unless it is already running.
    fn kick(&self) {
        if self
            .worker_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let this = self.clone();
        let worker = async move {
            loop {
                let next = {
                    let mut jobs = this.jobs.lock().unwrap();
                    jobs.iter_mut()
                        .find(|j| j.progress.status == DownloadStatus::Queued)
                        .map(|j| {
                            j.progress.status = DownloadStatus::Downloading;
                            j.cancel.store(false, Ordering::SeqCst);
                            (
                                j.progress.id,
                                j.items.clone(),
                                j.target_dir.clone(),
                                j.cancel.clone(),
                            )
                        })
                };
                let Some((id, items, target_dir, cancel)) = next else {
                    this.worker_active.store(false, Ordering::SeqCst);
                    // A job may have been queued between the check and the store.
                    let more = this
                        .jobs
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|j| j.progress.status == DownloadStatus::Queued);
                    if more
                        && this
                            .worker_active
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                    {
                        continue;
                    }
                    return;
                };
                this.persist_async().await;

                // The OAuth access token is refreshed here when it is about to expire.
                let token = crate::hf_auth::current_token().await;
                let job = JobCtx { id, cancel };
                let result = this
                    .run_job(&job, &items, &target_dir, token.as_deref())
                    .await;
                let status = match result {
                    Ok(()) => DownloadStatus::Completed,
                    Err(e) if e == CANCELLED => DownloadStatus::Paused,
                    Err(e) => DownloadStatus::Error(e),
                };
                // A finished image model (or the last companion it waited for) gets its model
                // definition; the library has no other way to make one for it.
                if status == DownloadStatus::Completed
                    && items.iter().any(|i| {
                        crate::image_bundles::concerns_image_models(&i.filename)
                            || crate::image_bundles::is_bundle_companion(
                                &target_dir.join(&i.filename).to_string_lossy(),
                            )
                    })
                {
                    let synced =
                        tokio::task::spawn_blocking(crate::image_bundles::sync_library_definitions)
                            .await;
                    if let Ok(Err(e)) = synced {
                        eprintln!("[downloads] image model definitions: {e}");
                    }
                }
                if status == DownloadStatus::Completed
                    && items.iter().any(|i| i.filename.ends_with(".gguf"))
                {
                    let _ = tokio::task::spawn_blocking(|| {
                        let models = crate::models::ModelManager.scan_models();
                        crate::library_definitions::ensure(
                            &models,
                            &[crate::domain::ModelKind::Tts],
                        )
                    })
                    .await;
                }
                // The recommended embedding model of the knowledge bases gets its definition
                // (pinned to the CPU) as soon as it is complete.
                if status == DownloadStatus::Completed
                    && items
                        .iter()
                        .any(|i| i.filename == crate::rag::embedder::FILE)
                {
                    let _ =
                        tokio::task::spawn_blocking(crate::rag::embedder::sync_definition).await;
                }
                this.update(id, |p| {
                    if status == DownloadStatus::Completed {
                        p.downloaded_bytes = p.total_bytes.max(p.downloaded_bytes);
                    }
                    p.status = status;
                    p.speed_bytes_per_sec = 0;
                });
                this.persist_async().await;
            }
        };
        match &self.runtime {
            Some(runtime) => {
                runtime.spawn(worker);
            }
            None => {
                tokio::spawn(worker);
            }
        }
    }

    async fn run_job(
        &self,
        job: &JobCtx,
        items: &[DownloadItem],
        target_dir: &Path,
        token: Option<&str>,
    ) -> Result<(), String> {
        let cache = tokio::task::spawn_blocking(Self::cache_dir)
            .await
            .map_err(join_error)?;
        tokio::fs::create_dir_all(&cache).await.map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", cache.display())
                    .with("error", e),
            )
        })?;
        tokio::fs::create_dir_all(target_dir).await.map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", target_dir.display())
                    .with("error", e),
            )
        })?;

        let client = if items.iter().any(|item| item.custom_url) {
            Self::custom_client()?
        } else {
            Self::client()?
        };
        let mut done_before = 0u64;

        for (index, item) in items.iter().enumerate() {
            if job.cancelled() {
                return Err(CANCELLED.to_string());
            }

            let target_path = target_dir.join(&item.filename);
            let checked_target = target_path.clone();
            let checked_dir = target_dir.to_path_buf();
            if !tokio::task::spawn_blocking(move || paths::is_within(&checked_target, &checked_dir))
                .await
                .map_err(join_error)?
            {
                return Err(err("downloads.invalid_target_path")
                    .with("name", &item.filename)
                    .into());
            }
            let part_path = cache.join(part_name(&item.url, &item.filename));

            self.update(job.id, |p| {
                p.current_shard_name = item.filename.clone();
                p.current_shard_index = index as u32 + 1;
            });

            // Already present and complete?
            if let Some(existing_size) = existing_target_is_complete(&target_path, item).await? {
                done_before += existing_size;
                self.update(job.id, |p| p.downloaded_bytes = done_before);
                continue;
            }
            if tokio::fs::try_exists(&target_path).await.map_err(|e| {
                String::from(
                    err("fs.read_failed")
                        .with("path", target_path.display())
                        .with("error", e),
                )
            })? {
                let corrupt_path = preserve_corrupt_file(&target_path).await?;
                eprintln!(
                    "Doğrulanamayan mevcut model korundu: {}",
                    corrupt_path.display()
                );
            }

            let written = self
                .fetch_to_part(job, &client, item, &part_path, done_before, token)
                .await?;

            self.set_status(job.id, DownloadStatus::Verifying);
            let verify_path = part_path.clone();
            let verify_name = item.filename.clone();
            tokio::task::spawn_blocking(move || verify_model_magic(&verify_path, &verify_name))
                .await
                .map_err(join_error)??;
            let expected = match item.sha256.as_deref().and_then(valid_sha256) {
                Some(hash) => Some(hash),
                None => match repo_from_hf_url(&item.url) {
                    Some(repo) => {
                        let current_token = crate::hf_auth::current_token().await;
                        Self::refresh_file_sha256(&repo, item, current_token.as_deref(), HF_API)
                            .await
                    }
                    None => None,
                },
            };
            if let Some(actual) =
                verify_download_sha256(&part_path, item, expected.as_deref()).await?
            {
                let hash_path = target_path.clone();
                let hash = actual.clone();
                if let Err(error) =
                    tokio::task::spawn_blocking(move || record_hash(&hash_path, &hash))
                        .await
                        .map_err(join_error)?
                {
                    eprintln!("Model özeti kaydedilemedi: {}", error);
                }
            }
            self.set_status(job.id, DownloadStatus::Downloading);

            if let Some(parent) = target_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    String::from(
                        err("fs.create_dir_failed")
                            .with("path", parent.display())
                            .with("error", e),
                    )
                })?;
            }
            let _mutation = crate::models::LibraryMutation::begin();
            tokio::fs::rename(&part_path, &target_path)
                .await
                .map_err(|e| {
                    String::from(
                        err("fs.rename_failed")
                            .with("from", part_path.display())
                            .with("to", target_path.display())
                            .with("error", e),
                    )
                })?;
            // Remember the source so a missing part of this set can be fetched again later.
            if let Err(error) =
                crate::library::sources::record(&target_path, &item.url, item.custom_url)
            {
                eprintln!("Model kaynağı kaydedilemedi: {}", error);
            }
            let metadata_path = part_metadata_path(&part_path);
            if let Err(error) = tokio::fs::remove_file(&metadata_path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("Kısmi indirme bilgisi silinemedi: {}", error);
                }
            }

            done_before += written;
            self.update(job.id, |p| p.downloaded_bytes = done_before);
        }
        Ok(())
    }

    /// Downloads (or resumes) one file into its `.part` file. Returns the final part size.
    async fn fetch_to_part(
        &self,
        job: &JobCtx,
        client: &reqwest::Client,
        item: &DownloadItem,
        part_path: &Path,
        done_before: u64,
        token: Option<&str>,
    ) -> Result<u64, String> {
        let mut resume_offset = tokio::fs::metadata(part_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if item.size > 0 && resume_offset >= item.size {
            if resume_offset > item.size {
                reset_part(part_path).await?;
                resume_offset = 0;
            } else {
                return Ok(resume_offset);
            }
        }

        let metadata_path = part_metadata_path(part_path);
        let metadata = load_part_metadata(&metadata_path).await;
        let if_range = metadata
            .as_ref()
            .filter(|metadata| metadata.source_url == item.url)
            .and_then(PartMetadata::validator)
            .map(str::to_string);
        let mut restarted = false;
        let (res, append, total_for_item) = loop {
            let response = if item.custom_url {
                send_custom_request(
                    client,
                    reqwest::Method::GET,
                    reqwest::Url::parse(&item.url).map_err(|e| {
                        String::from(
                            err("downloads.invalid_url")
                                .with("url", &item.url)
                                .with("error", e),
                        )
                    })?,
                    token,
                    (resume_offset > 0).then_some(resume_offset),
                    if_range.as_deref(),
                )
                .await
            } else {
                let mut request = Self::apply_hf_auth(client.get(&item.url), &item.url, token);
                if resume_offset > 0 {
                    request =
                        request.header(reqwest::header::RANGE, format!("bytes={}-", resume_offset));
                    if let Some(validator) = if_range.as_deref() {
                        request = request.header(reqwest::header::IF_RANGE, validator);
                    }
                }
                request
                    .send()
                    .await
                    .map_err(|e| crate::http_error::transport(&e, &item.url))
            }
            .map_err(|e| {
                String::from(
                    err("downloads.connection_failed")
                        .with("file", &item.filename)
                        .with("error", e),
                )
            })?;

            if Self::is_hf_url(&item.url) {
                let repo = repo_from_hf_url(&item.url).unwrap_or_else(|| item.url.clone());
                let token_present = token.is_some_and(|t| !t.trim().is_empty());
                if let Some(e) =
                    hf_refusal(response.status(), response.headers(), &repo, token_present)
                {
                    return Err(e);
                }
            }
            let content_range = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok());
            let action = decide_resume_response(
                response.status(),
                content_range,
                resume_offset,
                item.size,
                response.content_length(),
            )?;
            match action {
                ResumeAction::Complete => return Ok(resume_offset),
                ResumeAction::Restart => {
                    if restarted {
                        return Err(err("downloads.bad_range_response")
                            .with("file", &item.filename)
                            .into());
                    }
                    reset_part(part_path).await?;
                    resume_offset = 0;
                    restarted = true;
                }
                ResumeAction::Download { append, total } => break (response, append, total),
            }
        };

        if item.size == 0 && total_for_item > 0 {
            self.update(job.id, |p| p.total_bytes = done_before + total_for_item);
        }

        let response_metadata = PartMetadata {
            source_url: item.url.clone(),
            etag: header_string(res.headers(), reqwest::header::ETAG),
            last_modified: header_string(res.headers(), reqwest::header::LAST_MODIFIED),
            total_size: total_for_item,
        };
        if let Ok(data) = serde_json::to_vec(&response_metadata) {
            if let Err(error) = tokio::fs::write(&metadata_path, data).await {
                eprintln!("Kısmi indirme bilgisi kaydedilemedi: {}", error);
            }
        }

        if !append {
            resume_offset = 0;
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(part_path)
            .await
            .map_err(|e| {
                String::from(
                    err("fs.write_failed")
                        .with("path", part_path.display())
                        .with("error", e),
                )
            })?;
        let mut file = BufWriter::new(file);

        let mut stream = res.bytes_stream();
        let mut downloaded = resume_offset;
        let mut last_instant = Instant::now();
        let mut bytes_since_last = 0u64;

        while let Some(chunk_res) = stream.next().await {
            if job.cancelled() {
                if let Err(error) = file.flush().await {
                    eprintln!("Kısmi dosya yazılamadı: {}", error);
                }
                return Err(CANCELLED.to_string());
            }
            let chunk = match chunk_res {
                Ok(chunk) => chunk,
                Err(error) => {
                    // BufWriter drops buffered bytes on error. Keep every received byte
                    // on disk so the next pull can resume with Range/If-Range.
                    file.flush().await.map_err(|e| {
                        String::from(
                            err("fs.write_failed")
                                .with("path", part_path.display())
                                .with("error", e),
                        )
                    })?;
                    return Err(err("downloads.interrupted")
                        .with("file", &item.filename)
                        .with("error", crate::http_error::transport(&error, &item.url))
                        .into());
                }
            };
            file.write_all(&chunk).await.map_err(|e| {
                String::from(
                    err("fs.write_failed")
                        .with("path", part_path.display())
                        .with("error", e),
                )
            })?;

            downloaded += chunk.len() as u64;
            bytes_since_last += chunk.len() as u64;

            let elapsed = last_instant.elapsed();
            if elapsed.as_millis() >= 500 {
                let speed = (bytes_since_last as f64 / elapsed.as_secs_f64()) as u64;
                self.update(job.id, |p| {
                    p.downloaded_bytes = done_before + downloaded;
                    p.speed_bytes_per_sec = speed;
                });
                last_instant = Instant::now();
                bytes_since_last = 0;
            }
        }
        file.flush().await.map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", part_path.display())
                    .with("error", e),
            )
        })?;

        if total_for_item > 0 && downloaded != total_for_item {
            return Err(err("downloads.incomplete")
                .with("file", &item.filename)
                .num("downloaded", downloaded)
                .num("total", total_for_item)
                .into());
        }
        Ok(downloaded)
    }

    async fn refresh_file_sha256(
        repo: &str,
        item: &DownloadItem,
        token: Option<&str>,
        api: &str,
    ) -> Option<String> {
        let result = Self::list_repo_files_from(repo, token, api).await;
        let hash = result.as_ref().ok().and_then(|items| {
            items.iter().find_map(|entry| {
                if entry.get("path").and_then(|v| v.as_str()) != Some(&item.filename) {
                    return None;
                }
                entry
                    .get("lfs")?
                    .get("oid")?
                    .as_str()
                    .and_then(valid_sha256)
            })
        });
        if hash.is_none() {
            eprintln!(
                "SHA-256 alınamadı, boyut kontrolüyle devam ediliyor: {}/{} ({})",
                repo,
                item.filename,
                result
                    .err()
                    .unwrap_or_else(|| "özet eksik veya maskeli".into())
            );
        }
        hash
    }

    /// Checks whether a locally downloaded file differs from the version on Hugging Face.
    /// The local file is resolved on the backend (never trusted from the frontend) and
    /// compared by SHA-256 when a recorded hash exists, otherwise by size.
    pub async fn check_model_artifact_update(
        repo_id: &str,
        filename: &str,
        token: Option<&str>,
    ) -> Result<ModelArtifactUpdate, String> {
        let items = Self::list_repo_files(repo_id, token).await?;
        let item = items
            .iter()
            .find(|i| i.get("path").and_then(|v| v.as_str()) == Some(filename))
            .ok_or_else(|| {
                String::from(
                    err("hf.file_not_in_repo")
                        .with("repo", repo_id)
                        .with("file", filename),
                )
            })?;

        let remote_size = item
            .get("lfs")
            .and_then(|l| l.get("size"))
            .and_then(|v| v.as_u64())
            .or_else(|| item.get("size").and_then(|v| v.as_u64()))
            .unwrap_or(0);
        let remote_oid = item
            .get("lfs")
            .and_then(|l| l.get("oid"))
            .and_then(|v| v.as_str())
            .and_then(valid_sha256);

        let settings = AppSettings::load();
        let local_path = settings
            .all_models_dirs()
            .into_iter()
            .map(|d| d.join(repo_id).join(filename))
            .find(|p| p.exists());

        let Some(local_path) = local_path else {
            return Ok(ModelArtifactUpdate {
                repo_id: repo_id.to_string(),
                filename: filename.to_string(),
                has_update: false,
                remote_size,
                local_size: 0,
                remote_oid,
                message: err("downloads.update_not_local").into(),
            });
        };

        let local_size = fs::metadata(&local_path).map(|m| m.len()).unwrap_or(0);
        let local_hash = recorded_hash(&local_path);
        let has_update = match (&remote_oid, &local_hash) {
            (Some(r), Some(l)) => !r.eq_ignore_ascii_case(l),
            _ => remote_size > 0 && remote_size != local_size,
        };
        let message = if has_update {
            err("downloads.update_available")
                .num("remote", remote_size)
                .num("local", local_size)
                .into()
        } else if local_hash.is_some() {
            err("downloads.up_to_date_hash").into()
        } else {
            err("downloads.up_to_date_size").into()
        };

        Ok(ModelArtifactUpdate {
            repo_id: repo_id.to_string(),
            filename: filename.to_string(),
            has_update,
            remote_size,
            local_size,
            remote_oid,
            message,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PartMetadata {
    source_url: String,
    etag: Option<String>,
    last_modified: Option<String>,
    total_size: u64,
}

impl PartMetadata {
    fn validator(&self) -> Option<&str> {
        self.etag.as_deref().or(self.last_modified.as_deref())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ResumeAction {
    Complete,
    Restart,
    Download { append: bool, total: u64 },
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedContentRange {
    start: Option<u64>,
    end: Option<u64>,
    total: Option<u64>,
}

fn parse_content_range(value: &str) -> Option<ParsedContentRange> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let total = match total {
        "*" => None,
        value => Some(value.parse().ok()?),
    };
    if range == "*" {
        return Some(ParsedContentRange {
            start: None,
            end: None,
            total,
        });
    }
    let (start, end) = range.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    (end >= start).then_some(ParsedContentRange {
        start: Some(start),
        end: Some(end),
        total,
    })
}

fn decide_resume_response(
    status: reqwest::StatusCode,
    content_range: Option<&str>,
    resume_offset: u64,
    expected_size: u64,
    content_length: Option<u64>,
) -> Result<ResumeAction, String> {
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && resume_offset > 0 {
        let advertised_total = content_range
            .and_then(parse_content_range)
            .and_then(|range| range.total);
        if expected_size > 0 && advertised_total.is_some_and(|total| total != expected_size) {
            return Ok(ResumeAction::Restart);
        }
        let expected_total = if expected_size > 0 {
            Some(expected_size)
        } else {
            advertised_total
        };
        return Ok(if expected_total == Some(resume_offset) {
            ResumeAction::Complete
        } else {
            ResumeAction::Restart
        });
    }

    if !status.is_success() {
        let key = if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
        {
            "downloads.http_gated"
        } else {
            "downloads.http_failed"
        };
        return Err(err(key).with("status", status).into());
    }

    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let Some(range) = content_range.and_then(parse_content_range) else {
            return Ok(ResumeAction::Restart);
        };
        let valid_length = match (range.start, range.end, content_length) {
            (Some(start), Some(end), Some(length)) => {
                end - start + 1 == length && range.total.is_none_or(|total| end < total)
            }
            (Some(_), Some(end), None) => range.total.is_none_or(|total| end < total),
            _ => false,
        };
        if range.start != Some(resume_offset)
            || !valid_length
            || (expected_size > 0 && range.total != Some(expected_size))
        {
            return Ok(ResumeAction::Restart);
        }
        return Ok(ResumeAction::Download {
            append: resume_offset > 0,
            total: if expected_size > 0 {
                expected_size
            } else {
                range.total.unwrap_or(0)
            },
        });
    }

    Ok(ResumeAction::Download {
        append: false,
        total: if expected_size > 0 {
            expected_size
        } else {
            content_length.unwrap_or(0)
        },
    })
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

async fn load_part_metadata(path: &Path) -> Option<PartMetadata> {
    tokio::fs::read(path)
        .await
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
}

async fn reset_part(path: &Path) -> Result<(), String> {
    tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .await
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    let metadata = part_metadata_path(path);
    if let Err(error) = tokio::fs::remove_file(metadata).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("Kısmi indirme bilgisi silinemedi: {}", error);
        }
    }
    Ok(())
}

async fn existing_target_is_complete(
    path: &Path,
    item: &DownloadItem,
) -> Result<Option<u64>, String> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(err("fs.read_failed")
                .with("path", path.display())
                .with("error", error)
                .into())
        }
    };
    let size = metadata.len();
    if item.size > 0 && size != item.size {
        return Ok(None);
    }
    let Some(expected) = item.sha256.as_deref().and_then(valid_sha256) else {
        return Ok((item.size > 0 && size == item.size).then_some(size));
    };
    let hash_path = path.to_path_buf();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
        .await
        .map_err(join_error)??;
    if actual.eq_ignore_ascii_case(&expected) {
        let record_path = path.to_path_buf();
        let record_hash_value = actual;
        if let Err(error) =
            tokio::task::spawn_blocking(move || record_hash(&record_path, &record_hash_value))
                .await
                .map_err(join_error)?
        {
            eprintln!("Model özeti kaydedilemedi: {}", error);
        }
        Ok(Some(size))
    } else {
        Ok(None)
    }
}

async fn preserve_corrupt_file(path: &Path) -> Result<PathBuf, String> {
    let _mutation = crate::models::LibraryMutation::begin();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model");
    for suffix in 0u32.. {
        let name = if suffix == 0 {
            format!("{}.corrupt", file_name)
        } else {
            format!("{}.corrupt.{}", file_name, suffix)
        };
        let candidate = path.with_file_name(name);
        if !tokio::fs::try_exists(&candidate).await.map_err(|e| {
            String::from(
                err("fs.read_failed")
                    .with("path", candidate.display())
                    .with("error", e),
            )
        })? {
            tokio::fs::rename(path, &candidate).await.map_err(|e| {
                String::from(
                    err("fs.rename_failed")
                        .with("from", path.display())
                        .with("to", candidate.display())
                        .with("error", e),
                )
            })?;
            return Ok(candidate);
        }
    }
    unreachable!()
}

fn validate_custom_url_structure(url: &reqwest::Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err(err("downloads.url_https_required").into());
    }
    if url.host_str().is_none() {
        return Err(err("downloads.url_no_host").into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(err("downloads.url_credentials").into());
    }
    Ok(())
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    crate::network::is_private_network_ip(ip)
}

async fn validate_public_url(url: &reqwest::Url) -> Result<(), String> {
    validate_custom_url_structure(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| String::from(err("downloads.url_no_host")))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| {
            String::from(
                err("downloads.url_dns_failed")
                    .with("host", host)
                    .with("error", e),
            )
        })?
        .collect();
    if addresses.is_empty() {
        return Err(err("downloads.url_unresolved").with("host", host).into());
    }
    if addresses.iter().any(|address| is_blocked_ip(address.ip())) {
        return Err(err("downloads.url_private_network")
            .with("host", host)
            .into());
    }
    Ok(())
}

fn redirect_target(current: &reqwest::Url, location: &str) -> Result<reqwest::Url, String> {
    let target = current.join(location).map_err(|e| {
        String::from(
            err("downloads.redirect_invalid")
                .with("url", location)
                .with("error", e),
        )
    })?;
    validate_custom_url_structure(&target)?;
    Ok(target)
}

async fn send_custom_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    mut url: reqwest::Url,
    token: Option<&str>,
    resume_offset: Option<u64>,
    if_range: Option<&str>,
) -> Result<reqwest::Response, String> {
    for _ in 0..=10 {
        validate_public_url(&url).await?;
        let mut request = DownloadManager::apply_hf_auth(
            client.request(method.clone(), url.clone()),
            url.as_str(),
            token,
        );
        if let Some(offset) = resume_offset {
            request = request.header(reqwest::header::RANGE, format!("bytes={}-", offset));
            if let Some(validator) = if_range {
                request = request.header(reqwest::header::IF_RANGE, validator);
            }
        }
        let response = request
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, url.as_str()))?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| String::from(err("downloads.redirect_no_location")))?;
        url = redirect_target(&url, location)?;
    }
    Err(err("downloads.too_many_redirects").into())
}

fn join_error(e: tokio::task::JoinError) -> String {
    err("task.join_failed").with("error", e).into()
}

/// Identity + cancel flag of the job being run.
struct JobCtx {
    id: u64,
    cancel: Arc<AtomicBool>,
}

impl JobCtx {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// URL-derived cache key. Legacy filename-only `.part` files are intentionally ignored:
/// their source cannot be identified safely, so migrating them could resume the wrong model.
fn part_name(source_url: &str, filename: &str) -> String {
    let readable = filename.replace(['/', '\\'], "__");
    let readable = paths::sanitize_file_name(&readable).unwrap_or_else(|| "model.gguf".to_string());
    let digest = Sha256::digest(source_url.as_bytes());
    let short_hash: String = digest[..8]
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect();
    format!("{}-{}.part", readable, short_hash)
}

fn part_metadata_path(part_path: &Path) -> PathBuf {
    let mut name = part_path.as_os_str().to_os_string();
    name.push(".json");
    PathBuf::from(name)
}

/// Extracts the `rel="next"` URL from an HTTP Link header.
fn parse_next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let part = part.trim();
        if part.contains("rel=\"next\"") {
            let start = part.find('<')? + 1;
            let end = part.find('>')?;
            Some(part[start..end].to_string())
        } else {
            None
        }
    })
}

/// A whisper.cpp model of a Hugging Face repo (`ggerganov/whisper.cpp`: `ggml-base.bin`,
/// `ggml-large-v3-turbo-q5_0.bin`): the file name is `ggml-<...>.bin`. Other `.bin` files of a
/// repo (PyTorch weights, `for-tests-ggml-*.bin` fixtures) are not models of ours.
fn is_ggml_bin_name(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    name.starts_with("ggml-") && name.ends_with(".bin")
}

/// The ggml container magic of whisper.cpp models (`ggml` as a little-endian u32).
const GGML_MAGIC: u32 = 0x6767_6d6c;

/// What the first four bytes of a custom-URL file say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SniffedKind {
    Gguf,
    WhisperGgml,
    Other,
}

fn sniff_kind(magic: [u8; 4]) -> SniffedKind {
    if &magic == b"GGUF" {
        SniffedKind::Gguf
    } else if u32::from_le_bytes(magic) == GGML_MAGIC {
        SniffedKind::WhisperGgml
    } else {
        SniffedKind::Other
    }
}

/// The file name a custom-URL download is saved under. `.gguf` names are kept. Otherwise the
/// first bytes (`magic`, `None` when they could not be read) decide: a whisper ggml model is
/// saved as `ggml-<name>.bin` (the name the library scan and the magic check expect, never
/// `.gguf`), a GGUF behind another name gets `.gguf`, and a file that is neither (an HTML page,
/// an archive, PyTorch weights) is refused. Without readable bytes the name decides: a
/// `ggml-*.bin` is kept (the magic is checked after the download), anything else is a GGUF.
fn custom_download_name(sanitized: Option<&str>, magic: Option<[u8; 4]>) -> Result<String, String> {
    let name = sanitized.unwrap_or("custom_model");
    if name.to_lowercase().ends_with(".gguf") {
        return Ok(name.to_string());
    }
    let stem = match name.len().checked_sub(4).and_then(|i| name.get(i..)) {
        Some(ext) if ext.eq_ignore_ascii_case(".bin") => &name[..name.len() - 4],
        _ => name,
    };
    match magic.map(sniff_kind) {
        Some(SniffedKind::WhisperGgml) => {
            let bin = format!("{stem}.bin");
            Ok(if is_ggml_bin_name(&bin) {
                bin
            } else {
                format!("ggml-{bin}")
            })
        }
        Some(SniffedKind::Gguf) => Ok(format!("{stem}.gguf")),
        Some(SniffedKind::Other) => {
            Err(err("downloads.custom_not_model").with("name", name).into())
        }
        None if is_ggml_bin_name(name) => Ok(name.to_string()),
        None => Ok(format!("{name}.gguf")),
    }
}

/// The first four bytes of a custom URL (its redirects pass the same public-host checks as the
/// download); `None` when the server does not answer or sends less.
async fn sniff_custom_magic(
    client: &reqwest::Client,
    url: &reqwest::Url,
    token: Option<&str>,
) -> Option<[u8; 4]> {
    let mut response = send_custom_request(
        client,
        reqwest::Method::GET,
        url.clone(),
        token,
        Some(0),
        None,
    )
    .await
    .ok()
    .filter(|r| r.status().is_success())?;
    let mut head = Vec::with_capacity(4);
    while head.len() < 4 {
        let chunk = response.chunk().await.ok()??;
        head.extend_from_slice(&chunk);
    }
    head[..4].try_into().ok()
}

/// A file a model variant can consist of: a GGUF or a whisper ggml `.bin`.
fn is_model_file_name(path: &str) -> bool {
    path.to_lowercase().ends_with(".gguf") || is_ggml_bin_name(path)
}

/// Small files of an ONNX encoder repo that the engine reads next to the graph: the tokenizer
/// and the Hugging Face / sentence-transformers configs (`1_Pooling/config.json`...). Weights
/// of modules the engine cannot run (`2_Dense`) are not among them.
const ONNX_COMPANIONS: [&str; 11] = [
    "tokenizer.json",
    "vocab.txt",
    "config.json",
    // Vision models: resize, crop and normalization of the image processor.
    "preprocessor_config.json",
    "tokenizer_config.json",
    "sentence_bert_config.json",
    "modules.json",
    "config_sentence_transformers.json",
    "vocab.json",
    "special_tokens_map.json",
    "added_tokens.json",
];

fn is_onnx_repo_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.ends_with(".onnx") || lower.ends_with(".onnx_data") || lower.ends_with(".onnx.data") {
        return true;
    }
    if ONNX_COMPANIONS.contains(&lower.as_str()) {
        return true;
    }
    // `1_Pooling/config.json`, `2_Normalize/config.json`.
    let mut parts = lower.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(dir), Some("config.json"), None)
            if dir.split_once('_').is_some_and(|(n, kind)| {
                n.chars().all(|c| c.is_ascii_digit()) && (kind == "pooling" || kind == "normalize")
            })
    )
}

/// The variants of an ONNX encoder repo: one per `.onnx` file (`onnx/model.onnx`,
/// `onnx/model_quantized.onnx`...), each with its external weights, the tokenizer and the
/// config files, keeping the repo's folder layout so the engine finds them. Empty when the repo
/// has no tokenizer (a vision or speech graph).
/// [`onnx_variants`] of an encoder repo only. The ONNX family serves encoders (embeddings,
/// rerankers); a generative model ships a `generation_config.json`, and its `onnx/` export
/// (transformers.js, optimum) is a decoder the encoder engine cannot run. Measured:
/// SmolLM2-135M-Instruct listed eight such files and hid its safetensors checkpoint.
fn encoder_onnx_variants(
    repo_id: &str,
    items: &[serde_json::Value],
) -> Result<Vec<HfModelVariant>, String> {
    let generative = items
        .iter()
        .any(|i| i.get("path").and_then(|v| v.as_str()) == Some("generation_config.json"));
    if generative {
        return Ok(Vec::new());
    }
    onnx_variants(repo_id, items)
}

fn onnx_variants(
    repo_id: &str,
    items: &[serde_json::Value],
) -> Result<Vec<HfModelVariant>, String> {
    struct F {
        path: String,
        size: u64,
        sha256: Option<String>,
    }
    let files: Vec<F> = items
        .iter()
        .filter_map(|item| {
            let path = item.get("path").and_then(|v| v.as_str())?.to_string();
            let size = item
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("size").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let sha256 = item
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|v| v.as_str())
                .and_then(valid_sha256);
            Some(F { path, size, sha256 })
        })
        .collect();
    let ctc =
        crate::hub_compat::has_ctc_files(&files.iter().map(|f| f.path.clone()).collect::<Vec<_>>());
    // An encoder reads its tokenizer, a vision model its image processor; a bare graph
    // (or one for another runtime) has neither.
    if !files.iter().any(|f| {
        matches!(
            f.path.as_str(),
            "tokenizer.json" | "vocab.txt" | "preprocessor_config.json"
        )
    }) {
        return Ok(Vec::new());
    }
    let shard = |f: &F, index: u32, total: u32| -> Result<HfShardInfo, String> {
        Ok(HfShardInfo {
            download_url: DownloadManager::hf_file_url(repo_id, "resolve", &f.path)?.to_string(),
            filename: f.path.clone(),
            size_bytes: f.size,
            shard_index: index,
            total_shards: total,
            sha256: f.sha256.clone(),
        })
    };
    let companions: Vec<&F> = files
        .iter()
        .filter(|f| {
            let lower = f.path.to_lowercase();
            is_onnx_repo_file(&f.path)
                && !lower.ends_with(".onnx")
                && !lower.ends_with(".onnx_data")
                && !lower.ends_with(".onnx.data")
        })
        .collect();
    // An encoder-decoder (an image captioner) is one model in two graphs: every encoder
    // variant brings its decoder, and the decoder graphs are no variants of their own.
    let graph_named = |prefix: &str| {
        files.iter().any(|f| {
            f.path.to_lowercase().ends_with(".onnx")
                && f.path
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.starts_with(prefix))
        })
    };
    let encoder_decoder = graph_named("encoder_model") && graph_named("decoder_model");
    let mut variants = Vec::new();
    for graph in files
        .iter()
        .filter(|f| f.path.to_lowercase().ends_with(".onnx"))
    {
        let (dir, file) = graph.path.rsplit_once('/').unwrap_or(("", &graph.path));
        // Only the repo root and one level (`onnx/`): deeper folders are other pipelines.
        if dir.contains('/') || (ctc && !dir.is_empty() && dir != "onnx") {
            continue;
        }
        let stem = file.trim_end_matches(".onnx").trim_end_matches(".ONNX");
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{}/", dir)
        };
        // A dual encoder (CLIP, SigLIP) ships its two towers beside the joint graph; a tower
        // alone cannot score an image against labels.
        let tower = stem.starts_with("text_model") || stem.starts_with("vision_model");
        if tower
            && files
                .iter()
                .any(|f| f.path.starts_with(&format!("{prefix}model")) && f.path.ends_with(".onnx"))
        {
            continue;
        }
        if encoder_decoder && stem.starts_with("decoder_") {
            continue;
        }
        let external = |stem: &str| -> Vec<&F> {
            files
                .iter()
                .filter(|f| {
                    f.path == format!("{prefix}{stem}.onnx_data")
                        || f.path == format!("{prefix}{stem}.onnx.data")
                })
                .collect()
        };
        let mut parts: Vec<&F> = vec![graph];
        parts.extend(external(stem));
        if let Some(suffix) = stem
            .strip_prefix("encoder_model")
            .filter(|_| encoder_decoder)
        {
            // The same precision's merged decoder first (as the sidecar picks it).
            let decoder = [
                format!("decoder_model_merged{suffix}"),
                format!("decoder_model{suffix}"),
                "decoder_model_merged".to_string(),
                "decoder_model".to_string(),
            ]
            .into_iter()
            .find_map(|d| {
                files
                    .iter()
                    .find(|f| f.path == format!("{prefix}{d}.onnx"))
                    .map(|f| (f, d))
            });
            match decoder {
                Some((f, d)) => {
                    parts.push(f);
                    parts.extend(external(&d));
                }
                None => continue,
            }
        }
        parts.extend(companions.iter().copied());
        let total = parts.len() as u32;
        let shards = parts
            .iter()
            .enumerate()
            .map(|(i, f)| shard(f, i as u32 + 1, total))
            .collect::<Result<Vec<_>, _>>()?;
        variants.push(HfModelVariant {
            not_runnable: None,
            name: stem.to_string(),
            quantization: harness_onnx::package::variant_quantization(stem).to_string(),
            total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
            is_split: false,
            shards,
        });
    }
    // The repo's own `model` first, then the others by size; same-named files of the root and
    // `onnx/` get a number.
    variants.sort_by(|a, b| {
        (a.name != "model")
            .cmp(&(b.name != "model"))
            .then(b.total_size_bytes.cmp(&a.total_size_bytes))
    });
    let mut seen = std::collections::HashSet::new();
    for v in &mut variants {
        let base = v.name.clone();
        let mut n = 2;
        while !seen.insert(v.name.clone()) {
            v.name = format!("{}-{}", base, n);
            n += 1;
        }
    }
    Ok(variants)
}

/// A file of an onnxruntime-genai model folder: the graph and its weights, `genai_config.json`,
/// the tokenizer and chat template files. Documentation, images and Python files are not.
fn is_genai_repo_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.ends_with(".onnx") || lower.ends_with(".onnx_data") || lower.ends_with(".onnx.data") {
        return true;
    }
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if name == "readme.md" || name.starts_with("license") || name.starts_with('.') {
        return false;
    }
    matches!(
        name.rsplit_once('.').map(|(_, ext)| ext),
        Some("json" | "jinja" | "model" | "txt" | "tiktoken")
    )
}

/// Where a genai variant folder runs, judged by its name (`directml-int4-...`,
/// `cpu-int4-...`, `cuda-fp16`): lower is better on this machine. `None` for targets the ONNX
/// Runtime builds Harness ships have no execution provider for (NPUs, WebGPU, OpenVINO).
fn genai_target_rank(folder_path: &str) -> Option<u8> {
    let lower = folder_path.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if has(&[
        "webgpu",
        "npu",
        "qnn",
        "ryzenai",
        "vitis",
        "openvino",
        "ov-",
        "nvtensorrt",
        "trtrtx",
        "coreml",
        "mobile-web",
        "onnx-web",
    ]) {
        return None;
    }
    let windows = cfg!(windows);
    Some(if has(&["directml", "dml"]) {
        if windows {
            0
        } else {
            3
        }
    } else if has(&["cpu"]) {
        1
    } else if has(&["cuda", "gpu"]) {
        2
    } else {
        1
    })
}

/// The variants of an onnxruntime-genai repo (`genai_config.json` in a folder): one per folder,
/// with every file of the folder (graph, external weights, tokenizer, chat template, configs)
/// at the repo's own paths. Microsoft's repos keep one folder per target
/// (`directml/directml-int4-awq-block-128`, `cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4`,
/// `cuda/cuda-fp16`), small community repos the model at the root. The folders that fit this
/// machine come first (DirectML on Windows, then the CPU, then CUDA); NPU and WebGPU folders are
/// left out. Empty when the repo has no `genai_config.json`.
fn genai_variants(
    repo_id: &str,
    items: &[serde_json::Value],
) -> Result<Vec<HfModelVariant>, String> {
    struct F {
        path: String,
        size: u64,
        sha256: Option<String>,
    }
    let files: Vec<F> = items
        .iter()
        .filter_map(|item| {
            let path = item.get("path").and_then(|v| v.as_str())?.to_string();
            let size = item
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("size").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let sha256 = item
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|v| v.as_str())
                .and_then(valid_sha256);
            Some(F { path, size, sha256 })
        })
        .collect();
    let dir_of = |p: &str| {
        p.rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default()
    };
    let mut dirs: Vec<String> = files
        .iter()
        .filter(|f| f.path.rsplit('/').next() == Some("genai_config.json"))
        .map(|f| dir_of(&f.path))
        .filter(|d| d.matches('/').count() <= 2)
        .collect();
    dirs.sort();
    dirs.dedup();
    let repo_name = repo_id.rsplit('/').next().unwrap_or(repo_id);
    let mut variants: Vec<(u8, HfModelVariant)> = Vec::new();
    for dir in dirs {
        let Some(rank) = genai_target_rank(&dir) else {
            continue;
        };
        let parts: Vec<&F> = files
            .iter()
            .filter(|f| dir_of(&f.path) == dir && is_genai_repo_file(&f.path))
            .collect();
        // A folder without a graph is not a model (a stray config).
        if !parts
            .iter()
            .any(|f| f.path.to_lowercase().ends_with(".onnx"))
        {
            continue;
        }
        let total = parts.len() as u32;
        let mut shards = Vec::new();
        for (i, f) in parts.iter().enumerate() {
            shards.push(HfShardInfo {
                download_url: DownloadManager::hf_file_url(repo_id, "resolve", &f.path)?
                    .to_string(),
                filename: f.path.clone(),
                size_bytes: f.size,
                shard_index: i as u32 + 1,
                total_shards: total,
                sha256: f.sha256.clone(),
            });
        }
        let name = if dir.is_empty() {
            repo_name.to_string()
        } else {
            dir.rsplit('/').next().unwrap_or(&dir).to_string()
        };
        variants.push((
            rank,
            HfModelVariant {
                not_runnable: None,
                quantization: harness_onnx::genai_pkg::precision_of(&name).to_string(),
                total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
                is_split: false,
                name,
                shards,
            },
        ));
    }
    variants.sort_by_key(|(rank, _)| *rank);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (_, mut v) in variants {
        let base = v.name.clone();
        let mut n = 2;
        while !seen.insert(v.name.clone()) {
            v.name = format!("{base}-{n}");
            n += 1;
        }
        out.push(v);
    }
    Ok(out)
}

/// Small root files of a transformers checkpoint that mistral.rs reads next to the weights:
/// the config, the tokenizer, the generation defaults, the chat template and a VLM's processor.
const SAFETENSORS_COMPANIONS: [&str; 16] = [
    "config.json",
    "generation_config.json",
    "model.safetensors.index.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "tokenizer.model",
    "special_tokens_map.json",
    "added_tokens.json",
    "vocab.json",
    "vocab.txt",
    "merges.txt",
    "chat_template.json",
    "chat_template.jinja",
    "preprocessor_config.json",
    "processor_config.json",
    "video_preprocessor_config.json",
];

/// More small root files the Python engine reads: sentence-transformers' module list and
/// settings, SentencePiece and Marian vocabularies, feature extractors, the PyTorch shard index
/// and the model card (its `pipeline_tag` names the task).
const PYTHON_COMPANIONS: [&str; 13] = [
    "modules.json",
    "sentence_bert_config.json",
    "config_sentence_transformers.json",
    "spiece.model",
    "sentencepiece.bpe.model",
    "source.spm",
    "target.spm",
    "feature_extractor_config.json",
    "image_processor_config.json",
    "pytorch_model.bin.index.json",
    "readme.md",
    "normalizer.json",
    "spm.model",
];

/// Root tensor files a family loads beside its weights: Chatterbox Turbo's default voice
/// (`conds.safetensors`). Other root tensor files are not downloaded.
const ROOT_TENSOR_COMPANIONS: [&str; 1] = ["conds.safetensors"];

/// Sub-model folders a checkpoint loads from beside its root weights: Qwen3-TTS's
/// `speech_tokenizer/` (its audio codec; mlx-audio refuses to speak without it).
const SUBMODEL_DIRS: [&str; 1] = ["speech_tokenizer"];

/// A companion file of a safetensors checkpoint at the repo root ([`SAFETENSORS_COMPANIONS`],
/// [`PYTHON_COMPANIONS`], a Jinja template or a tiktoken vocabulary), a sentence-transformers
/// module folder's file (`1_Pooling/config.json`, `2_Dense/model.safetensors`) or a
/// [`SUBMODEL_DIRS`] file (`speech_tokenizer/model.safetensors`).
fn is_safetensors_repo_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    if let Some((dir, rest)) = lower.split_once('/') {
        let module = dir
            .split_once('_')
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            || SUBMODEL_DIRS.contains(&dir);
        return module
            && !rest.contains('/')
            && (rest.ends_with(".json")
                || rest.ends_with(".safetensors")
                || rest == "pytorch_model.bin");
    }
    SAFETENSORS_COMPANIONS.contains(&lower.as_str())
        || PYTHON_COMPANIONS.contains(&lower.as_str())
        // A SentencePiece model under another name (SpeechT5's `spm_char.model`).
        || lower.ends_with(".model")
        || lower.ends_with(".jinja")
        || lower.ends_with(".tiktoken")
}

/// A PyTorch weight file at the repo root (`pytorch_model.bin`, `pytorch_model-00001-of-00002.bin`).
fn is_torch_weights(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    !lower.contains('/') && lower.starts_with("pytorch_model") && lower.ends_with(".bin")
}

/// Weight variant of a diffusers component file (`diffusion_pytorch_model.fp16.safetensors` ->
/// `fp16`, a shard `model.fp16-00001-of-00002.safetensors` too); `None` for the plain weights.
fn diffusers_weight_variant(name: &str) -> Option<String> {
    let stem = name
        .strip_suffix(".safetensors")
        .or_else(|| name.strip_suffix(".bin"))
        .unwrap_or(name);
    let stem = Regex::new(r"-\d{5}-of-\d{5}$")
        .map(|re| re.replace(stem, "").into_owned())
        .unwrap_or_else(|_| stem.to_string());
    stem.split_once('.').map(|(_, v)| v.to_string())
}

/// A non-weight file of a diffusers component folder (`unet/config.json`, `tokenizer/merges.txt`,
/// `scheduler/scheduler_config.json`): one folder down, a config or tokenizer file.
fn is_diffusers_repo_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    let Some((_, name)) = lower.split_once('/') else {
        return lower == "model_index.json";
    };
    !name.contains('/')
        && !name.contains(".index.")
        && (name.ends_with(".json")
            || name.ends_with(".txt")
            || name.ends_with(".model")
            || name == "tokenizer.json")
}

/// A weight file one folder down (`unet/diffusion_pytorch_model.safetensors`): a diffusers
/// component's, which [`diffusers_variants`] chooses among.
fn is_diffusers_component_weight(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    lower.split('/').count() == 2 && (lower.ends_with(".safetensors") || lower.ends_with(".bin"))
}

/// The variant of a diffusers pipeline repo (the Python engine loads it from its folders): one
/// download of `model_index.json` and every component folder (`unet/`, `vae/`, `text_encoder/`,
/// `tokenizer/`, `scheduler/`...) in the repo layout, with one set of weights: the fp16
/// safetensors when every component has them, else the plain safetensors, and PyTorch `.bin`
/// only for a component that has no safetensors (`segmind/tiny-sd`). Other variants
/// (`non_ema`...), Flax, ONNX and OpenVINO exports, nested folders and root checkpoints are
/// left out. Empty when the repo has no `model_index.json` or no component weights.
fn diffusers_variants(
    repo_id: &str,
    items: &[serde_json::Value],
) -> Result<Vec<HfModelVariant>, String> {
    struct F {
        path: String,
        dir: String,
        name: String,
        size: u64,
        sha256: Option<String>,
    }
    let files: Vec<F> = items
        .iter()
        .filter_map(|item| {
            let path = item.get("path").and_then(|v| v.as_str())?.to_string();
            let lower = path.to_ascii_lowercase();
            let (dir, name) = match lower.split_once('/') {
                Some((d, n)) if !n.contains('/') => (d.to_string(), n.to_string()),
                Some(_) => return None,
                None => (String::new(), lower.clone()),
            };
            let size = item
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("size").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let sha256 = item
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|v| v.as_str())
                .and_then(valid_sha256);
            Some(F {
                path,
                dir,
                name,
                size,
                sha256,
            })
        })
        .collect();
    if !files
        .iter()
        .any(|f| f.dir.is_empty() && f.name == "model_index.json")
    {
        return Ok(Vec::new());
    }
    let safetensors = |f: &F| !f.dir.is_empty() && f.name.ends_with(".safetensors");
    let torch_bin = |f: &F| {
        !f.dir.is_empty()
            && f.name.ends_with(".bin")
            && (f.name.starts_with("diffusion_pytorch_model")
                || f.name.starts_with("pytorch_model")
                || f.name.starts_with("model"))
    };
    let mut dirs: Vec<&str> = files
        .iter()
        .filter(|f| safetensors(f) || torch_bin(f))
        .map(|f| f.dir.as_str())
        // A component has its config next to its weights (`unet/config.json`).
        .filter(|d| files.iter().any(|f| f.dir == *d && f.name == "config.json"))
        .collect();
    dirs.sort();
    dirs.dedup();
    if dirs.is_empty() {
        return Ok(Vec::new());
    }
    let variant = |f: &F| diffusers_weight_variant(&f.name);
    let fp16 = |f: &F| variant(f).as_deref() == Some("fp16");
    // fp16 only when every component has it: diffusers loads one variant for the whole pipeline.
    let all_fp16 = dirs.iter().all(|d| {
        files
            .iter()
            .any(|f| f.dir == *d && safetensors(f) && fp16(f))
    });
    let mut parts: Vec<&F> = Vec::new();
    for d in &dirs {
        let mine = |f: &&F| f.dir == *d;
        let pick: Vec<&F> = if all_fp16 {
            files
                .iter()
                .filter(mine)
                .filter(|f| safetensors(f) && fp16(f))
                .collect()
        } else {
            let plain: Vec<&F> = files
                .iter()
                .filter(mine)
                .filter(|f| safetensors(f) && variant(f).is_none())
                .collect();
            if plain.is_empty() {
                files
                    .iter()
                    .filter(mine)
                    .filter(|f| torch_bin(f) && variant(f).is_none())
                    .collect()
            } else {
                plain
            }
        };
        parts.extend(pick);
    }
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    // The shard index of the chosen weights (`x.safetensors.index.json`, `x.safetensors.index.fp16.json`).
    let index_of = |name: &str| {
        let wanted = if all_fp16 {
            ".index.fp16.json"
        } else {
            ".index.json"
        };
        let ext = if parts.iter().any(|p| p.name.ends_with(".safetensors")) {
            ".safetensors"
        } else {
            ".bin"
        };
        name.ends_with(wanted) && name.contains(ext)
    };
    let mut companions: Vec<&F> = files
        .iter()
        .filter(|f| {
            is_diffusers_repo_file(&f.path)
                || (!f.dir.is_empty() && f.name.contains(".index.") && index_of(&f.name))
        })
        .collect();
    companions.sort_by(|a, b| a.path.cmp(&b.path));
    parts.sort_by(|a, b| a.path.cmp(&b.path));
    let parts: Vec<&F> = parts.into_iter().chain(companions).collect();
    let total = parts.len() as u32;
    let shards = parts
        .iter()
        .enumerate()
        .map(|(i, f)| {
            Ok(HfShardInfo {
                download_url: DownloadManager::hf_file_url(repo_id, "resolve", &f.path)?
                    .to_string(),
                filename: f.path.clone(),
                size_bytes: f.size,
                shard_index: i as u32 + 1,
                total_shards: total,
                sha256: f.sha256.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(vec![HfModelVariant {
        not_runnable: None,
        name: repo_id.rsplit('/').next().unwrap_or(repo_id).to_string(),
        quantization: if all_fp16 { "FP16" } else { "DIFFUSERS" }.to_string(),
        total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
        is_split: false,
        shards,
    }])
}

/// The variant of a transformers checkpoint repo (a chat model mistral.rs runs from its folder,
/// or a task model of the Python engine): one download of the root weights
/// (`consolidated*.safetensors` left out when transformers shards are there; PyTorch `.bin`
/// shards only when there is no safetensors file) and every companion file, kept in the repo
/// layout. Empty when the root has no `config.json` or no weights.
fn safetensors_variants(
    repo_id: &str,
    items: &[serde_json::Value],
) -> Result<Vec<HfModelVariant>, String> {
    struct F {
        path: String,
        size: u64,
        sha256: Option<String>,
    }
    let files: Vec<F> = items
        .iter()
        .filter_map(|item| {
            let path = item.get("path").and_then(|v| v.as_str())?.to_string();
            if path.contains('/') && !is_safetensors_repo_file(&path) {
                return None;
            }
            let size = item
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("size").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let sha256 = item
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|v| v.as_str())
                .and_then(valid_sha256);
            Some(F { path, size, sha256 })
        })
        .collect();
    let root = |f: &F| !f.path.contains('/');
    let is_safetensors = |f: &F| root(f) && f.path.to_ascii_lowercase().ends_with(".safetensors");
    let consolidated = |f: &F| f.path.to_ascii_lowercase().starts_with("consolidated");
    let mut weights: Vec<&F> = files.iter().filter(|f| is_safetensors(f)).collect();
    if weights.iter().any(|f| !consolidated(f)) {
        weights.retain(|f| !consolidated(f));
    }
    // One weight set: the default `model*.safetensors` (transformers loads that name, with its
    // `model.safetensors.index.json`) over a precision copy (`model.fp32-0000x-of-0000y`) or a
    // second layout (`transformers-0000x-of-0000y`) of the same model. Only root files are
    // weights here; module folders (`2_Dense/model.safetensors`) are companions and stay.
    let weight_stem = |f: &F| {
        let lower = f.path.to_ascii_lowercase();
        let stem = lower.strip_suffix(".safetensors").unwrap_or(&lower);
        Regex::new(r"-\d{5}-of-\d{5}$")
            .map(|re| re.replace(stem, "").into_owned())
            .unwrap_or_else(|_| stem.to_string())
    };
    if weights.iter().any(|f| weight_stem(f) == "model") {
        weights.retain(|f| weight_stem(f) == "model");
    } else if weights.iter().any(|f| !weight_stem(f).contains('.')) {
        weights.retain(|f| !weight_stem(f).contains('.'));
    }
    // A PyTorch-only repo: its `.bin` files (read with `torch.load(weights_only=True)`).
    if weights.is_empty() {
        weights = files.iter().filter(|f| is_torch_weights(&f.path)).collect();
    }
    // mlx-examples' older Whisper conversions: `weights.npz` (mlx-audio loads it).
    let is_npz = |f: &F| root(f) && f.path.to_ascii_lowercase().ends_with(".npz");
    if weights.is_empty() {
        weights = files.iter().filter(|f| is_npz(f)).collect();
    }
    let is_weights =
        |f: &F| root(f) && (is_safetensors(f) || is_torch_weights(&f.path) || is_npz(f));
    if weights.is_empty() || !files.iter().any(|f| f.path == "config.json") {
        return Ok(Vec::new());
    }
    weights.sort_by(|a, b| a.path.cmp(&b.path));
    // A root tensor file a family loads beside its weights ([`ROOT_TENSOR_COMPANIONS`]); any
    // other root tensor file (a VAE, EMA or precision copy) stays on the Hub.
    let tensor_companion = |f: &F| {
        root(f)
            && ROOT_TENSOR_COMPANIONS.contains(&f.path.to_ascii_lowercase().as_str())
            && !weights.iter().any(|w| w.path == f.path)
    };
    let mut companions: Vec<&F> = files
        .iter()
        .filter(|f| (!is_weights(f) && is_safetensors_repo_file(&f.path)) || tensor_companion(f))
        .collect();
    companions.sort_by(|a, b| a.path.cmp(&b.path));
    let parts: Vec<&F> = weights.into_iter().chain(companions).collect();
    let total = parts.len() as u32;
    let shards = parts
        .iter()
        .enumerate()
        .map(|(i, f)| {
            Ok(HfShardInfo {
                download_url: DownloadManager::hf_file_url(repo_id, "resolve", &f.path)?
                    .to_string(),
                filename: f.path.clone(),
                size_bytes: f.size,
                shard_index: i as u32 + 1,
                total_shards: total,
                sha256: f.sha256.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(vec![HfModelVariant {
        not_runnable: None,
        name: repo_id.rsplit('/').next().unwrap_or(repo_id).to_string(),
        quantization: "SAFETENSORS".to_string(),
        total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
        is_split: false,
        shards,
    }])
}

/// The variant of a repo in mflux's saved layout ([`crate::hub_compat::is_mflux_layout`]): one
/// download of the model card (the mflux engine reads its `base_model`) and every file of
/// `transformer/`, `text_encoder/`, `vae/` and `tokenizer/`, in the repo layout. Nothing else
/// (nested folders, other root files) is fetched.
fn mflux_variants(
    repo_id: &str,
    items: &[serde_json::Value],
) -> Result<Vec<HfModelVariant>, String> {
    let mut files: Vec<(String, u64, Option<String>)> = items
        .iter()
        .filter_map(|item| {
            let path = item.get("path").and_then(|v| v.as_str())?;
            let wanted = path == "README.md"
                || path.split_once('/').is_some_and(|(dir, name)| {
                    (crate::engine::mlx::MFLUX_PARTS.contains(&dir) || dir == "tokenizer")
                        && !name.is_empty()
                        && !name.contains(['/', '\\'])
                        && !name.starts_with('.')
                });
            if !wanted {
                return None;
            }
            let size = item
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_u64())
                .or_else(|| item.get("size").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let sha256 = item
                .get("lfs")
                .and_then(|l| l.get("oid"))
                .and_then(|v| v.as_str())
                .and_then(valid_sha256);
            Some((path.to_string(), size, sha256))
        })
        .collect();
    files.sort();
    let total = files.len() as u32;
    let shards = files
        .into_iter()
        .enumerate()
        .map(|(i, (path, size, sha256))| {
            Ok(HfShardInfo {
                download_url: DownloadManager::hf_file_url(repo_id, "resolve", &path)?.to_string(),
                filename: path,
                size_bytes: size,
                shard_index: i as u32 + 1,
                total_shards: total,
                sha256,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(vec![HfModelVariant {
        not_runnable: None,
        name: repo_id.rsplit('/').next().unwrap_or(repo_id).to_string(),
        quantization: "SAFETENSORS".to_string(),
        total_size_bytes: shards.iter().map(|s| s.size_bytes).sum(),
        is_split: false,
        shards,
    }])
}

/// Checks the container magic of a finished download: `GGUF`, or for a whisper ggml `.bin`
/// the ggml magic. A wrong file (an HTML error page...) is deleted.
fn verify_model_magic(path: &Path, filename: &str) -> Result<(), String> {
    let lower = filename.to_ascii_lowercase();
    // A licence page of a model repository (audio-cpp/Nemotron-3-Diarization-GGUF ships
    // LICENSE.html next to the weights) is HTML by design: there is no error page to catch.
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        return Ok(());
    }
    if lower.ends_with(".safetensors") {
        return verify_safetensors_header(path);
    }
    // Pinned text companions (pronunciation dictionaries); their SHA-256 is checked as well.
    if lower.ends_with(".json") || lower.ends_with(".dict") {
        return verify_not_a_web_page(path);
    }
    if lower.ends_with(".ckpt")
        || lower.ends_with("pytorch_model.bin")
        || is_torch_weights(filename)
    {
        return verify_ckpt_magic(path);
    }
    if is_diffusers_repo_file(filename)
        || is_onnx_repo_file(filename)
        || is_genai_repo_file(filename)
        || is_safetensors_repo_file(filename)
    {
        return verify_not_a_web_page(path);
    }
    if lower.ends_with(".gguf") {
        return verify_gguf_magic(path);
    }
    if !is_ggml_bin_name(filename) {
        return verify_not_a_web_page(path);
    }
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = fs::File::open(path).map_err(read_failed)?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).map_err(read_failed)?;
    if u32::from_le_bytes(magic) != GGML_MAGIC {
        let _ = fs::remove_file(path);
        return Err(err("downloads.not_ggml").into());
    }
    Ok(())
}

/// An ONNX graph (protobuf) and the JSON files of its repo have no magic number; a finished
/// download that is empty or starts like an HTML page (an error or login page) is deleted.
fn verify_not_a_web_page(path: &Path) -> Result<(), String> {
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = fs::File::open(path).map_err(read_failed)?;
    let mut head = [0u8; 64];
    let n = f.read(&mut head).map_err(read_failed)?;
    let text = String::from_utf8_lossy(&head[..n]);
    let trimmed = text.trim_start();
    if n == 0 || trimmed.starts_with("<!") || trimmed.to_lowercase().starts_with("<html") {
        let _ = fs::remove_file(path);
        return Err(err("downloads.not_a_model_file").into());
    }
    Ok(())
}

/// A safetensors file starts with the length of its JSON header (8 bytes, little endian) and
/// the header itself, which opens with `{`. An HTML error page does not.
fn verify_safetensors_header(path: &Path) -> Result<(), String> {
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = fs::File::open(path).map_err(read_failed)?;
    let mut head = [0u8; 9];
    f.read_exact(&mut head).map_err(read_failed)?;
    let len = u64::from_le_bytes(head[..8].try_into().expect("8 bytes"));
    if head[8] != b'{' || len == 0 || len > 100 * 1024 * 1024 {
        let _ = fs::remove_file(path);
        return Err(err("downloads.not_safetensors").into());
    }
    Ok(())
}

/// A PyTorch checkpoint is a zip (`PK`) or an old pickle (`0x80`).
fn verify_ckpt_magic(path: &Path) -> Result<(), String> {
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = fs::File::open(path).map_err(read_failed)?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).map_err(read_failed)?;
    if &magic != b"PK" && magic[0] != 0x80 {
        let _ = fs::remove_file(path);
        return Err(err("downloads.not_checkpoint").into());
    }
    Ok(())
}

fn verify_gguf_magic(path: &Path) -> Result<(), String> {
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = fs::File::open(path).map_err(read_failed)?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).map_err(read_failed)?;
    if &magic != b"GGUF" {
        let _ = fs::remove_file(path);
        return Err(err("downloads.not_gguf").into());
    }
    Ok(())
}

/// No variant matches the requested file / quantization.
pub const ERR_VARIANT_NOT_FOUND: &str = "downloads.variant_not_found";
/// Several variants match; name the file.
pub const ERR_VARIANT_AMBIGUOUS: &str = "downloads.variant_ambiguous";

/// The variant named by `file` (a file of the variant or the variant's name) or by `quant`
/// (quantization, case-insensitive; projector files are skipped unless asked for). Without
/// either, the repo must have exactly one model variant.
pub fn pick_variant(
    variants: &[HfModelVariant],
    file: Option<&str>,
    quant: Option<&str>,
) -> Result<HfModelVariant, String> {
    let file = file.map(str::trim).filter(|f| !f.is_empty());
    let quant = quant.map(str::trim).filter(|q| !q.is_empty());
    let is_file = |v: &HfModelVariant, f: &str| {
        v.name.eq_ignore_ascii_case(f)
            || v.shards.iter().any(|s| {
                s.filename.eq_ignore_ascii_case(f)
                    || s.filename
                        .rsplit('/')
                        .next()
                        .is_some_and(|n| n.eq_ignore_ascii_case(f))
            })
    };
    let matches: Vec<&HfModelVariant> = variants
        .iter()
        .filter(|v| match (file, quant) {
            (Some(f), _) => is_file(v, f),
            // `repo:onnx/model.onnx`: a path after the colon names a file, not a quantization.
            (None, Some(q)) => {
                v.quantization.eq_ignore_ascii_case(q) || (looks_like_file(q) && is_file(v, q))
            }
            (None, None) => v.quantization != "mmproj",
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(ERR_VARIANT_NOT_FOUND.to_string()),
        many => {
            let label = |v: &&HfModelVariant| match v.shards.as_slice() {
                [one] => one.filename.clone(),
                _ => v.name.clone(),
            };
            let shown: Vec<String> = many.iter().take(MAX_LISTED_VARIANTS).map(label).collect();
            let mut files = shown.join(", ");
            if many.len() > shown.len() {
                files.push_str(&format!(", ... (+{} more)", many.len() - shown.len()));
            }
            Err(err(ERR_VARIANT_AMBIGUOUS)
                .num("count", many.len() as u64)
                .with("files", files)
                .into())
        }
    }
}

/// Candidate files named in the "several files match" error.
const MAX_LISTED_VARIANTS: usize = 10;

/// Whether a `repo:<tag>` tag is a file path (`onnx/model.onnx`, `m.gguf`) rather than a
/// quantization (`Q4_K_M`).
pub fn looks_like_file(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    tag.contains('/')
        || [".gguf", ".onnx", ".safetensors", ".bin", ".ckpt", ".pt"]
            .iter()
            .any(|ext| lower.ends_with(ext))
}

fn valid_sha256(value: &str) -> Option<String> {
    (value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

async fn verify_download_sha256(
    part_path: &Path,
    item: &DownloadItem,
    expected: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(expected) = expected else {
        return Ok(None);
    };
    let path = part_path.to_path_buf();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&path))
        .await
        .map_err(join_error)??;
    if !actual.eq_ignore_ascii_case(expected) {
        if let Err(error) = tokio::fs::remove_file(part_path).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Bozuk kısmi dosya silinemedi: {}", error);
            }
        }
        return Err(err("downloads.sha256_mismatch")
            .with("file", &item.filename)
            .into());
    }
    Ok(Some(actual))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = fs::File::open(path).map_err(read_failed)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(read_failed)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect())
}

fn hashes_file() -> PathBuf {
    paths::config_dir().join("model_hashes.json")
}

/// SHA-256 of downloaded files as verified at download time (`model_hashes.json`, keyed by
/// the file path). Empty when the file is missing or unreadable.
pub fn recorded_hashes() -> HashMap<String, String> {
    load_hashes().unwrap_or_default()
}

fn load_hashes() -> Result<HashMap<String, String>, String> {
    crate::store::JsonStore::new(hashes_file())
        .load_versioned_or_default::<HashMap<String, String>>(&HASHES_SCHEMA)
        .map(|v| v.data)
}

fn record_hash(path: &Path, hash: &str) -> Result<(), String> {
    let key = path.to_string_lossy().to_string();
    let hash = hash.to_string();
    crate::store::JsonStore::new(hashes_file()).update_versioned::<HashMap<String, String>, _>(
        &HASHES_SCHEMA,
        |map| {
            map.insert(key, hash);
            Ok(())
        },
    )
}

fn recorded_hash(path: &Path) -> Option<String> {
    match load_hashes() {
        Ok(hashes) => hashes.get(&path.to_string_lossy().to_string()).cloned(),
        Err(error) => {
            eprintln!("Model özetleri okunamadı: {}", error);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn interrupted_download_fails_without_final_file_and_resumes() {
        use axum::{body::Body, http::Response, routing::get, Router};
        use futures_util::stream;

        const BODY: &[u8] = b"GGUF local download fixture bytes";
        const PREFIX: usize = 12;
        // Test both metadata-known and HTTP-only sizes, plus a clean stream EOF
        // whose Content-Range still advertises missing bytes.
        for (size, clean_eof) in [(BODY.len() as u64, false), (0, false), (0, true)] {
            let app = Router::new().route(
                "/model.gguf",
                get(move |headers: axum::http::HeaderMap| async move {
                    if let Some(range) = headers.get("range") {
                        assert_eq!(range, "bytes=12-");
                        assert_eq!(headers.get("if-range").unwrap(), "\"fixture-v1\"");
                        return Response::builder()
                            .status(206)
                            .header(
                                "content-range",
                                format!("bytes {PREFIX}-{}/{}", BODY.len() - 1, BODY.len()),
                            )
                            .header("etag", "\"fixture-v1\"")
                            .body(Body::from(&BODY[PREFIX..]))
                            .unwrap();
                    }
                    let body = stream::unfold(false, |sent| async move {
                        if sent {
                            // Let hyper send headers and the prefix before closing.
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            None
                        } else {
                            Some((Ok::<_, std::io::Error>(&BODY[..PREFIX]), true))
                        }
                    });
                    let mut response = Response::builder().header("etag", "\"fixture-v1\"").header(
                        "content-length",
                        if clean_eof { PREFIX } else { BODY.len() },
                    );
                    if clean_eof {
                        response = response.status(206).header(
                            "content-range",
                            format!("bytes 0-{}/{}", PREFIX - 1, BODY.len()),
                        );
                    }
                    response.body(Body::from_stream(body)).unwrap()
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let item = DownloadItem {
                url,
                filename: "model.gguf".into(),
                size,
                shard_index: 1,
                sha256: Some(crate::hex::lower(&Sha256::digest(BODY))),
                custom_url: false,
            };
            let dir = tempfile::tempdir().unwrap();
            let manager = DownloadManager::default();
            let job = JobCtx {
                id: 1,
                cancel: Arc::new(AtomicBool::new(false)),
            };
            let failure = manager
                .run_job(&job, std::slice::from_ref(&item), dir.path(), None)
                .await
                .unwrap_err();
            let (key, params) = crate::error_key::parse(&failure).unwrap();
            assert_eq!(
                key,
                if clean_eof {
                    "downloads.incomplete"
                } else {
                    "downloads.interrupted"
                }
            );
            assert_eq!(params["file"], "model.gguf");
            assert!(!dir.path().join("model.gguf").exists());
            let part = DownloadManager::cache_dir().join(part_name(&item.url, &item.filename));
            assert_eq!(fs::read(&part).unwrap(), &BODY[..PREFIX]);
            manager
                .run_job(&job, &[item], dir.path(), None)
                .await
                .unwrap();
            assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), BODY);
            assert!(!part.exists());
            server.abort();
            let _ = server.await;
        }
    }

    #[test]
    fn an_mflux_repo_downloads_its_folders_and_card() {
        let file =
            |p: &str, size: u64| serde_json::json!({"type": "file", "path": p, "size": size});
        let lfs = |p: &str, size: u64| serde_json::json!({"type": "file", "path": p, "size": 100, "lfs": {"size": size, "oid": "a".repeat(64)}});
        let items = vec![
            file(".gitattributes", 1),
            file(".DS_Store", 1),
            file("README.md", 2),
            lfs("text_encoder/0.safetensors", 2000),
            file("text_encoder/model.safetensors.index.json", 3),
            file("tokenizer/tokenizer.json", 4),
            file("tokenizer/chat_template.jinja", 5),
            lfs("transformer/0.safetensors", 1000),
            file("transformer/model.safetensors.index.json", 6),
            file("transformer/extra/x.json", 7),
            lfs("vae/0.safetensors", 300),
            file("vae/model.safetensors.index.json", 8),
            file("vae/.hidden", 9),
            file("assets/sample.png", 10),
        ];
        let paths: Vec<String> = items
            .iter()
            .map(|i| i["path"].as_str().unwrap().to_string())
            .collect();
        assert!(crate::hub_compat::is_mflux_layout(&paths));
        let v = mflux_variants("mflux-community/flux2-klein-4b-mflux-q4", &items).unwrap();
        assert_eq!(v.len(), 1);
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            names,
            [
                "README.md",
                "text_encoder/0.safetensors",
                "text_encoder/model.safetensors.index.json",
                "tokenizer/chat_template.jinja",
                "tokenizer/tokenizer.json",
                "transformer/0.safetensors",
                "transformer/model.safetensors.index.json",
                "vae/0.safetensors",
                "vae/model.safetensors.index.json",
            ]
        );
        assert_eq!(
            v[0].total_size_bytes,
            2 + 2000 + 3 + 5 + 4 + 1000 + 6 + 300 + 8
        );
        assert_eq!(
            v[0].shards[1].sha256.as_deref(),
            Some("a".repeat(64).as_str())
        );
        assert!(!v[0].is_split);
    }

    #[test]
    fn card_keeps_listing_verdict_for_non_gguf_engines() {
        let item = serde_json::json!({"id":"sentence-transformers/all-MiniLM-L6-v2",
            "pipeline_tag":"sentence-similarity","library_name":"sentence-transformers",
            "tags":["sentence-transformers","onnx","safetensors"],
            "siblings":[{"rfilename":"config.json"},{"rfilename":"onnx/model.onnx"},
                {"rfilename":"model.safetensors"},{"rfilename":"tokenizer.json"}]});
        let analysis = crate::hub_tasks::analyze_repo(&item);
        // The raw analysis is llama-server's "no GGUF" verdict, which the card used to show.
        assert!(analysis.not_runnable.is_some());
        let listed = parse_search_item(&item).unwrap();
        assert!(matches!(
            listed.compatibility,
            crate::hub_compat::Compatibility::Runnable { .. }
        ));
        assert_eq!(listed.not_runnable, None);
        assert_eq!(card_not_runnable(&item, analysis.not_runnable), None);
    }

    #[test]
    fn qwen3_tts_gguf_speaks_with_llama_cpp() {
        let item = serde_json::json!({"id":"ggml-org/Qwen3-TTS-12Hz-1.7B-Base-GGUF",
            "tags":["gguf","endpoints_compatible","region:us"],
            "gguf":{"architecture":"qwen3tts","total":1733157888},
            "siblings":[{"rfilename":"Qwen3-TTS-12Hz-1.7B-Base-Q4_K_M.gguf"},
                {"rfilename":"mmproj-Qwen3-TTS-12Hz-1.7B-Base-Q8_0.gguf"}]});
        let listed = parse_search_item(&item).unwrap();
        assert_eq!(listed.pipeline_tag.as_deref(), Some("text-to-speech"));
        assert_eq!(
            listed.compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: "llama.cpp".into()
            }
        );
        assert_eq!(listed.not_runnable, None);
    }

    #[test]
    fn bundle_companion_in_a_subfolder_is_found_by_its_path() {
        let items = vec![
            serde_json::json!({"path":"split_files/diffusion_models/flux2_dev_fp8mixed.safetensors","size":1}),
            serde_json::json!({"path":"split_files/vae/flux2-vae.safetensors","size":134,
                "lfs":{"size":336213556,"oid":"d64f3a68e1cc4f9f4e29b6e0da38a0204fe9a49f2d4053f0ec1fa1ca02f9c4b5"}}),
        ];
        let v = companion_variant_from(
            "Comfy-Org/flux2-dev",
            "split_files/vae/flux2-vae.safetensors",
            &items,
        )
        .unwrap()
        .unwrap();
        assert_eq!(v.total_size_bytes, 336213556);
        assert_eq!(
            v.shards[0].filename,
            "split_files/vae/flux2-vae.safetensors"
        );
        assert!(v.shards[0].sha256.is_some());
        assert!(
            companion_variant_from("Comfy-Org/flux2-dev", "vae/missing.safetensors", &items)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn transcribe_cpp_speech_recognition_repos_run_on_transcribe_cpp() {
        // handy-computer listings (2026-10-16): `library_name: transcribe.cpp`.
        for repo in [
            "handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf",
            "handy-computer/parakeet-unified-en-0.6b-gguf",
            "handy-computer/cohere-transcribe-03-2026-gguf",
            "handy-computer/canary-180m-flash-gguf",
        ] {
            let item = serde_json::json!({"id":repo,"pipeline_tag":"automatic-speech-recognition",
                "library_name":"transcribe.cpp","tags":["gguf","transcribe.cpp"],
                "siblings":[{"rfilename":"model-Q4_K_M.gguf"}]});
            assert_eq!(
                parse_search_item(&item).unwrap().compatibility,
                crate::hub_compat::Compatibility::Runnable {
                    runtime: crate::engine::TRANSCRIBE_CPP.into()
                },
                "{repo}"
            );
        }
        // Voice activity detection is not speech recognition.
        let vad = serde_json::json!({"id":"handy-computer/silero-vad-gguf",
            "pipeline_tag":"voice-activity-detection","library_name":"transcribe.cpp",
            "siblings":[{"rfilename":"silero-vad.gguf"}]});
        assert_ne!(
            parse_search_item(&vad).unwrap().compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: crate::engine::TRANSCRIBE_CPP.into()
            }
        );
    }

    #[test]
    fn crispasr_tts_hub_offers_only_initial_backends() {
        for repo in ["cstr/kokoro-82m-GGUF", "cstr/pocket-tts-GGUF"] {
            let item = serde_json::json!({"id":repo,"pipeline_tag":"text-to-speech", "tags":["gguf"],
                "siblings":[{"rfilename":"model.gguf"}]});
            assert_eq!(
                parse_search_item(&item).unwrap().compatibility,
                crate::hub_compat::Compatibility::Runnable {
                    runtime: crate::engine::CRISPASR.into()
                }
            );
        }
        for repo in [
            "cstr/Qwen3-TTS-GGUF",
            "cstr/kokoro-voices-GGUF",
            "cstr/chatterbox-GGUF",
        ] {
            let item = serde_json::json!({"id":repo,"pipeline_tag":"text-to-speech", "tags":["gguf"],
                "siblings":[{"rfilename":"model.gguf"}]});
            assert!(matches!(
                parse_search_item(&item).unwrap().compatibility,
                crate::hub_compat::Compatibility::Unsupported { .. }
            ));
        }
    }

    #[test]
    fn test_valid_sha256() {
        assert_eq!(valid_sha256(&"*".repeat(64)), None);
        assert_eq!(valid_sha256(&"a".repeat(63)), None);
        assert_eq!(valid_sha256(&"a".repeat(65)), None);
        assert_eq!(valid_sha256(&"g".repeat(64)), None);
        assert_eq!(
            valid_sha256(&"ABCDEF01".repeat(8)),
            Some("abcdef01".repeat(8))
        );
        assert_eq!(
            valid_sha256(&"abcdef01".repeat(8)),
            Some("abcdef01".repeat(8))
        );
    }

    #[tokio::test]
    async fn test_gated_sha256_loopback() {
        use tokio::io::AsyncReadExt;

        let bytes = b"GGUF gated model fixture";
        let actual = crate::hex::lower(&Sha256::digest(bytes));
        for (initial, refreshed, mismatch) in [
            ("*".repeat(64), "*".repeat(64), false),
            ("*".repeat(64), actual.clone(), false),
            ("*".repeat(64), "unavailable".into(), false),
            ("*".repeat(64), "0".repeat(64), true),
            ("0".repeat(64), actual.clone(), true),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let first_oid = initial.clone();
            let server = tokio::spawn(async move {
                let mut listings = 0;
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut chunk = [0u8; 1024];
                        let read = stream.read(&mut chunk).await.unwrap();
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&request);
                    if !request.starts_with("GET /model.gguf ")
                        && listings > 0
                        && refreshed == "unavailable"
                    {
                        stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                        continue;
                    }
                    let body = if request.starts_with("GET /model.gguf ") {
                        bytes.to_vec()
                    } else {
                        let oid = if listings == 0 {
                            &first_oid
                        } else {
                            &refreshed
                        };
                        listings += 1;
                        serde_json::to_vec(&serde_json::json!([{
                            "type": "file", "path": "model.gguf",
                            "lfs": { "size": bytes.len(), "oid": oid }
                        }]))
                        .unwrap()
                    };
                    let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n", body.len());
                    stream.write_all(headers.as_bytes()).await.unwrap();
                    stream.write_all(&body).await.unwrap();
                }
            });
            let api = format!("{base}/api");
            let listing = DownloadManager::list_repo_files_from("owner/model", None, &api)
                .await
                .unwrap();
            assert_eq!(listing[0]["lfs"]["oid"].as_str(), Some(initial.as_str()));
            // A persisted job may still contain the masked value from the old listing.
            let item = DownloadItem {
                url: format!("{base}/model.gguf"),
                filename: "model.gguf".into(),
                size: bytes.len() as u64,
                shard_index: 1,
                sha256: Some(initial),
                custom_url: false,
            };
            let temp = tempfile::tempdir().unwrap();
            let part = temp.path().join("model.part");
            let job = JobCtx {
                id: 1,
                cancel: Arc::new(AtomicBool::new(false)),
            };
            let manager = DownloadManager::default();
            let written = manager
                .fetch_to_part(
                    &job,
                    &DownloadManager::client().unwrap(),
                    &item,
                    &part,
                    0,
                    None,
                )
                .await
                .unwrap();
            assert_eq!(written, item.size);
            verify_model_magic(&part, &item.filename).unwrap();
            let expected = match item.sha256.as_deref().and_then(valid_sha256) {
                Some(hash) => Some(hash),
                None => {
                    DownloadManager::refresh_file_sha256("owner/model", &item, None, &api).await
                }
            };
            let result = verify_download_sha256(&part, &item, expected.as_deref()).await;
            if mismatch {
                assert!(result.unwrap_err().contains("downloads.sha256_mismatch"));
                assert!(!part.exists());
            } else {
                assert!(result.is_ok());
                assert_eq!(fs::read(&part).unwrap(), bytes);
            }
            server.abort();
        }
    }

    #[test]
    fn test_hf_token_scoping() {
        assert!(DownloadManager::is_hf_url(
            "https://huggingface.co/a/b/resolve/main/x.gguf"
        ));
        assert!(DownloadManager::is_hf_url(
            "https://cdn-lfs.huggingface.co/x"
        ));
        assert!(DownloadManager::is_hf_url("https://hf.co/x"));
        assert!(!DownloadManager::is_hf_url("http://huggingface.co/x"));
        assert!(!DownloadManager::is_hf_url(
            "https://evil.com/huggingface.co"
        ));
        assert!(!DownloadManager::is_hf_url(
            "https://huggingface.co.evil.com/x"
        ));
        assert!(!DownloadManager::is_hf_url(
            "https://example.com/model.gguf"
        ));
    }

    #[test]
    fn test_validate_repo_id() {
        assert!(DownloadManager::validate_repo_id("bartowski/Llama-3.2-1B-Instruct-GGUF").is_ok());
        assert!(DownloadManager::validate_repo_id("../etc").is_err());
        assert!(DownloadManager::validate_repo_id("a/b/c").is_err());
        assert!(DownloadManager::validate_repo_id("a/..").is_err());
    }

    #[test]
    fn test_hf_file_url_encodes_path_segments() {
        let url =
            DownloadManager::hf_file_url("owner/repo", "resolve", "alt dir/a #100%?.ğguf").unwrap();
        assert_eq!(
            url.as_str(),
            "https://huggingface.co/owner/repo/resolve/main/alt%20dir/a%20%23100%25%3F.%C4%9Fguf"
        );
        assert_eq!(reqwest::Url::parse(url.as_str()).unwrap(), url);
    }

    #[test]
    fn test_resume_response_validation() {
        let metadata = PartMetadata {
            source_url: "https://example.com/model.gguf".into(),
            etag: Some("\"v1\"".into()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            total_size: 20,
        };
        assert_eq!(metadata.validator(), Some("\"v1\""));
        assert_eq!(
            decide_resume_response(
                reqwest::StatusCode::PARTIAL_CONTENT,
                Some("bytes 10-19/20"),
                10,
                20,
                Some(10),
            )
            .unwrap(),
            ResumeAction::Download {
                append: true,
                total: 20
            }
        );
        assert_eq!(
            decide_resume_response(
                reqwest::StatusCode::PARTIAL_CONTENT,
                Some("bytes 9-18/20"),
                10,
                20,
                Some(10),
            )
            .unwrap(),
            ResumeAction::Restart
        );
        assert_eq!(
            decide_resume_response(
                reqwest::StatusCode::PARTIAL_CONTENT,
                Some("bytes 10-19/21"),
                10,
                20,
                Some(10),
            )
            .unwrap(),
            ResumeAction::Restart
        );
        assert_eq!(
            decide_resume_response(
                reqwest::StatusCode::RANGE_NOT_SATISFIABLE,
                Some("bytes */20"),
                20,
                20,
                None,
            )
            .unwrap(),
            ResumeAction::Complete
        );
        assert_eq!(
            decide_resume_response(
                reqwest::StatusCode::RANGE_NOT_SATISFIABLE,
                Some("bytes */20"),
                19,
                20,
                None,
            )
            .unwrap(),
            ResumeAction::Restart
        );
    }

    #[test]
    fn test_part_names_include_source_url_hash() {
        let nested = part_name("https://example.com/repo/a/b.gguf", "a/b.gguf");
        let flattened = part_name("https://example.com/repo/a__b.gguf", "a__b.gguf");
        let other_repo = part_name("https://other.example/a/b.gguf", "a/b.gguf");
        assert_ne!(nested, flattened);
        assert_ne!(nested, other_repo);
        assert_ne!(nested, "a__b.gguf.part");
    }

    #[test]
    fn custom_url_whisper_bin_keeps_its_name() {
        // The real first four bytes of a whisper ggml file (`ggml`, little-endian u32).
        let ggml = [0x6c, 0x6d, 0x67, 0x67];
        assert_eq!(sniff_kind(ggml), SniffedKind::WhisperGgml);
        assert_eq!(sniff_kind(*b"GGUF"), SniffedKind::Gguf);
        // The name of the user's real failure: it used to become `ggml-base.en.bin.gguf`.
        assert_eq!(
            custom_download_name(Some("ggml-base.en.bin"), Some(ggml)).unwrap(),
            "ggml-base.en.bin"
        );
        assert_eq!(
            custom_download_name(Some("GGML-BASE.BIN"), Some(ggml)).unwrap(),
            "GGML-BASE.bin"
        );
        // A whisper model under a name the library scan would not look at gets the prefix.
        assert_eq!(
            custom_download_name(Some("my-whisper.bin"), Some(ggml)).unwrap(),
            "ggml-my-whisper.bin"
        );
        assert_eq!(
            custom_download_name(Some("whisper-tiny"), Some(ggml)).unwrap(),
            "ggml-whisper-tiny.bin"
        );
        assert!(is_ggml_bin_name(
            &custom_download_name(None, Some(ggml)).unwrap()
        ));
        // GGUF is as before: `.gguf` names are kept, a GGUF behind another name gets `.gguf`.
        assert_eq!(
            custom_download_name(Some("m-Q4_K_M.gguf"), None).unwrap(),
            "m-Q4_K_M.gguf"
        );
        assert_eq!(
            custom_download_name(Some("model"), Some(*b"GGUF")).unwrap(),
            "model.gguf"
        );
        assert_eq!(
            custom_download_name(Some("m.bin"), Some(*b"GGUF")).unwrap(),
            "m.gguf"
        );
        // Not a model: an HTML page, a zip, PyTorch weights (zip / pickle start).
        for head in [
            *b"<!DO",
            *b"<htm",
            [b'P', b'K', 3, 4],
            [0x80, 0x02, 0x8a, 0x0a],
        ] {
            let e = custom_download_name(Some("ggml-base.bin"), Some(head)).unwrap_err();
            assert!(e.starts_with("downloads.custom_not_model"), "{e}");
        }
        // Bytes could not be read: the name decides, the post-download magic check backs it.
        assert_eq!(
            custom_download_name(Some("ggml-base.en.bin"), None).unwrap(),
            "ggml-base.en.bin"
        );
        assert_eq!(
            custom_download_name(Some("model"), None).unwrap(),
            "model.gguf"
        );
        assert_eq!(
            custom_download_name(None, None).unwrap(),
            "custom_model.gguf"
        );
    }

    /// Real Hugging Face files through the custom-URL path: the first bytes of a whisper model
    /// and of a repo page that is not a model. Reads a few bytes, no download.
    /// `cargo test -p harness-core --lib real_custom_url_sniff -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn real_custom_url_sniff() {
        let client = DownloadManager::custom_client().unwrap();
        let url = reqwest::Url::parse(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        )
        .unwrap();
        let magic = sniff_custom_magic(&client, &url, None).await.unwrap();
        assert_eq!(sniff_kind(magic), SniffedKind::WhisperGgml);
        assert_eq!(
            custom_download_name(Some("ggml-tiny.en.bin"), Some(magic)).unwrap(),
            "ggml-tiny.en.bin"
        );
        let page = reqwest::Url::parse("https://huggingface.co/ggerganov/whisper.cpp").unwrap();
        let head = sniff_custom_magic(&client, &page, None).await.unwrap();
        assert_eq!(sniff_kind(head), SniffedKind::Other);
        assert!(custom_download_name(Some("whisper.cpp"), Some(head)).is_err());
    }

    /// The Hub search of the user's report: "whisper.cpp" and the speech task list the repo
    /// first, a search for something else does not.
    /// `cargo test -p harness-core --lib real_whisper_repo_in_search -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn real_whisper_repo_in_search() {
        let by_query = HfSearchOptions {
            query: "whisper.cpp".into(),
            ..Default::default()
        };
        let items = DownloadManager::search_models(&by_query, None)
            .await
            .unwrap();
        assert_eq!(items[0].id, WHISPER_CPP_REPO);
        assert!(items[0].not_runnable.is_none());
        let by_task = HfSearchOptions {
            pipeline_tag: Some("automatic-speech-recognition".into()),
            ..Default::default()
        };
        let items = DownloadManager::search_models(&by_task, None)
            .await
            .unwrap();
        assert_eq!(items[0].id, WHISPER_CPP_REPO);
        let other = HfSearchOptions {
            query: "gemma".into(),
            ..Default::default()
        };
        let items = DownloadManager::search_models(&other, None).await.unwrap();
        assert!(items.iter().all(|m| m.id != WHISPER_CPP_REPO));
    }

    /// With `include_non_gguf` the listing is no longer GGUF only; the default keeps the filter.
    #[test]
    fn search_params_gguf_filter_is_optional() {
        let has_gguf = |o: &HfSearchOptions| {
            search_params(o)
                .iter()
                .any(|(k, v)| *k == "filter" && v == "gguf")
        };
        assert!(has_gguf(&HfSearchOptions::default()));
        assert!(!has_gguf(&HfSearchOptions {
            include_non_gguf: Some(true),
            ..Default::default()
        }));
    }

    /// The real HF listing item of `ggerganov/whisper.cpp` is a whisper ggml repo; a repo of
    /// safetensors weights is not runnable by any Harness runtime.
    #[test]
    fn mistagged_sd_gguf_and_diffusion_loras_in_the_listing() {
        use crate::hub_compat::{Compatibility, ModelFormat};
        use crate::model_registry::Capability;
        let sd = serde_json::json!({
            "id": "calcuis/sd3.5-large-turbo",
            "pipeline_tag": "image-to-text",
            "tags": ["gguf", "stable-diffusion", "gguf-comfy", "image-to-text",
                "base_model:stabilityai/stable-diffusion-3.5-large-turbo",
                "base_model:quantized:stabilityai/stable-diffusion-3.5-large-turbo"],
            "siblings": [{"rfilename": "sd3.5_large_turbo-q4_0.gguf"}],
        });
        let repo = parse_search_item(&sd).unwrap();
        assert_eq!(repo.pipeline_tag.as_deref(), Some("text-to-image"));
        assert_eq!(repo.capabilities, vec![Capability::ImageGeneration]);
        assert_eq!(
            repo.compatibility,
            Compatibility::Runnable {
                runtime: "stable-diffusion.cpp".into()
            }
        );
        let lora = serde_json::json!({
            "id": "prithivMLmods/SD3.5-Large-Turbo-HyperRealistic-LoRA",
            "pipeline_tag": "text-to-image",
            "library_name": "diffusers",
            "tags": ["diffusers", "text-to-image", "lora", "template:diffusion-lora",
                "base_model:stabilityai/stable-diffusion-3.5-large-turbo",
                "base_model:adapter:stabilityai/stable-diffusion-3.5-large-turbo"],
            "siblings": [{"rfilename": "SD3.5-Large-Turbo-HyperRealistic-LoRA.safetensors"}],
        });
        let repo = parse_search_item(&lora).unwrap();
        assert_eq!(
            repo.not_runnable,
            Some(crate::hub_tasks::NotRunnable::Adapter)
        );
        assert!(repo.capabilities.is_empty());
        assert!(matches!(
            repo.compatibility,
            Compatibility::Unsupported { .. }
        ));
        assert_ne!(repo.format, ModelFormat::Gguf);
        assert!(repo.adapter);
        assert_eq!(
            repo.adapter_base.as_deref(),
            Some("stabilityai/stable-diffusion-3.5-large-turbo")
        );
    }

    /// A GGUF LLM LoRA stays downloadable (llama-server loads adapters) and says what it adapts.
    #[test]
    fn gguf_llm_lora_repo_stays_runnable_and_names_its_base() {
        use crate::hub_compat::{Compatibility, ModelFormat};
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/hf-repo-signals.json")).unwrap();
        let item = &all["ggml-org/LoRA-Qwen2.5-1.5B-Instruct-abliterated-F16-GGUF"];
        let repo = parse_search_item(item).unwrap();
        assert!(repo.adapter);
        assert_eq!(
            repo.adapter_base.as_deref(),
            Some("ngxson/LoRA-Qwen2.5-1.5B-Instruct-abliterated")
        );
        assert_eq!(repo.format, ModelFormat::Gguf);
        assert!(matches!(repo.compatibility, Compatibility::Runnable { .. }));
    }

    /// Text-to-image repos run on stable-diffusion.cpp when their weights are a GGUF or a root
    /// checkpoint; a diffusers repo of folders only stays unsupported.
    #[test]
    fn image_repos_are_runnable_by_the_image_runtime() {
        use crate::hub_compat::{Compatibility, ModelFormat};
        let gguf = serde_json::json!({
            "id": "city96/FLUX.1-schnell-gguf",
            "pipeline_tag": "text-to-image",
            "tags": ["gguf", "text-to-image"],
            "gguf": {"architecture": "flux"},
            "siblings": [{"rfilename": "flux1-schnell-Q4_K_S.gguf"}],
        });
        let repo = parse_search_item(&gguf).unwrap();
        assert_eq!(repo.not_runnable, None);
        assert_eq!(repo.format, ModelFormat::Gguf);
        assert_eq!(
            repo.compatibility,
            Compatibility::Runnable {
                runtime: "stable-diffusion.cpp".into()
            }
        );
        assert_eq!(
            repo.capabilities,
            vec![crate::model_registry::Capability::ImageGeneration]
        );
        let single = serde_json::json!({
            "id": "stabilityai/sdxl-turbo",
            "pipeline_tag": "text-to-image",
            "library_name": "diffusers",
            "tags": ["diffusers", "safetensors", "text-to-image"],
            "siblings": [{"rfilename": "model_index.json"},
                         {"rfilename": "sd_xl_turbo_1.0_fp16.safetensors"},
                         {"rfilename": "unet/diffusion_pytorch_model.safetensors"}],
        });
        let repo = parse_search_item(&single).unwrap();
        assert_eq!(repo.not_runnable, None);
        assert!(matches!(repo.compatibility, Compatibility::Runnable { .. }));
        let folders = serde_json::json!({
            "id": "someone/diffusers-only",
            "pipeline_tag": "text-to-image",
            "library_name": "diffusers",
            "tags": ["diffusers", "text-to-image"],
            "siblings": [{"rfilename": "model_index.json"},
                         {"rfilename": "unet/diffusion_pytorch_model.safetensors"}],
        });
        // A diffusers-only pipeline (no single-file checkpoint) is the Python engine's.
        let repo = parse_search_item(&folders).unwrap();
        assert_eq!(repo.not_runnable, None);
        assert_eq!(repo.format, ModelFormat::Diffusers);
        assert_eq!(
            repo.compatibility,
            Compatibility::Runnable {
                runtime: "python-hf".into()
            }
        );
    }

    #[test]
    fn listing_items_get_a_format_and_compatibility() {
        let item: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/hf-whisper-cpp-listing.json"))
                .unwrap();
        let repo = parse_search_item(&item).unwrap();
        assert_eq!(repo.format, crate::hub_compat::ModelFormat::WhisperGgml);
        assert_eq!(
            repo.compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: "whisper.cpp".into()
            }
        );
        assert_eq!(
            repo.capabilities,
            vec![crate::model_registry::Capability::Asr]
        );
        let st = serde_json::json!({
            "id": "someone/model",
            "tags": ["transformers", "safetensors", "llama"],
            "library_name": "transformers",
            "siblings": [{"rfilename": "model.safetensors"}, {"rfilename": "config.json"}],
        });
        let repo = parse_search_item(&st).unwrap();
        // No task is declared or inferable: no capability is made up.
        assert!(repo.capabilities.is_empty());
        assert_eq!(
            repo.compatibility,
            crate::hub_compat::Compatibility::Unsupported {
                format: crate::hub_compat::ModelFormat::Safetensors
            }
        );
    }

    /// Speech repos made for their own package are not offered for the Python engine; a
    /// transformers model type of the same task still is (listing shapes of 2026-10-14).
    #[test]
    fn speech_repos_need_a_model_type_transformers_loads() {
        let item = |id: &str, task: &str, model_type: &str| {
            serde_json::json!({
                "id": id,
                "pipeline_tag": task,
                "tags": ["safetensors", task],
                "siblings": [{"rfilename": "config.json"}, {"rfilename": "model.safetensors"}],
                "config": {"model_type": model_type},
            })
        };
        let python = crate::hub_compat::Compatibility::Runnable {
            runtime: crate::engine::PYTHON_HF.into(),
        };
        let unsupported = crate::hub_compat::Compatibility::Unsupported {
            format: crate::hub_compat::ModelFormat::Safetensors,
        };
        for (id, task, model_type, runs) in [
            (
                "Qwen/Qwen3-TTS-12Hz-0.6B-Base",
                "text-to-speech",
                "qwen3_tts",
                false,
            ),
            (
                "microsoft/VibeVoice-Realtime-0.5B",
                "text-to-speech",
                "vibevoice_streaming",
                false,
            ),
            (
                "remsky/kokoro-inno-clone-tuner",
                "text-to-speech",
                "kokoro-inno-reference-tuner",
                false,
            ),
            (
                "microsoft/VibeVoice-1.5B",
                "text-to-speech",
                "vibevoice",
                false,
            ),
            ("facebook/mms-tts-eng", "text-to-speech", "vits", true),
            (
                "openai/whisper-small",
                "automatic-speech-recognition",
                "whisper",
                true,
            ),
            (
                "someone/custom-asr",
                "automatic-speech-recognition",
                "my_asr",
                false,
            ),
        ] {
            let repo = parse_search_item(&item(id, task, model_type)).unwrap();
            let want = if runs { &python } else { &unsupported };
            assert_eq!(&repo.compatibility, want, "{id}");
        }
    }

    /// Order for a safetensors chat repo: GGUF (llama.cpp), then mistral.rs, then the Python
    /// engine. mistral.rs is the user's own build, so without it the Python engine's
    /// transformers runs the repo; the Python engine alone takes what mistral.rs cannot.
    #[test]
    fn safetensors_chat_repos_fall_back_to_the_python_engine() {
        use crate::hub_compat::{Compatibility, ModelFormat};
        let repo = |id: &str, task: &str, arch: &str, extra: &[&str]| {
            let mut siblings = vec![
                serde_json::json!({"rfilename": "config.json"}),
                serde_json::json!({"rfilename": "model.safetensors"}),
            ];
            siblings.extend(extra.iter().map(|f| serde_json::json!({"rfilename": f})));
            parse_search_item(&serde_json::json!({
                "id": id,
                "pipeline_tag": task,
                "library_name": "transformers",
                "tags": ["transformers", "safetensors", task],
                "config": {"architectures": [arch]},
                "siblings": siblings,
            }))
            .unwrap()
        };
        let runnable = |runtime: &str| Compatibility::Runnable {
            runtime: runtime.into(),
        };
        // mistral.rs first: it answers the architectures it loads, and keeps them when registered.
        let mut qwen = repo(
            "Qwen/Qwen2.5-0.5B-Instruct",
            "text-generation",
            "Qwen2ForCausalLM",
            &[],
        );
        assert_eq!(qwen.format, ModelFormat::Safetensors);
        assert_eq!(qwen.compatibility, runnable("mistral.rs"));
        assert!(qwen.python_chat);
        let mut with_mistral = qwen.clone();
        with_mistral.resolve_installed(&["mistral.rs".to_string(), "python-hf".to_string()]);
        assert_eq!(with_mistral.compatibility, runnable("mistral.rs"));
        // Without it the Python engine takes over: installed, or to be installed.
        let mut python = qwen.clone();
        python.resolve_installed(&["python-hf".to_string()]);
        assert_eq!(python.compatibility, runnable("python-hf"));
        qwen.resolve_installed(&["llama.cpp".to_string()]);
        assert_eq!(
            qwen.compatibility,
            Compatibility::NeedsRuntime {
                runtime: "python-hf".into()
            }
        );
        assert_eq!(qwen.not_runnable, None);
        // A vision chat model too.
        let vlm = repo(
            "HuggingFaceTB/SmolVLM-256M-Instruct",
            "image-text-to-text",
            "Idefics3ForConditionalGeneration",
            &[],
        );
        assert!(vlm.python_chat);
        assert!(vlm
            .capabilities
            .contains(&crate::model_registry::Capability::Vision));
        // An architecture mistral.rs does not load is the Python engine's from the start.
        let gpt2 = repo(
            "openai-community/gpt2",
            "text-generation",
            "GPT2LMHeadModel",
            &[],
        );
        assert_eq!(gpt2.compatibility, runnable("python-hf"));
        assert_eq!(gpt2.not_runnable, None);
        // A GGUF next to the weights stays llama.cpp's.
        let both = repo(
            "x/Qwen2.5-0.5B-Instruct-GGUF",
            "text-generation",
            "Qwen2ForCausalLM",
            &["qwen2.5-0.5b-instruct-q4_k_m.gguf"],
        );
        assert_eq!(both.format, ModelFormat::Gguf);
        assert_eq!(both.compatibility, runnable("llama.cpp"));
        assert!(!both.python_chat);
        // Quantized weights need packages the environment does not ship: not offered.
        let awq = parse_search_item(&serde_json::json!({
            "id": "x/Qwen2.5-0.5B-Instruct-AWQ",
            "pipeline_tag": "text-generation",
            "library_name": "transformers",
            "tags": ["transformers", "safetensors", "text-generation"],
            "config": {"architectures": ["Qwen2ForCausalLM"], "quantization_config": {"quant_method": "awq"}},
            "siblings": [{"rfilename": "config.json"}, {"rfilename": "model.safetensors"}],
        }))
        .unwrap();
        assert!(!awq.python_chat);
        // A repo that is not a chat model keeps its own verdict.
        let bert = repo("x/bert", "fill-mask", "BertForMaskedLM", &[]);
        assert_eq!(bert.compatibility, runnable("python-hf"));
        assert!(!bert.python_chat);
    }

    #[test]
    fn safetensors_chat_repos_need_the_mistralrs_runtime() {
        use crate::hub_compat::{resolve_installed, Compatibility, ModelFormat};
        let items: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../tests/data/hf-safetensors-llm-listing.json"
        ))
        .unwrap();
        let repos: Vec<HfModelSearchResult> = items
            .iter()
            .map(|i| parse_search_item(i).unwrap())
            .collect();
        let mistral = Compatibility::Runnable {
            runtime: "mistral.rs".into(),
        };
        // A chat model and a VLM in safetensors: runnable by mistral.rs, not "no GGUF".
        for r in &repos[..2] {
            assert_eq!(r.format, ModelFormat::Safetensors, "{}", r.id);
            assert_eq!(r.compatibility, mistral, "{}", r.id);
            assert_eq!(r.not_runnable, None, "{}", r.id);
            let mut missing = r.compatibility.clone();
            resolve_installed(&mut missing, &["llama.cpp".to_string()]);
            assert_eq!(
                missing,
                Compatibility::NeedsRuntime {
                    runtime: "mistral.rs".into()
                }
            );
            let mut installed = r.compatibility.clone();
            resolve_installed(&mut installed, &["mistral.rs".to_string()]);
            assert_eq!(installed, mistral);
        }
        assert!(repos[1]
            .capabilities
            .contains(&crate::model_registry::Capability::Vision));
        // GGUF wins; an embedder and weights in a sub-folder are not chat checkpoints (a
        // safetensors-only embedder runs on the Python engine).
        assert_eq!(
            repos[2].compatibility,
            Compatibility::Runnable {
                runtime: "llama.cpp".into()
            }
        );
        assert_eq!(
            repos[3].compatibility,
            Compatibility::Runnable {
                runtime: "python-hf".into()
            }
        );
        assert!(matches!(
            repos[4].compatibility,
            Compatibility::Unsupported { .. }
        ));
    }

    #[test]
    fn a_safetensors_repo_downloads_as_one_folder_variant() {
        let file =
            |p: &str, size: u64| serde_json::json!({"type": "file", "path": p, "size": size});
        let lfs = |p: &str, size: u64, oid: &str| serde_json::json!({"type": "file", "path": p, "size": 100, "lfs": {"size": size, "oid": oid}});
        let oid = "a".repeat(64);
        let items = vec![
            file(".gitattributes", 1),
            file("README.md", 5),
            file("config.json", 700),
            file("generation_config.json", 200),
            file("tokenizer.json", 7_000_000),
            file("tokenizer_config.json", 7_000),
            file("chat_template.jinja", 2_000),
            file("merges.txt", 1_600_000),
            file("vocab.json", 2_700_000),
            file("model.safetensors.index.json", 30_000),
            lfs("model-00001-of-00002.safetensors", 4_000_000_000, &oid),
            lfs("model-00002-of-00002.safetensors", 3_000_000_000, &oid),
            file("figures/benchmark.png", 50_000),
            file("original/consolidated.safetensors", 8_000_000_000),
        ];
        let v = safetensors_variants("Qwen/Qwen2.5-7B-Instruct", &items).unwrap();
        assert_eq!(v.len(), 1);
        let v = &v[0];
        assert_eq!(v.name, "Qwen2.5-7B-Instruct");
        assert_eq!(v.quantization, "SAFETENSORS");
        let names: Vec<&str> = v.shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            names,
            [
                "model-00001-of-00002.safetensors",
                "model-00002-of-00002.safetensors",
                "README.md",
                "chat_template.jinja",
                "config.json",
                "generation_config.json",
                "merges.txt",
                "model.safetensors.index.json",
                "tokenizer.json",
                "tokenizer_config.json",
                "vocab.json",
            ]
        );
        assert_eq!(v.shards[0].sha256.as_deref(), Some(oid.as_str()));
        assert_eq!(v.shards[0].size_bytes, 4_000_000_000);
        assert!(v.shards.iter().all(|s| s.total_shards == 11));
        assert!(v.shards[0]
            .download_url
            .ends_with("Qwen/Qwen2.5-7B-Instruct/resolve/main/model-00001-of-00002.safetensors"));
        assert!(!v.is_split);
        // A Mistral repo's root `consolidated.safetensors` next to the shards is left out.
        let mistral = vec![
            file("config.json", 700),
            file("tokenizer.json", 7),
            lfs("consolidated.safetensors", 14, &oid),
            lfs("model-00001-of-00001.safetensors", 14, &oid),
        ];
        let v = safetensors_variants("mistralai/x", &mistral).unwrap();
        assert!(!v[0]
            .shards
            .iter()
            .any(|s| s.filename.starts_with("consolidated")));
        // No root config (or no weights): not a checkpoint folder.
        assert!(safetensors_variants("a/b", &[file("model.safetensors", 1)])
            .unwrap()
            .is_empty());
        assert!(safetensors_variants("a/b", &[file("config.json", 1)])
            .unwrap()
            .is_empty());
        // The companions pass the download check as text files.
        assert!(is_safetensors_repo_file("chat_template.jinja"));
        assert!(is_safetensors_repo_file("tokenizer.model"));
        assert!(is_safetensors_repo_file("spm_char.model"));
        assert!(!is_safetensors_repo_file("sub/spm_char.model"));
        // The model card names the task of a Python engine model.
        assert!(is_safetensors_repo_file("README.md"));
        assert!(!is_safetensors_repo_file("onnx/model.onnx"));
        assert!(!is_safetensors_repo_file("figures/benchmark.png"));
    }

    #[test]
    fn diffusers_variant_takes_one_set_of_component_weights() {
        let file =
            |p: &str, size: u64| serde_json::json!({"type": "file", "path": p, "size": size});
        let names = |v: &[HfModelVariant]| -> Vec<String> {
            v[0].shards.iter().map(|s| s.filename.clone()).collect()
        };
        // segmind/tiny-sd: PyTorch weights only, so the .bin files come.
        let tiny = vec![
            file("README.md", 1),
            file("grid_tiny.png", 1),
            file("model_index.json", 1),
            file("scheduler/scheduler_config.json", 1),
            file("text_encoder/config.json", 1),
            file("text_encoder/pytorch_model.bin", 246),
            file("tokenizer/merges.txt", 1),
            file("tokenizer/special_tokens_map.json", 1),
            file("tokenizer/tokenizer_config.json", 1),
            file("tokenizer/vocab.json", 1),
            file("unet/config.json", 1),
            file("unet/diffusion_pytorch_model.bin", 647),
            file("vae/config.json", 1),
            file("vae/diffusion_pytorch_model.bin", 167),
        ];
        let v = diffusers_variants("segmind/tiny-sd", &tiny).unwrap();
        assert_eq!(v[0].quantization, "DIFFUSERS");
        assert_eq!(v[0].total_size_bytes, 246 + 647 + 167 + 9);
        assert_eq!(
            names(&v),
            [
                "text_encoder/pytorch_model.bin",
                "unet/diffusion_pytorch_model.bin",
                "vae/diffusion_pytorch_model.bin",
                "model_index.json",
                "scheduler/scheduler_config.json",
                "text_encoder/config.json",
                "tokenizer/merges.txt",
                "tokenizer/special_tokens_map.json",
                "tokenizer/tokenizer_config.json",
                "tokenizer/vocab.json",
                "unet/config.json",
                "vae/config.json",
            ]
        );
        // Every component has fp16 safetensors: only those, no .bin, non_ema, ONNX or root files.
        let mut sd = vec![
            file("model_index.json", 1),
            file("v1-5-pruned.safetensors", 7703),
            file("feature_extractor/preprocessor_config.json", 1),
            file("onnx/unet/model.onnx", 3000),
            file("unet/config.json", 1),
            file("unet/diffusion_pytorch_model.bin", 3438),
            file("unet/diffusion_pytorch_model.fp16.bin", 1719),
            file("unet/diffusion_pytorch_model.fp16.safetensors", 1719),
            file("unet/diffusion_pytorch_model.non_ema.safetensors", 3438),
            file("unet/diffusion_pytorch_model.safetensors", 3438),
            file("vae/config.json", 1),
            file("vae/diffusion_pytorch_model.fp16.safetensors", 167),
            file("vae/diffusion_pytorch_model.safetensors", 334),
            file("vae/diffusion_flax_model.msgpack", 334),
        ];
        let v = diffusers_variants("a/sd", &sd).unwrap();
        assert_eq!(v[0].quantization, "FP16");
        assert_eq!(
            names(&v),
            [
                "unet/diffusion_pytorch_model.fp16.safetensors",
                "vae/diffusion_pytorch_model.fp16.safetensors",
                "feature_extractor/preprocessor_config.json",
                "model_index.json",
                "unet/config.json",
                "vae/config.json",
            ]
        );
        // A component without fp16: the plain safetensors for all.
        sd.retain(|f| f["path"] != "vae/diffusion_pytorch_model.fp16.safetensors");
        let v = diffusers_variants("a/sd", &sd).unwrap();
        assert!(names(&v).contains(&"unet/diffusion_pytorch_model.safetensors".to_string()));
        assert!(names(&v).contains(&"vae/diffusion_pytorch_model.safetensors".to_string()));
        assert!(!names(&v)
            .iter()
            .any(|n| n.contains("fp16") || n.ends_with(".bin")));
        // Sharded weights keep the index of their variant.
        let sharded = vec![
            file("model_index.json", 1),
            file("transformer/config.json", 1),
            file(
                "transformer/diffusion_pytorch_model-00001-of-00002.safetensors",
                5,
            ),
            file(
                "transformer/diffusion_pytorch_model-00002-of-00002.safetensors",
                5,
            ),
            file(
                "transformer/diffusion_pytorch_model.safetensors.index.json",
                1,
            ),
            file("transformer/diffusion_pytorch_model.bin.index.json", 1),
        ];
        let v = diffusers_variants("a/flux", &sharded).unwrap();
        let n = names(&v);
        assert!(
            n.contains(&"transformer/diffusion_pytorch_model.safetensors.index.json".to_string())
        );
        assert!(!n.contains(&"transformer/diffusion_pytorch_model.bin.index.json".to_string()));
        assert_eq!(n.iter().filter(|n| n.ends_with(".safetensors")).count(), 2);
        // No model_index.json: not a pipeline.
        let no_index: Vec<_> = tiny
            .iter()
            .filter(|f| f["path"] != "model_index.json")
            .cloned()
            .collect();
        assert!(diffusers_variants("a/b", &no_index).unwrap().is_empty());
        assert_eq!(
            diffusers_weight_variant("model.fp16-00001-of-00002.safetensors").as_deref(),
            Some("fp16")
        );
        assert_eq!(
            diffusers_weight_variant("diffusion_pytorch_model-00001-of-00002.safetensors"),
            None
        );
    }

    #[test]
    fn checkpoint_variant_of_a_pytorch_only_and_a_sentence_transformers_repo() {
        let file =
            |p: &str, size: u64| serde_json::json!({"type": "file", "path": p, "size": size});
        let lfs = |p: &str, size: u64, oid: &str| serde_json::json!({"type": "file", "path": p, "size": 100, "lfs": {"size": size, "oid": oid}});
        let oid = "b".repeat(64);
        // PyTorch weights are taken only when the repo has no safetensors.
        let bin_only = vec![
            file("config.json", 700),
            file("vocab.txt", 200_000),
            lfs("pytorch_model.bin", 400_000_000, &oid),
            lfs("tf_model.h5", 400_000_000, &oid),
            lfs("flax_model.msgpack", 400_000_000, &oid),
        ];
        let v = safetensors_variants("dandelin/vilt-b32-finetuned-vqa", &bin_only).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(names, ["pytorch_model.bin", "config.json", "vocab.txt"]);
        let both = vec![
            file("config.json", 700),
            lfs("model.safetensors", 4, &oid),
            lfs("pytorch_model.bin", 4, &oid),
        ];
        let v = safetensors_variants("a/b", &both).unwrap();
        assert!(!v[0].shards.iter().any(|s| s.filename.ends_with(".bin")));
        // A repo with its own Whisper processor needs no processor companion.
        assert!(own_whisper_processor(&[
            "config.json",
            "preprocessor_config.json",
            "tokenizer.json"
        ]));
        assert!(own_whisper_processor(&[
            "preprocessor_config.json",
            "vocab.json"
        ]));
        assert!(!own_whisper_processor(&["config.json", "weights.npz"]));
        // The plan stands when the listing fails, and goes when the root has a processor.
        let libraries = [std::path::PathBuf::from("/lib")];
        let plan = || {
            companion_plan(
                "/lib/mlx-community/whisper-small-mlx/weights.npz",
                Some("mlx-community/whisper-small-mlx"),
                &libraries,
            )
            .expect("a processor plan")
        };
        assert_eq!(plan().bundle_id, crate::engine::mlx::WHISPER_BUNDLE);
        let listed = |names: &[&str]| -> Result<Vec<serde_json::Value>, String> {
            Ok(names
                .iter()
                .map(|n| serde_json::json!({"type": "file", "path": n}))
                .collect())
        };
        assert!(checked_whisper_plan(plan(), Err("offline".into())).is_some());
        assert!(checked_whisper_plan(plan(), listed(&["config.json", "weights.npz"])).is_some());
        assert!(checked_whisper_plan(
            plan(),
            listed(&["config.json", "preprocessor_config.json", "tokenizer.json"])
        )
        .is_none());
        // A processor in a subfolder is not the model's own.
        assert!(checked_whisper_plan(
            plan(),
            listed(&[
                "weights.npz",
                "x/preprocessor_config.json",
                "x/tokenizer.json"
            ])
        )
        .is_some());
        // mlx-examples' Whisper conversion: `weights.npz` is its weights.
        let npz = vec![
            file("config.json", 266),
            lfs("weights.npz", 481_307_592, &oid),
            file("README.md", 343),
        ];
        let v = safetensors_variants("mlx-community/whisper-small-mlx", &npz).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(names, ["weights.npz", "README.md", "config.json"]);
        // Alternative weight sets of one model come down once: openai/whisper-large-v3 (fp32
        // shards beside model.safetensors) and sesame/csm-1b (transformers shards beside it).
        let whisper = vec![
            file("config.json", 1272),
            file("generation_config.json", 3903),
            file("preprocessor_config.json", 340),
            file("tokenizer.json", 2_480_617),
            file("model.safetensors.index.fp32.json", 117_893),
            lfs("flax_model.msgpack", 6_174_007_324, &oid),
            lfs("model.fp32-00001-of-00002.safetensors", 4_993_448_880, &oid),
            lfs("model.fp32-00002-of-00002.safetensors", 1_180_663_192, &oid),
            lfs("model.safetensors", 3_087_130_976, &oid),
            lfs("pytorch_model.bin", 3_087_394_553, &oid),
            lfs("pytorch_model.fp32-00001-of-00002.bin", 4_993_677_094, &oid),
        ];
        let v = safetensors_variants("openai/whisper-large-v3", &whisper).unwrap();
        assert!(v[0]
            .shards
            .iter()
            .any(|s| s.filename == "model.safetensors"));
        assert!(!v[0].shards.iter().any(|s| s.filename.contains("fp32")));
        assert!(v[0].total_size_bytes < 3_100_000_000);
        let csm = vec![
            file("config.json", 3280),
            file("chat_template.jinja", 2002),
            lfs("ckpt.pt", 6_219_618_714, &oid),
            lfs("model.safetensors", 6_211_186_784, &oid),
            lfs(
                "transformers-00001-of-00002.safetensors",
                4_944_026_784,
                &oid,
            ),
            lfs(
                "transformers-00002-of-00002.safetensors",
                2_189_474_180,
                &oid,
            ),
            file("transformers.safetensors.index.json", 59_730),
            lfs("prompts/conversational_a.wav", 2_646_044, &oid),
        ];
        let v = safetensors_variants("sesame/csm-1b", &csm).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            names,
            ["model.safetensors", "chat_template.jinja", "config.json"]
        );
        // A tensor file beside the weights that is no copy of them comes along (Chatterbox
        // Turbo's default voice, `conds.safetensors`).
        let chatterbox = vec![
            file("config.json", 2565),
            file("model.safetensors.index.json", 252_012),
            lfs("model.safetensors", 414_992_166, &oid),
            lfs("conds.safetensors", 164_884, &oid),
            file("tokenizer_config.json", 3878),
        ];
        let v = safetensors_variants("mlx-community/chatterbox-turbo-4bit", &chatterbox).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert!(names.contains(&"conds.safetensors"), "{names:?}");
        assert!(names.contains(&"model.safetensors"), "{names:?}");
        // Any other root tensor file stays on the Hub: a VAE, EMA weights, a precision copy of
        // `weights.safetensors`.
        let others = vec![
            file("config.json", 700),
            lfs("weights.safetensors", 400, &oid),
            lfs("weights.fp16.safetensors", 200, &oid),
        ];
        let v = safetensors_variants("a/b", &others).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(names, ["weights.safetensors", "config.json"]);
        let model_and_others = vec![
            file("config.json", 700),
            lfs("model.safetensors", 400, &oid),
            lfs("vae.safetensors", 100, &oid),
            lfs("ema.safetensors", 400, &oid),
        ];
        let v = safetensors_variants("a/b", &model_and_others).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(names, ["model.safetensors", "config.json"]);
        // A sharded model keeps its shards and drops a precision copy.
        let sharded = vec![
            file("config.json", 700),
            file("model.safetensors.index.json", 900),
            lfs("model-00001-of-00002.safetensors", 4, &oid),
            lfs("model-00002-of-00002.safetensors", 4, &oid),
            lfs("model.fp16.safetensors", 4, &oid),
        ];
        let v = safetensors_variants("a/b", &sharded).unwrap();
        assert_eq!(v[0].shards.len(), 4);
        assert!(!v[0].shards.iter().any(|s| s.filename.contains("fp16")));
        // sentence-transformers modules come along; ONNX and OpenVINO exports do not.
        let st = vec![
            file("config.json", 700),
            file("modules.json", 300),
            file("sentence_bert_config.json", 50),
            file("1_Pooling/config.json", 200),
            lfs("2_Dense/model.safetensors", 10, &oid),
            file("2_Dense/config.json", 100),
            lfs("model.safetensors", 90_000_000, &oid),
            lfs("onnx/model.onnx", 90_000_000, &oid),
            lfs("openvino/openvino_model.bin", 90_000_000, &oid),
        ];
        let v = safetensors_variants("sentence-transformers/x", &st).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            names,
            [
                "model.safetensors",
                "1_Pooling/config.json",
                "2_Dense/config.json",
                "2_Dense/model.safetensors",
                "config.json",
                "modules.json",
                "sentence_bert_config.json",
            ]
        );
        // Qwen3-TTS: its speech tokenizer folder (the codec) comes with the model.
        let qwen_tts = vec![
            file("config.json", 5_000),
            lfs("model.safetensors", 2_000, &oid),
            file("model.safetensors.index.json", 78),
            file("speech_tokenizer/config.json", 2_336),
            file("speech_tokenizer/configuration.json", 76),
            lfs("speech_tokenizer/model.safetensors", 680, &oid),
            file("speech_tokenizer/preprocessor_config.json", 234),
            file("other/model.safetensors", 1),
            file("speech_tokenizer/sub/x.json", 1),
            file("speech_tokenizer/../x.json", 1),
            file("speech_tokenizer\\sub\\x.json", 1),
        ];
        let v =
            safetensors_variants("mlx-community/Qwen3-TTS-12Hz-0.6B-Base-8bit", &qwen_tts).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            names,
            [
                "model.safetensors",
                "config.json",
                "model.safetensors.index.json",
                "speech_tokenizer/config.json",
                "speech_tokenizer/configuration.json",
                "speech_tokenizer/model.safetensors",
                "speech_tokenizer/preprocessor_config.json",
            ]
        );
        // Pruning a root precision copy leaves module weights in their folders alone.
        let st_fp16 = vec![
            file("config.json", 700),
            file("modules.json", 300),
            lfs("model.safetensors", 90, &oid),
            lfs("model.fp16.safetensors", 45, &oid),
            lfs("2_Dense/model.safetensors", 10, &oid),
            lfs("3_Dense/model.safetensors", 10, &oid),
        ];
        let v = safetensors_variants("sentence-transformers/y", &st_fp16).unwrap();
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            names,
            [
                "model.safetensors",
                "2_Dense/model.safetensors",
                "3_Dense/model.safetensors",
                "config.json",
                "modules.json",
            ]
        );
    }

    /// Real Hugging Face: with `include_non_gguf` a safetensors repo is listed and classified
    /// unsupported, `ggerganov/whisper.cpp` as whisper ggml.
    /// `cargo test -p harness-core --lib real_non_gguf_listing -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn real_non_gguf_listing() {
        use crate::hub_compat::{Compatibility, ModelFormat};
        let bert = HfSearchOptions {
            query: "bert-base-uncased".into(),
            author: Some("google-bert".into()),
            include_non_gguf: Some(true),
            limit: Some(5),
            ..Default::default()
        };
        let page = DownloadManager::search_models_page(&bert, None)
            .await
            .unwrap();
        let item = page
            .items
            .iter()
            .find(|m| m.id == "google-bert/bert-base-uncased")
            .expect("bert-base-uncased listed");
        println!("{} {:?} {:?}", item.id, item.format, item.compatibility);
        assert!(matches!(
            item.compatibility,
            Compatibility::Unsupported { .. }
        ));
        let whisper = HfSearchOptions {
            query: "whisper.cpp".into(),
            author: Some("ggerganov".into()),
            include_non_gguf: Some(true),
            ..Default::default()
        };
        let page = DownloadManager::search_models_page(&whisper, None)
            .await
            .unwrap();
        let item = page
            .items
            .iter()
            .find(|m| m.id == WHISPER_CPP_REPO)
            .expect("whisper.cpp listed");
        println!("{} {:?} {:?}", item.id, item.format, item.compatibility);
        assert_eq!(item.format, ModelFormat::WhisperGgml);
    }

    #[test]
    fn custom_url_name_and_file_magic_agree() {
        // The file saved under the chosen name passes the check that runs after the download.
        let dir = tempfile::tempdir().unwrap();
        let head = [0x6c, 0x6d, 0x67, 0x67, 0, 0, 0, 0];
        let name = custom_download_name(Some("tiny"), Some(head[..4].try_into().unwrap())).unwrap();
        let path = dir.path().join(&name);
        fs::write(&path, head).unwrap();
        assert!(verify_model_magic(&path, &name).is_ok());
        assert!(crate::engine::is_ggml_file(&path));
    }

    #[test]
    fn whisper_repo_is_listed_for_speech_searches() {
        let opts = |query: &str, task: Option<&str>| HfSearchOptions {
            query: query.into(),
            pipeline_tag: task.map(String::from),
            ..Default::default()
        };
        // The search of the user's report, and the speech task of the picker.
        assert!(wants_whisper_repo(&opts("whisper.cpp", None)));
        assert!(wants_whisper_repo(&opts("Whisper large v3", None)));
        assert!(wants_whisper_repo(&opts("ggml", None)));
        assert!(wants_whisper_repo(&opts(
            "",
            Some("automatic-speech-recognition")
        )));
        assert!(wants_whisper_repo(&opts(
            "whisper",
            Some("automatic-speech-recognition")
        )));
        // Other searches and tasks are not touched.
        assert!(!wants_whisper_repo(&opts("", None)));
        assert!(!wants_whisper_repo(&opts("gemma", None)));
        assert!(!wants_whisper_repo(&opts(
            "gemma",
            Some("automatic-speech-recognition")
        )));
        assert!(!wants_whisper_repo(&opts(
            "whisper",
            Some("text-generation")
        )));
        // Filters that exclude the repo, and later pages.
        let mut o = opts("whisper", None);
        o.author = Some("Qwen".into());
        assert!(!wants_whisper_repo(&o));
        o.author = Some(" ggerganov ".into());
        o.license = Some("apache-2.0".into());
        assert!(!wants_whisper_repo(&o));
        o.license = Some("mit".into());
        assert!(wants_whisper_repo(&o));
        o.cursor = Some("eyJhIjoxfQ==".into());
        assert!(!wants_whisper_repo(&o));
    }

    /// The real HF listing item of `ggerganov/whisper.cpp` (no GGUF file in it): it is a speech
    /// repo Harness can run, with the f16 / quantized `ggml-*.bin` models as quants.
    #[test]
    fn whisper_repo_listing_item_is_runnable() {
        let item: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/hf-whisper-cpp-listing.json"))
                .unwrap();
        let repo = parse_search_item(&item).unwrap();
        assert_eq!(repo.id, WHISPER_CPP_REPO);
        assert_eq!(
            repo.pipeline_tag.as_deref(),
            Some("automatic-speech-recognition")
        );
        assert!(repo.not_runnable.is_none(), "{:?}", repo.not_runnable);
        assert_eq!(repo.license.as_deref(), Some("mit"));
        for q in ["F16", "Q5_0", "Q5_1", "Q8_0"] {
            assert!(
                repo.quants.iter().any(|x| x == q),
                "{q} in {:?}",
                repo.quants
            );
        }
    }

    #[test]
    fn test_custom_url_ssrf_rules() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "fc00::1",
            "fe80::1",
            "::",
        ] {
            assert!(is_blocked_ip(ip.parse().unwrap()), "{} engellenmeliydi", ip);
        }
        assert!(!is_blocked_ip("93.184.216.34".parse().unwrap()));
        let http = reqwest::Url::parse("http://example.com/model.gguf").unwrap();
        assert!(validate_custom_url_structure(&http).is_err());
        let https = reqwest::Url::parse("https://example.com/model.gguf").unwrap();
        assert!(validate_custom_url_structure(&https).is_ok());
        assert!(redirect_target(&https, "http://127.0.0.1/model.gguf").is_err());
    }

    #[tokio::test]
    async fn test_existing_sha_mismatch_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        tokio::fs::write(&path, b"GGUFbad").await.unwrap();
        let item = DownloadItem {
            url: "https://example.com/model.gguf".into(),
            filename: "model.gguf".into(),
            size: 7,
            shard_index: 1,
            sha256: Some("00".repeat(32)),
            custom_url: false,
        };
        assert_eq!(
            existing_target_is_complete(&path, &item).await.unwrap(),
            None
        );
        let preserved = preserve_corrupt_file(&path).await.unwrap();
        assert_eq!(preserved.file_name().unwrap(), "model.gguf.corrupt");
        assert_eq!(tokio::fs::read(preserved).await.unwrap(), b"GGUFbad");
        assert!(!tokio::fs::try_exists(path).await.unwrap());
    }

    #[tokio::test]
    async fn test_reset_part_uses_async_io() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("model.part");
        tokio::fs::write(&part, b"partial").await.unwrap();
        tokio::fs::write(part_metadata_path(&part), b"{}")
            .await
            .unwrap();
        reset_part(&part).await.unwrap();
        assert_eq!(tokio::fs::metadata(&part).await.unwrap().len(), 0);
        assert!(!tokio::fs::try_exists(part_metadata_path(&part))
            .await
            .unwrap());
    }

    #[test]
    fn test_store_replaces_and_cleans_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let store = crate::store::JsonStore::new(&path);
        store.write_bytes(b"one").unwrap();
        store.write_bytes(b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_parse_next_link() {
        let h = r#"<https://huggingface.co/api/models/a/b/tree/main?recursive=true&cursor=xyz>; rel="next""#;
        assert_eq!(
            parse_next_link(h).as_deref(),
            Some("https://huggingface.co/api/models/a/b/tree/main?recursive=true&cursor=xyz")
        );
        assert_eq!(parse_next_link(r#"<https://x>; rel="prev""#), None);
    }

    #[test]
    fn test_strip_front_matter_and_license() {
        let md = "---\nlicense: mit\ntags: [a]\n---\n\n# Title\nBody";
        assert_eq!(strip_front_matter(md), "# Title\nBody");
        assert_eq!(strip_front_matter("# No front matter"), "# No front matter");
        assert_eq!(
            license_from_tags(&["gguf".into(), "license:apache-2.0".into()]).as_deref(),
            Some("apache-2.0")
        );
        assert!(simple_token("text-generation"));
        assert!(!simple_token("a&b=c"));
    }

    #[test]
    fn test_queue_lifecycle_without_network() {
        let mgr = DownloadManager::default();
        let job = |name: &str| Job {
            progress: DownloadProgress {
                id: 0,
                repo_id: "a/b".into(),
                variant_name: name.into(),
                total_bytes: 1,
                downloaded_bytes: 0,
                current_shard_name: String::new(),
                current_shard_index: 0,
                total_shards: 1,
                speed_bytes_per_sec: 0,
                status: DownloadStatus::Paused,
                created_at: 0,
            },
            items: vec![],
            target_dir: PathBuf::from("x"),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        {
            let mut jobs = mgr.jobs.lock().unwrap();
            for (i, n) in ["one", "two", "three"].iter().enumerate() {
                let mut j = job(n);
                j.progress.id = i as u64 + 1;
                j.progress.status = DownloadStatus::Queued;
                jobs.push(j);
            }
        }
        mgr.prioritize(3);
        assert_eq!(mgr.list()[0].variant_name, "three");
        mgr.cancel_download(Some(3));
        assert_eq!(mgr.list()[0].status, DownloadStatus::Paused);
        mgr.remove_download(3, false).unwrap();
        assert_eq!(mgr.list().len(), 2);
        assert!(mgr.remove_download(99, false).is_err());
    }

    #[test]
    fn test_sha256_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn search_params_sort_changes_only_order_for_every_filter_and_cursor() {
        for include_non_gguf in [false, true] {
            for task in [
                None,
                Some("text-to-image"),
                Some(crate::hub_tasks::DECISION_TASK),
            ] {
                for cursor in [None, Some("eyJhIjoxfQ==")] {
                    let mut baseline = None;
                    for sort in SORT_KEYS {
                        let params = search_params(&HfSearchOptions {
                            query: " sd-turbo ".into(),
                            pipeline_tag: task.map(String::from),
                            license: Some("mit".into()),
                            author: Some(" stabilityai ".into()),
                            include_non_gguf: Some(include_non_gguf),
                            sort: Some((*sort).into()),
                            limit: Some(30),
                            cursor: cursor.map(String::from),
                            ..Default::default()
                        });
                        assert!(params.contains(&("sort", (*sort).into())));
                        assert!(params.contains(&("direction", "-1".into())));
                        assert!(params.contains(&("search", "sd-turbo".into())));
                        let filters: Vec<_> = params
                            .into_iter()
                            .filter(|(key, _)| *key != "sort")
                            .collect();
                        if let Some(expected) = &baseline {
                            assert_eq!(&filters, expected);
                        } else {
                            baseline = Some(filters);
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    #[ignore] // network: compare every page with HF, including ties and page boundaries
    async fn real_hf_sort_parity() {
        let mut sets = Vec::new();
        for sort in ["downloads", "likes"] {
            let mut options = HfSearchOptions {
                query: "sd-turbo".into(),
                sort: Some(sort.into()),
                include_non_gguf: Some(true),
                limit: Some(30),
                ..Default::default()
            };
            let mut ids = std::collections::BTreeSet::new();
            loop {
                let (raw, next) = DownloadManager::fetch_listing(&options, None)
                    .await
                    .unwrap();
                let page = DownloadManager::search_models_page(&options, None)
                    .await
                    .unwrap();
                assert_eq!(
                    page.items.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
                    raw.iter()
                        .map(|m| m["id"].as_str().unwrap())
                        .collect::<Vec<_>>()
                );
                // HF includes a per-request searchSequenceToken in otherwise equivalent
                // cursors. Compare the actual boundary, not that ephemeral session token.
                let boundary = |cursor: &Option<String>| {
                    use base64::Engine;
                    cursor.as_ref().map(|cursor| {
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(cursor)
                            .unwrap();
                        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        value.as_object_mut().unwrap().remove("searchSequenceToken");
                        value
                    })
                };
                assert_eq!(boundary(&page.next_cursor), boundary(&next));
                for item in page.items {
                    assert!(ids.insert(item.id), "duplicate across pages");
                }
                options.cursor = next;
                if options.cursor.is_none() {
                    break;
                }
            }
            assert!(!ids.is_empty());
            sets.push(ids);
        }
        assert_eq!(sets[0], sets[1]);
    }

    #[test]
    fn search_params_list_by_filters_without_query() {
        let opts = HfSearchOptions {
            query: "  ".into(),
            pipeline_tag: Some("feature-extraction".into()),
            license: Some("apache-2.0".into()),
            author: Some(" Qwen ".into()),
            sort: Some("trendingScore".into()),
            include_untagged: None,
            include_non_gguf: None,
            limit: Some(8),
            cursor: Some("eyJhIjoxfQ==".into()),
        };
        let p = search_params(&opts);
        let get = |k: &str| {
            p.iter()
                .filter(|(n, _)| *n == k)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert!(get("search").is_empty(), "{p:?}");
        assert_eq!(get("filter"), ["gguf", "license:apache-2.0"]);
        assert_eq!(get("sort"), ["trendingScore"]);
        assert_eq!(get("direction"), ["-1"]);
        assert_eq!(get("limit"), ["8"]);
        assert_eq!(get("pipeline_tag"), ["feature-extraction"]);
        assert_eq!(get("author"), ["Qwen"]);
        assert_eq!(get("cursor"), ["eyJhIjoxfQ=="]);
    }

    /// Decision models are listed by their `decision-model` tag (HF has no such
    /// `pipeline_tag`): `filter=gguf&filter=decision-model`.
    #[test]
    fn decision_models_are_listed_by_their_tag() {
        let opts = HfSearchOptions {
            pipeline_tag: Some(crate::hub_tasks::DECISION_TASK.into()),
            ..Default::default()
        };
        let p = search_params(&opts);
        let get = |k: &str| {
            p.iter()
                .filter(|(n, _)| *n == k)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("filter"), ["gguf", "decision-model"]);
        assert!(get("pipeline_tag").is_empty(), "{p:?}");
    }

    /// A GGUF decision model runs on llama-server whatever its `pipeline_tag` says (listing items
    /// of ggml-org/Kev-4B-GGUF and ggml-org/Bespoke-Nimble-9B-v3-GGUF); the same tags without the
    /// decision tag stay another engine's.
    #[test]
    fn decision_gguf_repos_are_runnable_whatever_their_pipeline_tag() {
        for task in ["text-classification", "zero-shot-classification"] {
            let item = |tags: serde_json::Value| {
                serde_json::json!({"id": "ggml-org/Kev-4B-GGUF", "pipeline_tag": task, "tags": tags,
                    "siblings": [{"rfilename": "Kev-4B-Q8_0.gguf"}]})
            };
            let repo =
                parse_search_item(&item(serde_json::json!(["gguf", "decision-model"]))).unwrap();
            assert_eq!(
                repo.compatibility,
                crate::hub_compat::ModelFormat::Gguf.compatibility()
            );
            let plain = parse_search_item(&item(serde_json::json!(["gguf"]))).unwrap();
            assert!(matches!(
                plain.compatibility,
                crate::hub_compat::Compatibility::Unsupported { .. }
            ));
        }
    }

    #[test]
    fn search_params_drop_unsafe_values() {
        let opts = HfSearchOptions {
            query: "llama 3".into(),
            pipeline_tag: Some("text-generation&x=1".into()),
            sort: Some("random".into()),
            limit: Some(1000),
            cursor: Some("a&b".into()),
            ..Default::default()
        };
        let p = search_params(&opts);
        assert!(p.contains(&("search", "llama 3".into())));
        assert!(p.contains(&("sort", "downloads".into())));
        assert!(p.contains(&("limit", "100".into())));
        assert!(!p
            .iter()
            .any(|(k, _)| *k == "pipeline_tag" || *k == "cursor"));
    }

    #[test]
    fn merged_cursor_halves_and_sort_keys() {
        assert_eq!(split_cursor(None), (None, None));
        assert_eq!(split_cursor(Some("abc=")), (Some("abc=".into()), None));
        assert_eq!(
            split_cursor(Some("a-b.c_d")),
            (Some("a-b".into()), Some("c_d".into()))
        );
        // An exhausted half stays empty.
        assert_eq!(split_cursor(Some(".c_d")), (None, Some("c_d".into())));
        assert_eq!(split_cursor(Some("a.")), (Some("a".into()), None));
        let big = serde_json::json!({"downloads": 1234, "likes": 7, "createdAt": "2026-01-02T00:00:00.000Z"});
        assert!(sort_key(&big, "downloads") > sort_key(&big, "likes"));
        assert_eq!(sort_key(&big, "createdAt"), "2026-01-02T00:00:00.000Z");
        let small = serde_json::json!({"downloads": 99});
        assert!(sort_key(&big, "downloads") > sort_key(&small, "downloads"));
    }

    #[test]
    fn untagged_repo_gets_an_inferred_task_tagged_one_does_not() {
        let untagged = serde_json::json!({
            "id": "lmstudio-community/Qwen3.8-27B-GGUF",
            "tags": ["gguf", "base_model:quantized:Qwen/Qwen3.8-27B", "conversational"],
            "siblings": [{"rfilename": "mmproj-Qwen3.8-27B-BF16.gguf"}, {"rfilename": "m-Q4_K_M.gguf"}],
        });
        let item = parse_search_item(&untagged).unwrap();
        assert_eq!(item.pipeline_tag, None);
        assert_eq!(item.inferred_task.as_deref(), Some("image-text-to-text"));
        let tagged = serde_json::json!({
            "id": "a/b", "pipeline_tag": "text-generation", "tags": ["gguf", "conversational"],
        });
        let item = parse_search_item(&tagged).unwrap();
        assert_eq!(item.pipeline_tag.as_deref(), Some("text-generation"));
        assert_eq!(item.inferred_task, None);
        // An empty tag counts as none.
        let empty = serde_json::json!({"id": "a/b", "pipeline_tag": "", "tags": ["gguf", "conversational"]});
        assert_eq!(
            parse_search_item(&empty).unwrap().inferred_task.as_deref(),
            Some("text-generation")
        );
    }

    #[test]
    fn listing_asks_for_the_gguf_header_and_library_name() {
        let p = search_params(&HfSearchOptions::default());
        let expand: Vec<&str> = p
            .iter()
            .filter(|(k, _)| *k == "expand[]")
            .map(|(_, v)| v.as_str())
            .collect();
        for f in [
            "gguf",
            "library_name",
            "pipeline_tag",
            "siblings",
            "tags",
            "createdAt",
        ] {
            assert!(expand.contains(&f), "{f}");
        }
        assert!(!p.iter().any(|(k, _)| *k == "full"), "full= excludes gguf");
    }

    #[test]
    fn grab_bag_repo_and_no_gguf_repo_in_a_listing() {
        // Real model JSON (tests/data/hf-repo-signals.json).
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/hf-repo-signals.json")).unwrap();
        let moved = parse_search_item(&all["ggml-org/models-moved"]).unwrap();
        assert_eq!(moved.pipeline_tag, None);
        assert_eq!(moved.inferred_task.as_deref(), Some("text-generation"));
        assert_eq!(
            moved.inferred_source,
            Some(crate::hub_tasks::TaskSource::Gguf)
        );
        assert_eq!(moved.not_runnable, None);
        // A safetensors chat model: llama-server cannot run it, mistral.rs can (D6), so it is
        // not "no GGUF" but a repo of the mistral.rs runtime.
        let safetensors = parse_search_item(&all["Qwen/Qwen3-0.6B"]).unwrap();
        assert_eq!(safetensors.not_runnable, None);
        assert_eq!(
            safetensors.compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: "mistral.rs".into()
            }
        );
        assert_eq!(safetensors.inferred_task, None);
    }

    /// Real network: the listing's `expand[]` fields reach the parser, so an untagged repo
    /// (`ggml-org/models-moved`) gets its task from the GGUF header.
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_listing_reads_gguf_header_of_untagged_repo() {
        let opts = HfSearchOptions {
            query: "models-moved".into(),
            author: Some("ggml-org".into()),
            limit: Some(5),
            ..Default::default()
        };
        let page = DownloadManager::search_models_page(&opts, None)
            .await
            .unwrap();
        let moved = page
            .items
            .iter()
            .find(|m| m.id == "ggml-org/models-moved")
            .expect("repo in listing");
        assert_eq!(moved.pipeline_tag, None);
        assert_eq!(moved.inferred_task.as_deref(), Some("text-generation"));
        assert_eq!(moved.not_runnable, None);
    }

    /// Real network: with a task filter, repos HF lists without a `pipeline_tag` come through
    /// with an inferred task, tagged repos keep theirs, and the merged cursor pages on.
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_task_filter_includes_untagged_repos() {
        let opts = HfSearchOptions {
            pipeline_tag: Some("text-generation".into()),
            sort: Some("downloads".into()),
            limit: Some(100),
            ..Default::default()
        };
        let with = DownloadManager::search_models_page(&opts, None)
            .await
            .unwrap();
        let without = DownloadManager::search_models_page(
            &HfSearchOptions {
                include_untagged: Some(false),
                ..opts.clone()
            },
            None,
        )
        .await
        .unwrap();
        let untagged: Vec<_> = with
            .items
            .iter()
            .filter(|m| m.pipeline_tag.is_none())
            .collect();
        eprintln!(
            "tagged-only {} items, merged {} items, {} untagged: {:?}",
            without.items.len(),
            with.items.len(),
            untagged.len(),
            untagged.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
        assert!(without
            .items
            .iter()
            .all(|m| m.pipeline_tag.as_deref() == Some("text-generation")));
        assert!(
            !untagged.is_empty(),
            "no untagged text-generation repos in the first 100"
        );
        assert!(untagged
            .iter()
            .all(|m| m.inferred_task.as_deref() == Some("text-generation")));
        assert_eq!(with.items.len(), 100);
        assert!(untagged
            .iter()
            .all(|m| without.items.iter().all(|tagged| tagged.id != m.id)));
        // The merged list is ordered by downloads.
        let dl: Vec<u64> = with.items.iter().map(|m| m.downloads).collect();
        assert!(dl.windows(2).all(|w| w[0] >= w[1]), "{dl:?}");
        // The merged cursor continues the listing with different repos.
        let cursor = with.next_cursor.clone().expect("next cursor");
        assert!(cursor.starts_with("tasks1."));
        let next = DownloadManager::search_models_page(
            &HfSearchOptions {
                cursor: Some(cursor),
                ..opts.clone()
            },
            None,
        )
        .await
        .unwrap();
        let first: std::collections::HashSet<_> = with.items.iter().map(|m| &m.id).collect();
        assert!(!next.items.is_empty());
        assert!(next.items.iter().all(|m| !first.contains(&m.id)));
    }

    /// Real network: embedding / reranker / VLM filters also pick up untagged repos.
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_untagged_embedding_reranker_vlm() {
        for (task, probe) in [
            ("feature-extraction", "embed"),
            ("text-ranking", "rerank"),
            ("image-text-to-text", "vl"),
        ] {
            let page = DownloadManager::search_models_page(
                &HfSearchOptions {
                    pipeline_tag: Some(task.into()),
                    sort: Some("downloads".into()),
                    limit: Some(100),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
            let untagged: Vec<&str> = page
                .items
                .iter()
                .filter(|m| m.pipeline_tag.is_none())
                .map(|m| m.id.as_str())
                .collect();
            eprintln!(
                "{task}: {} items, untagged {:?} (probe {probe})",
                page.items.len(),
                untagged
            );
            assert!(page
                .items
                .iter()
                .all(|m| m.pipeline_tag.as_deref() == Some(task)
                    || m.inferred_task.as_deref() == Some(task)));
        }
    }

    /// Real network: filter-only listing (no search term) by task + sort, then the next page
    /// through the `Link` cursor returns different repos.
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_filter_only_listing_and_cursor() {
        let opts = HfSearchOptions {
            pipeline_tag: Some("text-generation".into()),
            sort: Some("trendingScore".into()),
            limit: Some(5),
            ..Default::default()
        };
        let first = DownloadManager::search_models_page(&opts, None)
            .await
            .unwrap();
        assert_eq!(first.items.len(), 5);
        assert!(first
            .items
            .iter()
            .all(|m| m.pipeline_tag.as_deref() == Some("text-generation")));
        assert!(first.items.iter().all(|m| m.created_at.is_some()));
        let cursor = first.next_cursor.clone().expect("next cursor");
        let second = DownloadManager::search_models_page(
            &HfSearchOptions {
                cursor: Some(cursor),
                ..opts.clone()
            },
            None,
        )
        .await
        .unwrap();
        assert!(!second.items.is_empty());
        assert!(second
            .items
            .iter()
            .all(|b| first.items.iter().all(|a| a.id != b.id)));

        let newest = DownloadManager::search_models_page(
            &HfSearchOptions {
                sort: Some("createdAt".into()),
                limit: Some(3),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        let dates: Vec<_> = newest
            .items
            .iter()
            .map(|m| m.created_at.clone().unwrap())
            .collect();
        assert!(dates.windows(2).all(|w| w[0] >= w[1]), "{dates:?}");
    }

    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_search_api() {
        let opts = HfSearchOptions {
            query: "Llama-3.2-1B-Instruct".into(),
            license: Some("llama3.2".into()),
            sort: Some("likes".into()),
            ..Default::default()
        };
        let models = DownloadManager::search_models(&opts, None).await.unwrap();
        assert!(!models.is_empty());
        assert!(models
            .iter()
            .all(|m| m.license.as_deref() == Some("llama3.2")));
        let card = DownloadManager::get_model_card(&models[0].id, None)
            .await
            .unwrap();
        assert!(card.readme.is_some());
        let access = DownloadManager::check_access("meta-llama/Llama-3.2-1B", None)
            .await
            .unwrap();
        assert!(access.gated.is_some());
        assert_eq!(access.has_access, Some(false));
    }

    /// Real network: a gated repo reports `gated` and refuses anonymous access with the keyed
    /// error, a public repo is open, a missing repo is "not accessible".
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_gated_detection() {
        let gated = DownloadManager::check_access("meta-llama/Llama-3.2-1B", None)
            .await
            .unwrap();
        assert!(gated.gated.is_some(), "{gated:?}");
        assert_eq!(gated.has_access, Some(false));
        assert!(!gated.token_present);
        assert_eq!(gated.license.as_deref(), Some("llama3.2"));
        let reason = gated.reason.unwrap();
        assert!(reason.starts_with("hub.gated_no_token"), "{reason}");

        // The download path maps the real 401 + X-Error-Code: GatedRepo the same way.
        let url = "https://huggingface.co/meta-llama/Llama-3.2-1B/resolve/main/config.json";
        let res = DownloadManager::client()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        let e = hf_refusal(
            res.status(),
            res.headers(),
            "meta-llama/Llama-3.2-1B",
            false,
        )
        .unwrap();
        assert!(e.starts_with("hub.gated_no_token"), "{e}");
        assert_eq!(
            repo_from_hf_url(url).as_deref(),
            Some("meta-llama/Llama-3.2-1B")
        );

        let open = DownloadManager::check_access("Qwen/Qwen2.5-0.5B-Instruct-GGUF", None)
            .await
            .unwrap();
        assert_eq!(open.gated, None);
        assert_eq!(open.has_access, Some(true));
        assert!(open.reason.is_none());

        let missing = DownloadManager::check_access("harness-studio-test/no-such-repo-xyz", None)
            .await
            .unwrap();
        assert_eq!(missing.has_access, Some(false));
        assert!(missing
            .reason
            .unwrap()
            .starts_with("hf.repo_not_accessible"));
    }

    fn variant(name: &str, quant: &str, files: &[&str]) -> HfModelVariant {
        HfModelVariant {
            not_runnable: None,
            name: name.into(),
            quantization: quant.into(),
            total_size_bytes: 1,
            is_split: files.len() > 1,
            shards: files
                .iter()
                .enumerate()
                .map(|(i, f)| HfShardInfo {
                    filename: (*f).into(),
                    download_url: format!("https://huggingface.co/o/r/resolve/main/{}", f),
                    size_bytes: 1,
                    shard_index: i as u32 + 1,
                    total_shards: files.len() as u32,
                    sha256: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_pick_variant_by_file_quant_or_single() {
        let list = vec![
            variant("m-Q4_K_M.gguf", "Q4_K_M", &["m-Q4_K_M.gguf"]),
            variant(
                "m-Q8_0",
                "Q8_0",
                &[
                    "Q8_0/m-Q8_0-00001-of-00002.gguf",
                    "Q8_0/m-Q8_0-00002-of-00002.gguf",
                ],
            ),
            variant("mmproj-m-f16.gguf", "mmproj", &["mmproj-m-f16.gguf"]),
        ];
        assert_eq!(
            pick_variant(&list, None, Some("q4_k_m")).unwrap().name,
            "m-Q4_K_M.gguf"
        );
        assert_eq!(
            pick_variant(&list, Some("m-Q8_0-00002-of-00002.gguf"), None)
                .unwrap()
                .quantization,
            "Q8_0"
        );
        assert_eq!(
            pick_variant(
                &list,
                Some("Q8_0/m-Q8_0-00001-of-00002.gguf"),
                Some("Q4_K_M")
            )
            .unwrap()
            .quantization,
            "Q8_0",
            "file wins over quant"
        );
        assert_eq!(
            pick_variant(&list, None, Some("mmproj"))
                .unwrap()
                .quantization,
            "mmproj"
        );
        assert_eq!(
            pick_variant(&list, None, Some("Q2_K")).unwrap_err(),
            ERR_VARIANT_NOT_FOUND
        );
        let ambiguous = pick_variant(&list, None, None).unwrap_err();
        let (key, params) = crate::error_key::parse(&ambiguous).unwrap();
        assert_eq!(key, ERR_VARIANT_AMBIGUOUS);
        assert_eq!(params["count"], 2);
        assert_eq!(params["files"], "m-Q4_K_M.gguf, m-Q8_0");
        assert_eq!(
            pick_variant(&list[..1], None, None).unwrap().quantization,
            "Q4_K_M"
        );
        assert_eq!(
            pick_variant(&[], None, None).unwrap_err(),
            ERR_VARIANT_NOT_FOUND
        );
    }

    #[test]
    fn test_pick_variant_file_after_colon_and_candidate_list() {
        let mut list: Vec<HfModelVariant> = ["model", "model_fp16", "model_quantized"]
            .iter()
            .map(|n| {
                let f = format!("onnx/{n}.onnx");
                variant(&f, "FP32", &[&f])
            })
            .collect();
        assert_eq!(
            pick_variant(&list, None, Some("onnx/model_fp16.onnx"))
                .unwrap()
                .name,
            "onnx/model_fp16.onnx"
        );
        assert_eq!(
            pick_variant(&list, None, Some("model_quantized.onnx"))
                .unwrap()
                .name,
            "onnx/model_quantized.onnx"
        );
        assert!(looks_like_file("onnx/model.onnx") && looks_like_file("m.GGUF"));
        assert!(!looks_like_file("Q4_K_M"));
        let (_, p) =
            crate::error_key::parse(&pick_variant(&list, None, None).unwrap_err()).unwrap();
        assert_eq!(
            p["files"],
            "onnx/model.onnx, onnx/model_fp16.onnx, onnx/model_quantized.onnx"
        );
        for i in 0..12 {
            let f = format!("onnx/extra{i}.onnx");
            list.push(variant(&f, "FP32", &[&f]));
        }
        let (_, p) =
            crate::error_key::parse(&pick_variant(&list, None, None).unwrap_err()).unwrap();
        assert_eq!(p["count"], 15);
        assert!(p["files"].as_str().unwrap().ends_with(", ... (+5 more)"));
    }

    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_hf_get_variants_and_split_grouping() {
        let variants = DownloadManager::get_model_variants(
            "tinh2406/full_llama2_split_merged-Q4_K_M-GGUF",
            None,
        )
        .await
        .unwrap();
        let v = variants
            .iter()
            .find(|v| v.is_split)
            .expect("Expected split variant to be grouped");
        assert_eq!(v.shards.len(), 2);
        assert_eq!(v.shards[0].shard_index, 1);
    }

    #[tokio::test]
    #[ignore]
    async fn test_hf_get_qwen_variants() {
        let variants =
            DownloadManager::get_model_variants("Qwen/Qwen2.5-Coder-14B-Instruct-GGUF", None)
                .await
                .unwrap();
        println!("Qwen variants count: {}", variants.len());
        for v in &variants {
            println!(
                "  Variant: {} (quant: {}, size: {})",
                v.name, v.quantization, v.total_size_bytes
            );
        }
        assert!(!variants.is_empty());
    }

    /// Real queue run: download, dedupe, cancel → paused → resume → completed + SHA-256.
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn test_queue_real_download() {
        use std::time::{Duration, Instant};
        let file = "tinyllamas/stories15M-q4_0.gguf";
        let variant = HfModelVariant {
            not_runnable: None,
            name: "stories15M-q4_0".into(),
            quantization: "Q4_0".into(),
            total_size_bytes: 19077344,
            is_split: false,
            shards: vec![HfShardInfo {
                filename: file.into(),
                download_url: format!(
                    "https://huggingface.co/ggml-org/models/resolve/main/{}",
                    file
                ),
                size_bytes: 19077344,
                shard_index: 1,
                total_shards: 1,
                sha256: Some(
                    "66967fbece6dbe97886593fdbb73589584927e29119ec31f08090732d1861739".into(),
                ),
            }],
        };
        let target = DownloadManager::models_dir()
            .join("ggml-org/models")
            .join(file);
        let _ = fs::remove_file(&target);
        let url = variant.shards[0].download_url.clone();
        let _ = fs::remove_file(DownloadManager::cache_dir().join(part_name(&url, file)));

        let mgr = DownloadManager::default();
        let id = mgr
            .start_variant_download("ggml-org/models".into(), variant.clone())
            .unwrap();
        assert!(
            mgr.start_variant_download("ggml-org/models".into(), variant.clone())
                .is_err(),
            "duplicate while pending"
        );

        // Cancel as soon as some bytes arrived.
        let t = Instant::now();
        while mgr.list()[0].downloaded_bytes == 0 {
            assert!(t.elapsed() < Duration::from_secs(60), "no progress");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        mgr.cancel_download(Some(id));
        let wait = |want: DownloadStatus| {
            let mgr = mgr.clone();
            async move {
                let t = Instant::now();
                loop {
                    let st = mgr.list()[0].status.clone();
                    if st == want {
                        return;
                    }
                    if let DownloadStatus::Error(e) = &st {
                        panic!("download failed: {}", e);
                    }
                    assert!(t.elapsed() < Duration::from_secs(120), "stuck in {:?}", st);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        };
        wait(DownloadStatus::Paused).await;

        mgr.resume_download(id).unwrap();
        wait(DownloadStatus::Completed).await;
        assert_eq!(fs::metadata(&target).unwrap().len(), 19077344);
        assert!(
            recorded_hash(&target).is_some(),
            "sha256 verified and recorded"
        );
        assert_eq!(mgr.list().len(), 1);
        mgr.clear_finished();
        assert!(mgr.list().is_empty());
    }

    /// The tree of microsoft/Phi-3-mini-4k-instruct-onnx (names of 2026-10, sizes rounded).
    fn phi3_tree() -> Vec<serde_json::Value> {
        let file = |path: &str, size: u64| {
            serde_json::json!({"type": "file", "path": path, "size": size,
                "lfs": {"oid": "b".repeat(64), "size": size}})
        };
        let mut v = vec![file("README.md", 5_000), file(".gitattributes", 1_000)];
        for (dir, graph) in [
            (
                "directml/directml-int4-awq-block-128",
                "model.onnx".to_string(),
            ),
            (
                "cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4",
                "phi3-mini-4k-instruct-cpu-int4-rtn-block-32-acc-level-4.onnx".to_string(),
            ),
            ("cuda/cuda-int4-rtn-block-32", "model.onnx".to_string()),
            ("npu/qnn-int4", "model.onnx".to_string()),
        ] {
            v.push(file(&format!("{dir}/genai_config.json"), 1_600));
            v.push(file(&format!("{dir}/{graph}"), 2_000_000));
            v.push(file(&format!("{dir}/{graph}.data"), 2_100_000_000));
            v.push(file(&format!("{dir}/tokenizer.json"), 1_900_000));
            v.push(file(&format!("{dir}/tokenizer.model"), 500_000));
            v.push(file(&format!("{dir}/tokenizer_config.json"), 3_400));
            v.push(file(&format!("{dir}/configuration_phi3.py"), 10_000));
        }
        v
    }

    #[test]
    fn genai_repo_variants_follow_the_folders_and_this_machine() {
        let v = genai_variants("microsoft/Phi-3-mini-4k-instruct-onnx", &phi3_tree()).unwrap();
        let names: Vec<&str> = v.iter().map(|v| v.name.as_str()).collect();
        // The NPU folder is left out; DirectML first on Windows, the CPU first elsewhere.
        if cfg!(windows) {
            assert_eq!(
                names,
                [
                    "directml-int4-awq-block-128",
                    "cpu-int4-rtn-block-32-acc-level-4",
                    "cuda-int4-rtn-block-32"
                ]
            );
        } else {
            assert_eq!(names[0], "cpu-int4-rtn-block-32-acc-level-4");
            assert_eq!(names.len(), 3);
        }
        let dml = v.iter().find(|v| v.name.starts_with("directml")).unwrap();
        assert_eq!(dml.quantization, "INT4");
        let files: Vec<&str> = dml.shards.iter().map(|s| s.filename.as_str()).collect();
        // The layout of the repo is kept; the Python file and the docs are not downloaded.
        assert!(files.contains(&"directml/directml-int4-awq-block-128/model.onnx.data"));
        assert!(files.contains(&"directml/directml-int4-awq-block-128/genai_config.json"));
        assert!(files
            .iter()
            .all(|f| !f.ends_with(".py") && !f.ends_with(".md")));
        assert!(dml.total_size_bytes > 2_000_000_000);
        assert!(dml.shards[0]
            .download_url
            .ends_with("Phi-3-mini-4k-instruct-onnx/resolve/main/directml/directml-int4-awq-block-128/genai_config.json")
            || dml.shards.iter().any(|s| s.download_url.contains("/resolve/main/directml/")));
        // The same repo is not an encoder repo, and an encoder repo has no genai variants.
        assert!(
            genai_variants("sentence-transformers/all-MiniLM-L6-v2", &minilm_tree())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_genai_model_at_the_repo_root_is_one_variant_named_after_the_repo() {
        let file =
            |path: &str, size: u64| serde_json::json!({"type": "file", "path": path, "size": size});
        let v = genai_variants(
            "hb-dev/Qwen3-0.6B-ONNX-GenAI",
            &[
                file("genai_config.json", 1_520),
                file("model.onnx", 524_589_133),
                file("tokenizer.json", 11_422_648),
                file("chat_template.jinja", 4_168),
                file("tokenizer_config.json", 663),
                file("README.md", 28),
            ],
        )
        .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "Qwen3-0.6B-ONNX-GenAI");
        assert_eq!(v[0].shards.len(), 5);
        assert!(v[0]
            .shards
            .iter()
            .any(|s| s.filename == "chat_template.jinja"));
        // A folder with a config but no graph is not a model.
        assert!(genai_variants("a/b", &[file("genai_config.json", 10)])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn genai_repo_files_are_recognised() {
        for yes in [
            "directml/x/genai_config.json",
            "model.onnx",
            "model.onnx.data",
            "tokenizer.model",
            "chat_template.jinja",
            "special_tokens_map.json",
            "merges.txt",
        ] {
            assert!(is_genai_repo_file(yes), "{yes}");
        }
        for no in [
            "README.md",
            "LICENSE",
            ".gitattributes",
            "x/configuration_phi3.py",
            "img.png",
        ] {
            assert!(!is_genai_repo_file(no), "{no}");
        }
        assert_eq!(genai_target_rank("npu/qnn-int4"), None);
        assert_eq!(genai_target_rank("web/webgpu-int4"), None);
    }

    /// The file list of sentence-transformers/all-MiniLM-L6-v2 (HF tree API shape, names and
    /// sizes of 2026-10).
    fn minilm_tree() -> Vec<serde_json::Value> {
        let file = |path: &str, size: u64| {
            serde_json::json!({"type": "file", "path": path, "size": size,
                "lfs": {"oid": "a".repeat(64), "size": size}})
        };
        vec![
            file("1_Pooling/config.json", 190),
            file("config.json", 612),
            file("modules.json", 349),
            file("model.safetensors", 90_868_376),
            file("onnx/model.onnx", 90_405_214),
            file("onnx/model_O4.onnx", 45_212_349),
            file("onnx/model_qint8_avx512.onnx", 23_000_000),
            file("openvino/openvino_model.xml", 1_000),
            file("pytorch_model.bin", 90_888_945),
            file("sentence_bert_config.json", 53),
            file("tokenizer.json", 466_247),
            file("tokenizer_config.json", 350),
            file("2_Dense/model.safetensors", 4_000_000),
            file("2_Dense/config.json", 100),
        ]
    }

    #[test]
    fn a_generative_repo_offers_no_encoder_onnx_variants() {
        let mut tree = minilm_tree();
        assert!(!encoder_onnx_variants("acme/encoder", &tree)
            .unwrap()
            .is_empty());
        // The same listing with a generation config is a decoder export (SmolLM2, Qwen3-ONNX).
        tree.push(
            serde_json::json!({"type": "file", "path": "generation_config.json", "size": 132}),
        );
        assert!(encoder_onnx_variants("acme/chat", &tree)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn onnx_repo_variants_carry_their_companions_in_the_repo_layout() {
        let v = onnx_variants("sentence-transformers/all-MiniLM-L6-v2", &minilm_tree()).unwrap();
        assert_eq!(
            v.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            vec!["model", "model_O4", "model_qint8_avx512"],
            "the repo's own model first"
        );
        let base = &v[0];
        assert_eq!(base.quantization, "FP32");
        assert_eq!(v[1].quantization, "FP16");
        assert_eq!(v[2].quantization, "INT8");
        let names: Vec<&str> = base.shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(names[0], "onnx/model.onnx");
        for needed in [
            "tokenizer.json",
            "config.json",
            "tokenizer_config.json",
            "sentence_bert_config.json",
            "modules.json",
            "1_Pooling/config.json",
        ] {
            assert!(names.contains(&needed), "{needed} missing from {names:?}");
        }
        // Not the original weights, the other formats, or modules the engine cannot run.
        for skipped in [
            "model.safetensors",
            "pytorch_model.bin",
            "2_Dense/model.safetensors",
        ] {
            assert!(!names.contains(&skipped), "{skipped}");
        }
        assert!(!names.iter().any(|n| n.starts_with("openvino/")));
        assert!(base
            .shards
            .iter()
            .all(|s| s.total_shards == base.shards.len() as u32));
        assert_eq!(
            base.total_size_bytes,
            base.shards.iter().map(|s| s.size_bytes).sum::<u64>()
        );
        assert!(base.shards[0]
            .download_url
            .ends_with("all-MiniLM-L6-v2/resolve/main/onnx/model.onnx"));
        // The two other variants share the companions.
        let tail = |v: &HfModelVariant| -> Vec<String> {
            v.shards[1..].iter().map(|s| s.filename.clone()).collect()
        };
        assert_eq!(tail(&v[1]), tail(base));
    }

    #[test]
    fn onnx_repo_without_tokenizer_or_with_external_weights() {
        let file = |path: &str| serde_json::json!({"type": "file", "path": path, "size": 10});
        // A bare graph: no tokenizer and no image processor, no variants.
        assert!(onnx_variants("acme/resnet", &[file("model.onnx")])
            .unwrap()
            .is_empty());
        // A transformers.js vision repo: the graph with its configs.
        let v = onnx_variants(
            "Xenova/vit-base-patch16-224",
            &[
                file("config.json"),
                file("preprocessor_config.json"),
                file("onnx/model_quantized.onnx"),
            ],
        )
        .unwrap();
        assert_eq!(v.len(), 1);
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert!(names.contains(&"preprocessor_config.json"));
        // CLIP: the joint graphs are variants, the towers beside them are not.
        let v = onnx_variants(
            "Xenova/clip-vit-base-patch32",
            &[
                file("config.json"),
                file("preprocessor_config.json"),
                file("tokenizer.json"),
                file("onnx/model.onnx"),
                file("onnx/model_quantized.onnx"),
                file("onnx/text_model.onnx"),
                file("onnx/vision_model_quantized.onnx"),
            ],
        )
        .unwrap();
        let mut names: Vec<&str> = v.iter().map(|v| v.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["model", "model_quantized"]);
        // An image captioner: each encoder brings the decoder of its precision.
        let v = onnx_variants(
            "Xenova/vit-gpt2-image-captioning",
            &[
                file("config.json"),
                file("preprocessor_config.json"),
                file("tokenizer.json"),
                file("onnx/encoder_model.onnx"),
                file("onnx/encoder_model_quantized.onnx"),
                file("onnx/decoder_model.onnx"),
                file("onnx/decoder_model_merged.onnx"),
                file("onnx/decoder_model_merged_quantized.onnx"),
                file("onnx/decoder_with_past_model.onnx"),
            ],
        )
        .unwrap();
        let mut names: Vec<&str> = v.iter().map(|v| v.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["encoder_model", "encoder_model_quantized"]);
        let q = v
            .iter()
            .find(|v| v.name == "encoder_model_quantized")
            .unwrap();
        let files: Vec<&str> = q.shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(
            files[..2],
            [
                "onnx/encoder_model_quantized.onnx",
                "onnx/decoder_model_merged_quantized.onnx"
            ]
        );
        // Big models keep weights in a sibling file; it travels with the graph.
        let v = onnx_variants(
            "acme/big-embedder",
            &[
                file("tokenizer.json"),
                file("onnx/model.onnx"),
                file("onnx/model.onnx_data"),
                file("onnx/other/model.onnx"),
            ],
        )
        .unwrap();
        assert_eq!(v.len(), 1, "deeper folders are other pipelines");
        let names: Vec<&str> = v[0].shards.iter().map(|s| s.filename.as_str()).collect();
        assert_eq!(names[..2], ["onnx/model.onnx", "onnx/model.onnx_data"]);
        // An unmerged export: the plain decoder travels with every encoder.
        let other = onnx_variants(
            "acme/captioner",
            &[
                file("config.json"),
                file("tokenizer.json"),
                file("onnx/encoder_model.onnx"),
                file("onnx/encoder_model_fp16.onnx"),
                file("onnx/decoder_model.onnx"),
            ],
        )
        .unwrap();
        assert_eq!(other.len(), 2);
        for variant in &other {
            assert!(variant
                .shards
                .iter()
                .any(|s| s.filename == "onnx/decoder_model.onnx"));
        }
        // An encoder with no decoder at all is a plain encoder (a T5 encoder for embeddings).
        let other = onnx_variants(
            "acme/t5-encoder",
            &[file("tokenizer.json"), file("onnx/encoder_model.onnx")],
        )
        .unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].shards[0].filename, "onnx/encoder_model.onnx");
    }

    #[test]
    fn onnx_wordpiece_variant_keeps_the_legacy_vocabulary() {
        let file = |path: &str| serde_json::json!({"type": "file", "path": path, "size": 10});
        let variants = onnx_variants(
            "acme/bert-ner",
            &[
                file("config.json"),
                file("vocab.txt"),
                file("tokenizer_config.json"),
                file("special_tokens_map.json"),
                file("onnx/model.onnx"),
            ],
        )
        .unwrap();
        assert_eq!(variants.len(), 1);
        let names: Vec<&str> = variants[0]
            .shards
            .iter()
            .map(|s| s.filename.as_str())
            .collect();
        assert!(names.contains(&"vocab.txt"));
        assert!(names.contains(&"tokenizer_config.json"));
        assert!(names.contains(&"special_tokens_map.json"));
    }

    #[test]
    fn onnx_repo_files_are_recognised() {
        for yes in [
            "onnx/model.onnx",
            "onnx/model.onnx_data",
            "tokenizer.json",
            "1_Pooling/config.json",
            "2_Normalize/config.json",
            "modules.json",
        ] {
            assert!(is_onnx_repo_file(yes), "{yes}");
        }
        for no in [
            "2_Dense/config.json",
            "model.safetensors",
            "README.md",
            "a/b/config.json",
        ] {
            assert!(!is_onnx_repo_file(no), "{no}");
        }
    }

    #[test]
    fn finished_onnx_files_are_checked_for_web_pages_not_for_a_magic() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("model.onnx");
        fs::write(&ok, [0x08, 0x08, 0x12, 0x07]).unwrap(); // protobuf
        assert!(verify_model_magic(&ok, "onnx/model.onnx").is_ok());
        let json = dir.path().join("tokenizer.json");
        fs::write(&json, "{\"version\":\"1.0\"}").unwrap();
        assert!(verify_model_magic(&json, "tokenizer.json").is_ok());
        let page = dir.path().join("config.json");
        fs::write(&page, "<!DOCTYPE html><html>Sign in</html>").unwrap();
        assert!(verify_model_magic(&page, "config.json").is_err());
        assert!(!page.exists(), "the bad file is deleted");
        let empty = dir.path().join("e.onnx");
        fs::write(&empty, b"").unwrap();
        assert!(verify_model_magic(&empty, "e.onnx").is_err());
        // GGUF files still need their magic.
        let gguf = dir.path().join("m.gguf");
        fs::write(&gguf, b"NOPE").unwrap();
        assert!(verify_model_magic(&gguf, "m.gguf").is_err());
    }

    #[test]
    fn whisper_ggml_files_are_model_files() {
        for ok in [
            "ggml-base.bin",
            "ggml-large-v3-turbo-q5_0.bin",
            "ggml-tiny.en.bin",
            "sub/GGML-small.bin",
            "model.Q4_K_M.gguf",
        ] {
            assert!(is_model_file_name(ok), "{ok}");
        }
        for no in [
            "for-tests-ggml-base.bin",
            "pytorch_model.bin",
            "ggml-medium-encoder.mlmodelc.zip",
            "README.md",
        ] {
            assert!(!is_model_file_name(no), "{no}");
        }
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("ggml-base.bin");
        fs::write(&good, [0x6c, 0x6d, 0x67, 0x67, 0, 0]).unwrap();
        assert!(verify_model_magic(&good, "ggml-base.bin").is_ok());
        // An HTML error page saved under the model's name is refused and removed.
        let bad = dir.path().join("ggml-bad.bin");
        fs::write(&bad, b"<html>").unwrap();
        let e = verify_model_magic(&bad, "ggml-bad.bin").unwrap_err();
        assert!(e.starts_with("downloads.not_ggml"), "{e}");
        assert!(!bad.exists());
        // A GGUF keeps its own check.
        let gguf = dir.path().join("m.gguf");
        fs::write(&gguf, b"GGUF").unwrap();
        assert!(verify_model_magic(&gguf, "m.gguf").is_ok());
        // Image checkpoints: a safetensors header, a zip / pickle checkpoint.
        let st = dir.path().join("m.safetensors");
        let mut bytes = 2u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        fs::write(&st, bytes).unwrap();
        assert!(verify_model_magic(&st, "m.safetensors").is_ok());
        let html = dir.path().join("h.safetensors");
        fs::write(&html, b"<!DOCTYPE html><html>").unwrap();
        let e = verify_model_magic(&html, "h.safetensors").unwrap_err();
        assert!(e.starts_with("downloads.not_safetensors"), "{e}");
        assert!(!html.exists(), "a wrong file is deleted");
        let ckpt = dir.path().join("m.ckpt");
        fs::write(&ckpt, b"PK\x03\x04").unwrap();
        assert!(verify_model_magic(&ckpt, "m.ckpt").is_ok());
        fs::write(&ckpt, b"<html>").unwrap();
        assert!(verify_model_magic(&ckpt, "m.ckpt").is_err());
        let wrong = dir.path().join("w.gguf");
        fs::write(&wrong, [0x6c, 0x6d, 0x67, 0x67]).unwrap();
        assert!(verify_model_magic(&wrong, "w.gguf").is_err());
    }

    /// Real Hugging Face repo of whisper.cpp's models: the variants list, the download of
    /// `ggml-tiny.en.bin` (SHA-256 of the LFS pointer verified, ggml magic checked) and the
    /// library scan that finds it as a speech recognition model. Downloads ~75 MB.
    /// `cargo test -p harness-core --lib real_whisper_models_from_hf -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn real_whisper_models_from_hf() {
        use std::time::{Duration, Instant};
        let variants = DownloadManager::get_model_variants("ggerganov/whisper.cpp", None)
            .await
            .unwrap();
        for v in &variants {
            println!(
                "  {} (quant {}, {} MB)",
                v.name,
                v.quantization,
                v.total_size_bytes >> 20
            );
        }
        assert!(variants
            .iter()
            .all(|v| v.shards[0].filename.starts_with("ggml-")));
        let by = |n: &str| variants.iter().find(|v| v.name == n);
        assert_eq!(by("ggml-base").unwrap().quantization, "F16");
        assert_eq!(by("ggml-large-v3-turbo-q5_0").unwrap().quantization, "Q5_0");
        assert!(by("for-tests-ggml-base").is_none());
        let tiny = by("ggml-tiny.en").unwrap().clone();
        assert!(tiny.shards[0].sha256.is_some(), "LFS files carry a SHA-256");

        let target = DownloadManager::models_dir()
            .join("ggerganov/whisper.cpp")
            .join("ggml-tiny.en.bin");
        let _ = fs::remove_file(&target);
        let mgr = DownloadManager::default();
        let id = mgr
            .start_variant_download("ggerganov/whisper.cpp".into(), tiny.clone())
            .unwrap();
        let t = Instant::now();
        loop {
            let p = mgr.get(id).unwrap();
            match p.status {
                DownloadStatus::Completed => break,
                DownloadStatus::Error(e) => panic!("download failed: {e}"),
                other => {
                    assert!(t.elapsed() < Duration::from_secs(300), "stuck in {other:?}");
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert_eq!(fs::metadata(&target).unwrap().len(), tiny.total_size_bytes);
        assert!(
            recorded_hash(&target).is_some(),
            "SHA-256 verified and recorded"
        );
        let models = crate::models::ModelManager.scan_directories(&[DownloadManager::models_dir()]);
        let m = models.iter().find(|m| m.name == "ggml-tiny.en").unwrap();
        println!(
            "library: {} {:?} {} {}",
            m.name, m.kind, m.quantization, m.size_bytes
        );
        assert_eq!(m.kind, crate::domain::ModelKind::Asr);
        let _ = fs::remove_file(&target);
    }

    #[test]
    fn hub_onnx_ctc_root_graph_is_runnable_and_kept_with_companions() {
        for graph in ["model.onnx", "onnx/model.onnx"] {
            let external = format!("{graph}.data");
            let files = [
                graph,
                &external,
                "config.json",
                "vocab.json",
                "preprocessor_config.json",
            ];
            let siblings: Vec<_> = files
                .iter()
                .map(|f| serde_json::json!({"rfilename":f}))
                .collect();
            let mut item = serde_json::json!({"id":"acme/speech", "pipeline_tag":"automatic-speech-recognition", "config":{"architectures":["WavLMForCTC"]}, "siblings":siblings});
            let repo = parse_search_item(&item).unwrap();
            assert_eq!(repo.format, crate::hub_compat::ModelFormat::Onnx);
            assert!(matches!(
                repo.compatibility,
                crate::hub_compat::Compatibility::Runnable { .. }
            ));
            let mut tree: Vec<_> = files
                .iter()
                .map(|f| serde_json::json!({"path":f,"type":"file","size":10}))
                .collect();
            tree.push(serde_json::json!({"path":"other/model.onnx"}));
            tree.push(serde_json::json!({"path":"onnx/nested/model.onnx"}));
            let variants = onnx_variants("acme/speech", &tree).unwrap();
            assert_eq!(variants.len(), 1);
            assert_eq!(variants[0].name, "model");
            assert_eq!(variants[0].shards.len(), files.len());
            for f in files {
                assert!(variants[0].shards.iter().any(|s| s.filename == f));
            }
            item["config"]["architectures"] = serde_json::json!(["UnsupportedForCTC"]);
            assert!(matches!(
                parse_search_item(&item).unwrap().compatibility,
                crate::hub_compat::Compatibility::Unsupported { .. }
            ));
        }
    }

    #[test]
    fn ctc_hub_exports_are_downloadable_but_source_and_other_asr_are_not() {
        let files = [
            "onnx/model.onnx",
            "onnx/model_int8.onnx",
            "onnx/model_fp16.onnx",
            "onnx/model_quantized.onnx",
            "config.json",
            "vocab.json",
            "preprocessor_config.json",
            "tokenizer_config.json",
            "special_tokens_map.json",
            "added_tokens.json",
            "model.safetensors",
        ];
        let siblings: Vec<_> = files
            .iter()
            .map(|f| serde_json::json!({"rfilename":f}))
            .collect();
        let mut item = serde_json::json!({"id":"acme/speech", "config":{"architectures":["Wav2Vec2ForCTC"]}, "siblings":siblings});
        let repo = parse_search_item(&item).unwrap();
        assert_eq!(
            repo.inferred_task.as_deref(),
            Some("automatic-speech-recognition")
        );
        assert_eq!(repo.format, crate::hub_compat::ModelFormat::Onnx);
        assert_eq!(
            repo.compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: crate::engine::ONNX_FAMILY.into()
            }
        );
        let tree: Vec<_> = files
            .iter()
            .map(|f| serde_json::json!({"path":f,"type":"file","size":10}))
            .collect();
        let variants = onnx_variants("acme/speech", &tree).unwrap();
        assert_eq!(variants.len(), 4);
        assert_eq!(variants[0].name, "model");
        for variant in variants {
            for f in &files[4..10] {
                assert!(variant.shards.iter().any(|s| s.filename == *f));
            }
            assert!(!variant
                .shards
                .iter()
                .any(|s| s.filename.ends_with("safetensors")));
        }
        // The CTC config is checked before the encoder/vision path, which now also takes
        // `preprocessor_config.json` repos (vision models).
        // Source weights are not a native CTC export; they go to the optional Python engine.
        item["siblings"] =
            serde_json::json!([{"rfilename":"model.safetensors"},{"rfilename":"config.json"}]);
        // Runnable or NeedsRuntime depending on whether that engine is installed here.
        assert!(matches!(
            parse_search_item(&item).unwrap().compatibility,
            crate::hub_compat::Compatibility::Runnable { ref runtime }
                | crate::hub_compat::Compatibility::NeedsRuntime { ref runtime }
                if runtime == crate::hub_compat::PYTHON_HF_FAMILY
        ));
        item["siblings"] =
            serde_json::json!([{"rfilename":"onnx/model.onnx"},{"rfilename":"config.json"}]);
        assert!(matches!(
            parse_search_item(&item).unwrap().compatibility,
            crate::hub_compat::Compatibility::Unsupported { .. }
        ));
        item["siblings"] = serde_json::to_value(siblings).unwrap();
        item["config"]["architectures"] = serde_json::json!(["WhisperForConditionalGeneration"]);
        item["pipeline_tag"] = serde_json::json!("automatic-speech-recognition");
        // Not a native ONNX CTC model; it may still go to the optional Python engine.
        assert_ne!(
            parse_search_item(&item).unwrap().compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: crate::engine::ONNX_FAMILY.into()
            }
        );
    }

    #[test]
    fn test_clean_hf_search_query() {
        assert_eq!(
            clean_hf_search_query("Qwen2.5-Coder 14B Instruct GGUF (Q4_K_M)"),
            "Qwen2.5-Coder 14B Instruct"
        );
        assert_eq!(
            clean_hf_search_query("Llama-3.2-3B-Instruct-Q4_K_M.gguf"),
            "Llama-3.2-3B-Instruct-"
        );
        assert_eq!(
            clean_hf_search_query("deepseek-r1-14b [Q5_K_M]"),
            "deepseek-r1-14b"
        );
    }

    #[test]
    fn exact_repo_precedes_popular_matches_without_reordering_them() {
        let mut items: Vec<_> = [
            "other/Qwen3-4B-GGUF",
            "unsloth/Qwen3-4B-GGUF-extra",
            "unsloth/Qwen3-4B-GGUF",
        ]
        .into_iter()
        .map(|id| parse_search_item(&serde_json::json!({"id": id})).unwrap())
        .collect();
        prioritize_exact_repo(&mut items, " UNSLOTH/Qwen3-4B-GGUF ");
        let ids: Vec<_> = items.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "unsloth/Qwen3-4B-GGUF",
                "other/Qwen3-4B-GGUF",
                "unsloth/Qwen3-4B-GGUF-extra"
            ]
        );
    }

    #[test]
    fn runnable_extras_join_the_first_page_in_sort_order() {
        let mk = |id: &str, downloads: u64| {
            parse_search_item(&serde_json::json!({
                "id": id, "downloads": downloads, "tags": ["onnx", "sentence-transformers"],
                "pipeline_tag": "feature-extraction",
                "siblings": [{"rfilename": "onnx/model.onnx"}]
            }))
            .unwrap()
        };
        let mut items = vec![mk("a/gguf-1", 500), mk("a/gguf-2", 10)];
        merge_runnable_extras(
            &mut items,
            vec![mk("sentence-transformers/x", 100), mk("a/gguf-1", 500)],
            None,
        );
        let ids: Vec<&str> = items.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["a/gguf-1", "sentence-transformers/x", "a/gguf-2"]);
    }

    #[test]
    fn search_extras_merge_before_whisper_and_preserve_listing_ties_and_cursor() {
        let mk = |id: &str, downloads: u64| {
            parse_search_item(&serde_json::json!({"id": id, "downloads": downloads})).unwrap()
        };
        let mut page = HfSearchPage {
            items: vec![mk("a/first", 100), mk("a/second", 10)],
            next_cursor: Some("next-page".into()),
        };
        merge_search_extras(
            &mut page,
            vec![mk("a/extra", 100), mk("a/first", 999)],
            Some(mk(WHISPER_CPP_REPO, 0)),
            None,
        );
        let ids: Vec<_> = page.items.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, [WHISPER_CPP_REPO, "a/first", "a/extra", "a/second"]);
        assert_eq!(page.items[1].downloads, 100);
        assert_eq!(page.next_cursor.as_deref(), Some("next-page"));
    }

    #[test]
    fn search_extras_do_not_replace_or_promote_an_existing_whisper_repo() {
        let mk = |id: &str, likes: u64| {
            parse_search_item(&serde_json::json!({"id": id, "likes": likes})).unwrap()
        };
        for listed in [false, true] {
            let mut page = HfSearchPage {
                items: vec![mk("a/first", 100)],
                next_cursor: None,
            };
            let mut extras = vec![mk("a/extra", 50)];
            if listed {
                page.items.push(mk(WHISPER_CPP_REPO, 10));
            } else {
                extras.push(mk(WHISPER_CPP_REPO, 10));
            }
            merge_search_extras(
                &mut page,
                extras,
                Some(mk(WHISPER_CPP_REPO, 999)),
                Some("likes"),
            );
            let ids: Vec<_> = page.items.iter().map(|m| m.id.as_str()).collect();
            assert_eq!(ids, ["a/first", "a/extra", WHISPER_CPP_REPO]);
            assert_eq!(page.items[2].likes, 10);
        }
    }

    #[tokio::test]
    async fn shared_client_reuses_connections_across_calls() {
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            for _ in 0..2 {
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(socket.read_u8().await.unwrap());
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.to_ascii_lowercase().contains(&format!(
                    "user-agent: {}\r\n",
                    USER_AGENT.to_ascii_lowercase()
                )));
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                    .await
                    .unwrap();
            }
        });
        for _ in 0..2 {
            let response = DownloadManager::client()
                .unwrap()
                .get(&url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
                .unwrap();
            assert_eq!(response.text().await.unwrap(), "OK");
        }
        server.await.unwrap();
    }

    #[test]
    fn untagged_diffusion_gguf_takes_the_image_route() {
        let item = serde_json::json!({
            "id": "Green-Sky/SD-Turbo-GGUF", "tags": ["gguf"],
            "siblings": [{"rfilename": "sd_turbo-f16-q8_0.gguf"}]
        });
        let m = parse_search_item(&item).unwrap();
        assert_eq!(m.pipeline_tag, None);
        assert_eq!(m.inferred_task.as_deref(), Some("text-to-image"));
        assert_eq!(
            m.compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: crate::hub_compat::SDCPP_FAMILY.to_string()
            }
        );
        assert_eq!(m.not_runnable, None);
    }
}

#[cfg(test)]
mod audio_cpp_tests {
    use super::*;

    #[tokio::test]
    async fn restored_paused_job_shows_part_file_progress() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join(".cache");
        fs::create_dir_all(&cache).unwrap();
        let item = |name: &str, size: u64, index: u32| DownloadItem {
            url: format!("https://example.test/{name}"),
            filename: name.to_string(),
            size,
            shard_index: index,
            sha256: None,
            custom_url: false,
        };
        let items = vec![item("a.gguf", 100, 1), item("b.gguf", 100, 2)];
        // Shard 1 finished, shard 2 has a 40-byte partial.
        fs::write(dir.path().join("a.gguf"), vec![0u8; 100]).unwrap();
        fs::write(
            cache.join(part_name(&items[1].url, &items[1].filename)),
            vec![0u8; 40],
        )
        .unwrap();
        let job = |status: &str| -> Job {
            serde_json::from_value(serde_json::json!({
                "progress": {
                    "id": 1, "repo_id": "r/m", "variant_name": "v",
                    "total_bytes": 200, "downloaded_bytes": 0, "current_shard_name": "",
                    "current_shard_index": 0, "total_shards": 2, "speed_bytes_per_sec": 0,
                    "status": status
                },
                "items": items, "target_dir": dir.path()
            }))
            .unwrap()
        };
        let mut jobs = vec![job("Paused"), job("Completed")];
        restore_part_progress(&mut jobs, &cache);
        assert_eq!(jobs[0].progress.downloaded_bytes, 140);
        assert_eq!(jobs[1].progress.downloaded_bytes, 0);
    }

    #[tokio::test]
    async fn cached_piper_package_progress_counts_companions() {
        let dir = tempfile::tempdir().unwrap();
        let items: Vec<_> = [
            ("piper-en-us-lessac-medium-orig.gguf", "GGUF model"),
            ("LICENSE", "MIT License"),
            ("README.md", "# Piper TTS"),
        ]
        .into_iter()
        .map(|(name, contents)| {
            let filename = format!("Piper-TTS-GGUF/{name}");
            let path = dir.path().join(&filename);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
            DownloadItem {
                url: format!(
                    "https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/main/{filename}"
                ),
                filename,
                size: contents.len() as u64,
                shard_index: 1,
                sha256: name
                    .ends_with(".gguf")
                    .then(|| crate::hex::lower(&Sha256::digest(contents))),
                custom_url: false,
            }
        })
        .collect();
        let total: u64 = items.iter().map(|item| item.size).sum();
        let manager = DownloadManager::default();
        let job: Job = serde_json::from_value(serde_json::json!({
            "progress": {
                "id": 1, "repo_id": "audio-cpp/audio.cpp-gguf", "variant_name": "piper",
                "total_bytes": total, "downloaded_bytes": 0, "current_shard_name": "",
                "current_shard_index": 0, "total_shards": 3, "speed_bytes_per_sec": 0,
                "status": "Downloading"
            },
            "items": items, "target_dir": dir.path()
        }))
        .unwrap();
        manager.jobs.lock().unwrap().push(job);
        let context = JobCtx {
            id: 1,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        manager
            .run_job(&context, &items, dir.path(), None)
            .await
            .unwrap();
        let progress = manager.get(1).unwrap();
        assert_eq!(progress.current_shard_index, progress.total_shards);
        assert_eq!(progress.current_shard_name, "Piper-TTS-GGUF/README.md");
        assert_eq!(progress.downloaded_bytes, total);
    }

    #[test]
    fn piper_companions_do_not_require_gguf_magic() {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in [
            ("piper-en-us-lessac-medium-orig.gguf", "GGUF model"),
            ("LICENSE", "MIT License"),
            ("README.md", "# Piper TTS"),
            ("voices.bin", "voice data"),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, contents).unwrap();
            verify_model_magic(&path, &format!("Piper-TTS-GGUF/{name}")).unwrap();
            assert!(path.exists());
        }
        for name in [
            "invalid.gguf",
            "invalid.GGUF",
            "invalid.safetensors",
            "README.md",
        ] {
            let path = dir.path().join(name);
            fs::write(&path, "<html>Sign in</html>").unwrap();
            assert!(verify_model_magic(&path, name).is_err());
            assert!(!path.exists());
        }
    }

    /// audio-cpp/Nemotron-3-Diarization-GGUF lists LICENSE.html among the files of the package:
    /// the web-page check must not delete it and fail the whole download.
    #[test]
    fn a_licence_page_is_not_mistaken_for_an_error_page() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["LICENSE.html", "license.HTM"] {
            let path = dir.path().join(name);
            fs::write(
                &path,
                "<!doctype html>
<html lang=\"en-US\"><body>OpenMDW</body></html>",
            )
            .unwrap();
            verify_model_magic(&path, name).unwrap();
            assert!(path.exists());
        }
    }

    /// The root-level packages: the repository id names the family, so every GGUF of the repo
    /// is runnable and its licence page and README come along as companions.
    #[test]
    fn single_package_audio_repos_download_their_weights_and_licence() {
        for (repo, gguf, license) in [
            ("audio-cpp/RE-USE-GGUF", "reuse-f32.gguf", "LICENSE"),
            (
                "audio-cpp/Nemotron-3-Diarization-GGUF",
                "nemotron-3-diarization-q8_0.gguf",
                "LICENSE.html",
            ),
        ] {
            let items = [
                json!({"path": license, "size": 5}),
                json!({"path": "README.md", "size": 5}),
                json!({"path": gguf, "size": 100, "lfs": {"size": 100}}),
            ];
            let variants = audio_cpp_variants(repo, &items, None).unwrap();
            assert_eq!(variants.len(), 1, "{repo}");
            assert_eq!(variants[0].not_runnable, None, "{repo}");
            let files: Vec<_> = variants[0]
                .shards
                .iter()
                .map(|s| s.filename.as_str())
                .collect();
            // The weights first, then the companions in name order.
            assert_eq!(files, [gguf, license, "README.md"], "{repo}");
        }
        // Another repository's root-level GGUF is not captured.
        let items = [json!({"path": "model.gguf", "size": 100})];
        let variants = audio_cpp_variants("someone/else", &items, None).unwrap();
        assert!(variants[0].not_runnable.is_some());
    }

    #[test]
    fn mixed_audio_packages_mark_only_tts_variants_runnable() {
        let supported = [
            "PocketTTS-GGUF",
            "Kokoro-82M-GGUF",
            "Piper-TTS-GGUF",
            "Breeze-TTS-2-GGUF",
            "Qwen3-TTS-12Hz-1.7B-Base-GGUF",
            "Chatterbox-Turbo-GGUF",
            "VibeVoice-1.5B-GGUF",
            "AudioSR-GGUF",
            "BS-RoFormer-ep368-GGUF",
            "Mel-Band-RoFormer-GGUF",
            "SAM-Audio-GGUF",
            "Sidon-GGUF",
            "Apollo-GGUF",
            "UniverSR-GGUF",
            "HTDemucs-GGUF",
            "HTDemucs-6stems-GGUF",
            "Sortformer-Diar-4spk-v1-GGUF",
        ];
        let unsupported = [
            "ACE-Step1.5-GGUF",
            "AST-AudioSet-GGUF",
            "PersonaPlex-GGUF",
            "Phonon-2-GGUF",
            "VibeVoice-ASR-GGUF",
            "Qwen3-ASR-0.6B-GGUF",
            "unknown",
        ];
        let items: Vec<_> = supported
            .iter()
            .chain(&unsupported)
            .map(|folder| json!({"path":format!("{folder}/model.gguf"),"size":10}))
            .collect();
        let variants = audio_cpp_variants("audio-cpp/audio.cpp-gguf", &items, None).unwrap();
        assert_eq!(variants.len(), supported.len() + unsupported.len());
        for v in variants {
            let runnable = supported.contains(&v.name.split('/').next().unwrap());
            let dependency_missing = crate::engine::audio_cpp::family_from_package_path(&v.name)
                .is_some_and(crate::engine::audio_cpp::espeak_unavailable);
            assert_eq!(
                v.not_runnable.is_none(),
                runnable && !dependency_missing,
                "{}",
                v.name
            );
            if dependency_missing {
                assert_eq!(
                    v.not_runnable,
                    Some(crate::hub_tasks::NotRunnable::AudioCppEspeakMissing)
                );
            } else if !runnable {
                assert_eq!(
                    serde_json::to_value(v).unwrap()["not_runnable"],
                    "audio_cpp_unsupported"
                );
            }
        }
    }
    use serde_json::json;

    #[test]
    fn package_variants_keep_each_language_and_family_with_its_companions() {
        let items = [
            "PocketTTS-GGUF/english/pocket-q8_0.gguf",
            "PocketTTS-GGUF/english/embeddings/alba.safetensors",
            "PocketTTS-GGUF/french/pocket-q8_0.gguf",
            "PocketTTS-GGUF/french/embeddings/anna.safetensors",
            "Kokoro-GGUF/kokoro-q8_0.gguf",
            "Kokoro-GGUF/voices.bin",
        ]
        .iter()
        .map(|p| json!({"type":"file", "path":p, "size":10}))
        .collect::<Vec<_>>();
        let variants = audio_cpp_variants("audio-cpp/audio.cpp-gguf", &items, None).unwrap();
        assert_eq!(variants.len(), 3);
        for v in &variants {
            let folder = v.shards[0].filename.rsplit_once('/').unwrap().0;
            assert_eq!(v.shards.len(), 2);
            assert_eq!(v.total_size_bytes, 20);
            assert!(v.shards[0].filename.ends_with(".gguf"));
            assert!(v
                .shards
                .iter()
                .all(|s| s.filename.starts_with(&format!("{folder}/"))));
        }
        let item = json!({"id":"audio-cpp/audio.cpp-gguf", "siblings":[{"rfilename":"PocketTTS-GGUF/english/pocket.gguf"}]});
        let result = parse_search_item(&item).unwrap();
        assert_eq!(
            result.compatibility,
            crate::hub_compat::Compatibility::Runnable {
                runtime: crate::engine::AUDIO_CPP.into()
            }
        );
        assert_eq!(result.pipeline_tag.as_deref(), Some("text-to-speech"));
    }
}

#[cfg(test)]
mod false_positive_tests {
    use super::*;
    use crate::hub_compat::{Compatibility, ModelFormat};
    use crate::hub_tasks::NotRunnable;
    use serde_json::json;

    fn files(paths: &[&str]) -> Vec<serde_json::Value> {
        paths
            .iter()
            .map(|path| json!({"path": path, "type": "file", "size": 1}))
            .collect()
    }

    fn repo(
        id: &str,
        task: &str,
        library: &str,
        config: serde_json::Value,
        paths: &[&str],
    ) -> serde_json::Value {
        json!({"id": id, "pipeline_tag": task, "library_name": library, "tags": ["safetensors", task],
            "config": config, "siblings": paths.iter().map(|p| json!({"rfilename": p})).collect::<Vec<_>>()})
    }

    #[test]
    fn original_vibevoice_needs_a_converted_transformers_package() {
        let paths = [
            "config.json",
            "model-00001-of-00003.safetensors",
            "model-00002-of-00003.safetensors",
            "model-00003-of-00003.safetensors",
            "model.safetensors.index.json",
            "preprocessor_config.json",
        ];
        let mut item = repo(
            "microsoft/VibeVoice-1.5B",
            "text-to-speech",
            "transformers",
            json!({"model_type": "vibevoice", "architectures": ["VibeVoiceForConditionalGeneration"]}),
            &paths,
        );
        let listed = parse_search_item(&item).unwrap();
        assert!(matches!(
            listed.compatibility,
            Compatibility::Unsupported { .. }
        ));
        assert_eq!(listed.not_runnable, Some(NotRunnable::VibevoiceLayout));
        assert_eq!(card_not_runnable(&item, None), listed.not_runnable);
        // The full config cannot be rescued by adding a tokenizer alone.
        item["config"] = json!({"model_type": "vibevoice", "decoder_config": {"model_type": "qwen2"},
            "acoustic_tokenizer_config": {"model_type": "vibevoice_acoustic_tokenizer"}});
        item["siblings"]
            .as_array_mut()
            .unwrap()
            .push(json!({"rfilename": "tokenizer.json"}));
        assert!(crate::hub_compat::unsupported_vibevoice(
            item.get("config"),
            &sibling_files(&item)
        ));
        item["id"] = json!("converted/VibeVoice-1.5B");
        item["config"] = json!({"model_type": "vibevoice", "text_config": {"model_type": "qwen2"}, "audio_config": {}});
        assert!(matches!(parse_search_item(&item).unwrap().compatibility,
            Compatibility::Runnable { runtime } if runtime == crate::engine::PYTHON_HF));
        // The API's abbreviated config is also sufficient when tokenizer files are present.
        item["config"] = json!({"model_type": "vibevoice"});
        assert_eq!(parse_search_item(&item).unwrap().not_runnable, None);
    }

    #[test]
    fn auk_listing_agrees_with_its_nine_unsupported_variants() {
        let paths = [
            "auk-base-f16.gguf",
            "auk-base-f32.gguf",
            "auk-base-q8_0.gguf",
            "auk-flash-f16.gguf",
            "auk-flash-f32.gguf",
            "auk-flash-q8_0.gguf",
            "auk-vae-f32.gguf",
            "qwen2.5-omni-3b-bf16.gguf",
            "qwen2.5-omni-3b-q8_0.gguf",
            "config/auk-base.yaml",
            "config/auk-flash.yaml",
            "tokenizer/tokenizer.json",
            "tokenizer/tokenizer_config.json",
        ];
        let mut item = repo(
            "audio-cpp/AuK-Base-and-Flash-GGUF",
            "text-to-speech",
            "",
            json!({}),
            &paths,
        );
        item["tags"] = json!(["gguf", "text-to-speech"]);
        item["gguf"] = json!({"architecture": "audiocpp"});
        let listed = parse_search_item(&item).unwrap();
        assert!(matches!(
            listed.compatibility,
            Compatibility::Unsupported {
                format: ModelFormat::Gguf
            }
        ));
        assert_eq!(
            listed.not_runnable,
            Some(NotRunnable::AudioCppIncompletePackage)
        );
        assert_eq!(card_not_runnable(&item, None), listed.not_runnable);
        let variants =
            audio_cpp_variants("audio-cpp/AuK-Base-and-Flash-GGUF", &files(&paths), None).unwrap();
        assert_eq!(variants.len(), 9);
        assert!(variants.iter().all(|v| v.not_runnable.is_some()));
    }

    #[test]
    fn sdxl_refiner_uses_only_its_diffusers_folder() {
        let paths = [
            "model_index.json",
            "sd_xl_refiner_1.0.safetensors",
            "sd_xl_refiner_1.0_0.9vae.safetensors",
            "scheduler/scheduler_config.json",
            "text_encoder_2/config.json",
            "text_encoder_2/model.fp16.safetensors",
            "text_encoder_2/model.safetensors",
            "tokenizer_2/merges.txt",
            "tokenizer_2/vocab.json",
            "tokenizer_2/tokenizer_config.json",
            "unet/config.json",
            "unet/diffusion_pytorch_model.fp16.safetensors",
            "unet/diffusion_pytorch_model.safetensors",
            "vae/config.json",
            "vae/diffusion_pytorch_model.fp16.safetensors",
            "vae/diffusion_pytorch_model.safetensors",
        ];
        let item = repo(
            "stabilityai/stable-diffusion-xl-refiner-1.0",
            "image-to-image",
            "diffusers",
            json!({"diffusers": {"_class_name": "StableDiffusionXLImg2ImgPipeline"}}),
            &paths,
        );
        let listed = parse_search_item(&item).unwrap();
        assert_eq!(
            listed.compatibility,
            Compatibility::Runnable {
                runtime: crate::engine::PYTHON_HF.into()
            }
        );
        assert_eq!(listed.not_runnable, None);
        assert!(crate::hub_compat::is_sdxl_refiner(
            &listed.id,
            &sibling_files(&item)
        ));
        let variants = diffusers_variants(&listed.id, &files(&paths)).unwrap();
        assert_eq!(variants.len(), 1);
        let shards = &variants[0].shards;
        assert!(shards.iter().any(|s| s.filename == "model_index.json"));
        assert!(shards
            .iter()
            .any(|s| s.filename == "unet/diffusion_pytorch_model.fp16.safetensors"));
        assert!(!shards
            .iter()
            .any(|s| s.filename.starts_with("sd_xl_refiner")));
        for path in &paths[1..3] {
            assert!(crate::image_bundles::bundle_for(path).is_none());
        }
        let bare = repo(
            "owner/refiner",
            "image-to-image",
            "diffusers",
            json!({}),
            &paths[1..3],
        );
        let listed = parse_search_item(&bare).unwrap();
        assert!(matches!(
            listed.compatibility,
            Compatibility::Unsupported { .. }
        ));
        assert_eq!(
            listed.not_runnable,
            Some(NotRunnable::SdxlRefinerFolderRequired)
        );
        assert!(diffusers_variants(&listed.id, &files(&paths[1..3]))
            .unwrap()
            .is_empty());
        let mut unknown = item;
        unknown["siblings"] = json!([]);
        assert_eq!(
            parse_search_item(&unknown).unwrap().not_runnable,
            Some(NotRunnable::SdxlRefinerFolderRequired)
        );
    }

    fn uncensored_tree() -> Vec<serde_json::Value> {
        [
            (
                "split/diffusion_models/qwen-image-edit-2511-uncensored-Q4_K_M.gguf",
                11657701984u64,
            ),
            (
                "split/diffusion_models/qwen-image-edit-2511-uncensored-Q8_0.gguf",
                21723847264,
            ),
            (
                "split/text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf",
                1354162944,
            ),
            (
                "split/text_encoders/Qwen2.5-VL-7B-Instruct-q4_0.gguf",
                4431389248,
            ),
            ("split/vae/qwen_image_vae.safetensors", 253806246),
        ]
        .iter()
        .map(|(p, n)| json!({"type": "file", "path": p, "size": n, "lfs": {"size": n}}))
        .collect()
    }

    #[test]
    fn a_repos_text_encoder_and_projector_are_not_main_models() {
        let variants = DownloadManager::file_variants(
            "o/qwen-image-edit-uncensored-GGUF",
            &uncensored_tree(),
            false,
        )
        .unwrap();
        let quant = |needle: &str| {
            variants
                .iter()
                .find(|v| v.shards[0].filename.contains(needle))
                .unwrap()
                .quantization
                .clone()
        };
        assert_eq!(quant("Q4_K_M"), "Q4_K_M");
        assert_eq!(quant("Q8_0"), "Q8_0");
        assert_eq!(quant("mmproj"), "mmproj");
        assert_eq!(quant("Instruct-q4_0"), "component");
        // The real models come first, the parts last.
        assert!(variants[..2]
            .iter()
            .all(|v| v.shards[0].filename.contains("diffusion_models")));
    }

    #[test]
    fn a_repos_own_projector_is_chosen_over_the_curated_one() {
        assert_eq!(
            own_projector_from(
                "split/diffusion_models/qwen-image-edit-2511-Q4_K_M.gguf",
                &uncensored_tree()
            )
            .as_deref(),
            Some("split/text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf")
        );
        // Family compatibility takes precedence over precision.
        let two: Vec<_> = ["Qwen2.5-VL-mmproj-Q8_0.gguf", "Qwen3-VL-mmproj-F16.gguf"]
            .iter()
            .map(|p| json!({"type": "file", "path": p}))
            .collect();
        assert_eq!(
            own_projector_from("qwen-image-edit-2511-Q4_K_M.gguf", &two).as_deref(),
            Some("Qwen2.5-VL-mmproj-Q8_0.gguf")
        );
        assert_eq!(
            own_projector_from(
                "qwen-image-edit-2511-Q4_K_M.gguf",
                &[json!({"type": "file", "path": "m-Q4_K_M.gguf"})]
            ),
            None
        );
        let tree = uncensored_tree();
        let variant = companion_variant_from(
            "o/qwen-image-edit-uncensored-GGUF",
            "split/text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf",
            &tree,
        )
        .unwrap()
        .unwrap();
        assert_eq!(variant.total_size_bytes, 1354162944);
    }

    #[test]
    fn projector_download_and_runtime_agree_on_layout_and_family() {
        let lib = tempfile::tempdir().unwrap();
        let repo = lib.path().join("owner/repo");
        for (main, candidate, accepted) in [
            (
                "qwen-image-edit-2511-Q4_K_M.gguf",
                "mmproj/Qwen2.5-VL-mmproj-f16.gguf",
                false,
            ),
            (
                "split/diffusion_models/qwen-image-edit-2511-Q4_K_M.gguf",
                "split/text_encoders/Qwen2_5_VL-mmproj-f16.gguf",
                true,
            ),
            (
                "qwen-image-edit-2511-Q4_K_M.gguf",
                "Qwen2.5VL-mmproj-f16.gguf",
                true,
            ),
        ] {
            let main_path = repo.join(main);
            let path = repo.join(candidate);
            std::fs::create_dir_all(main_path.parent().unwrap()).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&main_path, b"x").unwrap();
            std::fs::write(&path, b"x").unwrap();
            let items = vec![json!({"path": candidate, "type": "file"})];
            let download = own_projector_from(main, &items).map(|p| repo.join(p));
            let runtime =
                crate::image_bundles::own_projector(&main_path, &[lib.path().to_path_buf()]);
            assert_eq!(download.is_some(), accepted, "{candidate}");
            assert_eq!(download, runtime, "{candidate}");
            std::fs::remove_file(path).unwrap();
        }
        assert!(own_projector_from(
            "qwen-image-edit-2511-Q4_K_M.gguf",
            &[json!({"path": "Qwen3-VL-mmproj-F16.gguf"})]
        )
        .is_none());
    }

    #[test]
    fn component_only_repos_keep_quantization_labels() {
        let items = vec![
            json!({"path": "vae/weights-Q8_0.gguf", "type": "file", "size": 1}),
            json!({"path": "text_encoder/weights-Q4_0.gguf", "type": "file", "size": 1}),
        ];
        let variants = DownloadManager::file_variants("o/repo", &items, false).unwrap();
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().all(|v| v.quantization != "component"));
        assert!(variants.iter().any(|v| v.quantization == "Q8_0"));
        assert!(variants.iter().any(|v| v.quantization == "Q4_0"));
    }

    #[test]
    fn component_folders_are_told_apart() {
        assert!(in_component_folder("split/text_encoders/x.gguf"));
        assert!(in_component_folder("vae/x.gguf"));
        assert!(!in_component_folder("split/diffusion_models/x.gguf"));
        assert!(!in_component_folder("vae.gguf"));
    }

    #[test]
    fn qwen_edit_2511_projector_resolves_from_the_companion_tree() {
        let tree = vec![
            json!({"type": "file", "path": "Qwen2.5-VL-7B-Instruct.mmproj-Q8_0.gguf", "size": 853119712,
            "lfs": {"size": 853119712, "oid": "2185ca0adf339f96845318c7b666f5d242f86bbace5657a89cb834785d0cea57"}}),
        ];
        for path in [
            "qwen-image-edit-2511-Q4_K_M.gguf",
            "split/diffusion_models/qwen-image-edit-2511-uncensored-Q4_K_M.gguf",
        ] {
            let bundle = crate::image_bundles::bundle_for(path).unwrap();
            assert_eq!(
                bundle.components.iter().map(|c| c.role).collect::<Vec<_>>(),
                ["vae", "llm", "llm_vision"]
            );
            let projector = bundle
                .components
                .iter()
                .find(|c| c.role == "llm_vision")
                .unwrap();
            assert_eq!(projector.repo, "mradermacher/Qwen2.5-VL-7B-Instruct-GGUF");
            let variant = companion_variant_from(projector.repo, projector.file, &tree)
                .unwrap()
                .unwrap();
            assert_eq!(variant.total_size_bytes, 853119712);
            assert_eq!(variant.shards[0].filename, projector.file);
        }
    }
}

#[cfg(test)]
#[path = "downloader_g8_tests.rs"]
mod g8_tests;

#[cfg(test)]
mod refusal_reason_tests {
    use super::*;
    use crate::hub_tasks::NotRunnable;

    fn listed(id: &str) -> HfModelSearchResult {
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/hf-refusal-repos.json")).unwrap();
        parse_search_item(&all[id]).unwrap()
    }

    fn reason(id: &str) -> Option<NotRunnable> {
        listed(id).not_runnable
    }

    #[test]
    fn custom_speech_checkpoints_need_their_own_package() {
        for id in [
            "hexgrad/Kokoro-82M",
            "coqui/XTTS-v2",
            "Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice",
            "ResembleAI/chatterbox",
            "SWivid/F5-TTS",
            "onnx-community/Kokoro-82M-v1.0-ONNX",
            "myshell-ai/MeloTTS-English",
            "mistralai/Voxtral-Mini-4B-Realtime-2602",
        ] {
            assert_eq!(reason(id), Some(NotRunnable::NeedsOwnPackage), "{id}");
        }
    }

    #[test]
    fn vocoder_and_decoder_components_are_component_only() {
        for id in [
            "speechbrain/tts-hifigan-libritts-22050Hz",
            "black-forest-labs/FLUX.2-small-decoder",
        ] {
            assert_eq!(reason(id), Some(NotRunnable::ComponentOnly), "{id}");
        }
    }

    #[test]
    fn pyannote_is_pipeline_config_only() {
        assert_eq!(
            reason("pyannote/speaker-diarization-3.1"),
            Some(NotRunnable::PipelineConfigOnly)
        );
    }

    #[test]
    fn faster_whisper_is_a_ctranslate2_export() {
        assert_eq!(
            reason("Systran/faster-whisper-small"),
            Some(NotRunnable::Ctranslate2Export)
        );
    }

    #[test]
    fn core_ml_and_mlx_repos_are_apple_only_off_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        for id in [
            "argmaxinc/whisperkit-coreml",
            "mlx-community/parakeet-tdt-0.6b-v3",
        ] {
            assert_eq!(reason(id), Some(NotRunnable::AppleOnly), "{id}");
        }
    }

    #[test]
    fn unknown_speech_model_type_is_an_unsupported_architecture() {
        // A transformers-style speech repo whose model_type no engine serves and that is not
        // one of the known own-package families.
        let item = serde_json::json!({
            "id": "someone/odd-asr",
            "pipeline_tag": "automatic-speech-recognition",
            "library_name": "transformers",
            "tags": ["transformers", "safetensors"],
            "config": {"model_type": "brand_new_asr", "architectures": ["BrandNewAsr"]},
            "siblings": [
                {"rfilename": "config.json"},
                {"rfilename": "model.safetensors"}
            ]
        });
        let listed = parse_search_item(&item).unwrap();
        assert_eq!(
            listed.not_runnable,
            Some(NotRunnable::UnsupportedArchitecture)
        );
    }

    #[test]
    fn transformers_language_model_with_safetensors_only_is_convertible() {
        // Remote code: neither mistral.rs nor the Python engine runs it as it is.
        let item = serde_json::json!({
            "id": "someone/Odd-LM-7B",
            "pipeline_tag": "text-generation",
            "library_name": "transformers",
            "tags": ["transformers", "safetensors", "text-generation", "custom_code"],
            "config": {"model_type": "oddlm", "architectures": ["OddLMForCausalLM"]},
            "siblings": [
                {"rfilename": "config.json"},
                {"rfilename": "model-00001-of-00002.safetensors"},
                {"rfilename": "model-00002-of-00002.safetensors"}
            ]
        });
        assert_eq!(
            parse_search_item(&item).unwrap().not_runnable,
            Some(NotRunnable::SafetensorsOnly)
        );
    }

    #[test]
    fn plain_gguf_less_language_model_keeps_no_gguf() {
        // PyTorch pickle weights only: no engine, no conversion hint, nothing more precise.
        let item = serde_json::json!({
            "id": "someone/old-lm",
            "pipeline_tag": "text-generation",
            "tags": ["pytorch", "text-generation"],
            "siblings": [
                {"rfilename": "config.json"},
                {"rfilename": "pytorch_model.bin"}
            ]
        });
        assert_eq!(
            parse_search_item(&item).unwrap().not_runnable,
            Some(NotRunnable::NoGguf)
        );
    }

    #[test]
    fn apple_files_are_not_refused_as_apple_only_on_a_mac() {
        use crate::hub_tasks::{refine_no_gguf, RefusalFacts};
        let files = vec!["AudioEncoder.mlmodelc/model.mil".to_string()];
        let facts = |apple| RefusalFacts {
            repo_id: "x/whisperkit-coreml",
            tags: &[],
            files: &files,
            library: Some("whisperkit"),
            task: Some("automatic-speech-recognition"),
            format: crate::hub_compat::ModelFormat::CoreMl,
            config: None,
            apple,
        };
        assert_eq!(refine_no_gguf(&facts(false)), NotRunnable::AppleOnly);
        assert_eq!(refine_no_gguf(&facts(true)), NotRunnable::NoGguf);
    }
}
