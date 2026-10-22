//! Real headless instances with no port overrides and separate temporary data roots.
mod common;

use common::{run, Serve};
use std::collections::HashSet;
use std::time::Duration;

static LISTENERS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn isolated_dir() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("isolated-listeners"))
        .tempdir()
        .unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    // Enable the extra compatibility listener, deliberately omitting all ports.
    std::fs::write(
        dir.path().join("config/settings.json"),
        r#"{"schema_version":3,"gateway":{"ollama_port_enabled":true}}"#,
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn two_isolated_instances_keep_every_listener_off_default_ports() {
    let _listeners = LISTENERS.lock().await;
    let first_dir = isolated_dir();
    let second_dir = isolated_dir();
    let mut first = Serve::start_auto(first_dir.path(), &["--gateway", "--no-restore"]);
    let mut second = Serve::start_auto(second_dir.path(), &["--gateway", "--no-restore"]);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let mut ports = HashSet::new();
    for serve in [&first, &second] {
        let ollama = serve.ready["gateway"]["ollama_port"]
            .as_u64()
            .unwrap_or_else(|| panic!("Ollama listener missing: {}", serve.ready))
            as u16;
        for port in [serve.port, serve.gateway_port(), ollama] {
            assert!(
                harness_core::ports::ISOLATED_AUTO_RANGE.contains(&port),
                "{port}"
            );
            assert!(![11434, 11435, 11440, 11500].contains(&port));
            assert!(ports.insert(port), "shared listener port {port}");
            assert!(common::port_open(port), "listener {port} is unreachable");
        }
        let data_dir = if serve.port == first.port {
            first_dir.path()
        } else {
            second_dir.path()
        };
        let info = harness_core::instance::read_info(data_dir).unwrap();
        assert_eq!(info.api.unwrap().port, serve.port);
        let state = harness_core::ports::PortState::load(data_dir);
        assert_eq!(state.gateway_history.last(), Some(&serve.gateway_port()));
        assert_eq!(state.control_api_history.last(), Some(&serve.port));
        let api = serve.ready["base_url"].as_str().unwrap();
        assert!(http
            .get(format!("{api}/health"))
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        for port in [serve.gateway_port(), ollama] {
            assert!(http
                .get(format!("http://127.0.0.1:{port}/health"))
                .send()
                .await
                .unwrap()
                .status()
                .is_success());
        }
        // MCP shares the Control API listener, including its authentication guard.
        assert_eq!(
            http.post(format!("{api}/mcp"))
                .json(&serde_json::json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
    }
    eprintln!("two isolated instances: six distinct live listener ports {ports:?}");
    for (dir, serve) in [(&first_dir, &mut first), (&second_dir, &mut second)] {
        let stopped = run(dir.path(), &["shutdown", "--json"]);
        assert_eq!(stopped.code, 0, "{}", stopped.stderr);
        assert_eq!(serve.wait_exit(Duration::from_secs(15)).0, 0);
    }
}

#[test]
fn gateway_overrides_take_precedence_over_an_implicit_api_port() {
    let _listeners = LISTENERS.blocking_lock();
    for saved in [false, true] {
        let dir = isolated_dir();
        let settings = if saved {
            serde_json::json!({"schema_version":3,"gateway":{"port":12441}})
        } else {
            serde_json::json!({"schema_version":3})
        };
        std::fs::write(
            dir.path().join("config/settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let extra: &[&str] = if saved {
            &["--gateway", "--no-restore"]
        } else {
            &["--gateway", "--no-restore", "--gateway-port", "12441"]
        };
        let mut serve = Serve::start_auto(dir.path(), extra);
        assert_eq!(serve.gateway_port(), 12441);
        assert_ne!(serve.port, 12441);
        assert!(harness_core::ports::ISOLATED_AUTO_RANGE.contains(&serve.port));
        assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
        assert_eq!(serve.wait_exit(Duration::from_secs(15)).0, 0);
    }
}

#[test]
fn an_explicit_ollama_port_is_not_silently_replaced() {
    let _listeners = LISTENERS.blocking_lock();
    let dir = isolated_dir();
    let held = (12470..=12499)
        .find_map(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok())
        .unwrap();
    let port = held.local_addr().unwrap().port();
    std::fs::write(
        dir.path().join("config/settings.json"),
        serde_json::json!({"schema_version":3,"gateway":{
            "ollama_port_enabled":true,"ollama_port":port
        }})
        .to_string(),
    )
    .unwrap();
    let mut serve = Serve::start_auto(dir.path(), &["--gateway", "--no-restore"]);
    assert!(serve.ready["gateway"]["ollama_port"].is_null());
    assert!(serve.ready["gateway"]["ollama_warning"]
        .as_str()
        .unwrap()
        .contains(&port.to_string()));
    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    assert_eq!(serve.wait_exit(Duration::from_secs(15)).0, 0);
}

#[test]
fn an_explicit_ollama_port_precedes_the_gateway_default() {
    let _listeners = LISTENERS.blocking_lock();
    let dir = isolated_dir();
    std::fs::write(
        dir.path().join("config/settings.json"),
        r#"{"schema_version":3,"gateway":{"ollama_port_enabled":true,"ollama_port":12440}}"#,
    )
    .unwrap();
    let mut serve = Serve::start_auto(dir.path(), &["--gateway", "--no-restore"]);
    assert_ne!(serve.gateway_port(), 12440);
    assert_eq!(serve.ready["gateway"]["ollama_port"], 12440);
    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    assert_eq!(serve.wait_exit(Duration::from_secs(15)).0, 0);
}

fn explicit_ports(avoid: &[u16]) -> (u16, u16) {
    let listeners: Vec<_> = (12470..=12499)
        .filter(|p| !avoid.contains(p))
        .filter_map(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok())
        .take(2)
        .collect();
    assert_eq!(listeners.len(), 2, "two free isolated test ports");
    (
        listeners[0].local_addr().unwrap().port(),
        listeners[1].local_addr().unwrap().port(),
    )
}

#[test]
fn isolated_instances_honor_configured_and_command_line_ports() {
    let _listeners = LISTENERS.blocking_lock();
    let dir = isolated_dir();
    let (api, gateway) = explicit_ports(&[]);
    let settings = serde_json::json!({"schema_version":3,
        "control_api":{"port":api}, "gateway":{"port":gateway}});
    std::fs::write(
        dir.path().join("config/settings.json"),
        settings.to_string(),
    )
    .unwrap();
    let mut serve = Serve::start_auto(dir.path(), &["--gateway", "--no-restore"]);
    assert_eq!(serve.port, api);
    assert_eq!(serve.gateway_port(), gateway);
    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    assert_eq!(serve.wait_exit(Duration::from_secs(15)).0, 0);
    let (override_api, override_gateway) = explicit_ports(&[api, gateway]);
    let mut serve = Serve::start_with(
        dir.path(),
        override_api,
        &[
            "--gateway",
            "--no-restore",
            "--gateway-port",
            &override_gateway.to_string(),
        ],
    );
    assert_eq!(serve.port, override_api);
    assert_eq!(serve.gateway_port(), override_gateway);
    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    assert_eq!(serve.wait_exit(Duration::from_secs(15)).0, 0);
}
