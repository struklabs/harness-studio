//! Portable model definitions: `*.harness.toml` (ARCHITECTURE §13.3).
//!
//! A definition travels without machine-specific paths where possible:
//! * a file inside one of the models directories is written relative to it
//!   (`unsloth/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf`, always with `/`);
//! * a file elsewhere keeps its absolute path;
//! * a definition whose base is not downloaded yet is `hf:<repo>:<file>`.
//!
//! Files downloaded by Harness Studio live under `<models dir>/<owner>/<repo>/`, so such a
//! relative path also gives the Hugging Face origin (`source`), which the import shows when
//! the file isn't found locally.
//!
//! The default inference preset can be embedded (`[preset]`); built-in presets are referenced
//! by id (`default_preset = "builtin:coding"`).

use super::presets::{is_builtin_id, InferencePreset};
use super::{AdapterRef, DraftSpec, ModelDefinition, ModelRef};
use crate::error_key::err;
use crate::params::{ParamValue, ParamValues};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "harness.model.v1";
pub const EXTENSION: &str = ".harness.toml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableAdapter {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableDraft {
    pub model: String,
    #[serde(default, skip_serializing_if = "ParamValues::is_empty")]
    pub params: ParamValues,
}

/// The file layout. Plain values come first (TOML needs them before tables).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableDefinition {
    pub format: String,
    pub alias: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Relative to a models directory, absolute, or `hf:<repo>:<file>`.
    pub model: String,
    /// Hugging Face origin of the model file (`hf:<repo>:<file>`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmproj: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<String>,
    /// Text of the Jinja file `load.chat_template_file` pointed at (a path is machine-specific, so
    /// the text travels; the import saves it as a new template file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_template_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_args: Option<String>,
    /// A built-in preset id, or the embedded `[preset]`'s name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<PortableAdapter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<PortableDraft>,
    #[serde(default, skip_serializing_if = "ParamValues::is_empty")]
    pub load: ParamValues,
    /// The default inference preset, embedded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<toml::Value>,
}

/// `/`-separated components of `p` (no trailing separator).
fn components(p: &Path) -> Vec<String> {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if s.is_empty() {
        return Vec::new();
    }
    s.split('/').map(str::to_string).collect()
}

/// Component equality: case-insensitive on Windows.
fn same_component(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// `path` relative to the first models directory containing it (with `/`), else as is. The
/// relative part is cut at component boundaries of the original string: case folding can change
/// byte lengths (Turkish capital dotted I), so a folded offset must never index the original.
pub fn portable_path(path: &Path, models_dirs: &[PathBuf]) -> String {
    let parts = components(path);
    for dir in models_dirs {
        let dparts = components(dir);
        if dparts.is_empty() || parts.len() <= dparts.len() {
            continue;
        }
        if dparts.iter().zip(&parts).all(|(d, p)| same_component(d, p)) {
            return parts[dparts.len()..].join("/");
        }
    }
    path.to_string_lossy().to_string()
}

fn is_relative(p: &str) -> bool {
    !p.starts_with("hf:") && Path::new(p).is_relative() && !p.starts_with('/')
}

/// `owner/repo/<file>` (the downloader's layout) -> `hf:owner/repo:<file>`.
fn source_of(rel: &str) -> Option<String> {
    let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    (parts.len() >= 3 && !parts[0].starts_with('.'))
        .then(|| format!("hf:{}/{}:{}", parts[0], parts[1], parts[2..].join("/")))
}

/// Finds a portable path on this machine: absolute as is, relative under each models
/// directory, `hf:` refs at their download location.
pub fn resolve_path(p: &str, models_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let ModelRef::Hf { repo, file } = ModelRef::parse(p) {
        let rel = format!("{}/{}", repo, file);
        return resolve_path(&rel, models_dirs);
    }
    if !is_relative(p) {
        let path = PathBuf::from(p);
        return path.is_file().then_some(path);
    }
    models_dirs
        .iter()
        .map(|d| d.join(p.replace('/', std::path::MAIN_SEPARATOR_STR)))
        .find(|c| c.is_file())
}

/// Where a missing file would be: under the primary models directory when relative.
fn unresolved(p: &str, models_dirs: &[PathBuf]) -> PathBuf {
    match models_dirs.first() {
        Some(d) if is_relative(p) => d.join(p.replace('/', std::path::MAIN_SEPARATOR_STR)),
        _ => PathBuf::from(p),
    }
}

/// Preset -> TOML value (JSON nulls dropped; TOML has none).
fn preset_to_toml(p: &InferencePreset) -> Result<toml::Value, String> {
    fn strip(v: serde_json::Value) -> Option<serde_json::Value> {
        match v {
            serde_json::Value::Null => None,
            serde_json::Value::Object(m) => Some(serde_json::Value::Object(
                m.into_iter()
                    .filter_map(|(k, v)| strip(v).map(|v| (k, v)))
                    .collect(),
            )),
            serde_json::Value::Array(a) => Some(serde_json::Value::Array(
                a.into_iter().filter_map(strip).collect(),
            )),
            other => Some(other),
        }
    }
    let mut copy = p.clone();
    copy.id = String::new();
    copy.builtin = false;
    copy.created_at = 0;
    copy.updated_at = 0;
    let mut json = serde_json::to_value(&copy)
        .map_err(|e| String::from(err("json.serialize_failed").with("error", e)))?;
    if let Some(m) = json.as_object_mut() {
        for k in ["id", "created_at", "updated_at", "builtin"] {
            m.remove(k);
        }
    }
    let json = strip(json).unwrap_or_default();
    toml::Value::try_from(json).map_err(|e| {
        err("portable.preset_to_toml_failed")
            .with("error", e)
            .into()
    })
}

/// `*.harness.toml` text of a definition (optionally with its default preset embedded).
pub fn export_definition(
    def: &ModelDefinition,
    preset: Option<&InferencePreset>,
    models_dirs: &[PathBuf],
) -> Result<String, String> {
    let rel = |p: &Path| portable_path(p, models_dirs);
    let (model, source) = match &def.base {
        ModelRef::Local { path } => {
            let m = rel(path);
            let source = is_relative(&m).then(|| source_of(&m)).flatten();
            (m, source)
        }
        hf @ ModelRef::Hf { .. } => (hf.to_string(), None),
    };
    let embedded = match (preset, def.default_preset_id.as_deref()) {
        (Some(p), _) if !p.builtin => Some(p),
        _ => None,
    };
    // A template file is embedded as text and dropped from `load` (its path means nothing on
    // another machine); an unreadable file keeps its path.
    let mut load = def.load.clone();
    let chat_template_text = match load.get(crate::chat_template::PARAM_ID) {
        Some(ParamValue::Text(p)) if !p.trim().is_empty() => {
            match crate::chat_template::read_template(Path::new(p.trim())) {
                Ok(text) if !text.trim().is_empty() => {
                    load.remove(crate::chat_template::PARAM_ID);
                    Some(text)
                }
                _ => None,
            }
        }
        _ => None,
    };
    let doc = PortableDefinition {
        format: FORMAT.to_string(),
        alias: def.alias.clone(),
        description: def.description.clone(),
        model,
        source,
        mmproj: def.mmproj.as_deref().map(rel),
        chat_template: def.chat_template.clone(),
        chat_template_kwargs: def.chat_template_kwargs.clone(),
        chat_template_text,
        extra_args: def.extra_args.clone(),
        default_preset: match embedded {
            Some(p) => Some(p.name.clone()),
            None => def.default_preset_id.clone().filter(|id| is_builtin_id(id)),
        },
        adapters: def
            .adapters
            .iter()
            .map(|a| PortableAdapter {
                path: rel(&a.path),
                scale: a.scale,
            })
            .collect(),
        draft: def.draft.as_ref().map(|d| PortableDraft {
            model: rel(&d.model),
            params: d.params.clone(),
        }),
        load,
        preset: embedded.map(preset_to_toml).transpose()?,
    };
    let body = toml::to_string_pretty(&doc)
        .map_err(|e| String::from(err("portable.toml_write_failed").with("error", e)))?;
    Ok(format!(
        "# Harness Studio model tanımı ({}). Yollar modeller klasörüne göredir.\n{}",
        FORMAT, body
    ))
}

/// What an import found (nothing is saved yet).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefinitionImport {
    /// The definition as it would be saved (`id` empty).
    pub definition: ModelDefinition,
    /// The base model file exists on this machine.
    pub model_found: bool,
    /// Hugging Face reference of the model (to download it when it's missing).
    pub hf_ref: Option<String>,
    /// Other referenced files (mmproj, adapters, draft) that were not found.
    pub missing: Vec<String>,
    /// The file's alias is already used here.
    pub alias_taken: bool,
    /// Free alias to use (`<alias>-2`... when taken).
    pub suggested_alias: String,
    /// Embedded default preset (saved as a new user preset on import).
    pub preset: Option<InferencePreset>,
    /// Embedded chat template text; the import saves it as a template file and points
    /// `load.chat_template_file` at it.
    #[serde(default)]
    pub chat_template_text: Option<String>,
}

/// Parses a `*.harness.toml` and resolves its paths on this machine.
pub fn parse_definition(
    text: &str,
    models_dirs: &[PathBuf],
    taken_aliases: &[String],
) -> Result<DefinitionImport, String> {
    let doc: PortableDefinition = toml::from_str(text)
        .map_err(|e| String::from(err("portable.invalid_toml").with("error", e)))?;
    if doc.format != FORMAT {
        return Err(err("portable.unsupported_format")
            .with("found", &doc.format)
            .with("expected", FORMAT)
            .into());
    }
    let alias = doc.alias.trim().to_string();
    super::validate_alias(&alias)?;
    // Base model: the path, else the Hugging Face origin at its download location.
    let hf_ref = match ModelRef::parse(&doc.model) {
        ModelRef::Hf { .. } => Some(doc.model.clone()),
        ModelRef::Local { .. } => doc.source.clone(),
    };
    let found = resolve_path(&doc.model, models_dirs)
        .or_else(|| hf_ref.as_deref().and_then(|r| resolve_path(r, models_dirs)));
    let (base, model_found) = match (found, &hf_ref) {
        (Some(path), _) => (ModelRef::Local { path }, true),
        (None, Some(r)) => (ModelRef::parse(r), false),
        (None, None) => (
            ModelRef::Local {
                path: unresolved(&doc.model, models_dirs),
            },
            false,
        ),
    };
    let mut missing = Vec::new();
    let mut file = |p: &str| -> PathBuf {
        match resolve_path(p, models_dirs) {
            Some(found) => found,
            None => {
                missing.push(p.to_string());
                unresolved(p, models_dirs)
            }
        }
    };

    let mut def = ModelDefinition::new(alias.clone(), base);
    def.description = doc.description.trim().to_string();
    def.mmproj = doc.mmproj.as_deref().map(&mut file);
    def.adapters = doc
        .adapters
        .iter()
        .map(|a| AdapterRef {
            path: file(&a.path),
            scale: a.scale,
        })
        .collect();
    def.draft = doc.draft.as_ref().map(|d| DraftSpec {
        model: file(&d.model),
        params: d.params.clone(),
    });
    def.chat_template = doc.chat_template.clone().filter(|s| !s.trim().is_empty());
    def.chat_template_kwargs = doc
        .chat_template_kwargs
        .clone()
        .filter(|s| !s.trim().is_empty());
    def.extra_args = doc.extra_args.clone().filter(|s| !s.trim().is_empty());
    def.load = doc.load.clone();
    super::launch::normalize_definition(&mut def);

    let preset = match &doc.preset {
        Some(v) => {
            let mut json = serde_json::to_value(v)
                .map_err(|e| String::from(err("json.serialize_failed").with("error", e)))?;
            if let Some(m) = json.as_object_mut() {
                m.insert("id".into(), serde_json::Value::String(String::new()));
                m.entry("name").or_insert_with(|| {
                    serde_json::Value::String(
                        doc.default_preset.clone().unwrap_or_else(|| alias.clone()),
                    )
                });
            }
            let mut p: InferencePreset = serde_json::from_value(json).map_err(|e| {
                String::from(err("portable.embedded_preset_invalid").with("error", e))
            })?;
            p.builtin = false;
            p.validate()?;
            Some(p)
        }
        None => None,
    };
    def.default_preset_id = match (&preset, doc.default_preset.as_deref()) {
        (Some(_), _) => None, // set to the new preset's id when it is saved
        (None, Some(id)) if is_builtin_id(id) => Some(id.to_string()),
        _ => None,
    };

    let alias_taken = taken_aliases.iter().any(|a| a.eq_ignore_ascii_case(&alias));
    let suggested_alias = super::unique_alias(&alias, taken_aliases.iter().map(|s| s.as_str()));
    Ok(DefinitionImport {
        definition: def,
        model_found,
        hf_ref,
        missing,
        alias_taken,
        suggested_alias,
        preset,
        chat_template_text: doc
            .chat_template_text
            .clone()
            .filter(|s| !s.trim().is_empty()),
    })
}

/// File name for an export (`<alias>.harness.toml`, safe on every OS).
pub fn file_name(alias: &str) -> String {
    let safe: String = alias
        .chars()
        .map(|c| if c == ':' { '-' } else { c })
        .collect();
    format!("{}{}", safe, EXTENSION)
}

#[cfg(test)]
mod portable_path_tests {
    use super::*;

    #[test]
    fn relative_to_models_dir_by_components() {
        let dirs = [PathBuf::from("C:/models")];
        assert_eq!(
            portable_path(Path::new(r"C:\models\org\repo\m.gguf"), &dirs),
            "org/repo/m.gguf"
        );
        // A sibling sharing a prefix and the directory itself are not inside.
        assert_eq!(
            portable_path(Path::new("C:/models2/a.gguf"), &dirs),
            "C:/models2/a.gguf"
        );
        assert_eq!(portable_path(Path::new("C:/models"), &dirs), "C:/models");
    }

    #[test]
    fn multibyte_paths_do_not_truncate_or_panic() {
        // Lowercasing U+0130 yields a longer string, so folded offsets used to cut the original.
        let dirs = [PathBuf::from("C:/Kullan\u{131}c\u{131}/\u{130}stanbul")];
        let p = Path::new("C:/Kullan\u{131}c\u{131}/\u{130}stanbul/\u{e7}ok/model.gguf");
        assert_eq!(portable_path(p, &dirs), "\u{e7}ok/model.gguf");
        let p = Path::new("C:/Kullan\u{131}c\u{131}/\u{130}stanbul/model.gguf");
        assert_eq!(portable_path(p, &dirs), "model.gguf");
        let q = Path::new("C:/Kullan\u{131}c\u{131}/\u{c7}ay/m.gguf");
        assert_eq!(
            portable_path(q, &dirs),
            "C:/Kullan\u{131}c\u{131}/\u{c7}ay/m.gguf"
        );
    }
}
