//! `harness image`: one image from an image model through the managed endpoint (the
//! gateway's `POST /v1/images/generations`; the model loads on demand and is unloaded after its
//! idle time like any other).

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

use crate::chat::managed_endpoint;
use crate::cli::ImageArgs;
use crate::error::{CliError, CliResult, Exit};
use crate::output::Out;
use crate::session::Session;

/// The request body for the arguments (the gateway validates the values).
pub fn request_body(args: &ImageArgs, prompt: &str) -> Value {
    let mut body = json!({
        "model": args.model,
        "prompt": prompt,
        "n": args.count,
        "response_format": "b64_json",
    });
    let mut set = |key: &str, value: Option<Value>| {
        if let Some(v) = value {
            body[key] = v;
        }
    };
    set("size", args.size.as_ref().map(|s| json!(s)));
    set("negative_prompt", args.negative.as_ref().map(|s| json!(s)));
    set("steps", args.steps.map(|v| json!(v)));
    set("cfg_scale", args.cfg_scale.map(|v| json!(v)));
    set("seed", args.seed.map(|v| json!(v)));
    set("sampler", args.sampler.as_ref().map(|s| json!(s)));
    set("output_format", args.format.as_ref().map(|s| json!(s)));
    body
}

/// Where image `index` of `count` goes: `out.png`, or `out-1.png`, `out-2.png`... for several.
pub fn output_path(base: &Path, index: usize, count: usize) -> PathBuf {
    if count <= 1 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".into());
    let name = match base.extension() {
        Some(ext) => format!("{stem}-{}.{}", index + 1, ext.to_string_lossy()),
        None => format!("{stem}-{}", index + 1),
    };
    base.with_file_name(name)
}

pub async fn run(session: &Session, args: &ImageArgs, out: &Out) -> CliResult<()> {
    let mut prompt = args.prompt.join(" ");
    if prompt.trim().is_empty() && !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut prompt)
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    }
    if prompt.trim().is_empty() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "Give the prompt: harness image \"a red fox\" --model <image model> -o fox.png",
        ));
    }
    let gateway = managed_endpoint(session).await.ok_or_else(|| {
        CliError::general(
            "cli.not_available",
            "Image generation goes through the gateway, which is not running: start Harness with the gateway on (`harness serve --gateway`).",
        )
    })?;
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .unwrap_or_default();
    out.info(format!(
        "Generating with {} (the model loads first when it is not running)...",
        args.model
    ));
    let mut req = http
        .post(format!("{}/images/generations", gateway.base_url))
        .json(&request_body(args, prompt.trim()))
        .timeout(Duration::from_secs(args.timeout));
    if let Some(token) = &gateway.token {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.map_err(|e| {
        CliError::general(
            "cli.chat_failed",
            format!("The gateway did not answer: {e}"),
        )
    })?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status != 200 {
        let msg = body["error"]["message"]
            .as_str()
            .unwrap_or("the request failed");
        let exit = if crate::messages::mentions_out_of_memory(msg) {
            Exit::OutOfMemory
        } else {
            match crate::error::exit_for_status(status) {
                Exit::Ok | Exit::Usage => Exit::General,
                other => other,
            }
        };
        return Err(CliError::new(
            exit,
            "cli.chat_failed",
            format!("Image generation failed (HTTP {status}): {msg}"),
        )
        .with("http_status", status));
    }
    let images = body["data"].as_array().cloned().unwrap_or_default();
    let ext = body["output_format"].as_str().unwrap_or("png");
    let base = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("image.{ext}")));
    let mut written = Vec::new();
    for (i, item) in images.iter().enumerate() {
        let b64 = item["b64_json"].as_str().ok_or_else(|| {
            CliError::general("cli.chat_failed", "The answer carries no image data")
        })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| {
                CliError::general("cli.io", format!("The image is not valid base64: {e}"))
            })?;
        let path = output_path(&base, i, images.len());
        std::fs::write(&path, bytes)
            .map_err(|e| CliError::general("cli.io", format!("{}: {e}", path.display())))?;
        written.push(path);
    }
    let value = json!({
        "model": args.model,
        "files": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    });
    out.data(&value, || {
        written
            .iter()
            .map(|p| format!("Saved {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args(list: &[&str]) -> ImageArgs {
        match crate::cli::Cli::parse_from(std::iter::once("harness").chain(list.iter().copied()))
            .command
        {
            crate::cli::Command::Image(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_arguments_become_an_openai_request() {
        let a = args(&[
            "image",
            "a red",
            "fox",
            "--model",
            "sd-turbo",
            "-o",
            "fox.png",
            "--size",
            "512x512",
            "--steps",
            "4",
            "--cfg-scale",
            "1",
            "--seed",
            "7",
            "--negative",
            "blurry",
            "--sampler",
            "euler_a",
            "-n",
            "2",
        ]);
        assert_eq!(a.prompt.join(" "), "a red fox");
        assert_eq!(
            request_body(&a, "a red fox"),
            json!({
                "model": "sd-turbo", "prompt": "a red fox", "n": 2, "response_format": "b64_json",
                "size": "512x512", "negative_prompt": "blurry", "steps": 4, "cfg_scale": 1.0,
                "seed": 7, "sampler": "euler_a"
            })
        );
        // Nothing but the model and the prompt: the model's own settings apply.
        let bare = args(&["image", "x", "--model", "m"]);
        assert_eq!(
            request_body(&bare, "x"),
            json!({"model": "m", "prompt": "x", "n": 1, "response_format": "b64_json"})
        );
    }

    #[test]
    fn several_images_are_numbered() {
        let base = Path::new("out/fox.png");
        assert_eq!(output_path(base, 0, 1), PathBuf::from("out/fox.png"));
        assert_eq!(output_path(base, 0, 2), PathBuf::from("out/fox-1.png"));
        assert_eq!(output_path(base, 1, 2), PathBuf::from("out/fox-2.png"));
    }
}
