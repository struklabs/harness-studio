//! Agent Skills: metadata discovery and explicitly requested, confined resource reads.
use crate::{store::JsonStore, tools::Workspace};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const LIMIT: u64 = 1024 * 1024;

pub fn root() -> PathBuf {
    crate::paths::data_root().join("skills")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    description: String,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

pub fn parse(text: &str) -> Result<(String, String, String), String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = text
        .strip_prefix("---\n")
        .ok_or("SKILL.md requires YAML frontmatter")?;
    let (header, body) = rest
        .split_once("\n---\n")
        .ok_or("Missing frontmatter delimiter")?;
    let header: Header =
        serde_norway::from_str(header).map_err(|e| format!("Invalid skill metadata: {e}"))?;
    if !valid_name(&header.name)
        || header.description.trim().is_empty()
        || header.description.chars().count() > 1024
        || body.trim().is_empty()
    {
        return Err("Invalid skill name, description or empty instructions".into());
    }
    Ok((header.name, header.description, body.trim().into()))
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    disabled: BTreeSet<String>,
}
fn state(root: &Path) -> State {
    JsonStore::new(root.join("state.json"))
        .load()
        .unwrap_or_default()
}
fn save(root: &Path, s: &State) -> Result<(), String> {
    JsonStore::new(root.join("state.json")).save(s)
}

fn directory(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err("Invalid skill name".into());
    }
    let path = root.join(name);
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("Skill must be a regular directory".into());
    }
    Workspace::new(root)
        .map_err(|e| e.message)?
        .resolve_existing(name)
        .map_err(|e| e.message)
}

fn read_at(root: &Path, name: &str, resource: &str) -> Result<String, String> {
    let dir = directory(root, name)?;
    if resource.contains('\\')
        || resource.contains(':')
        || resource
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || Path::new(resource).is_absolute()
    {
        return Err("Resource must be relative to the skill folder".into());
    }
    let path = Workspace::new(&dir)
        .map_err(|e| e.message)?
        .resolve_existing(resource)
        .map_err(|e| e.message)?;
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > LIMIT {
        return Err("Skill resource must be a UTF-8 file of at most 1 MiB".into());
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

pub fn list(root: &Path) -> Vec<Skill> {
    let disabled = state(root).disabled;
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !valid_name(&name) {
                continue;
            }
            let parsed = read_at(root, &name, "SKILL.md")
                .and_then(|s| parse(&s))
                .and_then(|s| {
                    if s.0 == name {
                        Ok(s)
                    } else {
                        Err("Skill name must match its folder".into())
                    }
                });
            let (description, error) = match parsed {
                Ok((_, d, _)) => (d, None),
                Err(e) => (String::new(), Some(e)),
            };
            out.push(Skill {
                enabled: !disabled.contains(&name) && error.is_none(),
                name,
                description,
                error,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn set_enabled(root: &Path, name: &str, enabled: bool) -> Result<(), String> {
    directory(root, name)?;
    if enabled {
        parse(&read_at(root, name, "SKILL.md")?)?;
    }
    let mut s = state(root);
    if enabled {
        s.disabled.remove(name);
    } else {
        s.disabled.insert(name.into());
    }
    save(root, &s)
}

/// Record external content as disabled before publishing its directory.
pub(crate) fn disable_for_install(root: &Path, name: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err("Invalid skill name".into());
    }
    let mut s = state(root);
    s.disabled.insert(name.into());
    save(root, &s)
}

pub fn remove(root: &Path, name: &str) -> Result<(), String> {
    let path = directory(root, name)?;
    std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    let mut s = state(root);
    s.disabled.remove(name);
    save(root, &s)
}

/// `allowed-tools` metadata never grants permissions. Resources are read, never executed.
pub fn load(root: &Path, name: &str, resource: Option<&str>) -> Result<String, String> {
    if !list(root).iter().any(|s| s.name == name && s.enabled) {
        return Err("Skill is disabled, invalid or unavailable".into());
    }
    let text = read_at(root, name, resource.unwrap_or("SKILL.md"))?;
    if resource.is_none() {
        Ok(parse(&text)?.2)
    } else {
        Ok(text)
    }
}

pub fn install(root: &Path, text: &str) -> Result<Skill, String> {
    let (name, description, _) = parse(text)?;
    if text.len() as u64 > LIMIT {
        return Err("Skill exceeds 1 MiB".into());
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = root.join(&name);
    if target.exists() {
        return Err("Skill already exists; remove it before installing".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".skill-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    std::fs::write(staging.path().join("SKILL.md"), text).map_err(|e| e.to_string())?;
    std::fs::rename(staging.path(), &target).map_err(|e| e.to_string())?;
    let mut s = state(root);
    if s.disabled.remove(&name) {
        save(root, &s)?;
    }
    Ok(Skill {
        name,
        description,
        enabled: true,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEXT: &str = "---\nname: example\ndescription: |\n  Use for examples.\nlicense: MIT\n---\nPrivate body. See references/guide.md.";
    #[test]
    fn metadata_lifecycle_and_lazy_resources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        install(root, TEXT).unwrap();
        assert!(install(root, TEXT).is_err());
        let items = list(root);
        assert_eq!(items.len(), 1);
        assert!(!serde_json::to_string(&items)
            .unwrap()
            .contains("Private body"));
        assert!(load(root, "example", None)
            .unwrap()
            .contains("Private body"));
        std::fs::create_dir(root.join("example/references")).unwrap();
        std::fs::write(root.join("example/references/guide.md"), "Guide").unwrap();
        assert_eq!(
            load(root, "example", Some("references/guide.md")).unwrap(),
            "Guide"
        );
        for p in [
            "../state.json",
            "/etc/passwd",
            "C:/secret",
            "references/../../state.json",
        ] {
            assert!(load(root, "example", Some(p)).is_err());
        }
        set_enabled(root, "example", false).unwrap();
        assert!(load(root, "example", None).is_err());
        set_enabled(root, "example", true).unwrap();
        assert!(list(root)[0].enabled);
        remove(root, "example").unwrap();
        assert!(list(root).is_empty());
    }
    #[test]
    fn validates_standard_frontmatter() {
        assert!(parse(&TEXT.replace('\n', "\r\n")).is_ok());
        for s in [
            TEXT.replace("example", "Bad_Name"),
            TEXT.replace("description:", "missing:"),
            TEXT.replace("example", "bad--name"),
            "No metadata".into(),
        ] {
            assert!(parse(&s).is_err());
        }
    }

    #[test]
    fn invalid_and_oversized_resources_are_excluded_without_loading_other_bodies() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        install(root, TEXT).unwrap();
        let folder = root.join("example");
        std::fs::write(folder.join("large.txt"), vec![b'x'; LIMIT as usize + 1]).unwrap();
        assert!(load(root, "example", Some("large.txt")).is_err());
        std::fs::write(folder.join("binary"), [255]).unwrap();
        assert!(load(root, "example", Some("binary")).is_err());
        std::fs::write(
            folder.join("SKILL.md"),
            TEXT.replace("name: example", "name: other"),
        )
        .unwrap();
        assert!(list(root)[0].error.is_some());
        assert!(!list(root)[0].enabled);
        assert!(load(root, "example", None).is_err());
        remove(root, "example").unwrap();
        assert!(list(root).is_empty());
    }
}
