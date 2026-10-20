//! Projects in `state.db`: the project rows, which conversations belong to them and the files
//! made for them. Folder checks, file handling and export live in `crate::projects`.

use super::{new_id, now_ms, StateDb, StateError, StateResult};
use crate::error_key::err;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

/// What a project allows and brings along. Stored as `projects.settings_json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectSettings {
    /// Folders besides the project folder that tools and agents may use.
    pub allowed_folders: Vec<String>,
    /// Attached Harness tools, skills and MCP servers (by name). Empty = no preference.
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The project's own folder (absolute); `None` = the project has no folder yet.
    pub folder: Option<String>,
    #[serde(flatten)]
    pub settings: ProjectSettings,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub chat_count: i64,
    pub output_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProjectInput {
    pub name: String,
    pub description: String,
    pub folder: Option<String>,
    #[serde(flatten)]
    pub settings: ProjectSettings,
}

/// A file made for a project and where it came from.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectOutput {
    pub id: String,
    pub project_id: String,
    pub path: String,
    /// `image | audio | text | file`.
    pub kind: String,
    /// `chat | tool | image | speech | agent | manual`.
    pub source: String,
    pub model: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_task_id: Option<String>,
    pub tool: Option<String>,
    pub size: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NewOutput {
    pub path: String,
    pub kind: String,
    pub source: String,
    pub model: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_task_id: Option<String>,
    pub tool: Option<String>,
    pub size: i64,
}

const PROJECT_COLS: &str = "p.id, p.name, p.description, p.folder, p.settings_json, p.archived, \
     p.created_at, p.updated_at, \
     (SELECT COUNT(*) FROM conversations c WHERE c.project_id = p.id), \
     (SELECT COUNT(*) FROM project_outputs o WHERE o.project_id = p.id)";

fn project_from_row(r: &Row<'_>) -> rusqlite::Result<Project> {
    let settings: String = r.get(4)?;
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        folder: r.get(3)?,
        settings: serde_json::from_str(&settings).unwrap_or_default(),
        archived: r.get::<_, i64>(5)? != 0,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        chat_count: r.get(8)?,
        output_count: r.get(9)?,
    })
}

const OUTPUT_COLS: &str = "id, project_id, path, kind, source, model, conversation_id, \
     agent_task_id, tool, size, created_at";

fn output_from_row(r: &Row<'_>) -> rusqlite::Result<ProjectOutput> {
    Ok(ProjectOutput {
        id: r.get(0)?,
        project_id: r.get(1)?,
        path: r.get(2)?,
        kind: r.get(3)?,
        source: r.get(4)?,
        model: r.get(5)?,
        conversation_id: r.get(6)?,
        agent_task_id: r.get(7)?,
        tool: r.get(8)?,
        size: r.get(9)?,
        created_at: r.get(10)?,
    })
}

fn not_found(id: &str) -> StateError {
    StateError::not_found(err("projects.not_found").with("id", id))
}

pub(crate) fn load_project(conn: &Connection, id: &str) -> StateResult<Project> {
    conn.query_row(
        &format!("SELECT {PROJECT_COLS} FROM projects p WHERE p.id = ?1"),
        [id],
        project_from_row,
    )
    .optional()?
    .ok_or_else(|| not_found(id))
}

fn clean_name(name: &str) -> StateResult<String> {
    let name = name.trim().chars().take(120).collect::<String>();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(StateError::invalid(err("projects.invalid_name")));
    }
    Ok(name)
}

fn clean_kind(kind: &str) -> &'static str {
    match kind {
        "image" => "image",
        "audio" => "audio",
        "text" => "text",
        _ => "file",
    }
}

fn clean_source(source: &str) -> &'static str {
    match source {
        "chat" => "chat",
        "tool" => "tool",
        "image" => "image",
        "speech" => "speech",
        "agent" => "agent",
        _ => "manual",
    }
}

fn label(s: Option<&String>) -> Option<String> {
    s.map(|s| s.trim().chars().take(256).collect::<String>())
        .filter(|s| !s.is_empty())
}

impl StateDb {
    /// Every project, most recently updated first; archived ones only with `archived`.
    pub fn projects(&self, archived: bool) -> StateResult<Vec<Project>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {PROJECT_COLS} FROM projects p WHERE (?1 = 1 OR p.archived = 0) \
                 ORDER BY p.archived, p.updated_at DESC, p.rid DESC"
            ))?;
            let rows = stmt
                .query_map([archived as i64], project_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn project(&self, id: &str) -> StateResult<Project> {
        self.read(|c| load_project(c, id))
    }

    /// Stores a project whose folders `crate::projects` already checked.
    pub fn create_project(&self, input: &ProjectInput) -> StateResult<Project> {
        let name = clean_name(&input.name)?;
        let settings = serde_json::to_string(&input.settings).unwrap_or_else(|_| "{}".into());
        self.write(|tx| {
            let id = new_id();
            let now = now_ms();
            tx.execute(
                "INSERT INTO projects(id, name, description, folder, settings_json, created_at, \
                 updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    id,
                    name,
                    input.description.trim(),
                    input.folder,
                    settings,
                    now
                ],
            )?;
            load_project(tx, &id)
        })
    }

    pub fn update_project(&self, id: &str, input: &ProjectInput) -> StateResult<Project> {
        let name = clean_name(&input.name)?;
        let settings = serde_json::to_string(&input.settings).unwrap_or_else(|_| "{}".into());
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE projects SET name = ?2, description = ?3, folder = ?4, settings_json = ?5, \
                 updated_at = ?6 WHERE id = ?1",
                params![
                    id,
                    name,
                    input.description.trim(),
                    input.folder,
                    settings,
                    now_ms()
                ],
            )?;
            if n == 0 {
                return Err(not_found(id));
            }
            load_project(tx, id)
        })
    }

    pub fn set_project_archived(&self, id: &str, archived: bool) -> StateResult<Project> {
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE projects SET archived = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, archived as i64, now_ms()],
            )?;
            if n == 0 {
                return Err(not_found(id));
            }
            load_project(tx, id)
        })
    }

    /// Deletes the project row and its output records; its chats stay, without a project.
    pub fn delete_project(&self, id: &str) -> StateResult<()> {
        self.write(|tx| {
            if tx.execute("DELETE FROM projects WHERE id = ?1", [id])? == 0 {
                return Err(not_found(id));
            }
            Ok(())
        })
    }

    /// Files a conversation under a project (`None` = no project). Archived projects take no
    /// new chats.
    pub fn set_conversation_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
    ) -> StateResult<()> {
        self.write(|tx| {
            if let Some(p) = project_id {
                if load_project(tx, p)?.archived {
                    return Err(StateError::invalid(err("projects.archived").with("id", p)));
                }
            }
            let n = tx.execute(
                "UPDATE conversations SET project_id = ?2 WHERE id = ?1",
                params![conversation_id, project_id],
            )?;
            if n == 0 {
                return Err(StateError::not_found(
                    err("conversations.not_found").with("id", conversation_id),
                ));
            }
            if let Some(p) = project_id {
                tx.execute(
                    "UPDATE projects SET updated_at = ?2 WHERE id = ?1",
                    params![p, now_ms()],
                )?;
            }
            Ok(())
        })
    }

    pub fn conversation_project(&self, conversation_id: &str) -> StateResult<Option<String>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT project_id FROM conversations WHERE id = ?1",
                [conversation_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
        })
    }

    /// Ids of the project's conversations, most recently updated first.
    pub fn project_conversation_ids(&self, project_id: &str) -> StateResult<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id FROM conversations WHERE project_id = ?1 \
                 ORDER BY updated_at DESC, rid DESC",
            )?;
            let ids = stmt
                .query_map([project_id], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(ids)
        })
    }

    /// Records a file made for the project. Recording the same path again refreshes it (newest
    /// source wins) instead of listing it twice.
    pub fn add_project_output(
        &self,
        project_id: &str,
        output: &NewOutput,
    ) -> StateResult<ProjectOutput> {
        self.write(|tx| {
            load_project(tx, project_id)?;
            let id = new_id();
            let now = now_ms();
            tx.execute(
                &format!(
                    "INSERT INTO project_outputs({OUTPUT_COLS}) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
                     ON CONFLICT(project_id, path) DO UPDATE SET kind = excluded.kind, \
                     source = excluded.source, model = excluded.model, \
                     conversation_id = excluded.conversation_id, \
                     agent_task_id = excluded.agent_task_id, tool = excluded.tool, \
                     size = excluded.size, created_at = excluded.created_at"
                ),
                params![
                    id,
                    project_id,
                    output.path,
                    clean_kind(&output.kind),
                    clean_source(&output.source),
                    label(output.model.as_ref()),
                    // A chat that was deleted meanwhile must not fail the record.
                    output.conversation_id.as_ref().filter(|c| {
                        tx.query_row("SELECT 1 FROM conversations WHERE id = ?1", [c], |_| Ok(()))
                            .is_ok()
                    }),
                    label(output.agent_task_id.as_ref()),
                    label(output.tool.as_ref()),
                    output.size.max(0),
                    now
                ],
            )?;
            tx.execute(
                "UPDATE projects SET updated_at = ?2 WHERE id = ?1",
                params![project_id, now],
            )?;
            Ok(tx.query_row(
                &format!(
                    "SELECT {OUTPUT_COLS} FROM project_outputs WHERE project_id = ?1 AND path = ?2"
                ),
                params![project_id, output.path],
                output_from_row,
            )?)
        })
    }

    pub fn project_outputs(&self, project_id: &str) -> StateResult<Vec<ProjectOutput>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {OUTPUT_COLS} FROM project_outputs WHERE project_id = ?1 \
                 ORDER BY created_at DESC, rid DESC"
            ))?;
            let rows = stmt
                .query_map([project_id], output_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn project_output(&self, id: &str) -> StateResult<ProjectOutput> {
        self.read(|c| {
            c.query_row(
                &format!("SELECT {OUTPUT_COLS} FROM project_outputs WHERE id = ?1"),
                [id],
                output_from_row,
            )
            .optional()?
            .ok_or_else(|| StateError::not_found(err("projects.output_not_found").with("id", id)))
        })
    }

    /// Forgets an output record; the file is untouched.
    pub fn remove_project_output(&self, id: &str) -> StateResult<()> {
        self.write(|tx| {
            if tx.execute("DELETE FROM project_outputs WHERE id = ?1", [id])? == 0 {
                return Err(StateError::not_found(
                    err("projects.output_not_found").with("id", id),
                ));
            }
            Ok(())
        })
    }
}
