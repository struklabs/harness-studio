use super::workflow::*;
use super::*;

use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{Path as AxPath, RawQuery, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];

// --- Graphs ---

fn object_info() -> Value {
    json!({
        "CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [["a.safetensors", "b.safetensors"], {}]}}},
        "LoraLoader": {"input": {"required": {
            "model": ["MODEL"], "clip": ["CLIP"], "lora_name": [["l.safetensors"]],
            "strength_model": ["FLOAT", {}], "strength_clip": ["FLOAT", {}]
        }}},
        "CLIPTextEncode": {"input": {"required": {"text": ["STRING", {"multiline": true}], "clip": ["CLIP"]}}},
        "EmptyLatentImage": {"input": {"required": {"width": ["INT", {}], "height": ["INT", {}], "batch_size": ["INT", {}]}}},
        "KSampler": {
            "input": {"required": {
                "model": ["MODEL"], "seed": ["INT", {"control_after_generate": true}], "steps": ["INT", {}],
                "cfg": ["FLOAT", {}], "sampler_name": [["euler"]], "scheduler": [["normal"]],
                "positive": ["CONDITIONING"], "negative": ["CONDITIONING"], "latent_image": ["LATENT"],
                "denoise": ["FLOAT", {}]
            }},
            "input_order": {"required": ["model", "seed", "steps", "cfg", "sampler_name", "scheduler", "positive", "negative", "latent_image", "denoise"]}
        },
        "VAEDecode": {"input": {"required": {"samples": ["LATENT"], "vae": ["VAE"]}}},
        "SaveImage": {"input": {"required": {"images": ["IMAGE"], "filename_prefix": ["STRING", {}]}}},
        "PreviewImage": {"input": {"required": {"images": ["IMAGE"]}}}
    })
}

/// An editor workflow: a bypassed LoRA between checkpoint and sampler, a reroute before the
/// save node, a primitive driving the steps, a note and a muted preview.
fn ui_workflow() -> Value {
    json!({
        "nodes": [
            {"id": 4, "type": "CheckpointLoaderSimple", "mode": 0, "inputs": [],
             "outputs": [{"type": "MODEL"}, {"type": "CLIP"}, {"type": "VAE"}], "widgets_values": ["b.safetensors"]},
            {"id": 10, "type": "LoraLoader", "mode": 4,
             "inputs": [{"name": "model", "type": "MODEL", "link": 1}, {"name": "clip", "type": "CLIP", "link": 2}],
             "outputs": [{"type": "MODEL"}, {"type": "CLIP"}], "widgets_values": ["l.safetensors", 1.0, 1.0]},
            {"id": 6, "type": "CLIPTextEncode", "mode": 0, "title": "Positive",
             "inputs": [{"name": "clip", "type": "CLIP", "link": 3}], "widgets_values": ["a cat"]},
            {"id": 7, "type": "CLIPTextEncode", "mode": 0,
             "inputs": [{"name": "clip", "type": "CLIP", "link": 4}], "widgets_values": ["blurry"]},
            {"id": 5, "type": "EmptyLatentImage", "mode": 0, "inputs": [], "widgets_values": [512, 512, 1]},
            {"id": 3, "type": "KSampler", "mode": 0,
             "inputs": [
                {"name": "model", "type": "MODEL", "link": 5},
                {"name": "positive", "type": "CONDITIONING", "link": 6},
                {"name": "negative", "type": "CONDITIONING", "link": 7},
                {"name": "latent_image", "type": "LATENT", "link": 8},
                {"name": "steps", "type": "INT", "link": 13, "widget": {"name": "steps"}}
             ],
             "widgets_values": [42, "randomize", 25, 6.5, "euler", "normal", 1.0]},
            {"id": 8, "type": "VAEDecode", "mode": 0,
             "inputs": [{"name": "samples", "type": "LATENT", "link": 9}, {"name": "vae", "type": "VAE", "link": 10}]},
            {"id": 20, "type": "Reroute", "mode": 0, "inputs": [{"name": "", "type": "*", "link": 11}], "outputs": [{"type": "IMAGE"}]},
            {"id": 9, "type": "SaveImage", "mode": 0, "inputs": [{"name": "images", "type": "IMAGE", "link": 12}], "widgets_values": ["mine"]},
            {"id": 30, "type": "PrimitiveNode", "mode": 0, "outputs": [{"type": "INT"}], "widgets_values": [25, "fixed"]},
            {"id": 31, "type": "Note", "mode": 0, "widgets_values": ["remember"]},
            {"id": 32, "type": "PreviewImage", "mode": 2, "inputs": [{"name": "images", "type": "IMAGE", "link": 14}]}
        ],
        "links": [
            [1, 4, 0, 10, 0, "MODEL"], [2, 4, 1, 10, 1, "CLIP"],
            [3, 10, 1, 6, 0, "CLIP"], [4, 10, 1, 7, 0, "CLIP"],
            [5, 10, 0, 3, 0, "MODEL"], [6, 6, 0, 3, 1, "CONDITIONING"], [7, 7, 0, 3, 2, "CONDITIONING"],
            [8, 5, 0, 3, 3, "LATENT"], [9, 3, 0, 8, 0, "LATENT"], [10, 4, 2, 8, 1, "VAE"],
            [11, 8, 0, 20, 0, "IMAGE"], [12, 20, 0, 9, 0, "IMAGE"], [13, 30, 0, 3, 4, "INT"],
            [14, 8, 0, 32, 0, "IMAGE"]
        ]
    })
}

fn params() -> Txt2ImgParams {
    Txt2ImgParams {
        prompt: "a red fox".into(),
        ..Default::default()
    }
}

#[test]
fn urls_and_names_are_validated() {
    assert_eq!(
        normalize_url(" http://127.0.0.1:8188/ ").unwrap(),
        "http://127.0.0.1:8188"
    );
    assert_eq!(
        normalize_url("https://gpu.lan/comfy/").unwrap(),
        "https://gpu.lan/comfy"
    );
    for bad in [
        "",
        "127.0.0.1:8188",
        "ftp://x",
        "http://u:p@x",
        "http://x/?a=1",
        "http://x/#f",
    ] {
        assert!(
            normalize_url(bad).unwrap_err().contains(ERR_URL_INVALID),
            "{bad}"
        );
    }
    assert!(saved_path_ok("portraits/sdxl.json"));
    for bad in [
        "",
        "../x.json",
        "a/../x.json",
        "/x.json",
        "a//x.json",
        "a\\x.json",
        "x.txt",
    ] {
        assert!(!saved_path_ok(bad), "{bad}");
    }
    assert!(template_name_ok("default") && template_name_ok("image_flux_text_to_image"));
    assert!(!template_name_ok("..") && !template_name_ok("a/b") && !template_name_ok(""));
}

#[test]
fn parameters_are_checked() {
    assert!(params().check().is_ok());
    let bad = |f: fn(&mut Txt2ImgParams), key: &str| {
        let mut p = params();
        f(&mut p);
        assert!(p.check().unwrap_err().contains(key), "{key}");
    };
    bad(|p| p.prompt = "  ".into(), "comfyui.prompt_empty");
    bad(|p| p.width = Some(500), "comfyui.size_invalid");
    bad(|p| p.height = Some(16), "comfyui.size_invalid");
    bad(|p| p.steps = Some(0), "comfyui.steps_invalid");
    bad(|p| p.cfg = Some(-1.0), "comfyui.cfg_invalid");
    bad(
        |p| p.prompt = "x".repeat(MAX_PROMPT_CHARS + 1),
        "comfyui.prompt_too_long",
    );
}

#[test]
fn builtin_graph_takes_the_parameters() {
    let mut g = builtin_txt2img("a.safetensors");
    let p = Txt2ImgParams {
        prompt: " a red fox ".into(),
        negative: "blurry".into(),
        width: Some(768),
        height: Some(512),
        steps: Some(8),
        cfg: Some(2.5),
        checkpoint: Some("b.safetensors".into()),
        ..Default::default()
    };
    apply_params(&mut g, &p, 1234).unwrap();
    assert_eq!(g["6"]["inputs"]["text"], "a red fox");
    assert_eq!(g["7"]["inputs"]["text"], "blurry");
    let k = &g["3"]["inputs"];
    assert_eq!(
        (k["seed"].as_u64(), k["steps"].as_u64(), k["cfg"].as_f64()),
        (Some(1234), Some(8), Some(2.5))
    );
    assert_eq!(k["model"], json!(["4", 0]));
    assert_eq!(
        (
            g["5"]["inputs"]["width"].as_u64(),
            g["5"]["inputs"]["height"].as_u64()
        ),
        (Some(768), Some(512))
    );
    assert_eq!(g["4"]["inputs"]["ckpt_name"], "b.safetensors");
    assert_eq!(g["9"]["inputs"]["filename_prefix"], FILENAME_PREFIX);
    assert!(is_api_format(&Value::Object(g)));

    // An empty negative keeps the workflow's own; a graph without a text prompt is refused.
    let mut g = builtin_txt2img("a.safetensors");
    g["7"]["inputs"]["text"] = json!("ugly");
    apply_params(&mut g, &params(), 1).unwrap();
    assert_eq!(g["7"]["inputs"]["text"], "ugly");
    let mut no_text = builtin_txt2img("a.safetensors");
    no_text.remove("6");
    assert!(apply_params(&mut no_text, &params(), 1)
        .unwrap_err()
        .contains("comfyui.no_prompt_input"));
}

#[test]
fn editor_workflows_are_converted() {
    let ui = ui_workflow();
    assert!(is_ui_format(&ui) && !is_api_format(&ui));
    let mut g = ui_to_api(&ui, &object_info()).unwrap();
    // Notes, reroutes, primitives, muted and bypassed nodes are not sent.
    let mut ids: Vec<&str> = g.keys().map(String::as_str).collect();
    ids.sort();
    assert_eq!(ids, ["3", "4", "5", "6", "7", "8", "9"]);
    let k = &g["3"]["inputs"];
    // The bypassed LoRA hands its own inputs on; the control widget after the seed is skipped;
    // the primitive's value stays in the widget it drives.
    assert_eq!(k["model"], json!(["4", 0]));
    assert_eq!(g["6"]["inputs"]["clip"], json!(["4", 1]));
    assert_eq!(
        (k["seed"].as_u64(), k["steps"].as_u64(), k["cfg"].as_f64()),
        (Some(42), Some(25), Some(6.5))
    );
    assert_eq!(
        (k["sampler_name"].as_str(), k["denoise"].as_f64()),
        (Some("euler"), Some(1.0))
    );
    assert_eq!(k["positive"], json!(["6", 0]));
    assert_eq!(g["9"]["inputs"]["images"], json!(["8", 0]));
    assert_eq!(g["9"]["inputs"]["filename_prefix"], "mine");
    assert_eq!(g["6"]["_meta"]["title"], "Positive");

    apply_params(
        &mut g,
        &Txt2ImgParams {
            width: Some(1024),
            ..params()
        },
        7,
    )
    .unwrap();
    assert_eq!(g["6"]["inputs"]["text"], "a red fox");
    assert_eq!(g["7"]["inputs"]["text"], "blurry");
    assert_eq!(g["3"]["inputs"]["seed"], 7);
    assert_eq!(g["5"]["inputs"]["width"], 1024);
    assert_eq!(g["5"]["inputs"]["height"], 512);

    // Version 1 serialization keeps links as objects.
    let mut v1 = ui_workflow();
    let links: Vec<Value> = v1["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| json!({"id": l[0], "origin_id": l[1], "origin_slot": l[2], "target_id": l[3], "target_slot": l[4], "type": l[5]}))
        .collect();
    v1["links"] = json!(links);
    assert_eq!(
        ui_to_api(&v1, &object_info()).unwrap(),
        ui_to_api(&ui, &object_info()).unwrap()
    );
}

#[test]
fn unsupported_workflows_say_why() {
    let mut sub = ui_workflow();
    sub["definitions"] = json!({"subgraphs": [{"id": "x"}]});
    assert!(ui_to_api(&sub, &object_info())
        .unwrap_err()
        .contains("comfyui.workflow_unsupported"));
    let mut unknown = ui_workflow();
    unknown["nodes"][0]["type"] = json!("FancyLoader");
    let e = ui_to_api(&unknown, &object_info()).unwrap_err();
    assert!(
        e.contains("comfyui.missing_node") && e.contains("FancyLoader"),
        "{e}"
    );
    assert!(ui_to_api(&json!({"nodes": []}), &object_info())
        .unwrap_err()
        .contains("comfyui.workflow_invalid"));
}

#[test]
fn saved_outputs_win_over_previews() {
    let entry = json!({"outputs": {
        "9": {"images": [{"filename": "a.png", "subfolder": "", "type": "output"}]},
        "10": {"images": [{"filename": "p.png", "subfolder": "", "type": "temp"}, {"filename": "../x", "type": "input"}]}
    }});
    let refs = output_images(&entry);
    assert_eq!(refs.len(), 1);
    assert_eq!(
        (refs[0].filename.as_str(), refs[0].kind.as_str()),
        ("a.png", "output")
    );
    let previews = json!({"outputs": {"10": {"images": [{"filename": "p.png", "subfolder": "s", "type": "temp"}]}}});
    assert_eq!(output_images(&previews)[0].subfolder, "s");
}

#[test]
fn images_are_saved_without_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let img = |name: &str| RunImage {
        filename: name.into(),
        bytes: PNG.to_vec(),
    };
    let saved = save_images(
        dir.path(),
        &[
            img("HarnessStudio_00001_.png"),
            img("HarnessStudio_00001_.png"),
            img("../../evil.png"),
        ],
    )
    .unwrap();
    let names: Vec<String> = saved
        .iter()
        .map(|s| {
            Path::new(&s.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        names,
        [
            "HarnessStudio_00001_.png",
            "HarnessStudio_00001_ (2).png",
            "evil.png"
        ]
    );
    assert!(saved
        .iter()
        .all(|s| s.format == "png" && Path::new(&s.path).starts_with(dir.path())));
    let bad = RunImage {
        filename: "x.png".into(),
        bytes: b"GIF89a".to_vec(),
    };
    assert!(save_images(dir.path(), &[bad])
        .unwrap_err()
        .contains("comfyui.invalid_image"));
}

// --- A mock ComfyUI ---

#[derive(Default)]
struct Mock {
    /// Graphs posted to `/prompt`.
    prompts: Vec<Value>,
    /// Runs never finish (to test cancelling); removed from the queue by `delete`.
    hang: bool,
    deleted: Vec<String>,
    interrupted: bool,
    /// The socket has sent the steps: `/history` may report the run.
    finished: bool,
}

type Shared = Arc<Mutex<Mock>>;

const PROMPT_ID: &str = "11111111-2222-3333-4444-555555555555";

async fn start(hang: bool) -> (String, Shared) {
    let state: Shared = Arc::new(Mutex::new(Mock {
        hang,
        ..Default::default()
    }));
    let app = Router::new()
        .route(
            "/system_stats",
            get(|| async {
                Json(json!({
                    "system": {"os": "darwin", "python_version": "3.12.8 (main)", "comfyui_version": "0.3.99"},
                    "devices": [{"name": "mps", "type": "mps", "vram_total": 1024, "vram_free": 512}]
                }))
            }),
        )
        .route(
            "/api/userdata",
            get(|RawQuery(q): RawQuery| async move {
                assert!(q.unwrap_or_default().contains("dir=workflows"));
                Json(json!([
                    {"path": "b/api.json", "size": 10, "modified": 1.5},
                    {"path": "A.json", "size": 5, "modified": 2.0},
                    {"path": "notes.txt", "size": 1, "modified": 2.0}
                ]))
            }),
        )
        .route(
            "/api/userdata/{file}",
            get(|AxPath(file): AxPath<String>| async move {
                match file.as_str() {
                    "workflows/A.json" => Json(ui_workflow()).into_response(),
                    "workflows/b/api.json" => Json(Value::Object(builtin_txt2img("a.safetensors"))).into_response(),
                    _ => StatusCode::NOT_FOUND.into_response(),
                }
            }),
        )
        .route(
            "/templates/index.json",
            get(|| async {
                Json(json!([
                    {"moduleName": "default", "title": "Basics", "type": "image", "templates": [
                        {"name": "default", "title": "Image Generation", "description": "Text to image"},
                        {"name": "api_paid_t2i"}, {"name": "studio.app"}, {"name": "../escape"}
                    ]},
                    {"moduleName": "default", "title": "Video", "type": "video", "templates": [{"name": "video_t2v"}]}
                ]))
            }),
        )
        .route("/workflow_templates", get(|| async { Json(json!({"comfy-pack": ["upscale"]})) }))
        .route("/templates/{file}", get(|| async { Json(ui_workflow()) }))
        .route(
            "/object_info/CheckpointLoaderSimple",
            get(|| async { Json(json!({"CheckpointLoaderSimple": object_info()["CheckpointLoaderSimple"]})) }),
        )
        .route("/object_info", get(|| async { Json(object_info()) }))
        .route(
            "/prompt",
            post(|State(s): State<Shared>, Json(body): Json<Value>| async move {
                let graph = body["prompt"].clone();
                if graph.get("bad").is_some() {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": {"type": "prompt_outputs_failed_validation", "message": "Prompt outputs failed validation", "details": ""},
                            "node_errors": {"4": {"class_type": "CheckpointLoaderSimple", "errors": [{"message": "Value not in list", "details": "ckpt_name: 'x' not in []"}]}}
                        })),
                    )
                        .into_response();
                }
                assert!(body["client_id"].as_str().is_some_and(|c| !c.is_empty()));
                s.lock().unwrap().prompts.push(graph);
                Json(json!({"prompt_id": PROMPT_ID, "number": 1, "node_errors": {}})).into_response()
            }),
        )
        .route(
            "/ws",
            get(|State(s): State<Shared>, ws: WebSocketUpgrade| async move {
                ws.on_upgrade(move |mut socket| async move {
                    // Wait for the prompt, then report two steps and the end.
                    loop {
                        let queued = { !s.lock().unwrap().prompts.is_empty() };
                        if queued {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    if s.lock().unwrap().hang {
                        let _ = socket.send(Message::Text(json!({"type": "execution_start", "data": {"prompt_id": PROMPT_ID}}).to_string().into())).await;
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        return;
                    }
                    for msg in [
                        json!({"type": "status", "data": {"status": {"exec_info": {"queue_remaining": 1}}}}),
                        json!({"type": "execution_start", "data": {"prompt_id": PROMPT_ID}}),
                        json!({"type": "progress", "data": {"value": 1, "max": 2, "prompt_id": "someone-else", "node": "3"}}),
                        json!({"type": "progress", "data": {"value": 1, "max": 2, "prompt_id": PROMPT_ID, "node": "3"}}),
                        json!({"type": "progress", "data": {"value": 2, "max": 2, "prompt_id": PROMPT_ID, "node": "3"}}),
                    ] {
                        let _ = socket.send(Message::Text(msg.to_string().into())).await;
                    }
                    let _ = socket.send(Message::Binary(vec![0, 0, 0, 1].into())).await;
                    s.lock().unwrap().finished = true;
                    let _ = socket.send(Message::Text(json!({"type": "executing", "data": {"node": null, "prompt_id": PROMPT_ID}}).to_string().into())).await;
                })
            }),
        )
        .route(
            "/history/{id}",
            get(|State(s): State<Shared>, AxPath(id): AxPath<String>| async move {
                let m = s.lock().unwrap();
                if id != PROMPT_ID || !m.finished {
                    return Json(json!({}));
                }
                Json(json!({PROMPT_ID: {
                    "status": {"status_str": "success", "completed": true, "messages": []},
                    "outputs": {"9": {"images": [
                        {"filename": "HarnessStudio_00001_.png", "subfolder": "", "type": "output"},
                        {"filename": "HarnessStudio_00002_.png", "subfolder": "", "type": "output"}
                    ]}}
                }}))
            }),
        )
        .route(
            "/queue",
            get(|State(s): State<Shared>| async move {
                let m = s.lock().unwrap();
                // `delete` only drops pending prompts; the executing one stops on `interrupt`.
                let running = !m.prompts.is_empty() && !m.finished && !m.interrupted;
                let running = if running { json!([[1, PROMPT_ID, {}, {}, ["9"]]]) } else { json!([]) };
                Json(json!({"queue_running": running, "queue_pending": []}))
            })
            .post(|State(s): State<Shared>, Json(body): Json<Value>| async move {
                let mut m = s.lock().unwrap();
                for id in body["delete"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    m.deleted.push(id.to_string());
                }
                StatusCode::OK
            }),
        )
        .route(
            "/interrupt",
            post(|State(s): State<Shared>, Json(body): Json<Value>| async move {
                assert_eq!(body["prompt_id"], PROMPT_ID);
                s.lock().unwrap().interrupted = true;
                StatusCode::OK
            }),
        )
        .route(
            "/view",
            get(|RawQuery(q): RawQuery| async move {
                let q = q.unwrap_or_default();
                assert!(q.contains("type=output") && q.contains("filename=HarnessStudio_"), "{q}");
                ([("content-type", "image/png")], PNG.to_vec())
            }),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, state)
}

#[tokio::test]
async fn detects_lists_and_runs_with_progress() {
    let (url, state) = start(false).await;
    let client = ComfyClient::new(&format!("{url}/")).unwrap();
    let status = client.status().await.unwrap();
    assert_eq!(status.url, url);
    assert_eq!(
        (status.version.as_deref(), status.python_version.as_deref()),
        (Some("0.3.99"), Some("3.12.8"))
    );
    assert_eq!(status.devices[0].kind, "mps");

    let catalog = client.catalog().await.unwrap();
    let paths: Vec<&str> = catalog.workflows.iter().map(|w| w.path.as_str()).collect();
    assert_eq!(paths, ["A.json", "b/api.json"]);
    assert_eq!(catalog.checkpoints, ["a.safetensors", "b.safetensors"]);
    let templates: Vec<(&str, &str, &str)> = catalog
        .templates
        .iter()
        .map(|t| (t.source.as_str(), t.name.as_str(), t.title.as_str()))
        .collect();
    assert_eq!(
        templates,
        [
            ("core", "default", "Image Generation"),
            ("comfy-pack", "upscale", "upscale")
        ]
    );

    // A saved editor workflow is converted; the parameters go in.
    let source = WorkflowSource::Saved {
        path: "A.json".into(),
    };
    let graph = client.prepare(&source, &params(), 99).await.unwrap();
    assert_eq!(graph["6"]["inputs"]["text"], "a red fox");
    assert_eq!(graph["4"]["inputs"]["ckpt_name"], "b.safetensors");
    // The built-in graph takes the first checkpoint when none is picked.
    let builtin = client
        .prepare(&WorkflowSource::Builtin, &params(), 5)
        .await
        .unwrap();
    assert_eq!(builtin["4"]["inputs"]["ckpt_name"], "a.safetensors");
    let api = client
        .prepare(
            &WorkflowSource::Saved {
                path: "b/api.json".into(),
            },
            &params(),
            5,
        )
        .await
        .unwrap();
    assert_eq!(api["3"]["inputs"]["seed"], 5);
    let template = WorkflowSource::Template {
        source: "core".into(),
        name: "default".into(),
    };
    assert!(client.prepare(&template, &params(), 5).await.is_ok());
    let escape = WorkflowSource::Saved {
        path: "../secrets.json".into(),
    };
    assert!(client
        .prepare(&escape, &params(), 5)
        .await
        .unwrap_err()
        .contains("comfyui.workflow_path_invalid"));

    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    let outcome = client
        .run(&graph, &move |e: RunEvent| seen.lock().unwrap().push(e))
        .await
        .unwrap();
    assert_eq!(outcome.prompt_id, PROMPT_ID);
    assert_eq!(outcome.images.len(), 2);
    assert_eq!(outcome.images[0].bytes, PNG);
    let events = events.lock().unwrap();
    let stages: Vec<(&str, u64, u64)> = events
        .iter()
        .map(|e| (e.stage.as_str(), e.value, e.max))
        .collect();
    assert_eq!(
        stages,
        [
            ("queued", 0, 0),
            ("running", 0, 0),
            ("progress", 1, 2),
            ("progress", 2, 2)
        ]
    );
    assert!(events.iter().all(|e| e.prompt_id == PROMPT_ID));
    assert_eq!(
        state.lock().unwrap().prompts[0]["6"]["inputs"]["text"],
        "a red fox"
    );
}

#[tokio::test]
async fn a_cancelled_run_stops_waiting() {
    let (url, state) = start(true).await;
    let client = Arc::new(ComfyClient::new(&url).unwrap());
    let graph = builtin_txt2img("a.safetensors");
    let runner = client.clone();
    let run = tokio::spawn(async move { runner.run(&graph, &|_| {}).await });
    // Wait until the prompt is queued, then cancel it.
    for _ in 0..100 {
        if !state.lock().unwrap().prompts.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    client.cancel(PROMPT_ID).await.unwrap();
    {
        let m = state.lock().unwrap();
        assert_eq!(m.deleted, [PROMPT_ID]);
        assert!(m.interrupted, "the executing run is interrupted");
    }
    let result = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().contains("comfyui.cancelled"));
    assert!(client.cancel("bad id!").await.is_err());
}

#[tokio::test]
async fn rejected_prompts_and_other_servers_are_reported() {
    let (url, _) = start(false).await;
    let client = ComfyClient::new(&url).unwrap();
    let mut graph = builtin_txt2img("x");
    graph.insert("bad".into(), json!({"class_type": "X", "inputs": {}}));
    let e = client.run(&graph, &|_| {}).await.unwrap_err();
    assert!(
        e.contains("comfyui.prompt_rejected")
            && e.contains("Value not in list")
            && e.contains("CheckpointLoaderSimple"),
        "{e}"
    );

    // Something else on the port: not ComfyUI.
    let other = Router::new().route("/", get(|| async { "hello" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, other).await.unwrap() });
    let e = ComfyClient::new(&other_url)
        .unwrap()
        .status()
        .await
        .unwrap_err();
    assert!(e.contains("comfyui.not_comfyui"), "{e}");
}

/// Against a real ComfyUI with at least one checkpoint:
/// `COMFYUI_URL=http://127.0.0.1:8188 cargo test -p harness-core --lib comfyui::tests::live -- --ignored`
#[tokio::test]
#[ignore = "needs a running ComfyUI (COMFYUI_URL)"]
async fn live_txt2img() {
    let url = std::env::var("COMFYUI_URL").expect("COMFYUI_URL");
    let client = ComfyClient::new(&url).unwrap();
    let status = client.status().await.unwrap();
    eprintln!(
        "ComfyUI {:?} on {:?}",
        status.version,
        status.devices.first().map(|d| &d.kind)
    );
    let catalog = client.catalog().await.unwrap();
    eprintln!(
        "{} workflows, {} templates, checkpoints {:?}",
        catalog.workflows.len(),
        catalog.templates.len(),
        catalog.checkpoints
    );
    let small = Txt2ImgParams {
        prompt: "a lighthouse on a cliff, watercolor".into(),
        width: Some(256),
        height: Some(256),
        steps: Some(4),
        ..Default::default()
    };
    let mut sources = vec![WorkflowSource::Builtin];
    // A plain editor-format template exercises the conversion (its SDXL checkpoint is swapped
    // for the first one installed).
    if catalog
        .templates
        .iter()
        .any(|t| t.source == "core" && t.name == "image_sdxl_simple")
    {
        sources.push(WorkflowSource::Template {
            source: "core".into(),
            name: "image_sdxl_simple".into(),
        });
    }
    let small = Txt2ImgParams {
        checkpoint: catalog.checkpoints.first().cloned(),
        ..small
    };
    for source in sources {
        let graph = client
            .prepare(&source, &small, random_seed())
            .await
            .unwrap();
        let steps = Arc::new(Mutex::new(0u64));
        let seen = steps.clone();
        let started = Instant::now();
        let outcome = client
            .run(&graph, &move |e: RunEvent| {
                if e.stage == "progress" {
                    *seen.lock().unwrap() = e.value;
                }
            })
            .await
            .unwrap();
        eprintln!(
            "{source:?}: {} image(s), last step {}, {:?}",
            outcome.images.len(),
            steps.lock().unwrap(),
            started.elapsed()
        );
        assert!(!outcome.images.is_empty());
        assert_eq!(image_format(&outcome.images[0].bytes), Some("png"));
        assert_eq!(*steps.lock().unwrap(), 4, "progress came over the socket");
    }
}
