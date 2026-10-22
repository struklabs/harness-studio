//! Inline video questions for llama.cpp: eight chronological stills, never client URLs.
use std::{path::Path, process::Stdio, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::errors::GatewayError;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const FRAMES: usize = 8;

fn bad(message: &str) -> GatewayError {
    GatewayError::new(400, "invalid_request", message).param("messages")
}

fn decode(part: &Value) -> Result<(&'static str, Vec<u8>), GatewayError> {
    let url = part["video_url"]["url"]
        .as_str()
        .ok_or_else(|| bad("video_url must contain an inline data URL."))?;
    let (head, encoded) = url
        .split_once(',')
        .ok_or_else(|| bad("Send an inline MP4 or WebM video."))?;
    let format = match head {
        "data:video/mp4;base64" => "mov",
        "data:video/webm;base64" => "matroska",
        _ => {
            return Err(bad(
                "Send an inline MP4 or WebM video; URLs and file paths are not accepted.",
            ))
        }
    };
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err(bad("Video must be at most 32 MiB."));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| bad("Video contains invalid base64."))?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(bad("Video must contain between 1 byte and 32 MiB."));
    }
    Ok((format, bytes))
}

async fn run(binary: &Path, args: &[&str]) -> Result<Vec<u8>, GatewayError> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    harness_core::orphans::protect(command.as_std_mut());
    let output = command
        .output()
        .await
        .map_err(|_| bad("Video questions need ffmpeg and ffprobe installed on PATH."))?;
    if !output.status.success() {
        return Err(bad("Video could not be decoded."));
    }
    Ok(output.stdout)
}

async fn sample(format: &str, bytes: &[u8]) -> Result<Vec<Value>, GatewayError> {
    let ffmpeg = harness_core::audio::ffmpeg_path()
        .ok_or_else(|| bad("Video questions need ffmpeg and ffprobe installed on PATH."))?;
    let probe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let dir = tempfile::tempdir().map_err(|_| bad("Could not create a temporary video file."))?;
    let input = dir.path().join("input");
    tokio::fs::write(&input, bytes)
        .await
        .map_err(|_| bad("Could not write the temporary video file."))?;
    let input = input
        .to_str()
        .ok_or_else(|| bad("Invalid temporary video path."))?;
    // Force a container demuxer: playlists, network protocols and external data references
    // cannot turn an inline upload into an arbitrary fetch or local-file read.
    let info = run(
        &probe,
        &[
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
            "-f",
            format,
            "-show_entries",
            "format=duration",
            "-of",
            "json",
            input,
        ],
    )
    .await?;
    let info: Value =
        serde_json::from_slice(&info).map_err(|_| bad("Video has no readable duration."))?;
    let duration = info["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 600.0)
        .ok_or_else(|| bad("Video must have a known duration of at most 10 minutes."))?;
    let mut frames = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        let timestamp = duration * i as f64 / FRAMES as f64;
        let time = format!("{timestamp:.6}");
        let png = run(
            &ffmpeg,
            &[
                "-v",
                "error",
                "-nostdin",
                "-threads",
                "2",
                "-protocol_whitelist",
                "file",
                "-f",
                format,
                "-ss",
                &time,
                "-i",
                input,
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-vf",
                "scale=512:512:force_original_aspect_ratio=decrease",
                "-threads",
                "1",
                "-f",
                "image2pipe",
                "-c:v",
                "png",
                "pipe:1",
            ],
        )
        .await?;
        if png.is_empty() {
            continue;
        }
        frames.push(json!({"type":"text", "text":format!("Video frame at {timestamp:.2}s:")}));
        frames.push(json!({"type":"image_url", "image_url":{"url":format!("data:image/png;base64,{}", STANDARD.encode(png))}}));
    }
    if frames.is_empty() {
        return Err(bad("Video contains no decodable frames."));
    }
    Ok(frames)
}

/// Called only for local llama.cpp chat; Python, cloud and remote requests keep their native bodies.
pub async fn prepare(body: Bytes) -> Result<Bytes, GatewayError> {
    let mut value: Value = serde_json::from_slice(&body).map_err(|_| bad("Invalid JSON body."))?;
    let count = value["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|p| p["type"] == "video_url")
        .count();
    if count == 0 {
        return Ok(body);
    }
    if count > 2 {
        return Err(bad("At most two videos are accepted per request."));
    }
    let Some(messages) = value["messages"].as_array_mut() else {
        return Ok(body);
    };
    for message in messages {
        let Some(parts) = message["content"].as_array_mut() else {
            continue;
        };
        let mut converted = Vec::new();
        for part in parts.iter() {
            if part["type"] != "video_url" {
                converted.push(part.clone());
                continue;
            }
            let (format, bytes) = decode(part)?;
            let frames = tokio::time::timeout(Duration::from_secs(60), sample(format, &bytes))
                .await
                .map_err(|_| bad("Video frame sampling timed out."))??;
            converted.extend(frames);
        }
        *parts = converted;
    }
    Ok(Bytes::from(serde_json::to_vec(&value).expect("JSON value")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn text_and_image_requests_are_unchanged() {
        let body = Bytes::from_static(br#"{"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,abc"}},{"type":"text","text":"hi"}]}],"stream":true}"#);
        assert_eq!(prepare(body.clone()).await.unwrap(), body);
    }

    #[tokio::test]
    async fn validates_request_budget_before_decoding_any_clip() {
        let part = json!({"type":"video_url", "video_url":{"url":"invalid"}});
        let body = json!({"messages":[{"content":[part.clone(),part.clone(),part]}]});
        let error = prepare(Bytes::from(body.to_string())).await.unwrap_err();
        assert_eq!(error.status, 400);
        assert!(error.message.contains("At most two"));
    }

    #[test]
    fn refuses_paths_urls_bad_base64_and_oversized_inputs() {
        for url in [
            "https://example.com/v.mp4",
            "file:///secret",
            "C:/secret",
            "data:video/mp4;base64,?",
            "data:video/mp4;base64,",
        ] {
            assert!(decode(&json!({"video_url":{"url":url}})).is_err());
        }
        let url = format!(
            "data:video/mp4;base64,{}",
            "A".repeat(MAX_BYTES.div_ceil(3) * 4 + 1)
        );
        assert!(decode(&json!({"video_url":{"url":url}})).is_err());
    }

    #[tokio::test]
    async fn real_clip_becomes_chronological_images() {
        let Some(ffmpeg) = harness_core::audio::ffmpeg_path() else {
            return;
        };
        for (format, codec) in [("webm", "libvpx-vp9"), ("mp4", "libx264")] {
            let dir = tempfile::tempdir().unwrap();
            let clip = dir.path().join(format!("clip.{format}"));
            run(
                &ffmpeg,
                &[
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=64x48:rate=8:duration=2",
                    "-threads",
                    "1",
                    "-c:v",
                    codec,
                    clip.to_str().unwrap(),
                ],
            )
            .await
            .unwrap();
            let bytes = std::fs::read(clip).unwrap();
            let body = json!({"model":"qwen", "stream":false, "messages":[{"role":"user", "content":[{"type":"text","text":"Describe the clip"},{"type":"video_url","video_url":{"url":format!("data:video/{format};base64,{}",STANDARD.encode(bytes))}}]}]});
            let out: Value =
                serde_json::from_slice(&prepare(Bytes::from(body.to_string())).await.unwrap())
                    .unwrap();
            let parts = out["messages"][0]["content"].as_array().unwrap();
            assert_eq!(parts.len(), 1 + FRAMES * 2);
            assert_eq!(parts[1]["text"], "Video frame at 0.00s:");
            assert_eq!(parts[15]["text"], "Video frame at 1.75s:");
            for part in parts.iter().filter(|p| p["type"] == "image_url") {
                let raw = STANDARD
                    .decode(
                        part["image_url"]["url"]
                            .as_str()
                            .unwrap()
                            .split_once(',')
                            .unwrap()
                            .1,
                    )
                    .unwrap();
                let image = image::load_from_memory(&raw).unwrap();
                assert!(image.width() <= 512 && image.height() <= 512);
            }
            assert_ne!(parts[2]["image_url"]["url"], parts[16]["image_url"]["url"]);
            assert_eq!(out["model"], "qwen");
            assert_eq!(out["stream"], false);
        }
    }
}
