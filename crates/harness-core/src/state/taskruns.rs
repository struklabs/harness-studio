//! Task runs in `state.db`: the run, its steps, checkpoints and event log. The runner, the
//! policy checks and the bundle format live in `crate::taskflow`.

use super::{new_id, now_ms, StateDb, StateError, StateResult};
use crate::error_key::err;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::{Deserialize, Serialize};

/// Per-million-token prices in USD, used when the model reports none.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Price {
    pub input_per_m: f64,
    pub output_per_m: f64,
}

/// How a run chooses models and copes with failures. Stored as `task_runs.policy_json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RunPolicy {
    /// Gateway model ids in order of preference: local aliases or `cloud:<provider>/<model>`
    /// (BYOK). The first one that meets the requirements runs the step; the others are the
    /// fallback chain.
    pub models: Vec<String>,
    /// A model with a known smaller context is skipped.
    pub min_context: Option<u64>,
    /// The model must support tool calling (declared, or proven by a one-off probe).
    pub require_tools: bool,
    /// Extra attempts per model after a transient failure (timeouts, 429, 5xx).
    pub max_retries: u32,
    /// First backoff between attempts; doubles each time, capped at 30 s.
    pub backoff_ms: u64,
    /// Per request.
    pub timeout_secs: u64,
    /// Output limit per step.
    pub max_tokens: u64,
    pub temperature: f64,
    /// Overrides the provider's own price (local models cost nothing by default).
    pub price: Option<Price>,
}

impl Default for RunPolicy {
    fn default() -> Self {
        RunPolicy {
            models: Vec::new(),
            min_context: None,
            require_tools: false,
            max_retries: 2,
            backoff_ms: 500,
            timeout_secs: 180,
            max_tokens: 768,
            temperature: 0.2,
            price: None,
        }
    }
}

/// A file a run works with, kept as a reference (never copied) relative to the project folder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct FileRef {
    /// Path relative to the project folder, `/` separated.
    pub path: String,
    pub size: u64,
    /// SHA-256 of the content when the reference was added; shows a changed file after resume.
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskRun {
    pub id: String,
    pub project_id: Option<String>,
    /// `agent_task | analysis`.
    pub kind: String,
    pub title: String,
    pub goal: String,
    /// `draft | running | paused | awaiting_approval | completed | failed | interrupted`.
    pub status: String,
    /// `auto` runs every step; `step` pauses after each one.
    pub mode: String,
    pub policy: RunPolicy,
    pub file_refs: Vec<FileRef>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// Micro-USD (1_000_000 = 1 USD) spent so far, including steps that were rolled back.
    pub cost_micro: i64,
    pub last_model: Option<String>,
    pub error: Option<String>,
    pub imported_from: Option<String>,
    pub heartbeat_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskStep {
    pub id: String,
    pub run_id: String,
    pub idx: i64,
    pub title: String,
    pub prompt: String,
    /// The step waits for the user's approval before it runs.
    pub gate: bool,
    /// `none | pending | granted | denied`.
    pub approval: String,
    /// `pending | running | done | skipped | failed`.
    pub status: String,
    pub output: String,
    pub model: Option<String>,
    /// Why another model than the first one ran (keyed error strings, one per line).
    pub fallback_reason: Option<String>,
    pub attempts: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_micro: i64,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskCheckpoint {
    pub id: String,
    pub run_id: String,
    /// Steps before this index were finished when the checkpoint was taken.
    pub step_idx: i64,
    pub label: String,
    pub model: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEvent {
    pub step_idx: Option<i64>,
    pub at: i64,
    pub kind: String,
    /// A keyed error string (translated by the UI) or plain text.
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StepInput {
    pub title: String,
    pub prompt: String,
    pub gate: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NewTaskRun {
    pub project_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub goal: String,
    pub mode: String,
    pub policy: RunPolicy,
    pub file_refs: Vec<FileRef>,
    pub steps: Vec<StepInput>,
}

/// Most steps a run can have; more is a plan that should be split.
pub const MAX_STEPS: usize = 40;
/// Largest prompt of one step (characters).
pub const MAX_PROMPT: usize = 8000;

const RUN_COLS: &str = "id, project_id, kind, title, goal, status, mode, policy_json, \
     file_refs_json, tokens_in, tokens_out, cost_micro, last_model, error, imported_from, \
     heartbeat_at, created_at, updated_at, finished_at";

fn run_from_row(r: &Row<'_>) -> rusqlite::Result<TaskRun> {
    let policy: String = r.get(7)?;
    let refs: String = r.get(8)?;
    Ok(TaskRun {
        id: r.get(0)?,
        project_id: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        goal: r.get(4)?,
        status: r.get(5)?,
        mode: r.get(6)?,
        policy: serde_json::from_str(&policy).unwrap_or_default(),
        file_refs: serde_json::from_str(&refs).unwrap_or_default(),
        tokens_in: r.get(9)?,
        tokens_out: r.get(10)?,
        cost_micro: r.get(11)?,
        last_model: r.get(12)?,
        error: r.get(13)?,
        imported_from: r.get(14)?,
        heartbeat_at: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
        finished_at: r.get(18)?,
    })
}

const STEP_COLS: &str = "id, run_id, idx, title, prompt, gate, approval, status, output, model, \
     fallback_reason, attempts, tokens_in, tokens_out, cost_micro, error, started_at, finished_at";

fn step_from_row(r: &Row<'_>) -> rusqlite::Result<TaskStep> {
    Ok(TaskStep {
        id: r.get(0)?,
        run_id: r.get(1)?,
        idx: r.get(2)?,
        title: r.get(3)?,
        prompt: r.get(4)?,
        gate: r.get::<_, i64>(5)? != 0,
        approval: r.get(6)?,
        status: r.get(7)?,
        output: r.get(8)?,
        model: r.get(9)?,
        fallback_reason: r.get(10)?,
        attempts: r.get(11)?,
        tokens_in: r.get(12)?,
        tokens_out: r.get(13)?,
        cost_micro: r.get(14)?,
        error: r.get(15)?,
        started_at: r.get(16)?,
        finished_at: r.get(17)?,
    })
}

fn run_not_found(id: &str) -> StateError {
    StateError::not_found(err("taskrun.not_found").with("id", id))
}

fn load_run(conn: &Connection, id: &str) -> StateResult<TaskRun> {
    conn.query_row(
        &format!("SELECT {RUN_COLS} FROM task_runs WHERE id = ?1"),
        [id],
        run_from_row,
    )
    .optional()?
    .ok_or_else(|| run_not_found(id))
}

fn load_steps(conn: &Connection, run_id: &str) -> StateResult<Vec<TaskStep>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {STEP_COLS} FROM task_steps WHERE run_id = ?1 ORDER BY idx"
    ))?;
    let rows = stmt
        .query_map([run_id], step_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn log_event(
    tx: &Connection,
    run_id: &str,
    step_idx: Option<i64>,
    kind: &str,
    message: &str,
) -> StateResult<()> {
    tx.execute(
        "INSERT INTO task_events(run_id, step_idx, at, kind, message) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![run_id, step_idx, now_ms(), kind, message],
    )?;
    Ok(())
}

fn clean_kind(kind: &str) -> &'static str {
    match kind {
        "analysis" => "analysis",
        _ => "agent_task",
    }
}

fn clean_mode(mode: &str) -> &'static str {
    match mode {
        "step" => "step",
        _ => "auto",
    }
}

fn check_steps(steps: &[StepInput]) -> StateResult<()> {
    if steps.is_empty() {
        return Err(StateError::invalid(err("taskrun.no_steps")));
    }
    if steps.len() > MAX_STEPS {
        return Err(StateError::invalid(
            err("taskrun.too_many_steps").num("max", MAX_STEPS as u64),
        ));
    }
    for s in steps {
        if s.prompt.trim().is_empty() || s.prompt.chars().count() > MAX_PROMPT {
            return Err(StateError::invalid(
                err("taskrun.invalid_step").num("max", MAX_PROMPT as u64),
            ));
        }
    }
    Ok(())
}

fn clean_title(title: &str) -> StateResult<String> {
    let title = title.trim().chars().take(120).collect::<String>();
    if title.is_empty() || title.chars().any(char::is_control) {
        return Err(StateError::invalid(err("taskrun.invalid_title")));
    }
    Ok(title)
}

fn insert_steps(tx: &Transaction<'_>, run_id: &str, steps: &[StepInput]) -> StateResult<()> {
    for (i, s) in steps.iter().enumerate() {
        let title: String = if s.title.trim().is_empty() {
            format!("Step {}", i + 1)
        } else {
            s.title.trim().chars().take(120).collect()
        };
        tx.execute(
            "INSERT INTO task_steps(id, run_id, idx, title, prompt, gate) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                new_id(),
                run_id,
                i as i64,
                title,
                s.prompt.trim(),
                s.gate as i64
            ],
        )?;
    }
    Ok(())
}

/// What the step needs to know to be picked up again.
#[derive(Debug, Clone, Default)]
pub struct StepResult {
    pub output: String,
    pub model: String,
    pub fallback_reason: Option<String>,
    pub attempts: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_micro: i64,
}

impl StateDb {
    // Publish only after commit: subscribers can immediately read the new snapshot.
    fn write_task_activity<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> StateResult<T>,
    ) -> StateResult<T> {
        let result = self.write(f);
        if result.is_ok() {
            crate::events::publish(crate::events::CoreEvent::OfficeActivityChanged);
        }
        result
    }

    /// All active office runs, across projects, including approval gates and paused runs.
    pub fn office_task_runs(&self) -> StateResult<Vec<TaskRun>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {RUN_COLS} FROM task_runs WHERE status IN ('running', 'awaiting_approval', 'paused') ORDER BY created_at, rid"
            ))?;
            let runs = stmt.query_map([], run_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(runs)
        })
    }

    /// Runs of the project (or the ones without a project), newest first.
    pub fn task_runs(&self, project_id: Option<&str>) -> StateResult<Vec<TaskRun>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {RUN_COLS} FROM task_runs WHERE (?1 IS NULL AND project_id IS NULL) \
                 OR project_id = ?1 ORDER BY updated_at DESC, rid DESC"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map([project_id], run_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn task_run(&self, id: &str) -> StateResult<TaskRun> {
        self.read(|c| load_run(c, id))
    }

    pub fn task_steps(&self, run_id: &str) -> StateResult<Vec<TaskStep>> {
        self.read(|c| load_steps(c, run_id))
    }

    pub fn task_checkpoints(&self, run_id: &str) -> StateResult<Vec<TaskCheckpoint>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, run_id, step_idx, label, model, created_at FROM task_checkpoints \
                 WHERE run_id = ?1 ORDER BY step_idx, rid",
            )?;
            let rows = stmt
                .query_map([run_id], |r| {
                    Ok(TaskCheckpoint {
                        id: r.get(0)?,
                        run_id: r.get(1)?,
                        step_idx: r.get(2)?,
                        label: r.get(3)?,
                        model: r.get(4)?,
                        created_at: r.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn task_events(&self, run_id: &str) -> StateResult<Vec<TaskEvent>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT step_idx, at, kind, message FROM task_events WHERE run_id = ?1 \
                 ORDER BY rid",
            )?;
            let rows = stmt
                .query_map([run_id], |r| {
                    Ok(TaskEvent {
                        step_idx: r.get(0)?,
                        at: r.get(1)?,
                        kind: r.get(2)?,
                        message: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn add_task_event(
        &self,
        run_id: &str,
        step_idx: Option<i64>,
        kind: &str,
        message: &str,
    ) -> StateResult<()> {
        self.write_task_activity(|tx| log_event(tx, run_id, step_idx, kind, message))
    }

    /// Creates a draft (nothing runs until it is started).
    pub fn create_task_run(&self, input: &NewTaskRun) -> StateResult<TaskRun> {
        let title = clean_title(&input.title)?;
        check_steps(&input.steps)?;
        let policy = serde_json::to_string(&input.policy).unwrap_or_else(|_| "{}".into());
        let refs = serde_json::to_string(&input.file_refs).unwrap_or_else(|_| "[]".into());
        self.write_task_activity(|tx| {
            if let Some(p) = &input.project_id {
                super::projects::load_project(tx, p)?;
            }
            let id = new_id();
            let now = now_ms();
            tx.execute(
                "INSERT INTO task_runs(id, project_id, kind, title, goal, mode, policy_json, \
                 file_refs_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, \
                 ?9, ?9)",
                params![
                    id,
                    input.project_id,
                    clean_kind(&input.kind),
                    title,
                    input.goal.trim().chars().take(4000).collect::<String>(),
                    clean_mode(&input.mode),
                    policy,
                    refs,
                    now
                ],
            )?;
            insert_steps(tx, &id, &input.steps)?;
            log_event(tx, &id, None, "created", "")?;
            load_run(tx, &id)
        })
    }

    /// Edits a draft or a paused run before its next step: title, goal, mode, policy, files
    /// and the steps that have not run yet (finished steps stay as they are).
    pub fn update_task_run(&self, id: &str, input: &NewTaskRun) -> StateResult<TaskRun> {
        let title = clean_title(&input.title)?;
        check_steps(&input.steps)?;
        let policy = serde_json::to_string(&input.policy).unwrap_or_else(|_| "{}".into());
        let refs = serde_json::to_string(&input.file_refs).unwrap_or_else(|_| "[]".into());
        self.write_task_activity(|tx| {
            let run = load_run(tx, id)?;
            if !matches!(
                run.status.as_str(),
                "draft" | "paused" | "interrupted" | "failed"
            ) {
                return Err(StateError::invalid(
                    err("taskrun.not_editable").with("status", &run.status),
                ));
            }
            let done = load_steps(tx, id)?
                .iter()
                .take_while(|s| matches!(s.status.as_str(), "done" | "skipped"))
                .count();
            tx.execute(
                "UPDATE task_runs SET title = ?2, goal = ?3, mode = ?4, policy_json = ?5, \
                 file_refs_json = ?6, updated_at = ?7 WHERE id = ?1",
                params![
                    id,
                    title,
                    input.goal.trim().chars().take(4000).collect::<String>(),
                    clean_mode(&input.mode),
                    policy,
                    refs,
                    now_ms()
                ],
            )?;
            tx.execute(
                "DELETE FROM task_steps WHERE run_id = ?1 AND idx >= ?2",
                params![id, done as i64],
            )?;
            let rest = input.steps.get(done..).unwrap_or_default();
            // New steps continue the numbering after the finished ones.
            for (i, s) in rest.iter().enumerate() {
                let title: String = if s.title.trim().is_empty() {
                    format!("Step {}", done + i + 1)
                } else {
                    s.title.trim().chars().take(120).collect()
                };
                tx.execute(
                    "INSERT INTO task_steps(id, run_id, idx, title, prompt, gate) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        new_id(),
                        id,
                        (done + i) as i64,
                        title,
                        s.prompt.trim(),
                        s.gate as i64
                    ],
                )?;
            }
            log_event(tx, id, None, "edited", "")?;
            load_run(tx, id)
        })
    }

    pub fn delete_task_run(&self, id: &str) -> StateResult<()> {
        self.write_task_activity(|tx| {
            let run = load_run(tx, id)?;
            if run.status == "running" && now_ms() - run.heartbeat_at < super::HEARTBEAT_STALE_MS {
                return Err(StateError::invalid(err("taskrun.busy")));
            }
            tx.execute("DELETE FROM task_runs WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Takes the run for one process: fails when another live process runs it or when too many
    /// runs are active. The run becomes `running` with a fresh heartbeat.
    pub fn claim_task_run(&self, id: &str, max_concurrent: usize) -> StateResult<TaskRun> {
        self.write_task_activity(|tx| {
            let run = load_run(tx, id)?;
            let now = now_ms();
            let live =
                |r: &TaskRun| r.status == "running" && now - r.heartbeat_at < HEARTBEAT_STALE;
            if live(&run) {
                return Err(StateError::invalid(err("taskrun.busy")));
            }
            if run.status == "completed" {
                return Err(StateError::invalid(err("taskrun.already_completed")));
            }
            let active: i64 = tx.query_row(
                "SELECT COUNT(*) FROM task_runs WHERE status = 'running' AND heartbeat_at > ?1 \
                 AND id <> ?2",
                params![now - HEARTBEAT_STALE, id],
                |r| r.get(0),
            )?;
            if active as usize >= max_concurrent.max(1) {
                return Err(StateError::invalid(
                    err("taskrun.too_many_running").num("max", max_concurrent as u64),
                ));
            }
            tx.execute(
                "UPDATE task_runs SET status = 'running', error = NULL, heartbeat_at = ?2, \
                 updated_at = ?2, finished_at = NULL WHERE id = ?1",
                params![id, now],
            )?;
            log_event(tx, id, None, "started", "")?;
            load_run(tx, id)
        })
    }

    pub fn heartbeat_task_run(&self, id: &str) -> StateResult<()> {
        self.write(|tx| {
            tx.execute(
                "UPDATE task_runs SET heartbeat_at = ?2 WHERE id = ?1 AND status = 'running'",
                params![id, now_ms()],
            )?;
            Ok(())
        })
    }

    /// Sets the run's state (and error) and logs it.
    pub fn set_task_run_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> StateResult<TaskRun> {
        self.write_task_activity(|tx| {
            let now = now_ms();
            let finished = matches!(status, "completed" | "failed").then_some(now);
            let n = tx.execute(
                "UPDATE task_runs SET status = ?2, error = ?3, updated_at = ?4, \
                 finished_at = ?5 WHERE id = ?1",
                params![id, status, error, now, finished],
            )?;
            if n == 0 {
                return Err(run_not_found(id));
            }
            log_event(tx, id, None, status, error.unwrap_or(""))?;
            load_run(tx, id)
        })
    }

    pub fn begin_task_step(&self, run_id: &str, idx: i64) -> StateResult<()> {
        self.write_task_activity(|tx| {
            tx.execute(
                "UPDATE task_steps SET status = 'running', error = NULL, started_at = ?3 \
                 WHERE run_id = ?1 AND idx = ?2",
                params![run_id, idx, now_ms()],
            )?;
            Ok(())
        })
    }

    /// Records the model choice and attempts of a step that is still running (so a crash
    /// leaves the reason visible).
    pub fn note_task_step(
        &self,
        run_id: &str,
        idx: i64,
        model: Option<&str>,
        fallback_reason: Option<&str>,
        attempts: i64,
    ) -> StateResult<()> {
        self.write_task_activity(|tx| {
            tx.execute(
                "UPDATE task_steps SET model = ?3, fallback_reason = ?4, attempts = ?5 \
                 WHERE run_id = ?1 AND idx = ?2",
                params![run_id, idx, model, fallback_reason, attempts],
            )?;
            Ok(())
        })
    }

    /// Puts a step that was in flight back to pending (a stop request, not a rewind).
    pub fn reset_task_step(&self, run_id: &str, idx: i64) -> StateResult<()> {
        self.write_task_activity(|tx| {
            tx.execute(
                "UPDATE task_steps SET status = 'pending', model = NULL, fallback_reason = NULL,                  attempts = 0, started_at = NULL WHERE run_id = ?1 AND idx = ?2                  AND status = 'running'",
                params![run_id, idx],
            )?;
            Ok(())
        })
    }

    /// Stores a finished step, adds its tokens and cost to the run and takes a checkpoint.
    pub fn finish_task_step(&self, run_id: &str, idx: i64, r: &StepResult) -> StateResult<()> {
        self.write_task_activity(|tx| {
            let now = now_ms();
            tx.execute(
                "UPDATE task_steps SET status = 'done', output = ?3, model = ?4, \
                 fallback_reason = ?5, attempts = ?6, tokens_in = ?7, tokens_out = ?8, \
                 cost_micro = ?9, error = NULL, finished_at = ?10 WHERE run_id = ?1 AND idx = ?2",
                params![
                    run_id,
                    idx,
                    r.output,
                    r.model,
                    r.fallback_reason,
                    r.attempts,
                    r.tokens_in,
                    r.tokens_out,
                    r.cost_micro,
                    now
                ],
            )?;
            tx.execute(
                "UPDATE task_runs SET tokens_in = tokens_in + ?2, tokens_out = tokens_out + ?3, \
                 cost_micro = cost_micro + ?4, last_model = ?5, updated_at = ?6 WHERE id = ?1",
                params![
                    run_id,
                    r.tokens_in,
                    r.tokens_out,
                    r.cost_micro,
                    r.model,
                    now
                ],
            )?;
            let title: String = tx.query_row(
                "SELECT title FROM task_steps WHERE run_id = ?1 AND idx = ?2",
                params![run_id, idx],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO task_checkpoints(id, run_id, step_idx, label, model, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![new_id(), run_id, idx + 1, title, r.model, now],
            )?;
            log_event(tx, run_id, Some(idx), "step_done", &r.model)?;
            Ok(())
        })
    }

    /// A step that failed for good (every model and retry used up).
    pub fn fail_task_step(
        &self,
        run_id: &str,
        idx: i64,
        error: &str,
        fallback_reason: Option<&str>,
        attempts: i64,
    ) -> StateResult<()> {
        self.write_task_activity(|tx| {
            tx.execute(
                "UPDATE task_steps SET status = 'failed', error = ?3, fallback_reason = ?4, \
                 attempts = ?5, finished_at = ?6 WHERE run_id = ?1 AND idx = ?2",
                params![run_id, idx, error, fallback_reason, attempts, now_ms()],
            )?;
            log_event(tx, run_id, Some(idx), "step_failed", error)?;
            Ok(())
        })
    }

    pub fn set_task_step_approval(
        &self,
        run_id: &str,
        idx: i64,
        approval: &str,
    ) -> StateResult<()> {
        self.write_task_activity(|tx| {
            tx.execute(
                "UPDATE task_steps SET approval = ?3 WHERE run_id = ?1 AND idx = ?2",
                params![run_id, idx, approval],
            )?;
            log_event(tx, run_id, Some(idx), &format!("approval_{approval}"), "")?;
            Ok(())
        })
    }

    pub fn skip_task_step(&self, run_id: &str, idx: i64) -> StateResult<()> {
        self.write_task_activity(|tx| {
            tx.execute(
                "UPDATE task_steps SET status = 'skipped', finished_at = ?3 \
                 WHERE run_id = ?1 AND idx = ?2 AND status <> 'done'",
                params![run_id, idx, now_ms()],
            )?;
            log_event(tx, run_id, Some(idx), "step_skipped", "")?;
            Ok(())
        })
    }

    /// Puts the run back before step `idx`: that step and every later one run again; earlier
    /// steps and their outputs stay. Tokens and cost already spent stay in the tally.
    pub fn rewind_task_run(&self, run_id: &str, idx: i64) -> StateResult<TaskRun> {
        self.write_task_activity(|tx| {
            let run = load_run(tx, run_id)?;
            if run.status == "running" && now_ms() - run.heartbeat_at < super::HEARTBEAT_STALE_MS {
                return Err(StateError::invalid(err("taskrun.busy")));
            }
            let steps = load_steps(tx, run_id)?;
            if idx < 0 || idx as usize > steps.len() {
                return Err(StateError::invalid(err("taskrun.invalid_step_index")));
            }
            let dropped = steps
                .iter()
                .filter(|s| s.idx >= idx && s.status == "done")
                .count();
            tx.execute(
                "UPDATE task_steps SET status = 'pending', output = '', model = NULL, \
                 fallback_reason = NULL, attempts = 0, tokens_in = 0, tokens_out = 0, \
                 cost_micro = 0, error = NULL, approval = CASE WHEN gate = 1 THEN 'none' \
                 ELSE approval END, started_at = NULL, finished_at = NULL \
                 WHERE run_id = ?1 AND idx >= ?2",
                params![run_id, idx],
            )?;
            tx.execute(
                "DELETE FROM task_checkpoints WHERE run_id = ?1 AND step_idx > ?2",
                params![run_id, idx],
            )?;
            tx.execute(
                "UPDATE task_runs SET status = 'paused', error = NULL, finished_at = NULL, \
                 updated_at = ?2 WHERE id = ?1",
                params![run_id, now_ms()],
            )?;
            log_event(tx, run_id, Some(idx), "rewound", &dropped.to_string())?;
            load_run(tx, run_id)
        })
    }

    /// Runs whose process vanished (no heartbeat for `stale_ms`) become `interrupted`; the step
    /// that was in flight is pending again. Returns the recovered run ids.
    pub fn recover_task_runs(&self, stale_ms: i64) -> StateResult<Vec<String>> {
        self.write_task_activity(|tx| {
            let now = now_ms();
            let mut stmt = tx.prepare(
                "SELECT id FROM task_runs WHERE status = 'running' AND heartbeat_at <= ?1",
            )?;
            let ids = stmt
                .query_map([now - stale_ms], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(stmt);
            for id in &ids {
                tx.execute(
                    "UPDATE task_steps SET status = 'pending', started_at = NULL \
                     WHERE run_id = ?1 AND status = 'running'",
                    [id],
                )?;
                tx.execute(
                    "UPDATE task_runs SET status = 'interrupted', updated_at = ?2 WHERE id = ?1",
                    params![id, now],
                )?;
                log_event(tx, id, None, "recovered", "")?;
            }
            Ok(ids)
        })
    }

    /// Imports a finished bundle as a new paused run (see `crate::taskflow::bundle`).
    pub(crate) fn import_task_run(
        &self,
        run: &TaskRun,
        steps: &[TaskStep],
        checkpoints: &[TaskCheckpoint],
        events: &[TaskEvent],
        project_id: Option<&str>,
        imported_from: &str,
    ) -> StateResult<TaskRun> {
        let policy = serde_json::to_string(&run.policy).unwrap_or_else(|_| "{}".into());
        let refs = serde_json::to_string(&run.file_refs).unwrap_or_else(|_| "[]".into());
        self.write_task_activity(|tx| {
            if let Some(p) = project_id {
                super::projects::load_project(tx, p)?;
            }
            let id = new_id();
            let now = now_ms();
            let all_done = steps.iter().all(|s| matches!(s.status.as_str(), "done" | "skipped"));
            let status = if all_done { "completed" } else { "paused" };
            tx.execute(
                "INSERT INTO task_runs(id, project_id, kind, title, goal, status, mode, \
                 policy_json, file_refs_json, tokens_in, tokens_out, cost_micro, last_model, \
                 imported_from, created_at, updated_at, finished_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15, ?16)",
                params![
                    id,
                    project_id,
                    clean_kind(&run.kind),
                    run.title,
                    run.goal,
                    status,
                    clean_mode(&run.mode),
                    policy,
                    refs,
                    run.tokens_in,
                    run.tokens_out,
                    run.cost_micro,
                    run.last_model,
                    imported_from,
                    now,
                    all_done.then_some(now)
                ],
            )?;
            for s in steps {
                // Whatever was in flight on the other device starts over here.
                let (state, out) = match s.status.as_str() {
                    "done" | "skipped" => (s.status.as_str(), s.output.as_str()),
                    _ => ("pending", ""),
                };
                tx.execute(
                    "INSERT INTO task_steps(id, run_id, idx, title, prompt, gate, approval, \
                     status, output, model, fallback_reason, attempts, tokens_in, tokens_out, \
                     cost_micro) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                     ?14, ?15)",
                    params![
                        new_id(),
                        id,
                        s.idx,
                        s.title,
                        s.prompt,
                        s.gate as i64,
                        if s.gate && state == "pending" { "none" } else { s.approval.as_str() },
                        state,
                        out,
                        if state == "pending" { None } else { s.model.as_deref() },
                        if state == "pending" { None } else { s.fallback_reason.as_deref() },
                        if state == "pending" { 0 } else { s.attempts },
                        s.tokens_in,
                        s.tokens_out,
                        s.cost_micro
                    ],
                )?;
            }
            for c in checkpoints {
                tx.execute(
                    "INSERT INTO task_checkpoints(id, run_id, step_idx, label, model, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![new_id(), id, c.step_idx, c.label, c.model, c.created_at],
                )?;
            }
            for e in events {
                tx.execute(
                    "INSERT INTO task_events(run_id, step_idx, at, kind, message) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, e.step_idx, e.at, e.kind, e.message],
                )?;
            }
            log_event(tx, &id, None, "imported", imported_from)?;
            load_run(tx, &id)
        })
    }
}

const HEARTBEAT_STALE: i64 = super::HEARTBEAT_STALE_MS;
