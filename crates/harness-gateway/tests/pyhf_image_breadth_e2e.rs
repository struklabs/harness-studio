//! The Python engine's image breadth end to end (no mocks): the managed `hf-cpu` environment is
//! built from its hashed lock (it holds PEFT for LoRA), a Stable Diffusion inpainting checkpoint
//! inpaints under an OpenAI or a black and white mask, one LoRA loads for one request and is gone
//! for the next, a Swin2SR model upscales an image through `/v1/images/upscale`, and the gateway
//! refuses what the worker would stretch or ignore (a mask of another size, a bad LoRA).
//!
//! ```text
//! HS_IMGB_MODELS=<models dir with <owner>/<repo> folders: stable-diffusion-v1-5/stable-diffusion-inpainting,
//!                 caidas/swin2SR-classical-sr-x2-64 and local/tiny-sd (a tiny random SD pipeline)>
//! HS_IMGB_INPUT=<folder with start.png (256x256), mask_openai.png, mask_bw.png, mask_small.png (128x128), small.png (128x96)>
//! HS_IMGB_LORA=<an SD 1.5 LoRA .safetensors, e.g. LCM-LoRA> HS_IMGB_TINY_LORA=<a LoRA made for local/tiny-sd>
//! HS_IMGB_OUT=<folder for the result PNGs> HS_IMGB_ROOT=<empty folder for the data root and the environment>
//!   cargo test -p harness-gateway --test pyhf_image_breadth_e2e -- --ignored --nocapture
//! ```
//!
//! Without `HS_IMGB_ROOT` the data root is a temporary folder (uv, Python and the packages:
//! about 1.5 GB). The test skips when a variable is not set.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use base64::Engine as _;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn file(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

fn data_url(path: &Path) -> String {
    format!(
        "data:image/png;base64,{}",
        B64.encode(std::fs::read(path).unwrap())
    )
}

fn decode(b64: &str) -> Vec<u8> {
    B64.decode(b64).unwrap()
}

fn png_size(b: &[u8]) -> (u32, u32) {
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    (
        u32::from_be_bytes(b[16..20].try_into().unwrap()),
        u32::from_be_bytes(b[20..24].try_into().unwrap()),
    )
}

/// Mean absolute difference of two PNGs of one size, over the pixels `inside` says yes to.
fn mean_diff(a: &[u8], b: &[u8], inside: impl Fn(u32, u32) -> bool) -> f64 {
    let a = image::load_from_memory(a).unwrap().to_rgb8();
    let b = image::load_from_memory(b).unwrap().to_rgb8();
    assert_eq!(a.dimensions(), b.dimensions());
    let (mut sum, mut n) = (0f64, 0f64);
    for (x, y, p) in a.enumerate_pixels() {
        if inside(x, y) {
            let q = b.get_pixel(x, y);
            sum += (0..3)
                .map(|i| (p[i] as f64 - q[i] as f64).abs())
                .sum::<f64>()
                / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

struct Gw {
    http: reqwest::Client,
    base: String,
    out: PathBuf,
}

impl Gw {
    async fn post(&self, path: &str, body: Value) -> (u16, Value, f64) {
        let t = Instant::now();
        let res = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        let code = res.status().as_u16();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        (code, v, t.elapsed().as_secs_f64())
    }

    /// An images answer's first picture (PNG bytes), saved as `<name>.png`.
    fn picture(&self, name: &str, body: &Value) -> Vec<u8> {
        let png = decode(body["data"][0]["b64_json"].as_str().expect("an image"));
        std::fs::write(self.out.join(format!("{name}.png")), &png).unwrap();
        png
    }
}

#[tokio::test]
#[ignore = "builds the hf-cpu environment (~1.5 GB) and needs the HS_IMGB_* models"]
async fn python_image_breadth_through_the_gateway() {
    let (Some(models), Some(input), Some(lora), Some(tiny_lora), Some(out)) = (
        dir("HS_IMGB_MODELS"),
        dir("HS_IMGB_INPUT"),
        file("HS_IMGB_LORA"),
        file("HS_IMGB_TINY_LORA"),
        dir("HS_IMGB_OUT"),
    ) else {
        eprintln!("HS_IMGB_* not set, skipping");
        return;
    };
    // A request's LoRA file must sit inside a Harness model folder.
    let in_models = |file: &std::path::Path| {
        let dir = models.join("loras");
        std::fs::create_dir_all(&dir).unwrap();
        let to = dir.join(file.file_name().unwrap());
        if to != file {
            std::fs::copy(file, &to).unwrap();
        }
        to
    };
    let (lora, tiny_lora) = (in_models(&lora), in_models(&tiny_lora));
    let inpaint_dir = models.join("stable-diffusion-v1-5/stable-diffusion-inpainting");
    let swin_dir = models.join("caidas/swin2SR-classical-sr-x2-64");
    let tiny_dir = models.join("local/tiny-sd");
    harness_core::secrets::use_memory_store();
    let root = match std::env::var_os("HS_IMGB_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("pyhf-imgb-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(root.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.models_dir = models.to_string_lossy().to_string();
    settings.control_api.port = 11782;
    settings.save().unwrap();

    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("hf-cpu").await.unwrap();
    eprintln!("environment in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::python_hf::binary().unwrap();

    // The library: an inpainting checkpoint is an image-to-image folder, Swin2SR its own task.
    assert_eq!(
        harness_core::engine::python_hf::detect(&inpaint_dir).as_deref(),
        Some("image-to-image")
    );
    assert_eq!(
        harness_core::engine::python_hf::detect(&tiny_dir).as_deref(),
        Some("text-to-image")
    );
    assert_eq!(
        harness_core::engine::python_hf::detect(&swin_dir).as_deref(),
        Some("image-super-resolution")
    );
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    for (path, task) in [
        (&inpaint_dir, "image-to-image"),
        (&tiny_dir, "text-to-image"),
        (&swin_dir, "image-super-resolution"),
    ] {
        let found = library
            .iter()
            .find(|m| &m.path == path)
            .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
        assert_eq!(found.kind, harness_core::domain::ModelKind::Pipeline);
        assert_eq!(found.task.as_deref(), Some(task), "{path:?}");
        eprintln!("library: {} {:?} {}", found.name, found.architecture, task);
    }

    let store = harness_core::domain::store::Domain::global();
    for (alias, path) in [
        ("inpaint", &inpaint_dir),
        ("tiny-sd", &tiny_dir),
        ("swin2sr", &swin_dir),
    ] {
        for old in store.definitions().unwrap() {
            if old.alias == alias {
                store.delete_definition(&old.id).unwrap();
            }
        }
        store
            .save_definition(ModelDefinition::new(
                alias,
                ModelRef::Local { path: path.clone() },
            ))
            .unwrap();
    }
    harness_core::servers::save_profile(harness_core::servers::ServerProfile {
        id: "keep-off-11440".into(),
        name: "Keep Off 11440".into(),
        model_id: None,
        description: String::new(),
        config: harness_core::process::LaunchConfig {
            host: "127.0.0.1".into(),
            port: 11440,
            ..Default::default()
        },
        auto_start: false,
        updated_at: 0,
    })
    .unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let logs = registry.clone();
    tokio::spawn(async move {
        let mut seen = std::collections::HashMap::<String, u64>::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            for (id, pm) in logs.all() {
                let since = seen.entry(id.clone()).or_default();
                let (next, lines) = pm.get_run_logs_since(*since);
                *since = next;
                for line in lines {
                    eprintln!("[worker {id}] {line}");
                }
            }
        }
    });
    let server = GatewayServer::new();
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: root.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 11781,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let gw = Gw {
        http: reqwest::Client::new(),
        base: status.endpoint.unwrap().host_url,
        out,
    };

    let start = data_url(&input.join("start.png"));
    let openai_mask = data_url(&input.join("mask_openai.png"));
    let bw_mask = data_url(&input.join("mask_bw.png"));
    let small_mask = data_url(&input.join("mask_small.png"));
    // The hole of both masks (x 60..200, y 60..170).
    let in_hole = |x: u32, y: u32| (60..200).contains(&x) && (60..170).contains(&y);
    let outside = |x: u32, y: u32| !in_hole(x, y);

    // --- super-resolution -------------------------------------------------------------------
    let small = data_url(&input.join("small.png"));
    let (code, body, secs) = gw
        .post(
            "/v1/images/upscale",
            json!({"model": "swin2sr", "image": small}),
        )
        .await;
    eprintln!("upscale x2 (128x96) in {secs:.1}s (load included): code {code}");
    assert_eq!(code, 200, "{body}");
    let png = gw.picture("upscaled-x2", &body);
    assert_eq!(png_size(&png), (256, 192));
    assert_eq!(
        (
            body["width"].as_u64(),
            body["height"].as_u64(),
            body["scale"].as_u64()
        ),
        (Some(256), Some(192), Some(2))
    );
    // A second request is warm.
    let (code, body, secs) = gw
        .post(
            "/v1/images/upscale",
            json!({"model": "swin2sr", "image": data_url(&input.join("small.png")), "scale": 2, "response_format": "url"}),
        )
        .await;
    eprintln!("upscale x2 warm in {secs:.2}s");
    assert_eq!(code, 200, "{body}");
    assert!(body["data"][0]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    for (what, request, status, needle) in [
        (
            "another scale",
            json!({"model": "swin2sr", "image": data_url(&input.join("small.png")), "scale": 4}),
            400,
            "upscales by 2",
        ),
        (
            "a URL",
            json!({"model": "swin2sr", "image": "https://example.com/a.png"}),
            400,
            "does not fetch URLs",
        ),
        (
            "a drawing model",
            json!({"model": "tiny-sd", "image": data_url(&input.join("small.png"))}),
            400,
            "needs a model for the image-super-resolution task",
        ),
    ] {
        let (code, body, _) = gw.post("/v1/images/upscale", request).await;
        assert_eq!(code, status, "{what}: {body}");
        assert!(body.to_string().contains(needle), "{what}: {body}");
    }
    // The upscale model is no image model.
    let (code, body, _) = gw
        .post(
            "/v1/images/generations",
            json!({"model": "swin2sr", "prompt": "a fox"}),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["error"]["code"], "model_not_image", "{body}");

    // --- inpainting ---------------------------------------------------------------------------
    let edit = |mask: &str, extra: Value| {
        let mut b = json!({
            "model": "inpaint", "prompt": "a red castle on a hill", "image": start,
            "mask": mask, "steps": 4, "cfg_scale": 5, "seed": 7, "strength": 1.0
        });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let (code, body, secs) = gw
        .post("/v1/images/edits", edit(&openai_mask, json!({})))
        .await;
    eprintln!("inpaint (OpenAI mask, 256x256, 4 steps) in {secs:.1}s (load included): code {code}");
    assert_eq!(code, 200, "{body}");
    let plain = gw.picture("inpaint-openai-mask", &body);
    assert_eq!(png_size(&plain), (256, 256));
    let original = std::fs::read(input.join("start.png")).unwrap();
    let kept = mean_diff(&plain, &original, outside);
    let redrawn = mean_diff(&plain, &original, in_hole);
    eprintln!(
        "mean difference to the start image: outside the mask {kept:.1}, inside {redrawn:.1}"
    );
    assert!(
        redrawn > kept * 1.5,
        "the masked area should change more than the rest ({redrawn} vs {kept})"
    );
    // The black and white mask (white = repaint) paints the same picture.
    let (code, body, secs) = gw.post("/v1/images/edits", edit(&bw_mask, json!({}))).await;
    eprintln!("inpaint (black and white mask) warm in {secs:.1}s");
    assert_eq!(code, 200, "{body}");
    let bw = gw.picture("inpaint-bw-mask", &body);
    assert!(
        mean_diff(&bw, &plain, |_, _| true) < 2.0,
        "both masks mark the same area"
    );
    // What the gateway and the worker refuse.
    let (code, body, _) = gw
        .post("/v1/images/edits", edit(&small_mask, json!({})))
        .await;
    assert_eq!(
        (code, body["error"]["param"].as_str()),
        (400, Some("mask")),
        "{body}"
    );
    assert!(body.to_string().contains("128x128"), "{body}");
    let mut no_mask = edit(&bw_mask, json!({}));
    no_mask.as_object_mut().unwrap().remove("mask");
    let (code, body, _) = gw.post("/v1/images/edits", no_mask).await;
    assert_eq!(code, 400, "{body}");
    assert!(body.to_string().contains("send a mask"), "{body}");
    let (code, body, _) = gw
        .post("/v1/images/edits", edit(&bw_mask, json!({"strength": 1.5})))
        .await;
    assert_eq!(
        (code, body["error"]["param"].as_str()),
        (400, Some("strength")),
        "{body}"
    );
    let (code, body, _) = gw
        .post(
            "/v1/images/generations",
            json!({"model": "inpaint", "prompt": "a fox"}),
        )
        .await;
    assert_eq!(
        (code, body["error"]["code"].as_str()),
        (400, Some("image_required")),
        "{body}"
    );

    // --- LoRA: one request only ------------------------------------------------------------------
    let lora_path = lora.to_string_lossy().to_string();
    let (code, body, secs) = gw
        .post(
            "/v1/images/edits",
            edit(
                &openai_mask,
                json!({"lora": lora_path, "lora_scale": 1.0, "steps": 4}),
            ),
        )
        .await;
    eprintln!("inpaint with a LoRA in {secs:.1}s: code {code}");
    assert_eq!(code, 200, "{body}");
    let with_lora = gw.picture("inpaint-with-lora", &body);
    assert!(
        mean_diff(&with_lora, &plain, |_, _| true) > 1.0,
        "the LoRA changed nothing"
    );
    // The next request has none again: the same picture as the first.
    let (code, body, _) = gw
        .post("/v1/images/edits", edit(&openai_mask, json!({})))
        .await;
    assert_eq!(code, 200, "{body}");
    let again = gw.picture("inpaint-after-lora", &body);
    assert_eq!(
        mean_diff(&again, &plain, |_, _| true),
        0.0,
        "the LoRA stayed loaded"
    );
    // A scale of 0 is the plain model too.
    let (code, body, _) = gw
        .post(
            "/v1/images/edits",
            edit(&openai_mask, json!({"lora": lora_path, "lora_scale": 0.0})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let zero = gw.picture("inpaint-lora-scale-0", &body);
    assert!(mean_diff(&zero, &plain, |_, _| true) < 0.5);
    // Bad LoRAs are refused before the model works.
    for (lora, scale, param) in [
        (json!("/nope/missing.safetensors"), json!(1), "lora"),
        (json!("style.safetensors"), json!(1), "lora"),
        (json!(lora_path), json!(9), "lora_scale"),
    ] {
        let (code, body, _) = gw
            .post(
                "/v1/images/edits",
                edit(&openai_mask, json!({"lora": lora, "lora_scale": scale})),
            )
            .await;
        assert_eq!(
            (code, body["error"]["param"].as_str()),
            (400, Some(param)),
            "{body}"
        );
    }
    // A repository that is not in the Hugging Face cache cannot be fetched (the worker is offline).
    let (code, body, _) = gw
        .post(
            "/v1/images/edits",
            edit(&openai_mask, json!({"lora": "owner/not-in-cache"})),
        )
        .await;
    assert_eq!(code, 400, "{body}");
    assert!(body.to_string().contains("Hugging Face cache"), "{body}");

    // A generation on a text-to-image pipeline takes a LoRA too.
    let generate = |extra: Value| {
        let mut b = json!({
            "model": "tiny-sd", "prompt": "a", "size": "64x64", "steps": 2, "seed": 5
        });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let (code, body, _) = gw.post("/v1/images/generations", generate(json!({}))).await;
    assert_eq!(code, 200, "{body}");
    let tiny_plain = gw.picture("tiny-plain", &body);
    let tiny_lora = tiny_lora.to_string_lossy().to_string();
    let (code, body, _) = gw
        .post(
            "/v1/images/generations",
            generate(json!({"lora": tiny_lora, "lora_scale": 1.0})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let tiny_with = gw.picture("tiny-with-lora", &body);
    assert!(mean_diff(&tiny_with, &tiny_plain, |_, _| true) > 0.5);
    let (code, body, _) = gw.post("/v1/images/generations", generate(json!({}))).await;
    assert_eq!(code, 200, "{body}");
    let tiny_after = gw.picture("tiny-after", &body);
    assert_eq!(mean_diff(&tiny_after, &tiny_plain, |_, _| true), 0.0);

    server.stop().await;
}
