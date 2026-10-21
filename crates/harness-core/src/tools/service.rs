//! [`ToolService`]: the one entry point every surface (Control API, CLI, later the Playground's
//! agent loop, MCP and the gateway) uses to run a tool: policy → working directory → guarded run
//! → tool log. The surfaces only translate their own request/response shapes; the rules live here.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::analytics::{AnalyticsDb, ToolCallRow};
use crate::config::AppSettings;

use super::limits::ToolLimits;
use super::mcp_client::McpHub;
use super::policy::{Decision, ToolPolicy, ToolSettings, WebSearchSettings};
use super::registry::{CancelToken, RunLimits, ToolCtx, ToolRegistry, DEFAULT_TIMEOUT};
use super::runs::RunTable;
use super::scope::Workspace;
use super::types::*;

/// Where the tool log goes.
#[derive(Clone)]
pub enum LogSink {
    /// Nowhere.
    None,
    /// The process-wide `analytics.db`, when analytics recording is on.
    Shared,
    /// A given database (tests).
    Db(Arc<AnalyticsDb>),
}

/// Options of one run.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// The working directory of file tools and the shell. `None`: `settings.tools.workdir`.
    pub workdir: Option<PathBuf>,
    /// The user approved this call (they were asked). Only counts when the policy says "ask":
    /// it never overrides a policy of "off".
    pub approved: bool,
    /// Cancels the call (kills a running shell's process tree).
    pub cancel: CancelToken,
    /// Time limit; `None` = the default (120 s). Tools may bound themselves tighter.
    pub timeout: Option<Duration>,
    /// Policy to apply; `None` = the saved settings.
    pub settings: Option<ToolSettings>,
}

/// A tool with what the current policy says about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolEntry {
    #[serde(flatten)]
    pub def: ToolDef,
    /// What a call would do under the current policy: run, ask or refuse.
    pub decision: Decision,
    /// The MCP server a tool comes from (`None` for built-in tools).
    #[serde(default)]
    pub server: Option<String>,
    /// The class the tool has by itself (an MCP tool's annotation class), before the user's
    /// own choice in `settings.tools.mcp_classes`; `def.class` is the one in force.
    #[serde(default)]
    pub default_class: Option<super::types::PermissionClass>,
}

/// `GET /tools`: the tools and the policy they are judged by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolListing {
    pub policy: ToolPolicy,
    pub trusted: Vec<String>,
    /// Default working directory (empty = none).
    pub workdir: String,
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub missing_workspaces: Vec<String>,
    pub web_search: WebSearchSettings,
    /// `settings.tools.mcp_expose`: the built-in tools are offered to MCP clients.
    #[serde(default)]
    pub mcp_expose: bool,
    /// Rounds, per-call time and result size every surface shares.
    pub limits: ToolLimits,
    pub tools: Vec<ToolEntry>,
}

pub struct ToolService {
    registry: Arc<ToolRegistry>,
    sink: LogSink,
    mcp: McpHub,
}

/// Replaces long strings (file contents, whole scripts) by their start and length and caps the
/// total: the tool log records what was asked, not the data that went through. Credentials in
/// what is kept are replaced too ([`crate::leakguard`]).
pub fn redact_args(args: &Value) -> String {
    let mut args = args.clone();
    crate::leakguard::redact_value(&mut args, crate::leakguard::Mode::Broad);
    let args = &args;
    const MAX_STRING: usize = 200;
    const KEEP: usize = 120;
    const MAX_TOTAL: usize = 4000;
    fn shorten(v: &Value) -> Value {
        match v {
            Value::String(s) if s.chars().count() > MAX_STRING => {
                let head: String = s.chars().take(KEEP).collect();
                Value::String(format!("{head}…[{} chars]", s.chars().count()))
            }
            Value::Array(a) => Value::Array(a.iter().map(shorten).collect()),
            Value::Object(o) => {
                Value::Object(o.iter().map(|(k, v)| (k.clone(), shorten(v))).collect())
            }
            other => other.clone(),
        }
    }
    let text = shorten(args).to_string();
    if text.chars().count() > MAX_TOTAL {
        let head: String = text.chars().take(MAX_TOTAL).collect();
        format!("{head}…")
    } else {
        text
    }
}

/// The tools in an order that does not depend on when they were registered: the built-ins in
/// their own fixed order first, then the installed and MCP tools by name. MCP servers connect in
/// parallel and a reconnect appends its tools again, so registration order varies between
/// listings; a chat sends the tools in this order, and a stable list keeps the prompt's prefix
/// (and llama-server's prompt cache) the same across turns.
fn stable_order(mut tools: Vec<ToolDef>) -> Vec<ToolDef> {
    // A stable sort: built-ins compare equal among themselves and keep their order.
    tools.sort_by(|a, b| {
        let builtin = |d: &ToolDef| d.source == ToolSource::Builtin;
        match (builtin(a), builtin(b)) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => a.name.cmp(&b.name),
        }
    });
    tools
}

static SHARED: OnceLock<ToolService> = OnceLock::new();

impl ToolService {
    /// A service without MCP servers (tests, built-in tools only).
    pub fn new(registry: ToolRegistry, sink: LogSink) -> Self {
        let registry = Arc::new(registry);
        let mcp = McpHub::none(registry.clone());
        Self {
            registry,
            sink,
            mcp,
        }
    }

    /// A service whose MCP tools come from `mcp_servers.json` (the app's own client).
    pub fn with_app_mcp(registry: ToolRegistry, sink: LogSink) -> Self {
        let registry = Arc::new(registry);
        let mcp = McpHub::from_app_config(registry.clone());
        Self {
            registry,
            sink,
            mcp,
        }
    }

    /// A service over a given connection manager (it registers into `registry`).
    pub fn with_mcp(registry: Arc<ToolRegistry>, sink: LogSink, mcp: McpHub) -> Self {
        Self {
            registry,
            sink,
            mcp,
        }
    }

    /// The MCP connection manager.
    pub fn mcp(&self) -> &McpHub {
        &self.mcp
    }

    /// [`Self::sync_mcp`] that returns after at most `wait`: the start-up goes on in the
    /// background (the tools appear in the registry when it is done, and `mcp().status()` shows
    /// `connecting` meanwhile). For screens that must not hang on a slow first `npx` start.
    pub async fn sync_mcp_bounded(&self, retry_failed: bool, wait: std::time::Duration) {
        let hub = self.mcp.clone();
        let mut task = tokio::spawn(async move { hub.sync(retry_failed).await });
        tokio::select! {
            _ = &mut task => {}
            _ = tokio::time::sleep(wait) => {}
        }
    }

    /// Connects the configured MCP servers that are not connected yet and registers their
    /// tools (see [`McpHub::sync`]). Surfaces call this before they list tools.
    pub async fn sync_mcp(&self, retry_failed: bool) {
        self.mcp.sync(retry_failed).await;
    }

    /// The process-wide service: built-in tools, the installed tools of `<data dir>/tools/` and
    /// the MCP servers of `mcp_servers.json`, logging to `analytics.db`. After the installed set
    /// changes, [`Self::reload_installed`] brings the registry up to date.
    pub fn shared() -> &'static ToolService {
        SHARED.get_or_init(|| {
            let registry = ToolRegistry::with_builtin();
            for (name, error) in super::installed::sync(&super::installed::tools_root(), &registry)
            {
                eprintln!("[Harness Tools] installed tool {name} not loaded: {error}");
            }
            crate::automations::tools::sync(&registry);
            ToolService::with_app_mcp(registry, LogSink::Shared)
        })
    }

    /// The process-wide service when something already created it (settings code that only
    /// needs to refresh the registry must not start the service).
    pub fn try_shared() -> Option<&'static ToolService> {
        SHARED.get()
    }

    /// Re-reads `<data dir>/tools/` into the registry (after an install, removal or switch).
    /// Returns the packages left out, as `(name, keyed error)`.
    pub fn reload_installed(&self) -> Vec<(String, String)> {
        super::installed::sync(&super::installed::tools_root(), &self.registry)
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    fn settings(opts_settings: &Option<ToolSettings>) -> ToolSettings {
        opts_settings
            .clone()
            .unwrap_or_else(|| AppSettings::load().tools)
    }

    /// `def` as the user's settings and permissions judge it: the class in force and what a
    /// call does. An MCP tool is also judged by its server's trust level and grants
    /// (`crate::permissions`); a class the user gave this very tool wins over the server's.
    pub fn judge(
        &self,
        settings: &ToolSettings,
        perms: &crate::permissions::Snapshot,
        def: &ToolDef,
    ) -> (ToolDef, Decision) {
        let mut eff = settings.effective(def);
        let mut trusted = settings.is_trusted(&def.name);
        if def.source == ToolSource::Mcp {
            if let Some(server) = self.mcp.server_of(&def.name) {
                let by_hand = settings
                    .mcp_classes
                    .as_ref()
                    .is_some_and(|m| m.contains_key(&def.name));
                let (class, granted) = perms.judge_mcp(&server, eff.class, by_hand);
                eff.class = class;
                trusted |= granted;
            }
        }
        let decision = super::policy::decide(settings.policy, eff.class, trusted);
        (eff, decision)
    }

    /// The tools with the decision each would get under `settings`.
    pub fn listing(&self, settings: &ToolSettings) -> ToolListing {
        let perms = crate::permissions::Permissions::shared().snapshot();
        ToolListing {
            policy: settings.policy,
            trusted: settings.trusted.clone(),
            workdir: settings.workdir.clone(),
            workspaces: settings.clone().normalized().workspaces.unwrap_or_default(),
            missing_workspaces: settings
                .clone()
                .normalized()
                .workspaces
                .unwrap_or_default()
                .into_iter()
                .filter(|p| !std::path::Path::new(p).is_dir())
                .collect(),
            web_search: settings.web_search.clone(),
            mcp_expose: settings.mcp_expose,
            limits: ToolLimits::default(),
            tools: stable_order(self.registry.list())
                .into_iter()
                .map(|def| {
                    let default_class = Some(def.class);
                    let (def, decision) = self.judge(settings, &perms, &def);
                    ToolEntry {
                        default_class,
                        decision,
                        server: self.mcp.server_of(&def.name),
                        def,
                    }
                })
                .collect(),
        }
    }

    /// The saved settings' [`Self::listing`].
    pub fn current_listing(&self) -> ToolListing {
        self.listing(&AppSettings::load().tools)
    }

    /// What a call of `tool` would do right now (`None` = no such tool).
    pub fn decision(&self, tool: &str, settings: &ToolSettings) -> Option<Decision> {
        let t = self.registry.get(tool)?;
        let perms = crate::permissions::Permissions::shared().snapshot();
        Some(self.judge(settings, &perms, t.def()).1)
    }

    /// The class in force and the decision of `tool` under `settings` (`None`: unknown tool).
    pub fn judged(&self, tool: &str, settings: &ToolSettings) -> Option<(ToolDef, Decision)> {
        let t = self.registry.get(tool)?;
        let perms = crate::permissions::Permissions::shared().snapshot();
        Some(self.judge(settings, &perms, t.def()))
    }

    /// Runs one call: policy, working directory, limits, log. Every outcome is a
    /// [`ToolResult`]; the caller turns `error` into whatever its surface needs.
    pub async fn run(&self, inv: ToolInvocation, opts: RunOptions) -> ToolResult {
        let started = std::time::Instant::now();
        let settings = Self::settings(&opts.settings);
        // A tool the registry does not know may belong to an MCP server that is not connected
        // yet (a CLI call, an API call before anyone listed the tools).
        if self.registry.get(&inv.tool).is_none() {
            self.mcp.sync(false).await;
        }
        let perms = crate::permissions::Permissions::shared().snapshot();
        let judged = self
            .registry
            .get(&inv.tool)
            .map(|t| self.judge(&settings, &perms, t.def()));
        let def = judged.as_ref().map(|(d, _)| d.clone());
        let mut approved = false;
        let result = match &judged {
            None => ToolResult::failure(
                ToolErrorCode::UnknownTool,
                format!("unknown tool: {}", inv.tool),
                0,
            ),
            Some((def, decision)) => match *decision {
                Decision::Refuse => ToolResult::failure(
                    ToolErrorCode::PolicyOff,
                    "tools are turned off (tool policy: off); the user can turn them on in Settings",
                    0,
                ),
                Decision::Ask if !opts.approved => ToolResult::failure(
                    ToolErrorCode::ApprovalRequired,
                    format!(
                        "the tool \"{}\" ({}) needs the user's approval and none was given",
                        def.name,
                        def.class.as_str()
                    ),
                    0,
                ),
                decision => {
                    approved = decision == Decision::Ask;
                    self.execute(&inv, def, &opts, &settings).await
                }
            },
        };
        let mut result = result;
        if result.duration_ms == 0 {
            result.duration_ms = started.elapsed().as_millis() as u64;
        }
        self.log(&inv, def.as_ref(), approved, &result).await;
        result
    }

    /// Logs a call the user was asked about and refused. Nothing runs (not even if the policy
    /// was loosened while the question was open): the log gets the same `approval_required`
    /// row a refused call has, so refusals show in the tool log next to the runs.
    pub async fn record_denied(&self, inv: ToolInvocation) -> ToolResult {
        let def = self.registry.get(&inv.tool).map(|t| t.def().clone());
        let result = ToolResult::failure(
            ToolErrorCode::ApprovalRequired,
            "the user denied this tool call",
            0,
        );
        self.log(&inv, def.as_ref(), false, &result).await;
        result
    }

    /// [`Self::run`] that can be cancelled from outside while it runs: the call is listed in
    /// `table` under `inv.id` (the Playground's Stop button cancels by that id). A token
    /// already in `opts.cancel` is replaced by the table's.
    pub async fn run_tracked(
        &self,
        table: &Arc<RunTable>,
        inv: ToolInvocation,
        mut opts: RunOptions,
    ) -> ToolResult {
        let (token, _guard) = table.begin(&inv.id);
        opts.cancel = token;
        self.run(inv, opts).await
    }

    async fn execute(
        &self,
        inv: &ToolInvocation,
        def: &ToolDef,
        opts: &RunOptions,
        settings: &ToolSettings,
    ) -> ToolResult {
        let workspace = if def.uses_workspace {
            let dir = opts.workdir.clone().or_else(|| {
                (!settings.workdir.is_empty()).then(|| PathBuf::from(&settings.workdir))
            });
            match dir {
                None => {
                    return ToolResult::failure(
                        ToolErrorCode::NoWorkspace,
                        "no working directory: name one for this call or set a default in the tool settings",
                        0,
                    )
                }
                Some(dir) => match Workspace::new(&dir) {
                    Ok(ws) => Some(ws),
                    Err(e) => return ToolResult::failure(e.code, e.message, 0),
                },
            }
        } else {
            None
        };
        let mut ctx = ToolCtx::new(workspace);
        ctx.cancel = opts.cancel.clone();
        ctx.checkpoint_group = Some(
            inv.origin
                .conversation
                .clone()
                .unwrap_or_else(|| inv.origin.client.clone()),
        );
        let limits = RunLimits {
            timeout: opts.timeout.unwrap_or(DEFAULT_TIMEOUT),
            ..Default::default()
        };
        self.registry.run(inv, &ctx, &limits).await
    }

    async fn log(
        &self,
        inv: &ToolInvocation,
        def: Option<&ToolDef>,
        approved: bool,
        result: &ToolResult,
    ) {
        let row = ToolCallRow {
            id: inv.id.clone(),
            started_at_ms: inv.started_at_ms,
            client: inv.origin.client.clone(),
            conversation: inv.origin.conversation.clone(),
            model: inv.origin.model.clone(),
            tool: inv.tool.clone(),
            class: def.map_or("unknown", |d| d.class.as_str()).to_string(),
            source: def.map_or("unknown", |d| d.source.as_str()).to_string(),
            args: redact_args(&inv.args),
            approved,
            error_code: result.error.map(|c| c.as_str().to_string()),
            duration_ms: result.duration_ms,
            result_chars: result.content.chars().count() as u64,
            truncated: result.truncated,
            network_target: result.network_target.clone(),
        };
        let db = match &self.sink {
            LogSink::None => return,
            LogSink::Db(db) => db.clone(),
            LogSink::Shared => {
                if !AppSettings::load().analytics.enabled {
                    return;
                }
                match crate::analytics::shared() {
                    Ok(db) => db,
                    Err(e) => {
                        eprintln!("[Harness Tools] {e}");
                        return;
                    }
                }
            }
        };
        match tokio::task::spawn_blocking(move || db.insert_tool_call(&row)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("[Harness Tools] {e}"),
            Err(e) => eprintln!("[Harness Tools] {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::ToolCallQuery;
    use crate::tools::policy::ToolPolicy;
    use serde_json::json;

    /// Built-ins keep their order; MCP and installed tools follow by name, however they were
    /// registered (MCP servers connect in parallel).
    #[test]
    fn listing_order_does_not_follow_registration() {
        let def = |name: &str, source| ToolDef {
            name: name.into(),
            description: String::new(),
            input_schema: json!({}),
            class: PermissionClass::Safe,
            source,
            uses_workspace: false,
        };
        let names = |tools: Vec<ToolDef>| -> Vec<String> {
            stable_order(tools).into_iter().map(|d| d.name).collect()
        };
        let a = names(vec![
            def("read_file", ToolSource::Builtin),
            def("srv_b__x", ToolSource::Mcp),
            def("weather", ToolSource::Builtin),
            def("my_pkg", ToolSource::Installed),
            def("srv_a__y", ToolSource::Mcp),
        ]);
        let b = names(vec![
            def("srv_a__y", ToolSource::Mcp),
            def("read_file", ToolSource::Builtin),
            def("my_pkg", ToolSource::Installed),
            def("srv_b__x", ToolSource::Mcp),
            def("weather", ToolSource::Builtin),
        ]);
        assert_eq!(
            a,
            ["read_file", "weather", "my_pkg", "srv_a__y", "srv_b__x"]
        );
        assert_eq!(a, b);
    }

    // Fields drop in order: the service and database close before `_dir` is removed (Windows
    // cannot delete a directory with an open SQLite file).
    struct Fixture {
        work: tempfile::TempDir,
        service: ToolService,
        db: Arc<AnalyticsDb>,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(AnalyticsDb::open(&dir.path().join("a.db")).unwrap());
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("a.txt"), "hello\n").unwrap();
        Fixture {
            service: ToolService::new(ToolRegistry::with_builtin(), LogSink::Db(db.clone())),
            _dir: dir,
            work,
            db,
        }
    }

    fn opts(f: &Fixture, policy: ToolPolicy, trusted: &[&str], approved: bool) -> RunOptions {
        RunOptions {
            workdir: Some(f.work.path().to_path_buf()),
            approved,
            settings: Some(ToolSettings {
                policy,
                trusted: trusted.iter().map(|s| s.to_string()).collect(),
                workdir: String::new(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn call(tool: &str, args: Value) -> ToolInvocation {
        ToolInvocation::new(
            tool,
            args,
            ToolOrigin::new("test").conversation("c1").model("qwen"),
        )
    }

    #[tokio::test]
    async fn the_policy_gates_every_call() {
        let f = fixture();
        let write = || call("write_file", json!({"path": "w.txt", "content": "x"}));
        let read = || call("read_file", json!({"path": "a.txt"}));

        // Off: nothing runs, not even with an approval.
        let r = f
            .service
            .run(read(), opts(&f, ToolPolicy::Off, &[], true))
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::PolicyOff));

        // Ask every time: refused until approved.
        let r = f
            .service
            .run(read(), opts(&f, ToolPolicy::AskEveryTime, &[], false))
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::ApprovalRequired));
        let r = f
            .service
            .run(write(), opts(&f, ToolPolicy::AskEveryTime, &[], false))
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::ApprovalRequired));
        assert!(!f.work.path().join("w.txt").exists());
        let r = f
            .service
            .run(read(), opts(&f, ToolPolicy::AskEveryTime, &[], true))
            .await;
        assert_eq!(r.content, "hello\n");

        // Allow trusted: only the trusted tool runs unasked.
        let r = f
            .service
            .run(
                write(),
                opts(&f, ToolPolicy::AllowTrusted, &["read_file"], false),
            )
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::ApprovalRequired));
        let r = f
            .service
            .run(
                read(),
                opts(&f, ToolPolicy::AllowTrusted, &["read_file"], false),
            )
            .await;
        assert!(!r.is_error());
        let r = f
            .service
            .run(
                write(),
                opts(&f, ToolPolicy::AllowTrusted, &["write_file"], false),
            )
            .await;
        assert!(!r.is_error(), "{}", r.content);
        assert!(f.work.path().join("w.txt").exists());

        // Allow all.
        let r = f
            .service
            .run(
                call("write_file", json!({"path": "w2.txt", "content": "y"})),
                opts(&f, ToolPolicy::AllowAll, &[], false),
            )
            .await;
        assert!(!r.is_error());
        // Unknown tool.
        let r = f
            .service
            .run(
                call("nope", json!({})),
                opts(&f, ToolPolicy::AllowAll, &[], false),
            )
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::UnknownTool));
    }

    #[tokio::test]
    async fn calls_need_a_working_directory() {
        let f = fixture();
        let mut o = opts(&f, ToolPolicy::AllowAll, &[], false);
        o.workdir = None;
        let r = f
            .service
            .run(call("read_file", json!({"path": "a.txt"})), o.clone())
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::NoWorkspace));
        // The default from the settings is used when the call names none.
        o.settings.as_mut().unwrap().workdir = f.work.path().to_string_lossy().to_string();
        let r = f
            .service
            .run(call("read_file", json!({"path": "a.txt"})), o.clone())
            .await;
        assert_eq!(r.content, "hello\n");
        // A directory that is not there.
        o.workdir = Some(f.work.path().join("missing"));
        let r = f
            .service
            .run(call("read_file", json!({"path": "a.txt"})), o)
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::NoWorkspace));
    }

    #[tokio::test]
    async fn every_call_is_logged_with_its_origin_and_outcome() {
        let f = fixture();
        let long = "z".repeat(5000);
        let ok = call("write_file", json!({"path": "big.txt", "content": long}));
        let ok_id = ok.id.clone();
        f.service
            .run(ok, opts(&f, ToolPolicy::AskEveryTime, &[], true))
            .await;
        let refused = call("read_file", json!({"path": "a.txt"}));
        let refused_id = refused.id.clone();
        f.service
            .run(refused, opts(&f, ToolPolicy::AskEveryTime, &[], false))
            .await;
        let escaped = call("read_file", json!({"path": "../x"}));
        f.service
            .run(escaped, opts(&f, ToolPolicy::AllowAll, &[], false))
            .await;
        f.service
            .run(
                call("nope", json!({})),
                opts(&f, ToolPolicy::AllowAll, &[], false),
            )
            .await;

        let rows = f.db.tool_calls(&ToolCallQuery::default()).unwrap();
        assert_eq!(rows.len(), 4);
        let by_id = |id: &str| rows.iter().find(|r| r.id == id).unwrap();

        let w = by_id(&ok_id);
        assert_eq!(
            (
                w.client.as_str(),
                w.conversation.as_deref(),
                w.model.as_deref()
            ),
            ("test", Some("c1"), Some("qwen"))
        );
        assert_eq!(
            (w.tool.as_str(), w.class.as_str(), w.source.as_str()),
            ("write_file", "dangerous", "builtin")
        );
        assert!(w.approved && w.error_code.is_none());
        // The 5000-character content is not stored, only its length.
        assert!(
            w.args.contains("[5000 chars]") && w.args.len() < 400,
            "{}",
            w.args
        );
        assert!(w.result_chars > 0);

        let r = by_id(&refused_id);
        assert!(!r.approved);
        assert_eq!(r.error_code.as_deref(), Some("approval_required"));

        let codes: Vec<_> = rows
            .iter()
            .filter_map(|r| r.error_code.as_deref())
            .collect();
        assert!(
            codes.contains(&"outside_workspace") && codes.contains(&"unknown_tool"),
            "{codes:?}"
        );
    }

    /// The Playground's Stop button: a call started with `run_tracked` is cancelled by its id
    /// while it runs, and is no longer listed once it ended.
    #[tokio::test]
    async fn a_tracked_call_is_cancelled_by_its_id() {
        let f = fixture();
        let table = RunTable::new();
        let sleep = if cfg!(windows) {
            "ping -n 30 127.0.0.1"
        } else {
            "sleep 30"
        };
        let inv = call("exec_shell_command", json!({"command": sleep}));
        let id = inv.id.clone();
        let o = opts(&f, ToolPolicy::AllowAll, &[], false);
        let t = table.clone();
        let canceller = tokio::spawn(async move {
            for _ in 0..200 {
                if t.cancel(&id) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            false
        });
        let started = std::time::Instant::now();
        let r = f.service.run_tracked(&table, inv, o).await;
        assert!(canceller.await.unwrap(), "the call was never listed");
        assert_eq!(r.error, Some(ToolErrorCode::Cancelled), "{}", r.content);
        assert!(started.elapsed() < Duration::from_secs(20));
        assert_eq!(table.running(), 0);
    }

    #[tokio::test]
    async fn a_denied_call_is_logged_and_never_runs() {
        let f = fixture();
        let inv = call("write_file", json!({"path": "no.txt", "content": "x"}));
        let id = inv.id.clone();
        let r = f.service.record_denied(inv).await;
        assert_eq!(r.error, Some(ToolErrorCode::ApprovalRequired));
        assert!(!f.work.path().join("no.txt").exists());
        let rows = f.db.tool_calls(&ToolCallQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert!(!rows[0].approved);
        assert_eq!(rows[0].error_code.as_deref(), Some("approval_required"));
        assert_eq!(rows[0].class, "dangerous");
    }

    #[test]
    fn listing_shows_the_decision_per_tool() {
        let f = fixture();
        let l = f.service.listing(&ToolSettings {
            policy: ToolPolicy::AllowTrusted,
            trusted: vec!["write_file".into()],
            workdir: String::new(),
            ..Default::default()
        });
        let decision = |n: &str| l.tools.iter().find(|t| t.def.name == n).unwrap().decision;
        assert_eq!(decision("write_file"), Decision::Run);
        assert_eq!(decision("exec_shell_command"), Decision::Ask);
        assert_eq!(decision("read_file"), Decision::Ask);
        assert_eq!(decision("load_skill"), Decision::Run);
        assert_eq!(l.tools.len(), 14);
        let json = serde_json::to_value(&l).unwrap();
        // The definition's fields sit beside `decision`.
        assert_eq!(json["tools"][0]["name"], "read_file");
        assert_eq!(json["tools"][0]["class"], "restricted");
        assert_eq!(json["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(json["policy"], "allow_trusted");
    }

    #[test]
    fn args_are_shortened_for_the_log() {
        let v = json!({"path": "a", "edits": [{"old_text": "x".repeat(300), "new_text": "y"}]});
        let s = redact_args(&v);
        assert!(s.contains("[300 chars]") && !s.contains(&"x".repeat(150)));
        let many: Vec<String> = (0..2000).map(|i| format!("item-{i}")).collect();
        assert!(redact_args(&json!({"v": many})).chars().count() <= 4001);
    }
}
