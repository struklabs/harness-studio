//! Versioned office scenes and atomic JSON collections in the owner's data directory.
use crate::store::JsonStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct OfficeRoom {
    pub id: String,
    pub name: String,
    pub color: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct OfficeFurniture {
    pub id: String,
    pub kind: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct OfficeScene {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub size: String,
    pub theme: String,
    pub rooms: Vec<OfficeRoom>,
    pub furniture: Vec<OfficeFurniture>,
    pub desk_agents: BTreeMap<String, String>,
    pub room_projects: BTreeMap<String, String>,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 200 && !s.chars().any(char::is_control)
}
pub fn dimensions(size: &str) -> Option<(u32, u32)> {
    match size {
        "small" => Some((16, 12)),
        "medium" => Some((24, 16)),
        "large" => Some((32, 20)),
        _ => None,
    }
}
impl OfficeScene {
    pub fn validate(&self) -> Result<(), String> {
        let invalid = || "office.invalid_scene".to_string();
        let (w, h) = dimensions(&self.size).ok_or_else(invalid)?;
        if self.version != 1
            || !valid_id(&self.id)
            || !text(&self.name)
            || !matches!(self.theme.as_str(), "classic" | "pixel" | "night")
            || self.rooms.is_empty()
            || self.rooms.len() > 32
            || self.furniture.len() > 256
            || self.desk_agents.len() > 256
            || self.room_projects.len() > 32
        {
            return Err(invalid());
        }
        let bounds = |x: u32, y: u32, rw: u32, rh: u32| {
            rw > 0 && rh > 0 && x < w && y < h && rw <= w - x && rh <= h - y
        };
        let mut ids = HashSet::new();
        for r in &self.rooms {
            if !valid_id(&r.id)
                || !ids.insert(&r.id)
                || !text(&r.name)
                || r.color.len() != 7
                || !r.color.starts_with('#')
                || !r.color[1..].bytes().all(|b| b.is_ascii_hexdigit())
                || !bounds(r.x, r.y, r.width, r.height)
            {
                return Err(invalid());
            }
        }
        for (i, a) in self.rooms.iter().enumerate() {
            if self.rooms.iter().skip(i + 1).any(|b| {
                a.x < b.x + b.width
                    && b.x < a.x + a.width
                    && a.y < b.y + b.height
                    && b.y < a.y + a.height
            }) {
                return Err(invalid());
            }
        }
        for f in &self.furniture {
            if !valid_id(&f.id)
                || !ids.insert(&f.id)
                || !matches!(
                    f.kind.as_str(),
                    "desk" | "meeting" | "kitchen" | "plant" | "board"
                )
                || !bounds(f.x, f.y, f.width, f.height)
            {
                return Err(invalid());
            }
        }
        let mut agents = HashSet::new();
        for (desk, agent) in &self.desk_agents {
            if !text(agent)
                || !agents.insert(agent)
                || !self
                    .furniture
                    .iter()
                    .any(|f| f.id == *desk && f.kind == "desk")
            {
                return Err(invalid());
            }
        }
        for (room, project) in &self.room_projects {
            if !text(project) || !self.rooms.iter().any(|r| r.id == *room) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
pub fn builtins() -> Vec<OfficeScene> {
    serde_json::from_str(include_str!("../../../shared/office-presets.json"))
        .expect("bundled office presets")
}
fn collection(root: &Path, presets: bool) -> JsonStore {
    JsonStore::new(collection_path(root, presets))
}
fn collection_path(root: &Path, presets: bool) -> std::path::PathBuf {
    root.join("agent-office").join(if presets {
        "presets.json"
    } else {
        "offices.json"
    })
}
pub fn list(root: &Path, presets: bool) -> Result<Vec<OfficeScene>, String> {
    let mut entries: Vec<OfficeScene> = if collection_path(root, presets).exists() {
        collection(root, presets).load()?
    } else {
        Vec::new()
    };
    for scene in &entries {
        scene.validate()?;
    }
    if presets {
        entries.splice(0..0, builtins());
    }
    Ok(entries)
}
fn task_views(root: &Path) -> Result<Vec<crate::modules::agents::TaskStatus>, String> {
    let path = root.join("agents/history.json");
    let mut tasks: Vec<crate::modules::agents::TaskStatus> = if path.exists() {
        JsonStore::sensitive(path).load()?
    } else {
        Vec::new()
    };
    let active =
        if crate::paths::normalize(root) == crate::paths::normalize(&crate::paths::data_root()) {
            Some(crate::modules::agents::task_status())
        } else {
            None
        };
    for task in &mut tasks {
        if let Some(live) = active
            .as_ref()
            .filter(|live| !live.id.is_empty() && live.id == task.id)
        {
            *task = live.clone();
        } else if task.status == "running" {
            task.status = "interrupted".into();
        }
    }
    Ok(tasks)
}
pub fn save(root: &Path, presets: bool, scene: OfficeScene) -> Result<OfficeScene, String> {
    scene.validate()?;
    if scene.id.starts_with("builtin-") {
        return Err("office.builtin_readonly".into());
    }
    collection(root, presets).update::<Vec<OfficeScene>, _>(|v| {
        for existing in v.iter() {
            existing.validate()?;
        }
        if v.len() >= 100 && !v.iter().any(|s| s.id == scene.id) {
            return Err("office.limit".into());
        }
        v.retain(|s| s.id != scene.id);
        v.push(scene.clone());
        Ok(())
    })?;
    crate::events::publish(crate::events::CoreEvent::OfficesChanged);
    Ok(scene)
}
pub fn remove(root: &Path, presets: bool, id: &str) -> Result<bool, String> {
    if id.starts_with("builtin-") {
        return Err("office.builtin_readonly".into());
    }
    let removed = collection(root, presets).update::<Vec<OfficeScene>, _>(|v| {
        let len = v.len();
        v.retain(|s| s.id != id);
        Ok(len != v.len())
    })?;
    if removed {
        crate::events::publish(crate::events::CoreEvent::OfficesChanged);
    }
    Ok(removed)
}

/// Summary fields only. Prompts and output are available through the privileged run detail.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OfficeCharacter {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub run_id: Option<String>,
    pub project_id: Option<String>,
}
pub fn status(raw: &str) -> &'static str {
    match raw {
        "running" | "loading" | "thinking" => "thinking",
        "tool" | "running_tool" => "running_tool",
        "waiting_for_approval" => "waiting_for_approval",
        "failed" | "error" | "interrupted" => "error",
        "queued" | "stopped" | "sleeping" | "cancelling" => "sleeping",
        _ => "idle",
    }
}
pub fn activity(root: &Path) -> Result<Vec<OfficeCharacter>, String> {
    let tasks = task_views(root)?;
    let mut result = Vec::new();
    for kind in [
        crate::modules::agents::AgentKind::Hermes,
        crate::modules::agents::AgentKind::OpenClaw,
    ] {
        let key = serde_json::to_value(kind)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let latest = tasks
            .iter()
            .filter(|t| t.agent == kind)
            .max_by_key(|t| t.started_at);
        result.push(OfficeCharacter {
            id: format!("agent-{key}"),
            name: key,
            kind: "agent".into(),
            status: status(latest.map_or("idle", |t| t.status.as_str())).into(),
            run_id: latest.map(|t| t.id.clone()),
            project_id: latest.and_then(|t| t.project_id.clone()),
        });
    }
    for job in crate::jobs::global().snapshot() {
        result.push(OfficeCharacter {
            id: format!("job-{}", job.id),
            name: job.label,
            kind: "job".into(),
            status: status(match job.status {
                crate::jobs::Status::Running => "running_tool",
                crate::jobs::Status::Loading => "thinking",
                _ => "sleeping",
            })
            .into(),
            run_id: Some(job.id.to_string()),
            project_id: None,
        });
    }
    let hw = crate::hardware::detect_hardware();
    result.push(OfficeCharacter {
        id: "device-cpu".into(),
        name: hw.cpu_name,
        kind: "device".into(),
        status: "idle".into(),
        run_id: None,
        project_id: None,
    });
    for gpu in hw.gpus {
        result.push(OfficeCharacter {
            id: format!("device-gpu-{}", gpu.index),
            name: gpu.name,
            kind: "device".into(),
            status: "idle".into(),
            run_id: None,
            project_id: None,
        });
    }
    Ok(result)
}

/// Reads one stored task without exposing filesystem grants or credentials.
pub fn run(root: &Path, id: &str) -> Result<Option<serde_json::Value>, String> {
    let tasks = task_views(root)?;
    Ok(tasks.into_iter().find(|t|t.id==id).map(|t|serde_json::json!({"id":t.id,"status":t.status,"prompt":t.prompt,"output":t.output,"error":t.error,"audit":t.audit,"project_id":t.project_id})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scene_validation_rejects_bad_geometry_and_references() {
        for scene in builtins() {
            scene.validate().unwrap();
        }
        let mut s = builtins().remove(0);
        s.rooms[0].width = u32::MAX;
        assert!(s.validate().is_err());
        let mut s = builtins().remove(0);
        s.version = 2;
        assert!(s.validate().is_err());
        let mut s = builtins().remove(0);
        s.desk_agents.insert("missing".into(), "a".into());
        assert!(s.validate().is_err());
        let mut s = builtins().remove(0);
        s.rooms.push(s.rooms[0].clone());
        assert!(s.validate().is_err());
    }
    #[test]
    fn preset_crud_is_durable_and_builtins_are_immutable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(list(root, true).unwrap().len(), 4);
        let mut s = builtins().remove(0);
        assert!(save(root, true, s.clone()).is_err());
        s.id = "custom".into();
        save(root, true, s.clone()).unwrap();
        s.name = "Renamed".into();
        save(root, true, s.clone()).unwrap();
        assert_eq!(list(root, true).unwrap().last(), Some(&s));
        save(root, false, s).unwrap();
        assert_eq!(list(root, false).unwrap().len(), 1);
        assert!(remove(root, true, "custom").unwrap());
        assert!(!remove(root, true, "custom").unwrap());
        assert_eq!(list(root, true).unwrap().len(), 4);
        assert!(remove(root, true, "builtin-studio").is_err());
        std::fs::write(root.join("agent-office/presets.json"), "broken").unwrap();
        assert!(list(root, true).is_err());
    }
    #[test]
    fn activity_statuses_are_explicit() {
        for (raw, expected) in [
            ("running", "thinking"),
            ("running_tool", "running_tool"),
            ("waiting_for_approval", "waiting_for_approval"),
            ("failed", "error"),
            ("stopped", "sleeping"),
            ("completed", "idle"),
        ] {
            assert_eq!(status(raw), expected);
        }
    }
}
