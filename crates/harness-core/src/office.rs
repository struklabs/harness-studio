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
    /// Model identity for taskflow runs, when one has been selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}
pub fn status(raw: &str) -> &'static str {
    match raw {
        "running" | "loading" | "thinking" => "thinking",
        "tool" | "running_tool" => "running_tool",
        "waiting_for_approval" | "awaiting_approval" => "waiting_for_approval",
        "failed" | "error" | "interrupted" => "error",
        "queued" | "stopped" | "sleeping" | "cancelling" | "paused" => "sleeping",
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
            agent: None,
        });
    }
    if let Some(db) = task_db(root)? {
        for run in db.office_task_runs().map_err(|e| e.to_string())? {
            let mut character = task_character(&run);
            if run.status == "running" {
                let steps = db.task_steps(&run.id).map_err(|e| e.to_string())?;
                if let Some(model) = steps
                    .iter()
                    .find(|s| s.status == "running")
                    .and_then(|s| s.model.as_ref())
                {
                    character.agent = Some(model.clone());
                }
            }
            result.push(character);
        }
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
            agent: None,
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
        agent: None,
    });
    for gpu in hw.gpus {
        result.push(OfficeCharacter {
            id: format!("device-gpu-{}", gpu.index),
            name: gpu.name,
            kind: "device".into(),
            status: "idle".into(),
            run_id: None,
            project_id: None,
            agent: None,
        });
    }
    Ok(result)
}

/// Reads one stored task without exposing filesystem grants or credentials.
pub fn run(root: &Path, id: &str) -> Result<Option<serde_json::Value>, String> {
    if let Some(id) = id.strip_prefix("taskflow-") {
        let Some(db) = task_db(root)? else {
            return Ok(None);
        };
        // A missing run keeps the endpoint's existing 404 behavior.
        let run = match db.task_run(id) {
            Ok(run) => run,
            Err(crate::state::StateError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let steps = db.task_steps(id).map_err(|e| e.to_string())?;
        let events = db.task_events(id).map_err(|e| e.to_string())?;
        return Ok(Some(serde_json::json!({
            "id": format!("taskflow-{}", run.id), "status": run.status,
            "prompt": run.goal, "output": steps.iter().map(|s| s.output.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n"),
            "error": run.error, "audit": events.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(),
            "project_id": run.project_id
        })));
    }
    let tasks = task_views(root)?;
    Ok(tasks.into_iter().find(|t|t.id==id).map(|t|serde_json::json!({"id":t.id,"status":t.status,"prompt":t.prompt,"output":t.output,"error":t.error,"audit":t.audit,"project_id":t.project_id})))
}

fn task_db(root: &Path) -> Result<Option<std::sync::Arc<crate::state::StateDb>>, String> {
    let path = root.join("state.db");
    if !path.exists() {
        return Ok(None);
    }
    if crate::paths::normalize(root) == crate::paths::normalize(&crate::paths::data_root()) {
        return crate::state::shared().map(Some);
    }
    crate::state::StateDb::open(&path)
        .map(std::sync::Arc::new)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn task_character(run: &crate::state::TaskRun) -> OfficeCharacter {
    let id = format!("taskflow-{}", run.id);
    let activity = if run.status == "running"
        && crate::state::now_ms() - run.heartbeat_at >= crate::state::HEARTBEAT_STALE_MS
    {
        "interrupted"
    } else {
        &run.status
    };
    OfficeCharacter {
        id: id.clone(),
        name: run.title.clone(),
        kind: "agent".into(),
        status: status(activity).into(),
        run_id: Some(id),
        project_id: run.project_id.clone(),
        agent: run
            .last_model
            .clone()
            .or_else(|| run.policy.models.first().cloned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn taskflow_characters_cover_concurrent_projects_gates_and_details() {
        use crate::state::{NewTaskRun, ProjectInput, RunPolicy, StateDb, StepInput};
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let projects: Vec<_> = ["First", "Second"]
            .iter()
            .map(|name| {
                db.create_project(&ProjectInput {
                    name: (*name).into(),
                    ..Default::default()
                })
                .unwrap()
            })
            .collect();
        let mut ids = Vec::new();
        for i in 0..30 {
            let run = db
                .create_task_run(&NewTaskRun {
                    title: format!("Task {i}"),
                    goal: "Goal".into(),
                    project_id: Some(projects[i % 2].id.clone()),
                    policy: RunPolicy {
                        models: vec![format!("model-{i}")],
                        ..Default::default()
                    },
                    steps: vec![StepInput {
                        prompt: "Step".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .unwrap();
            db.claim_task_run(&run.id, 30).unwrap();
            ids.push(run.id);
        }
        db.set_task_run_status(&ids[0], "awaiting_approval", None)
            .unwrap();
        db.set_task_run_status(&ids[1], "paused", None).unwrap();
        let runs = db.office_task_runs().unwrap();
        assert_eq!(runs.len(), 30);
        let characters: Vec<_> = runs.iter().map(task_character).collect();
        assert_eq!(
            characters
                .iter()
                .map(|c| &c.id)
                .collect::<HashSet<_>>()
                .len(),
            30
        );
        let gated = characters
            .iter()
            .find(|c| c.id == format!("taskflow-{}", ids[0]))
            .unwrap();
        assert_eq!(gated.status, "waiting_for_approval");
        assert_eq!(gated.agent.as_deref(), Some("model-0"));
        assert_eq!(gated.project_id.as_deref(), Some(projects[0].id.as_str()));
        let paused = characters
            .iter()
            .find(|c| c.id == format!("taskflow-{}", ids[1]))
            .unwrap();
        assert_eq!(paused.status, "sleeping");
        let mut running = db.task_run(&ids[2]).unwrap();
        assert_eq!(task_character(&running).status, "thinking");
        running.heartbeat_at = crate::state::now_ms() - crate::state::HEARTBEAT_STALE_MS - 1;
        assert_eq!(task_character(&running).status, "error");
        assert_eq!(
            run(dir.path(), gated.run_id.as_ref().unwrap())
                .unwrap()
                .unwrap()["prompt"],
            "Goal"
        );
        assert!(run(dir.path(), "taskflow-missing").unwrap().is_none());
        db.set_task_run_status(&ids[2], "completed", None).unwrap();
        assert_eq!(db.office_task_runs().unwrap().len(), 29);
        let mut events = crate::events::subscribe();
        db.set_task_run_status(&ids[3], "awaiting_approval", None)
            .unwrap();
        let events: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert!(events
            .iter()
            .any(|e| matches!(e, crate::events::CoreEvent::OfficeActivityChanged)));
        assert_eq!(db.task_run(&ids[3]).unwrap().status, "awaiting_approval");
    }
    #[test]
    fn taskflow_activity_uses_running_step_model_and_details_keep_outputs() {
        use crate::state::{NewTaskRun, RunPolicy, StateDb, StepInput, StepResult};
        let dir = tempfile::tempdir().unwrap();
        assert!(run(dir.path(), "taskflow-missing").unwrap().is_none());
        assert!(!dir.path().join("state.db").exists());
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let task = db
            .create_task_run(&NewTaskRun {
                title: "Model fallback".into(),
                goal: "Private goal".into(),
                policy: RunPolicy {
                    models: vec!["preferred-model".into()],
                    ..Default::default()
                },
                steps: vec![
                    StepInput {
                        prompt: "First step".into(),
                        ..Default::default()
                    },
                    StepInput {
                        prompt: "Second step".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
            .unwrap();
        db.claim_task_run(&task.id, 1).unwrap();
        db.begin_task_step(&task.id, 0).unwrap();
        db.finish_task_step(
            &task.id,
            0,
            &StepResult {
                model: "previous-model".into(),
                output: "First output".into(),
                ..Default::default()
            },
        )
        .unwrap();
        db.begin_task_step(&task.id, 1).unwrap();
        db.note_task_step(&task.id, 1, Some("running-model"), None, 1)
            .unwrap();
        let id = format!("taskflow-{}", task.id);
        let character = activity(dir.path())
            .unwrap()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap();
        assert_eq!(character.agent.as_deref(), Some("running-model"));
        assert_eq!(character.status, "thinking");
        let summary = serde_json::to_value(character).unwrap();
        assert!(summary.get("prompt").is_none());
        assert!(summary.get("output").is_none());
        db.finish_task_step(
            &task.id,
            1,
            &StepResult {
                model: "running-model".into(),
                output: "Second output".into(),
                ..Default::default()
            },
        )
        .unwrap();
        db.set_task_run_status(&task.id, "completed", None).unwrap();
        assert!(db.office_task_runs().unwrap().is_empty());
        let detail = run(dir.path(), &id).unwrap().unwrap();
        assert_eq!(detail["status"], "completed");
        assert_eq!(detail["prompt"], "Private goal");
        assert_eq!(detail["output"], "First output\n\nSecond output");
        assert!(detail["audit"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("step_done")));
    }
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
