//! OpenClaw (upstream npm package `openclaw`) pieces of the agent adapter: where it keeps its
//! state, how Harness is wired in as a model provider, how a task is launched and how a managed
//! install is made. The shared connect/restore/history/run machinery lives in `agents.rs`.
//!
//! Upstream facts (docs.openclaw.ai, checked against the pinned release):
//! * install: `npm install -g openclaw@<version>`; Node 24.16+ (24.x) or 26.1+ is required;
//! * config: JSON5 file `openclaw.json` in the state directory (`~/.openclaw` by default);
//!   `OPENCLAW_STATE_DIR` moves the directory, `OPENCLAW_CONFIG_PATH` the file;
//! * one headless turn: `openclaw agent exec --config <file> --state-dir <dir> --cwd <dir>`.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Pinned upstream release (like `HERMES_REVISION`); an update is a deliberate change here.
pub(super) const OPENCLAW_VERSION: &str = "2026.10.5";
/// Provider id Harness registers inside OpenClaw's model catalog.
pub(super) const PROVIDER: &str = "harness";
/// First npm that understands `--allow-scripts` (upstream documents it for newer npm only).
const ALLOW_SCRIPTS_NPM: (u32, u32) = (11, 16);

fn home_dir() -> Option<PathBuf> {
    // Node's os.homedir(): USERPROFILE on Windows, HOME elsewhere.
    #[cfg(windows)]
    let var = "USERPROFILE";
    #[cfg(not(windows))]
    let var = "HOME";
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}
pub(super) fn state_dir() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("OPENCLAW_STATE_DIR").filter(|v| !v.is_empty()) {
        return Ok(p.into());
    }
    std::env::var_os("OPENCLAW_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(home_dir)
        .map(|p| p.join(".openclaw"))
        .ok_or_else(|| "agents.home_missing".into())
}
pub(super) fn config_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("OPENCLAW_CONFIG_PATH").filter(|v| !v.is_empty()) {
        return Ok(p.into());
    }
    Ok(state_dir()?.join("openclaw.json"))
}
pub(super) fn managed_root() -> PathBuf {
    crate::paths::bin_dir().join("modules/openclaw")
}
fn bin_name() -> &'static str {
    if cfg!(windows) {
        "openclaw.cmd"
    } else {
        "openclaw"
    }
}
pub(super) fn managed_executable() -> Option<PathBuf> {
    // `npm install --global --prefix <root>`: shims sit in the prefix on Windows, in bin/ elsewhere.
    let root = managed_root();
    let p = if cfg!(windows) {
        root.join(bin_name())
    } else {
        root.join("bin").join(bin_name())
    };
    p.is_file().then_some(p)
}
pub(super) fn external_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(bin_name()));
            if cfg!(windows) {
                candidates.push(dir.join("openclaw.exe"));
            }
        }
    }
    #[cfg(windows)]
    if let Some(dir) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(dir).join("npm").join(bin_name()));
    }
    if let Some(dir) = home_dir() {
        candidates.push(dir.join(".local/bin/openclaw"));
        candidates.push(dir.join(".npm-global/bin/openclaw"));
    }
    candidates.into_iter().find(|p| p.is_file())
}
pub(super) fn executable() -> Option<PathBuf> {
    managed_executable().or_else(external_executable)
}

/// The command that runs `exe`. An npm `.cmd` shim is bypassed (node runs the package entry
/// directly) so that no argument ever passes through `cmd.exe`.
pub(super) fn launcher(exe: &Path) -> std::process::Command {
    #[cfg(windows)]
    if exe
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd"))
    {
        if let Some(dir) = exe.parent() {
            // `npm install --global --prefix`: the shim sits next to `node_modules/openclaw`.
            let entry = dir.join("node_modules/openclaw/openclaw.mjs");
            if entry.is_file() {
                let node = Some(dir.join("node.exe"))
                    .filter(|p| p.is_file())
                    .unwrap_or_else(|| PathBuf::from("node"));
                let mut c = super::agents::command(node);
                c.arg(entry);
                return c;
            }
        }
    }
    super::agents::command(exe)
}

/// A canonicalized Windows path without the verbatim `\\?\` prefix, which Node tools do not
/// expect (UNC paths keep it).
pub(super) fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// `(major, minor)` of a `v24.16.0` / `24.16.0` style version line.
fn parse_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}
/// Upstream `engines.node`: `>=24.16.0 <25 || >=26.1.0`.
pub(super) fn node_supported(version: &str) -> bool {
    match parse_version(version) {
        Some((24, minor)) => minor >= 16,
        Some((major, minor)) => major > 26 || major == 26 && minor >= 1,
        None => false,
    }
}
pub(super) fn npm_allows_scripts_flag(version: &str) -> bool {
    parse_version(version).is_some_and(|v| v >= ALLOW_SCRIPTS_NPM)
}

/// The config as a JSON object. OpenClaw reads JSON5; comments and trailing commas are
/// understood here, anything else it accepts is refused rather than guessed at.
pub(super) fn parse_config(bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "agents.config_invalid".to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let parsed = jsonc_parser::parse_to_ast(
        text,
        &jsonc_parser::CollectOptions::default(),
        &jsonc_parser::ParseOptions::default(),
    )
    .map_err(|_| "agents.config_invalid".to_string())?;
    match parsed.value.map(Value::from) {
        None | Some(Value::Null) => Ok(json!({})),
        Some(v) if v.is_object() => Ok(v),
        _ => Err("agents.config_invalid".into()),
    }
}
/// The model reference OpenClaw uses for `model` (provider id + first `/`).
pub(super) fn model_ref(model: &str) -> String {
    format!("{PROVIDER}/{model}")
}
fn provider(base: &str, model: &str, key: &str, context: Option<u32>) -> Value {
    let window = context.unwrap_or(32768).max(4096);
    json!({
        "baseUrl": base,
        "apiKey": key,
        "api": "openai-completions",
        "models": [{
            "id": model,
            "name": model,
            "reasoning": false,
            "input": ["text"],
            "contextWindow": window,
            "maxTokens": (window / 4).clamp(1024, 8192),
        }],
    })
}
fn object<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    if !value[key].is_object() {
        value[key] = json!({});
    }
    &mut value[key]
}
pub(super) fn connected_config(
    old: Option<&[u8]>,
    base: &str,
    model: &str,
    key: &str,
    context: Option<u32>,
) -> Result<Vec<u8>, String> {
    let mut value = old.map(parse_config).transpose()?.unwrap_or(json!({}));
    let models = object(&mut value, "models");
    object(models, "providers")[PROVIDER] = provider(base, model, key, context);
    let defaults = object(object(&mut value, "agents"), "defaults");
    // Upstream accepts a plain string here; an object keeps the other keys (fallbacks).
    if defaults["model"].is_string() {
        defaults["model"] = json!({});
    }
    object(defaults, "model")["primary"] = model_ref(model).into();
    serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())
}

/// Configuration of one isolated task: only the Harness provider, no channels, plugins, hooks
/// or MCP servers of the user's setup. A read grant allows the `read` tool inside that single
/// folder (the agent workspace); without a grant no tool exists at all.
pub(super) fn task_config(
    provider_config: &Value,
    model: &str,
    workspace: &Path,
    read: bool,
) -> Value {
    let mut tools = json!({"deny": ["*"]});
    if read {
        tools = json!({
            "allow": ["read"],
            "deny": ["exec", "process", "write", "edit", "apply_patch", "browser", "web_search", "web_fetch", "message", "gateway", "cron", "sessions_spawn"],
            "fs": {"workspaceOnly": true},
        });
    }
    json!({
        "models": {"mode": "replace", "providers": {PROVIDER: provider_config}},
        "agents": {"defaults": {
            "model": {"primary": model_ref(model)},
            "workspace": workspace.to_string_lossy(),
        }},
        "tools": tools,
    })
}

/// `openclaw agent exec` for one task; the caller adds the environment.
pub(super) fn task_command(
    exe: &Path,
    config: &Path,
    state: &Path,
    cwd: &Path,
    prompt_file: &Path,
    model: &str,
    timeout_secs: u64,
) -> std::process::Command {
    let mut c = launcher(exe);
    c.args(["agent", "exec", "--config"])
        .arg(config)
        .arg("--state-dir")
        .arg(state)
        .arg("--cwd")
        .arg(cwd)
        .arg("--message-file")
        .arg(prompt_file)
        .arg("--model")
        .arg(model_ref(model))
        .args(["--timeout", &timeout_secs.to_string()]);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_windows_prefix_is_removed_only_for_drive_paths() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\C:\work\folder")),
            PathBuf::from(r"C:\work\folder")
        );
        let unc = PathBuf::from(r"\\?\UNC\server\share");
        assert_eq!(plain_path(unc.clone()), unc);
        assert_eq!(plain_path(PathBuf::from("/tmp/x")), PathBuf::from("/tmp/x"));
    }

    #[test]
    fn node_and_npm_versions_follow_the_upstream_requirements() {
        for ok in ["v24.16.0", "24.17.2", "v26.1.0", "26.11.1", "v27.0.0"] {
            assert!(node_supported(ok), "{ok}");
        }
        for bad in ["v24.11.1", "v22.12.0", "v25.3.0", "v26.0.9", "garbage", ""] {
            assert!(!node_supported(bad), "{bad}");
        }
        assert!(npm_allows_scripts_flag("11.20.0"));
        assert!(npm_allows_scripts_flag("12.0.1"));
        assert!(!npm_allows_scripts_flag("11.15.9"));
    }

    #[test]
    fn config_keeps_user_settings_and_registers_the_provider() {
        let old = br#"{
            // comments and trailing commas are fine
            channels: undefined_is_not_json,
        }"#;
        assert_eq!(
            connected_config(Some(old), "http://127.0.0.1:1/v1", "m", "k", None).unwrap_err(),
            "agents.config_invalid"
        );
        let old = br#"{
            // comments and trailing commas are fine
            "channels": {"whatsapp": {"allowFrom": ["+1"]}},
            "agents": {"defaults": {"model": "old/model", "workspace": "~/w"}},
        }"#;
        let bytes = connected_config(
            Some(old),
            "http://127.0.0.1:1/v1",
            "chat",
            "key",
            Some(8192),
        )
        .unwrap();
        let cfg = parse_config(&bytes).unwrap();
        assert_eq!(cfg["channels"]["whatsapp"]["allowFrom"][0], "+1");
        assert_eq!(cfg["agents"]["defaults"]["workspace"], "~/w");
        assert_eq!(
            cfg["agents"]["defaults"]["model"]["primary"],
            "harness/chat"
        );
        let p = &cfg["models"]["providers"]["harness"];
        assert_eq!(p["baseUrl"], "http://127.0.0.1:1/v1");
        assert_eq!(p["apiKey"], "key");
        assert_eq!(p["api"], "openai-completions");
        assert_eq!(p["models"][0]["id"], "chat");
        assert_eq!(p["models"][0]["contextWindow"], 8192);
        // An absent or empty file is a valid starting point; a non-object is not.
        assert!(connected_config(None, "http://127.0.0.1:1/v1", "m", "k", None).is_ok());
        assert!(connected_config(Some(b"  \n"), "http://127.0.0.1:1/v1", "m", "k", None).is_ok());
        assert!(connected_config(Some(b"[1]"), "http://127.0.0.1:1/v1", "m", "k", None).is_err());
        assert!(
            connected_config(Some(b"{broken"), "http://127.0.0.1:1/v1", "m", "k", None).is_err()
        );
    }

    #[test]
    fn link_models_keep_their_full_id_in_the_model_reference() {
        let bytes = connected_config(
            None,
            "http://127.0.0.1:1/v1",
            "remote:lab/qwen3:8b",
            "k",
            None,
        )
        .unwrap();
        let cfg = parse_config(&bytes).unwrap();
        assert_eq!(
            cfg["agents"]["defaults"]["model"]["primary"],
            "harness/remote:lab/qwen3:8b"
        );
        assert_eq!(
            cfg["models"]["providers"]["harness"]["models"][0]["id"],
            "remote:lab/qwen3:8b"
        );
    }

    #[test]
    fn task_config_denies_tools_unless_a_folder_is_granted() {
        let provider = provider("http://127.0.0.1:1/v1", "m", "k", None);
        let tmp = tempfile::tempdir().unwrap();
        let none = task_config(&provider, "m", tmp.path(), false);
        assert_eq!(none["tools"]["deny"][0], "*");
        assert!(none["tools"].get("allow").is_none());
        assert_eq!(none["models"]["mode"], "replace");
        assert!(none.get("channels").is_none() && none.get("plugins").is_none());
        let read = task_config(&provider, "m", tmp.path(), true);
        assert_eq!(read["tools"]["allow"], json!(["read"]));
        assert_eq!(read["tools"]["fs"]["workspaceOnly"], true);
        let denied = read["tools"]["deny"].as_array().unwrap();
        assert!(["exec", "write", "edit", "apply_patch"]
            .iter()
            .all(|t| denied.iter().any(|d| d == t)));
    }
}
