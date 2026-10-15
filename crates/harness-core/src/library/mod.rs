//! "My Models" (Kütüphanem): the local model files as the library screen shows them.
//!
//! Everything here is derived from a scan ([`crate::models::ModelManager::scan_models`]) plus
//! the user's state in `state.db` (`model_state`: favorite, last use, arrival time):
//! * [`params`]: parameter count (exact from the GGUF tensor table, else an estimate).
//! * capabilities (tools / reasoning from the chat template, vision / audio from a matching
//!   projector, embedding / rerank from the kind).
//! * [`draft`]: which small local LLMs can draft for a bigger one (same tokenizer).
//! * [`dupes`]: identical files (size + first/last MiB pre-hash, full SHA-256 on demand).
//! * [`ops`]: reveal in folder, delete preview and safe delete, shard repair, the in-use
//!   check and recording the last use when a server starts.

macro_rules! bench_step {
    ($label:expr) => {
        #[cfg(test)]
        crate::models::scan_bench::step($label);
    };
}

pub mod draft;
pub mod dupes;
pub mod ops;
pub mod params;
pub mod sources;
#[cfg(test)]
mod tests;

use crate::domain::{ModelDefinition, ModelKind};
use crate::models::LocalModel;
use crate::state::{AliasUse, ModelState, StateDb};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use params::ParamInfo;

/// What a model can do, from its header, template and companion files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelCaps {
    /// Has a chat template (text generation).
    pub chat: bool,
    /// The chat template handles `tools`.
    pub tools: bool,
    /// The chat template can reason (see `gguf::template_supports_reasoning`).
    pub reasoning: bool,
    #[serde(default)]
    pub reasoning_control: crate::reasoning::ReasoningControl,
    /// A vision projector belongs to it (matched file, linked projector or a definition).
    pub vision: bool,
    /// Its projector has an audio encoder.
    pub audio: bool,
    /// Audio content parts accepted by the model, independent of vision.
    #[serde(default)]
    pub audio_input: bool,
    pub embedding: bool,
    pub rerank: bool,
    /// Speech recognition (Whisper, sherpa-onnx or an ONNX CTC package).
    #[serde(default)]
    pub asr: bool,
    /// Image / video generation (a stable-diffusion.cpp model).
    #[serde(default)]
    pub image: bool,
    /// Speech synthesis (a Kokoro or Piper folder run by sherpa-onnx).
    #[serde(default)]
    pub tts: bool,
    /// What `/props` of a real run of this file reported; `None` until it was loaded once.
    /// Without it `tools` / `reasoning` come from the chat template text.
    #[serde(default)]
    pub verified: Option<VerifiedCaps>,
}

/// Chat-template capabilities as llama-server itself reported them (`/props`
/// `chat_template_caps`), recorded by the capability registry on every start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VerifiedCaps {
    /// `supports_tools`.
    pub tools: bool,
    /// The template takes a reasoning level (`supports_reasoning_effort`) or keeps the
    /// reasoning of earlier turns (`supports_preserve_reasoning`). Reporting `false` is not
    /// conclusive (templates that think but drop it from history report it too), so a
    /// template-detected reasoning ability is not taken back.
    pub reasoning: bool,
    #[serde(default)]
    pub reasoning_control: crate::reasoning::ReasoningControl,
    /// The chat template the server really used (`/props.chat_template`) lets the model
    /// reason: the fallback when the flags above stay `false` (hybrid Qwen3 reports neither,
    /// yet its template switches thinking with `enable_thinking`). `None`: no template sent.
    #[serde(default)]
    pub template_reasoning: Option<bool>,
    /// Unix seconds of that run.
    pub observed_at: u64,
}

impl VerifiedCaps {
    /// From one `/props` observation; `None` when it carries no chat-template caps (an
    /// embedding / rerank run, or a build that does not report them).
    pub fn from_observation(obs: &crate::capabilities::ModelObservation) -> Option<Self> {
        let caps = &obs.chat_template_caps;
        let get = |k: &str| caps.get(k).copied();
        if get("supports_tools").is_none()
            && get("supports_reasoning_effort").is_none()
            && get("supports_preserve_reasoning").is_none()
            && obs.template_reasoning.is_none()
        {
            return None;
        }
        Some(Self {
            tools: get("supports_tools").unwrap_or(false),
            reasoning: get("supports_reasoning_effort").unwrap_or(false)
                || get("supports_preserve_reasoning").unwrap_or(false),
            template_reasoning: obs.template_reasoning,
            reasoning_control: obs.reasoning_control,
            observed_at: obs.observed_at,
        })
    }
}

/// Folds `/props` observations of real runs into the entries: `verified` is set, `tools`
/// follows the server's own answer, `reasoning` is raised when the server confirms it or the
/// template it served has the reasoning switches.
/// `observe` maps a model path to its newest observation (the capability registry).
pub fn apply_observations(
    entries: &mut [LibraryEntry],
    observe: &dyn Fn(&str) -> Option<crate::capabilities::ModelObservation>,
) {
    for e in entries
        .iter_mut()
        .filter(|e| e.model.kind == ModelKind::Llm)
    {
        let Some(v) = observe(&e.model.path.to_string_lossy())
            .as_ref()
            .and_then(VerifiedCaps::from_observation)
        else {
            continue;
        };
        e.caps.tools = v.tools;
        e.caps.reasoning |= v.reasoning || v.template_reasoning == Some(true);
        if v.template_reasoning == Some(false) || v.reasoning_control.supported() {
            e.caps.reasoning_control = v.reasoning_control;
        }
        e.caps.verified = Some(v);
    }
}

/// Parts of a split model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ShardStatus {
    pub total: u32,
    pub present: u32,
    /// File names of the missing parts.
    pub missing: Vec<String>,
}

/// Where a missing part could come from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RepairInfo {
    /// Hugging Face repo, known from the download layout `<library>/<owner>/<repo>/...`.
    pub repo_id: Option<String>,
    /// Folder the parts go to (`<library>/<owner>/<repo>`).
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub target_dir: Option<PathBuf>,
    /// Why it cannot be repaired automatically (`None` = it can).
    pub reason: Option<String>,
    /// Address of a part that is still there, recorded when the file was downloaded (custom
    /// URL, or a Hugging Face file that no longer sits in its repo folder). The missing parts
    /// are fetched from the same address with the part number replaced.
    #[serde(default)]
    pub source_url: Option<String>,
    /// The user can give the address of one part to repair with (the file is in a library
    /// folder and not linked): offered when there is neither a repo folder nor a record.
    #[serde(default)]
    pub url_allowed: bool,
}

/// One local model file (or split set) as the library shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LibraryEntry {
    pub model: LocalModel,
    pub params: ParamInfo,
    pub caps: ModelCaps,
    /// Capabilities, format and runtime of the file as the model registry joins them
    /// ([`crate::model_registry`]); the same record the Hub builds for a repo.
    pub record: crate::model_registry::ModelRecord,
    /// LLM: projector files that belong to it. Projector: the LLMs it belongs to (ids).
    pub pairs: Vec<String>,
    /// LLM with a projector (or vision in a definition): "VLM / multimodal".
    pub multimodal: bool,
    /// Split models only.
    pub shards: Option<ShardStatus>,
    /// Incomplete split models only.
    pub repair: Option<RepairInfo>,
    /// Hugging Face repo (`owner/name`) the file was downloaded from, read from its folder
    /// (`<library>/<owner>/<repo>/…`); the UI shows the owner's avatar.
    pub repo_id: Option<String>,
    pub favorite: bool,
    pub last_used_at: Option<i64>,
    pub last_definition_id: Option<String>,
    /// Preset used last: the newest conversation with one of its aliases, else the default
    /// preset of the definition that started it last.
    pub preset_used: Option<String>,
    /// When the file arrived (Unix ms; download completion or copy time).
    pub added_at: Option<i64>,
    /// Bigger local LLMs this one can draft for (ids).
    pub draft_for: Vec<String>,
}

/// The library listing: entries plus a note when the user state could not be read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LibraryView {
    pub entries: Vec<LibraryEntry>,
    /// `state.db` unavailable or read-only: favorites / last use are missing or cannot change.
    pub state_error: Option<String>,
}

/// Newest modification time of the model's files (Unix ms). A finished download is renamed
/// into place, so this is its completion time; copies keep (Windows) or refresh the time.
pub fn arrival_time(model: &LocalModel) -> Option<i64> {
    model
        .shard_paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

/// Scans the library and joins the user state. `db` is `state::shared()`; without it the
/// entries are still listed.
pub fn list(db: Option<&StateDb>, state_error: Option<String>) -> LibraryView {
    let models = crate::models::ModelManager.scan_models();
    let defs = crate::domain::store::Domain::global()
        .definitions_for_models(&models)
        .unwrap_or_default();
    let roots = crate::config::AppSettings::load().all_models_dirs();
    list_from(models, &defs, &roots, db, state_error)
}

/// [`list`] over given models / definitions / library roots (tests use temp dirs).
pub fn list_from(
    models: Vec<LocalModel>,
    defs: &[ModelDefinition],
    roots: &[PathBuf],
    db: Option<&StateDb>,
    mut state_error: Option<String>,
) -> LibraryView {
    let mut states = Vec::new();
    let mut alias_uses = HashMap::new();
    if let Some(db) = db {
        match db.model_states() {
            Ok(s) => states = s,
            Err(e) => {
                state_error.get_or_insert(e.to_string());
            }
        }
        bench_step!("model_states query");
        // Existing arrival times are immutable. Avoid both file stats and a write
        // transaction on a steady-state listing; newly seen files are one batch.
        if !db.is_read_only() {
            let noted: std::collections::HashSet<&str> = states
                .iter()
                .filter(|s| s.added_at.is_some())
                .map(|s| s.model_id.as_str())
                .collect();
            let arrived: Vec<_> = models
                .iter()
                .filter_map(|m| {
                    let key = state_key(&m.path);
                    if noted.contains(key.as_str()) {
                        return None;
                    }
                    Some((key, arrival_time(m)?))
                })
                .collect();
            bench_step!("arrival metadata (missing rows only)");
            if !arrived.is_empty() {
                match db.note_models_added(&arrived) {
                    Ok(()) => match db.model_states() {
                        Ok(s) => states = s,
                        Err(e) => {
                            state_error.get_or_insert(e.to_string());
                        }
                    },
                    Err(e) => {
                        state_error.get_or_insert(e.to_string());
                    }
                }
            }
        }
        bench_step!("note_models_added + refresh if new");
        alias_uses = db.latest_alias_uses().unwrap_or_default();
        if db.is_read_only() {
            state_error.get_or_insert_with(|| {
                crate::state::StateError::ReadOnly {
                    found: db.user_version().unwrap_or(0),
                }
                .to_string()
            });
        }
    }
    bench_step!("latest_alias_uses query");
    let mut entries = build_entries(models, &states, defs, &alias_uses, roots);
    apply_observations(&mut entries, &|p| {
        crate::capabilities::registry().observation_for(p)
    });
    bench_step!("capability observation_for loop");
    LibraryView {
        entries,
        state_error,
    }
}

/// Key of a model file in `state.db` (`model_state.model_id`): the normalized launch path
/// (see [`norm`]), so the same file always maps to one row whatever separators or case a
/// caller used.
pub fn state_key(path: &Path) -> String {
    norm(path)
}

/// Comparable form of a path without touching the disk: lexically normalized, `/`
/// separators, lower case on Windows (case-insensitive file system).
pub(crate) fn norm(p: &Path) -> String {
    let s = crate::paths::normalize(p)
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// Joins scanned models with their state, definitions and each other (pure).
pub fn build_entries(
    models: Vec<LocalModel>,
    states: &[ModelState],
    defs: &[ModelDefinition],
    alias_uses: &HashMap<String, AliasUse>,
    roots: &[PathBuf],
) -> Vec<LibraryEntry> {
    let state_of: HashMap<String, &ModelState> = states
        .iter()
        .map(|s| (norm(Path::new(&s.model_id)), s))
        .collect();
    bench_step!("state index");
    let param_infos: Vec<ParamInfo> = models.iter().map(params::param_info).collect();

    bench_step!("params::param_info loop");

    // Resolve each repository once: repo_of checks containment on disk (including
    // symlinks). Repeating it for every LLM/projector pair is expensive on Windows.
    let repo_lookup = ops::RepoLookup::new(roots);
    let repos: Vec<_> = models.iter().map(|m| repo_lookup.repo_of(m)).collect();
    let mut by_dir: HashMap<Option<&Path>, Vec<usize>> = HashMap::new();
    let mut by_repo: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, m) in models
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == ModelKind::Mmproj)
    {
        by_dir.entry(m.path.parent()).or_default().push(i);
        if let Some(repo) = repos[i].as_deref() {
            by_repo.entry(repo).or_default().push(i);
        }
    }
    let mut pairs: Vec<Vec<String>> = vec![Vec::new(); models.len()];
    for (i, m) in models
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == ModelKind::Llm)
    {
        let same_dir = by_dir
            .get(&m.path.parent())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut candidates = same_dir.to_vec();
        if let Some(indices) = repos[i].as_deref().and_then(|r| by_repo.get(r)) {
            candidates.extend(indices);
        }
        // Preserve scan order, including when directory and repository both match.
        candidates.sort_unstable();
        candidates.dedup();
        for j in candidates {
            if projector_matches_in_repo(m, &models[j], same_dir.len(), roots, true) {
                pairs[i].push(models[j].id.clone());
                pairs[j].push(m.id.clone());
            }
        }
    }
    bench_step!("projector pairs grouping");
    let draft_for = draft::draft_targets(&models, &param_infos);
    bench_step!("draft grouping");
    // What each projector adds to the LLMs it belongs to: (vision, audio).
    let projector_caps: HashMap<String, (bool, bool)> = models
        .iter()
        .filter(|m| m.kind == ModelKind::Mmproj)
        .map(|m| {
            let c = caps_of(m, false, false);
            (m.id.clone(), (c.vision, c.audio))
        })
        .collect();

    let mut defs_by_path: HashMap<String, Vec<&ModelDefinition>> = HashMap::new();
    for d in defs {
        if let Some(path) = d.base.local_path() {
            defs_by_path.entry(norm(path)).or_default().push(d);
        }
    }
    let entries = models
        .into_iter()
        .enumerate()
        .map(|(i, model)| {
            let file = norm(&model.path);
            let st = state_of.get(&file).copied();
            let my_defs = defs_by_path
                .get(&file)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let paired = |f: fn(&(bool, bool)) -> bool| {
                pairs[i]
                    .iter()
                    .any(|p| projector_caps.get(p).is_some_and(f))
            };
            let linked: Vec<_> = my_defs
                .iter()
                .filter_map(|d| d.mmproj.as_deref())
                .chain(model.mmproj_path.as_deref())
                .filter_map(|p| crate::gguf::read_gguf_metadata(p).ok())
                .collect();
            let vision = paired(|c| c.0)
                || linked
                    .iter()
                    .any(|g| g.has_vision_encoder || !g.has_audio_encoder);
            let audio = paired(|c| c.1) || linked.iter().any(|g| g.has_audio_encoder);
            let caps = caps_of(&model, vision, audio);
            let multimodal = model.kind == ModelKind::Llm && (caps.vision || caps.audio);

            // Preset used: newest conversation with one of its aliases, unless a later start
            // recorded the definition's default preset.
            let conv = my_defs
                .iter()
                .filter_map(|d| alias_uses.get(&d.alias.to_lowercase()))
                .max_by_key(|u| u.updated_at);
            let preset_used = match (conv, st) {
                (Some(u), Some(s)) if s.last_used_at.is_some_and(|t| t > u.updated_at) => {
                    s.last_preset_id.clone().or_else(|| u.preset_id.clone())
                }
                (Some(u), _) if u.preset_id.is_some() => u.preset_id.clone(),
                (_, Some(s)) => s.last_preset_id.clone(),
                _ => None,
            };

            let shards = model.is_split.then(|| shard_status(&model));
            let repair = shards
                .as_ref()
                .filter(|s| !s.missing.is_empty())
                .map(|_| ops::repair_info(&model, roots));
            let repo_id = repos[i].clone();
            LibraryEntry {
                repo_id,
                params: param_infos[i].clone(),
                record: crate::model_registry::local_record(&model, &caps),
                caps,
                pairs: std::mem::take(&mut pairs[i]),
                multimodal,
                shards,
                repair,
                favorite: st.is_some_and(|s| s.favorite),
                last_used_at: st.and_then(|s| s.last_used_at),
                last_definition_id: st.and_then(|s| s.last_definition_id.clone()),
                preset_used,
                added_at: st.and_then(|s| s.added_at).or_else(|| arrival_time(&model)),
                draft_for: draft_for[i].clone(),
                model,
            }
        })
        .collect();
    bench_step!("build entries (definitions, roots, records)");
    entries
}

/// Capabilities of a file; for LLMs `vision` / `audio` come from their projectors.
fn caps_of(model: &LocalModel, vision: bool, audio: bool) -> ModelCaps {
    let g = model.gguf.as_ref();
    let llm = model.kind == ModelKind::Llm;
    // A safetensors checkpoint (mistral.rs, MLX): its folder tells the template and the vision
    // tower.
    if llm && g.is_none() {
        if let Some(info) = crate::engine::mistralrs_inspect(&model.path)
            .or_else(|| crate::engine::mlx::inspect(&model.path))
        {
            return ModelCaps {
                chat: info.chat_template,
                vision: info.vision,
                ..Default::default()
            };
        }
    }
    // An onnxruntime-genai model has no GGUF header; the folder's chat template says it chats.
    let genai = llm && g.is_none() && harness_onnx::genai_pkg::is_chat_model(&model.path);
    let chat = genai || (llm && g.is_some_and(|g| g.has_chat_template));
    let proj = model.kind == ModelKind::Mmproj;
    // Older projectors carry neither flag: they are vision projectors.
    let proj_audio = proj && g.is_some_and(|g| g.has_audio_encoder);
    let proj_vision = proj && g.is_some_and(|g| g.has_vision_encoder || !g.has_audio_encoder);
    ModelCaps {
        chat,
        tools: chat && g.is_some_and(|g| g.template_supports_tools),
        reasoning: chat && g.is_some_and(|g| g.template_supports_reasoning),
        reasoning_control: if chat {
            g.map(|g| g.reasoning_control).unwrap_or_default()
        } else {
            Default::default()
        },
        vision: (llm && vision) || proj_vision,
        audio: (llm && audio) || proj_audio,
        audio_input: (llm && audio) || proj_audio,
        embedding: model.kind == ModelKind::Embedding,
        rerank: model.kind == ModelKind::Reranker,
        asr: model.kind == ModelKind::Asr,
        image: model.kind == ModelKind::Image,
        tts: model.kind == ModelKind::Tts,
        verified: None,
    }
}

fn shard_status(model: &LocalModel) -> ShardStatus {
    let total = model.total_shards.unwrap_or(1);
    let present: std::collections::HashSet<u32> = model
        .shard_paths
        .iter()
        .filter_map(|p| shard_index(p))
        .collect();
    let base = model
        .filename
        .strip_suffix(&format!("-00001-of-{:05}.gguf", total))
        .unwrap_or(&model.name)
        .to_string();
    ShardStatus {
        total,
        present: present.len() as u32,
        missing: (1..=total)
            .filter(|i| !present.contains(i))
            .map(|i| format!("{}-{:05}-of-{:05}.gguf", base, i, total))
            .collect(),
    }
}

/// `3` for `...-00003-of-00005.gguf`.
pub(crate) fn shard_index(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".gguf")
        .or_else(|| name.strip_suffix(".GGUF"))?;
    let mut parts = stem.rsplitn(4, '-');
    let _total = parts.next()?;
    if parts.next()? != "of" {
        return None;
    }
    parts.next()?.parse().ok()
}

/// Lower-case file stem without the projector marker and the quant tag
/// ("mmproj-gemma-4-12B-it-BF16" -> "gemma-4-12b-it").
fn projector_core(proj: &LocalModel) -> String {
    let stem = proj.name.to_lowercase();
    let mut core = stem.as_str();
    for p in ["mmproj-", "mmproj_", "mmproj."] {
        core = core.strip_prefix(p).unwrap_or(core);
    }
    for s in ["-mmproj", "_mmproj", ".mmproj"] {
        if let Some(i) = core.find(s) {
            core = &core[..i];
        }
    }
    let quant = crate::gguf::extract_quant_from_filename(core).to_lowercase();
    let mut out = core.to_string();
    if quant != "unknown" {
        if let Some(i) = out.rfind(&quant) {
            out.truncate(i);
        }
    }
    out.trim_matches(|c| c == '-' || c == '_' || c == '.')
        .to_string()
}

/// Whether projector `proj` belongs to `llm` (both in the same folder): same
/// `general.basename`, or the projector's name (minus marker and quant) starts the LLM's
/// name, or it is the only projector in a model's own folder (not a library root).
/// Verified on real LM Studio folders (Gemma 4, Qwen3.6: both rules match).
pub fn projector_matches(
    llm: &LocalModel,
    proj: &LocalModel,
    projectors_in_dir: usize,
    roots: &[PathBuf],
) -> bool {
    let repo = ops::repo_of(llm, roots);
    let same_repo = repo.is_some() && repo == ops::repo_of(proj, roots);
    projector_matches_in_repo(llm, proj, projectors_in_dir, roots, same_repo)
}

fn projector_matches_in_repo(
    llm: &LocalModel,
    proj: &LocalModel,
    projectors_in_dir: usize,
    roots: &[PathBuf],
    same_repo: bool,
) -> bool {
    let same_dir = llm.path.parent() == proj.path.parent();
    if !same_dir && !same_repo {
        return false;
    }
    let base = |m: &LocalModel| {
        m.gguf
            .as_ref()
            .and_then(|g| g.basename.clone())
            .map(|b| b.to_lowercase())
    };
    if let (Some(a), Some(b)) = (base(llm), base(proj)) {
        if a == b {
            return true;
        }
    }
    let core = projector_core(proj);
    if core.len() >= 3 && llm.name.to_lowercase().starts_with(&core) {
        return true;
    }
    let dir = llm.path.parent();
    let is_root = dir.is_some_and(|d| roots.iter().any(|r| norm(r) == norm(d)));
    same_dir && projectors_in_dir == 1 && !is_root
}

/// The projector file proposed for a model in the launch form and the definition editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectorSuggestion {
    pub path: PathBuf,
    pub name: String,
}

/// The projector that belongs to the LLM at `model_path`: the same pairing the library uses
/// ([`projector_matches`], projectors in the model's own folder). A model linked from Ollama /
/// LM Studio brings its own (`mmproj_path`) and wins. Several fitting projectors (F16, BF16...):
/// the matching quantization, then F16 / Q8, else the first by name. `None` for anything but an LLM, or without a match.
pub fn suggest_projector(model_path: &Path, models: &[LocalModel]) -> Option<ProjectorSuggestion> {
    let key = norm(model_path);
    let llm = models
        .iter()
        .find(|m| norm(&m.path) == key && m.kind == ModelKind::Llm)?;
    if let Some(own) = &llm.mmproj_path {
        let name = own.file_stem()?.to_string_lossy().to_string();
        return Some(ProjectorSuggestion {
            path: own.clone(),
            name,
        });
    }
    // Library roots are flat folders of many models: a lone projector there proves nothing.
    let roots: Vec<PathBuf> = if llm.root_dir.is_empty() {
        Vec::new()
    } else {
        vec![PathBuf::from(&llm.root_dir)]
    };
    let projectors: Vec<&LocalModel> = models
        .iter()
        .filter(|m| m.kind == ModelKind::Mmproj)
        .collect();
    let mut fitting: Vec<&LocalModel> = projectors
        .iter()
        .copied()
        .filter(|p| {
            projector_matches(
                llm,
                p,
                projectors
                    .iter()
                    .filter(|p| p.path.parent() == llm.path.parent())
                    .count(),
                &roots,
            )
        })
        .collect();
    fitting.sort_by_key(|p| {
        (
            p.quantization != llm.quantization,
            !p.name.to_lowercase().contains("f16") || p.name.to_lowercase().contains("bf16"),
            !p.name.to_lowercase().contains("q8"),
            p.name.clone(),
        )
    });
    fitting.first().map(|p| ProjectorSuggestion {
        path: p.path.clone(),
        name: p.name.clone(),
    })
}
