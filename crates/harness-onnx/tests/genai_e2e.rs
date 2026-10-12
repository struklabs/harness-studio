//! Real chat runs of the sidecar on onnxruntime-genai models (no mocks), all `#[ignore]`.
//!
//! ```text
//! HS_ORT_LIB=<onnxruntime.dll>   next to it: onnxruntime-genai.dll (and DirectML.dll for GPU)
//! HS_GENAI_MODEL=<folder>/model.onnx   a folder with genai_config.json, e.g.
//!   microsoft/Phi-3-mini-4k-instruct-onnx/directml/directml-int4-awq-block-128
//! HS_GENAI_MODEL_CPU=<folder>/<decoder>.onnx   (optional) a CPU variant, for the speed comparison
//! cargo test -p harness-onnx --test genai_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The DirectML run and the CPU run answer the same prompt greedily; both must say "Paris" and
//! the tokens per second of each are printed.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name).map(PathBuf::from);
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

struct Server {
    child: Child,
    base: String,
}

impl Server {
    fn start(model: &Path, lib: &Path, provider: &str) -> Server {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let child = Command::new(env!("CARGO_BIN_EXE_harness-onnx"))
            .arg("--model")
            .arg(model)
            .arg("--ort-lib")
            .arg(lib)
            .args(["--provider", provider, "--port", &port.to_string()])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start harness-onnx");
        let base = format!("http://127.0.0.1:{port}");
        let started = Instant::now();
        loop {
            if reqwest::blocking::get(format!("{base}/health")).is_ok_and(|r| r.status() == 200) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(180),
                "the server did not become healthy"
            );
            std::thread::sleep(Duration::from_millis(500));
        }
        Server { child, base }
    }

    fn chat(&self, body: Value) -> Value {
        reqwest::blocking::Client::new()
            .post(format!("{}/v1/chat/completions", self.base))
            .json(&body)
            .timeout(Duration::from_secs(300))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn paris(s: &Server, label: &str) -> f64 {
    let ask = |prompt: &str, max: u32| {
        s.chat(json!({
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max, "temperature": 0
        }))
    };
    let r = ask(
        "What is the capital of France? Answer in one short sentence.",
        32,
    );
    let text = r["choices"][0]["message"]["content"].as_str().unwrap();
    eprintln!("{label}: {text:?}");
    assert!(text.contains("Paris"), "{label}: {r}");
    // Speed: a long answer after a warm-up run (graph capture, kernel compilation).
    ask("Write a poem about the sea.", 8);
    let r = ask("Write a long poem about the sea.", 96);
    assert_eq!(r["usage"]["completion_tokens"], 96, "{r}");
    let tps = r["timings"]["predicted_per_second"].as_f64().unwrap();
    eprintln!(
        "{label}: {} prompt + {} generated tokens, {:.1} tokens/s",
        r["usage"]["prompt_tokens"], r["usage"]["completion_tokens"], tps
    );
    tps
}

#[test]
#[ignore = "needs an ONNX Runtime + GenAI folder and a genai model on disk"]
fn chat_on_directml_streams_and_stops() {
    let (Some(lib), Some(model)) = (env_path("HS_ORT_LIB"), env_path("HS_GENAI_MODEL")) else {
        return;
    };
    let s = Server::start(&model, &lib, "auto");
    let models: Value = reqwest::blocking::get(format!("{}/v1/models", s.base))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(models["object"], "list");
    paris(&s, "auto");

    // Streaming: role chunk, text deltas, a finish chunk, usage, [DONE].
    let text = reqwest::blocking::Client::new()
        .post(format!("{}/v1/chat/completions", s.base))
        .json(&json!({
            "messages": [{"role": "user", "content": "Count from one to five in words."}],
            "max_tokens": 40, "temperature": 0, "stream": true,
            "stream_options": {"include_usage": true}
        }))
        .send()
        .unwrap()
        .text()
        .unwrap();
    let events: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .collect();
    assert_eq!(events.last(), Some(&"[DONE]"), "{text}");
    let chunks: Vec<Value> = events[..events.len() - 1]
        .iter()
        .map(|e| serde_json::from_str(e).unwrap())
        .collect();
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    let joined: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert!(
        joined.to_lowercase().contains("one") && joined.to_lowercase().contains("five"),
        "{joined}"
    );
    let usage = chunks.last().unwrap();
    assert!(
        usage["usage"]["completion_tokens"].as_u64().unwrap() > 3,
        "{usage}"
    );

    // A stop string ends the answer before it; the prompt cannot overflow the context.
    let r = s.chat(json!({
        "messages": [{"role": "user", "content": "Repeat exactly these words and nothing else: alpha beta gamma delta"}],
        "max_tokens": 60, "temperature": 0, "stop": ["gamma"]
    }));
    let stopped = r["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .to_lowercase();
    assert!(
        !stopped.contains("gamma") && stopped.contains("alpha"),
        "{r}"
    );
    assert_eq!(r["choices"][0]["finish_reason"], "stop");
    let huge = "word ".repeat(20_000);
    let r = s.chat(json!({"messages": [{"role": "user", "content": huge}]}));
    assert_eq!(r["error"]["code"], 400, "{r}");
}

#[test]
#[ignore = "needs an ONNX Runtime + GenAI folder and genai models on disk"]
fn directml_and_cpu_agree_and_the_speeds_are_printed() {
    let (Some(lib), Some(model)) = (env_path("HS_ORT_LIB"), env_path("HS_GENAI_MODEL")) else {
        return;
    };
    let gpu = paris(&Server::start(&model, &lib, "directml"), "directml");
    if let Some(cpu_model) = env_path("HS_GENAI_MODEL_CPU") {
        let cpu = paris(&Server::start(&cpu_model, &lib, "cpu"), "cpu");
        eprintln!(
            "DirectML {gpu:.1} tokens/s, CPU {cpu:.1} tokens/s, {:.1}x",
            gpu / cpu
        );
    }
}
