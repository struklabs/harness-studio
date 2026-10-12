//! Golden outputs of the llama.cpp-specific layers, written before the `Engine` refactor
//! and kept as its contract: argv, `--help` parsing, the parameter
//! catalog, `/health`, `/metrics` + `/slots`, the memory estimate and the `runtimes.json`
//! shape. Inputs are real captures in `tests/data/`; expected outputs are in
//! `tests/data/golden/`. A deliberate change is re-recorded with `HS_UPDATE_GOLDEN=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::capabilities::{BinaryKey, RuntimeCapabilities};
use crate::process::golden::{base, full};
use crate::process::{health_ready, ProcessManager, ServerCaps};
use crate::telemetry::{
    apply_metrics, context_tokens, parse_prometheus, parse_slots_json, server_totals, slot_infos,
    slot_progress, TelemetrySnapshot,
};
use crate::vram::{base_input_from_launch, estimate_raw, fit_with, FitInput, MemoryBudget};

const BUILDS: [&str; 3] = ["b11026", "b11236", "b11337"];

fn data_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn data(name: &str) -> String {
    std::fs::read_to_string(data_path(name)).unwrap()
}

/// Compares `actual` with `tests/data/golden/<name>.json`; `HS_UPDATE_GOLDEN=1` rewrites it.
fn golden(name: &str, actual: &Value) {
    let path = data_path(&format!("golden/{name}.json"));
    // The data root is a temporary folder per test process (the LoRA slot save path).
    let root = serde_json::to_string(&crate::paths::data_root().to_string_lossy()).unwrap();
    let text = (serde_json::to_string_pretty(actual).unwrap() + "\n")
        .replace(root.trim_matches('"'), "<data>")
        .replace("<data>\\\\", "<data>/"); // one golden for Windows and Unix
    if std::env::var_os("HS_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| {
            panic!(
                "missing {}; record it with HS_UPDATE_GOLDEN=1",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    assert!(
        expected == text,
        "golden {name} changed:\n--- expected\n{expected}\n--- actual\n{text}"
    );
}

fn runtime_caps(build: &str) -> RuntimeCapabilities {
    RuntimeCapabilities::from_outputs(
        BinaryKey {
            path: PathBuf::from(format!("llama-server-{build}.exe")),
            mtime_ms: Some(1),
            size: Some(1),
            build: None,
            commit: None,
            version: None,
        },
        &data(&format!("llama-server-{build}.version.txt")),
        &data(&format!("llama-server-{build}.help.txt")),
        &data(&format!("llama-server-{build}.list-devices.txt")),
    )
}

#[test]
fn golden_launch_args() {
    let mut out = BTreeMap::new();
    for build in BUILDS {
        let help = data(&format!("llama-server-{build}.help.txt"));
        let caps = ServerCaps::from_help(&help);
        let runtime = runtime_caps(build);
        for (name, config) in [("base", base()), ("full", full())] {
            let plain = ProcessManager::build_args(&config, &caps).unwrap();
            let with = ProcessManager::build_args_with(&config, &caps, Some(&runtime)).unwrap();
            out.insert(
                format!("{build}/{name}"),
                json!({
                    "args": plain,
                    "args_with_runtime": with.args,
                    "skipped": with.skipped,
                }),
            );
        }
    }
    golden("launch-args", &json!(out));
}

#[test]
fn golden_capabilities() {
    let mut out = BTreeMap::new();
    for build in BUILDS {
        let caps = runtime_caps(build);
        let server = caps.server_caps();
        // Without the help prose (shown verbatim, not interpreted), one line per flag.
        let canonical: BTreeMap<_, _> = caps
            .flags
            .iter()
            .map(|(k, v)| {
                let mut v = serde_json::to_value(v).unwrap();
                v.as_object_mut().unwrap().remove("description");
                (k.clone(), v.to_string())
            })
            .collect();
        let keys: Vec<_> = caps
            .flags
            .values()
            .filter(|v| v.description.is_empty())
            .map(|v| &v.canonical)
            .collect();
        out.insert(
            build.to_string(),
            json!({
                "version": caps.version,
                "help_parsed": caps.help_parsed,
                "without_description": keys,
                "flags": canonical,
                "devices": caps.devices,
                "looks_like_server": caps.looks_like_server(),
                "backends": caps.backends(),
                "server_caps": format!("{server:?}"),
                "server_caps_from_help": format!(
                    "{:?}",
                    ServerCaps::from_help(&data(&format!("llama-server-{build}.help.txt")))
                ),
            }),
        );
    }
    golden("capabilities", &json!(out));
}

#[test]
fn golden_param_catalog() {
    golden(
        "param-catalog",
        &serde_json::to_value(crate::params::catalog()).unwrap(),
    );
}

#[test]
fn golden_health() {
    let mut out = BTreeMap::new();
    for case in ["health-loading", "health-ok"] {
        let v: Value =
            serde_json::from_str(&data(&format!("b11337-qwen3-0.6b.{case}.json"))).unwrap();
        let status = v["status"].as_u64().unwrap() as u16;
        let body = v["body"].to_string();
        out.insert(case.to_string(), json!(health_ready(status, &body)));
    }
    golden("health", &json!(out));
}

#[test]
fn golden_metrics() {
    let mut out = BTreeMap::new();
    for file in [
        "b11026-qwen2.5-0.5b-two-chats.metrics.txt",
        "b11236.metrics.txt",
        "b11236-qwen3-4b-draft.metrics.txt",
        "b11337-qwen3-0.6b.metrics.txt",
    ] {
        let metrics = parse_prometheus(&data(file));
        let sorted: BTreeMap<_, _> = metrics.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let mut snapshot = TelemetrySnapshot::default();
        apply_metrics(&metrics, None, &mut snapshot);
        out.insert(
            file.to_string(),
            json!({
                "metrics": sorted,
                "totals": server_totals(&metrics),
                "snapshot": snapshot,
            }),
        );
    }
    golden("metrics", &json!(out));
}

#[test]
fn golden_slots() {
    let mut out = BTreeMap::new();
    for file in [
        "b11236-qwen3-4b.slots.json",
        "b11337-qwen3-0.6b.slots-busy.json",
        "b11337-qwen3-0.6b.slots-idle.json",
    ] {
        let v: Value = serde_json::from_str(&data(file)).unwrap();
        let mut snapshot = TelemetrySnapshot::default();
        parse_slots_json(&v, &mut snapshot);
        let progress: BTreeMap<_, _> = slot_progress(&v)
            .into_iter()
            .map(|(id, p)| (id, format!("{p:?}")))
            .collect();
        out.insert(
            file.to_string(),
            json!({
                "snapshot": snapshot,
                "slots": slot_infos(&v),
                "progress": progress,
                "context_tokens": context_tokens(&v),
            }),
        );
    }
    golden("slots", &json!(out));
}

fn shapes() -> Vec<(String, u64, crate::gguf::GgufMetadata)> {
    let v: Value = serde_json::from_str(&data("gguf-vram-shapes.json")).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["file"].as_str().unwrap().to_string(),
                e["size_bytes"].as_u64().unwrap(),
                serde_json::from_value(e["meta"].clone()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn golden_memory_estimate() {
    let mut out = BTreeMap::new();
    out.insert(
        "input/base".to_string(),
        json!(base_input_from_launch(&base())),
    );
    out.insert(
        "input/full".to_string(),
        json!(base_input_from_launch(&full())),
    );

    let budget = MemoryBudget {
        vram_bytes: 4 * 1024 * 1024 * 1024,
        ram_bytes: 16 * 1024 * 1024 * 1024,
        ..Default::default()
    };
    for (file, size, meta) in shapes() {
        for (ctx, cache) in [(4096, None), (32768, Some("q8_0"))] {
            // Through serde so the defaults are the ones the commands get (full offload).
            let input: FitInput = serde_json::from_value(json!({
                "model_path": file,
                "model_size_bytes": size,
                "context_size": ctx,
                "cache_type_k": cache,
                "cache_type_v": cache,
            }))
            .unwrap();
            out.insert(
                format!("{file}/c{ctx}/{}", cache.unwrap_or("f16")),
                json!({
                    "estimate": estimate_raw(&input, Some(&meta), true),
                    "fit_4g": fit_with(&input, Some(&meta), &budget, &[]),
                }),
            );
        }
    }
    golden("memory-estimate", &json!(out));
}

#[test]
fn golden_runtimes_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("runtimes.json");
    std::fs::copy(data_path("runtimes-v1.json"), &file).unwrap();
    let registry = crate::runtimes::RuntimeRegistry::with_paths(file, dir.path().join("bin"));
    let entries = registry.entries().unwrap();
    golden("runtimes-json", &serde_json::to_value(entries).unwrap());
}
