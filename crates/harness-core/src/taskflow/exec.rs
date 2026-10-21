//! The runner: executes the pending steps of a task run through the gateway.

use super::{check_model, fetch_models, http, ModelCheck, MAX_FILE_EXCERPT};
use crate::{
    compaction::Endpoint,
    error_key::err,
    state::{FileRef, StateDb, StepResult, TaskRun, TaskStep},
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// How far one `run_task` call goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunAction {
    /// Every remaining step (stops at gates and, in step mode, after one step).
    #[default]
    Continue,
    /// Exactly the next step.
    Step,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// The gateway's `.../v1` and its client token header; `model` is ignored.
    pub endpoint: Endpoint,
    /// Runs that may execute at the same time (across processes); more are refused.
    pub max_concurrent_runs: usize,
    pub action: RunAction,
}

impl RunOptions {
    pub fn new(endpoint: Endpoint) -> Self {
        RunOptions {
            endpoint,
            max_concurrent_runs: 2,
            action: RunAction::Continue,
        }
    }
}

/// Keeps the run's heartbeat alive while the runner future lives; dropping the future (abort,
/// crash of the task) stops it, and `recover` then sees a stale run.
struct Heartbeat(tokio::task::JoinHandle<()>);

impl Heartbeat {
    fn start(db: Arc<StateDb>, id: String) -> Self {
        Heartbeat(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let (db, id) = (db.clone(), id.clone());
                if tokio::task::spawn_blocking(move || db.heartbeat_task_run(&id))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }))
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn db_call<T: Send + 'static>(
    db: &Arc<StateDb>,
    f: impl FnOnce(&StateDb) -> crate::state::StateResult<T> + Send + 'static,
) -> Result<T, String> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || f(&db).map_err(String::from))
        .await
        .map_err(|e| e.to_string())?
}

async fn wait_cancel(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Backoff before retry number `attempt` (0-based): `base`, `2*base`, ... at most 30 s.
pub(crate) fn backoff(base_ms: u64, attempt: u32) -> Duration {
    let ms = base_ms.saturating_mul(1u64 << attempt.min(16)).min(30_000);
    Duration::from_millis(ms)
}

/// Micro-USD of a request at per-million-token prices.
pub(crate) fn cost_micro(tokens_in: u64, tokens_out: u64, input: f64, output: f64) -> i64 {
    (tokens_in as f64 * input + tokens_out as f64 * output).round() as i64
}

struct Failure {
    retryable: bool,
    error: String,
}

struct Reply {
    content: String,
    tokens_in: u64,
    tokens_out: u64,
}

fn strip_think(text: &str) -> String {
    match (text.find("<think>"), text.find("</think>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &text[..a], &text[b + "</think>".len()..])
            .trim()
            .to_string(),
        _ => text.trim().to_string(),
    }
}

async fn request(
    ep: &Endpoint,
    model: &str,
    messages: &[Value],
    run: &TaskRun,
) -> Result<Reply, Failure> {
    let body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": run.policy.max_tokens.max(16),
        "temperature": run.policy.temperature,
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
    });
    let url = format!("{}/chat/completions", ep.base_url.trim_end_matches('/'));
    let mut req = http()
        .post(&url)
        .timeout(Duration::from_secs(run.policy.timeout_secs.clamp(5, 3600)))
        .json(&body);
    for (k, v) in &ep.headers {
        req = req.header(k, v);
    }
    let res = req.send().await.map_err(|e| Failure {
        retryable: true,
        error: if e.is_timeout() {
            err("taskrun.timeout")
                .num("secs", run.policy.timeout_secs)
                .into()
        } else {
            crate::http_error::transport(&e, &url)
        },
    })?;
    let status = res.status();
    let text = res.text().await.map_err(|e| Failure {
        retryable: true,
        error: crate::http_error::transport(&e, &url),
    })?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v["error"]["message"]
                    .as_str()
                    .or(v["error"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(Failure {
            retryable: matches!(status.as_u16(), 408 | 425 | 429 | 500..=599),
            error: format!("HTTP {}: {}", status.as_u16(), detail),
        });
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| Failure {
        retryable: false,
        error: e.to_string(),
    })?;
    let content = strip_think(v["choices"][0]["message"]["content"].as_str().unwrap_or(""));
    if content.is_empty() {
        return Err(Failure {
            retryable: true,
            error: err("taskrun.empty_answer").into(),
        });
    }
    let chars_in: usize = messages
        .iter()
        .map(|m| m["content"].as_str().map_or(0, str::len))
        .sum();
    Ok(Reply {
        tokens_in: v["usage"]["prompt_tokens"]
            .as_u64()
            .unwrap_or((chars_in / 4) as u64),
        tokens_out: v["usage"]["completion_tokens"]
            .as_u64()
            .unwrap_or((content.len() / 4) as u64),
        content,
    })
}

/// Text of the referenced files for the prompt: bounded excerpts, missing files named.
fn file_context(db: &StateDb, run: &TaskRun, refs: &[FileRef]) -> (String, Vec<String>) {
    let root = run
        .project_id
        .as_deref()
        .and_then(|p| db.project(p).ok())
        .and_then(|p| p.folder)
        .map(PathBuf::from);
    let mut text = String::new();
    let mut events = Vec::new();
    let mut total = 0usize;
    for r in refs {
        let path = root
            .as_ref()
            .zip(super::clean_relative(&r.path))
            .map(|(root, rel)| root.join(rel))
            .filter(|p| {
                p.is_file()
                    && root
                        .as_ref()
                        .is_some_and(|root| crate::paths::is_within(p, root))
            });
        let Some(path) = path else {
            events.push(err("taskrun.file_missing").with("path", &r.path).into());
            text.push_str(&format!("\n[file {}: missing on this device]\n", r.path));
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if super::sha256_file(&path).is_ok_and(|(_, h)| h != r.sha256) {
            events.push(err("taskrun.file_changed").with("path", &r.path).into());
        }
        let room = MAX_FILE_EXCERPT.min(3 * MAX_FILE_EXCERPT - total.min(3 * MAX_FILE_EXCERPT));
        if room == 0 {
            break;
        }
        let cut = bytes.len().min(room);
        let mut excerpt = String::from_utf8_lossy(&bytes[..cut]).into_owned();
        if bytes.len() > cut {
            excerpt.push_str("\n[...cut]");
        }
        total += cut;
        text.push_str(&format!("\n--- file: {} ---\n{}\n", r.path, excerpt));
    }
    (text, events)
}

fn messages_for(run: &TaskRun, steps: &[TaskStep], idx: usize, files: &str) -> Vec<Value> {
    let step = &steps[idx];
    let mut system = format!(
        "You are working through a multi-step task. Goal: {}\nYou are on step {} of {}: {}. \
         Answer this step only, concisely, and build on the earlier results.",
        if run.goal.is_empty() {
            &run.title
        } else {
            &run.goal
        },
        idx + 1,
        steps.len(),
        step.title
    );
    if run.kind == "analysis" {
        system.push_str(" This is an analysis session: base statements on the given files.");
    }
    if !files.is_empty() {
        system.push_str("\n\nReferenced files:");
        system.push_str(files);
    }
    let mut messages = vec![json!({"role": "system", "content": system})];
    let mut earlier = String::new();
    for s in steps.iter().take(idx).filter(|s| s.status == "done") {
        let out: String = s.output.chars().take(1500).collect();
        earlier.push_str(&format!(
            "Result of step {} ({}):\n{}\n\n",
            s.idx + 1,
            s.title,
            out
        ));
    }
    if !earlier.is_empty() {
        let earlier: String = earlier.chars().take(6000).collect();
        messages.push(json!({"role": "user", "content": format!("Earlier results:\n{earlier}")}));
        messages.push(json!({"role": "assistant", "content": "Understood."}));
    }
    messages.push(json!({"role": "user", "content": step.prompt}));
    messages
}

/// Outcome of the model chain for one step.
enum StepOutcome {
    Done(StepResult),
    Failed {
        error: String,
        reasons: Vec<String>,
        attempts: i64,
    },
    Cancelled,
}

async fn run_step(
    db: &Arc<StateDb>,
    run: &TaskRun,
    steps: &[TaskStep],
    idx: usize,
    ep: &Endpoint,
    listing: Option<&[Value]>,
    cancel: &AtomicBool,
) -> StepOutcome {
    let run_id = run.id.clone();
    let mut reasons: Vec<String> = Vec::new();
    let mut attempts = 0i64;
    let (files, file_events) = {
        let (db2, run2, refs) = (db.clone(), run.clone(), run.file_refs.clone());
        tokio::task::spawn_blocking(move || file_context(&db2, &run2, &refs))
            .await
            .unwrap_or_default()
    };
    for e in file_events {
        let (rid, i) = (run_id.clone(), idx as i64);
        let _ = db_call(db, move |d| d.add_task_event(&rid, Some(i), "file", &e)).await;
    }
    let messages = messages_for(run, steps, idx, &files);
    let candidates = run.policy.models.clone();
    for model in candidates.iter() {
        let check: ModelCheck = check_model(
            db,
            ep,
            listing,
            model,
            run.policy.min_context,
            run.policy.require_tools,
        )
        .await;
        if !check.ok {
            let reason = check.reason.clone().unwrap_or_default();
            let (rid, i, r) = (run_id.clone(), idx as i64, reason.clone());
            let _ = db_call(db, move |d| {
                d.add_task_event(&rid, Some(i), "skip_model", &r)
            })
            .await;
            reasons.push(reason);
            continue;
        }
        let info = check.info.unwrap_or_default();
        for attempt in 0..=run.policy.max_retries {
            if cancel.load(Ordering::SeqCst) {
                return StepOutcome::Cancelled;
            }
            attempts += 1;
            let reply = tokio::select! {
                r = request(ep, model, &messages, run) => r,
                _ = wait_cancel(cancel) => return StepOutcome::Cancelled,
            };
            match reply {
                Ok(reply) => {
                    let price = run.policy.price.map_or_else(
                        || {
                            (
                                info.input_price.unwrap_or(0.0),
                                info.output_price.unwrap_or(0.0),
                            )
                        },
                        |p| (p.input_per_m, p.output_per_m),
                    );
                    return StepOutcome::Done(StepResult {
                        cost_micro: cost_micro(reply.tokens_in, reply.tokens_out, price.0, price.1),
                        tokens_in: reply.tokens_in as i64,
                        tokens_out: reply.tokens_out as i64,
                        output: reply.content,
                        model: model.clone(),
                        fallback_reason: (!reasons.is_empty()).then(|| reasons.join("\n")),
                        attempts,
                    });
                }
                Err(f) => {
                    let again = f.retryable && attempt < run.policy.max_retries;
                    let msg = if again {
                        format!(
                            "{} (retry {} of {})",
                            f.error,
                            attempt + 1,
                            run.policy.max_retries
                        )
                    } else {
                        f.error.clone()
                    };
                    let (rid, i, kind) = (
                        run_id.clone(),
                        idx as i64,
                        if again { "retry" } else { "model_failed" },
                    );
                    let _ = db_call(db, move |d| d.add_task_event(&rid, Some(i), kind, &msg)).await;
                    if !again {
                        reasons.push(
                            err("taskrun.reason.request_failed")
                                .with("model", model)
                                .with("error", &f.error)
                                .into(),
                        );
                        break;
                    }
                    let wait = backoff(run.policy.backoff_ms, attempt);
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = wait_cancel(cancel) => return StepOutcome::Cancelled,
                    }
                }
            }
        }
    }
    StepOutcome::Failed {
        error: err("taskrun.no_model_qualifies").into(),
        reasons,
        attempts,
    }
}

/// Runs the pending steps of `run_id`. Returns the run as it stands: `completed`, `paused`
/// (step mode, a stop request or a denied gate), `awaiting_approval` or `failed`.
///
/// The run is claimed first (one process per run, at most `max_concurrent_runs` live runs).
/// If this future is dropped mid-step, the run stays `running` until its heartbeat is stale
/// and [`super::recover`] marks it `interrupted`; the unfinished step then runs again.
pub async fn run_task(
    db: Arc<StateDb>,
    run_id: &str,
    opts: RunOptions,
    cancel: Arc<AtomicBool>,
) -> Result<TaskRun, String> {
    let max = opts.max_concurrent_runs;
    let rid = run_id.to_string();
    let run = db_call(&db, move |d| d.claim_task_run(&rid, max)).await?;
    let _beat = Heartbeat::start(db.clone(), run.id.clone());
    if run.policy.models.is_empty() {
        let (rid, e) = (run.id.clone(), String::from(err("taskrun.no_models")));
        return db_call(&db, move |d| {
            d.set_task_run_status(&rid, "failed", Some(&e))
        })
        .await;
    }
    let listing = fetch_models(&opts.endpoint).await;
    let mut ran_one = false;
    loop {
        if cancel.load(Ordering::SeqCst) {
            let rid = run.id.clone();
            return db_call(&db, move |d| d.set_task_run_status(&rid, "paused", None)).await;
        }
        let rid = run.id.clone();
        let (cur, steps) = {
            let rid2 = rid.clone();
            db_call(&db, move |d| Ok((d.task_run(&rid2)?, d.task_steps(&rid2)?))).await?
        };
        let Some(idx) = steps
            .iter()
            .position(|s| !matches!(s.status.as_str(), "done" | "skipped"))
        else {
            return db_call(&db, move |d| d.set_task_run_status(&rid, "completed", None)).await;
        };
        if ran_one && (opts.action == RunAction::Step || cur.mode == "step") {
            return db_call(&db, move |d| d.set_task_run_status(&rid, "paused", None)).await;
        }
        let step = &steps[idx];
        if step.gate && step.approval != "granted" {
            if step.approval == "denied" {
                let i = idx as i64;
                db_call(&db, move |d| d.skip_task_step(&rid, i)).await?;
                continue;
            }
            let (rid2, i) = (rid.clone(), idx as i64);
            db_call(&db, move |d| d.set_task_step_approval(&rid2, i, "pending")).await?;
            return db_call(&db, move |d| {
                d.set_task_run_status(&rid, "awaiting_approval", None)
            })
            .await;
        }
        let (rid2, i) = (rid.clone(), idx as i64);
        db_call(&db, move |d| d.begin_task_step(&rid2, i)).await?;
        match run_step(
            &db,
            &cur,
            &steps,
            idx,
            &opts.endpoint,
            listing.as_deref(),
            &cancel,
        )
        .await
        {
            StepOutcome::Done(result) => {
                let (rid2, i) = (rid.clone(), idx as i64);
                db_call(&db, move |d| d.finish_task_step(&rid2, i, &result)).await?;
                ran_one = true;
            }
            StepOutcome::Cancelled => {
                let (rid2, i) = (rid.clone(), idx as i64);
                // The step starts over on resume.
                db_call(&db, move |d| d.reset_task_step(&rid2, i)).await?;
                return db_call(&db, move |d| d.set_task_run_status(&rid, "paused", None)).await;
            }
            StepOutcome::Failed {
                error,
                reasons,
                attempts,
            } => {
                let reason = (!reasons.is_empty()).then(|| reasons.join("\n"));
                let (rid2, i, e) = (rid.clone(), idx as i64, error.clone());
                db_call(&db, move |d| {
                    d.fail_task_step(&rid2, i, &e, reason.as_deref(), attempts)
                })
                .await?;
                return db_call(&db, move |d| {
                    d.set_task_run_status(&rid, "failed", Some(&error))
                })
                .await;
            }
        }
    }
}
