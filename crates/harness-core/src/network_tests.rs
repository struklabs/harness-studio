use super::*;
use crate::params::{ParamValue, ParamValues};
use crate::process::{LaunchConfig, ProcessManager, ServerCaps};
use std::path::PathBuf;

fn settings_with(host: &str) -> AppSettings {
    let mut s = AppSettings::default();
    s.server.host = host.to_string();
    s
}

fn config(host: &str) -> LaunchConfig {
    LaunchConfig {
        binary_path: PathBuf::from("llama-server.exe"),
        model_path: PathBuf::from("models/test.gguf"),
        gpu_layers: 0,
        context_size: 512,
        host: host.to_string(),
        port: 18190,
        ..Default::default()
    }
}

#[test]
fn loopback_variants() {
    for h in [
        "127.0.0.1",
        "127.5.6.7",
        " 127.0.0.1 ",
        "localhost",
        "LocalHost",
        "::1",
        "[::1]",
        "::ffff:127.0.0.1",
    ] {
        assert!(is_loopback_host(h), "{h}");
        assert_eq!(bind_scope(h), Ok(BindScope::Loopback), "{h}");
    }
    for h in [
        "0.0.0.0",
        "::",
        "[::]",
        "192.168.1.20",
        "10.0.0.5",
        "172.16.3.4",
        "100.64.0.1",
        "fe80::1",
        "::ffff:192.168.1.2",
    ] {
        assert!(!is_loopback_host(h), "{h}");
        assert_eq!(bind_scope(h), Ok(BindScope::Lan), "{h}");
    }
}

#[test]
fn bind_host_validation() {
    assert_eq!(normalize_bind_host("localhost").unwrap(), "127.0.0.1");
    assert_eq!(normalize_bind_host(" 0.0.0.0 ").unwrap(), "0.0.0.0");
    assert_eq!(normalize_bind_host("[::1]").unwrap(), "::1");
    assert_eq!(normalize_bind_host("[::]").unwrap(), "::");
    for bad in [
        "",
        "   ",
        "myhost.local",
        "127.0.0.1,0.0.0.0",
        "127.0.0.1 0.0.0.0",
        "127.0.0.1:8080",
        "/tmp/llama.sock",
        "256.1.1.1",
        "[::1",
    ] {
        assert_eq!(
            normalize_bind_host(bad),
            Err(ERR_INVALID_HOST.to_string()),
            "{bad:?}"
        );
        assert!(!is_loopback_host(bad), "{bad:?} must not count as loopback");
    }
}

#[test]
fn exposure_matrix() {
    // Loopback: always fine.
    for (key, tools, ack) in [
        (false, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        assert_eq!(
            check_exposure("127.0.0.1", key, tools, ack),
            Ok(Exposure::Loopback)
        );
        assert_eq!(
            check_exposure("::1", key, tools, ack),
            Ok(Exposure::Loopback)
        );
    }
    for lan in ["0.0.0.0", "::", "192.168.1.20"] {
        // Key set: fine with or without tools.
        assert_eq!(
            check_exposure(lan, true, false, false),
            Ok(Exposure::LanWithKey)
        );
        assert_eq!(
            check_exposure(lan, true, true, false),
            Ok(Exposure::LanWithKey)
        );
        // No key, tools off: rejected unless acknowledged.
        assert_eq!(
            check_exposure(lan, false, false, false),
            Err(ERR_LAN_WITHOUT_KEY.to_string())
        );
        assert_eq!(
            check_exposure(lan, false, false, true),
            Ok(Exposure::LanAcknowledged)
        );
        // No key, tools/MCP on: rejected even when acknowledged.
        assert_eq!(
            check_exposure(lan, false, true, true),
            Err(ERR_TOOLS_WITHOUT_KEY.to_string())
        );
    }
    assert_eq!(
        check_exposure("evil,0.0.0.0", true, false, true),
        Err(ERR_INVALID_HOST.to_string())
    );
    // The tools-only helper keeps its old contract.
    assert!(crate::mcp::check_exposure("0.0.0.0", None, true).is_err());
    assert!(crate::mcp::check_exposure("0.0.0.0", Some(" "), true).is_err());
    assert!(crate::mcp::check_exposure("0.0.0.0", Some("k"), true).is_ok());
}

#[test]
fn prepare_launch_fills_default_host_and_applies_rules() {
    // Empty host takes the default bind address; loopback default leaves CORS to llama-server.
    let mut c = config("");
    assert_eq!(
        prepare_launch(&mut c, &settings_with("127.0.0.1"), "default"),
        Ok(Exposure::Loopback)
    );
    assert_eq!(c.host, "127.0.0.1");
    assert_eq!(c.cors_origins, None);

    // LAN default without a key: rejected with the i18n key.
    let lan = settings_with("0.0.0.0");
    let mut c = config("");
    assert_eq!(
        prepare_launch(&mut c, &lan, "default"),
        Err(ERR_LAN_WITHOUT_KEY.to_string())
    );

    // A profile's own host wins over the default.
    let mut c = config("localhost");
    assert_eq!(prepare_launch(&mut c, &lan, "p1"), Ok(Exposure::Loopback));
    assert_eq!(c.host, "127.0.0.1");

    // LAN with a key: browser origins closed by default.
    let mut c = config("");
    c.api_key = Some("k".into());
    assert_eq!(
        prepare_launch(&mut c, &lan, "default"),
        Ok(Exposure::LanWithKey)
    );
    assert_eq!(c.cors_origins.as_deref(), Some(CORS_DENY_ALL));

    // The acknowledgement is per server id and never comes from the caller.
    let mut acked = lan.clone();
    acked.network.exposure_acks.insert(
        "p1".into(),
        ExposureAck {
            acknowledged_at: 1,
            host: "0.0.0.0".into(),
        },
    );
    let mut c = config("");
    c.lan_exposure_ack = true;
    assert_eq!(
        prepare_launch(&mut c, &acked, "p2"),
        Err(ERR_LAN_WITHOUT_KEY.to_string())
    );
    let mut c = config("");
    assert_eq!(
        prepare_launch(&mut c, &acked, "p1"),
        Ok(Exposure::LanAcknowledged)
    );
    // ...but it never covers MCP.
    let mut c = config("");
    c.mcp_enabled = true;
    assert_eq!(
        prepare_launch(&mut c, &acked, "p1"),
        Err(ERR_TOOLS_WITHOUT_KEY.to_string())
    );

    // User origins are passed as given for any bind.
    let mut with_origins = settings_with("127.0.0.1");
    with_origins.network.cors_origins = vec!["http://a.example".into(), "localhost".into()];
    let mut c = config("");
    prepare_launch(&mut c, &with_origins, "default").unwrap();
    assert_eq!(
        c.cors_origins.as_deref(),
        Some("http://a.example,localhost")
    );

    // Invalid host.
    let mut c = config("0.0.0.0,127.0.0.1");
    assert_eq!(
        prepare_launch(&mut c, &settings_with("127.0.0.1"), "default"),
        Err(ERR_INVALID_HOST.to_string())
    );
}

#[test]
fn build_args_enforces_the_same_rule() {
    let caps = ServerCaps::default();
    // Direct ProcessManager use (no prepare_launch) is checked too.
    let err = ProcessManager::build_args(&config("0.0.0.0"), &caps).unwrap_err();
    assert_eq!(err, ERR_LAN_WITHOUT_KEY);
    let mut c = config("0.0.0.0");
    c.lan_exposure_ack = true;
    assert!(ProcessManager::build_args(&c, &caps).is_ok());
    c.mcp_enabled = true;
    c.mcp_config_path = Some(PathBuf::from("mcp.json"));
    assert_eq!(
        ProcessManager::build_args(&c, &caps).unwrap_err(),
        ERR_TOOLS_WITHOUT_KEY
    );
    let mut c = config("0.0.0.0");
    c.api_key = Some("secret".into());
    c.cors_origins = Some(CORS_DENY_ALL.into());
    let args = ProcessManager::build_args(&c, &caps).unwrap();
    let i = args.iter().position(|a| a == "--cors-origins").unwrap();
    assert_eq!(args[i + 1], CORS_DENY_ALL);
    let h = args.iter().position(|a| a == "--host").unwrap();
    assert_eq!(args[h + 1], "0.0.0.0");
    assert!(!args.iter().any(|a| a.contains("secret")));
}

#[test]
fn cors_flag_follows_runtime_support() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let help = std::fs::read_to_string(dir.join("llama-server-b11236.help.txt")).unwrap();
    let caps = ServerCaps::from_help(&help);
    assert!(caps.supports_cors_origins, "b11236 has --cors-origins");

    // A build without the flag: skipped and reported, never emitted.
    let old = ServerCaps {
        supports_cors_origins: false,
        ..caps
    };
    let mut c = config("127.0.0.1");
    c.cors_origins = Some("http://a.example".into());
    let out = ProcessManager::build_args_with(&c, &old, None).unwrap();
    assert!(!out.args.iter().any(|a| a == "--cors-origins"));
    assert!(out
        .skipped
        .iter()
        .any(|s| s.id == "cors_origins" && s.reason == "unsupported_by_runtime"));
}

#[test]
fn protected_flags_cannot_be_smuggled() {
    let caps = ServerCaps::default();
    for raw in [
        "--host 0.0.0.0",
        "--host=0.0.0.0",
        "--port 1",
        "--cors-origins *",
        "--cors-origins=*",
        "--cors_origins=*",
        "--api-key x",
        "--api_key=x",
        "--api-key-file k.txt",
        "--tools all",
        "--mcp-servers-json {}",
        "-ag",
        "--verbose \"--host\" 0.0.0.0",
    ] {
        let mut c = config("127.0.0.1");
        c.custom_args = Some(raw.to_string());
        assert!(
            ProcessManager::build_args(&c, &caps).is_err(),
            "custom args {raw:?} must be refused"
        );
    }
    // Catalog: cors_origins belongs to the network settings now; unknown ids never render.
    let mut c = config("127.0.0.1");
    let mut params = ParamValues::new();
    params.insert("cors_origins".into(), ParamValue::Text("*".into()));
    params.insert("host".into(), ParamValue::Text("0.0.0.0".into()));
    c.params = params;
    let help = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/llama-server-b11236.help.txt"),
    )
    .unwrap();
    let runtime =
        crate::capabilities::RuntimeCapabilities::from_outputs(Default::default(), "", &help, "");
    let out =
        ProcessManager::build_args_with(&c, &ServerCaps::from_help(&help), Some(&runtime)).unwrap();
    assert!(!out.args.iter().any(|a| a == "--cors-origins" || a == "*"));
    let reasons: std::collections::BTreeMap<_, _> = out
        .skipped
        .iter()
        .map(|s| (s.id.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(reasons["cors_origins"], "legacy_field");
    assert_eq!(reasons["host"], "unknown_param");
    assert_eq!(out.args.iter().filter(|a| *a == "--host").count(), 1);
    assert!(crate::params::is_protected("--cors-origins"));
    assert!(crate::params::is_protected("--cors_origins=x"));
}

#[test]
fn origin_validation() {
    assert_eq!(
        normalize_origin("http://a.example/").unwrap(),
        "http://a.example"
    );
    assert_eq!(
        normalize_origin(" HTTPS://App.Example:8443 ").unwrap(),
        "https://app.example:8443"
    );
    assert_eq!(
        normalize_origin("http://[::1]:3000").unwrap(),
        "http://[::1]:3000"
    );
    assert_eq!(normalize_origin("localhost").unwrap(), "localhost");
    assert_eq!(normalize_origin("*").unwrap(), "*");
    for bad in [
        "a.example",
        "ftp://a.example",
        "http://",
        "http://a.example/path",
        "http://a.example,http://b.example",
        "http://a b",
        "null",
    ] {
        assert_eq!(
            normalize_origin(bad),
            Err(ERR_INVALID_ORIGIN.to_string()),
            "{bad:?}"
        );
    }
    assert_eq!(
        normalize_origins(&[
            "http://a.example".into(),
            "".into(),
            "http://a.example/".into(),
            "localhost".into()
        ])
        .unwrap(),
        vec!["http://a.example", "localhost"]
    );
    assert_eq!(cors_arg(&[], BindScope::Loopback), None);
    assert_eq!(
        cors_arg(&[], BindScope::Lan).as_deref(),
        Some(CORS_DENY_ALL)
    );
    assert_eq!(
        cors_arg(&[" http://a.example ".into()], BindScope::Lan).as_deref(),
        Some("http://a.example")
    );
}

#[test]
fn worker_origins_are_the_window_and_exact_user_origins() {
    let app = APP_ORIGINS.join(",");
    assert_eq!(worker_origins(None), app);
    // llama-server values never open a worker to every page.
    assert_eq!(worker_origins(Some("*")), app);
    assert_eq!(worker_origins(Some(CORS_DENY_ALL)), app);
    assert_eq!(worker_origins(Some("localhost")), app);
    assert_eq!(
        worker_origins(Some(
            "http://a.example, https://b.example:8443,tauri://localhost"
        )),
        format!("{app},http://a.example,https://b.example:8443")
    );
    // The form a browser sends: lowercase, no trailing slash; wildcards and paths never match.
    assert_eq!(
        worker_origins(Some(
            "HTTP://Example.COM/,http://*,http://*.example,http://a.example/path,https://,http://:80,http://example.com"
        )),
        format!("{app},http://example.com")
    );
}

#[test]
fn sanitize_preserves_audio_cpp_consent_in_both_directions() {
    for enabled in [false, true] {
        let stored = AppSettings {
            audio_cpp_enabled: enabled,
            ..Default::default()
        };
        let mut incoming = AppSettings {
            audio_cpp_enabled: !enabled,
            ..Default::default()
        };
        sanitize_settings(&mut incoming, &stored).unwrap();
        assert_eq!(incoming.audio_cpp_enabled, enabled);
    }
}

#[test]
fn sanitize_keeps_stored_acks() {
    let mut stored = AppSettings::default();
    stored
        .network
        .exposure_acks
        .insert("x".into(), ExposureAck::default());
    let mut incoming = settings_with(" localhost ");
    incoming.network.cors_origins = vec!["http://a.example/".into()];
    incoming
        .network
        .exposure_acks
        .insert("forged".into(), ExposureAck::default());
    sanitize_settings(&mut incoming, &stored).unwrap();
    assert_eq!(incoming.server.host, "127.0.0.1");
    assert_eq!(incoming.network.cors_origins, vec!["http://a.example"]);
    assert_eq!(
        incoming.network.exposure_acks.keys().collect::<Vec<_>>(),
        vec!["x"]
    );
    let mut bad = settings_with("my-host");
    assert_eq!(
        sanitize_settings(&mut bad, &stored),
        Err(ERR_INVALID_HOST.to_string())
    );
}

#[test]
fn base64url_vectors() {
    // RFC 4648 §10 vectors (URL alphabet, no padding) + the two URL-specific characters.
    assert_eq!(base64url(b""), "");
    assert_eq!(base64url(b"f"), "Zg");
    assert_eq!(base64url(b"fo"), "Zm8");
    assert_eq!(base64url(b"foo"), "Zm9v");
    assert_eq!(base64url(b"foob"), "Zm9vYg");
    assert_eq!(base64url(b"fooba"), "Zm9vYmE");
    assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64url(&[0xfb, 0xff, 0xbf]), "-_-_");
}

#[test]
fn generated_keys_are_strong_and_url_safe() {
    let a = generate_api_key().unwrap();
    let b = generate_api_key().unwrap();
    assert_eq!(a.len(), 43, "32 bytes -> 43 base64url chars");
    assert!(a
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    assert_ne!(a, b);
    // No byte value dominates across many keys (sanity check of the OS random source).
    let mut counts = [0u32; 64];
    for _ in 0..200 {
        for c in generate_api_key().unwrap().chars() {
            let idx = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
                .find(c)
                .unwrap();
            counts[idx] += 1;
        }
    }
    assert!(counts.iter().all(|&n| n > 40 && n < 300), "{counts:?}");
}

#[test]
fn virtual_and_non_lan_addresses_are_filtered() {
    assert!(is_lan_candidate("192.168.1.20".parse().unwrap()));
    assert!(is_lan_candidate("10.1.2.3".parse().unwrap()));
    for ip in [
        "127.0.0.1",
        "169.254.10.2",
        "0.0.0.0",
        "255.255.255.255",
        "224.0.0.1",
    ] {
        assert!(!is_lan_candidate(ip.parse().unwrap()), "{ip}");
    }
    for name in [
        "vEthernet (WSL (Hyper-V firewall))",
        "VirtualBox Host-Only Network",
        "VMware Network Adapter VMnet8",
        "docker0",
        "veth12ab",
        "br-5f2c",
        "virbr0",
        "lo",
        "Loopback Pseudo-Interface 1",
    ] {
        assert!(looks_virtual(name), "{name}");
    }
    for name in [
        "Wi-Fi",
        "Ethernet",
        "en0",
        "eth0",
        "wlan0",
        "Intel(R) Wi-Fi 6 AX201 160MHz",
    ] {
        assert!(!looks_virtual(name), "{name}");
    }
}

/// Real interface enumeration: every reported address is a usable LAN IPv4 address, and the
/// address the OS routes outbound traffic from (UDP connect, no packet sent) is among them
/// when it is a LAN candidate on a non-virtual interface.
#[test]
fn lan_addresses_from_real_interfaces() {
    let list = lan_ipv4_addresses();
    for a in &list {
        let ip: Ipv4Addr = a.ip.parse().unwrap();
        assert!(is_lan_candidate(ip), "{a:?}");
        assert!(!looks_virtual(&a.interface), "{a:?}");
    }
    let all = platform_ipv4_interfaces();
    let primary = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
        .and_then(|s| s.local_addr())
        .ok()
        .and_then(|a| match a.ip() {
            IpAddr::V4(v4) => Some(v4),
            _ => None,
        });
    eprintln!("lan: {list:?}\nall: {all:?}\nprimary: {primary:?}");
    if let Some(p) = primary.filter(|p| is_lan_candidate(*p)) {
        let iface = all.iter().find(|(ip, _, _)| *ip == p);
        assert!(iface.is_some(), "primary address {p} not enumerated");
        let (_, name, desc) = iface.unwrap();
        if !looks_virtual(name) && !looks_virtual(desc) {
            assert!(list.iter().any(|a| a.ip == p.to_string()), "{p} missing");
        }
    }
}

// --- End-to-end against the real llama-server ---
//
// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<.gguf> cargo test -p harness-core network::tests::e2e -- --ignored --test-threads=1

fn e2e_env() -> Option<(PathBuf, PathBuf)> {
    match (
        std::env::var("HS_LLAMA_SERVER"),
        std::env::var("HS_TEST_MODEL"),
    ) {
        (Ok(bin), Ok(model)) => Some((bin.into(), model.into())),
        _ => {
            eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
            None
        }
    }
}

fn e2e_config(bin: &std::path::Path, model: &std::path::Path, port: u16) -> LaunchConfig {
    LaunchConfig {
        binary_path: bin.to_path_buf(),
        model_path: model.to_path_buf(),
        gpu_layers: 0,
        context_size: 512,
        host: String::new(),
        port,
        ..Default::default()
    }
}

async fn wait_running(pm: &ProcessManager) {
    let t = std::time::Instant::now();
    while pm.get_status() != crate::process::ServerStatus::Running {
        assert!(
            t.elapsed() < std::time::Duration::from_secs(120),
            "{:?}\n{}",
            pm.get_status(),
            pm.get_logs(Some(30)).join("\n")
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

async fn stop(pm: &ProcessManager, port: u16) {
    pm.stop().await.unwrap();
    assert!(!pm.has_child());
    assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());
}

#[tokio::test]
#[ignore]
async fn e2e_lan_bind_without_key_is_rejected() {
    let Some((bin, model)) = e2e_env() else {
        return;
    };
    let port = 18191;
    let settings = settings_with(LAN_BIND_HOST);
    let mut c = e2e_config(&bin, &model, port);
    assert_eq!(
        prepare_launch(&mut c, &settings, "e2e-lan"),
        Err(ERR_LAN_WITHOUT_KEY.to_string())
    );
    // Even a caller that skips prepare_launch can't start it.
    c.host = LAN_BIND_HOST.into();
    let pm = ProcessManager::with_id("e2e-lan-nokey");
    assert_eq!(pm.start(c).await.unwrap_err(), ERR_LAN_WITHOUT_KEY);
    assert!(!pm.has_child());
    assert!(crate::servers::ensure_port_free("0.0.0.0", port).is_ok());
}

#[tokio::test]
#[ignore]
async fn e2e_lan_bind_with_key_requires_authorization() {
    let Some((bin, model)) = e2e_env() else {
        return;
    };
    let port = 18192;
    let key = generate_api_key().unwrap();
    let settings = settings_with(LAN_BIND_HOST);
    let mut c = e2e_config(&bin, &model, port);
    c.api_key = Some(key.clone());
    assert_eq!(
        prepare_launch(&mut c, &settings, "e2e-lan"),
        Ok(Exposure::LanWithKey)
    );
    assert_eq!(c.cors_origins.as_deref(), Some(CORS_DENY_ALL));
    let pm = ProcessManager::with_id("e2e-lan-key");
    pm.start(c).await.unwrap();
    let result = async {
        wait_running(&pm).await;
        let client = reqwest::Client::new();
        let mut hosts = vec!["127.0.0.1".to_string()];
        // Reachable through the LAN address too (same auth rules).
        hosts.extend(lan_ipv4_addresses().into_iter().take(1).map(|a| a.ip));
        for host in &hosts {
            let url = format!("http://{host}:{port}/v1/models");
            let anon = client.get(&url).send().await.unwrap();
            assert_eq!(anon.status().as_u16(), 401, "{url} without key");
            let wrong = client.get(&url).bearer_auth("wrong").send().await.unwrap();
            assert_eq!(wrong.status().as_u16(), 401, "{url} wrong key");
            let ok = client.get(&url).bearer_auth(&key).send().await.unwrap();
            assert_eq!(ok.status().as_u16(), 200, "{url} with key");
        }
        // No browser origin is allowed by default on a LAN bind.
        let res = client
            .get(format!("http://127.0.0.1:{port}/v1/models"))
            .header("Origin", "http://evil.example")
            .bearer_auth(&key)
            .send()
            .await
            .unwrap();
        let acao = res
            .headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap_or("").to_string());
        assert_ne!(acao.as_deref(), Some("http://evil.example"));
        assert_ne!(acao.as_deref(), Some("*"));
        hosts
    }
    .await;
    eprintln!("checked hosts: {result:?}");
    stop(&pm, port).await;
}

#[tokio::test]
#[ignore]
async fn e2e_cors_allowed_origin() {
    let Some((bin, model)) = e2e_env() else {
        return;
    };
    let caps = ServerCaps::detect(&bin);
    if !caps.supports_cors_origins {
        eprintln!("runtime has no --cors-origins, skipping");
        return;
    }
    let port = 18193;
    let mut settings = settings_with(DEFAULT_BIND_HOST);
    settings.network.cors_origins = vec!["http://allowed.example".into()];
    let mut c = e2e_config(&bin, &model, port);
    assert_eq!(
        prepare_launch(&mut c, &settings, "e2e-cors"),
        Ok(Exposure::Loopback)
    );
    let pm = ProcessManager::with_id("e2e-cors");
    pm.start(c).await.unwrap();
    wait_running(&pm).await;
    let client = reqwest::Client::new();
    let acao = |origin: &'static str| {
        let client = client.clone();
        async move {
            client
                .get(format!("http://127.0.0.1:{port}/v1/models"))
                .header("Origin", origin)
                .send()
                .await
                .unwrap()
                .headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap().to_string())
        }
    };
    assert_eq!(
        acao("http://allowed.example").await.as_deref(),
        Some("http://allowed.example")
    );
    // Any other page gets a header that doesn't name it (the browser blocks the response).
    let other = acao("http://evil.example").await;
    assert_ne!(other.as_deref(), Some("http://evil.example"));
    assert_ne!(other.as_deref(), Some("*"));
    let active = pm.active_server().unwrap();
    assert_eq!(
        active.cors_origins.as_deref(),
        Some("http://allowed.example")
    );
    assert!(!active.exposure_acknowledged);
    stop(&pm, port).await;
}

#[test]
fn browser_origin_gate() {
    let list = vec!["http://localhost:3000".to_string()];
    // No Origin: curl, SDKs, CLI.
    assert_eq!(check_browser_origin(None, None, &list), Ok(()));
    assert_eq!(check_browser_origin(None, Some("none"), &list), Ok(()));
    assert_eq!(
        check_browser_origin(None, Some("same-origin"), &list),
        Ok(())
    );
    assert_eq!(
        check_browser_origin(None, Some("cross-site"), &list),
        Err(OriginDenied::CrossSite)
    );
    // Harness's own window and the user's list (case and trailing slash do not matter).
    for o in [
        "tauri://localhost",
        "http://tauri.localhost",
        "http://localhost:1420",
    ] {
        assert_eq!(
            check_browser_origin(Some(o), Some("cross-site"), &[]),
            Ok(()),
            "{o}"
        );
    }
    for o in ["http://localhost:3000", "HTTP://LocalHost:3000/"] {
        assert_eq!(
            check_browser_origin(Some(o), Some("cross-site"), &list),
            Ok(()),
            "{o}"
        );
    }
    // Everything else.
    assert_eq!(
        check_browser_origin(Some("null"), None, &list),
        Err(OriginDenied::Null)
    );
    for o in [
        "https://evil.example",
        "http://localhost:3001",
        "http://127.0.0.1:3000",
        "",
    ] {
        assert_eq!(
            check_browser_origin(Some(o), None, &list),
            Err(OriginDenied::NotAllowed),
            "{o}"
        );
    }
}

#[test]
fn public_tool_dns_answers_reject_every_non_public_address() {
    let public: SocketAddr = "8.8.8.8:443".parse().unwrap();
    assert!(check_public_addresses("public.example", &[public]).is_ok());
    assert!(check_public_addresses("empty.example", &[]).is_err());
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "169.254.1.1",
        "100.64.0.1",
        "100.127.255.254",
        "0.0.0.0",
        "224.0.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "64:ff9b::a00:1",
    ] {
        let private = SocketAddr::new(ip.parse().unwrap(), 443);
        assert!(
            check_public_addresses("rebind.example", &[public, private]).is_err(),
            "{ip}"
        );
    }
}
