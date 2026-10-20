//! `harness tools list` / `harness tools run` against the real binary (a temporary instance per
//! command, temp data dirs): the policy gate, the approval question, the working directory.

mod common;

use std::path::Path;

use common::{cmd, data_dir, run, Run};
use serde_json::{json, Value};

/// Writes `settings.tools` into the temp data dir's settings.json.
fn set_policy(dir: &Path, policy: &str, trusted: &[&str]) {
    let path = dir.join("config/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    settings["tools"] = json!({"policy": policy, "trusted": trusted});
    std::fs::write(&path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
}

fn work() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.txt"), "hello\n").unwrap();
    d
}

fn tools_run(dir: &Path, work: &Path, extra: &[&str]) -> Run {
    let mut args = vec!["tools", "run"];
    args.extend_from_slice(extra);
    let w = work.to_string_lossy().to_string();
    args.extend_from_slice(&["--workdir", &w]);
    run(dir, &args)
}

#[test]
fn list_shows_class_source_and_the_policys_decision() {
    let dir = data_dir(None, 18500);
    let r = run(dir.path(), &["tools", "list", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["policy"], "ask_every_time");
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 14);
    assert!(tools.iter().all(|t| t["source"] == "builtin"));
    let skill = tools.iter().find(|t| t["name"] == "load_skill").unwrap();
    assert_eq!(skill["class"], "safe");
    assert_eq!(skill["decision"], "run");
    assert_eq!(
        tools.iter().find(|t| t["name"] == "calculator").unwrap()["decision"],
        "run"
    );
    assert_eq!(
        tools
            .iter()
            .find(|t| t["name"] == "exec_shell_command")
            .unwrap()["class"],
        "dangerous"
    );

    set_policy(dir.path(), "allow_trusted", &["read_file"]);
    let r = run(dir.path(), &["tools", "ls"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stdout
            .starts_with("Tool policy: allow_trusted (trusted: read_file)"),
        "{}",
        r.stdout
    );
    let line = |name: &str| {
        r.stdout
            .lines()
            .find(|l| l.starts_with(name))
            .unwrap()
            .to_string()
    };
    assert!(line("read_file").contains("restricted") && line("read_file").contains("run"));
    assert!(line("write_file").contains("dangerous") && line("write_file").contains("ask"));
    let skill_line = line("load_skill");
    assert!(
        skill_line.contains("safe") && skill_line.contains("builtin") && skill_line.contains("run"),
        "{skill_line}"
    );
    assert!(r.stdout.contains("NAME") && r.stdout.contains("POLICY"));
}

#[test]
fn search_provider_is_configured_through_the_control_api() {
    let dir = data_dir(None, 18510);
    let r = run(
        dir.path(),
        &[
            "tools",
            "search-provider",
            "--provider",
            "searxng",
            "--url",
            "https://search.example.org",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("searxng"));
    let r = run(dir.path(), &["tools", "search-provider", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["provider"], "searxng");
    assert_eq!(v["searxng_url"], "https://search.example.org");
    assert!(v.get("api_key").is_none());
    let r = run(
        dir.path(),
        &[
            "tools",
            "search-provider",
            "--provider",
            "duckduckgo",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.json()["provider"], "duckduckgo");
    assert_eq!(r.json()["api_key_set"], false);
}

#[test]
fn a_tool_the_policy_asks_about_needs_yes_without_a_terminal() {
    let dir = data_dir(None, 18500);
    let w = work();
    let write_args = r#"{"path": "w.txt", "content": "x"}"#;

    // No terminal, no --yes: refused (exit 2), nothing written.
    let r = tools_run(
        dir.path(),
        w.path(),
        &["write_file", "--args", write_args, "--json"],
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert_eq!(r.error()["code"], "cli.confirmation_required");
    assert!(!w.path().join("w.txt").exists());

    // --yes: the approval is given, the tool runs.
    let r = tools_run(
        dir.path(),
        w.path(),
        &["write_file", "--args", write_args, "--yes", "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["tool"], "write_file");
    assert!(v["error"].is_null());
    assert_eq!(
        std::fs::read_to_string(w.path().join("w.txt")).unwrap(),
        "x"
    );

    // Human output is the tool's text only.
    let r = tools_run(
        dir.path(),
        w.path(),
        &["read_file", "--args", r#"{"path":"a.txt"}"#, "-y"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "hello\n");
}

#[test]
fn trusted_and_allow_all_need_no_question_and_off_refuses_even_with_yes() {
    let dir = data_dir(None, 18500);
    let w = work();
    set_policy(dir.path(), "allow_trusted", &["read_file"]);
    let r = tools_run(
        dir.path(),
        w.path(),
        &["read_file", "--args", r#"{"path":"a.txt"}"#],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "hello\n");
    // Not trusted: still asks.
    let r = tools_run(
        dir.path(),
        w.path(),
        &[
            "exec_shell_command",
            "--args",
            r#"{"command":"echo hi","timeout":60}"#,
            "--json",
        ],
    );
    assert_eq!(r.code, 2);

    set_policy(dir.path(), "allow_all", &[]);
    let r = tools_run(
        dir.path(),
        w.path(),
        &[
            "exec_shell_command",
            "--args",
            r#"{"command":"echo from-cli","timeout":60}"#,
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stdout.contains("from-cli") && r.stdout.contains("[exit code: 0]"),
        "{}",
        r.stdout
    );

    set_policy(dir.path(), "off", &[]);
    let r = tools_run(
        dir.path(),
        w.path(),
        &[
            "read_file",
            "--args",
            r#"{"path":"a.txt"}"#,
            "--yes",
            "--json",
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert_eq!(r.error()["code"], "tools.disabled");
}

#[test]
fn failures_usage_errors_and_the_default_working_directory() {
    let dir = data_dir(None, 18500);
    let w = work();
    set_policy(dir.path(), "allow_all", &[]);

    // The tool ran and failed: exit 1, the sentence on stderr, the result on stdout with --json.
    let r = tools_run(
        dir.path(),
        w.path(),
        &[
            "read_file",
            "--args",
            r#"{"path":"../outside.txt"}"#,
            "--json",
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert_eq!(r.json()["error"], "outside_workspace");
    assert_eq!(r.error()["code"], "cli.tool_failed");
    let r = tools_run(
        dir.path(),
        w.path(),
        &["read_file", "--args", r#"{"path":"nope.txt"}"#],
    );
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("cannot stat file: nope.txt"),
        "{}",
        r.stderr
    );

    // Usage errors: not JSON, not an object; an unknown tool is "not found".
    let r = tools_run(
        dir.path(),
        w.path(),
        &["read_file", "--args", "{nope", "--json"],
    );
    assert_eq!(
        (r.code, r.error()["code"].as_str().unwrap().to_string()),
        (2, "cli.invalid_argument".into())
    );
    let r = tools_run(
        dir.path(),
        w.path(),
        &["read_file", "--args", "[1]", "--json"],
    );
    assert_eq!(r.code, 2);
    let r = tools_run(dir.path(), w.path(), &["no_such_tool", "--json"]);
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert_eq!(r.error()["code"], "cli.not_found");

    // Without --workdir the current directory is the working directory.
    let out = cmd(dir.path())
        .current_dir(w.path())
        .args(["tools", "run", "read_file", "--args", r#"{"path":"a.txt"}"#])
        .output()
        .unwrap();
    let r: Run = out.into();
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "hello\n");
}
