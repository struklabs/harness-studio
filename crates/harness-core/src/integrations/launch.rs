//! `harness launch`: start a coding agent against a Harness endpoint with
//! process-only settings. Nothing in the user's global configuration is written.
//!
//! | Tool        | How it is pointed at Harness                                                   |
//! |-------------|--------------------------------------------------------------------------------|
//! | Claude Code | env (`ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, model vars) + a private temp settings file passed with `--settings` (its `env` block outranks `~/.claude/settings.json`, which would otherwise override process env) |
//! | Codex       | `-c` overrides: `model_provider=harness`, `model_providers.harness.*` (Responses API), `-m <alias>`, `model_context_window` |
//! | OpenCode    | `OPENCODE_CONFIG_CONTENT` (inline config, merged after the user's files)       |
//! | Aider       | `--model openai/<alias>`, `--openai-api-base`, `OPENAI_API_KEY`, a temp `--model-metadata-file` with the context size |
//! | Goose       | env (`GOOSE_PROVIDER=openai`, `GOOSE_MODEL`, `OPENAI_HOST`, `OPENAI_BASE_PATH`, `OPENAI_API_KEY`, `GOOSE_CONTEXT_LIMIT`) |
//!
//! Secrets (the gateway client token or a named token) travel only in the child's
//! environment or in a temp file readable only by the user, and are redacted in every printed
//! plan. Sources for each format: `docs/en/INTEGRATIONS.md`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::error_key::err;

/// A key the gateway accepts when it needs no token (loopback, token not required): tools
/// that insist on a non-empty key get this.
pub const PLACEHOLDER_KEY: &str = "harness-local";

/// Env var carrying the token for tools that read it by name (OpenCode `{env:...}`).
pub const TOKEN_ENV: &str = "HARNESS_API_KEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchTool {
    Claude,
    Codex,
    OpenCode,
    Aider,
    Goose,
}

/// Which API a tool speaks (also used by "test connection").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    /// Anthropic Messages (`/v1/messages`).
    Anthropic,
    /// OpenAI Responses (`/v1/responses`).
    Responses,
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    Chat,
}

impl LaunchTool {
    pub const ALL: [LaunchTool; 5] = [
        LaunchTool::Claude,
        LaunchTool::Codex,
        LaunchTool::OpenCode,
        LaunchTool::Aider,
        LaunchTool::Goose,
    ];

    pub fn id(self) -> &'static str {
        match self {
            LaunchTool::Claude => "claude",
            LaunchTool::Codex => "codex",
            LaunchTool::OpenCode => "opencode",
            LaunchTool::Aider => "aider",
            LaunchTool::Goose => "goose",
        }
    }

    pub fn parse(s: &str) -> Option<LaunchTool> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(LaunchTool::Claude),
            "codex" => Some(LaunchTool::Codex),
            "opencode" => Some(LaunchTool::OpenCode),
            "aider" => Some(LaunchTool::Aider),
            "goose" => Some(LaunchTool::Goose),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LaunchTool::Claude => "Claude Code",
            LaunchTool::Codex => "Codex",
            LaunchTool::OpenCode => "OpenCode",
            LaunchTool::Aider => "Aider",
            LaunchTool::Goose => "Goose",
        }
    }

    /// The executable name looked up on PATH.
    pub fn program(self) -> &'static str {
        self.id()
    }

    /// The tool's executable: `program()` on PATH, else (OpenCode) the full CLI a desktop build
    /// ships (`opencode-cli`, see [`super::opencode_desktop`]).
    pub fn find_executable(self) -> Option<PathBuf> {
        find_program(self.program()).or_else(|| {
            let roots = super::Roots::from_env();
            match self {
                LaunchTool::OpenCode => super::opencode_desktop::bundled_cli(&roots),
                LaunchTool::Claude => bundled_claude_code(&roots),
                LaunchTool::Codex => bundled_codex(),
                _ => None,
            }
        })
    }

    pub fn wire_api(self) -> WireApi {
        match self {
            LaunchTool::Claude => WireApi::Anthropic,
            LaunchTool::Codex => WireApi::Responses,
            _ => WireApi::Chat,
        }
    }

    /// Smallest per-request context the tool works with. Agent tools send their system prompt
    /// and tool schemas with every request: Claude Code ~20k tokens, Codex ~12k, OpenCode ~29k
    /// (measured for P0/OpenCode); Aider's repo map is small.
    pub fn min_ctx(self) -> u32 {
        match self {
            LaunchTool::Claude => 32_768,
            LaunchTool::Codex => 32_768,
            LaunchTool::OpenCode => 49_152,
            LaunchTool::Aider => 8_192,
            LaunchTool::Goose => 32_768,
        }
    }

    /// Context "Prepare the server" sets (room for a few turns before compaction).
    pub fn recommended_ctx(self) -> u32 {
        match self {
            LaunchTool::Aider => 32_768,
            _ => 65_536,
        }
    }
}

/// Is the context big enough?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum CtxVerdict {
    Ok,
    /// Below the tool's minimum.
    TooSmall {
        have: u32,
        min: u32,
        recommended: u32,
    },
    /// Not known yet (the model is not loaded and its definition sets no context).
    Unknown {
        min: u32,
        recommended: u32,
    },
}

pub fn check_context(tool: LaunchTool, ctx: Option<u32>) -> CtxVerdict {
    let (min, recommended) = (tool.min_ctx(), tool.recommended_ctx());
    match ctx {
        Some(c) if c >= min => CtxVerdict::Ok,
        Some(c) => CtxVerdict::TooSmall {
            have: c,
            min,
            recommended,
        },
        None => CtxVerdict::Unknown { min, recommended },
    }
}

/// Load values "Prepare the server" writes into the model definition: the context, flash
/// attention (long prompts are several times slower without it), a q8_0 KV cache (practically
/// lossless, half the memory of f16) and the automatic concurrent-request count (`slots`: the
/// gateway loads the definition for agents, every concurrent request keeps an agent-sized share
/// of the context).
/// The fixed fallback recommendation, used when the machine's memory cannot be planned for (a
/// remote instance); otherwise [`crate::prepare::plan`] picks the values.
pub fn prepare_values(tool: LaunchTool, current_ctx: Option<u32>) -> Value {
    let ctx = current_ctx.unwrap_or(0).max(tool.recommended_ctx());
    json!({
        "context_size": ctx,
        "flash_attn": true,
        "cache_type_k": "q8_0",
        "cache_type_v": "q8_0",
        "parallel_slots": null,
    })
}

/// Where the tool connects.
#[derive(Debug, Clone)]
pub struct LaunchInput {
    pub tool: LaunchTool,
    /// Keep OpenCode external skills out of its prompt (child process only).
    pub hide_external_skills: bool,
    /// `http://host:port` (no path).
    pub host_url: String,
    /// Definition alias (the gateway routes it; a direct server serves it).
    pub alias: String,
    /// Gateway client token, a named `inference` token or a llama-server key. Secret.
    pub token: Option<String>,
    /// Per-request context the model has (for tools that size their prompts).
    pub context: Option<u32>,
    /// The managed endpoint (gateway) rather than a llama-server port.
    pub managed: bool,
    /// Arguments for the tool, appended (a leading subcommand like `exec`/`run` stays first).
    pub extra_args: Vec<String>,
    /// Directory for temp files (created by the caller, removed after the tool exits).
    pub temp_dir: PathBuf,
}

/// A file written for the duration of the launch.
#[derive(Debug, Clone, Serialize)]
pub struct TempFile {
    pub path: PathBuf,
    #[serde(skip)]
    pub content: String,
    /// Contains a secret (never printed).
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchPlan {
    pub tool: LaunchTool,
    pub program: String,
    pub args: Vec<String>,
    /// Set in the child's environment.
    #[serde(skip)]
    pub env: Vec<(String, String)>,
    /// Removed from the child's environment (inherited values that would override ours).
    pub env_remove: Vec<String>,
    /// Names in `env` whose values are secret.
    pub secret_env: Vec<String>,
    pub files: Vec<TempFile>,
    pub base_url: String,
    pub model: String,
    pub managed: bool,
}

/// Both flags are needed: OpenCode has separate external and Claude skill discovery paths.
pub const OPENCODE_SKILL_ENV: [&str; 2] = [
    "OPENCODE_DISABLE_EXTERNAL_SKILLS",
    "OPENCODE_DISABLE_CLAUDE_CODE_SKILLS",
];

pub fn opencode_skill_env() -> Vec<(String, String)> {
    OPENCODE_SKILL_ENV
        .iter()
        .map(|key| ((*key).into(), "1".into()))
        .collect()
}

/// Apply only to the command being launched; never mutate Harness's environment.
pub fn apply_opencode_skill_env(cmd: &mut std::process::Command, hide: bool) {
    for key in OPENCODE_SKILL_ENV {
        if hide {
            cmd.env(key, "1");
        } else {
            cmd.env_remove(key);
        }
    }
}

const REDACTED: &str = "***";

impl LaunchPlan {
    /// The plan as printed by `--dry-run` / shown in the GUI: secrets replaced by `***`.
    pub fn redacted(&self) -> Value {
        let env: serde_json::Map<String, Value> = self
            .env
            .iter()
            .map(|(k, v)| {
                let shown = if self.secret_env.contains(k) {
                    REDACTED.to_string()
                } else {
                    redact_text(v, self.secret_values())
                };
                (k.clone(), Value::String(shown))
            })
            .collect();
        let files: Vec<Value> = self
            .files
            .iter()
            .map(|f| {
                json!({
                    "path": f.path,
                    "content": redact_text(&f.content, self.secret_values()),
                })
            })
            .collect();
        json!({
            "tool": self.tool,
            "program": self.program,
            "args": self.args.iter().map(|a| redact_text(a, self.secret_values())).collect::<Vec<_>>(),
            "env": env,
            "env_remove": self.env_remove,
            "files": files,
            "base_url": self.base_url,
            "model": self.model,
            "managed": self.managed,
        })
    }

    fn secret_values(&self) -> Vec<&str> {
        self.env
            .iter()
            .filter(|(k, v)| self.secret_env.contains(k) && v.as_str() != PLACEHOLDER_KEY)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// Writes the temp files (readable only by the user).
    pub fn write_files(&self) -> Result<(), String> {
        for f in &self.files {
            if let Some(dir) = f.path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| {
                    err("fs.create_dir_failed")
                        .with("path", dir.display())
                        .with("error", e)
                })?;
            }
            crate::instance::write_private_file(&f.path, f.content.as_bytes())?;
        }
        Ok(())
    }

    /// The child process (program resolved on PATH, or the tool's bundled CLI; env
    /// set/removed; args).
    pub fn command(&self) -> std::process::Command {
        let found = if self.program == self.tool.program() {
            self.tool.find_executable()
        } else {
            find_program(&self.program)
        };
        let program = found
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| self.program.clone());
        let mut cmd = std::process::Command::new(program);
        cmd.args(&self.args);
        apply_augmented_path(&mut cmd);
        for k in &self.env_remove {
            cmd.env_remove(k);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
}

fn redact_text(text: &str, secrets: Vec<&str>) -> String {
    let mut out = text.to_string();
    for s in secrets {
        if s.len() >= 8 {
            out = out.replace(s, REDACTED);
        }
    }
    out
}

/// The executable for `name` on PATH (Windows: `.exe`, `.cmd`, `.bat`, `.com`).
/// Claude Code outside PATH: the native installer's `~/.local/bin/claude` (a GUI app often
/// starts without it on PATH), the old npm local install `~/.claude/local/claude`, and the copy
/// the Claude desktop app keeps in `<config>/Claude/claude-code/<version>/claude(.exe)`
/// (newest version first; seen on Linux as `~/.config/Claude/claude-code/2.1.284/claude`).
pub fn bundled_claude_code(roots: &super::Roots) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    let mut candidates = Vec::new();
    if let Some(home) = &roots.home {
        candidates.push(home.join(".local").join("bin").join(exe));
        candidates.push(home.join(".claude").join("local").join(exe));
    }
    if let Some(appdata) = &roots.appdata {
        candidates.extend(
            newest_versions(&appdata.join("Claude").join("claude-code")).map(|d| d.join(exe)),
        );
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Codex outside PATH: the CLI the ChatGPT desktop app ships (Linux package: seen as
/// `/usr/lib/chatgpt/resources/codex`, codex-cli 0.159.2). Other platforms: PATH only.
pub fn bundled_codex() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    [
        "/usr/lib/chatgpt/resources/codex",
        "/opt/ChatGPT/resources/codex",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

/// Subdirectories named like versions (`2.1.284`), newest first.
fn newest_versions(dir: &Path) -> impl Iterator<Item = PathBuf> {
    let key = |name: &str| -> Option<Vec<u64>> {
        name.split('.').map(|p| p.parse::<u64>().ok()).collect()
    };
    let mut dirs: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((key(&e.file_name().to_string_lossy())?, e.path())))
        .collect();
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.into_iter().map(|(_, p)| p)
}

pub fn find_program(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = search_path()?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{}{}", name, ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Some(found) = find_on_login_shell_path(name) {
        return Some(found);
    }
    if cfg!(unix) {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        return user_bin_dirs(&home)
            .into_iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file());
    }
    None
}

/// Program folders a macOS app started from Finder/Dock lacks: launchd gives it only
/// `/usr/bin:/bin:/usr/sbin:/sbin`, while Homebrew (`/opt/homebrew`, `/usr/local`) and the
/// Node installer (`/usr/local/bin`: `node`, `npx`) put programs elsewhere.
#[cfg(target_os = "macos")]
const MACOS_EXTRA_BIN_DIRS: &[&str] =
    &["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"];

/// `path` with the `extra` folders it lacks appended (the user's own order keeps priority);
/// `None` when nothing is added.
pub fn append_path_dirs(path: Option<&OsStr>, extra: &[PathBuf]) -> Option<OsString> {
    let mut dirs: Vec<PathBuf> = path
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    let before = dirs.len();
    for d in extra {
        if !dirs.contains(d) {
            dirs.push(d.clone());
        }
    }
    if dirs.len() == before {
        return None;
    }
    std::env::join_paths(dirs).ok()
}

/// Opens the login shell's `PATH` in its output (profile scripts may print other text).
const PATH_START: &str = "__HS_PATH__";
/// Closes it.
const PATH_END: &str = "__HS_END__";

/// The `PATH` between [`PATH_START`] and the next [`PATH_END`] in `output`, split on `:`,
/// keeping only absolute entries (first occurrence wins). Empty without both markers.
pub fn parse_marked_path(output: &str) -> Vec<PathBuf> {
    let Some((_, rest)) = output.split_once(PATH_START) else {
        return Vec::new();
    };
    let Some((path, _)) = rest.split_once(PATH_END) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    // Always a Unix shell's PATH: a string check, so the parser (and its test) behave the same
    // on every platform (`Path::is_absolute` needs a drive letter on Windows).
    for entry in path.split(':') {
        if !entry.starts_with('/') {
            continue;
        }
        let dir = PathBuf::from(entry);
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The user's login shell: `$SHELL` when it is an absolute path to an executable file;
/// otherwise none (no guessing).
#[cfg(target_os = "macos")]
fn login_shell() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let shell = PathBuf::from(std::env::var_os("SHELL")?);
    let executable = std::fs::metadata(&shell)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false);
    (shell.is_absolute() && executable).then_some(shell)
}

/// Runs `$SHELL -l -c` (login, not interactive: no prompt, no TTY, stdin null) to read the
/// `PATH` the user's profile (`/etc/zprofile`, `~/.zprofile`...) builds. The shell runs in its
/// own process group, which is killed after 3 s, so a hung profile cannot hang the app.
/// Existing directories only.
#[cfg(target_os = "macos")]
fn query_login_shell_path() -> Vec<PathBuf> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(3);
    let Some(shell) = login_shell() else {
        return Vec::new();
    };
    let script = format!("printf '{PATH_START}%s{PATH_END}' \"$PATH\"");
    let Ok(mut child) = Command::new(&shell)
        .args(["-l", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    else {
        return Vec::new();
    };
    let kill_group = |pid: u32| {
        // SAFETY: plain syscall; a negative pid addresses the group the child leads.
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    };
    // Read on a separate thread: a chatty profile must not block on a full pipe, and a
    // background process inheriting stdout must not hang us after the shell exits.
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                kill_group(child.id());
                let _ = child.wait();
                return Vec::new();
            }
        }
    }
    let wait = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(100));
    let Ok(buf) = rx.recv_timeout(wait) else {
        return Vec::new();
    };
    parse_marked_path(&String::from_utf8_lossy(&buf))
        .into_iter()
        .filter(|d| d.is_dir())
        .collect()
}

#[cfg(target_os = "macos")]
static LOGIN_SHELL_PATH: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();

/// The login shell's `PATH` folders, computed on first call and kept for the process lifetime.
#[cfg(target_os = "macos")]
fn login_shell_path() -> &'static [PathBuf] {
    LOGIN_SHELL_PATH.get_or_init(query_login_shell_path)
}

/// The login shell's `PATH` folders if a lookup already computed them (never runs the shell).
#[cfg(target_os = "macos")]
fn login_shell_path_if_known() -> &'static [PathBuf] {
    LOGIN_SHELL_PATH.get().map(Vec::as_slice).unwrap_or(&[])
}

/// The login shell's `PATH` folders, asked now rather than cached (a profile may just have
/// changed). Empty on other systems or when the shell does not answer.
pub fn login_shell_path_now() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        query_login_shell_path()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// After a lookup on [`search_path`] missed: macOS asks the login shell for its `PATH` (once
/// per process) and returns `file_name` in its folders. `None` elsewhere.
fn find_on_login_shell_path(file_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        login_shell_path()
            .iter()
            .map(|d| d.join(file_name))
            .find(|p| p.is_file())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = file_name;
        None
    }
}

/// `file_name` (exact, no `PATHEXT`) in a folder of [`search_path`]; on a miss, macOS also
/// tries the login shell's `PATH` (see [`find_on_login_shell_path`]).
pub fn find_file_on_path(file_name: &str) -> Option<PathBuf> {
    search_path()
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(file_name))
                .find(|p| p.is_file())
        })
        .or_else(|| find_on_login_shell_path(file_name))
}

/// macOS: this process's `PATH`, then the folders it lacks from the login shell's `PATH` (only
/// if a lookup already computed it: spawning a child never runs the shell), the existing
/// per-user folders (`~/.local/bin`, `~/.cargo/bin`, `~/.bun/bin`, `~/.volta/bin`) and
/// [`MACOS_EXTRA_BIN_DIRS`]. `None` when nothing is missing, and always on other systems
/// (their PATH is left as is).
pub fn augmented_path() -> Option<OsString> {
    #[cfg(target_os = "macos")]
    {
        let mut extra: Vec<PathBuf> = login_shell_path_if_known().to_vec();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            extra.extend(
                [".local/bin", ".cargo/bin", ".bun/bin", ".volta/bin"]
                    .iter()
                    .map(|d| home.join(d))
                    .filter(|d| d.is_dir()),
            );
        }
        extra.extend(
            MACOS_EXTRA_BIN_DIRS
                .iter()
                .map(PathBuf::from)
                .filter(|d| d.is_dir()),
        );
        append_path_dirs(std::env::var_os("PATH").as_deref(), &extra)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The `PATH` to search programs on: [`augmented_path`], else this process's `PATH`.
pub fn search_path() -> Option<OsString> {
    augmented_path().or_else(|| std::env::var_os("PATH"))
}

/// Gives a child the [`augmented_path`] (no-op when there is none). Call before setting the
/// child's own env, so an explicit `PATH` there still wins.
pub fn apply_augmented_path(cmd: &mut std::process::Command) {
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
}

/// Per-user program folders a desktop-started app usually lacks on PATH (the login shell's
/// profile adds them): `~/.local/bin`, installer folders (`~/.opencode/bin`, bun, volta,
/// npm's `~/.npm-global`), and the newest Node of nvm / fnm, where `npm i -g` puts
/// `opencode`, `codex`, `claude`...
pub fn user_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        ".local/bin",
        ".opencode/bin",
        ".bun/bin",
        ".volta/bin",
        ".npm-global/bin",
        "bin",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect();
    let node = |versions: PathBuf, tail: &str| {
        let key = |name: &str| -> Option<Vec<u64>> {
            name.trim_start_matches('v')
                .split('.')
                .map(|p| p.parse::<u64>().ok())
                .collect()
        };
        let mut found: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(&versions)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Some((key(&e.file_name().to_string_lossy())?, e.path().join(tail))))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        found.into_iter().map(|(_, p)| p).collect::<Vec<_>>()
    };
    let nvm_dir = std::env::var_os("NVM_DIR").map(PathBuf::from);
    for root in [
        nvm_dir,
        Some(home.join(".nvm")),
        Some(home.join(".config/nvm")),
    ]
    .into_iter()
    .flatten()
    {
        dirs.extend(node(root.join("versions/node"), "bin"));
    }
    dirs.extend(node(
        home.join(".local/share/fnm/node-versions"),
        "installation/bin",
    ));
    dirs.dedup();
    dirs
}

/// Output tokens to allow: a quarter of the context, 1024..=8192.
fn output_tokens(ctx: Option<u32>) -> u32 {
    ctx.map(|c| (c / 4).clamp(1024, 8192)).unwrap_or(8192)
}

/// Our args after a leading subcommand of `extra` (`exec`, `run`...), then the rest.
fn with_extra(ours: Vec<String>, extra: &[String]) -> Vec<String> {
    match extra.first() {
        Some(first) if !first.starts_with('-') => {
            let mut v = vec![first.clone()];
            v.extend(ours);
            v.extend(extra[1..].iter().cloned());
            v
        }
        _ => {
            let mut v = ours;
            v.extend(extra.iter().cloned());
            v
        }
    }
}

/// Environment variables for Claude Code (also written by the persistent integration into
/// `settings.json` `env`). Values are strings, as Claude Code expects.
pub fn claude_env(
    host_url: &str,
    alias: &str,
    token: &str,
    ctx: Option<u32>,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("ANTHROPIC_BASE_URL", host_url.to_string()),
        ("ANTHROPIC_AUTH_TOKEN", token.to_string()),
        ("ANTHROPIC_MODEL", alias.to_string()),
        ("ANTHROPIC_DEFAULT_OPUS_MODEL", alias.to_string()),
        ("ANTHROPIC_DEFAULT_SONNET_MODEL", alias.to_string()),
        ("ANTHROPIC_DEFAULT_HAIKU_MODEL", alias.to_string()),
        ("CLAUDE_CODE_SUBAGENT_MODEL", alias.to_string()),
        (
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
            output_tokens(ctx).to_string(),
        ),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".to_string()),
        ("CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS", "1".to_string()),
        (
            "ANTHROPIC_CUSTOM_HEADERS",
            "X-Harness-Client: claude-code".to_string(),
        ),
    ];
    if let Some(c) = ctx {
        env.push(("CLAUDE_CODE_MAX_CONTEXT_TOKENS", c.to_string()));
    }
    env.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Claude Code settings inherited from the user's shell that would send requests elsewhere.
pub const CLAUDE_ENV_REMOVE: [&str; 6] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ANTHROPIC_BEDROCK_BASE_URL",
];

/// The OpenCode provider block (also the persistent integration's): every `(alias, context)`
/// in `models` is listed, so the user can switch between them inside OpenCode.
pub fn opencode_provider(base_url: &str, models: &[(String, u32)], api_key: &str) -> Value {
    let list: serde_json::Map<String, Value> = models
        .iter()
        .map(|(alias, ctx)| {
            (
                alias.clone(),
                json!({
                    "name": format!("{} (Harness Studio)", alias),
                    "tool_call": true,
                    "limit": { "context": ctx, "output": output_tokens(Some(*ctx)) }
                }),
            )
        })
        .collect();
    json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": "Harness Studio",
        "options": {
            "baseURL": base_url,
            "apiKey": api_key,
            "headers": { "X-Harness-Client": "opencode" }
        },
        "models": list
    })
}

pub fn plan(input: &LaunchInput) -> LaunchPlan {
    let base_url = format!("{}/v1", input.host_url.trim_end_matches('/'));
    let key = input
        .token
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| PLACEHOLDER_KEY.to_string());
    let alias = input.alias.as_str();
    let mut env: Vec<(String, String)> = Vec::new();
    let mut env_remove: Vec<String> = Vec::new();
    let mut secret_env: Vec<String> = Vec::new();
    let mut files = Vec::new();
    let args: Vec<String>;
    match input.tool {
        LaunchTool::Claude => {
            env = claude_env(&input.host_url, alias, &key, input.context);
            secret_env.push("ANTHROPIC_AUTH_TOKEN".into());
            env_remove = CLAUDE_ENV_REMOVE.iter().map(|s| s.to_string()).collect();
            // `~/.claude/settings.json` `env` beats process env; `--settings` beats it.
            let settings_path = input.temp_dir.join("claude-settings.json");
            let block: serde_json::Map<String, Value> = env
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            files.push(TempFile {
                path: settings_path.clone(),
                content: serde_json::to_string_pretty(&json!({ "env": block })).unwrap_or_default(),
                secret: true,
            });
            args = with_extra(
                vec![
                    "--settings".into(),
                    settings_path.to_string_lossy().to_string(),
                ],
                &input.extra_args,
            );
        }
        LaunchTool::Codex => {
            let mut ours = vec![
                "-c".to_string(),
                "model_provider=harness".to_string(),
                "-c".to_string(),
                "model_providers.harness.name=Harness Studio".to_string(),
                "-c".to_string(),
                format!("model_providers.harness.base_url={}", base_url),
                "-c".to_string(),
                "model_providers.harness.wire_api=responses".to_string(),
                // Codex's hosted web search cannot run on llama.cpp; the gateway rejects it
                // (never drops it silently), so Codex is told not to offer it.
                "-c".to_string(),
                "web_search=disabled".to_string(),
            ];
            if input.token.is_some() {
                ours.push("-c".into());
                ours.push(format!("model_providers.harness.env_key={}", TOKEN_ENV));
                env.push((TOKEN_ENV.into(), key.clone()));
                secret_env.push(TOKEN_ENV.into());
            }
            if let Some(c) = input.context {
                ours.push("-c".into());
                ours.push(format!("model_context_window={}", c));
            }
            ours.push("-m".into());
            ours.push(alias.to_string());
            args = with_extra(ours, &input.extra_args);
        }
        LaunchTool::OpenCode => {
            if input.hide_external_skills {
                env.extend(opencode_skill_env());
            } else {
                env_remove.extend(OPENCODE_SKILL_ENV.iter().map(|k| (*k).to_string()));
            }
            let ctx = input.context.unwrap_or(32_768);
            let content = json!({
                "$schema": "https://opencode.ai/config.json",
                "provider": {
                    "harness": opencode_provider(&base_url, &[(alias.to_string(), ctx)], &format!("{{env:{}}}", TOKEN_ENV))
                },
                "model": format!("harness/{}", alias),
            });
            env.push(("OPENCODE_CONFIG_CONTENT".into(), content.to_string()));
            env.push((TOKEN_ENV.into(), key.clone()));
            secret_env.push(TOKEN_ENV.into());
            args = with_extra(Vec::new(), &input.extra_args);
        }
        LaunchTool::Aider => {
            let model = format!("openai/{}", alias);
            let meta_path = input.temp_dir.join("aider-model-metadata.json");
            let out = output_tokens(input.context);
            let mut meta = json!({
                "max_tokens": out,
                "max_output_tokens": out,
                "input_cost_per_token": 0,
                "output_cost_per_token": 0,
                "litellm_provider": "openai",
                "mode": "chat",
            });
            if let Some(c) = input.context {
                meta["max_input_tokens"] = json!(c);
            }
            files.push(TempFile {
                path: meta_path.clone(),
                content: serde_json::to_string_pretty(&json!({ model.clone(): meta }))
                    .unwrap_or_default(),
                secret: false,
            });
            env.push(("OPENAI_API_KEY".into(), key.clone()));
            env.push(("OPENAI_API_BASE".into(), base_url.clone()));
            secret_env.push("OPENAI_API_KEY".into());
            env_remove = vec![
                "AIDER_OPENAI_API_KEY".into(),
                "AIDER_OPENAI_API_BASE".into(),
                "AIDER_MODEL".into(),
            ];
            args = with_extra(
                vec![
                    "--model".into(),
                    model,
                    "--openai-api-base".into(),
                    base_url.clone(),
                    "--model-metadata-file".into(),
                    meta_path.to_string_lossy().to_string(),
                    "--no-show-model-warnings".into(),
                ],
                &input.extra_args,
            );
        }
        LaunchTool::Goose => {
            env.push(("GOOSE_PROVIDER".into(), "openai".into()));
            env.push(("GOOSE_MODEL".into(), alias.to_string()));
            env.push((
                "OPENAI_HOST".into(),
                input.host_url.trim_end_matches('/').to_string(),
            ));
            env.push(("OPENAI_BASE_PATH".into(), "v1/chat/completions".into()));
            env.push(("OPENAI_API_KEY".into(), key.clone()));
            env.push((
                "OPENAI_CUSTOM_HEADERS".into(),
                "X-Harness-Client=goose".into(),
            ));
            if let Some(c) = input.context {
                env.push(("GOOSE_CONTEXT_LIMIT".into(), c.to_string()));
            }
            secret_env.push("OPENAI_API_KEY".into());
            args = if input.extra_args.is_empty() {
                vec!["session".into()]
            } else {
                input.extra_args.clone()
            };
        }
    }
    LaunchPlan {
        tool: input.tool,
        program: input.tool.program().to_string(),
        args,
        env,
        env_remove,
        secret_env,
        files,
        base_url,
        model: alias.to_string(),
        managed: input.managed,
    }
}

/// A fresh private temp directory for one launch.
pub fn new_temp_dir() -> Result<PathBuf, String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let suffix: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let dir = std::env::temp_dir().join(format!("harness-launch-{}", suffix));
    std::fs::create_dir_all(&dir).map_err(|e| {
        err("fs.create_dir_failed")
            .with("path", dir.display())
            .with("error", e)
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_path_dirs_keeps_user_order_and_adds_missing_dirs_once() {
        let user = std::env::join_paths(["/usr/local/bin", "/usr/bin", "/bin"]).unwrap();
        let extra = [
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
        ];
        let out = append_path_dirs(Some(&user), &extra).unwrap();
        let dirs: Vec<PathBuf> = std::env::split_paths(&out).collect();
        assert_eq!(
            dirs,
            ["/usr/local/bin", "/usr/bin", "/bin", "/opt/homebrew/bin"]
                .map(PathBuf::from)
                .to_vec()
        );
        // Nothing missing: no change.
        assert_eq!(append_path_dirs(Some(&out), &extra), None);
        assert_eq!(append_path_dirs(Some(&user), &[]), None);
        // No PATH at all: just the extras.
        let only = append_path_dirs(None, &extra[..1]).unwrap();
        assert_eq!(
            std::env::split_paths(&only).collect::<Vec<_>>(),
            vec![PathBuf::from("/opt/homebrew/bin")]
        );
    }

    #[test]
    fn parse_marked_path_ignores_profile_noise() {
        let out = "Welcome!\nlast login...__HS_PATH__/Users/a/.local/node/bin:/usr/bin:\
                   relative/bin::/usr/bin:~/bin:/bin__HS_END__trailing __HS_END__ noise";
        assert_eq!(
            parse_marked_path(out),
            ["/Users/a/.local/node/bin", "/usr/bin", "/bin"]
                .map(PathBuf::from)
                .to_vec()
        );
        // Missing or incomplete marker: nothing.
        assert!(parse_marked_path("/usr/bin:/bin").is_empty());
        assert!(parse_marked_path("__HS_PATH__/usr/bin:/bin").is_empty());
        assert!(parse_marked_path("/usr/bin:/bin__HS_END__").is_empty());
        assert!(parse_marked_path("").is_empty());
        assert!(parse_marked_path("__HS_PATH____HS_END__").is_empty());
    }

    /// Manual check: `env -i HOME=$HOME SHELL=/bin/zsh PATH=/usr/bin:/bin:/usr/sbin:/sbin
    /// cargo test -p harness-core --lib login_shell_path_reaches_profile_dirs -- --ignored
    /// --nocapture` (Finder-like PATH; the login shell's profile dirs must show up).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn login_shell_path_reaches_profile_dirs() {
        // Spawning alone never runs the shell.
        assert!(login_shell_path_if_known().is_empty());
        let start = std::time::Instant::now();
        let opencode = find_program("opencode");
        eprintln!("opencode: {opencode:?} ({:?})", start.elapsed());
        let dirs = login_shell_path_if_known();
        eprintln!("login shell PATH: {dirs:?}");
        let path = augmented_path().unwrap_or_default();
        eprintln!("augmented PATH: {}", path.to_string_lossy());
        assert!(!dirs.is_empty());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn augmented_path_is_macos_only() {
        assert_eq!(augmented_path(), None);
        assert_eq!(search_path(), std::env::var_os("PATH"));
    }

    #[test]
    fn user_bin_dirs_cover_installers_and_the_newest_node() {
        let d = tempfile::tempdir().unwrap();
        for v in ["v18.20.1", "v22.23.3", "v22.9.0"] {
            std::fs::create_dir_all(
                d.path()
                    .join(".config/nvm/versions/node")
                    .join(v)
                    .join("bin"),
            )
            .unwrap();
        }
        let dirs = user_bin_dirs(d.path());
        assert_eq!(dirs[0], d.path().join(".local/bin"));
        assert!(dirs.contains(&d.path().join(".opencode/bin")));
        let nvm: Vec<_> = dirs
            .iter()
            .filter(|p| p.starts_with(d.path().join(".config/nvm")))
            .collect();
        assert_eq!(
            nvm[0],
            &d.path().join(".config/nvm/versions/node/v22.23.3/bin")
        );
        assert_eq!(
            nvm.last().unwrap(),
            &&d.path().join(".config/nvm/versions/node/v18.20.1/bin")
        );
    }

    #[test]
    fn claude_code_from_the_desktop_app_is_found_newest_version_first() {
        let d = tempfile::tempdir().unwrap();
        let exe = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        let appdata = d.path().join("appdata");
        for v in ["2.1.9", "2.1.284", "2.1.275", "notes"] {
            let dir = appdata.join("Claude").join("claude-code").join(v);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(exe), b"").unwrap();
        }
        let roots = crate::integrations::Roots {
            home: Some(d.path().join("home")),
            appdata: Some(appdata.clone()),
            ..Default::default()
        };
        assert_eq!(
            bundled_claude_code(&roots),
            Some(
                appdata
                    .join("Claude")
                    .join("claude-code")
                    .join("2.1.284")
                    .join(exe)
            )
        );
        // The native installer's copy wins over the desktop app's.
        let native = d.path().join("home").join(".local").join("bin");
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(native.join(exe), b"").unwrap();
        assert_eq!(bundled_claude_code(&roots), Some(native.join(exe)));
        let empty = crate::integrations::Roots::default();
        assert_eq!(bundled_claude_code(&empty), None);
    }

    fn input(tool: LaunchTool, token: Option<&str>) -> LaunchInput {
        LaunchInput {
            tool,
            hide_external_skills: true,
            host_url: "http://127.0.0.1:11435".into(),
            alias: "qwen3-4b".into(),
            token: token.map(str::to_string),
            context: Some(65_536),
            managed: true,
            extra_args: Vec::new(),
            temp_dir: PathBuf::from("/tmp/hl"),
        }
    }

    fn env_of<'a>(p: &'a LaunchPlan, k: &str) -> Option<&'a str> {
        p.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn opencode_external_skills_are_process_only_and_optional() {
        let mut i = input(LaunchTool::OpenCode, None);
        for hide in [true, false] {
            i.hide_external_skills = hide;
            let p = plan(&i);
            let cmd = p.command();
            for key in OPENCODE_SKILL_ENV {
                assert_eq!(env_of(&p, key), hide.then_some("1"));
                assert_eq!(p.env_remove.iter().any(|k| k == key), !hide);
                let value = cmd.get_envs().find(|(k, _)| *k == key).unwrap().1;
                assert_eq!(value, hide.then_some(OsStr::new("1")));
                let mut desktop = std::process::Command::new("unused");
                apply_opencode_skill_env(&mut desktop, hide);
                assert_eq!(
                    desktop.get_envs().find(|(k, _)| *k == key).unwrap().1,
                    value
                );
            }
        }
        let other = plan(&input(LaunchTool::Codex, None));
        assert!(OPENCODE_SKILL_ENV
            .iter()
            .all(|key| env_of(&other, key).is_none()));
    }

    #[test]
    fn claude_uses_env_and_a_settings_file_and_never_prints_the_token() {
        let secret = "hsg_0123456789abcdef";
        let p = plan(&input(LaunchTool::Claude, Some(secret)));
        assert_eq!(
            env_of(&p, "ANTHROPIC_BASE_URL"),
            Some("http://127.0.0.1:11435")
        );
        assert_eq!(env_of(&p, "ANTHROPIC_AUTH_TOKEN"), Some(secret));
        assert_eq!(env_of(&p, "ANTHROPIC_MODEL"), Some("qwen3-4b"));
        assert_eq!(env_of(&p, "CLAUDE_CODE_MAX_CONTEXT_TOKENS"), Some("65536"));
        assert!(p.env_remove.contains(&"ANTHROPIC_API_KEY".to_string()));
        assert_eq!(p.args[0], "--settings");
        let settings: Value = serde_json::from_str(&p.files[0].content).unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], secret);
        assert!(p.files[0].secret);
        let shown = p.redacted().to_string();
        assert!(!shown.contains(secret), "{shown}");
        assert!(shown.contains("***"));
    }

    #[test]
    fn codex_uses_config_overrides_and_the_responses_api() {
        let p = plan(&LaunchInput {
            extra_args: vec!["exec".into(), "say hi".into()],
            ..input(LaunchTool::Codex, None)
        });
        assert_eq!(p.args[0], "exec");
        let joined = p.args.join(" ");
        assert!(joined.contains("model_provider=harness"));
        assert!(joined.contains("model_providers.harness.base_url=http://127.0.0.1:11435/v1"));
        assert!(joined.contains("model_providers.harness.wire_api=responses"));
        assert!(joined.contains("web_search=disabled"));
        assert!(!joined.contains("env_key"), "no token, no env_key");
        assert!(joined.ends_with("-m qwen3-4b say hi"));
        let with_token = plan(&input(LaunchTool::Codex, Some("hs_abc_0123456789")));
        assert!(with_token
            .args
            .join(" ")
            .contains("env_key=HARNESS_API_KEY"));
        assert_eq!(env_of(&with_token, TOKEN_ENV), Some("hs_abc_0123456789"));
    }

    #[test]
    fn opencode_inline_config() {
        let p = plan(&input(LaunchTool::OpenCode, None));
        let cfg: Value =
            serde_json::from_str(env_of(&p, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        assert_eq!(cfg["model"], "harness/qwen3-4b");
        assert_eq!(
            cfg["provider"]["harness"]["options"]["baseURL"],
            "http://127.0.0.1:11435/v1"
        );
        assert_eq!(
            cfg["provider"]["harness"]["options"]["apiKey"],
            "{env:HARNESS_API_KEY}"
        );
        assert_eq!(env_of(&p, TOKEN_ENV), Some(PLACEHOLDER_KEY));
    }

    #[test]
    fn aider_and_goose() {
        let a = plan(&input(LaunchTool::Aider, None));
        assert_eq!(
            a.args[..2],
            ["--model".to_string(), "openai/qwen3-4b".to_string()]
        );
        let meta: Value = serde_json::from_str(&a.files[0].content).unwrap();
        assert_eq!(meta["openai/qwen3-4b"]["max_input_tokens"], 65_536);
        let g = plan(&input(LaunchTool::Goose, None));
        assert_eq!(env_of(&g, "OPENAI_HOST"), Some("http://127.0.0.1:11435"));
        assert_eq!(env_of(&g, "GOOSE_CONTEXT_LIMIT"), Some("65536"));
        assert_eq!(g.args, vec!["session".to_string()]);
    }

    #[test]
    fn context_checks() {
        assert_eq!(
            check_context(LaunchTool::Claude, Some(65_536)),
            CtxVerdict::Ok
        );
        assert!(matches!(
            check_context(LaunchTool::OpenCode, Some(32_768)),
            CtxVerdict::TooSmall { min: 49_152, .. }
        ));
        assert!(matches!(
            check_context(LaunchTool::Aider, None),
            CtxVerdict::Unknown { .. }
        ));
        assert_eq!(
            prepare_values(LaunchTool::Claude, Some(8192))["context_size"],
            65_536
        );
        assert_eq!(
            prepare_values(LaunchTool::Claude, Some(131_072))["context_size"],
            131_072
        );
    }
}
