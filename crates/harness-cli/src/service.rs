//! `harness service install|uninstall|print`: run `harness serve` when the user logs in.
//!
//! * Linux: a `systemd --user` unit (`~/.config/systemd/user/<name>.service`), enabled with
//!   `systemctl --user enable --now`. SIGTERM on stop → clean shutdown.
//! * macOS: a LaunchAgent plist (`~/Library/LaunchAgents/<label>.plist`), loaded with
//!   `launchctl bootstrap gui/<uid>`.
//! * Windows: a Task Scheduler task with a logon trigger for the current user (`schtasks
//!   /Create /XML`), least privilege, no time limit, restart on failure. The task starts
//!   `harness-hidden.exe` (installed next to `harness.exe`), a GUI-subsystem launcher that
//!   runs `harness serve` without a console window; without it the task runs `harness.exe`
//!   and a console window stays open. Ending the task terminates the launcher; its Job Object
//!   takes `harness serve` and the llama-servers with it.
//!
//! The definitions are generated here (unit-tested), the OS tools install them.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::cli::{ServiceArgs, ServicePlatform};
use crate::error::{CliError, CliResult, Exit};
use crate::output::Out;

pub const DEFAULT_SYSTEMD_NAME: &str = "harness";
pub const DEFAULT_LAUNCHD_LABEL: &str = "com.harnessstudio.harness";
pub const DEFAULT_TASK_NAME: &str = "HarnessStudio\\harness-serve";
const DESCRIPTION: &str = "Harness Studio headless instance (harness serve)";

/// What the service runs.
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    pub name: String,
    /// Program the service starts (on Windows the console-less launcher when present).
    pub exe: PathBuf,
    pub args: Vec<String>,
    /// Log file (launchd has no journal).
    pub log_file: Option<PathBuf>,
}

pub fn this_platform() -> ServicePlatform {
    if cfg!(windows) {
        ServicePlatform::Windows
    } else if cfg!(target_os = "macos") {
        ServicePlatform::Launchd
    } else {
        ServicePlatform::Systemd
    }
}

fn default_name(p: ServicePlatform) -> &'static str {
    match p {
        ServicePlatform::Systemd => DEFAULT_SYSTEMD_NAME,
        ServicePlatform::Launchd => DEFAULT_LAUNCHD_LABEL,
        ServicePlatform::Windows => DEFAULT_TASK_NAME,
    }
}

fn validate_name(name: &str, p: ServicePlatform) -> CliResult<()> {
    let ok = !name.is_empty()
        && name.len() <= 200
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '-' | '_' | '.' | '@')
                || (p == ServicePlatform::Windows && matches!(c, '\\' | ' '))
        })
        && !name.contains("..");
    if ok {
        Ok(())
    } else {
        Err(CliError::usage(
            "cli.invalid_argument",
            format!("Invalid service name \"{}\"", name),
        ))
    }
}

// --- systemd ---

/// systemd `ExecStart=` quoting: every word in double quotes, `\` and `"` escaped, `%`
/// doubled (specifiers), `$` doubled (variable expansion).
fn systemd_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn systemd_unit(spec: &ServiceSpec) -> String {
    let exec: Vec<String> = std::iter::once(spec.exe.to_string_lossy().to_string())
        .chain(spec.args.iter().cloned())
        .map(|a| systemd_quote(&a))
        .collect();
    format!(
        "[Unit]\n\
Description={DESCRIPTION}\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={}\n\
Restart=on-failure\n\
RestartSec=5\n\
KillSignal=SIGTERM\n\
TimeoutStopSec=30\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        exec.join(" ")
    )
}

// --- launchd ---

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn launchd_plist(spec: &ServiceSpec) -> String {
    let mut lines = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string(),
        r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#.to_string(),
        r#"<plist version="1.0">"#.to_string(),
        "<dict>".to_string(),
        "    <key>Label</key>".to_string(),
        format!("    <string>{}</string>", xml_escape(&spec.name)),
        "    <key>ProgramArguments</key>".to_string(),
        "    <array>".to_string(),
    ];
    for a in
        std::iter::once(spec.exe.to_string_lossy().to_string()).chain(spec.args.iter().cloned())
    {
        lines.push(format!("        <string>{}</string>", xml_escape(&a)));
    }
    lines.extend(
        [
            "    </array>",
            "    <key>RunAtLoad</key>",
            "    <true/>",
            "    <key>KeepAlive</key>",
            "    <dict>",
            "        <key>SuccessfulExit</key>",
            "        <false/>",
            "    </dict>",
            "    <key>ProcessType</key>",
            "    <string>Background</string>",
        ]
        .map(String::from),
    );
    if let Some(log) = &spec.log_file {
        let log = xml_escape(&log.to_string_lossy());
        for key in ["StandardOutPath", "StandardErrorPath"] {
            lines.push(format!("    <key>{}</key>", key));
            lines.push(format!("    <string>{}</string>", log));
        }
    }
    lines.push("</dict>".into());
    lines.push("</plist>".into());
    lines.join("\n") + "\n"
}

// --- Windows Task Scheduler ---

/// One Windows command-line argument (CommandLineToArgvW rules).
fn win_quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| matches!(c, ' ' | '\t' | '"')) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Task definition (Task Scheduler schema 1.2). `user` = `DOMAIN\name` of the account whose
/// logon starts it.
pub fn windows_task_xml(spec: &ServiceSpec, user: &str) -> String {
    let args: Vec<String> = spec.args.iter().map(|a| win_quote(a)).collect();
    let user = xml_escape(user);
    let lines = [
        r#"<?xml version="1.0" encoding="UTF-16"?>"#.to_string(),
        r#"<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">"#
            .to_string(),
        "  <RegistrationInfo>".into(),
        format!("    <Description>{}</Description>", DESCRIPTION),
        "  </RegistrationInfo>".into(),
        "  <Triggers>".into(),
        "    <LogonTrigger>".into(),
        "      <Enabled>true</Enabled>".into(),
        format!("      <UserId>{}</UserId>", user),
        "    </LogonTrigger>".into(),
        "  </Triggers>".into(),
        "  <Principals>".into(),
        r#"    <Principal id="Author">"#.into(),
        format!("      <UserId>{}</UserId>", user),
        "      <LogonType>InteractiveToken</LogonType>".into(),
        "      <RunLevel>LeastPrivilege</RunLevel>".into(),
        "    </Principal>".into(),
        "  </Principals>".into(),
        "  <Settings>".into(),
        "    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>".into(),
        "    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>".into(),
        "    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>".into(),
        "    <AllowHardTerminate>true</AllowHardTerminate>".into(),
        "    <StartWhenAvailable>false</StartWhenAvailable>".into(),
        "    <IdleSettings>".into(),
        "      <StopOnIdleEnd>false</StopOnIdleEnd>".into(),
        "      <RestartOnIdle>false</RestartOnIdle>".into(),
        "    </IdleSettings>".into(),
        "    <AllowStartOnDemand>true</AllowStartOnDemand>".into(),
        "    <Enabled>true</Enabled>".into(),
        "    <Hidden>false</Hidden>".into(),
        "    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>".into(),
        "    <RestartOnFailure>".into(),
        "      <Interval>PT1M</Interval>".into(),
        "      <Count>3</Count>".into(),
        "    </RestartOnFailure>".into(),
        "  </Settings>".into(),
        r#"  <Actions Context="Author">"#.into(),
        "    <Exec>".into(),
        format!(
            "      <Command>{}</Command>",
            xml_escape(&spec.exe.to_string_lossy())
        ),
        format!(
            "      <Arguments>{}</Arguments>",
            xml_escape(&args.join(" "))
        ),
        "    </Exec>".into(),
        "  </Actions>".into(),
        "</Task>".into(),
    ];
    lines.join("\r\n") + "\r\n"
}

/// UTF-16LE with BOM (what `schtasks /XML` expects for `encoding="UTF-16"`).
pub fn utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

// --- install / uninstall ---

fn home() -> CliResult<PathBuf> {
    dirs::home_dir().ok_or_else(|| CliError::general("cli.io", "No home directory"))
}

fn unit_path(p: ServicePlatform, name: &str) -> CliResult<Option<PathBuf>> {
    Ok(match p {
        ServicePlatform::Systemd => Some(
            dirs::config_dir()
                .unwrap_or(home()?.join(".config"))
                .join("systemd/user")
                .join(format!("{}.service", name)),
        ),
        ServicePlatform::Launchd => Some(
            home()?
                .join("Library/LaunchAgents")
                .join(format!("{}.plist", name)),
        ),
        ServicePlatform::Windows => None,
    })
}

fn windows_user() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(d) if !d.is_empty() => format!("{}\\{}", d, user),
        _ => user,
    }
}

/// File name of the console-less launcher (`src/bin/harness-hidden.rs`).
pub const HIDDEN_LAUNCHER: &str = "harness-hidden.exe";

/// The console-less launcher next to `harness.exe`, when it was installed with it.
pub fn hidden_launcher(harness_exe: &Path) -> Option<PathBuf> {
    let launcher = harness_exe.with_file_name(HIDDEN_LAUNCHER);
    launcher.is_file().then_some(launcher)
}

fn spec_for(
    platform: ServicePlatform,
    name: &str,
    data_dir: Option<&Path>,
    log_dir: &Path,
) -> CliResult<ServiceSpec> {
    let exe = std::env::current_exe()
        .map_err(|e| CliError::general("cli.io", format!("Own path unknown: {}", e)))?;
    let exe = exe
        .canonicalize()
        .map(|p| {
            harness_core::tools::scope::strip_verbatim(&p.to_string_lossy())
                .map_or(p, PathBuf::from)
        })
        .unwrap_or(exe);
    // Task Scheduler opens a console window for a console program; the launcher has none.
    let exe = match platform {
        ServicePlatform::Windows if cfg!(windows) => hidden_launcher(&exe).unwrap_or(exe),
        _ => exe,
    };
    let mut args = vec!["serve".to_string()];
    if let Some(d) = data_dir {
        args.push("--data-dir".into());
        args.push(d.to_string_lossy().to_string());
    }
    Ok(ServiceSpec {
        name: name.to_string(),
        exe,
        args,
        log_file: Some(log_dir.join("harness-serve.log")),
    })
}

fn content(p: ServicePlatform, spec: &ServiceSpec) -> String {
    match p {
        ServicePlatform::Systemd => systemd_unit(spec),
        ServicePlatform::Launchd => launchd_plist(spec),
        ServicePlatform::Windows => windows_task_xml(spec, &windows_user()),
    }
}

fn run_tool(program: &str, args: &[&str]) -> CliResult<String> {
    let output = harness_core::process::hidden_command(program)
        .args(args)
        .output()
        .map_err(|e| {
            CliError::new(
                Exit::General,
                "cli.service_failed",
                format!("{} could not be run: {}", program, e),
            )
        })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();
    if output.status.success() {
        Ok(text)
    } else {
        Err(CliError::new(
            Exit::General,
            "cli.service_failed",
            format!("{} {} failed: {}", program, args.join(" "), text),
        )
        .with("output", text))
    }
}

fn uid() -> CliResult<String> {
    run_tool("id", &["-u"]).map(|s| s.trim().to_string())
}

pub enum Action {
    Install,
    Uninstall,
    Print,
}

/// `data_dir_override`: the `--data-dir` given to this command (passed on to the service).
pub fn run(
    action: Action,
    args: &ServiceArgs,
    data_dir_override: Option<&Path>,
    data_dir: &Path,
    out: &Out,
) -> CliResult<()> {
    let platform = match (&action, args.platform) {
        (Action::Print, Some(p)) => p,
        (_, Some(p)) if p != this_platform() => {
            return Err(CliError::usage(
                "cli.invalid_argument",
                "--platform only applies to `service print` for another OS",
            ))
        }
        _ => this_platform(),
    };
    let name = args
        .name
        .clone()
        .unwrap_or_else(|| default_name(platform).to_string());
    validate_name(&name, platform)?;
    let spec = spec_for(platform, &name, data_dir_override, &data_dir.join("logs"))?;
    let hidden = platform == ServicePlatform::Windows
        && spec.exe.file_name().and_then(|n| n.to_str()) == Some(HIDDEN_LAUNCHER);
    let text = content(platform, &spec);
    let path = unit_path(platform, &name)?;
    let platform_name = format!("{:?}", platform).to_lowercase();
    match action {
        Action::Print => {
            let value =
                json!({"platform": platform_name, "name": name, "path": path, "content": text});
            out.data(&value, || text.clone());
            Ok(())
        }
        Action::Install => {
            match platform {
                ServicePlatform::Systemd => {
                    let path = path.clone().expect("unit path");
                    std::fs::create_dir_all(path.parent().unwrap())
                        .and_then(|_| std::fs::write(&path, &text))
                        .map_err(|e| CliError::general("cli.io", e.to_string()))?;
                    run_tool("systemctl", &["--user", "daemon-reload"])?;
                    run_tool(
                        "systemctl",
                        &["--user", "enable", "--now", &format!("{}.service", name)],
                    )?;
                }
                ServicePlatform::Launchd => {
                    let path = path.clone().expect("plist path");
                    std::fs::create_dir_all(path.parent().unwrap())
                        .and_then(|_| std::fs::write(&path, &text))
                        .map_err(|e| CliError::general("cli.io", e.to_string()))?;
                    let _ = std::fs::create_dir_all(data_dir.join("logs"));
                    run_tool(
                        "launchctl",
                        &[
                            "bootstrap",
                            &format!("gui/{}", uid()?),
                            &path.to_string_lossy(),
                        ],
                    )?;
                }
                ServicePlatform::Windows => {
                    let tmp = std::env::temp_dir()
                        .join(format!("harness-task-{}.xml", std::process::id()));
                    std::fs::write(&tmp, utf16_with_bom(&text))
                        .map_err(|e| CliError::general("cli.io", e.to_string()))?;
                    let result = run_tool(
                        "schtasks",
                        &[
                            "/Create",
                            "/TN",
                            &name,
                            "/XML",
                            &tmp.to_string_lossy(),
                            "/F",
                        ],
                    );
                    let _ = std::fs::remove_file(&tmp);
                    result?;
                }
            }
            let mut value =
                json!({"platform": platform_name, "name": name, "path": path, "installed": true});
            if platform == ServicePlatform::Windows {
                value["hidden_window"] = json!(hidden);
            }
            out.data(&value, || {
                let mut text = format!(
                    "Installed {} ({}); `harness serve` starts at login.",
                    name, platform_name
                );
                if platform == ServicePlatform::Windows && !hidden {
                    text.push_str(&format!(
                        "\n{} was not found next to harness.exe: the task opens a console window.",
                        HIDDEN_LAUNCHER
                    ));
                }
                text
            });
            Ok(())
        }
        Action::Uninstall => {
            match platform {
                ServicePlatform::Systemd => {
                    let unit = format!("{}.service", name);
                    let _ = run_tool("systemctl", &["--user", "disable", "--now", &unit]);
                    if let Some(p) = &path {
                        if p.exists() {
                            std::fs::remove_file(p)
                                .map_err(|e| CliError::general("cli.io", e.to_string()))?;
                        } else {
                            return Err(CliError::not_found("service", &name));
                        }
                    }
                    run_tool("systemctl", &["--user", "daemon-reload"])?;
                }
                ServicePlatform::Launchd => {
                    let _ = run_tool(
                        "launchctl",
                        &["bootout", &format!("gui/{}/{}", uid()?, name)],
                    );
                    if let Some(p) = &path {
                        if p.exists() {
                            std::fs::remove_file(p)
                                .map_err(|e| CliError::general("cli.io", e.to_string()))?;
                        } else {
                            return Err(CliError::not_found("service", &name));
                        }
                    }
                }
                ServicePlatform::Windows => {
                    if run_tool("schtasks", &["/Query", "/TN", &name]).is_err() {
                        return Err(CliError::not_found("service", &name));
                    }
                    // A running instance is ended with the task.
                    let _ = run_tool("schtasks", &["/End", "/TN", &name]);
                    run_tool("schtasks", &["/Delete", "/TN", &name, "/F"])?;
                }
            }
            let value =
                json!({"platform": platform_name, "name": name, "path": path, "installed": false});
            out.data(&value, || format!("Uninstalled {}", name));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(exe: &str, args: &[&str]) -> ServiceSpec {
        ServiceSpec {
            name: "harness".into(),
            exe: PathBuf::from(exe),
            args: args.iter().map(|s| s.to_string()).collect(),
            log_file: Some(PathBuf::from(
                "/home/u/.local/share/com.harnessstudio.app/logs/harness-serve.log",
            )),
        }
    }

    #[test]
    fn systemd_unit_runs_serve_and_restarts_on_failure() {
        let unit = systemd_unit(&spec(
            "/opt/harness 1/harness",
            &["serve", "--data-dir", "/data/100% \"x\" $HOME"],
        ));
        assert!(unit.contains(
            "ExecStart=\"/opt/harness 1/harness\" \"serve\" \"--data-dir\" \"/data/100%% \\\"x\\\" $$HOME\"\n"
        ), "{unit}");
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.contains("KillSignal=SIGTERM\n"));
        assert!(unit.contains("[Install]\nWantedBy=default.target\n"));
        assert!(unit.starts_with("[Unit]\nDescription="));
    }

    #[test]
    fn launchd_plist_is_escaped_and_runs_at_load() {
        let mut s = spec("/Applications/Harness <&>.app/harness", &["serve"]);
        s.name = DEFAULT_LAUNCHD_LABEL.into();
        let plist = launchd_plist(&s);
        assert!(plist.contains("<string>com.harnessstudio.harness</string>"));
        assert!(plist.contains("<string>/Applications/Harness &lt;&amp;&gt;.app/harness</string>\n        <string>serve</string>"), "{plist}");
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n        <false/>"));
        assert!(plist.contains("<key>StandardErrorPath</key>"));
        assert!(plist.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn windows_task_has_a_logon_trigger_for_the_user() {
        let xml = windows_task_xml(
            &spec(
                r"C:\Program Files\Harness\harness.exe",
                &["serve", "--data-dir", r"C:\Data dir\x\"],
            ),
            r"PC\ugur & co",
        );
        assert!(xml.contains("<LogonTrigger>\r\n      <Enabled>true</Enabled>\r\n      <UserId>PC\\ugur &amp; co</UserId>"), "{xml}");
        assert!(xml.contains("<Command>C:\\Program Files\\Harness\\harness.exe</Command>"));
        // The trailing backslash is doubled so it does not escape the closing quote.
        assert!(
            xml.contains(r#"<Arguments>serve --data-dir &quot;C:\Data dir\x\\&quot;</Arguments>"#),
            "{xml}"
        );
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        let bytes = utf16_with_bom("<a/>");
        assert_eq!(&bytes[..4], &[0xFF, 0xFE, b'<', 0]);
    }

    #[test]
    fn hidden_launcher_is_used_only_when_installed() {
        let dir = tempfile::tempdir().unwrap();
        let harness = dir.path().join("harness.exe");
        std::fs::write(&harness, b"").unwrap();
        assert_eq!(hidden_launcher(&harness), None);
        std::fs::write(dir.path().join(HIDDEN_LAUNCHER), b"").unwrap();
        assert_eq!(
            hidden_launcher(&harness),
            Some(dir.path().join(HIDDEN_LAUNCHER))
        );
        let xml = windows_task_xml(
            &spec(
                &dir.path().join(HIDDEN_LAUNCHER).to_string_lossy(),
                &["serve"],
            ),
            "PC\\u",
        );
        assert!(xml.contains("harness-hidden.exe</Command>"), "{xml}");
        assert!(xml.contains("<Arguments>serve</Arguments>"));
    }

    #[test]
    fn windows_argument_quoting() {
        assert_eq!(win_quote("serve"), "serve");
        assert_eq!(win_quote("a b"), "\"a b\"");
        assert_eq!(win_quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(win_quote(r"C:\x y\"), r#""C:\x y\\""#);
        assert_eq!(win_quote(""), "\"\"");
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("harness", ServicePlatform::Systemd).is_ok());
        assert!(validate_name("HarnessStudio\\harness-serve", ServicePlatform::Windows).is_ok());
        assert!(validate_name("../x", ServicePlatform::Systemd).is_err());
        assert!(validate_name("a;rm", ServicePlatform::Systemd).is_err());
        assert!(validate_name("a\\b", ServicePlatform::Systemd).is_err());
    }
}
