//! The real `harness` binary: `--json` schemas and exit codes, temporary instances, the
//! single-owner lock and `harness serve`. No llama-server needed (see `e2e.rs` for that).

mod common;

use std::time::Duration;

use common::{data_dir, free_port, run, Serve};
use serde_json::json;

#[test]
fn help_version_and_usage_errors() {
    let dir = data_dir(None, 18500);
    let r = run(dir.path(), &["--version"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("harness"));
    let r = run(dir.path(), &["--help"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("Exit codes"));
    let r = run(dir.path(), &["bogus"]);
    assert_eq!(r.code, 2);
    let r = run(dir.path(), &["--json", "bogus"]);
    assert_eq!(r.code, 2);
    assert_eq!(r.error()["code"], "cli.usage");
    assert_eq!(r.error()["exit_code"], 2);
    // Invalid values are usage errors too.
    let r = run(dir.path(), &["--json", "pull", "not-a-repo"]);
    assert_eq!(r.code, 2);
    assert_eq!(r.error()["code"], "cli.invalid_argument");
    let r = run(dir.path(), &["completion", "bash"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("harness"));
}

#[test]
fn library_commands_run_a_temporary_instance_and_release_the_lock() {
    let dir = data_dir(None, 18500);
    let r = run(dir.path(), &["ls", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert!(v["entries"].as_array().unwrap().is_empty());
    assert!(v["definitions"].is_array());
    assert!(v.get("state_error").is_some());
    // The temporary owner cleaned up: no instance.json, no session token file, lock free.
    assert!(!dir.path().join("instance.json").exists());
    assert!(!dir.path().join("api-token").exists());
    let status = run(dir.path(), &["status", "--json"]).json();
    assert_eq!(status["owner"]["state"], "none");
    assert!(status["api"].is_null());

    let r = run(dir.path(), &["show", "missing-model", "--json"]);
    assert_eq!(r.code, 3);
    assert_eq!(r.error()["code"], "cli.not_found");
    let r = run(dir.path(), &["rm", "missing-model", "--json"]);
    assert_eq!(r.code, 3);

    let r = run(dir.path(), &["ps", "--all", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.json().is_array());
    let r = run(dir.path(), &["benchmark", "ls", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.json()["runs"].is_array());
    let r = run(dir.path(), &["benchmark", "show", "nope", "--json"]);
    assert_eq!(r.code, 3);
    assert_eq!(r.error()["code"], "api.not_found");
}

#[test]
fn server_commands_need_an_instance_or_headless() {
    let dir = data_dir(None, 18500);
    for args in [
        vec!["load", "x", "--json"],
        vec!["run", "x", "hi", "--json"],
        vec!["stop", "--all", "--json"],
        vec!["logs", "default", "--json"],
        vec!["unload", "x", "--json"],
        vec!["shutdown", "--json"],
        vec!["profile", "run", "x", "--json"],
    ] {
        let r = run(dir.path(), &args);
        assert_eq!(r.code, 6, "{args:?}: {}", r.stderr);
        assert_eq!(r.error()["code"], "cli.no_instance", "{args:?}");
    }
    // --headless: a temporary instance; the unknown model is "not found".
    let r = run(
        dir.path(),
        &["--headless", "load", "no-such-model", "--json"],
    );
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert!(!dir.path().join("instance.json").exists());
}

#[test]
fn api_tokens_offline() {
    let dir = data_dir(None, 18500);
    let r = run(
        dir.path(),
        &[
            "api-token",
            "create",
            "ci",
            "--scope",
            "read,models:write",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let created = r.json();
    let token = created["token"].as_str().unwrap();
    assert!(token.starts_with("hs_"));
    let id = created["info"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["info"]["scopes"], json!(["read", "models:write"]));

    let list = run(dir.path(), &["api-token", "ls", "--json"]).json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(
        !list.to_string().contains(&token[20..]),
        "values are never listed"
    );

    let r = run(
        dir.path(),
        &["api-token", "create", "x", "--scope", "root", "--json"],
    );
    assert_eq!(r.code, 2);
    // Destructive without a terminal needs --yes.
    let r = run(dir.path(), &["api-token", "rm", &id, "--json"]);
    assert_eq!(r.code, 2);
    assert_eq!(r.error()["code"], "cli.confirmation_required");
    let r = run(dir.path(), &["api-token", "rm", &id, "--yes", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let r = run(dir.path(), &["api-token", "rm", &id, "--yes", "--json"]);
    assert_eq!(r.code, 3);
    assert_eq!(r.error()["code"], "api.token_not_found");
}

#[test]
fn token_policy_flags_show_and_set() {
    let dir = data_dir(None, 18500);
    let r = run(
        dir.path(),
        &[
            "token",
            "create",
            "agent",
            "--scope",
            "inference",
            "--model",
            "qwen3-*,llama-*",
            "--endpoint",
            "chat",
            "--endpoint",
            "models",
            "--rpm",
            "60",
            "--concurrency",
            "2",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let created = r.json();
    let id = created["info"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["info"]["models"], json!(["qwen3-*", "llama-*"]));
    assert_eq!(created["info"]["endpoints"], json!(["chat", "models"]));
    assert_eq!(created["info"]["limits"]["requests_per_minute"], 60);
    assert_eq!(created["info"]["limits"]["max_concurrent"], 2);

    // Human listing shows the policy columns.
    let r = run(dir.path(), &["api-token", "ls"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    for col in ["MODELS", "ENDPOINTS", "LIMITS", "qwen3-*"] {
        assert!(r.stdout.contains(col), "{col}: {}", r.stdout);
    }

    // Only the flags given change; 0 clears a limit.
    let r = run(
        dir.path(),
        &[
            "token",
            "set",
            &id,
            "--tpm",
            "5000",
            "--rpm",
            "0",
            "--scope",
            "inference,read",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let info = run(dir.path(), &["token", "show", &id, "--json"]).json();
    assert_eq!(info["name"], "agent");
    assert_eq!(info["scopes"], json!(["read", "inference"]));
    assert_eq!(info["models"], json!(["qwen3-*", "llama-*"]));
    assert!(info["limits"]["requests_per_minute"].is_null(), "{info}");
    assert_eq!(info["limits"]["tokens_per_minute"], 5000);
    assert_eq!(info["limits"]["max_concurrent"], 2);

    // `all` / `*` lift the restrictions.
    let r = run(
        dir.path(),
        &[
            "token",
            "set",
            &id,
            "--endpoint",
            "all",
            "--model",
            "*",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let info = r.json();
    assert_eq!(info["endpoints"], json!([]));
    assert_eq!(info["models"], json!([]));

    // Bad values are usage errors.
    let r = run(
        dir.path(),
        &["token", "set", &id, "--endpoint", "files", "--json"],
    );
    assert_eq!(r.code, 2);
    let r = run(
        dir.path(),
        &["token", "set", &id, "--model", "bad pattern/x y", "--json"],
    );
    assert_ne!(r.code, 0);
    let r = run(dir.path(), &["token", "show", "nope", "--json"]);
    assert_eq!(r.code, 3);
    assert_eq!(r.error()["code"], "api.token_not_found");
}

/// One-shot HTTP receiver: answers the first request with `status` and returns what it read.
fn http_once(status: u16) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = sock.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let len = text[..end]
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len || n == 0 {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let reason = if status == 200 { "OK" } else { "Unauthorized" };
        write!(
            sock,
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/x-protobuf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        String::from_utf8_lossy(&buf).to_string()
    });
    (url, handle)
}

#[test]
fn otel_status_and_test_export() {
    let dir = data_dir(None, 18500);
    let r = run(dir.path(), &["otel", "status", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["settings"]["enabled"], false);
    assert_eq!(v["status"]["built"], true);

    // A real HTTP receiver gets one protobuf POST to /v1/traces.
    let (url, got) = http_once(200);
    let r = run(dir.path(), &["otel", "test", "--endpoint", &url, "--json"]);
    assert_eq!(r.code, 0, "{} {}", r.stdout, r.stderr);
    let v = r.json();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["url"], format!("{url}/v1/traces"));
    let request = got.join().unwrap();
    assert!(request.starts_with("POST /v1/traces"), "{request}");
    assert!(request
        .to_ascii_lowercase()
        .contains("application/x-protobuf"));

    // The collector refuses: exit code 1 with the reason.
    let (url, got) = http_once(401);
    let r = run(dir.path(), &["otel", "test", "--endpoint", &url]);
    assert_eq!(r.code, 1, "{} {}", r.stdout, r.stderr);
    assert!(
        format!("{}{}", r.stdout, r.stderr).contains("401"),
        "{} {}",
        r.stdout,
        r.stderr
    );
    got.join().unwrap();

    // Unknown protocol: usage error.
    let r = run(
        dir.path(),
        &[
            "otel",
            "test",
            "--endpoint",
            &url,
            "--protocol",
            "udp",
            "--json",
        ],
    );
    assert_eq!(r.code, 2);

    // gRPC only in `otel-grpc` builds: `status` says which protocols are built in, and a
    // build without it refuses gRPC with a clear usage error before sending anything.
    let r = run(dir.path(), &["otel", "status", "--json"]);
    let grpc = r.json()["status"]["grpc"].as_bool();
    assert_eq!(grpc, Some(harness_core::otel::grpc_built()));
    let r = run(dir.path(), &["otel", "status"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("http/protobuf"), "{}", r.stdout);
    if grpc == Some(false) {
        let r = run(
            dir.path(),
            &["otel", "test", "--endpoint", &url, "--protocol", "grpc"],
        );
        assert_eq!(r.code, 2, "{} {}", r.stdout, r.stderr);
        assert!(
            format!("{}{}", r.stdout, r.stderr).contains("without OTLP/gRPC"),
            "{} {}",
            r.stdout,
            r.stderr
        );
    }
}

#[test]
fn profile_import_export_roundtrip() {
    let dir = data_dir(None, 18500);
    let doc = json!({
        "format": "harness-profile",
        "version": 1,
        "profile": {"id": "cli-roundtrip", "name": "Roundtrip", "description": "test",
            "config": {"model_path": "/models/m.gguf", "host": "127.0.0.1", "port": 18544,
                       "gpu_layers": 0, "context_size": 2048, "api_key_ref": "profile:elsewhere"}},
        "definition": {"id": "md-other-machine", "alias": "roundtrip-model",
            "base": {"kind": "local", "path": "/models/m.gguf"}},
    });
    let file = dir.path().join("profile.json");
    std::fs::write(&file, doc.to_string()).unwrap();
    // An invalid profile fails without leaving its new definition behind.
    let mut broken = doc.clone();
    broken["profile"]["config"]
        .as_object_mut()
        .unwrap()
        .remove("gpu_layers");
    let bad = dir.path().join("broken.json");
    std::fs::write(&bad, broken.to_string()).unwrap();
    let r = run(
        dir.path(),
        &["profile", "import", bad.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert_eq!(r.error()["code"], "api.invalid_body");
    let defs = run(dir.path(), &["ls", "--json"]).json();
    assert!(defs["definitions"].as_array().unwrap().is_empty(), "{defs}");

    let r = run(
        dir.path(),
        &["profile", "import", file.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let created = r.json();
    assert_eq!(created["id"], "cli-roundtrip");
    assert_eq!(created["is_profile"], true);
    assert_eq!(created["served_alias"], "roundtrip-model");

    let r = run(
        dir.path(),
        &["profile", "import", file.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.code, 4, "same id again is a conflict: {}", r.stderr);

    let list = run(dir.path(), &["profile", "ls", "--json"]).json();
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "cli-roundtrip"));
    let shown = run(dir.path(), &["profile", "show", "cli-roundtrip", "--json"]).json();
    assert_eq!(shown["profile"]["config"]["port"], 18544);

    let r = run(dir.path(), &["profile", "export", "cli-roundtrip"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let exported = r.json();
    assert_eq!(exported["format"], "harness-profile");
    assert_eq!(exported["definition"]["alias"], "roundtrip-model");
    assert!(exported["profile"]["config"].get("api_key_ref").is_none());
    let r = run(dir.path(), &["profile", "show", "nope", "--json"]);
    assert_eq!(r.code, 3);

    // Into a fresh data dir: the definition is created there.
    let other = data_dir(None, 18500);
    let out = other.path().join("p.json");
    std::fs::write(&out, exported.to_string()).unwrap();
    let r = run(
        other.path(),
        &["profile", "import", out.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let show = run(other.path(), &["show", "roundtrip-model", "--json"]).json();
    assert_eq!(show["definitions"][0]["alias"], "roundtrip-model");
    assert!(show["model"].is_null(), "file is not in this library");
}

#[test]
fn serve_owns_the_directory_and_commands_use_its_api() {
    let dir = data_dir(None, 18500);
    let port = free_port(18600);
    let mut serve = Serve::start(dir.path(), port);
    assert_eq!(serve.ready["event"], "ready");
    assert_eq!(serve.ready["kind"], "headless");
    let base = serve.ready["base_url"].as_str().unwrap().to_string();
    assert!(base.contains(&format!(":{}/api/harness/v1", port)));
    assert!(dir.path().join("instance.json").exists());
    assert!(dir.path().join("api-token").exists());

    // Discovery through instance.json.
    let status = run(dir.path(), &["status", "--json"]).json();
    assert_eq!(status["via"], "instance");
    assert_eq!(status["owner"]["state"], "running");
    assert_eq!(status["api"]["instance"]["kind"], "headless");
    assert_eq!(status["api"]["api_version"], "v1");
    let pid = serve.child.id();
    assert_eq!(status["api"]["instance"]["pid"], pid);

    // Changes go through the owner's API (the token is created in its process).
    let r = run(
        dir.path(),
        &["api-token", "create", "remote", "--scope", "read", "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let token = r.json()["token"].as_str().unwrap().to_string();

    // A second owner is refused (exit 4), with the owner's pid.
    let second = common::cmd(dir.path())
        .args(["serve", "--json", "--port", &free_port(18650).to_string()])
        .output()
        .unwrap();
    let second = common::Run::from(second);
    assert_eq!(second.code, 4, "{}", second.stderr);
    assert_eq!(second.error()["code"], "cli.lock_held");
    assert_eq!(second.error()["params"]["pid"], pid);

    // The same port from another data dir: the bind fails (conflict).
    let other = data_dir(None, 18500);
    let r = common::cmd(other.path())
        .args(["serve", "--json", "--port", &port.to_string()])
        .output()
        .unwrap();
    let r = common::Run::from(r);
    assert_eq!(r.code, 4, "{}", r.stderr);
    assert_eq!(r.error()["code"], "api.bind_failed");

    // Remote access with a named token: scopes apply.
    let remote = |args: &[&str], token: &str| {
        let mut c = common::cmd(other.path());
        c.env("HARNESS_HOST", format!("127.0.0.1:{}", port))
            .env("HARNESS_TOKEN", token)
            .args(args);
        common::Run::from(c.output().unwrap())
    };
    let r = remote(&["ps", "--all", "--json"], &token);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.json().is_array());
    let r = remote(&["api-token", "ls", "--json"], &token);
    assert_eq!(r.code, 1);
    assert_eq!(r.error()["code"], "api.insufficient_scope");
    let r = remote(&["ps", "--json"], "hs_wrong_token");
    assert_eq!(r.code, 1);
    assert_eq!(r.error()["code"], "api.unauthorized");
    let r = common::Run::from(
        common::cmd(other.path())
            .args([
                "--host",
                &format!("127.0.0.1:{}", free_port(18700)),
                "--token",
                "x",
            ])
            .args(["ps", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(r.code, 6);
    let error = r.error();
    assert_eq!(error["code"], "http.connect_failed_local");
    assert!(error["params"]["transport_detail"].is_string());
    let message = error["message"].as_str().unwrap();
    assert!(message.contains(error["params"]["url"].as_str().unwrap()));
    assert!(!message.contains("error sending request"));
    let r = remote(&["shutdown", "--json"], &token);
    assert_eq!(r.code, 1, "a read token cannot shut the instance down");

    // Clean shutdown through the API.
    let r = run(dir.path(), &["shutdown", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let (code, last) = serve.wait_exit(Duration::from_secs(20));
    assert_eq!(code, 0);
    let last: serde_json::Value = serde_json::from_str(&last.unwrap()).unwrap();
    assert_eq!(last["event"], "stopped");
    assert_eq!(last["reason"], "api");
    assert!(!dir.path().join("instance.json").exists());
    assert!(!dir.path().join("api-token").exists());
    assert!(!common::port_open(port));
}

#[test]
fn doctor_and_status_only_read() {
    let dir = data_dir(None, 18500);
    let r = run(dir.path(), &["doctor", "--json"]);
    assert!(r.code == 0 || r.code == 1, "{}", r.stderr);
    let v = r.json();
    let ids: Vec<&str> = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for id in [
        "data_dir",
        "instance",
        "runtime",
        "gpu_libraries",
        "hardware",
        "ports",
    ] {
        assert!(ids.contains(&id), "{ids:?}");
    }
    for c in v["checks"].as_array().unwrap() {
        assert!(["ok", "warn", "fail"].contains(&c["status"].as_str().unwrap()));
        assert!(c["message_key"].as_str().unwrap().starts_with("doctor."));
        assert!(c["params"].is_object());
        assert!(!c["summary"].as_str().unwrap().is_empty());
    }
    assert_eq!(v["ok"], r.code == 0);
    assert!(
        !dir.path().join("instance.lock").exists(),
        "doctor takes no lock"
    );
    let r = run(dir.path(), &["status", "--json"]);
    assert_eq!(r.code, 0);
    assert!(
        !dir.path().join("instance.lock").exists(),
        "status takes no lock"
    );
}

#[test]
fn service_definitions_print_for_every_platform() {
    let dir = data_dir(None, 18500);
    for (platform, needle) in [
        ("systemd", "ExecStart="),
        ("launchd", "<key>ProgramArguments</key>"),
        ("windows", "<LogonTrigger>"),
    ] {
        let r = run(
            dir.path(),
            &["service", "print", "--platform", platform, "--json"],
        );
        assert_eq!(r.code, 0, "{}", r.stderr);
        let v = r.json();
        assert_eq!(v["platform"], platform);
        let content = v["content"].as_str().unwrap();
        assert!(content.contains(needle), "{content}");
        assert!(content.contains("serve"));
        // HARNESS_DATA_DIR is passed on to the service.
        assert!(content.contains("--data-dir"), "{content}");
    }
    let r = run(
        dir.path(),
        &["service", "print", "--name", "../x", "--json"],
    );
    assert_eq!(r.code, 2);
}

#[test]
fn launch_needs_a_model_and_rejects_unknown_tools() {
    let dir = data_dir(None, 18500);
    // Unknown tool: usage error.
    let r = run(dir.path(), &["--json", "launch", "vim"]);
    assert_eq!(r.code, 2, "{}", r.stderr);
    // No definitions yet: nothing to launch with (dry run only reads the library).
    let r = run(dir.path(), &["--json", "launch", "claude", "--dry-run"]);
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert_eq!(r.error()["code"], "cli.no_definitions");
    // An unknown model name.
    let r = run(
        dir.path(),
        &[
            "--json",
            "launch",
            "codex",
            "--model",
            "missing-model",
            "--dry-run",
        ],
    );
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert_eq!(r.error()["code"], "cli.not_found");
    // --prepare and --no-prepare conflict.
    let r = run(
        dir.path(),
        &["launch", "aider", "--prepare", "--no-prepare", "--dry-run"],
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    // The temporary owner cleaned up.
    assert!(!dir.path().join("instance.json").exists());
}

/// `harness import modelfile`: dry run, real import, relative paths from the Modelfile's
/// directory, and errors (no llama-server needed).
#[test]
fn import_modelfile_creates_definition_and_preset() {
    let dir = data_dir(None, 18500);
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("m.gguf"), b"GGUF   ").unwrap();
    let file = work.path().join("Modelfile");
    std::fs::write(
        &file,
        "FROM ./m.gguf
PARAMETER num_ctx 4096
PARAMETER temperature 0.3
PARAMETER stop \"<end>\"
SYSTEM Answer in one word.
TEMPLATE \"{{ .Prompt }}\"
",
    )
    .unwrap();
    let f = file.to_str().unwrap();

    let r = run(
        dir.path(),
        &["import", "modelfile", f, "--dry-run", "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["import"]["model_found"], true);
    assert_eq!(v["warnings"][0]["code"], "template_ignored");
    let ls = run(dir.path(), &["ls", "--json"]).json();
    assert!(
        ls["definitions"].as_array().unwrap().is_empty(),
        "dry run saves nothing"
    );

    let r = run(
        dir.path(),
        &["import", "modelfile", f, "--alias", "one-word", "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["definition"]["alias"], "one-word");
    assert_eq!(v["definition"]["load"]["context_size"], 4096);
    let ls = run(dir.path(), &["ls", "--json"]).json();
    assert_eq!(ls["definitions"][0]["alias"], "one-word");

    let r = run(
        dir.path(),
        &["import", "modelfile", f, "--alias", "one-word", "--json"],
    );
    assert_ne!(r.code, 0, "alias is taken");
    let r = run(
        dir.path(),
        &[
            "import",
            "modelfile",
            "--ollama",
            "no-such-model:1b",
            "--json",
        ],
    );
    assert_ne!(r.code, 0);
    let r = run(dir.path(), &["import", "modelfile", f, "--ollama", "x"]);
    assert_eq!(r.code, 2, "FILE and --ollama conflict");
}

/// `runtime managed status|rollback` through a temporary instance: nothing installed, no
/// backup (rollback refused with a conflict exit code, nothing removed).
#[test]
fn runtime_managed_status_and_rollback_offline() {
    let dir = data_dir(None, 18560);
    let r = run(dir.path(), &["runtime", "managed", "status", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert!(v["backup"].is_null(), "{v}");
    assert!(v["download"].is_null(), "{v}");
    let r = run(
        dir.path(),
        &["runtime", "managed", "rollback", "--yes", "--json"],
    );
    assert_ne!(r.code, 0);
    assert_eq!(r.error()["code"], "runtime.no_backup", "{}", r.stderr);
}
