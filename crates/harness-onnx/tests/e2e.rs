//! Real runs of the sidecar with real Hugging Face models (no mocks), all `#[ignore]`: they need
//! an ONNX Runtime library and the model repos on disk.
//!
//! ```text
//! HS_ORT_LIB=<onnxruntime.dll / libonnxruntime.so>          (any 1.24+ build)
//! HS_ORT_LIB_DML=<onnxruntime.dll of the DirectML build>     (optional, runs the GPU checks)
//! HS_ONNX_MODELS=<folder holding the repos as downloaded by Harness>
//!   sentence-transformers/all-MiniLM-L6-v2/onnx/model.onnx (+ tokenizer.json, modules.json,
//!   1_Pooling/config.json, sentence_bert_config.json, config.json)
//!   cross-encoder/ms-marco-MiniLM-L6-v2/onnx/model.onnx (+ tokenizer.json, config.json)
//! cargo test -p harness-onnx --test e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The reference vector is what `sentence_transformers.SentenceTransformer("all-MiniLM-L6-v2")
//! .encode("This is an example sentence")` returns (first ten values, the model card's own
//! quick-start output); the reranker scores are the ones the cross-encoder model card lists for
//! its Berlin example.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_harness-onnx"))
            .args(["--model"])
            .arg(model)
            .args(["--ort-lib"])
            .arg(lib)
            .args(["--provider", provider, "--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start harness-onnx");
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                if let Some(base) = line.strip_prefix("harness-onnx: listening on ") {
                    let _ = tx.send(base.to_owned());
                }
            }
        });
        let mut server = Server {
            child,
            base: String::new(),
        };
        server.base = rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let base = &server.base;
        let started = Instant::now();
        loop {
            if let Ok(r) = reqwest::blocking::get(format!("{base}/health")) {
                if r.status() == 200 {
                    break;
                }
            }
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "server exited before ready"
            );
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "the server did not become healthy"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        server
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = reqwest::blocking::Client::new()
            .post(format!("{}{}", self.base, path))
            .json(&body)
            .send()
            .unwrap();
        (r.status().as_u16(), r.json().unwrap_or(Value::Null))
    }

    fn embed(&self, texts: &[&str]) -> Vec<Vec<f32>> {
        let (status, v) = self.post("/v1/embeddings", json!({"input": texts}));
        assert_eq!(status, 200, "{v}");
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                d["embedding"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap() as f32)
                    .collect()
            })
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// First ten values of the model card's `encode("This is an example sentence")`.
const MINILM_REFERENCE: [f32; 10] = [
    0.067_656_9,
    0.063_495_9,
    0.048_713_1,
    0.079_304_9,
    0.037_448_1,
    0.002_652_8,
    0.039_375_0,
    -0.007_098_4,
    0.059_361_4,
    0.031_537_0,
];

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    harness_onnx::pooling::cosine(a, b)
}

fn minilm(models: &Path) -> PathBuf {
    models.join("sentence-transformers/all-MiniLM-L6-v2/onnx/model.onnx")
}

fn cross_encoder(models: &Path) -> PathBuf {
    models.join("cross-encoder/ms-marco-MiniLM-L6-v2/onnx/model.onnx")
}

fn check_embeddings(s: &Server, provider: &str, tolerance: f32) -> Vec<f32> {
    let v = s.embed(&[
        "This is an example sentence",
        "Each sentence is converted",
        "A dog is chasing a ball in the park",
        "A puppy runs after a toy in the garden",
        "Quarterly revenue grew by twelve percent",
    ]);
    assert_eq!(v[0].len(), 384);
    for (got, want) in v[0].iter().zip(MINILM_REFERENCE) {
        assert!(
            (got - want).abs() < tolerance,
            "{provider}: {got} vs sentence-transformers {want}"
        );
    }
    // Normalized (the model has a Normalize module).
    let norm = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-3, "{norm}");
    // Semantic sanity: paraphrases are closer than unrelated sentences.
    let (dog, puppy, money) = (&v[2], &v[3], &v[4]);
    eprintln!(
        "{provider}: dog~puppy {:.3}, dog~revenue {:.3}",
        cosine(dog, puppy),
        cosine(dog, money)
    );
    assert!(cosine(dog, puppy) > 0.3 && cosine(dog, puppy) > cosine(dog, money) + 0.2);
    // Batching does not change a vector: the same text alone gives the same result.
    let alone = s.embed(&["A dog is chasing a ball in the park"]);
    assert!(
        cosine(&alone[0], dog) > 0.9999,
        "{}",
        cosine(&alone[0], dog)
    );
    v[0].clone()
}

#[test]
#[ignore = "needs HS_ORT_LIB and HS_ONNX_MODELS"]
fn minilm_matches_sentence_transformers_on_the_cpu() {
    let (Some(lib), Some(models)) = (env_path("HS_ORT_LIB"), env_path("HS_ONNX_MODELS")) else {
        return;
    };
    let s = Server::start(&minilm(&models), &lib, "cpu");
    check_embeddings(&s, "cpu", 2e-5);

    // OpenAI shapes: base64, dimensions, usage, errors.
    let (status, v) = s.post(
        "/v1/embeddings",
        json!({"model": "x", "input": "hello world", "encoding_format": "base64"}),
    );
    assert_eq!(status, 200);
    assert!(v["data"][0]["embedding"].is_string());
    assert!(v["usage"]["prompt_tokens"].as_u64().unwrap() >= 3);
    let (status, v) = s.post(
        "/v1/embeddings",
        json!({"input": ["a", "b"], "dimensions": 64}),
    );
    assert_eq!(status, 200);
    let short = v["data"][1]["embedding"].as_array().unwrap();
    assert_eq!(short.len(), 64);
    let norm: f64 = short.iter().map(|x| x.as_f64().unwrap().powi(2)).sum();
    assert!((norm - 1.0).abs() < 1e-3);
    let (status, v) = s.post("/v1/embeddings", json!({"input": []}));
    assert_eq!(status, 400, "{v}");
    // An embedding model is not a reranker.
    let (status, v) = s.post("/v1/rerank", json!({"query": "q", "documents": ["d"]}));
    assert_eq!(status, 501, "{v}");
    // A text longer than the model's window is truncated, not refused.
    let long = "word ".repeat(5000);
    let (status, v) = s.post("/v1/embeddings", json!({"input": long}));
    assert_eq!(status, 200, "{v}");
    assert!(v["usage"]["prompt_tokens"].as_u64().unwrap() <= 256);
    let models_list: Value = reqwest::blocking::get(format!("{}/v1/models", s.base))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(models_list["data"][0]["object"], "model");
}

#[test]
#[ignore = "needs HS_ORT_LIB and HS_ONNX_MODELS"]
fn cross_encoder_ranks_the_berlin_example() {
    let (Some(lib), Some(models)) = (env_path("HS_ORT_LIB"), env_path("HS_ONNX_MODELS")) else {
        return;
    };
    let s = Server::start(&cross_encoder(&models), &lib, "cpu");
    let docs = [
        "Berlin had a population of 3,520,031 registered inhabitants in an area of 891.82 square kilometers.",
        "Berlin is well known for its museums.",
        "The weather is nice today.",
    ];
    // Shuffled on purpose: the answer must come out by score, with the original indices.
    let (status, v) = s.post(
        "/v1/rerank",
        json!({"query": "How many people live in Berlin?",
               "documents": [docs[2], docs[0], docs[1]], "top_n": 3}),
    );
    assert_eq!(status, 200, "{v}");
    let results = v["results"].as_array().unwrap();
    let order: Vec<u64> = results
        .iter()
        .map(|r| r["index"].as_u64().unwrap())
        .collect();
    assert_eq!(order, vec![1, 2, 0], "{v}");
    let score = |i: usize| results[i]["relevance_score"].as_f64().unwrap();
    // The cross-encoder model card's logits (8.607, -4.320, -11.14).
    assert!((score(0) - 8.607).abs() < 0.01, "{}", score(0));
    assert!((score(1) - (-4.320)).abs() < 0.01, "{}", score(1));
    assert!((score(2) - (-11.140)).abs() < 0.01, "{}", score(2));
    // top_n and return_documents.
    let (_, v) = s.post(
        "/v1/rerank",
        json!({"query": "How many people live in Berlin?", "documents": docs, "top_n": 1,
               "return_documents": true}),
    );
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    assert_eq!(v["results"][0]["document"]["text"], docs[0]);
    // A reranker is not an embedding model.
    let (status, _) = s.post("/v1/embeddings", json!({"input": "x"}));
    assert_eq!(status, 501);
}

#[test]
#[ignore = "needs HS_ORT_LIB_DML and HS_ONNX_MODELS"]
fn directml_agrees_with_the_cpu() {
    let (Some(lib), Some(models)) = (env_path("HS_ORT_LIB_DML"), env_path("HS_ONNX_MODELS")) else {
        return;
    };
    let dml = Server::start(&minilm(&models), &lib, "directml");
    let on_gpu = check_embeddings(&dml, "directml", 1e-3);
    let cpu = Server::start(&minilm(&models), &lib, "cpu");
    let on_cpu = check_embeddings(&cpu, "cpu", 1e-3);
    assert!(cosine(&on_gpu, &on_cpu) > 0.999);

    let ce = Server::start(&cross_encoder(&models), &lib, "directml");
    let (status, v) = ce.post(
        "/v1/rerank",
        json!({"query": "How many people live in Berlin?",
               "documents": ["Berlin is well known for its museums.",
                             "Berlin had a population of 3,520,031 registered inhabitants.",
                             "The weather is nice today."]}),
    );
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["results"][0]["index"], 1);
}
