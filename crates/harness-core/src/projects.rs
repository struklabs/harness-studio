//! Projects: a name, a folder and a description that group chats, attached tools, skills and
//! MCP servers, agent tasks and the files made for them. Rows live in `state.db`
//! ([`crate::state::StateDb::projects`]); this module checks folders, keeps tools and agents
//! inside a project's folders, saves and records outputs, and exports or deletes a project.

use crate::{
    error_key::err,
    paths,
    state::{NewOutput, Project, ProjectInput, ProjectOutput, StateDb},
};
use serde::Serialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// More extra folders than this is a configuration mistake, not a project.
const MAX_ALLOWED_FOLDERS: usize = 16;
/// Agents get the project folder plus extra folders as read grants, at most this many.
const MAX_AGENT_FOLDERS: usize = 8;
/// Outputs written by Harness itself go here, inside the project folder.
pub const OUTPUTS_DIR: &str = "outputs";

/// An absolute, existing folder, canonicalized; anything else is refused.
fn existing_folder(raw: &str) -> Result<String, String> {
    let path = PathBuf::from(raw.trim());
    if !path.is_absolute() || !path.is_dir() || path.parent().is_none() {
        return Err(err("projects.folder_invalid").with("path", raw).into());
    }
    let canonical = std::fs::canonicalize(&path)
        .map_err(|_| String::from(err("projects.folder_invalid").with("path", raw)))?;
    Ok(paths::display_path(&canonical))
}

/// Checks and normalizes the folders of `input` (attachments are kept as names).
pub fn check_input(mut input: ProjectInput) -> Result<ProjectInput, String> {
    input.folder = match input.folder.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(existing_folder(raw)?),
    };
    if input.settings.allowed_folders.len() > MAX_ALLOWED_FOLDERS {
        return Err(err("projects.too_many_folders")
            .num("max", MAX_ALLOWED_FOLDERS as u64)
            .into());
    }
    let mut folders = Vec::new();
    for raw in input.settings.allowed_folders.iter().map(|f| f.trim()) {
        if raw.is_empty() {
            continue;
        }
        let folder = existing_folder(raw)?;
        if Some(&folder) != input.folder.as_ref() && !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    input.settings.allowed_folders = folders;
    for list in [
        &mut input.settings.tools,
        &mut input.settings.skills,
        &mut input.settings.mcp_servers,
    ] {
        list.retain(|n| !n.trim().is_empty() && n.len() <= 256);
        list.sort();
        list.dedup();
    }
    input.description = input.description.chars().take(4000).collect();
    Ok(input)
}

/// The folders tools and agents of the project may use: its own folder first.
pub fn roots(project: &Project) -> Vec<PathBuf> {
    project
        .folder
        .iter()
        .chain(project.settings.allowed_folders.iter())
        .map(PathBuf::from)
        .collect()
}

/// `path` lies inside one of the project's folders (after resolving links).
pub fn contains(project: &Project, path: &Path) -> bool {
    roots(project)
        .iter()
        .any(|root| paths::is_within(path, root))
}

/// The working folder a tool call of the project runs in: the requested one when it lies inside
/// the project, the project folder when none was asked for. Anything else is refused, so a chat
/// filed under a project cannot point its tools somewhere the project does not allow.
pub fn tool_workdir(project: &Project, requested: Option<&Path>) -> Result<PathBuf, String> {
    match requested {
        Some(dir) if contains(project, dir) => Ok(dir.to_path_buf()),
        Some(dir) => Err(err("projects.outside_folders")
            .with("path", dir.display())
            .into()),
        None => project
            .folder
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| err("projects.no_folder").into()),
    }
}

/// Read grants for an agent task of the project.
pub fn agent_grants(project: &Project) -> crate::modules::agents::RunGrants {
    crate::modules::agents::RunGrants {
        read_folders: roots(project).into_iter().take(MAX_AGENT_FOLDERS).collect(),
    }
}

/// Starts an agent task for the project: its folders are the task's only read grants.
pub fn start_agent_task(db: &StateDb, project_id: &str, prompt: String) -> Result<(), String> {
    let project = active(db, project_id)?;
    crate::modules::agents::start_task_for_project(prompt, agent_grants(&project), Some(project.id))
}

/// The agent tasks started for the project, newest first.
pub fn agent_tasks(project_id: &str) -> Result<Vec<crate::modules::agents::TaskStatus>, String> {
    Ok(crate::modules::agents::task_history()?
        .into_iter()
        .filter(|t| t.project_id.as_deref() == Some(project_id))
        .collect())
}

/// A project that can take new chats, outputs and tasks.
fn active(db: &StateDb, project_id: &str) -> Result<Project, String> {
    let project = db.project(project_id).map_err(String::from)?;
    if project.archived {
        return Err(err("projects.archived").with("id", project_id).into());
    }
    Ok(project)
}

fn kind_of(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => "image",
        "wav" | "mp3" | "ogg" | "flac" | "m4a" => "audio",
        "md" | "txt" | "json" | "csv" | "html" | "xml" | "yaml" | "yml" => "text",
        _ => "file",
    }
}

/// A free name in `dir` for `name`: `name`, then `name (2)`, `name (3)`...
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = Path::new(name).extension().and_then(|e| e.to_str());
    (2..10_000)
        .map(|n| match ext {
            Some(ext) => dir.join(format!("{stem} ({n}).{ext}")),
            None => dir.join(format!("{stem} ({n})")),
        })
        .find(|p| !p.exists())
        .unwrap_or(candidate)
}

/// Where a file came from, for [`save_output`].
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct OutputSource {
    pub source: String,
    pub model: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_task_id: Option<String>,
}

/// Writes `bytes` to `<project folder>/outputs/<name>` (never overwriting) and records it.
pub fn save_output(
    db: &StateDb,
    project_id: &str,
    name: &str,
    bytes: &[u8],
    from: &OutputSource,
) -> Result<ProjectOutput, String> {
    let project = active(db, project_id)?;
    let folder = project
        .folder
        .as_ref()
        .ok_or_else(|| String::from(err("projects.no_folder")))?;
    let name = paths::sanitize_file_name(name).unwrap_or_else(|| "output".into());
    let dir = Path::new(folder).join(OUTPUTS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| {
        String::from(
            err("projects.write_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })?;
    let path = free_path(&dir, &name);
    let mut file = std::fs::File::create_new(&path).map_err(|e| {
        String::from(
            err("projects.write_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    file.write_all(bytes).map_err(|e| {
        String::from(
            err("projects.write_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    db.add_project_output(
        project_id,
        &NewOutput {
            path: paths::display_path(&path),
            kind: kind_of(&name).into(),
            source: from.source.clone(),
            model: from.model.clone(),
            conversation_id: from.conversation_id.clone(),
            agent_task_id: from.agent_task_id.clone(),
            tool: None,
            size: bytes.len() as i64,
        },
    )
    .map_err(String::from)
}

/// Records an existing file inside the project's folders as an output.
pub fn add_existing(
    db: &StateDb,
    project_id: &str,
    path: &Path,
    from: &OutputSource,
    tool: Option<&str>,
) -> Result<ProjectOutput, String> {
    let project = active(db, project_id)?;
    let meta = std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .ok_or_else(|| String::from(err("projects.output_missing").with("path", path.display())))?;
    if !contains(&project, path) {
        return Err(err("projects.outside_folders")
            .with("path", path.display())
            .into());
    }
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    db.add_project_output(
        project_id,
        &NewOutput {
            path: paths::display_path(&canonical),
            kind: kind_of(&name).into(),
            source: from.source.clone(),
            model: from.model.clone(),
            conversation_id: from.conversation_id.clone(),
            agent_task_id: from.agent_task_id.clone(),
            tool: tool.map(str::to_string),
            size: meta.len() as i64,
        },
    )
    .map_err(String::from)
}

/// Tools whose successful run leaves a file at `args.path`.
const FILE_TOOLS: &[&str] = &["write_file", "edit_file"];

/// After a successful file tool call of a project chat, records the written file as an output.
/// Anything that does not resolve to a file inside the project is ignored.
pub fn record_tool_output(
    db: &StateDb,
    project_id: &str,
    tool: &str,
    args: &serde_json::Value,
    workdir: &Path,
    from: &OutputSource,
) -> Option<ProjectOutput> {
    if !FILE_TOOLS.contains(&tool) {
        return None;
    }
    let raw = args["path"].as_str()?;
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        workdir.join(raw)
    };
    add_existing(db, project_id, &path, from, Some(tool)).ok()
}

/// An output that still exists and lies inside the project: what open and reveal may act on.
pub fn openable(db: &StateDb, output_id: &str) -> Result<PathBuf, String> {
    let output = db.project_output(output_id).map_err(String::from)?;
    let project = db.project(&output.project_id).map_err(String::from)?;
    let path = PathBuf::from(&output.path);
    if !path.is_file() {
        return Err(err("projects.output_missing")
            .with("path", &output.path)
            .into());
    }
    if !contains(&project, &path) {
        return Err(err("projects.outside_folders")
            .with("path", &output.path)
            .into());
    }
    Ok(path)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeleteReport {
    /// Output files removed from disk.
    pub deleted_files: usize,
    /// Output files that were missing, outside the project's folders or could not be removed.
    pub kept_files: usize,
    /// Chats that stay, now without a project.
    pub unassigned_chats: usize,
}

/// Deletes the project. Its chats stay (without a project). With `delete_files`, the recorded
/// output files are removed too, but only files inside the project's folders; folders themselves
/// are never removed.
pub fn delete(db: &StateDb, project_id: &str, delete_files: bool) -> Result<DeleteReport, String> {
    let project = db.project(project_id).map_err(String::from)?;
    let outputs = db.project_outputs(project_id).map_err(String::from)?;
    let mut report = DeleteReport {
        unassigned_chats: project.chat_count as usize,
        ..Default::default()
    };
    if delete_files {
        for output in &outputs {
            let path = Path::new(&output.path);
            let removable = path.is_file() && contains(&project, path);
            if removable && std::fs::remove_file(path).is_ok() {
                report.deleted_files += 1;
            } else {
                report.kept_files += 1;
            }
        }
    } else {
        report.kept_files = outputs.len();
    }
    db.delete_project(project_id).map_err(String::from)?;
    Ok(report)
}

pub const EXPORT_SCHEMA: &str = "harness.project.v1";

#[derive(Debug, Serialize)]
struct ExportManifest<'a> {
    schema: &'static str,
    exported_at: i64,
    project: &'a Project,
    outputs: Vec<ExportedOutput<'a>>,
    chats: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExportedOutput<'a> {
    #[serde(flatten)]
    output: &'a ProjectOutput,
    /// Path inside the archive; `None` when the file is missing or outside the project.
    archive_path: Option<String>,
}

/// A zip entry time (local, like the zip tools that show it); outside the format's 1980-2107
/// range it falls back to the format's epoch.
fn zip_time(at: chrono::DateTime<chrono::Local>) -> zip::DateTime {
    use chrono::{Datelike, Timelike};
    u16::try_from(at.year())
        .ok()
        .and_then(|year| {
            zip::DateTime::from_date_and_time(
                year,
                at.month() as u8,
                at.day() as u8,
                at.hour() as u8,
                at.minute() as u8,
                at.second() as u8,
            )
            .ok()
        })
        .unwrap_or_default()
}

/// Writes `project.json`, every chat (`chats/<id>.json`, the conversation export format) and
/// the output files that still exist inside the project (`outputs/`) into a zip at `dest`.
pub fn export_zip(db: &StateDb, project_id: &str, dest: &Path) -> Result<usize, String> {
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};
    let project = db.project(project_id).map_err(String::from)?;
    let outputs = db.project_outputs(project_id).map_err(String::from)?;
    let chats = db
        .project_conversation_ids(project_id)
        .map_err(String::from)?;
    let fail = |e: &dyn std::fmt::Display| -> String {
        err("projects.export_failed")
            .with("path", dest.display())
            .with("error", e)
            .into()
    };
    let tmp = dest.with_extension("zip.part");
    let file = std::fs::File::create(&tmp).map_err(|e| fail(&e))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip_time(chrono::Local::now()));
    let result = (|| -> Result<usize, String> {
        let mut names = std::collections::HashSet::new();
        let mut exported = Vec::new();
        for output in &outputs {
            let path = Path::new(&output.path);
            let archive_path = if path.is_file() && contains(&project, path) {
                let base = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(paths::sanitize_file_name)
                    .unwrap_or_else(|| "output".into());
                let mut name = format!("outputs/{base}");
                let mut n = 2;
                while !names.insert(name.clone()) {
                    name = format!("outputs/{n}-{base}");
                    n += 1;
                }
                let modified = std::fs::metadata(path).and_then(|m| m.modified());
                let file_options = match modified {
                    Ok(at) => options.last_modified_time(zip_time(at.into())),
                    Err(_) => options,
                };
                zip.start_file(&name, file_options).map_err(|e| fail(&e))?;
                let mut src = std::fs::File::open(path).map_err(|e| fail(&e))?;
                std::io::copy(&mut src, &mut zip).map_err(|e| fail(&e))?;
                Some(name)
            } else {
                None
            };
            exported.push(ExportedOutput {
                output,
                archive_path,
            });
        }
        for id in &chats {
            let export = db.export_json(id).map_err(String::from)?;
            zip.start_file(format!("chats/{id}.json"), options)
                .map_err(|e| fail(&e))?;
            serde_json::to_writer_pretty(&mut zip, &export).map_err(|e| fail(&e))?;
        }
        let files = exported.iter().filter(|o| o.archive_path.is_some()).count();
        let manifest = ExportManifest {
            schema: EXPORT_SCHEMA,
            exported_at: chrono_now_ms(),
            project: &project,
            outputs: exported,
            chats: chats.clone(),
        };
        zip.start_file("project.json", options)
            .map_err(|e| fail(&e))?;
        serde_json::to_writer_pretty(&mut zip, &manifest).map_err(|e| fail(&e))?;
        Ok(files)
    })();
    match result.and_then(|files| zip.finish().map(|_| files).map_err(|e| fail(&e))) {
        Ok(files) => {
            std::fs::rename(&tmp, dest).map_err(|e| fail(&e))?;
            Ok(files)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
