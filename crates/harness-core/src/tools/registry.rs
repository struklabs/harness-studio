//! The tool registry and the guarded way to run a tool: argument check, timeout, result size
//! limit, cancellation. The registry does not decide *whether* a call may run (that is the
//! policy, applied by [`super::ToolService`]); it makes sure a call that runs stays bounded.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::Value;
use tokio::sync::watch;

use super::scope::Workspace;
use super::types::*;

/// Default time one call may take.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// A cancellation signal that blocking helpers (file walks, child processes) can poll and
/// async code can await.
#[derive(Debug, Clone)]
pub struct CancelToken {
    tx: Arc<watch::Sender<bool>>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            tx: Arc::new(watch::channel(false).0),
        }
    }

    pub fn cancel(&self) {
        self.tx.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.tx.borrow()
    }

    /// Completes when [`Self::cancel`] was called.
    pub async fn cancelled(&self) {
        let mut rx = self.tx.subscribe();
        let _ = rx.wait_for(|c| *c).await;
    }
}

/// Cancels its token when dropped: whatever a call started (a blocking walk, a child process)
/// stops when the call's future is dropped, by a timeout or by a client that went away.
struct CancelOnDrop(CancelToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// What a running tool gets besides its arguments.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    /// Set for tools with `uses_workspace`.
    pub workspace: Option<Workspace>,
    /// Cancelled when the call times out, is cancelled or is dropped. Long loops poll it.
    pub cancel: CancelToken,
    /// How long the call may take in all (the tool may use less).
    pub timeout: Duration,
    /// Id of the call being run (the invocation id), set by [`ToolRegistry::run`]. Lets a tool
    /// that waits for the user (the n8n proposals) tell the UI which call it belongs to.
    pub call_id: Option<String>,
    /// The run this call belongs to. When set, file-changing tools keep the previous content
    /// of a file as a checkpoint (`super::checkpoint`) so the change can be undone.
    pub checkpoint_group: Option<String>,
    network_target: Arc<Mutex<Option<String>>>,
}

impl ToolCtx {
    pub fn new(workspace: Option<Workspace>) -> Self {
        Self {
            workspace,
            cancel: CancelToken::new(),
            timeout: DEFAULT_TIMEOUT,
            call_id: None,
            checkpoint_group: None,
            network_target: Arc::new(Mutex::new(None)),
        }
    }

    /// Keeps the current content of `path` before `tool` changes it (see
    /// [`ToolCtx::checkpoint_group`]); `None` when checkpoints are off for this call.
    pub fn checkpoint(
        &self,
        tool: &str,
        path: &std::path::Path,
    ) -> Option<super::checkpoint::PendingChange> {
        let group = self.checkpoint_group.as_deref()?;
        super::checkpoint::PendingChange::begin(group, tool, path)
    }

    pub fn workspace(&self) -> Result<&Workspace, ToolError> {
        self.workspace.as_ref().ok_or_else(|| {
            ToolError::new(
                ToolErrorCode::NoWorkspace,
                "this tool needs a working directory and none was given",
            )
        })
    }

    pub fn set_network_target(&self, target: impl Into<String>) {
        *self
            .network_target
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(target.into());
    }

    pub(crate) fn network_target(&self) -> Option<String> {
        self.network_target
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// A tool: its definition and the code that runs it.
pub trait Tool: Send + Sync {
    fn def(&self) -> &ToolDef;
    fn call<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>>;
}

/// Per-call limits.
#[derive(Debug, Clone)]
pub struct RunLimits {
    pub timeout: Duration,
    pub max_result_chars: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_result_chars: RESULT_MAX_CHARS,
        }
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Mutex<Vec<Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry with the built-in tools (the ones llama-server ships with `--tools`).
    pub fn with_builtin() -> Self {
        let r = Self::new();
        super::builtin::register(&r);
        r
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Arc<dyn Tool>>> {
        self.tools.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Adds a tool; one with the same name is replaced (keeping its place in the list).
    pub fn register(&self, tool: Arc<dyn Tool>) {
        let mut tools = self.lock();
        match tools.iter().position(|t| t.def().name == tool.def().name) {
            Some(i) => tools[i] = tool,
            None => tools.push(tool),
        }
        drop(tools);
        crate::events::publish(crate::events::CoreEvent::ToolsChanged);
    }

    pub fn unregister(&self, name: &str) -> bool {
        let mut tools = self.lock();
        let before = tools.len();
        tools.retain(|t| t.def().name != name);
        let changed = tools.len() != before;
        drop(tools);
        if changed {
            crate::events::publish(crate::events::CoreEvent::ToolsChanged);
        }
        changed
    }

    pub fn list(&self) -> Vec<ToolDef> {
        self.lock().iter().map(|t| t.def().clone()).collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.lock().iter().find(|t| t.def().name == name).cloned()
    }

    /// Runs one call with the limits: arguments checked against the schema, the time limit,
    /// the cancellation of `ctx.cancel`, the result size limit. Never panics the caller: every
    /// outcome is a [`ToolResult`]. The policy is *not* consulted here.
    pub async fn run(&self, inv: &ToolInvocation, ctx: &ToolCtx, limits: &RunLimits) -> ToolResult {
        let started = Instant::now();
        let done = |r: Result<ToolOutput, ToolError>| -> ToolResult {
            let duration_ms = started.elapsed().as_millis() as u64;
            match r {
                Ok(out) => {
                    let (content, truncated) = truncate_chars(out.content, limits.max_result_chars);
                    ToolResult {
                        content,
                        error: None,
                        duration_ms,
                        truncated,
                        network_target: out.network_target,
                    }
                }
                Err(e) => {
                    let (content, truncated) = truncate_chars(e.message, limits.max_result_chars);
                    ToolResult {
                        content,
                        error: Some(e.code),
                        duration_ms,
                        truncated,
                        network_target: e.network_target.or_else(|| ctx.network_target()),
                    }
                }
            }
        };
        let Some(tool) = self.get(&inv.tool) else {
            return done(Err(ToolError::new(
                ToolErrorCode::UnknownTool,
                format!("unknown tool: {}", inv.tool),
            )));
        };
        if let Err(e) = validate_args(&tool.def().input_schema, &inv.args) {
            return done(Err(e));
        }
        // The tool sees a run-scoped token: cancelled when this call ends in any way (timeout,
        // cancellation, a dropped future), so blocking helpers and child processes stop. The
        // caller's token is only listened to, never cancelled by us.
        let run_cancel = CancelToken::new();
        let _guard = CancelOnDrop(run_cancel.clone());
        let mut run_ctx = ctx.clone();
        run_ctx.cancel = run_cancel;
        run_ctx.timeout = limits.timeout;
        run_ctx.call_id = Some(inv.id.clone());
        let call = tool.call(inv.args.clone(), &run_ctx);
        let result = tokio::select! {
            r = call => r,
            _ = tokio::time::sleep(limits.timeout) => Err(ToolError::new(
                ToolErrorCode::Timeout,
                format!("the tool did not finish within {} seconds and was stopped", limits.timeout.as_secs().max(1)),
            )),
            _ = ctx.cancel.cancelled() => Err(ToolError::new(
                ToolErrorCode::Cancelled,
                "the call was cancelled",
            )),
        };
        done(result)
    }
}

/// The JSON-Schema subset the built-in schemas use: `required` properties must be present and
/// the declared `type` of each given property must fit. Lenient like the models that write the
/// calls: an integer may come as a whole number or numeric text, a boolean as `"true"`/`"false"`.
pub fn validate_args(schema: &Value, args: &Value) -> Result<(), ToolError> {
    let Some(obj) = args.as_object() else {
        return Err(ToolError::invalid("arguments must be a JSON object"));
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if obj.get(name).is_none_or(Value::is_null) {
                return Err(ToolError::invalid(format!(
                    "missing required argument \"{name}\""
                )));
            }
        }
    }
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (name, value) in obj {
            if value.is_null() {
                continue;
            }
            if let Some(ty) = props
                .get(name)
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
            {
                if !type_fits(ty, value) {
                    return Err(ToolError::invalid(format!(
                        "argument \"{name}\" must be of type {ty}"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn type_fits(ty: &str, v: &Value) -> bool {
    match ty {
        "string" => v.is_string(),
        "integer" => {
            v.is_i64()
                || v.is_u64()
                || v.as_f64().is_some_and(|f| f.fract() == 0.0)
                || v.as_str().is_some_and(|s| s.trim().parse::<i64>().is_ok())
        }
        "number" => v.is_number() || v.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok()),
        "boolean" => v.is_boolean() || matches!(v.as_str(), Some("true" | "false")),
        "array" => v.is_array(),
        "object" => v.is_object(),
        _ => true,
    }
}

/// Argument readers shared by the built-in tools (same leniency as [`validate_args`]).
pub mod args {
    use super::*;

    pub fn string(a: &Value, key: &str) -> Result<String, ToolError> {
        a.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ToolError::invalid(format!("missing required argument \"{key}\"")))
    }

    pub fn opt_string(a: &Value, key: &str) -> Option<String> {
        a.get(key).and_then(Value::as_str).map(str::to_string)
    }

    pub fn opt_i64(a: &Value, key: &str) -> Option<i64> {
        let v = a.get(key)?;
        v.as_i64()
            .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    }

    pub fn opt_bool(a: &Value, key: &str) -> Option<bool> {
        let v = a.get(key)?;
        v.as_bool().or(match v.as_str() {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Echo(ToolDef);

    impl Tool for Echo {
        fn def(&self) -> &ToolDef {
            &self.0
        }
        fn call<'a>(
            &'a self,
            args: Value,
            _ctx: &'a ToolCtx,
        ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
            Box::pin(async move {
                match args["mode"].as_str() {
                    Some("sleep") => {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        Ok(ToolOutput::text("late"))
                    }
                    Some("big") => Ok(ToolOutput::text("y".repeat(RESULT_MAX_CHARS * 2))),
                    _ => Ok(ToolOutput::text(
                        args["text"].as_str().unwrap_or("").to_string(),
                    )),
                }
            })
        }
    }

    fn echo() -> Arc<dyn Tool> {
        Arc::new(Echo(ToolDef {
            name: "echo".into(),
            description: "echo".into(),
            input_schema: json!({"type": "object", "properties": {
                "text": {"type": "string"}, "mode": {"type": "string"}, "n": {"type": "integer"}
            }, "required": ["text"]}),
            class: PermissionClass::Safe,
            source: ToolSource::Builtin,
            uses_workspace: false,
        }))
    }

    fn inv(args: Value) -> ToolInvocation {
        ToolInvocation::new("echo", args, ToolOrigin::new("test"))
    }

    #[test]
    fn register_list_get_replace_unregister() {
        let r = ToolRegistry::new();
        r.register(echo());
        r.register(echo());
        assert_eq!(r.list().len(), 1);
        assert!(r.get("echo").is_some() && r.get("nope").is_none());
        assert!(r.unregister("echo") && !r.unregister("echo"));
        assert!(r.list().is_empty());
    }

    #[test]
    fn registry_mutations_notify_tool_consumers() {
        let mut rx = crate::events::subscribe();
        let r = ToolRegistry::new();
        r.register(echo());
        assert!(std::iter::from_fn(|| rx.try_recv().ok())
            .any(|e| matches!(e, crate::events::CoreEvent::ToolsChanged)));
        r.register(echo()); // Replacement must refresh schemas too.
        assert!(std::iter::from_fn(|| rx.try_recv().ok())
            .any(|e| matches!(e, crate::events::CoreEvent::ToolsChanged)));
        assert!(r.unregister("echo"));
        assert!(std::iter::from_fn(|| rx.try_recv().ok())
            .any(|e| matches!(e, crate::events::CoreEvent::ToolsChanged)));
        assert!(!r.unregister("missing"));
    }

    #[tokio::test]
    async fn run_validates_arguments_and_reports_unknown_tools() {
        let r = ToolRegistry::new();
        r.register(echo());
        let ctx = ToolCtx::new(None);
        let lim = RunLimits::default();
        let ok = r.run(&inv(json!({"text": "hi"})), &ctx, &lim).await;
        assert_eq!((ok.content.as_str(), ok.error), ("hi", None));
        for (args, needle) in [
            (json!({}), "missing required argument \"text\""),
            (json!({"text": 5}), "must be of type string"),
            (json!({"text": "a", "n": "x"}), "must be of type integer"),
            (json!("nope"), "must be a JSON object"),
        ] {
            let res = r.run(&inv(args), &ctx, &lim).await;
            assert_eq!(res.error, Some(ToolErrorCode::InvalidArguments), "{needle}");
            assert!(res.content.contains(needle), "{}", res.content);
        }
        // Numeric text is accepted for integers, like models send it.
        let ok = r
            .run(&inv(json!({"text": "a", "n": "12"})), &ctx, &lim)
            .await;
        assert!(!ok.is_error());
        let unknown = r
            .run(
                &ToolInvocation::new("zzz", json!({}), ToolOrigin::new("t")),
                &ctx,
                &lim,
            )
            .await;
        assert_eq!(unknown.error, Some(ToolErrorCode::UnknownTool));
    }

    #[tokio::test]
    async fn long_results_are_truncated_and_marked() {
        let r = ToolRegistry::new();
        r.register(echo());
        let res = r
            .run(
                &inv(json!({"text": "x", "mode": "big"})),
                &ToolCtx::new(None),
                &RunLimits::default(),
            )
            .await;
        assert!(res.truncated && !res.is_error());
        assert!(res
            .content
            .ends_with(&format!("…[truncated {} chars]", RESULT_MAX_CHARS)));
        assert_eq!(
            res.content.chars().count(),
            RESULT_MAX_CHARS
                + format!("\n…[truncated {} chars]", RESULT_MAX_CHARS)
                    .chars()
                    .count()
        );
    }

    #[tokio::test]
    async fn timeout_and_cancellation_end_the_call() {
        let r = ToolRegistry::new();
        r.register(echo());
        let lim = RunLimits {
            timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let t = Instant::now();
        let res = r
            .run(
                &inv(json!({"text": "x", "mode": "sleep"})),
                &ToolCtx::new(None),
                &lim,
            )
            .await;
        assert_eq!(res.error, Some(ToolErrorCode::Timeout));
        assert!(t.elapsed() < Duration::from_secs(5));

        let ctx = ToolCtx::new(None);
        let token = ctx.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token.cancel();
        });
        let res = r
            .run(
                &inv(json!({"text": "x", "mode": "sleep"})),
                &ctx,
                &RunLimits::default(),
            )
            .await;
        assert_eq!(res.error, Some(ToolErrorCode::Cancelled));
        assert!(ctx.cancel.is_cancelled());
    }
}
