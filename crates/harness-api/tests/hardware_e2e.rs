//! Devices and the measured VRAM footprint with a real llama-server and model:
//!
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-api
//!   --test hardware_e2e -- --ignored --nocapture
//!
//! `/hardware/devices` (real `--list-devices`) → estimate before loading → start a profile at
//! `-lv 4` → the load log and the process's GPU memory are recorded → the same estimate now
//! comes back as `measured`, with the log's buffer sizes → stop, nothing left.

mod common;

use std::time::{Duration, Instant};

use common::json;
use reqwest::Method;
use serde_json::json;

fn encode(id: &str) -> String {
    id.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[tokio::test]
#[ignore]
async fn e2e_devices_and_measured_footprint() {
    let (Some(_bin), Some(model)) = (common::llama_server(), common::test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let api = common::start().await;
    // The model's directory becomes a library directory of the temporary data root.
    let mut settings = harness_core::config::AppSettings::load();
    settings.extra_models_dirs = vec![model.parent().unwrap().to_string_lossy().to_string()];
    settings.save().unwrap();

    let (status, devices) = json(api.get("/hardware/devices?refresh=true").await).await;
    assert_eq!(status, 200, "{devices}");
    eprintln!("devices: {devices:#}");
    assert!(devices["error"].is_null(), "{devices}");
    let list = devices["devices"].as_array().unwrap();
    assert!(!list.is_empty());
    assert!(list
        .iter()
        .all(|d| d["total_bytes"].as_u64().unwrap_or(0) > 0));

    let (_, status_doc) = json(api.get("/status").await).await;
    assert_eq!(
        status_doc["runtime"]["devices"].as_array().map(|a| a.len()),
        Some(list.len())
    );

    let (_, models) = json(api.get("/models").await).await;
    let file = model.file_name().unwrap().to_string_lossy().to_string();
    let entry = models["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| {
            e["model"]["path"]
                .as_str()
                .is_some_and(|p| p.ends_with(&file))
        })
        .unwrap_or_else(|| panic!("{file} not in the library: {models}"))
        .clone();
    let model_id = encode(entry["model"]["id"].as_str().unwrap());
    let fit_url = format!("/models/{model_id}/fit?context_size=8192&gpu_layers=99");
    let (status, before) = json(api.get(&fit_url).await).await;
    assert_eq!(status, 200, "{before}");
    assert_eq!(before["source"], "estimate", "{before}");
    eprintln!("estimate before load: {:#}", before["estimate"]);

    let port = api
        .ctx
        .registry
        .suggest_port("127.0.0.1", 18600, &[])
        .expect("free port >= 18600");
    let (status, profile) = json(
        api.post(
            "/servers",
            json!({"name": "E2E VRAM", "config": {"model_path": model, "gpu_layers": 99,
                   "context_size": 8192, "host": "127.0.0.1", "port": port,
                   "custom_args": "-lv 4"}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{profile}");
    let id = profile["id"].as_str().unwrap().to_string();

    let result: Result<serde_json::Value, String> = async {
        let (status, started) =
            json(api.post(&format!("/servers/{id}/start"), json!({})).await).await;
        if status >= 300 {
            return Err(format!("start: {status} {started}"));
        }
        let deadline = Instant::now() + Duration::from_secs(240);
        loop {
            let (_, server) = json(api.get(&format!("/servers/{id}")).await).await;
            if server["status"] == "Running" {
                break;
            }
            if server["status"].get("Error").is_some() || Instant::now() > deadline {
                let (_, logs) = json(api.get(&format!("/servers/{id}/logs?limit=40")).await).await;
                return Err(format!("not running: {server}\n{logs}"));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        // The measurement is stored right after the post-start checks.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let (_, fit) = json(api.get(&fit_url).await).await;
            if fit["source"] == "measured" {
                return Ok(fit);
            }
            if Instant::now() > deadline {
                return Err(format!("no measurement: {fit}"));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    .await;

    let pm = api.ctx.registry.get(&id);
    let _ = api.post(&format!("/servers/{id}/stop"), json!({})).await;
    if let Some(pm) = &pm {
        let _ = pm.stop().await;
    }
    let _ = api
        .send(Method::DELETE, &format!("/servers/{id}"), &api.token, None)
        .await;
    api.server.stop().await;

    let after = result.unwrap();
    eprintln!("measured: {:#}", after["estimate"]);
    let est = |v: &serde_json::Value, k: &str| v["estimate"][k].as_u64().unwrap() as f64;
    // The measured total is what the OS reported (or the log's buffers + overhead).
    let diff =
        (est(&after, "vram_bytes") - est(&before, "vram_bytes")).abs() / est(&after, "vram_bytes");
    eprintln!(
        "estimate {:.0} MiB vs measured {:.0} MiB ({:.1}%)",
        est(&before, "vram_bytes") / 1048576.0,
        est(&after, "vram_bytes") / 1048576.0,
        diff * 100.0
    );
    assert!(diff < 0.05, "estimate off by {:.1}%", diff * 100.0);
    assert!(est(&after, "kv_gpu_bytes") > 0.0);
    assert!(pm.is_none_or(|pm| !pm.has_child()), "no llama-server left");
    assert!(harness_core::servers::ensure_port_free("127.0.0.1", port).is_ok());
}
