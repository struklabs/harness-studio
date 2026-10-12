//! "Start" in a new terminal window on Windows, Linux and macOS. The tool's settings (environment,
//! arguments, working directory) go into a private launcher script (0600, in the plan's temp
//! directory) instead of the terminal's command line, so the token never shows in `ps`.
//! Terminals that hand the window to a running server process (xfce4-terminal, gnome-terminal)
//! would not pass Harness' environment on; the script sets it itself. When the tool exits (or the
//! window is closed) the script removes the temp directory, with the settings files and itself.
//!
//! Linux runs a terminal emulator on the script ([`pick_terminal`]). macOS asks Terminal.app or
//! iTerm2 for a window through `osascript` ([`open_in_macos_terminal`]); only the script's path
//! is in the AppleScript, never the token.
//! Windows reconnects the native child's standard handles to the new console in a private
//! PowerShell launcher, bypassing PowerShell pipelines and supporting npm `.cmd` shims.

use super::launch::LaunchPlan;
use std::path::{Path, PathBuf};

/// Windows GUI processes can inherit pipes or invalid standard handles. A new console alone
/// does not replace them. Reopen all three handles before starting the native TUI.
/// Only a private script contains the plan (including secrets); the command line has its path.
#[cfg(windows)]
pub fn windows_console_command(
    plan: &LaunchPlan,
    program: &Path,
    cwd: Option<&Path>,
    temp_dir: &Path,
) -> Result<std::process::Command, String> {
    use std::os::windows::process::CommandExt;
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut script = String::from("$ErrorActionPreference = 'Stop'\n$status = 1\ntry {\n");
    // Explicitly set the process standard handles; the native child must see console handles.
    script.push_str(r#"Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HarnessConsole {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetStdHandle(int kind, IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    public static void Connect() {
        for (int i = 0; i < 3; i++) {
            IntPtr handle = CreateFile(i == 0 ? "CONIN$" : "CONOUT$", 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (handle == new IntPtr(-1) || !SetHandleInformation(handle, 1, 1) || !SetStdHandle(-10 - i, handle))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
    }
}
'@
[HarnessConsole]::Connect()
"#);
    if let Some(dir) = cwd {
        script.push_str(&format!(
            "Set-Location -LiteralPath {}\n",
            quote(&dir.to_string_lossy())
        ));
    }
    for key in &plan.env_remove {
        script.push_str(&format!(
            "[Environment]::SetEnvironmentVariable({}, $null, 'Process')\n",
            quote(key)
        ));
    }
    for (key, value) in &plan.env {
        script.push_str(&format!(
            "[Environment]::SetEnvironmentVariable({}, {}, 'Process')\n",
            quote(key),
            quote(value)
        ));
    }
    // Calling a native program with PowerShell's `&` creates output pipes. Use Process
    // directly so the TUI inherits the reopened console handles, with no PS pipeline.
    script.push_str("$start = New-Object System.Diagnostics.ProcessStartInfo\n$start.UseShellExecute = $false\n");
    if let Some(dir) = cwd {
        script.push_str(&format!(
            "$start.WorkingDirectory = {}\n",
            quote(&dir.to_string_lossy())
        ));
    }
    let batch = program
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let (file, arguments) = if batch {
        let mut command = String::from("/d /s /c \"");
        for (i, value) in std::iter::once(program.to_string_lossy().as_ref())
            .chain(plan.args.iter().map(String::as_str))
            .enumerate()
        {
            let key = format!("HARNESS_LAUNCH_ARG_{i}");
            // Expand data once, inside quotes; percent sequences in the value are not
            // expanded recursively. Refuse quotes/control characters in batch arguments.
            if value.contains('"') || value.chars().any(char::is_control) {
                return Err("launch.bad_batch_argument".into());
            }
            script.push_str(&format!(
                "[Environment]::SetEnvironmentVariable({}, {}, 'Process')\n",
                quote(&key),
                quote(value)
            ));
            command.push_str(&format!("\"%{key}%\" "));
        }
        command.push('"');
        ("cmd.exe".to_string(), command)
    } else {
        (
            program.to_string_lossy().to_string(),
            plan.args
                .iter()
                .map(|a| windows_native_arg(a))
                .collect::<Vec<_>>()
                .join(" "),
        )
    };
    script.push_str(&format!("$start.FileName = {}\n$start.Arguments = {}\n$child = [System.Diagnostics.Process]::Start($start)\n$child.WaitForExit()\n$status = $child.ExitCode\n$child.Dispose()\n", quote(&file), quote(&arguments)));
    script.push_str("} catch { Write-Host $_; $status = 1 } finally {\n");
    script.push_str(&format!(
        "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue\n",
        quote(&temp_dir.to_string_lossy())
    ));
    script.push_str("}\nif ($status -ne 0) { Write-Host \"[exit $status]\"; Read-Host 'Enter to close' }\nexit $status\n");
    let path = temp_dir.join("launch.ps1");
    // Windows PowerShell 5.1 needs a BOM to decode non-ASCII paths/arguments as UTF-8.
    crate::instance::write_private_file(&path, format!("\u{feff}{script}").as_bytes())?;
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ]);
    cmd.arg(path);
    super::launch::apply_augmented_path(&mut cmd);
    cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE; no DETACHED_PROCESS/start/wt handoff.
    Ok(cmd)
}

#[cfg(windows)]
fn windows_native_arg(value: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.push_str(&"\\".repeat(if c == '"' { slashes * 2 + 1 } else { slashes }));
        out.push(c);
        slashes = 0;
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

/// Single-quoted for `sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn valid_env_name(k: &str) -> bool {
    let mut chars = k.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// The launcher script's text. `program` is the resolved executable.
pub fn launcher_script(
    plan: &LaunchPlan,
    program: &Path,
    cwd: Option<&Path>,
    temp_dir: &Path,
) -> Result<String, String> {
    let mut s = String::from("#!/bin/sh\n# Harness Studio launcher: removes its folder (settings and this file) when the tool exits.\n");
    s.push_str(&format!(
        "HARNESS_LAUNCH_DIR={}\n",
        sh_quote(&temp_dir.to_string_lossy())
    ));
    s.push_str("trap 'rm -rf -- \"$HARNESS_LAUNCH_DIR\"' EXIT\ntrap 'exit 129' HUP\ntrap 'exit 130' INT\ntrap 'exit 143' TERM\n");
    if let Some(dir) = cwd {
        s.push_str(&format!(
            "cd -- {} || exit 1\n",
            sh_quote(&dir.to_string_lossy())
        ));
    }
    for k in &plan.env_remove {
        if !valid_env_name(k) {
            return Err(format!("launch.bad_env_name: {k}"));
        }
        s.push_str(&format!("unset {k}\n"));
    }
    for (k, v) in &plan.env {
        if !valid_env_name(k) {
            return Err(format!("launch.bad_env_name: {k}"));
        }
        s.push_str(&format!("{k}={}\nexport {k}\n", sh_quote(v)));
    }
    s.push_str(&sh_quote(&program.to_string_lossy()));
    for a in &plan.args {
        s.push(' ');
        s.push_str(&sh_quote(a));
    }
    s.push_str("\nstatus=$?\n");
    // Keep the window open when the tool failed at once, so the error can be read.
    s.push_str("if [ \"$status\" -ne 0 ]; then printf '\\n[exit %s] Enter to close. ' \"$status\"; read -r _; fi\nexit \"$status\"\n");
    Ok(s)
}

/// Writes the launcher script (0600) into `temp_dir` and returns its path.
pub fn write_launcher(
    plan: &LaunchPlan,
    program: &Path,
    cwd: Option<&Path>,
    temp_dir: &Path,
) -> Result<PathBuf, String> {
    let text = launcher_script(plan, program, cwd, temp_dir)?;
    let path = temp_dir.join("launch.sh");
    crate::instance::write_private_file(&path, text.as_bytes())?;
    Ok(path)
}

/// A terminal emulator and the arguments that run `sh <script>` in a new window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Known terminals, in order: `$TERMINAL`, the Debian/Ubuntu alternative, then common ones.
/// `find` resolves a program name (PATH lookup; injectable for tests).
pub fn pick_terminal(
    script: &Path,
    terminal_env: Option<&str>,
    find: impl Fn(&str) -> Option<PathBuf>,
) -> Option<TerminalCommand> {
    let script = script.to_string_lossy().to_string();
    let with = |program: PathBuf, head: &[&str]| {
        let mut args: Vec<String> = head.iter().map(|s| s.to_string()).collect();
        args.push("sh".into());
        args.push(script.clone());
        TerminalCommand { program, args }
    };
    // Per-terminal "run this command" syntax.
    let args_for = |name: &str| -> &'static [&'static str] {
        match name {
            "xfce4-terminal" => &["--disable-server", "-x"],
            "gnome-terminal" | "kgx" | "ptyxis" | "wezterm" => &["--"],
            "kitty" => &[],
            "foot" | "footclient" => &[],
            "tilix" | "terminator" | "mate-terminal" => &["-x"],
            _ => &["-e"],
        }
    };
    if let Some(t) = terminal_env
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.contains(' '))
    {
        if let Some(p) = find(t) {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            return Some(with(p, args_for(&name)));
        }
    }
    // `x-terminal-emulator` may point at a wrapper (xfce4-terminal.wrapper, gnome-terminal.wrapper)
    // that takes `-e`; resolve it to know the real terminal and use its own syntax.
    if let Some(p) = find("x-terminal-emulator") {
        let real = std::fs::canonicalize(&p).unwrap_or(p.clone());
        let name = real
            .file_name()
            .map(|n| n.to_string_lossy().trim_end_matches(".wrapper").to_string())
            .unwrap_or_default();
        return Some(match find(&name) {
            Some(direct) if !name.is_empty() => with(direct, args_for(&name)),
            _ => with(p, &["-e"]),
        });
    }
    for name in [
        "xfce4-terminal",
        "gnome-terminal",
        "konsole",
        "kgx",
        "ptyxis",
        "tilix",
        "mate-terminal",
        "terminator",
        "kitty",
        "alacritty",
        "wezterm",
        "foot",
        "xterm",
    ] {
        if let Some(p) = find(name) {
            return Some(with(p, args_for(name)));
        }
    }
    None
}

// --- macOS ---

/// The terminal apps Harness can open a window in on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacTerminal {
    Terminal,
    ITerm,
}

/// Bundle id of iTerm2.
const ITERM_BUNDLE_ID: &str = "com.googlecode.iterm2";

/// A string literal's content for AppleScript (`\` and `"` are escaped).
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The AppleScript lines (one per `osascript -e`) that open a new window running
/// `sh <script>`. The command is typed into Terminal.app's shell / run as iTerm2's session
/// command, so it holds the launcher's path and nothing else.
pub fn macos_applescript(terminal: MacTerminal, script: &Path) -> Result<Vec<String>, String> {
    let path = script.to_string_lossy();
    if path.chars().any(char::is_control) {
        return Err(format!("launch.bad_path: {path}"));
    }
    let command = applescript_escape(&format!("sh {}", sh_quote(&path)));
    Ok(match terminal {
        MacTerminal::Terminal => vec![
            "tell application \"Terminal\"".into(),
            "activate".into(),
            format!("do script \"exec {command}\""),
            "end tell".into(),
        ],
        MacTerminal::ITerm => vec![
            "tell application \"iTerm\"".into(),
            "activate".into(),
            format!("create window with default profile command \"{command}\""),
            "end tell".into(),
        ],
    })
}

/// iTerm2 when it is installed and the default handler of shell scripts / `.command` files
/// (`public.unix-executable`, the closest macOS has to a default terminal); Terminal.app
/// otherwise. `handler`: [`default_script_handler`] of `defaults read` output.
pub fn pick_macos_terminal(handler: Option<&str>, iterm_installed: bool) -> MacTerminal {
    if iterm_installed && handler.is_some_and(|h| h.eq_ignore_ascii_case(ITERM_BUNDLE_ID)) {
        MacTerminal::ITerm
    } else {
        MacTerminal::Terminal
    }
}

/// Bundle id that opens shell scripts, from the old-style plist text of
/// `defaults read com.apple.LaunchServices/com.apple.launchservices.secure LSHandlers`: the
/// entry whose content type is `public.unix-executable` (or whose tag is `command`) and whose
/// `LSHandlerRoleAll` is not the "-" placeholder.
pub fn default_script_handler(defaults_output: &str) -> Option<String> {
    let mut depth = 0usize;
    let mut is_script = false;
    let mut role: Option<String> = None;
    for line in defaults_output.lines() {
        let line = line.trim();
        let value = |key: &str| -> Option<String> {
            let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
            Some(
                rest.trim()
                    .trim_end_matches(';')
                    .trim()
                    .trim_matches('"')
                    .to_string(),
            )
        };
        if let Some(v) = value("LSHandlerContentType") {
            is_script |= v == "public.unix-executable";
        }
        if let Some(v) = value("LSHandlerContentTypeTag") {
            is_script |= v == "command";
        }
        if let Some(v) = value("LSHandlerRoleAll") {
            if v != "-" && !v.is_empty() {
                role = Some(v);
            }
        }
        depth += line.matches('{').count();
        depth = depth.saturating_sub(line.matches('}').count());
        if depth == 0 && line.contains('}') {
            if is_script {
                if let Some(r) = role.take() {
                    return Some(r);
                }
            }
            is_script = false;
            role = None;
        }
    }
    None
}

/// Opens a new Terminal.app / iTerm2 window running the launcher `script` and returns the
/// `osascript` process id. iTerm2 is tried first only when it is the default; Terminal.app is
/// the fallback. `Err("launch.terminal_unsupported")` when neither window opened (the UI then
/// offers the `harness launch` command). Runs `osascript` on any OS; callers gate on macOS.
pub fn open_in_macos_terminal(script: &Path) -> Result<u32, String> {
    use std::process::{Command, Stdio};
    let handler = Command::new("defaults")
        .args([
            "read",
            "com.apple.LaunchServices/com.apple.launchservices.secure",
            "LSHandlers",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| default_script_handler(&String::from_utf8_lossy(&o.stdout)));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let iterm_installed = [
        Some(PathBuf::from("/Applications/iTerm.app")),
        home.map(|h| h.join("Applications/iTerm.app")),
    ]
    .into_iter()
    .flatten()
    .any(|p| p.is_dir());
    let first = pick_macos_terminal(handler.as_deref(), iterm_installed);
    let mut order = vec![first];
    if first != MacTerminal::Terminal {
        order.push(MacTerminal::Terminal);
    }
    for terminal in order {
        let mut cmd = Command::new("osascript");
        for line in macos_applescript(terminal, script)? {
            cmd.arg("-e").arg(line);
        }
        let Ok(mut child) = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
            continue;
        };
        let pid = child.id();
        if child.wait().is_ok_and(|s| s.success()) {
            return Ok(pid);
        }
    }
    Err("launch.terminal_unsupported".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::launch::{plan, LaunchInput, LaunchTool};

    #[cfg(windows)]
    #[test]
    fn new_console_does_not_replace_inherited_redirected_handles() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "[Console]::IsInputRedirected; [Console]::IsOutputRedirected; [Console]::IsErrorRedirected"])
            .stdin(Stdio::null())
            .creation_flags(0x10)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .replace('\r', ""),
            "True\nTrue\nTrue"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_launcher_reconnects_all_tools_to_console_and_keeps_plan() {
        use std::process::Stdio;
        let d = tempfile::tempdir().unwrap();
        let probe = d.path().join("probe.ps1");
        let batch_probe = d.path().join("probe.cmd");
        std::fs::write(
            &batch_probe,
            "@echo off\r\npowershell.exe -NoProfile -File \"%~dp0probe.ps1\" %*\r\n",
        )
        .unwrap();
        std::fs::write(&probe, "$environment = @{}; foreach ($key in $env:HS_ENV_NAMES.Split(';')) { $environment[$key] = [Environment]::GetEnvironmentVariable($key) }\n$config = $env:OPENCODE_CONFIG_CONTENT\n[pscustomobject]@{ environment = $environment; input = [Console]::IsInputRedirected; output = [Console]::IsOutputRedirected; error = [Console]::IsErrorRedirected; key = $env:HARNESS_API_KEY; config = $config; removed = $env:HS_REMOVE; cwd = (Get-Location).Path; args = $args } | ConvertTo-Json -Compress | Set-Content -LiteralPath $env:HS_RESULT -Encoding UTF8\n").unwrap();
        for (tool, batch) in LaunchTool::ALL
            .into_iter()
            .flat_map(|tool| [(tool, false), (tool, true)])
        {
            let launch_dir = d.path().join(format!("{}-{batch}", tool.id()));
            std::fs::create_dir(&launch_dir).unwrap();
            let mut p = plan(&LaunchInput {
                tool,
                hide_external_skills: true,
                host_url: "http://127.0.0.1:11435".into(),
                alias: "fixture".into(),
                token: Some("tok-secret-123456".into()),
                context: Some(65536),
                managed: true,
                extra_args: Vec::new(),
                temp_dir: launch_dir.clone(),
            });
            p.write_files().unwrap();
            // Probe reports the actual plan's environment without running an agent/account.
            let expected_env = p.env.clone();
            p.env.push((
                "HS_ENV_NAMES".into(),
                expected_env
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>()
                    .join(";"),
            ));
            p.args = vec![
                "-NoProfile".into(),
                "-File".into(),
                probe.to_string_lossy().into(),
                "İş klasörü".into(),
                "it's $HOME & data".into(),
            ];
            if batch {
                p.args.drain(..3);
            }
            p.env_remove.push("HS_REMOVE".into());
            let result = d.path().join(format!("{}.json", tool.id()));
            p.env
                .push(("HS_RESULT".into(), result.to_string_lossy().into()));
            let mut cmd = windows_console_command(
                &p,
                if batch {
                    &batch_probe
                } else {
                    Path::new("powershell.exe")
                },
                Some(d.path()),
                &launch_dir,
            )
            .unwrap();
            assert!(!format!("{cmd:?}").contains("tok-secret-123456"));
            let mut child = cmd
                .env("HS_REMOVE", "inherited")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let start = std::time::Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if start.elapsed() > std::time::Duration::from_secs(60) {
                    child.kill().unwrap();
                    panic!("Windows launcher timed out for {}", tool.id());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            };
            assert!(status.success(), "{}", tool.id());
            let text = std::fs::read_to_string(result).unwrap();
            let v: serde_json::Value =
                serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
            for key in ["input", "output", "error"] {
                assert_eq!(v[key], false, "{tool:?}: {v}");
            }
            assert_eq!(v["removed"], serde_json::Value::Null);
            // CI temp dirs can be 8.3 short paths (RUNNER~1); compare canonical forms.
            let cwd = std::fs::canonicalize(v["cwd"].as_str().unwrap()).unwrap();
            assert_eq!(cwd, std::fs::canonicalize(d.path()).unwrap());
            assert_eq!(
                v["args"],
                serde_json::json!(["İş klasörü", "it's $HOME & data"])
            );
            for (key, value) in expected_env {
                assert_eq!(v["environment"][&key], value, "{tool:?}: {key}");
            }
            assert!(!launch_dir.exists());
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "opens the installed OpenCode CLI in an isolated new console"]
    fn installed_opencode_runs_in_isolated_windows_console() {
        let d = tempfile::tempdir().unwrap();
        let program = LaunchTool::OpenCode
            .find_executable()
            .expect("installed OpenCode");
        let launch_dir = d.path().join("launch");
        std::fs::create_dir(&launch_dir).unwrap();
        let mut p = plan(&LaunchInput {
            tool: LaunchTool::OpenCode,
            hide_external_skills: true,
            host_url: "http://127.0.0.1:11435".into(),
            alias: "fixture".into(),
            token: None,
            context: Some(65536),
            managed: true,
            extra_args: vec!["--version".into()],
            temp_dir: launch_dir.clone(),
        });
        for key in [
            "HARNESS_DATA_DIR",
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
        ] {
            let root = d.path().join(key);
            std::fs::create_dir(&root).unwrap();
            p.env.push((key.into(), root.to_string_lossy().into()));
        }
        p.env
            .push(("OPENCODE_DISABLE_PROJECT_CONFIG".into(), "true".into()));
        p.env
            .push(("OPENCODE_DISABLE_AUTOUPDATE".into(), "true".into()));
        p.write_files().unwrap();
        assert!(
            windows_console_command(&p, &program, Some(d.path()), &launch_dir)
                .unwrap()
                .status()
                .unwrap()
                .success()
        );
        assert!(!launch_dir.exists());
    }

    fn sample_plan(dir: &Path) -> LaunchPlan {
        plan(&LaunchInput {
            tool: LaunchTool::OpenCode,
            hide_external_skills: true,
            host_url: "http://127.0.0.1:11435".into(),
            alias: "qwen it's".into(),
            token: Some("tok-secret-123456".into()),
            context: Some(32768),
            managed: true,
            extra_args: vec!["--weird arg's".into()],
            temp_dir: dir.to_path_buf(),
        })
    }

    #[test]
    fn script_sets_env_quotes_args_and_cleans_up() {
        let d = tempfile::tempdir().unwrap();
        let p = sample_plan(d.path());
        let s = launcher_script(
            &p,
            Path::new("/opt/o c/opencode"),
            Some(Path::new("/home/u/İş klasörü")),
            d.path(),
        )
        .unwrap();
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(s.contains("trap 'rm -rf -- \"$HARNESS_LAUNCH_DIR\"' EXIT"));
        assert!(s.contains("cd -- '/home/u/İş klasörü' || exit 1"));
        assert!(s.contains("'/opt/o c/opencode'"));
        assert!(s.contains(r"'--weird arg'\''s'"));
        for (k, _) in &p.env {
            assert!(s.contains(&format!("export {k}\n")), "{k}");
        }
        for k in &p.env_remove {
            assert!(s.contains(&format!("unset {k}\n")), "{k}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn script_runs_under_sh_with_the_plan_environment_and_removes_its_folder() {
        use std::process::Command;
        let d = tempfile::tempdir().unwrap();
        let launch_dir = d.path().join("launch");
        std::fs::create_dir_all(&launch_dir).unwrap();
        let mut p = sample_plan(&launch_dir);
        p.env = vec![("HS_T1".into(), "a b 'c' $HOME".into())];
        p.env_remove = vec!["HS_T2".into()];
        p.args = vec![
            "-c".into(),
            "printf '%s|%s' \"$HS_T1\" \"${HS_T2-unset}\" > \"$0\"".into(),
            d.path().join("out").to_string_lossy().to_string(),
        ];
        let script = write_launcher(&p, Path::new("/bin/sh"), Some(d.path()), &launch_dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&script).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let st = Command::new("sh")
            .arg(&script)
            .env("HS_T2", "inherited")
            .status()
            .unwrap();
        assert!(st.success());
        assert_eq!(
            std::fs::read_to_string(d.path().join("out")).unwrap(),
            "a b 'c' $HOME|unset"
        );
        assert!(!launch_dir.exists(), "launch folder removed");
    }

    #[test]
    fn macos_terminal_script_runs_only_the_launcher_and_holds_no_token() {
        let d = Path::new("/var/folders/ab/T/harness-launch-0123456789abcdef");
        let p = sample_plan(d);
        let token = p
            .env
            .iter()
            .map(|(_, v)| v.clone())
            .find(|v| v.contains("tok-secret-123456"))
            .expect("the plan carries the token in its environment");
        let launcher = launcher_script(&p, Path::new("/opt/o c/opencode"), None, d).unwrap();
        assert!(
            launcher.contains(&token),
            "the token lives in the 0600 launcher"
        );
        // A literal, not `d.join(..)`: Windows would join with a backslash.
        let script = Path::new("/var/folders/ab/T/harness-launch-0123456789abcdef/launch.sh");
        for terminal in [MacTerminal::Terminal, MacTerminal::ITerm] {
            let lines = macos_applescript(terminal, script).unwrap();
            let text = lines.join("\n");
            assert!(!text.contains("tok-secret"), "{text}");
            assert!(lines.first().unwrap().starts_with("tell application"));
            assert_eq!(lines.last().unwrap(), "end tell");
            assert!(
                text.contains("sh '/var/folders/ab/T/harness-launch-0123456789abcdef/launch.sh'"),
                "{text}"
            );
        }
        let t = macos_applescript(MacTerminal::Terminal, script).unwrap();
        assert_eq!(t[0], "tell application \"Terminal\"");
        assert!(t[2].starts_with("do script \"exec sh '"), "{}", t[2]);
        let i = macos_applescript(MacTerminal::ITerm, script).unwrap();
        assert_eq!(i[0], "tell application \"iTerm\"");
        assert!(
            i[2].starts_with("create window with default profile command \"sh '"),
            "{}",
            i[2]
        );
    }

    #[test]
    fn macos_applescript_escapes_quotes_backslashes_and_refuses_control_characters() {
        // sh quoting first (`'` -> `'\''`), then AppleScript string escaping (`\` -> `\\`).
        let lines = macos_applescript(
            MacTerminal::Terminal,
            Path::new("/tmp/it's \"x\" \\y/launch.sh"),
        )
        .unwrap();
        assert_eq!(
            lines[2],
            r#"do script "exec sh '/tmp/it'\\''s \"x\" \\y/launch.sh'""#
        );
        assert!(macos_applescript(MacTerminal::ITerm, Path::new("/tmp/a\nb/launch.sh")).is_err());
    }

    #[test]
    fn macos_terminal_choice_needs_iterm_installed_and_default() {
        assert_eq!(
            pick_macos_terminal(Some("com.googlecode.iterm2"), true),
            MacTerminal::ITerm
        );
        assert_eq!(
            pick_macos_terminal(Some("com.googlecode.iterm2"), false),
            MacTerminal::Terminal
        );
        assert_eq!(
            pick_macos_terminal(Some("com.apple.Terminal"), true),
            MacTerminal::Terminal
        );
        assert_eq!(pick_macos_terminal(None, true), MacTerminal::Terminal);
    }

    /// The old-style plist text `defaults read ... LSHandlers` prints (hand-written from that
    /// format; not captured on a Mac).
    const LS_HANDLERS: &str = r#"(
        {
        LSHandlerContentType = "public.html";
        LSHandlerPreferredVersions =         {
            LSHandlerRoleAll = "-";
        };
        LSHandlerRoleAll = "com.google.chrome";
    },
        {
        LSHandlerContentType = "public.unix-executable";
        LSHandlerPreferredVersions =         {
            LSHandlerRoleAll = "-";
        };
        LSHandlerRoleAll = "com.googlecode.iterm2";
    },
        {
        LSHandlerRoleAll = "com.apple.mail";
        LSHandlerURLScheme = mailto;
    }
)
"#;

    #[test]
    fn default_script_handler_reads_the_unix_executable_entry() {
        assert_eq!(
            default_script_handler(LS_HANDLERS).as_deref(),
            Some("com.googlecode.iterm2")
        );
        let tagged = "(\n    {\n    LSHandlerContentTypeTag = command;\n    LSHandlerContentTagClass = \"public.filename-extension\";\n    LSHandlerRoleAll = \"com.apple.Terminal\";\n}\n)";
        assert_eq!(
            default_script_handler(tagged).as_deref(),
            Some("com.apple.Terminal")
        );
        assert_eq!(default_script_handler(""), None);
        // Only the placeholder: no handler.
        let none = "(\n  {\n  LSHandlerContentType = \"public.unix-executable\";\n  LSHandlerRoleAll = \"-\";\n  }\n)";
        assert_eq!(default_script_handler(none), None);
    }

    #[test]
    fn terminal_choice_follows_terminal_env_then_alternative_then_known() {
        let s = Path::new("/tmp/x/launch.sh");
        let only = |names: &'static [&'static str]| {
            move |n: &str| {
                names
                    .contains(&n)
                    .then(|| PathBuf::from(format!("/usr/bin/{n}")))
            }
        };
        let t = pick_terminal(s, Some("kitty"), only(&["kitty", "xterm"])).unwrap();
        assert_eq!(t.args, vec!["sh", "/tmp/x/launch.sh"]);
        let t = pick_terminal(s, None, only(&["xfce4-terminal", "xterm"])).unwrap();
        assert_eq!(t.program, PathBuf::from("/usr/bin/xfce4-terminal"));
        assert_eq!(
            t.args,
            vec!["--disable-server", "-x", "sh", "/tmp/x/launch.sh"]
        );
        let t = pick_terminal(s, None, only(&["gnome-terminal"])).unwrap();
        assert_eq!(t.args, vec!["--", "sh", "/tmp/x/launch.sh"]);
        let t = pick_terminal(s, None, only(&["xterm"])).unwrap();
        assert_eq!(t.args, vec!["-e", "sh", "/tmp/x/launch.sh"]);
        assert!(pick_terminal(s, None, only(&[])).is_none());
    }
}
