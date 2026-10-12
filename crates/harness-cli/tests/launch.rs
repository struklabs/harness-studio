//! `harness launch` end to end with a real llama-server, model and gateway (ports 19120+):
//!
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-cli --test launch
//!   -- --ignored --nocapture --test-threads=1
//!
//! The real coding agents run only when opted in, each with a temporary config directory so the
//! user's own account config is never read or changed:
//!   HS_TEST_LAUNCH_CLAUDE=1  Claude Code (`claude` on PATH, temp CLAUDE_CONFIG_DIR)
//!   HS_TEST_LAUNCH_CODEX=1   Codex CLI (`codex` on PATH, temp CODEX_HOME)

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{cmd, data_dir, free_port, llama_server, run, test_model, Serve};
use harness_core::integrations::conntest::test_connection;
use harness_core::integrations::launch::{find_program, WireApi};

/// `harness` with this test's data dir and none of the caller's Claude / Codex / OpenAI settings.
fn clean(dir: &Path) -> Command {
    let mut c = cmd(dir);
    for (k, _) in std::env::vars() {
        if ["CLAUDE", "ANTHROPIC", "CODEX", "OPENAI"]
            .iter()
            .any(|p| k.starts_with(p))
        {
            c.env_remove(k);
        }
    }
    c
}

fn opted_in(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| v == "1")
}

#[test]
#[ignore]
fn e2e_launch_dry_run_and_real_agents() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(19120));
    let api_port = free_port(19140);
    let gw_port = free_port(19160);
    let mut serve = Serve::start_with(
        dir.path(),
        api_port,
        &["--gateway", "--gateway-port", &gw_port.to_string()],
    );
    assert_eq!(serve.ready["gateway"]["running"], true, "{}", serve.ready);
    let model = model.to_str().unwrap();
    let gw_host = format!("http://127.0.0.1:{gw_port}");

    // Dry run: managed endpoint, the token redacted, nothing started.
    let r = run(
        dir.path(),
        &["--json", "launch", "claude", "--model", model, "--dry-run"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    eprintln!("dry run: {v}");
    assert_eq!(v["endpoint"]["kind"], "managed");
    assert_eq!(v["endpoint"]["base_url"], format!("{gw_host}/v1"));
    let env = &v["plan"]["env"];
    assert_eq!(env["ANTHROPIC_BASE_URL"], gw_host);
    assert_eq!(env["ANTHROPIC_MODEL"], v["model"]);
    let alias = v["model"].as_str().unwrap().to_string();
    // The gateway client token (if any) never shows up in output.
    if let Ok(token) = std::fs::read_to_string(dir.path().join("gateway-token")) {
        let token = token.trim();
        if !token.is_empty() {
            assert!(!r.stdout.contains(token) && !r.stderr.contains(token));
            assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "***");
        }
    }

    for tool in ["codex", "opencode", "aider", "goose"] {
        let r = run(
            dir.path(),
            &["--json", "launch", tool, "--model", &alias, "--dry-run"],
        );
        assert_eq!(r.code, 0, "{tool}: {}", r.stderr);
        let v = r.json();
        assert_eq!(v["tool"], tool);
        assert_eq!(v["plan"]["tool"], tool);
        eprintln!("{tool}: {}", v["plan"]);
    }

    // A missing tool is reported (exit 3) instead of crashing.
    if find_program("goose").is_none() {
        let r = run(
            dir.path(),
            &[
                "--json",
                "launch",
                "goose",
                "--model",
                &alias,
                "--no-prepare",
            ],
        );
        assert_eq!(r.code, 3, "{}", r.stderr);
        assert_eq!(r.error()["code"], "cli.launch_not_installed");
    }

    let scratch = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("launch-agents"))
        .tempdir()
        .unwrap();

    if opted_in("HS_TEST_LAUNCH_CLAUDE") && find_program("claude").is_some() {
        let config = scratch.path().join("claude-config");
        std::fs::create_dir_all(&config).unwrap();
        let out = clean(dir.path())
            .env("CLAUDE_CONFIG_DIR", &config)
            .current_dir(scratch.path())
            .args([
                "launch",
                "claude",
                "--model",
                &alias,
                "--prepare",
                "--",
                "-p",
                "Reply with the single word PONG.",
            ])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        eprintln!(
            "claude: {:?} {stdout}\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(0));
        assert!(stdout.to_uppercase().contains("PONG"));
        // Process-only: nothing Harness-specific was written into the tool's config dir.
        let settings = config.join("settings.json");
        if settings.exists() {
            assert!(!std::fs::read_to_string(settings)
                .unwrap()
                .contains("ANTHROPIC_BASE_URL"));
        }
    } else {
        eprintln!("Claude Code: skipped (HS_TEST_LAUNCH_CLAUDE=1 and `claude` on PATH needed)");
    }

    if opted_in("HS_TEST_LAUNCH_CODEX") && find_program("codex").is_some() {
        let home = scratch.path().join("codex-home");
        std::fs::create_dir_all(&home).unwrap();
        let out = clean(dir.path())
            .env("CODEX_HOME", &home)
            .current_dir(scratch.path())
            .args([
                "launch",
                "codex",
                "--model",
                &alias,
                "--prepare",
                "--",
                "exec",
                "--skip-git-repo-check",
                "Reply with the single word PONG.",
            ])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        eprintln!(
            "codex: {:?} {stdout}\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(0));
        assert!(stdout.to_uppercase().contains("PONG"));
        let config = home.join("config.toml");
        if config.exists() {
            assert!(!std::fs::read_to_string(config)
                .unwrap()
                .contains("model_providers.harness"));
        }
    } else {
        eprintln!("Codex: skipped (HS_TEST_LAUNCH_CODEX=1 and `codex` on PATH needed)");
    }

    // "Bağlantıyı test et": one small tool-enabled request in each API, through the gateway.
    let rt = tokio::runtime::Runtime::new().unwrap();
    for api in [WireApi::Anthropic, WireApi::Responses, WireApi::Chat] {
        let r = rt.block_on(test_connection(
            api,
            &format!("{gw_host}/v1"),
            None,
            &alias,
            "harness-launch-test",
        ));
        eprintln!("test_connection {api:?}: {r:?}");
        assert!(r.ok, "{api:?}: {}", r.detail);
        assert_eq!(r.status, Some(200));
    }

    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    let (code, _) = serve.wait_exit(Duration::from_secs(60));
    assert_eq!(code, 0);
}
