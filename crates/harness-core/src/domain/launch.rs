//! Effective launch configuration: layering and the mapping between the typed
//! [`LaunchConfig`] and catalog values.
//!
//! A *layer* is a [`ParamValues`] map keyed by catalog ids (`params.toml`). The typed
//! `LaunchConfig` fields have catalog ids too (`legacy_field` entries: `gpu_layers`,
//! `cache_type_k`, `mmproj_path`...). Three Harness-level keys have no flag of their own:
//! [`CUSTOM_ARGS`] (the free-form extra arguments), [`LORA_SCALE`] (scale of `lora_path`) and
//! [`RUNTIME`] (which llama-server build runs the launch).
//! In an override layer an empty text value means "unset" (back to llama-server's default).
//!
//! Precedence: [`LaunchDefaults`] ← `ModelDefinition` ← `ServerProfile.serve_overrides` ←
//! transient overrides (e.g. what the Dashboard sends for one launch).

use super::{AdapterRef, DraftSpec, ModelDefinition, ModelRef, ServerProfile};
use crate::error_key::err;
use crate::params::{ParamValue, ParamValues};
use crate::process::LaunchConfig;
use std::path::PathBuf;

/// Free-form extra command-line arguments (`LaunchConfig.custom_args`).
pub const CUSTOM_ARGS: &str = "custom_args";
/// Scale of the single typed LoRA (`LaunchConfig.lora_scale`).
pub const LORA_SCALE: &str = "lora_scale";
/// Runtime (llama-server build) id (`LaunchConfig.runtime`, see `runtimes`). Model-level:
/// kept in a definition's `load`; a profile can override it in `serve_overrides`.
pub const RUNTIME: &str = "runtime";

/// Ids owned by typed `ModelDefinition` fields; never kept in `load`.
const DEFINITION_OWNED: &[&str] = &[
    "lora_scaled",
    "chat_template",
    "chat_template_kwargs",
    CUSTOM_ARGS,
    "mmproj_path",
    "lora_path",
    LORA_SCALE,
    "draft_model_path",
    "draft_gpu_layers",
];

/// Values used when no layer sets them. Everything else falls back to llama-server's own
/// default (nothing is emitted).
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchDefaults {
    pub gpu_layers: u32,
    pub context_size: u32,
    pub host: String,
    pub port: u16,
    /// Router mode without a directory serves this one.
    pub models_dir: PathBuf,
}

impl LaunchDefaults {
    pub fn from_settings(settings: &crate::config::AppSettings) -> Self {
        Self {
            gpu_layers: settings.server.default_ngl,
            context_size: settings.server.default_ctx,
            host: settings.server.host.clone(),
            port: settings.server.port,
            models_dir: settings.primary_models_dir(),
        }
    }

    pub fn load() -> Self {
        Self::from_settings(&crate::config::AppSettings::load())
    }

    /// Defaults equal to a config's own values (for computing differences against it).
    pub fn from_config(c: &LaunchConfig) -> Self {
        Self {
            gpu_layers: c.gpu_layers,
            context_size: c.context_size,
            host: c.host.clone(),
            port: c.port,
            models_dir: PathBuf::new(),
        }
    }
}

/// Serving-only ids (catalog groups `server` and `router`): kept in `ServerProfile`, never in
/// a definition.
pub fn is_serve_key(id: &str) -> bool {
    crate::params::catalog()
        .get(id)
        .is_some_and(|s| matches!(s.group.as_str(), "server" | "router"))
}

// --- value conversion ---

/// `key` names the expected kind (`domain.param_not_*`).
fn bad(id: &str, key: &'static str) -> String {
    err(key).with("param", id).into()
}

fn unset(v: &ParamValue) -> bool {
    match v {
        ParamValue::Text(s) => s.trim().is_empty(),
        ParamValue::List(l) => l.iter().all(|s| s.trim().is_empty()),
        _ => false,
    }
}

fn to_u32(id: &str, v: &ParamValue) -> Result<Option<u32>, String> {
    if unset(v) {
        return Ok(None);
    }
    let n = match v {
        ParamValue::Int(i) => Some(*i as f64),
        ParamValue::Float(f) => Some(*f),
        ParamValue::Text(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    match n {
        Some(n) if n.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&n) => Ok(Some(n as u32)),
        _ => Err(bad(id, "domain.param_not_uint")),
    }
}

fn to_f32(id: &str, v: &ParamValue) -> Result<Option<f32>, String> {
    if unset(v) {
        return Ok(None);
    }
    match v {
        ParamValue::Int(i) => Ok(Some(*i as f32)),
        ParamValue::Float(f) if f.is_finite() => Ok(Some(*f as f32)),
        ParamValue::Text(s) => s
            .trim()
            .parse::<f32>()
            .map(Some)
            .map_err(|_| bad(id, "domain.param_not_number")),
        _ => Err(bad(id, "domain.param_not_number")),
    }
}

fn to_bool(id: &str, v: &ParamValue) -> Result<Option<bool>, String> {
    match v {
        ParamValue::Bool(b) => Ok(Some(*b)),
        v if unset(v) => Ok(None),
        _ => Err(bad(id, "domain.param_not_bool")),
    }
}

fn to_text(id: &str, v: &ParamValue) -> Result<Option<String>, String> {
    match v {
        v if unset(v) => Ok(None),
        ParamValue::Text(s) => Ok(Some(s.clone())),
        ParamValue::Int(i) => Ok(Some(i.to_string())),
        ParamValue::Float(f) => Ok(Some(f.to_string())),
        _ => Err(bad(id, "domain.param_not_text")),
    }
}

/// f32 -> f64 without binary noise (`0.8f32` stays `0.8`, not `0.800000011920929`).
fn f32_clean(f: f32) -> f64 {
    f.to_string().parse().unwrap_or(f as f64)
}

fn path_text(p: &std::path::Path) -> ParamValue {
    ParamValue::Text(p.to_string_lossy().to_string())
}

// --- LaunchConfig <-> layer ---

/// Every launch setting of `config` as a layer (typed fields under their catalog ids plus
/// `config.params`). Connection fields (model, host, port, API key, router directory) are
/// not part of it.
pub fn config_layer(c: &LaunchConfig) -> ParamValues {
    let mut out = c.params.clone();
    out.retain(|_, v| !unset(v));
    let mut put = |id: &str, v: Option<ParamValue>| {
        if let Some(v) = v {
            out.insert(id.to_string(), v);
        }
    };
    let int = |n: Option<u32>| n.map(|n| ParamValue::Int(n as i64));
    let text = |s: &Option<String>| {
        s.as_ref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| ParamValue::Text(s.clone()))
    };
    let path = |p: &Option<PathBuf>| {
        p.as_ref()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| path_text(p))
    };
    let float = |f: Option<f32>| f.map(|f| ParamValue::Float(f32_clean(f)));
    put("gpu_layers", int(Some(c.gpu_layers)));
    put("context_size", int(Some(c.context_size)));
    put("threads", int(c.threads));
    put("flash_attn", c.flash_attn.then_some(ParamValue::Bool(true)));
    put("cache_type_k", text(&c.cache_type_k));
    put("cache_type_v", text(&c.cache_type_v));
    put("parallel_slots", int(c.parallel_slots));
    put("logical_batch", int(c.logical_batch));
    put("physical_batch", int(c.physical_batch));
    put("context_shift", c.context_shift.map(ParamValue::Bool));
    put("split_mode", text(&c.split_mode));
    put("main_gpu", int(c.main_gpu));
    put("tensor_split", text(&c.tensor_split));
    put("rope_scaling", text(&c.rope_scaling));
    put("rope_freq_base", float(c.rope_freq_base));
    put("rope_freq_scale", float(c.rope_freq_scale));
    put("chat_template", text(&c.chat_template));
    put(CUSTOM_ARGS, text(&c.custom_args));
    put(
        RUNTIME,
        c.runtime
            .as_ref()
            .filter(|r| !crate::runtimes::is_official(Some(r)))
            .map(|r| ParamValue::Text(r.trim().to_string())),
    );
    put("draft_model_path", path(&c.draft_model_path));
    put("draft_gpu_layers", int(c.draft_gpu_layers));
    put("lora_path", path(&c.lora_path));
    put(LORA_SCALE, float(c.lora_scale));
    put("mmproj_path", path(&c.mmproj_path));
    put(
        "mcp_enabled",
        c.mcp_enabled.then_some(ParamValue::Bool(true)),
    );
    out.retain(|id, _| !c.auto_fields.iter().any(|a| a == id));
    out
}

/// Applies a layer on top of `c`. Typed ids set typed fields; every other id goes to
/// `c.params`. An unset value clears the setting (required numbers keep their value).
pub fn apply_layer(c: &mut LaunchConfig, layer: &ParamValues) -> Result<(), String> {
    for (id, v) in layer {
        let id = id.as_str();
        match id {
            "gpu_layers" => {
                if let Some(n) = to_u32(id, v)? {
                    c.gpu_layers = n
                }
            }
            "context_size" => {
                if let Some(n) = to_u32(id, v)? {
                    c.context_size = n
                }
            }
            "threads" => c.threads = to_u32(id, v)?,
            "flash_attn" => c.flash_attn = to_bool(id, v)?.unwrap_or(false),
            "cache_type_k" => c.cache_type_k = to_text(id, v)?,
            "cache_type_v" => c.cache_type_v = to_text(id, v)?,
            "parallel_slots" => c.parallel_slots = to_u32(id, v)?,
            "logical_batch" => c.logical_batch = to_u32(id, v)?,
            "physical_batch" => c.physical_batch = to_u32(id, v)?,
            "context_shift" => c.context_shift = to_bool(id, v)?,
            "split_mode" => c.split_mode = to_text(id, v)?,
            "main_gpu" => c.main_gpu = to_u32(id, v)?,
            "tensor_split" => c.tensor_split = to_text(id, v)?,
            "rope_scaling" => c.rope_scaling = to_text(id, v)?,
            "rope_freq_base" => c.rope_freq_base = to_f32(id, v)?,
            "rope_freq_scale" => c.rope_freq_scale = to_f32(id, v)?,
            "chat_template" => c.chat_template = to_text(id, v)?,
            CUSTOM_ARGS => c.custom_args = to_text(id, v)?,
            RUNTIME => {
                c.runtime = to_text(id, v)?.filter(|r| !crate::runtimes::is_official(Some(r)))
            }
            "draft_model_path" => c.draft_model_path = to_text(id, v)?.map(PathBuf::from),
            "draft_gpu_layers" => c.draft_gpu_layers = to_u32(id, v)?,
            "lora_path" => c.lora_path = to_text(id, v)?.map(PathBuf::from),
            LORA_SCALE => c.lora_scale = to_f32(id, v)?,
            "mmproj_path" => c.mmproj_path = to_text(id, v)?.map(PathBuf::from),
            // The old llama-server `--tools` list (removed): saved layers that carry it still
            // apply, the value is dropped.
            "server_tools" => {}
            "mcp_enabled" => c.mcp_enabled = to_bool(id, v)?.unwrap_or(false),
            "router_mode" => c.router_mode = to_bool(id, v)?.unwrap_or(false),
            _ if unset(v) => {
                c.params.remove(id);
            }
            _ => {
                c.params.insert(id.to_string(), v.clone());
            }
        }
    }
    Ok(())
}

/// The model-level part of a layer (serve-only ids removed).
pub fn model_part(layer: &ParamValues) -> ParamValues {
    layer
        .iter()
        .filter(|(k, _)| !is_serve_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The serve-only part of a layer.
pub fn serve_part(layer: &ParamValues) -> ParamValues {
    layer
        .iter()
        .filter(|(k, _)| is_serve_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// A definition's settings as one layer (`load` plus its typed fields).
pub fn definition_layer(def: &ModelDefinition) -> ParamValues {
    let mut out = def.load.clone();
    out.retain(|k, v| !unset(v) && !is_serve_key(k));
    let mut put = |id: &str, v: ParamValue| {
        out.insert(id.to_string(), v);
    };
    if let Some(t) = def.chat_template.as_ref().filter(|t| !t.trim().is_empty()) {
        put("chat_template", ParamValue::Text(t.clone()));
    }
    if let Some(k) = def
        .chat_template_kwargs
        .as_ref()
        .filter(|k| !k.trim().is_empty())
    {
        put("chat_template_kwargs", ParamValue::Text(k.clone()));
    }
    if let Some(a) = def.extra_args.as_ref().filter(|a| !a.trim().is_empty()) {
        put(CUSTOM_ARGS, ParamValue::Text(a.clone()));
    }
    if let Some(m) = &def.mmproj {
        put("mmproj_path", path_text(m));
    }
    match def.adapters.as_slice() {
        [] => {}
        [one] => {
            put("lora_path", path_text(&one.path));
            if let Some(s) = one.scale {
                put(LORA_SCALE, ParamValue::Float(s));
            }
        }
        many => put(
            "lora_scaled",
            ParamValue::List(
                many.iter()
                    .map(|a| format!("{}:{}", a.path.display(), a.scale.unwrap_or(1.0)))
                    .collect(),
            ),
        ),
    }
    if let Some(d) = &def.draft {
        put("draft_model_path", path_text(&d.model));
        for (k, v) in &d.params {
            put(k, v.clone());
        }
    }
    out
}

/// Fills `def`'s base, typed fields and `load` from a launch config (model-level settings
/// only). Settings a typed field can't hold stay in `load`, so
/// `definition_layer(def) == model_part(&config_layer(config))` always holds.
pub fn fill_definition(def: &mut ModelDefinition, c: &LaunchConfig) {
    let mut load = model_part(&config_layer(c));
    let take_text = |load: &mut ParamValues, id: &str| match load.remove(id) {
        Some(ParamValue::Text(t)) => Some(t),
        Some(other) => {
            load.insert(id.to_string(), other);
            None
        }
        None => None,
    };
    def.base = ModelRef::Local {
        path: c.model_path.clone(),
    };
    def.chat_template = take_text(&mut load, "chat_template");
    def.chat_template_kwargs = take_text(&mut load, "chat_template_kwargs");
    def.extra_args = take_text(&mut load, CUSTOM_ARGS);
    def.mmproj = take_text(&mut load, "mmproj_path").map(PathBuf::from);
    def.adapters = match take_text(&mut load, "lora_path") {
        Some(p) => {
            let scale = match load.remove(LORA_SCALE) {
                Some(ParamValue::Float(f)) => Some(f),
                Some(ParamValue::Int(i)) => Some(i as f64),
                Some(other) => {
                    load.insert(LORA_SCALE.to_string(), other);
                    None
                }
                None => None,
            };
            vec![AdapterRef {
                path: PathBuf::from(p),
                scale,
            }]
        }
        None => Vec::new(),
    };
    def.draft = take_text(&mut load, "draft_model_path").map(|model| {
        let mut params = ParamValues::new();
        if let Some(v) = load.remove("draft_gpu_layers") {
            params.insert("draft_gpu_layers".into(), v);
        }
        DraftSpec {
            model: PathBuf::from(model),
            params,
        }
    });
    def.load = load;
}

/// `path:scale` items of `--lora-scaled` back into adapters (the path may contain `:` on
/// Windows, so the scale is what follows the last one).
fn parse_scaled_adapters(items: &[String]) -> Vec<AdapterRef> {
    items
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|item| match item.rsplit_once(':') {
            Some((path, scale)) if scale.trim().parse::<f64>().is_ok() && !path.is_empty() => {
                AdapterRef {
                    path: PathBuf::from(path),
                    scale: scale.trim().parse().ok(),
                }
            }
            _ => AdapterRef {
                path: PathBuf::from(item),
                scale: None,
            },
        })
        .collect()
}

/// Drops ids a typed definition field owns from `load` (moving them to the field when it is
/// empty), so a hand-edited or API-saved definition has one place for each setting.
pub fn normalize_definition(def: &mut ModelDefinition) {
    if def
        .base
        .local_path()
        .is_some_and(crate::engine::crispasr::is_model)
    {
        def.load.insert(
            "task".into(),
            crate::params::ParamValue::Text("crispasr".into()),
        );
    }
    if def
        .base
        .local_path()
        .is_some_and(crate::asr::is_onnx_ctc_file)
    {
        def.load.insert(
            "task".into(),
            crate::params::ParamValue::Text("asr_ctc".into()),
        );
    }
    let owned: ParamValues = def
        .load
        .iter()
        .filter(|(k, _)| DEFINITION_OWNED.contains(&k.as_str()) || is_serve_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if owned.is_empty() {
        return;
    }
    def.load.retain(|k, _| !owned.contains_key(k));
    let text = |id: &str| match owned.get(id) {
        Some(ParamValue::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
        _ => None,
    };
    if def.chat_template.is_none() {
        def.chat_template = text("chat_template");
    }
    if def.chat_template_kwargs.is_none() {
        def.chat_template_kwargs = text("chat_template_kwargs");
    }
    if def.extra_args.is_none() {
        def.extra_args = text(CUSTOM_ARGS);
    }
    if def.mmproj.is_none() {
        def.mmproj = text("mmproj_path").map(PathBuf::from);
    }
    if def.adapters.is_empty() {
        if let Some(p) = text("lora_path") {
            let scale = match owned.get(LORA_SCALE) {
                Some(ParamValue::Float(f)) => Some(*f),
                Some(ParamValue::Int(i)) => Some(*i as f64),
                _ => None,
            };
            def.adapters.push(AdapterRef {
                path: p.into(),
                scale,
            });
        }
    }
    if def.adapters.is_empty() {
        if let Some(ParamValue::List(items)) = owned.get("lora_scaled") {
            def.adapters = parse_scaled_adapters(items);
        }
    }
    if def.draft.is_none() {
        if let Some(m) = text("draft_model_path") {
            let mut params = ParamValues::new();
            if let Some(v) = owned.get("draft_gpu_layers") {
                params.insert("draft_gpu_layers".into(), v.clone());
            }
            def.draft = Some(DraftSpec {
                model: m.into(),
                params,
            });
        }
    }
}

/// `serve_overrides` that reproduce `config` exactly on top of `def` at the time of the call:
/// every id either side sets, with "unset" markers for what `config` doesn't set.
pub fn snapshot_overrides(def: Option<&ModelDefinition>, config: &LaunchConfig) -> ParamValues {
    let mut out = config_layer(config);
    if let Some(def) = def {
        for (id, _) in definition_layer(def) {
            let marker = unset_marker(&id);
            out.entry(id).or_insert(marker);
        }
    }
    out
}

fn unset_marker(id: &str) -> ParamValue {
    match id {
        "flash_attn" | "mcp_enabled" => ParamValue::Bool(false),
        _ => ParamValue::Text(String::new()),
    }
}

/// The layer that turns `base` into `target` (every setting id, connection fields excluded):
/// ids whose value differs get `target`'s value, ids only `base` sets get an "unset" marker.
/// `apply_layer(base, diff_layer(base, target))` has `target`'s settings.
pub fn diff_layer(base: &LaunchConfig, target: &LaunchConfig) -> ParamValues {
    let have = config_layer(base);
    let want = config_layer(target);
    let mut out = ParamValues::new();
    for (id, v) in &want {
        if have.get(id) != Some(v) {
            out.insert(id.clone(), v.clone());
        }
    }
    for id in have.keys() {
        if !want.contains_key(id) && !target.auto_fields.iter().any(|a| a == id) {
            out.insert(id.clone(), unset_marker(id));
        }
    }
    out
}

/// Model-level settings of `config` that differ from what `def` loads by itself: the
/// transient layer a launch form puts over its definition ("unsaved changes").
pub fn transient_overrides(
    defaults: &LaunchDefaults,
    def: &ModelDefinition,
    config: &LaunchConfig,
) -> Result<ParamValues, String> {
    let base = resolve_launch(defaults, Some(def), None, &ParamValues::new())?;
    Ok(model_part(&diff_layer(&base, config)))
}

/// Effective launch configuration of a definition served by a profile, with transient
/// overrides on top. `binary_path`, the resolved API key and the MCP config are filled in
/// by the orchestrator at launch.
pub fn resolve_launch(
    defaults: &LaunchDefaults,
    def: Option<&ModelDefinition>,
    profile: Option<&ServerProfile>,
    transient: &ParamValues,
) -> Result<LaunchConfig, String> {
    resolve_launch_with(defaults, def, profile, transient, &recommended_layers)
}

/// The `-ngl` the VRAM fit recommends for `c` (the estimate the Dashboard shows), or `None`
/// when it cannot be made: no readable GGUF header (router mode, missing file, bad file).
/// Reads the header (cached by path/size/mtime) and the cached hardware probe.
fn recommended_layers(c: &LaunchConfig) -> Option<u32> {
    recommended_layers_with(c, &crate::hardware::detect_hardware())
}

fn recommended_layers_with(
    c: &LaunchConfig,
    hw: &crate::hardware::SystemHardwareInfo,
) -> Option<u32> {
    let audio_cpp = crate::engine::audio_cpp::is_model(&c.model_path);
    if !audio_cpp
        && (!c.model_path.is_file() || crate::models::cached_metadata(&c.model_path).is_none())
    {
        return None;
    }
    let mut candidate = c.clone();
    // Whole-model audio runtimes have no partial layer offload. Estimate the GPU
    // candidate before deciding; the inherited default of zero would estimate CPU.
    if audio_cpp {
        candidate.gpu_layers = if hw.gpus.is_empty() { 0 } else { 999 };
    }
    Some(crate::vram::fit_launch(&candidate, hw).recommended_gpu_layers)
}

/// The definition's own `load` sets `gpu_layers` (otherwise a launch gets the recommendation).
pub(crate) fn definition_sets_gpu_layers(def: &ModelDefinition) -> bool {
    sets_gpu_layers(&definition_layer(def))
}

/// Whether loading `def` with the one-time `overrides` sets `gpu_layers` explicitly: an
/// override decides (an "unset" marker means automatic again), else the definition does.
pub(crate) fn load_sets_gpu_layers(def: &ModelDefinition, overrides: &ParamValues) -> bool {
    match overrides.get("gpu_layers") {
        Some(v) => !unset(v),
        None => definition_sets_gpu_layers(def),
    }
}

/// `def` as a load with the one-time `overrides` sees it: a `gpu_layers` override that is
/// "unset" (automatic) drops the definition's own value so the recommendation applies again
/// (in a layer an unset value alone would just be skipped).
pub(crate) fn definition_for_load(
    def: &ModelDefinition,
    overrides: &ParamValues,
) -> ModelDefinition {
    let mut def = def.clone();
    if overrides.get("gpu_layers").is_some_and(unset) {
        def.load.remove("gpu_layers");
    }
    def
}

/// Persists load settings into the definition ("Load and save to the definition"): a value
/// replaces the setting, an "unset" value removes it (back to automatic). Ids a typed field of
/// the definition owns and serving-only ids are ignored. Returns the ids that changed.
pub fn merge_load_overrides(def: &mut ModelDefinition, overrides: &ParamValues) -> Vec<String> {
    let mut changed = Vec::new();
    for (id, v) in overrides {
        if DEFINITION_OWNED.contains(&id.as_str()) || is_serve_key(id) {
            continue;
        }
        let before = def.load.get(id).cloned();
        if unset(v) {
            def.load.remove(id);
        } else {
            def.load.insert(id.clone(), v.clone());
        }
        if def.load.get(id) != before.as_ref() {
            changed.push(id.clone());
        }
    }
    changed
}

/// Whether a layer sets `gpu_layers` to a value (an "unset" marker does not).
fn sets_gpu_layers(layer: &ParamValues) -> bool {
    layer.get("gpu_layers").is_some_and(|v| !unset(v))
}

/// [`resolve_launch`] with the estimate injected. A definition launch whose layers (the
/// definition's `load`, the profile's overrides, the transient layer) leave `gpu_layers`
/// unset gets the recommended layer count instead of `defaults.gpu_layers`; an explicit
/// value always wins, and so does the default when no estimate can be made.
pub(crate) fn resolve_launch_with(
    defaults: &LaunchDefaults,
    def: Option<&ModelDefinition>,
    profile: Option<&ServerProfile>,
    transient: &ParamValues,
    recommend: &dyn Fn(&LaunchConfig) -> Option<u32>,
) -> Result<LaunchConfig, String> {
    let mut c = LaunchConfig {
        gpu_layers: defaults.gpu_layers,
        context_size: defaults.context_size,
        host: defaults.host.clone(),
        port: defaults.port,
        ..Default::default()
    };
    if let Some(def) = def {
        c.alias = Some(def.alias.clone());
        c.definition_id = Some(def.id.clone());
        c.model_path = match &def.base {
            ModelRef::Local { path } => path.clone(),
            ModelRef::Hf { repo, file } => {
                return Err(err("definitions.not_downloaded")
                    .with("alias", &def.alias)
                    .with("repo", repo)
                    .with("file", file)
                    .into())
            }
        };
        apply_layer(&mut c, &definition_layer(def))?;
    }
    if let Some(p) = profile {
        c.host = p.host.clone();
        c.port = p.port;
        c.port_auto = p.port_auto;
        c.api_key_ref = p.api_key_ref.clone();
        if let Some(r) = &p.router {
            c.router_mode = true;
            c.models_path = r.models_dir.clone();
            c.model_path = r
                .models_dir
                .clone()
                .unwrap_or_else(|| defaults.models_dir.clone());
        } else if def.is_none() {
            return Err(err("profiles.definition_missing")
                .with("name", &p.name)
                .into());
        }
        apply_layer(&mut c, &p.serve_overrides)?;
    }
    apply_layer(&mut c, transient)?;
    if let Some(def) = def {
        let explicit = sets_gpu_layers(&definition_layer(def))
            || profile.is_some_and(|p| sets_gpu_layers(&p.serve_overrides))
            || sets_gpu_layers(transient);
        if !explicit && !c.router_mode {
            if let Some(n) = recommend(&c) {
                c.gpu_layers = n;
                c.gpu_layers_auto = true;
            }
        }
    }
    c.auto_fields = if c.gpu_layers_auto {
        vec!["gpu_layers".to_string()]
    } else {
        Vec::new()
    };
    Ok(c)
}

/// A launch without a definition whose form left settings automatic (`auto_fields`): the
/// context and slot count go back to automatic, the layer count to the memory estimate's
/// recommendation (`gpu_layers_auto`), so the default-load rule applies as it does for a
/// definition. Settings the user set stay as they are.
pub fn apply_auto_fields(c: &mut LaunchConfig) {
    for id in c.auto_fields.clone() {
        match id.as_str() {
            "context_size" => c.context_size = 0,
            "parallel_slots" => c.parallel_slots = None,
            "cache_type_k" => c.cache_type_k = None,
            "cache_type_v" => c.cache_type_v = None,
            "flash_attn" => c.flash_attn = false,
            "gpu_layers" => {
                if let Some(n) = recommended_layers(c) {
                    c.gpu_layers = n;
                    c.gpu_layers_auto = true;
                }
            }
            _ => {}
        }
    }
    c.auto_fields = if c.gpu_layers_auto {
        vec!["gpu_layers".to_string()]
    } else {
        Vec::new()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_cpp_auto_load_uses_gpu_despite_zero_default_and_keeps_explicit_cpu() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pocket.gguf");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"GGUF").unwrap();
        file.write_all(&3u32.to_le_bytes()).unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap();
        file.write_all(&2u64.to_le_bytes()).unwrap();
        for (key, value) in [
            ("general.architecture", "audiocpp"),
            ("audiocpp.model_spec.family", "pocket_tts"),
        ] {
            file.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
            file.write_all(key.as_bytes()).unwrap();
            file.write_all(&8u32.to_le_bytes()).unwrap();
            file.write_all(&(value.len() as u64).to_le_bytes()).unwrap();
            file.write_all(value.as_bytes()).unwrap();
        }
        drop(file);
        let mut hw = crate::hardware::detect_hardware();
        hw.gpus = vec![crate::hardware::GpuInfo {
            index: 0,
            name: "Test AMD".into(),
            vendor: crate::hardware::GpuVendor::Amd,
            vram_bytes: Some(8 * 1024 * 1024 * 1024),
            driver_version: None,
            is_discrete: true,
            is_default: true,
        }];
        hw.primary_gpu_index = Some(0);
        hw.recommended_backend = crate::hardware::RecommendedBackend::Vulkan;
        let mut def = ModelDefinition::new("pocket", ModelRef::Local { path });
        def.load
            .insert("backend".into(), ParamValue::Text("vulkan".into()));
        let mut defaults = defaults();
        defaults.gpu_layers = 0;
        let recommend = |c: &LaunchConfig| recommended_layers_with(c, &hw);
        let config =
            resolve_launch_with(&defaults, Some(&def), None, &Default::default(), &recommend)
                .unwrap();
        assert_eq!(config.gpu_layers, 999);
        def.load.insert("gpu_layers".into(), ParamValue::Int(0));
        let config =
            resolve_launch_with(&defaults, Some(&def), None, &Default::default(), &recommend)
                .unwrap();
        assert_eq!(config.gpu_layers, 0);
    }

    fn defaults() -> LaunchDefaults {
        LaunchDefaults {
            gpu_layers: 33,
            context_size: 4096,
            host: "127.0.0.1".into(),
            port: 8080,
            models_dir: PathBuf::from(r"D:\Models"),
        }
    }

    /// The shipped default context is automatic (0): an empty definition keeps it, so the
    /// launch (gateway `default_context_for`, `ProcessManager`) resolves it from the model
    /// instead of ending at llama.cpp's 4096.
    #[test]
    fn an_empty_definition_keeps_the_automatic_context() {
        let shipped = LaunchDefaults::from_settings(&crate::config::AppSettings::default());
        assert_eq!(shipped.context_size, 0);
        let c = resolve_launch(
            &shipped,
            Some(&def(ParamValues::new())),
            None,
            &ParamValues::new(),
        )
        .unwrap();
        assert_eq!(c.context_size, 0);
        let own = vals(&[("context_size", ParamValue::Int(8192))]);
        let c = resolve_launch(&shipped, Some(&def(own)), None, &ParamValues::new()).unwrap();
        assert_eq!(c.context_size, 8192);
    }

    fn def(load: ParamValues) -> ModelDefinition {
        ModelDefinition {
            id: "md-1".into(),
            alias: "qwen3-4b".into(),
            base: ModelRef::Local {
                path: r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf".into(),
            },
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

    fn profile(overrides: ParamValues) -> ServerProfile {
        ServerProfile {
            id: "coder".into(),
            name: "Coder".into(),
            purpose: String::new(),
            definition_id: Some("md-1".into()),
            router: None,
            host: "0.0.0.0".into(),
            port: 8090,
            port_auto: false,
            auto_start: false,
            api_key_ref: Some("profile:coder".into()),
            serve_overrides: overrides,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn vals(pairs: &[(&str, ParamValue)]) -> ParamValues {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    /// The estimate of the real Qwen2.5-Coder-14B Q4_K_M header (`tests/data`) on a 6 GiB GPU:
    /// a partial offload (`vram_tests::categories_and_recommended_layers`).
    fn fixture_estimate(c: &LaunchConfig) -> Option<u32> {
        use crate::vram::{base_input_from_launch, fit_launch_with, MemoryBudget};
        const GIB: u64 = 1024 * 1024 * 1024;
        let v: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/data/gguf-vram-shapes.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let e = v
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["file"] == "qwen2.5-coder-14b-instruct-q4_k_m.gguf")
            .unwrap();
        let meta: crate::gguf::GgufMetadata = serde_json::from_value(e["meta"].clone()).unwrap();
        let mut base = base_input_from_launch(c);
        base.model_size_bytes = e["size_bytes"].as_u64().unwrap();
        let budget = MemoryBudget {
            vram_bytes: 6 * GIB,
            ram_bytes: 32 * GIB,
            unified: false,
            shared_igpu: false,
            igpu_offload_faster: None,
            cuda: false,
            rocm: false,
        };
        Some(
            fit_launch_with(
                c,
                &base,
                Some(&meta),
                &budget,
                &[],
                false,
                crate::hardware::RecommendedBackend::Vulkan,
            )
            .recommended_gpu_layers,
        )
    }

    #[test]
    fn auto_fields_keep_the_automatic_choice_out_of_every_layer() {
        // A definition launch with no layer count of its own: the estimate's count is automatic.
        let d = def(ParamValues::new());
        let c = resolve_launch_with(
            &defaults(),
            Some(&d),
            None,
            &ParamValues::new(),
            &fixture_estimate,
        )
        .unwrap();
        assert!(c.gpu_layers_auto);
        assert_eq!(c.auto_fields, vec!["gpu_layers".to_string()]);
        assert!(!config_layer(&c).contains_key("gpu_layers"));
        // An explicit count is no automatic choice.
        let ex = resolve_launch_with(
            &defaults(),
            Some(&d),
            None,
            &vals(&[("gpu_layers", ParamValue::Int(20))]),
            &fixture_estimate,
        )
        .unwrap();
        assert!(ex.auto_fields.is_empty());
        assert_eq!(
            config_layer(&ex).get("gpu_layers"),
            Some(&ParamValue::Int(20))
        );
        // A form that filled the context, layers and cache by itself: the transient layer, the
        // snapshot and a definition made from it leave them out; a value the user changed stays.
        let mut form = c.clone();
        form.context_size = 12288;
        form.gpu_layers = 37;
        form.parallel_slots = Some(1);
        form.threads = Some(6);
        form.auto_fields = ["context_size", "gpu_layers", "parallel_slots"]
            .map(String::from)
            .to_vec();
        let diff = diff_layer(&c, &form);
        assert_eq!(diff.get("threads"), Some(&ParamValue::Int(6)));
        for id in ["context_size", "gpu_layers", "parallel_slots"] {
            assert!(!diff.contains_key(id), "{id}");
            assert!(
                !snapshot_overrides(Some(&d), &form).contains_key(id),
                "{id}"
            );
        }
        let mut made = def(ParamValues::new());
        fill_definition(&mut made, &form);
        for id in ["context_size", "gpu_layers", "parallel_slots"] {
            assert!(!made.load.contains_key(id), "{id}");
        }
        assert_eq!(made.load.get("threads"), Some(&ParamValue::Int(6)));
        form.auto_fields = vec!["context_size".to_string()];
        assert_eq!(
            diff_layer(&c, &form).get("gpu_layers"),
            Some(&ParamValue::Int(37))
        );
    }

    #[test]
    fn definition_without_gpu_layers_uses_the_recommendation() {
        let d = def(ParamValues::new());
        let c = resolve_launch_with(
            &defaults(),
            Some(&d),
            None,
            &ParamValues::new(),
            &fixture_estimate,
        )
        .unwrap();
        assert_eq!(Some(c.gpu_layers), fixture_estimate(&c));
        assert!(c.gpu_layers > 0 && c.gpu_layers < 48 && c.gpu_layers != 33);
        // A profile that does not set it keeps the recommendation too; an unset marker is not a value.
        let p = profile(vals(&[("gpu_layers", ParamValue::Text(String::new()))]));
        let c = resolve_launch_with(
            &defaults(),
            Some(&d),
            Some(&p),
            &ParamValues::new(),
            &fixture_estimate,
        )
        .unwrap();
        assert_eq!(Some(c.gpu_layers), fixture_estimate(&c));
    }

    #[test]
    fn explicit_gpu_layers_win_over_the_recommendation() {
        use ParamValue::Int;
        let never = |_: &LaunchConfig| -> Option<u32> { panic!("no estimate expected") };
        let d = def(vals(&[("gpu_layers", Int(37))]));
        let c =
            resolve_launch_with(&defaults(), Some(&d), None, &ParamValues::new(), &never).unwrap();
        assert_eq!(c.gpu_layers, 37);
        let d = def(ParamValues::new());
        let p = profile(vals(&[("gpu_layers", Int(12))]));
        let c = resolve_launch_with(&defaults(), Some(&d), Some(&p), &ParamValues::new(), &never)
            .unwrap();
        assert_eq!(c.gpu_layers, 12);
        let t = vals(&[("gpu_layers", Int(0))]);
        let c = resolve_launch_with(&defaults(), Some(&d), None, &t, &never).unwrap();
        assert_eq!(c.gpu_layers, 0);
    }

    /// One-time load settings sit over the definition (transient layer) without changing it.
    #[test]
    fn one_time_overrides_layer_over_the_definition() {
        use ParamValue::{Bool, Int, Text};
        let rec = |_: &LaunchConfig| Some(21);
        let d = def(vals(&[
            ("context_size", Int(8192)),
            ("gpu_layers", Int(37)),
            ("cache_type_k", Text("q8_0".into())),
        ]));
        let ov = vals(&[
            ("context_size", Int(16384)),
            ("flash_attn", Bool(true)),
            ("parallel_slots", Int(2)),
        ]);
        let c = resolve_launch_with(
            &defaults(),
            Some(&definition_for_load(&d, &ov)),
            None,
            &ov,
            &rec,
        )
        .unwrap();
        assert_eq!(c.context_size, 16384);
        assert!(c.flash_attn);
        assert_eq!(c.parallel_slots, Some(2));
        // Untouched settings come from the definition; an override of gpu_layers is absent.
        assert_eq!(c.gpu_layers, 37);
        assert_eq!(c.cache_type_k.as_deref(), Some("q8_0"));
        assert!(load_sets_gpu_layers(&d, &ov));
        assert_eq!(d.load.get("context_size"), Some(&Int(8192)));
    }

    /// Automatic again: unset gpu_layers drops the definition's value (the recommendation
    /// applies), context 0 is the automatic context, an unset cache type clears the definition's.
    #[test]
    fn automatic_overrides_reset_the_definitions_values() {
        use ParamValue::{Int, Text};
        let rec = |_: &LaunchConfig| Some(21);
        let d = def(vals(&[
            ("context_size", Int(8192)),
            ("gpu_layers", Int(37)),
            ("cache_type_k", Text("q4_0".into())),
            ("cache_type_v", Text("q4_0".into())),
        ]));
        let ov = vals(&[
            ("context_size", Int(0)),
            ("gpu_layers", Text(String::new())),
            ("cache_type_k", Text(String::new())),
            ("cache_type_v", Text(String::new())),
        ]);
        assert!(!load_sets_gpu_layers(&d, &ov));
        let c = resolve_launch_with(
            &defaults(),
            Some(&definition_for_load(&d, &ov)),
            None,
            &ov,
            &rec,
        )
        .unwrap();
        assert_eq!(c.context_size, 0);
        assert_eq!(c.gpu_layers, 21);
        assert_eq!(c.cache_type_k, None);
        assert_eq!(c.cache_type_v, None);
        // The stored definition is untouched.
        assert_eq!(d.load.get("gpu_layers"), Some(&Int(37)));
    }

    #[test]
    fn saving_load_settings_merges_into_the_definition() {
        use ParamValue::{Int, Text};
        let mut d = def(vals(&[
            ("context_size", Int(8192)),
            ("gpu_layers", Int(37)),
            ("cache_type_k", Text("q8_0".into())),
        ]));
        let ov = vals(&[
            ("context_size", Int(4096)),
            ("gpu_layers", Text(String::new())),
            ("cache_type_k", Text("q8_0".into())),
            ("parallel_slots", Int(3)),
            // Owned by typed fields / serving-only: never kept in `load`.
            ("chat_template", Text("chatml".into())),
            ("timeout", Int(600)),
        ]);
        let mut changed = merge_load_overrides(&mut d, &ov);
        changed.sort();
        assert_eq!(changed, ["context_size", "gpu_layers", "parallel_slots"]);
        assert_eq!(d.load.get("context_size"), Some(&Int(4096)));
        assert_eq!(d.load.get("gpu_layers"), None);
        assert_eq!(d.load.get("parallel_slots"), Some(&Int(3)));
        assert!(!d.load.contains_key("chat_template") && !d.load.contains_key("timeout"));
        // The next load of the saved definition uses the new settings.
        let c = resolve_launch_with(&defaults(), Some(&d), None, &ParamValues::new(), &|_| {
            Some(21)
        })
        .unwrap();
        assert_eq!(
            (c.context_size, c.gpu_layers, c.parallel_slots),
            (4096, 21, Some(3))
        );
    }

    #[test]
    fn failed_estimate_falls_back_to_default_ngl() {
        let d = def(ParamValues::new());
        let none = |_: &LaunchConfig| -> Option<u32> { None };
        let c =
            resolve_launch_with(&defaults(), Some(&d), None, &ParamValues::new(), &none).unwrap();
        assert_eq!(c.gpu_layers, 33);
        // The real estimator on a file that does not exist: same fallback.
        let mut d = d;
        d.base = ModelRef::Local {
            path: "/nonexistent/model.gguf".into(),
        };
        let c = resolve_launch(&defaults(), Some(&d), None, &ParamValues::new()).unwrap();
        assert_eq!(c.gpu_layers, 33);
    }

    #[test]
    fn precedence_defaults_definition_profile_transient() {
        use ParamValue::*;
        let d = def(vals(&[
            ("gpu_layers", Int(37)),
            ("context_size", Int(65536)),
            ("cache_type_k", Text("q8_0".into())),
            ("jinja", Bool(true)),
            ("cache_ram", Int(4096)),
        ]));
        let p = profile(vals(&[
            ("context_size", Int(32768)),
            ("server_tools", List(vec!["read_file".into()])), // legacy id: ignored
            ("cache_ram", Text(String::new())),               // unset marker
        ]));
        let t = vals(&[("context_size", Int(8192)), ("threads", Int(6))]);

        // Defaults only.
        let c = resolve_launch(&defaults(), None, None, &ParamValues::new()).unwrap();
        assert_eq!((c.gpu_layers, c.context_size, c.port), (33, 4096, 8080));

        // Definition over defaults.
        let c = resolve_launch(&defaults(), Some(&d), None, &ParamValues::new()).unwrap();
        assert_eq!((c.gpu_layers, c.context_size), (37, 65536));
        assert_eq!(c.cache_type_k.as_deref(), Some("q8_0"));
        assert_eq!(c.params.get("cache_ram"), Some(&Int(4096)));
        assert_eq!(c.model_path, d.base.local_path().unwrap());

        // Profile over definition (connection fields, overrides, unset marker).
        let c = resolve_launch(&defaults(), Some(&d), Some(&p), &ParamValues::new()).unwrap();
        assert_eq!((c.gpu_layers, c.context_size), (37, 32768));
        assert_eq!((c.host.as_str(), c.port), ("0.0.0.0", 8090));
        assert_eq!(c.api_key_ref.as_deref(), Some("profile:coder"));
        assert!(
            !c.params.contains_key("server_tools"),
            "legacy id is dropped"
        );
        assert!(!c.params.contains_key("cache_ram"));
        assert_eq!(c.params.get("jinja"), Some(&Bool(true)));

        // Transient over everything.
        let c = resolve_launch(&defaults(), Some(&d), Some(&p), &t).unwrap();
        assert_eq!((c.context_size, c.threads), (8192, Some(6)));
        assert_eq!(c.gpu_layers, 37);
    }

    #[test]
    fn hf_base_and_profile_without_definition_are_errors() {
        let mut d = def(ParamValues::new());
        d.base = ModelRef::parse("hf:unsloth/x:y.gguf");
        assert!(resolve_launch(&defaults(), Some(&d), None, &ParamValues::new()).is_err());
        let p = profile(ParamValues::new());
        assert!(resolve_launch(&defaults(), None, Some(&p), &ParamValues::new()).is_err());
    }

    #[test]
    fn router_profile_serves_a_directory() {
        let mut p = profile(vals(&[("models_max", ParamValue::Int(2))]));
        p.definition_id = None;
        p.router = Some(Default::default());
        let c = resolve_launch(&defaults(), None, Some(&p), &ParamValues::new()).unwrap();
        assert!(c.router_mode);
        assert_eq!(c.model_path, PathBuf::from(r"D:\Models"));
        assert_eq!(c.models_path, None);
        assert_eq!(c.params.get("models_max"), Some(&ParamValue::Int(2)));
    }

    /// Every typed field survives config -> definition + serve layer -> config.
    #[test]
    fn launch_config_round_trips_through_definition_and_profile() {
        let mut params = ParamValues::new();
        params.insert("jinja".into(), ParamValue::Bool(true));
        params.insert(
            "chat_template_kwargs".into(),
            ParamValue::Text("{\"enable_thinking\":false}".into()),
        );
        params.insert("timeout".into(), ParamValue::Int(600));
        let config = LaunchConfig {
            model_path: "models/a.gguf".into(),
            gpu_layers: 21,
            context_size: 16384,
            host: "0.0.0.0".into(),
            port: 18123,
            threads: Some(6),
            flash_attn: true,
            cache_type_k: Some("q8_0".into()),
            cache_type_v: Some("q4_0".into()),
            parallel_slots: Some(2),
            logical_batch: Some(1024),
            physical_batch: Some(256),
            context_shift: Some(false),
            split_mode: Some("layer".into()),
            main_gpu: Some(1),
            tensor_split: Some("3,1".into()),
            rope_scaling: Some("yarn".into()),
            rope_freq_base: Some(10000.0),
            rope_freq_scale: Some(0.5),
            chat_template: Some("chatml".into()),
            api_key_ref: Some("profile:coder".into()),
            custom_args: Some("--no-mmap --verbose".into()),
            draft_model_path: Some("models/draft.gguf".into()),
            draft_gpu_layers: Some(9),
            lora_path: Some("models/l.gguf".into()),
            lora_scale: Some(0.8),
            mmproj_path: Some("models/mm.gguf".into()),
            mcp_enabled: true,
            params,
            ..Default::default()
        };
        let mut d = def(ParamValues::new());
        fill_definition(&mut d, &config);
        assert_eq!(d.adapters[0].scale, Some(0.8));
        assert_eq!(d.draft.as_ref().unwrap().params.len(), 1);
        assert!(d
            .load
            .keys()
            .all(|k| !DEFINITION_OWNED.contains(&k.as_str())));
        assert!(!d.load.contains_key("timeout") && !d.load.contains_key("server_tools"));
        assert_eq!(definition_layer(&d), model_part(&config_layer(&config)));

        let mut p = profile(serve_part(&config_layer(&config)));
        p.host = config.host.clone();
        p.port = config.port;
        let mut back =
            resolve_launch(&defaults(), Some(&d), Some(&p), &ParamValues::new()).unwrap();
        // The launch names its definition and serves the definition's alias.
        assert_eq!(back.alias.take().as_deref(), Some("qwen3-4b"));
        assert_eq!(back.definition_id.take().as_deref(), Some("md-1"));
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&config).unwrap()
        );
        // Float noise stays out of the stored file.
        assert!(serde_json::to_string(&d).unwrap().contains("\"scale\":0.8"));
    }

    #[test]
    fn snapshot_overrides_pin_the_config_over_a_later_definition() {
        let mut d = def(vals(&[
            ("threads", ParamValue::Int(8)),
            ("flash_attn", ParamValue::Bool(true)),
        ]));
        let config = LaunchConfig {
            model_path: d.base.local_path().unwrap().into(),
            gpu_layers: 10,
            context_size: 2048,
            host: "127.0.0.1".into(),
            port: 8080,
            ..Default::default()
        };
        let mut p = profile(snapshot_overrides(Some(&d), &config));
        p.host = config.host.clone();
        p.port = config.port;
        p.api_key_ref = None;
        // The definition changes afterwards; the snapshot still reproduces the launch.
        d.load.insert("gpu_layers".into(), ParamValue::Int(99));
        let mut back =
            resolve_launch(&defaults(), Some(&d), Some(&p), &ParamValues::new()).unwrap();
        back.alias = None;
        back.definition_id = None;
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&config).unwrap()
        );
    }

    #[test]
    fn normalize_moves_owned_ids_out_of_load() {
        let mut d = def(vals(&[
            ("mmproj_path", ParamValue::Text("mm.gguf".into())),
            ("lora_path", ParamValue::Text("l.gguf".into())),
            (LORA_SCALE, ParamValue::Float(0.5)),
            ("timeout", ParamValue::Int(5)),
            ("gpu_layers", ParamValue::Int(3)),
        ]));
        normalize_definition(&mut d);
        assert_eq!(d.mmproj, Some(PathBuf::from("mm.gguf")));
        assert_eq!(d.adapters[0].scale, Some(0.5));
        assert_eq!(d.load.keys().collect::<Vec<_>>(), vec!["gpu_layers"]);
    }

    #[test]
    fn normalize_reads_a_scaled_list_into_adapters() {
        let mut d = def(vals(&[(
            "lora_scaled",
            ParamValue::List(vec![
                r"C:\lora\a.gguf:0.5".into(),
                "b.gguf:1".into(),
                "c.gguf".into(),
            ]),
        )]));
        normalize_definition(&mut d);
        assert!(d.load.is_empty());
        assert_eq!(d.adapters.len(), 3);
        assert_eq!(d.adapters[0].path, PathBuf::from(r"C:\lora\a.gguf"));
        assert_eq!(d.adapters[0].scale, Some(0.5));
        assert_eq!(d.adapters[1].scale, Some(1.0));
        assert_eq!(d.adapters[2].scale, None);
    }

    #[test]
    fn multiple_adapters_use_lora_scaled() {
        let mut d = def(ParamValues::new());
        d.adapters = vec![
            AdapterRef {
                path: "a.gguf".into(),
                scale: Some(0.5),
            },
            AdapterRef {
                path: "b.gguf".into(),
                scale: None,
            },
        ];
        let c = resolve_launch(&defaults(), Some(&d), None, &ParamValues::new()).unwrap();
        assert_eq!(c.lora_path, None);
        assert_eq!(
            c.params.get("lora_scaled"),
            Some(&ParamValue::List(vec![
                "a.gguf:0.5".into(),
                "b.gguf:1".into()
            ]))
        );
    }

    #[test]
    fn invalid_layer_values_are_errors() {
        let mut c = LaunchConfig::default();
        assert!(apply_layer(&mut c, &vals(&[("gpu_layers", ParamValue::Int(-1))])).is_err());
        assert!(apply_layer(&mut c, &vals(&[("flash_attn", ParamValue::Int(1))])).is_err());
        assert!(apply_layer(&mut c, &vals(&[("threads", ParamValue::Float(1.5))])).is_err());
    }
}
