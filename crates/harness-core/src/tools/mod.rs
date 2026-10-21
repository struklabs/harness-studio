//! The tool engine: tools live in Harness's own core instead of in
//! llama-server's `--tools`, so every runtime, the Playground, the CLI and the Control API use
//! the same tools under the same rules.
//!
//! # Pieces
//! * [`ToolDef`] / [`ToolInvocation`] / [`ToolResult`] ([`types`]): what a tool is, one call, its
//!   outcome (text, error code, duration, truncation, network target).
//! * [`PermissionClass`] and [`ToolPolicy`] ([`policy`]): *safe* tools run freely, *restricted*
//!   ones (network, file reads) and *dangerous* ones (writes, shell, processes, credentials) are
//!   governed by the user's policy. The class is fixed at registration and the policy is the
//!   user's setting (`settings.tools`), so a model cannot raise its own permission.
//! * [`ToolRegistry`] ([`registry`]): register / list / get and the guarded `run`: argument check
//!   against the schema, a time limit, a result size limit (16 000 characters, the cut is
//!   marked), cancellation.
//! * [`Workspace`] ([`scope`]): file tools see one directory tree; `..`, absolute paths and
//!   symlinks cannot leave it.
//! * [`builtin`]: llama-server's seven tools in Rust (`read_file`, `file_glob_search`,
//!   `grep_search`, `get_info`, `write_file`, `edit_file`, `exec_shell_command`).
//! * [`ToolService`] ([`service`]): policy → working directory → run → tool log
//!   (`analytics.db` `tool_calls`). The Control API and the CLI go through it.
//!
//! * [`ToolLimits`] ([`limits`]): rounds, per-call time and result size, read by every surface.
//! * [`RunTable`] ([`runs`]): running calls by client id, so a Stop button can cancel one.
//! * [`agent`]: the non-streaming agent loop (`run_agent`) and the pieces both loops share.
//! * [`agent_stream`]: the streaming agent loop: tokens to a sink, `tool_calls` collected
//!   from the deltas, policy → run → result → next round, cancellation, a hook before each next
//!   round. The gateway's server-side tools run on it; the Playground keeps its own loop (see
//!   `private hs-internal architecture notes`, "Tool loop").
//!
//! * [`package`], [`js_engine`], [`installed`]: the tool package format, the embedded JS
//!   runtime whose only reach into the world is `harness.http` / `harness.fs` under the
//!   manifest's permissions, and the packages kept in `<data dir>/tools/` and registered with
//!   source `installed`.
//!
//! * [`package_test`], [`generator`]: the test file of a package, and the user-started tool
//!   generator (the chat model writes manifest + code + test, the user approves, the tool is
//!   registered with source `generated`).
//!
//! # Not here yet
//! The MCP server surface (Harness tools offered to other MCP clients).

pub mod agent;
pub mod agent_stream;
pub mod automatic;
pub mod builtin;
pub mod checkpoint;
pub mod generator;
pub mod glob;
pub mod installed;
pub mod js_engine;
pub mod limits;
pub mod mcp_client;
pub mod official_registry;
pub mod package;
pub mod package_test;
pub mod policy;
pub mod registry;
pub mod runs;
pub mod scope;
pub mod service;
pub mod text_call;
pub mod types;

pub use limits::{ToolLimits, MAX_ROUNDS};
pub use mcp_client::{
    McpAuthState, McpHub, McpServerState, McpServerStatus, RemoteResource, RemoteResourceTemplate,
    ResourceContent, ResourceList,
};
pub use policy::{
    decide, save_settings, save_web_search_settings, set_mcp_class, web_search_settings, Decision,
    SearchProviderKind, SetWebSearchSettings, ToolPolicy, ToolSettings, WebSearchSettings,
    WebSearchSettingsView,
};
pub use registry::{CancelToken, RunLimits, Tool, ToolCtx, ToolRegistry, DEFAULT_TIMEOUT};
pub use runs::{RunGuard, RunTable};
pub use scope::Workspace;
pub use service::{redact_args, LogSink, RunOptions, ToolEntry, ToolListing, ToolService};
pub use types::*;
