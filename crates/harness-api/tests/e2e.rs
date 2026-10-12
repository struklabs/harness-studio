//! End to end with a real llama-server and model:
//!
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-api --test e2e
//!   -- --ignored --nocapture
//!
//! API up → definition + profile → `/load` by alias → SSE `Starting`/`Running` → effective
//! values, logs → a real chat completion → `/servers/{id}/metrics` → `/unload` → nothing left.

mod common;

use std::time::{Duration, Instant};

use common::json;
use reqwest::Method;
use serde_json::json;

#[tokio::test]
#[ignore]
async fn e2e_api_loads_a_real_model_and_reports_metrics() {
    let (Some(_bin), Some(model)) = (common::llama_server(), common::test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let api = common::start().await;
    // The GUI runs this sampler; a headless owner runs the same core loop.
    let sampler = tokio::spawn(harness_core::orchestrator::run_telemetry_sampler(
        api.ctx.telemetry.clone(),
        api.ctx.registry.clone(),
        |_| {},
    ));
    let port = api
        .ctx
        .registry
        .suggest_port("127.0.0.1", 18490, &[])
        .expect("free port >= 18490");
    let alias = format!("e2e-api-{}", std::process::id());

    let (status, def) = json(
        api.post(
            "/definitions",
            json!({"alias": alias, "base": {"kind": "local", "path": model},
                   "load": {"gpu_layers": 99, "context_size": 2048}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{def}");
    let def_id = def["id"].as_str().unwrap().to_string();
    let (status, profile) = json(
        api.post(
            "/servers",
            json!({"name": "E2E API", "config": {"model_path": model, "gpu_layers": 99,
                   "context_size": 2048, "host": "127.0.0.1", "port": port,
                   "definition_id": def_id}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{profile}");
    let id = profile["id"].as_str().unwrap().to_string();

    let events = api
        .http
        .get(api.url("/events?types=server_status"))
        .bearer_auth(&api.token)
        .send()
        .await
        .unwrap();
    let reader = tokio::spawn({
        let id = id.clone();
        common::read_sse(events, Duration::from_secs(240), move |_, d| {
            d["server_id"] == id.as_str()
                && (d["status"] == "Running" || d["status"].get("Error").is_some())
        })
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    let (status, loaded) = json(
        api.post("/load", json!({"alias": alias, "server_id": id}))
            .await,
    )
    .await;
    assert_eq!(status, 202, "{loaded}");
    assert_eq!(loaded["server_id"], id.as_str());

    let result: Result<(), String> = async {
        let events = reader.await.map_err(|e| e.to_string())?;
        let statuses: Vec<&serde_json::Value> = events.iter().map(|(_, d)| &d["status"]).collect();
        eprintln!(
            "status events: {statuses:?} ({:.1}s)",
            started.elapsed().as_secs_f32()
        );
        if statuses.first().copied() != Some(&json!("Starting"))
            || statuses.last().copied() != Some(&json!("Running"))
        {
            let (_, logs) = json(api.get(&format!("/servers/{id}/logs?limit=30")).await).await;
            return Err(format!("events {statuses:?}\n{logs}"));
        }

        let (_, server) = json(api.get(&format!("/servers/{id}")).await).await;
        assert_eq!(server["status"], "Running");
        assert_eq!(server["served_alias"], alias.as_str());
        assert_eq!(server["definition_id"], def_id.as_str());
        let endpoint = server["endpoint"].as_str().unwrap().to_string();
        assert_eq!(endpoint, format!("http://127.0.0.1:{port}/v1"));
        assert!(server["active"].get("api_key").is_none());

        // The served model id is the alias.
        let models: serde_json::Value = reqwest::get(format!("{endpoint}/models"))
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(models["data"][0]["id"], alias.as_str());

        let (_, logs) = json(api.get(&format!("/servers/{id}/logs?limit=5")).await).await;
        assert!(logs["next"].as_u64().unwrap() > 0);
        assert!(logs["lines"].as_array().unwrap().len() <= 5);

        let reply: serde_json::Value = api
            .http
            .post(format!("{endpoint}/chat/completions"))
            .json(&json!({"model": alias, "max_tokens": 16,
                          "messages": [{"role": "user", "content": "Say hello."}]}))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        eprintln!("reply: {}", reply["choices"][0]["message"]["content"]);
        assert!(reply["usage"]["completion_tokens"].as_u64().unwrap() > 0);

        // Metrics: the sampler polls once a second.
        let deadline = Instant::now() + Duration::from_secs(15);
        let metrics = loop {
            let (status, m) =
                json(api.get(&format!("/servers/{id}/metrics?history=5")).await).await;
            assert_eq!(status, 200, "{m}");
            let generated = m["snapshot"]["totals"]["generated_tokens"]
                .as_u64()
                .unwrap_or(0);
            if m["snapshot"]["server_online"] == true && generated > 0 {
                break m;
            }
            if Instant::now() > deadline {
                return Err(format!("no metrics: {m}"));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        eprintln!("metrics: {}", metrics["snapshot"]);
        assert!(metrics["snapshot"]["total_slots"].as_u64().unwrap() >= 1);
        assert!(!metrics["history"].as_array().unwrap().is_empty());

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (_, eff) = json(api.get(&format!("/servers/{id}/effective")).await).await;
            if eff["values"].is_object() {
                assert_eq!(eff["values"]["model_alias"], alias.as_str());
                assert!(eff["config"].get("api_key").is_none());
                break;
            }
            if Instant::now() > deadline {
                return Err(format!("no effective values: {eff}"));
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        Ok(())
    }
    .await;

    // Always clean up: unload by alias, then remove profile and definition.
    let (status, unloaded) = json(api.post("/unload", json!({"alias": alias})).await).await;
    let pm = api.ctx.registry.get(&id);
    if status != 200 {
        if let Some(pm) = &pm {
            let _ = pm.stop().await;
        }
    }
    let _ = api
        .send(Method::DELETE, &format!("/servers/{id}"), &api.token, None)
        .await;
    let _ = api
        .send(
            Method::DELETE,
            &format!("/definitions/{def_id}"),
            &api.token,
            None,
        )
        .await;
    sampler.abort();
    api.server.stop().await;

    result.unwrap();
    assert_eq!(status, 200, "{unloaded}");
    assert_eq!(unloaded["stopped"], json!([id]));
    assert!(pm.is_none_or(|pm| !pm.has_child()), "no llama-server left");
    assert!(harness_core::servers::ensure_port_free("127.0.0.1", port).is_ok());
}

/// A real llama-bench run through the API: `POST /benchmarks` → SSE `benchmark_progress` and
/// `benchmark_finished` with the same run id → `GET /benchmarks/{id}` from the history.
#[tokio::test]
#[ignore]
async fn e2e_api_benchmark_streams_progress_and_lands_in_history() {
    let (Some(_bin), Some(model)) = (common::llama_server(), common::test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let api = common::start().await;
    let events = api
        .http
        .get(api.url("/events?types=benchmark_progress,benchmark_finished"))
        .bearer_auth(&api.token)
        .send()
        .await
        .unwrap();
    let reader = tokio::spawn(common::read_sse(
        events,
        Duration::from_secs(300),
        |event, _| event == "benchmark_finished",
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (status, started) = json(
        api.post(
            "/benchmarks",
            json!({"model_path": model, "n_prompt": [32], "n_gen": [8], "gpu_layers": [99],
                   "repetitions": 1}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 202, "{started}");
    let run_id = started["run_id"].as_str().unwrap().to_string();
    // A second start while it runs is refused with a code.
    let (status, body) = json(
        api.post(
            "/benchmarks",
            json!({"model_path": model, "n_prompt": [32], "n_gen": [0]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "benchmark.already_running");

    let events = reader.await.unwrap();
    eprintln!("benchmark events: {}", events.len());
    assert!(events
        .iter()
        .any(|(e, d)| e == "benchmark_progress" && d["run_id"] == run_id.as_str()));
    let (_, finished) = events.last().unwrap();
    assert_eq!(finished["run"]["id"], run_id.as_str());
    assert_eq!(finished["run"]["status"], "completed", "{finished}");
    assert!(finished["run"]["results"].as_array().unwrap().len() >= 2);

    let (status, view) = json(api.get(&format!("/benchmarks/{run_id}")).await).await;
    assert_eq!(status, 200);
    assert_eq!(view["running"], false);
    assert_eq!(view["run"]["id"], run_id.as_str());
    let (_, list) = json(api.get("/benchmarks").await).await;
    assert!(list["runs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == run_id.as_str()));
    let (status, _) = json(
        api.post(&format!("/benchmarks/{run_id}/cancel"), json!({}))
            .await,
    )
    .await;
    assert_eq!(status, 409, "finished runs cannot be cancelled");
    api.server.stop().await;
}

/// A Modelfile imported through the API drives a real llama-server: its `num_ctx` is the
/// loaded context, and a chat request built from the resulting preset honors `SYSTEM` and
/// `PARAMETER stop`.
///
/// The Modelfile counts 1..9 whatever the user says (SYSTEM) and stops at "5".
#[tokio::test]
#[ignore]
async fn e2e_modelfile_import_honors_system_stop_and_context() {
    let (Some(_bin), Some(model)) = (common::llama_server(), common::test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let api = common::start().await;
    let port = api
        .ctx
        .registry
        .suggest_port("127.0.0.1", 19600, &[])
        .expect("free port >= 19600");
    // A definition loaded without a profile starts on the settings' port (temp data root).
    let mut settings = harness_core::config::AppSettings::load();
    settings.server.port = port;
    settings.save().unwrap();
    let alias = format!("e2e-modelfile-{}", std::process::id());
    let modelfile = format!(
        "# e2e Modelfile\nFROM {}\nPARAMETER num_ctx 3072\nPARAMETER num_gpu 99\nPARAMETER temperature 0\nPARAMETER num_predict 64\nPARAMETER stop \"5\"\nSYSTEM \"\"\"\nYou are a counting robot. Whatever the user says, reply with only the numbers 1 to 9 separated by single spaces, and nothing else.\n\"\"\"\nTEMPLATE \"{{{{ .Prompt }}}}\"\nMESSAGE user hi\n",
        model.display()
    );
    let (status, imported) = json(
        api.post(
            "/modelfile/import",
            json!({"text": modelfile, "alias": alias}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{imported}");
    eprintln!("warnings: {}", imported["warnings"]);
    let def_id = imported["definition"]["id"].as_str().unwrap().to_string();
    let preset_id = imported["definition"]["default_preset_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, preset) = json(api.get(&format!("/presets/{preset_id}")).await).await;
    assert_eq!(preset["sampling"]["stop"], json!(["5"]));

    let events = api
        .http
        .get(api.url("/events?types=server_status"))
        .bearer_auth(&api.token)
        .send()
        .await
        .unwrap();
    let reader = tokio::spawn(common::read_sse(
        events,
        Duration::from_secs(240),
        |_, d| d["status"] == "Running" || d["status"].get("Error").is_some(),
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (status, loaded) = json(api.post("/load", json!({"alias": alias})).await).await;
    assert_eq!(status, 202, "{loaded}");
    let id = loaded["server_id"].as_str().unwrap().to_string();
    // The new instance takes the next free port from the configured one.
    let endpoint_url = loaded["server"]["endpoint"].as_str().unwrap().to_string();
    let port: u16 = endpoint_url
        .trim_end_matches("/v1")
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!((19600..19700).contains(&port), "{endpoint_url}");

    let result: Result<(), String> = async {
        let events = reader.await.map_err(|e| e.to_string())?;
        if events.last().map(|(_, d)| &d["status"]) != Some(&json!("Running")) {
            let (_, logs) = json(api.get(&format!("/servers/{id}/logs?limit=30")).await).await;
            return Err(format!("not running: {events:?}\n{logs}"));
        }
        let endpoint = format!("http://127.0.0.1:{port}");
        let props: serde_json::Value = reqwest::get(format!("{endpoint}/props"))
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let n_ctx = props["default_generation_settings"]["n_ctx"]
            .as_u64()
            .unwrap();
        eprintln!("n_ctx {n_ctx}, slots {}", props["total_slots"]);
        assert_eq!(n_ctx, 3072, "num_ctx became the loaded context");

        // What a client does with the imported preset: system message + request params.
        let (_, preset) = json(api.get(&format!("/presets/{preset_id}")).await).await;
        let mut body = serde_json::Map::new();
        for key in ["temperature", "max_tokens", "stop"] {
            body.insert(key.into(), preset["sampling"][key].clone());
        }
        body.insert("model".into(), alias.clone().into());
        body.insert(
            "messages".into(),
            json!([{"role": "system", "content": preset["system_prompt"]},
                   {"role": "user", "content": "Hello there!"}]),
        );
        let reply: serde_json::Value = api
            .http
            .post(format!("{endpoint}/v1/chat/completions"))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let text = reply["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        eprintln!(
            "reply: {text:?} finish={}",
            reply["choices"][0]["finish_reason"]
        );
        assert!(text.contains("1 2 3 4"), "SYSTEM honored: {text:?}");
        assert!(!text.contains('5'), "stop honored: {text:?}");
        assert_eq!(reply["choices"][0]["finish_reason"], "stop");
        Ok(())
    }
    .await;

    let (status, unloaded) = json(api.post("/unload", json!({"alias": alias})).await).await;
    let pm = api.ctx.registry.get(&id);
    if status != 200 {
        if let Some(pm) = &pm {
            let _ = pm.stop().await;
        }
    }
    let _ = api
        .send(
            Method::DELETE,
            &format!("/definitions/{def_id}"),
            &api.token,
            None,
        )
        .await;
    api.server.stop().await;
    result.unwrap();
    assert_eq!(status, 200, "{unloaded}");
    assert!(pm.is_none_or(|pm| !pm.has_child()), "no llama-server left");
    assert!(harness_core::servers::ensure_port_free("127.0.0.1", port).is_ok());
}
