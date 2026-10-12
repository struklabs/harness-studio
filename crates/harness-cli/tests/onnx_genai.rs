//! An onnxruntime-genai chat model through the whole stack, `#[ignore]`: the
//! library lists the folder as a chat model, `harness run` loads it on harness-onnx (the ONNX
//! Runtime, DirectML and GenAI libraries are downloaded into the temporary data folder on the
//! first launch, SHA-256 checked) and chats through the managed gateway.
//!
//! ```text
//! HS_GENAI_ROOT=<models folder holding owner/repo/<variant>/model.onnx as downloaded by Harness>
//! HS_GENAI_NAME=<library name, e.g. Phi-3-mini-4k-instruct-onnx-directml-int4-awq-block-128>
//! cargo test -p harness-cli --test onnx_genai -- --ignored --nocapture
//! ```
//! (`cargo build -p harness-onnx` first: the test runs the sidecar next to the `harness` binary.)

mod common;

use common::{data_dir, free_port, run, Serve};

#[test]
#[ignore = "downloads ONNX Runtime + GenAI and needs a genai model on disk"]
fn e2e_genai_chat_through_the_gateway() {
    let (Some(root), Ok(name)) = (
        std::env::var_os("HS_GENAI_ROOT"),
        std::env::var("HS_GENAI_NAME"),
    ) else {
        eprintln!("HS_GENAI_ROOT / HS_GENAI_NAME not set, skipping");
        return;
    };
    let dir = data_dir(None, free_port(18900));
    let settings = dir.path().join("config/settings.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    v["extra_models_dirs"] = serde_json::json!([root.to_string_lossy()]);
    std::fs::write(&settings, v.to_string()).unwrap();

    // The library lists the folder as a chat model of the ONNX family.
    let ls = run(dir.path(), &["ls", "--json"]);
    assert_eq!(ls.code, 0, "{}", ls.stderr);
    let ls = ls.json();
    let entry = ls["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["model"]["name"] == name.as_str())
        .unwrap_or_else(|| panic!("{name} not in the library: {ls}"));
    assert_eq!(entry["model"]["kind"], "llm");
    assert_eq!(entry["caps"]["chat"], true);
    assert_eq!(entry["record"]["runtime"], "onnxruntime");

    let gw_port = free_port(18950);
    let mut serve = Serve::start_with(
        dir.path(),
        free_port(18930),
        &["--gateway", "--gateway-port", &gw_port.to_string()],
    );
    assert_eq!(serve.ready["gateway"]["running"], true, "{}", serve.ready);

    let r = run(
        dir.path(),
        &[
            "run",
            &name,
            "What is the capital of France? Answer in one short sentence.",
            "--max-tokens",
            "48",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let out = r.json();
    eprintln!("run: {} via {}", out["content"], out["endpoint"]);
    assert!(out["content"].as_str().unwrap().contains("Paris"), "{out}");
    assert!(out["usage"]["completion_tokens"].as_u64().unwrap() > 0);
    eprintln!("timings: {}", out["timings"]);

    // The server is up on the ONNX family and listed by `ps`.
    let ps = run(dir.path(), &["ps", "--json"]);
    assert_eq!(ps.code, 0, "{}", ps.stderr);
    assert!(ps.stdout.contains("onnx"), "{}", ps.stdout);
    let _ = serve.child.kill();
}
