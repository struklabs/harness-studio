//! End to end with a real llama-server and model (ports 18500+):
//!
//! (`e2e_serve_gateway_run_uses_managed_endpoint` uses 18800+ and a gateway on 18897+.)
//!
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-cli --test e2e
//!   -- --ignored --nocapture --test-threads=1
//!
//! `e2e_out_of_memory_exit_code` additionally needs HS_TEST_OOM=1 (it asks the GPU for more
//! memory than a 16 GB card has; takes ~40 s).

mod common;

use std::io::Write;
use std::process::Stdio;
use std::time::Duration;

use common::{data_dir, free_port, llama_server, run, test_model, Serve};
use serde_json::{json, Value};

fn endpoint_port(endpoint: &str) -> u16 {
    endpoint
        .rsplit(':')
        .next()
        .and_then(|p| p.split('/').next())
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port in {endpoint}"))
}

#[test]
#[ignore]
fn e2e_serve_run_chat_logs_benchmark_unload() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18500));
    let mut serve = Serve::start(dir.path(), free_port(18560));
    let model = model.to_str().unwrap();

    // One-shot run by file path: a definition is created, a server loaded.
    let r = run(
        dir.path(),
        &[
            "run",
            model,
            "Say hello in three words.",
            "--max-tokens",
            "16",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let first = r.json();
    eprintln!("run: {}", first["content"]);
    assert!(!first["content"].as_str().unwrap().is_empty());
    assert!(first["usage"]["completion_tokens"].as_u64().unwrap() > 0);
    assert_eq!(first["loaded"], true);
    let alias = first["model"].as_str().unwrap().to_string();
    let server_id = first["server_id"].as_str().unwrap().to_string();
    let port = endpoint_port(first["endpoint"].as_str().unwrap());
    assert!(port >= 18500, "server port {port}");

    // The alias now resolves to the running server (no second load).
    let r = run(
        dir.path(),
        &[
            "run",
            &alias,
            "2+2=? Reply with the digit only.",
            "--max-tokens",
            "4",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.json()["loaded"], false);
    assert_eq!(r.json()["server_id"], server_id.as_str());

    let ps = run(dir.path(), &["ps", "--json"]).json();
    let running: Vec<&Value> = ps.as_array().unwrap().iter().collect();
    assert_eq!(running.len(), 1, "{ps}");
    assert_eq!(running[0]["status"], "Running");
    assert_eq!(running[0]["served_alias"], alias.as_str());

    // Chat keeps the conversation: two turns from stdin, one JSON line each.
    let mut child = common::cmd(dir.path())
        .args(["chat", &alias, "--max-tokens", "24", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"My name is Ugur.\nWhat is my name? Answer with the name only.\n/bye\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let turns: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(turns.len(), 2);
    eprintln!("chat: {} / {}", turns[0]["content"], turns[1]["content"]);
    assert!(turns[1]["content"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("ugur"));

    let logs = run(dir.path(), &["logs", &server_id, "-n", "5", "--json"]);
    assert_eq!(logs.code, 0, "{}", logs.stderr);
    let logs = logs.json();
    assert!(logs["lines"].as_array().unwrap().len() <= 5);
    assert!(logs["next"].as_u64().unwrap() > 0);

    // llama-bench next to the llama-server.
    let bench_name = if cfg!(windows) {
        "llama-bench.exe"
    } else {
        "llama-bench"
    };
    if bin.with_file_name(bench_name).is_file() {
        let r = run(
            dir.path(),
            &[
                "benchmark",
                "run",
                &alias,
                "--n-prompt",
                "64",
                "--n-gen",
                "8",
                "--repetitions",
                "1",
                "--json",
            ],
        );
        assert_eq!(r.code, 0, "{}", r.stderr);
        let b = r.json();
        assert_eq!(b["run"]["status"], "completed");
        assert!(!b["run"]["results"].as_array().unwrap().is_empty());
        let id = b["id"].as_str().unwrap();
        let listed = run(dir.path(), &["benchmark", "ls", "--json"]).json();
        assert!(listed["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == id));
        let shown = run(dir.path(), &["benchmark", "show", id, "--json"]);
        assert_eq!(shown.code, 0);
    } else {
        eprintln!("no {bench_name} next to the llama-server; benchmark skipped");
    }

    let r = run(dir.path(), &["unload", &alias, "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.json()["stopped"], json!([server_id]));
    assert!(run(dir.path(), &["ps", "--json"])
        .json()
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!common::port_open(port));

    // Load again (reuses the stopped instance), then a clean shutdown stops it.
    let r = run(dir.path(), &["load", &alias, "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let loaded = r.json();
    assert_eq!(loaded["server_id"], server_id.as_str());
    assert_eq!(loaded["server"]["status"], "Running");
    assert!(common::port_open(port));
    let r = run(dir.path(), &["shutdown", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let (code, _) = serve.wait_exit(Duration::from_secs(60));
    assert_eq!(code, 0);
    assert!(!common::port_open(port), "shutdown stops the llama-servers");
    assert!(!dir.path().join("instance.json").exists());
}

#[test]
#[ignore]
fn e2e_headless_run_cleans_up() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18520));
    let r = run(
        dir.path(),
        &[
            "--headless",
            "run",
            model.to_str().unwrap(),
            "Say OK.",
            "--max-tokens",
            "8",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert!(!v["content"].as_str().unwrap().is_empty());
    let port = endpoint_port(v["endpoint"].as_str().unwrap());
    assert!(
        !common::port_open(port),
        "the temporary instance stopped its server"
    );
    assert!(!dir.path().join("instance.json").exists());
    assert!(!dir.path().join("api-token").exists());
}

#[test]
#[ignore]
fn e2e_out_of_memory_exit_code() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    if std::env::var("HS_TEST_OOM").as_deref() != Ok("1") {
        eprintln!("HS_TEST_OOM=1 not set, skipping");
        return;
    }
    let dir = data_dir(Some(&bin), free_port(18700));
    let doc = json!({
        "format": "harness-profile", "version": 1,
        "profile": {"id": "oom", "name": "OOM", "config": {"model_path": model, "host": "127.0.0.1",
            "port": free_port(18710), "gpu_layers": 99, "context_size": 262144, "parallel_slots": 1}},
        "definition": {"alias": "oom-model", "base": {"kind": "local", "path": model}},
    });
    let file = dir.path().join("oom.json");
    std::fs::write(&file, doc.to_string()).unwrap();
    assert_eq!(
        run(dir.path(), &["profile", "import", file.to_str().unwrap()]).code,
        0
    );
    let r = run(
        dir.path(),
        &["--headless", "profile", "run", "oom", "--json"],
    );
    eprintln!("{}", r.stderr.lines().last().unwrap_or_default());
    assert_eq!(r.code, 5, "{}", r.stderr);
    let err = r.error();
    assert_eq!(err["code"], "servers.out_of_memory");
    assert_eq!(err["params"]["failure"]["params"]["stage"], "kv_cache");
}

/// Adds `server.<key> = value` to the settings of a test data dir.
fn set_server_setting(dir: &std::path::Path, key: &str, value: Value) {
    let path = dir.join("config/settings.json");
    let mut s: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    s["server"][key] = value;
    std::fs::write(&path, s.to_string()).unwrap();
}

/// Kills the llama-server listening on `port` (a server this test started).
fn kill_server_on_port(port: u16) {
    #[cfg(windows)]
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "$p = (Get-NetTCPConnection -State Listen -LocalPort {port}).OwningProcess | \
                 Select-Object -First 1; if ((Get-Process -Id $p).ProcessName -ne 'llama-server') \
                 {{ exit 3 }}; Stop-Process -Id $p -Force"
            ),
        ])
        .status()
        .unwrap();
    #[cfg(unix)]
    let status = std::process::Command::new("sh")
        .args([
            "-c",
            &format!("kill -9 $(lsof -t -iTCP:{port} -sTCP:LISTEN)"),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "could not kill the server on {port}");
}

fn server_view(dir: &std::path::Path, id: &str) -> Value {
    let r = run(dir, &["ps", "-a", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.json()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("no server {id}"))
}

/// OOM → recovery options → `harness retry --option` → running. Needs HS_TEST_OOM=1.
#[test]
#[ignore]
fn e2e_oom_recovery_retry() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    if std::env::var("HS_TEST_OOM").as_deref() != Ok("1") {
        eprintln!("HS_TEST_OOM=1 not set, skipping");
        return;
    }
    let dir = data_dir(Some(&bin), free_port(18720));
    let mut serve = Serve::start(dir.path(), free_port(18730));
    let doc = json!({
        "format": "harness-profile", "version": 1,
        "profile": {"id": "oom", "name": "OOM", "config": {"model_path": model, "host": "127.0.0.1",
            "port": free_port(18740), "gpu_layers": 99, "context_size": 262144, "parallel_slots": 1,
            // Explicit f16 (no automatic KV policy) and `--fit off`: otherwise Harness / llama-server
            // shrink the request and nothing runs out of memory.
            "cache_type_k": "f16", "cache_type_v": "f16", "flash_attn": true,
            "params": {"fit": "off"}}},
        "definition": {"alias": "oom-model", "base": {"kind": "local", "path": model}},
    });
    let file = dir.path().join("oom.json");
    std::fs::write(&file, doc.to_string()).unwrap();
    assert_eq!(
        run(dir.path(), &["profile", "import", file.to_str().unwrap()]).code,
        0
    );
    let r = run(dir.path(), &["profile", "run", "oom", "--json"]);
    assert_eq!(r.code, 5, "{}", r.stderr);
    let err = r.error();
    assert_eq!(err["code"], "servers.out_of_memory");
    assert_eq!(err["params"]["failure"]["kind"], "out_of_memory");
    let hints = err["params"]["recovery"].as_array().unwrap();
    assert!(!hints.is_empty(), "{err}");

    // The plan on its own; human output lists the options.
    let r = run(dir.path(), &["retry", "oom", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let plan = r.json();
    eprintln!("plan: {}", serde_json::to_string_pretty(&plan).unwrap());
    let options = plan["options"].as_array().unwrap();
    let ids: Vec<&str> = options.iter().filter_map(|o| o["id"].as_str()).collect();
    assert!(ids.contains(&"lower_context"), "{ids:?}");
    assert!(ids.contains(&"enable_fit"), "b11236 has --fit: {ids:?}");
    let text = run(dir.path(), &["retry", "oom"]);
    assert!(text.stdout.contains("lower_context"), "{}", text.stdout);

    // Retry with the lower context the plan computed: a real start that must run.
    let r = run(
        dir.path(),
        &["retry", "oom", "--option", "lower_context", "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let view = r.json();
    assert_eq!(view["status"], "Running", "{view}");
    assert!(view["failure"].is_null());
    let lowered = options.iter().find(|o| o["id"] == "lower_context").unwrap()["overrides"]
        ["context_size"]
        .as_u64()
        .unwrap();
    let eff = run(dir.path(), &["ps", "-a", "--json"]);
    assert!(eff.stdout.contains("\"Running\""));
    eprintln!("running with context {lowered}");
    // Not saved: the profile still asks for 262144.
    let show = run(dir.path(), &["profile", "show", "oom", "--json"]);
    assert!(show.stdout.contains("262144"), "{}", show.stdout);
    assert_eq!(run(dir.path(), &["stop", "oom"]).code, 0);

    // --fit: llama-server picks the layers itself (-ngl not sent), the context stays.
    assert_eq!(
        run(dir.path(), &["profile", "run", "oom", "--json"]).code,
        5
    );
    let r = run(
        dir.path(),
        &[
            "retry",
            "oom",
            "--option",
            "enable_fit",
            "--set",
            "context_size=65536",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let logs = run(dir.path(), &["logs", "oom", "--lines", "400"]);
    // This run only (the ring buffer also holds the failed runs before it).
    let last_run = logs
        .stdout
        .rsplit("[Harness Studio] Starting llama-server")
        .next()
        .unwrap();
    assert!(
        !last_run.contains("n_gpu_layers already set by user"),
        "fit must choose the layers: {last_run}"
    );
    assert!(last_run.contains("n_ctx_slot = 65536"), "{last_run}");
    assert_eq!(run(dir.path(), &["stop", "oom"]).code, 0);

    assert_eq!(run(dir.path(), &["shutdown"]).code, 0);
    serve.wait_exit(Duration::from_secs(30));
}

/// A crash of a running server with auto restart on: `restart` state, then running again.
#[test]
#[ignore]
fn e2e_crash_auto_restart() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18750));
    set_server_setting(dir.path(), "auto_restart", json!(true));
    set_server_setting(dir.path(), "restart_max_attempts", json!(2));
    let mut serve = Serve::start(dir.path(), free_port(18760));
    let r = run(dir.path(), &["load", model.to_str().unwrap(), "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let loaded = r.json();
    let id = loaded["server_id"].as_str().unwrap().to_string();
    let port = endpoint_port(loaded["server"]["endpoint"].as_str().unwrap());

    kill_server_on_port(port);
    // Crash detected, restart scheduled (attempt 1/2 after 2 s)...
    let t = std::time::Instant::now();
    let pending = loop {
        let v = server_view(dir.path(), &id);
        if !v["restart"].is_null() {
            break v;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "no restart: {v}");
        std::thread::sleep(Duration::from_millis(100));
    };
    eprintln!("pending: {}", pending["restart"]);
    assert_eq!(pending["restart"]["attempt"], 1);
    assert_eq!(pending["restart"]["max_attempts"], 2);
    assert_eq!(pending["failure"]["kind"], "crashed");
    // ...and running again.
    let t = std::time::Instant::now();
    loop {
        let v = server_view(dir.path(), &id);
        if v["status"] == "Running" {
            assert!(v["restart"].is_null());
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(90), "not restarted: {v}");
        std::thread::sleep(Duration::from_millis(250));
    }
    let logs = run(dir.path(), &["logs", &id, "--lines", "400"]);
    assert!(logs
        .stdout
        .contains("Otomatik yeniden başlatma (deneme 1/2)"));

    // A stop during the back-off cancels the restart.
    kill_server_on_port(port);
    let t = std::time::Instant::now();
    while server_view(dir.path(), &id)["restart"].is_null() {
        assert!(t.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(server_view(dir.path(), &id)["restart"]["attempt"], 2);
    assert_eq!(run(dir.path(), &["stop", &id]).code, 0);
    std::thread::sleep(Duration::from_secs(6));
    let v = server_view(dir.path(), &id);
    assert_eq!(v["status"], "Stopped", "{v}");
    assert!(!common::port_open(port));

    assert_eq!(run(dir.path(), &["shutdown"]).code, 0);
    serve.wait_exit(Duration::from_secs(30));
}

/// `harness serve --gateway`: the managed endpoint is in the ready line and `status`, and
/// `run` goes through it (the alias routes to the server `run` loaded).
#[test]
#[ignore]
fn e2e_serve_gateway_run_uses_managed_endpoint() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18800));
    let api_port = free_port(18860);
    let gw_port = free_port(18897);
    let mut serve = Serve::start_with(
        dir.path(),
        api_port,
        &["--gateway", "--gateway-port", &gw_port.to_string()],
    );
    let gw_port = serve.gateway_port();
    eprintln!("ready: {}", serve.ready);
    assert_eq!(serve.ready["gateway"]["running"], true, "{}", serve.ready);
    assert_eq!(
        endpoint_port(serve.ready["gateway"]["base_url"].as_str().unwrap()),
        gw_port
    );

    let status = run(dir.path(), &["status", "--json"]);
    assert_eq!(status.code, 0, "{}", status.stderr);
    let status = status.json();
    eprintln!("status gateway: {}", status["api"]["gateway"]);
    assert_eq!(status["api"]["gateway"]["running"], true, "{status}");

    let r = run(
        dir.path(),
        &[
            "run",
            model.to_str().unwrap(),
            "Say hello in three words.",
            "--max-tokens",
            "16",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let out = r.json();
    eprintln!("run: {} via {}", out["content"], out["endpoint"]);
    assert_eq!(endpoint_port(out["endpoint"].as_str().unwrap()), gw_port);
    assert!(!out["content"].as_str().unwrap().is_empty());
    assert!(out["usage"]["completion_tokens"].as_u64().unwrap() > 0);

    // The gateway lists the definition and reports it loaded.
    let models: Value = http_get_json(&format!("http://127.0.0.1:{gw_port}/v1/models"));
    eprintln!("models: {models}");
    assert!(models["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == out["model"]));

    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    let (code, _) = serve.wait_exit(Duration::from_secs(60));
    assert_eq!(code, 0);
    assert!(!common::port_open(gw_port), "shutdown stops the gateway");
}

/// Plain HTTP/1.0 GET on loopback (the body as JSON).
fn http_get_json(url: &str) -> Value {
    use std::io::Read;
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_at(rest.find('/').unwrap());
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    let body = buf
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or_default();
    serde_json::from_str(body).unwrap_or_else(|_| panic!("{buf}"))
}

/// The analytics record through every layer: `harness serve --gateway` records a gateway request in
/// analytics.db; `harness requests`, `harness requests <id>` and `harness analytics` read it
/// back over the Control API (ports 19010+).
#[test]
#[ignore]
fn e2e_serve_gateway_request_log_and_analytics() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(19020));
    let api_port = free_port(19010);
    let gw_port = free_port(19030);
    let mut serve = Serve::start_with(
        dir.path(),
        api_port,
        &["--gateway", "--gateway-port", &gw_port.to_string()],
    );
    let gw_port = serve.gateway_port();
    assert_eq!(serve.ready["gateway"]["running"], true, "{}", serve.ready);

    let r = run(
        dir.path(),
        &[
            "run",
            model.to_str().unwrap(),
            "Name two planets.",
            "--max-tokens",
            "16",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let out = r.json();
    assert_eq!(endpoint_port(out["endpoint"].as_str().unwrap()), gw_port);
    // Rows are written after the telemetry sample that follows the request.
    std::thread::sleep(Duration::from_millis(2500));

    let list = run(dir.path(), &["requests", "--json"]);
    assert_eq!(list.code, 0, "{}", list.stderr);
    let list = list.json();
    eprintln!("requests: {list}");
    assert_eq!(list["total"], 1);
    let row = &list["requests"][0];
    assert_eq!(row["alias"], out["model"]);
    assert_eq!(row["client"], "harness-cli");
    assert_eq!(row["status"], 200);
    // The CLI stops reading at `[DONE]`: a complete answer, not a cancellation.
    assert_eq!(row["cancelled"], false);
    assert_eq!(
        row["completion_tokens"].as_u64(),
        out["usage"]["completion_tokens"].as_u64()
    );
    // `harness run` loads the model itself (Control API), then asks the gateway.
    assert!(row["vram_peak_bytes"].as_u64().is_some());

    let id = row["id"].as_str().unwrap();
    let detail = run(dir.path(), &["requests", id, "--json"]);
    assert_eq!(detail.code, 0, "{}", detail.stderr);
    let detail = detail.json();
    let kinds: Vec<&str> = detail["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"prompt") && kinds.contains(&"generation"),
        "{kinds:?}"
    );
    assert!(
        detail["content"].is_null(),
        "content capture is off by default"
    );
    let human = run(dir.path(), &["requests", id]);
    assert_eq!(human.code, 0);
    eprintln!("{}", human.stdout);
    assert!(human.stdout.contains("Timeline"));

    let a = run(dir.path(), &["analytics", "--range", "24h", "--json"]);
    assert_eq!(a.code, 0, "{}", a.stderr);
    let a = a.json();
    assert_eq!(a["totals"]["requests"], 1);
    assert_eq!(a["models"][0]["alias"], out["model"]);
    assert_eq!(a["clients"][0]["client"], "harness-cli");
    let human = run(dir.path(), &["analytics"]);
    assert_eq!(human.code, 0);
    eprintln!("{}", human.stdout);
    assert!(human.stdout.contains("TTFT P50/P95"));
    let bad = run(dir.path(), &["analytics", "--range", "12w"]);
    assert_ne!(bad.code, 0);

    assert_eq!(run(dir.path(), &["shutdown", "--json"]).code, 0);
    let (code, _) = serve.wait_exit(Duration::from_secs(60));
    assert_eq!(code, 0);
    assert!(!common::port_open(gw_port));
}
