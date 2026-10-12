//! The built-in tools: llama-server's own agent tools (`--tools`), rewritten in Rust with the
//! same names, argument schemas and result formats, so a model that already knows them and the
//! Playground keep working.
//!
//! The schemas are not retyped: `llama_tools.json` is the `GET /tools` answer of a real
//! llama-server (build 11338, commit `dcd387a41`, `tools/server/server-tools.cpp`), and the
//! definitions here are read from it. A test compares every definition with it.
//!
//! Differences from llama-server, all on purpose:
//!
//! * Paths are confined to the working directory ([`Workspace`](super::Workspace)); `~` is not
//!   expanded and absolute paths must lie inside it.
//! * Listings skip symlinks/junctions and honour `.gitignore` files below the searched
//!   directory without calling `git` (a repository's own config can make `git` run programs).
//! * `grep_search` uses Rust `regex` syntax (no look-around or back-references) and skips
//!   binary files.
//! * `get_info` reports the working directory the tools are confined to.
//! * `exec_shell_command` kills the whole process tree on timeout or cancel.

mod calculator;
mod datetime;
mod fetch;
mod files;
pub(crate) mod http;
mod info;
mod shell;
mod weather;
mod web_search;

use std::sync::{Arc, OnceLock};

use futures_util::future::BoxFuture;
use serde_json::Value;

use super::registry::{Tool, ToolCtx, ToolRegistry};
use super::types::*;

const LLAMA_TOOLS: &str = include_str!("llama_tools.json");

/// `GET /tools` of llama-server, as captured (see the module docs).
fn llama_tools() -> &'static Vec<Value> {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS.get_or_init(|| serde_json::from_str(LLAMA_TOOLS).expect("llama_tools.json is valid JSON"))
}

fn def_of(name: &str, class: PermissionClass) -> ToolDef {
    let f = llama_tools()
        .iter()
        .find(|t| t["tool"] == name)
        .map(|t| &t["definition"]["function"])
        .unwrap_or_else(|| panic!("{name} is not in llama_tools.json"));
    ToolDef {
        name: name.to_string(),
        description: f["description"].as_str().unwrap_or_default().to_string(),
        input_schema: f["parameters"].clone(),
        class,
        source: ToolSource::Builtin,
        uses_workspace: true,
    }
}

type BlockingFn = fn(&Value, &ToolCtx) -> Result<ToolOutput, ToolError>;

pub(super) fn simple_def(
    name: &str,
    description: &str,
    class: PermissionClass,
    input_schema: Value,
) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema,
        class,
        source: ToolSource::Builtin,
        uses_workspace: false,
    }
}

type SimpleFn = fn(Value) -> Result<ToolOutput, ToolError>;

struct SimpleTool {
    def: ToolDef,
    run: SimpleFn,
}
impl Tool for SimpleTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }
    fn call<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move { (self.run)(args) })
    }
}

struct AsyncTool {
    def: ToolDef,
    run: for<'a> fn(Value, &'a ToolCtx) -> BoxFuture<'a, Result<ToolOutput, ToolError>>,
}
impl Tool for AsyncTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }
    fn call<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
        (self.run)(args, ctx)
    }
}

/// A tool whose work is blocking (file system, child process): run on the blocking pool; it
/// polls `ctx.cancel` where it can wait long.
struct BlockingTool {
    def: ToolDef,
    run: BlockingFn,
}

impl Tool for BlockingTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }

    fn call<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
        let run = self.run;
        let ctx = ctx.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || run(&args, &ctx))
                .await
                .unwrap_or_else(|e| Err(ToolError::failed(format!("the tool crashed: {e}"))))
        })
    }
}

fn add(reg: &ToolRegistry, name: &str, class: PermissionClass, uses_ws: bool, run: BlockingFn) {
    let mut def = def_of(name, class);
    def.uses_workspace = uses_ws;
    reg.register(Arc::new(BlockingTool { def, run }));
}

/// Registers llama-server's seven tools (in its order).
pub fn register(reg: &ToolRegistry) {
    use PermissionClass::{Dangerous, Restricted};
    add(reg, "read_file", Restricted, true, files::read_file);
    add(
        reg,
        "file_glob_search",
        Restricted,
        true,
        files::file_glob_search,
    );
    add(reg, "grep_search", Restricted, true, files::grep_search);
    add(
        reg,
        "exec_shell_command",
        Dangerous,
        true,
        shell::exec_shell_command,
    );
    add(reg, "write_file", Dangerous, true, files::write_file);
    add(reg, "edit_file", Dangerous, true, files::edit_file);
    add(reg, "get_info", Restricted, true, info::get_info);
    reg.register(Arc::new(SimpleTool {
        def: datetime::def(),
        run: datetime::run,
    }));
    reg.register(Arc::new(SimpleTool {
        def: calculator::def(),
        run: calculator::run,
    }));
    reg.register(Arc::new(AsyncTool {
        def: fetch::def(),
        run: fetch::run,
    }));
    reg.register(Arc::new(AsyncTool {
        def: web_search::def(),
        run: web_search::run,
    }));
    reg.register(Arc::new(AsyncTool {
        def: weather::geocode_def(),
        run: weather::geocode,
    }));
    reg.register(Arc::new(AsyncTool {
        def: weather::weather_def(),
        run: weather::weather,
    }));
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::tools::registry::{RunLimits, ToolRegistry};
    use serde_json::json;

    /// Runs a built-in tool in `dir`.
    pub async fn run_in(dir: &std::path::Path, tool: &str, args: Value) -> ToolResult {
        let reg = ToolRegistry::with_builtin();
        let ctx = ToolCtx::new(Some(crate::tools::Workspace::new(dir).unwrap()));
        let inv = ToolInvocation::new(tool, args, ToolOrigin::new("test"));
        reg.run(&inv, &ctx, &RunLimits::default()).await
    }

    pub fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let p = d.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        d
    }

    #[test]
    fn definitions_are_llama_servers() {
        let reg = ToolRegistry::with_builtin();
        let defs = reg.list();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            &names[..7],
            &[
                "read_file",
                "file_glob_search",
                "grep_search",
                "exec_shell_command",
                "write_file",
                "edit_file",
                "get_info"
            ]
        );
        for d in &defs[..7] {
            let llama = llama_tools()
                .iter()
                .find(|t| t["tool"] == d.name.as_str())
                .unwrap();
            assert_eq!(
                llama["definition"]["function"]["parameters"],
                d.input_schema
            );
            assert_eq!(
                llama["definition"]["function"]["description"],
                d.description
            );
            // llama-server's `permissions.write` is our Dangerous class.
            assert_eq!(
                llama["permissions"]["write"].as_bool().unwrap(),
                d.class == PermissionClass::Dangerous,
                "{}",
                d.name
            );
            assert!(d.source == ToolSource::Builtin);
        }
        // Spot-check the schema the models see.
        let edit = reg.get("edit_file").unwrap();
        assert_eq!(
            edit.def().input_schema["properties"]["edits"]["items"]["required"],
            json!(["old_text", "new_text"])
        );
        assert_eq!(
            reg.get("exec_shell_command").unwrap().def().class,
            PermissionClass::Dangerous
        );
        assert_eq!(
            reg.get("read_file").unwrap().def().class,
            PermissionClass::Restricted
        );
    }
}
