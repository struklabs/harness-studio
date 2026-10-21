//! A coding run: a checkpoint, then the agent loop over the project folder with the coding
//! tools, the workspace rules and the approval of shell commands, then the list of changes.
//!
//! The model is reached over OpenAI chat completions at `base_url`: in the app that is the
//! Harness Gateway, so local and remote models work alike. Tools are the engine's own
//! (`read_file`, `file_glob_search`, `grep_search`, `get_info`, `write_file`, `edit_file`,
//! `exec_shell_command`), the same ones the Playground offers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::checkpoint::{Checkpoint, FileChange, Store};
use super::guard;
use crate::state::Project;
use crate::tools::agent::{run_agent, AgentCall, AgentHooks, AgentRequest, Approver, StopReason};
use crate::tools::{CancelToken, ToolDef, ToolOrigin, ToolService, ToolSettings};

/// How long a command waits for an answer before it counts as refused.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ROUNDS: u32 = 16;

const SYSTEM_PROMPT: &str = "You are a coding assistant working inside one project folder. \
Use the tools to look at the code before you change it: file_glob_search and grep_search to \
find things, read_file to read them. Change files with edit_file (small, exact edits) or \
write_file (new files). Keep changes minimal and in the style of the surrounding code. After \
changing code, run the project's tests or build with exec_shell_command when there is a \
command for it (the user approves each command); if a command fails, read the error and fix \
the cause. Paths are relative to the project folder; you cannot leave it. When you are done, \
reply with a short summary of what you changed and what you ran.";

#[derive(Debug, Clone, Deserialize)]
pub struct RunRequest {
    /// OpenAI-compatible base URL with the version prefix (the Gateway's `.../v1`).
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    pub model: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    Running,
    Done,
    Error,
    /// Refused: by a workspace rule, the tool policy or the user.
    Denied,
}

/// What the Coding area is told while a run goes.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Started {
        run_id: String,
        checkpoint: Checkpoint,
    },
    Round {
        run_id: String,
        round: u32,
    },
    Call {
        run_id: String,
        call_id: String,
        tool: String,
        summary: String,
        status: CallStatus,
    },
    Approval {
        run_id: String,
        approval_id: String,
        tool: String,
        summary: String,
        warnings: Vec<&'static str>,
    },
    Finished {
        run_id: String,
        stop: String,
        text: String,
        error: Option<String>,
        changes: usize,
    },
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct CallSummary {
    pub round: u32,
    pub tool: String,
    pub summary: String,
    pub status: CallStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub checkpoint: Checkpoint,
    /// `answered`, `round_limit`, `cancelled` or `error`.
    pub stop: String,
    pub text: String,
    pub error: Option<String>,
    pub calls: Vec<CallSummary>,
    pub changes: Vec<FileChange>,
}

// --- approvals ------------------------------------------------------------------------------

fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<bool>>> {
    static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<bool>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The user's answer to an `Approval` event. `false`: nothing was waiting for it.
pub fn answer_approval(approval_id: &str, allow: bool) -> bool {
    let sender = pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(approval_id);
    sender.is_some_and(|tx| tx.send(allow).is_ok())
}

fn cut(text: &str, max: usize) -> String {
    let mut s: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        s.push('…');
    }
    s
}

/// One line about a call for the progress list and the approval card.
pub fn summarize(tool: &str, args: &Value) -> String {
    let text = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    match tool {
        "exec_shell_command" => cut(text("command"), 400),
        "grep_search" => format!("{} in {}", cut(text("pattern"), 80), text("path")),
        "get_info" => String::new(),
        _ => cut(text("path"), 200),
    }
}

struct Run {
    run_id: String,
    folder: PathBuf,
    sink: Sink,
    cancel: CancelToken,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "coding run {}", self.run_id)
    }
}

impl AgentHooks for Run {
    fn guard(&self, tool: &str, args: &Value) -> Result<(), String> {
        guard::check_call(&self.folder, tool, args).inspect_err(|_| {
            (self.sink)(Event::Call {
                run_id: self.run_id.clone(),
                call_id: String::new(),
                tool: tool.to_string(),
                summary: summarize(tool, args),
                status: CallStatus::Denied,
            });
        })
    }

    fn round(&self, round: u32) {
        (self.sink)(Event::Round {
            run_id: self.run_id.clone(),
            round,
        });
    }

    fn call_started(&self, id: &str, tool: &str, args: &Value) {
        (self.sink)(Event::Call {
            run_id: self.run_id.clone(),
            call_id: id.to_string(),
            tool: tool.to_string(),
            summary: summarize(tool, args),
            status: CallStatus::Running,
        });
    }

    fn call_finished(&self, call: &AgentCall) {
        if call.result.is_none() {
            return; // refused by the guard (already reported) or unusable arguments
        }
        (self.sink)(Event::Call {
            run_id: self.run_id.clone(),
            call_id: call.id.clone(),
            tool: call.tool.clone(),
            summary: summarize(&call.tool, &args_of(call)),
            status: status_of(call),
        });
    }
}

fn args_of(call: &AgentCall) -> Value {
    serde_json::from_str(&call.arguments).unwrap_or(Value::Null)
}

fn status_of(call: &AgentCall) -> CallStatus {
    use crate::tools::ToolErrorCode as C;
    match call.result.as_ref().and_then(|r| r.error) {
        None => CallStatus::Done,
        Some(C::ApprovalRequired | C::PolicyOff | C::OutsideWorkspace) => CallStatus::Denied,
        Some(_) => CallStatus::Error,
    }
}

impl Approver for Run {
    fn approve<'a>(&'a self, def: &'a ToolDef, args: &'a Value) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            if !guard::is_shell_tool(&def.name) {
                // Reads, and edits (checkpointed, so undoable), inside the workspace.
                return true;
            }
            let command = args.get("command").and_then(Value::as_str).unwrap_or("");
            let approval_id = uuid::Uuid::new_v4().simple().to_string();
            let (tx, rx) = oneshot::channel();
            pending()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(approval_id.clone(), tx);
            (self.sink)(Event::Approval {
                run_id: self.run_id.clone(),
                approval_id: approval_id.clone(),
                tool: def.name.clone(),
                summary: summarize(&def.name, args),
                warnings: guard::check_command(command).warnings,
            });
            let allowed = tokio::select! {
                answer = rx => answer.unwrap_or(false),
                _ = self.cancel.cancelled() => false,
                _ = tokio::time::sleep(APPROVAL_TIMEOUT) => false,
            };
            pending()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&approval_id);
            allowed
        })
    }
}

/// What a run reports to and is stopped by.
#[derive(Clone)]
pub struct RunHandle {
    pub run_id: String,
    pub sink: Sink,
    pub cancel: CancelToken,
}

/// Runs one coding task for `project`. `Err` only when it could not start (no folder, no
/// checkpoint); a failed model request ends the run with `error` set in the summary.
pub async fn run_task(
    service: &ToolService,
    store: &Store,
    project: &Project,
    req: RunRequest,
    user_settings: &ToolSettings,
    handle: RunHandle,
) -> Result<RunSummary, String> {
    let RunHandle {
        run_id,
        sink,
        cancel,
    } = handle;
    let folder = guard::workspace_folder(project)?;
    let checkpoint = {
        let (store, id, folder, label) = (
            store.clone(),
            project.id.clone(),
            folder.clone(),
            cut(req.prompt.trim(), 120),
        );
        tokio::task::spawn_blocking(move || store.create(&id, &folder, &label))
            .await
            .map_err(|e| e.to_string())??
    };
    sink(Event::Started {
        run_id: run_id.clone(),
        checkpoint: checkpoint.clone(),
    });

    let run = Arc::new(Run {
        run_id: run_id.clone(),
        folder: folder.clone(),
        sink: sink.clone(),
        cancel: cancel.clone(),
    });
    let mut agent = AgentRequest::new(
        req.base_url,
        req.model.clone(),
        vec![
            json!({"role": "system", "content": SYSTEM_PROMPT}),
            json!({"role": "user", "content": req.prompt}),
        ],
    );
    agent.api_key = req.api_key;
    agent.tools = guard::CODING_TOOLS.iter().map(|s| s.to_string()).collect();
    agent.origin = ToolOrigin::new("coding").model(req.model);
    agent.workdir = Some(folder);
    agent.call_timeout = Some(CALL_TIMEOUT);
    agent.settings = Some(guard::coding_settings(user_settings));
    agent.limits.max_rounds = MAX_ROUNDS;
    agent.temperature = Some(0.2);
    agent.cancel = cancel;
    agent.hooks = Some(run.clone());

    let outcome = run_agent(service, agent, run.as_ref()).await;
    let (stop, text, error, calls) = match outcome {
        Ok(out) => {
            let stop = match out.stop {
                StopReason::Answered | StopReason::ClientToolCalls => "answered",
                StopReason::RoundLimit => "round_limit",
                StopReason::Cancelled => "cancelled",
            };
            let calls = out
                .calls
                .iter()
                .map(|c| CallSummary {
                    round: c.round,
                    tool: c.tool.clone(),
                    summary: summarize(&c.tool, &args_of(c)),
                    status: status_of(c),
                })
                .collect();
            (stop.to_string(), out.text, None, calls)
        }
        Err(e) => ("error".to_string(), String::new(), Some(e), Vec::new()),
    };
    let changes = {
        let (store, cp) = (store.clone(), checkpoint.clone());
        tokio::task::spawn_blocking(move || store.changes(&cp))
            .await
            .map_err(|e| e.to_string())??
    };
    sink(Event::Finished {
        run_id: run_id.clone(),
        stop: stop.clone(),
        text: text.clone(),
        error: error.clone(),
        changes: changes.len(),
    });
    Ok(RunSummary {
        run_id,
        checkpoint,
        stop,
        text,
        error,
        calls,
        changes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_show_what_matters() {
        assert_eq!(
            summarize("exec_shell_command", &json!({"command": "cargo test"})),
            "cargo test"
        );
        assert_eq!(
            summarize("edit_file", &json!({"path": "src/a.rs", "edits": []})),
            "src/a.rs"
        );
        assert_eq!(
            summarize("grep_search", &json!({"path": ".", "pattern": "foo"})),
            "foo in ."
        );
    }

    #[test]
    fn an_answer_reaches_only_a_waiting_approval() {
        assert!(!answer_approval("nobody", true));
        let (tx, mut rx) = oneshot::channel();
        pending().lock().unwrap().insert("x1".into(), tx);
        assert!(answer_approval("x1", false));
        assert!(!rx.try_recv().unwrap());
        assert!(!answer_approval("x1", true));
    }
}
