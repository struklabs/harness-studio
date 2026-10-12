//! Custom chat templates: Jinja files kept in `<config dir>/chat-templates/` that a model
//! definition references through `load.chat_template_file` (`--chat-template-file`).
//!
//! The template editor of the definition dialog saves a file here (from scratch or from the
//! template embedded in the GGUF) and the definition stores its path. A portable
//! `*.harness.toml` embeds the template text instead of that machine-specific path; the
//! import writes it back as a file here (see `domain::portable`).

use crate::error_key::err;
use crate::paths::sanitize_file_name;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Catalog id of the template file parameter (`--chat-template-file`).
pub const PARAM_ID: &str = "chat_template_file";
/// Extension of saved templates.
pub const EXTENSION: &str = "jinja";
/// Refuse absurd inputs: real templates are a few KiB.
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedTemplate {
    /// File name without extension.
    pub name: String,
    pub path: PathBuf,
}

/// A file name (without extension) safe on every OS; never empty.
/// Only a `.jinja` suffix is dropped: model names such as `qwen2.5-1.5b` keep their dots.
fn safe_stem(name: &str) -> String {
    let base = Path::new(name.trim())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let suffix = format!(".{EXTENSION}");
    let stem = match base.len().checked_sub(suffix.len()) {
        Some(cut) if base.is_char_boundary(cut) && base[cut..].eq_ignore_ascii_case(&suffix) => {
            &base[..cut]
        }
        _ => base.as_str(),
    };
    sanitize_file_name(stem).unwrap_or_else(|| "template".to_string())
}

/// Writes `text` as `<dir>/<name>.jinja` (created if needed; an existing file of the same
/// name is replaced, which is how the editor saves changes).
pub fn save_template(dir: &Path, name: &str, text: &str) -> Result<SavedTemplate, String> {
    if text.len() > MAX_BYTES {
        return Err(err("fs.write_failed")
            .with("path", dir.display())
            .with("error", "template too large")
            .into());
    }
    let stem = safe_stem(name);
    let path = dir.join(format!("{stem}.{EXTENSION}"));
    fs::create_dir_all(dir).map_err(|e| write_error(&path, e))?;
    fs::write(&path, text).map_err(|e| write_error(&path, e))?;
    Ok(SavedTemplate { name: stem, path })
}

/// Like [`save_template`] but never replaces a file: a taken name gets `-2`, `-3`...
pub fn save_template_unique(dir: &Path, name: &str, text: &str) -> Result<SavedTemplate, String> {
    let stem = safe_stem(name);
    let mut candidate = stem.clone();
    let mut n = 2;
    while dir.join(format!("{candidate}.{EXTENSION}")).exists() {
        candidate = format!("{stem}-{n}");
        n += 1;
    }
    save_template(dir, &candidate, text)
}

/// Reads a template file.
pub fn read_template(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| {
        err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    })
}

/// Saved templates, sorted by name.
pub fn list_templates(dir: &Path) -> Vec<SavedTemplate> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<SavedTemplate> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION))
        })
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().to_string();
            Some(SavedTemplate { name, path: p })
        })
        .collect();
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

fn write_error(path: &Path, e: std::io::Error) -> String {
    err("fs.write_failed")
        .with("path", path.display())
        .with("error", e)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str =
        "{%- for m in messages -%}\n<|{{ m.role }}|>{{ m.content }}\n{%- endfor -%}\n";

    #[test]
    fn save_read_list_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let tpl = dir.path().join("chat-templates");
        assert!(list_templates(&tpl).is_empty());
        let saved = save_template(&tpl, "Qwen özel", TEMPLATE).unwrap();
        assert_eq!(saved.path, tpl.join("Qwen özel.jinja"));
        assert_eq!(read_template(&saved.path).unwrap(), TEMPLATE);
        // Saving again under the same name replaces (the editor's "save").
        save_template(&tpl, "Qwen özel", "x").unwrap();
        assert_eq!(read_template(&saved.path).unwrap(), "x");
        save_template(&tpl, "a", "y").unwrap();
        let names: Vec<String> = list_templates(&tpl).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["a", "Qwen özel"]);
    }

    #[test]
    fn model_names_keep_their_dots() {
        let dir = tempfile::tempdir().unwrap();
        let saved = save_template(dir.path(), "qwen2.5-1.5b-instruct-q4_k_m", "x").unwrap();
        assert_eq!(saved.name, "qwen2.5-1.5b-instruct-q4_k_m");
        let saved = save_template(dir.path(), "llama3.1.JINJA", "x").unwrap();
        assert_eq!(saved.name, "llama3.1");
    }

    #[test]
    fn names_cannot_escape_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let saved = save_template(dir.path(), "../../evil.jinja", "x").unwrap();
        assert_eq!(saved.path, dir.path().join("evil.jinja"));
        let saved = save_template(dir.path(), "  ", "x").unwrap();
        assert_eq!(saved.path, dir.path().join("template.jinja"));
    }

    #[test]
    fn unique_save_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let a = save_template_unique(dir.path(), "m", "one").unwrap();
        let b = save_template_unique(dir.path(), "m", "two").unwrap();
        assert_ne!(a.path, b.path);
        assert_eq!(b.name, "m-2");
        assert_eq!(read_template(&a.path).unwrap(), "one");
        assert_eq!(read_template(&b.path).unwrap(), "two");
    }
}
