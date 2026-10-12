//! `POST /api/create`, `/api/copy`, `DELETE /api/delete`, `/api/push` and `/api/blobs/:digest`
//! of the Ollama-compatible API against a real gateway on a temporary data root (no model is
//! loaded, so no llama-server is needed; the "GGUF" is a file with the `GGUF` magic).
//!
//! Gateway on 19960. The real `ollama` CLI version of this flow is run by hand (see
//! docs/en/INTEGRATIONS.md); this test pins the HTTP behaviour.

use std::path::PathBuf;
use std::sync::Arc;

use harness_core::domain::store::Domain;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn digest_of(bytes: &[u8]) -> String {
    let h = Sha256::digest(bytes);
    format!(
        "sha256:{}",
        h.iter().map(|b| format!("{:02x}", b)).collect::<String>()
    )
}

async fn ndjson(resp: reqwest::Response) -> Vec<Value> {
    resp.text()
        .await
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn definition(alias: &str) -> Option<ModelDefinition> {
    Domain::global()
        .definitions()
        .unwrap()
        .into_iter()
        .find(|d| d.alias == alias)
}

#[tokio::test]
async fn ollama_create_copy_delete_blobs() {
    harness_core::secrets::use_memory_store();
    let root = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("ollama-manage"))
        .tempdir()
        .unwrap();
    harness_core::paths::init(root.path().to_path_buf());
    let registry = Arc::new(ServerRegistry::default());
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: root.path().to_path_buf(),
        downloads: None,
    };
    let server = GatewayServer::new();
    let status = server
        .start(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: 19960,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let post = |path: &str, body: Value| http.post(format!("{base}{path}")).json(&body).send();

    // A file that passes for a GGUF (magic only; nothing is loaded in this test).
    let mut gguf = b"GGUF".to_vec();
    gguf.extend((0..2048u32).flat_map(|i| i.to_le_bytes()));
    let digest = digest_of(&gguf);
    let blob_url = format!("{base}/api/blobs/{digest}");

    // --- blobs ---
    assert_eq!(http.head(&blob_url).send().await.unwrap().status(), 404);
    let wrong = format!("{base}/api/blobs/{}", digest_of(b"something else"));
    let r = http.post(&wrong).body(gguf.clone()).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let body: Value = r.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("digest mismatch"),
        "{body}"
    );
    assert_eq!(
        http.post(format!("{base}/api/blobs/sha256:nope"))
            .body("x")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        http.post(&blob_url)
            .body(gguf.clone())
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    assert_eq!(http.head(&blob_url).send().await.unwrap().status(), 200);
    let hex = digest.strip_prefix("sha256:").unwrap();
    let stored: PathBuf = harness_core::downloader::DownloadManager::models_dir()
        .join("ollama-blobs")
        .join(format!("sha256-{hex}"));
    assert_eq!(std::fs::read(&stored).unwrap(), gguf);

    // --- create from an uploaded GGUF (streamed status lines) ---
    let r = post(
        "/api/create",
        json!({
            "model": "api-test",
            "files": {"model.gguf": digest},
            "system": "You are terse.",
            "parameters": {"temperature": 0.3, "num_ctx": 4096, "stop": ["<end>"]},
            "messages": [{"role": "user", "content": "ping"}, {"role": "assistant", "content": "pong"}],
            "template": "{{ .Prompt }}"
        }),
    )
    .await
    .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers()["content-type"]
        .to_str()
        .unwrap()
        .contains("ndjson"));
    let lines = ndjson(r).await;
    assert_eq!(lines.last().unwrap()["status"], "success", "{lines:?}");
    let texts: Vec<&str> = lines.iter().filter_map(|l| l["status"].as_str()).collect();
    assert!(texts.contains(&"parsing GGUF"));
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("warning: TEMPLATE ignored")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("MESSAGE turn")),
        "{texts:?}"
    );

    let def = definition("api-test").expect("definition created");
    assert_eq!(def.base.local_path(), Some(stored.as_path()));
    assert!(def.description.starts_with("Ollama API create"));
    assert_eq!(
        def.load.get("context_size"),
        Some(&harness_core::params::ParamValue::Int(4096))
    );
    let preset = Domain::global()
        .inference_preset(def.default_preset_id.as_deref().unwrap())
        .unwrap();
    assert_eq!(preset.sampling.temperature, Some(0.3));
    assert_eq!(preset.sampling.stop, vec!["<end>".to_string()]);
    let system = preset.system_prompt.unwrap();
    assert!(system.starts_with("You are terse.") && system.contains("Assistant: pong"));

    // Same name again replaces the API-made definition (one definition, new values).
    let r = post(
        "/api/create",
        json!({"model": "api-test:latest", "files": {"model.gguf": digest}, "stream": false,
               "parameters": {"temperature": 0.9}}),
    )
    .await
    .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"status": "success"})
    );
    let all = Domain::global().definitions().unwrap();
    assert_eq!(all.iter().filter(|d| d.alias == "api-test").count(), 1);
    assert_eq!(all.iter().filter(|d| d.id == def.id).count(), 1, "same id");
    let again = definition("api-test").unwrap();
    let preset = Domain::global()
        .inference_preset(again.default_preset_id.as_deref().unwrap())
        .unwrap();
    assert_eq!(preset.sampling.temperature, Some(0.9));
    assert!(preset.system_prompt.is_none());
    // The replaced definition's old preset is gone.
    let presets = Domain::global().inference_presets().unwrap();
    assert_eq!(presets.iter().filter(|p| p.name == "api-test").count(), 1);

    // --- create from an existing model: inherits and overlays ---
    let r = post(
        "/api/create",
        json!({"model": "child", "from": "api-test:latest", "system": "Child system.",
               "parameters": {"top_k": 7}, "stream": false}),
    )
    .await
    .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    let child = definition("child").unwrap();
    assert_eq!(child.base, again.base);
    let cp = Domain::global()
        .inference_preset(child.default_preset_id.as_deref().unwrap())
        .unwrap();
    assert_eq!(
        cp.sampling.temperature,
        Some(0.9),
        "inherited from the parent preset"
    );
    assert_eq!(cp.sampling.top_k, Some(7));
    assert_eq!(cp.system_prompt.as_deref(), Some("Child system."));

    // --- refusals ---
    let status_of = |r: reqwest::Response| async move {
        let s = r.status().as_u16();
        let b: Value = r.json().await.unwrap_or(Value::Null);
        (s, b["error"].as_str().unwrap_or_default().to_string())
    };
    let (s, e) = status_of(
        post(
            "/api/create",
            json!({"model": "q", "from": "api-test", "quantize": "q4_K_M"}),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(s, 501);
    assert!(e.contains("quantize"), "{e}");
    let (s, _) = status_of(
        post(
            "/api/create",
            json!({"model": "q", "from": "no-such-model"}),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(s, 404);
    let (s, e) = status_of(
        post(
            "/api/create",
            json!({"model": "q", "files": {"m.gguf": digest_of(b"never uploaded")}}),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(s, 400);
    assert!(e.contains("/api/blobs"), "{e}");
    let (s, _) = status_of(post("/api/create", json!({"model": "q"})).await.unwrap()).await;
    assert_eq!(s, 400);
    // A definition the API did not make is protected.
    let plain = Domain::global()
        .save_definition(ModelDefinition::new(
            "plain",
            ModelRef::Local {
                path: stored.clone(),
            },
        ))
        .unwrap();
    let (s, e) = status_of(
        post("/api/create", json!({"model": "plain", "from": "api-test"}))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(s, 409);
    assert!(e.contains("already exists"), "{e}");
    assert!(definition("plain").unwrap().description.is_empty());

    // --- copy ---
    let r = post(
        "/api/copy",
        json!({"source": "api-test:latest", "destination": "api-copy"}),
    )
    .await
    .unwrap();
    assert_eq!(r.status(), 200);
    let copy = definition("api-copy").unwrap();
    assert_eq!(copy.base, again.base);
    assert_ne!(copy.id, again.id);
    let (s, e) = status_of(
        post(
            "/api/copy",
            json!({"source": "api-test", "destination": "api-copy"}),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(s, 409, "{e}");
    let (s, _) = status_of(
        post("/api/copy", json!({"source": "nope", "destination": "x"}))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(s, 404);

    // --- delete: the blob stays while a definition uses it ---
    let delete = |model: &str| {
        http.delete(format!("{base}/api/delete"))
            .json(&json!({"model": model}))
            .send()
    };
    assert_eq!(delete("child").await.unwrap().status(), 200);
    assert!(definition("child").is_none());
    assert!(stored.is_file());
    assert_eq!(delete("child").await.unwrap().status(), 404);
    assert_eq!(delete("api-copy:latest").await.unwrap().status(), 200);
    assert_eq!(delete("api-test").await.unwrap().status(), 200);
    // `plain` still points at the blob, so it is kept; its own files are never deleted.
    assert!(stored.is_file());
    assert_eq!(delete("plain").await.unwrap().status(), 200);
    assert!(
        !stored.exists(),
        "unused uploaded blob is removed with its last definition"
    );
    let _ = plain;
    // Presets made by create are gone with their definitions.
    let presets = Domain::global().inference_presets().unwrap();
    assert!(presets.iter().all(|p| p.description != "Ollama API create"));

    // --- push ---
    let (s, e) = status_of(post("/api/push", json!({"model": "x"})).await.unwrap()).await;
    assert_eq!(s, 501);
    assert!(e.contains("no model registry"), "{e}");

    // --- the real `ollama` CLI (HS_OLLAMA_CLI=<ollama executable>), only against this port ---
    if let Some(cli) = std::env::var_os("HS_OLLAMA_CLI").map(PathBuf::from) {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("m.gguf"), &gguf).unwrap();
        std::fs::write(
            work.path().join("Modelfile"),
            "FROM ./m.gguf\nPARAMETER temperature 0.4\nPARAMETER stop \"<x>\"\nSYSTEM \"\"\"Be nice.\"\"\"\nTEMPLATE \"\"\"{{ .Prompt }}\"\"\"\n",
        )
        .unwrap();
        std::fs::write(
            work.path().join("Child.Modelfile"),
            "FROM clitest\nPARAMETER top_k 11\n",
        )
        .unwrap();
        let ollama = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let mut c = tokio::process::Command::new(&cli);
            c.args(&args)
                .current_dir(work.path())
                .env("OLLAMA_HOST", "127.0.0.1:19960")
                .env("NO_COLOR", "1");
            async move {
                let out = c.output().await.unwrap();
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                eprintln!(
                    "$ ollama {}\n[exit {:?}]\n{text}",
                    args.join(" "),
                    out.status.code()
                );
                (out.status.success(), text)
            }
        };
        let (ok, text) = ollama(&["create", "clitest", "-f", "Modelfile"]).await;
        assert!(ok, "{text}");
        assert!(text.contains("success"), "{text}");
        let def = definition("clitest").expect("created by the CLI");
        assert!(blobs_of(&def).is_file());
        let p = Domain::global()
            .inference_preset(def.default_preset_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(p.sampling.temperature, Some(0.4));
        assert_eq!(p.system_prompt.as_deref(), Some("Be nice."));
        // Same file again: the CLI sees the blob (HEAD 200) and only re-creates.
        let (ok, text) = ollama(&["create", "clitest", "-f", "Modelfile"]).await;
        assert!(ok, "{text}");
        let (ok, text) = ollama(&["create", "clitest-child", "-f", "Child.Modelfile"]).await;
        assert!(ok, "{text}");
        let child = definition("clitest-child").unwrap();
        let cp = Domain::global()
            .inference_preset(child.default_preset_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(
            (cp.sampling.top_k, cp.sampling.temperature),
            (Some(11), Some(0.4))
        );
        let (ok, text) = ollama(&["cp", "clitest", "clitest-copy"]).await;
        assert!(ok, "{text}");
        assert!(definition("clitest-copy").is_some());
        let (ok, text) = ollama(&["push", "clitest"]).await;
        assert!(!ok && text.contains("no model registry"), "{text}");
        let (ok, text) = ollama(&["create", "q", "--quantize", "q4_K_M", "-f", "Modelfile"]).await;
        assert!(!ok && text.contains("quantize"), "{text}");
        for name in ["clitest-copy", "clitest-child", "clitest"] {
            let (ok, text) = ollama(&["rm", name]).await;
            assert!(ok, "{text}");
            assert!(definition(name).is_none());
        }
        assert!(
            !blobs_of(&def).exists(),
            "blob removed with the last definition"
        );
        let (ok, text) = ollama(&["rm", "clitest"]).await;
        assert!(!ok && text.contains("not found"), "{text}");
    }

    server.stop().await;
    registry.stop_all().await;
}

fn blobs_of(def: &ModelDefinition) -> PathBuf {
    def.base.local_path().unwrap().to_path_buf()
}
