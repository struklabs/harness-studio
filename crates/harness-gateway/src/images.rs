//! Image generation: `POST /v1/images/generations` and `POST /v1/images/edits` (OpenAI's
//! images API), served by `sd-server` of stable-diffusion.cpp.
//!
//! The request names an image model (`model`, a definition whose file is a diffusion model);
//! the scheduler loads it on demand and unloads it after its TTL like any other model and its
//! memory counts in the same VRAM budget. `sd-server` has an OpenAI-looking route of its own,
//! but it takes the sampler settings in a side channel (`<sd_cpp_extra_args>` inside the
//! prompt); the native job API (`/sdcpp/v1/img_gen`, polled at `/sdcpp/v1/jobs/{id}`) takes
//! them as plain fields and can be cancelled, so this module
//!
//! * reads OpenAI's fields (`prompt`, `n`, `size`, `response_format`, `output_format`) and
//!   Harness's extras (`negative_prompt`, `steps`, `cfg_scale`, `seed` (absent = random, not
//!   sd-server's fixed 42), `sampler`, `scheduler`),
//! * starts a job, polls it until it is done and cancels it when the client goes away, and
//! * answers `{"created": ..., "data": [{"b64_json": ...}]}` (`response_format: "url"` gives a
//!   `data:` URL: Harness keeps no files to link to).
//!
//! A request without `size` leaves the size out, so the model definition's `width` / `height`
//! (the size the model was made for) apply.
//!
//! An edit (`/v1/images/edits`, OpenAI's multipart form or the same fields as JSON with the
//! images as base64 / `data:` URLs) is mapped the way sd-server's own edit route maps it: the
//! first image is the init image (image-to-image, `strength` says how much of it changes), every
//! image is also a reference image (what instruction-editing models such as FLUX.1 Kontext and
//! Qwen-Image-Edit read), `mask` is OpenAI's (transparent = edit, turned into sd.cpp's white = repaint) or already a
//! grayscale one, and without
//! `size` the first image's size is the canvas.

use std::time::{Duration, Instant};

use axum::extract::{FromRequest, Multipart};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use bytes::Bytes;
use harness_core::domain::ModelDefinition;
use harness_core::gateway::record::{ClientInfo, RequestRecord};
use harness_core::token_policy::GatewayEndpoint;
use serde_json::{json, Value};

use crate::errors::GatewayError;
use crate::proxy::{header_str, request_id, trace_context, unix_ms, RecordGuard};
use crate::GwState;

/// Most images one request may ask for (`n`).
pub const MAX_IMAGES: u64 = 8;
/// Most images one edit may send (OpenAI's limit).
pub const MAX_EDIT_IMAGES: usize = 16;
/// Smallest and largest side `size` may have.
const SIDE_RANGE: std::ops::RangeInclusive<u32> = 64..=4096;

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Which images route a request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `/v1/images/generations`: text to image.
    Generations,
    /// `/v1/images/edits`: image (and text) to image.
    Edits,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Kind::Generations => "/v1/images/generations",
            Kind::Edits => "/v1/images/edits",
        }
    }
}

/// Whether a definition serves image generation (its file is a diffusion model).
pub fn is_image_model(def: &ModelDefinition) -> bool {
    use harness_core::domain::ModelRef;
    match &def.base {
        ModelRef::Local { path } => harness_core::engine::is_image_path(path),
        ModelRef::Hf { file, .. } => {
            let f = file.to_ascii_lowercase();
            f.ends_with(".safetensors") || f.ends_with(".ckpt")
        }
    }
}

/// A request, validated and mapped onto `sd-server`'s native `img_gen` body.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The `POST /sdcpp/v1/img_gen` body.
    pub body: Value,
    /// `png`, `jpeg` or `webp`.
    pub format: String,
    /// `response_format`: the image inline (`b64_json`) or as a `data:` URL (`url`).
    pub as_url: bool,
}

/// `"1024x768"` -> `(1024, 768)`.
pub fn parse_size(size: &str) -> Result<(u32, u32), GatewayError> {
    let err = || {
        bad(format!(
            "size `{size}` is not valid; use WIDTHxHEIGHT with both sides between {} and {} and a multiple of 8, like 512x512.",
            SIDE_RANGE.start(),
            SIDE_RANGE.end()
        ))
        .param("size")
    };
    let (w, h) = size.split_once(['x', 'X']).ok_or_else(err)?;
    let w: u32 = w.trim().parse().map_err(|_| err())?;
    let h: u32 = h.trim().parse().map_err(|_| err())?;
    if !SIDE_RANGE.contains(&w)
        || !SIDE_RANGE.contains(&h)
        || !w.is_multiple_of(8)
        || !h.is_multiple_of(8)
    {
        return Err(err());
    }
    Ok((w, h))
}

pub(crate) fn number(v: &Value, key: &str) -> Result<Option<f64>, GatewayError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => x
            .as_f64()
            .map(Some)
            .ok_or_else(|| bad(format!("{key} must be a number.")).param(key)),
    }
}

pub(crate) fn text<'a>(v: &'a Value, key: &str) -> Result<Option<&'a str>, GatewayError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim()).filter(|s| !s.is_empty())),
        Some(_) => Err(bad(format!("{key} must be a string.")).param(key)),
    }
}

/// Validates a request body and maps it onto the native `img_gen` request.
pub fn plan(request: &Value) -> Result<Plan, GatewayError> {
    let prompt = text(request, "prompt")?
        .ok_or_else(|| bad("The `prompt` field (what to draw) is required.").param("prompt"))?;
    let n = match request.get("n") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=MAX_IMAGES).contains(n))
            .ok_or_else(|| {
                bad(format!("n must be a whole number from 1 to {MAX_IMAGES}.")).param("n")
            })?,
    };
    let as_url = match text(request, "response_format")? {
        None | Some("b64_json") => false,
        Some("url") => true,
        Some(other) => {
            return Err(bad(format!(
                "response_format `{other}` is not supported; use b64_json or url (a data: URL)."
            ))
            .param("response_format"))
        }
    };
    let format = match text(request, "output_format")? {
        None => "png".to_string(),
        Some(f) if ["png", "jpeg", "webp"].contains(&f) => f.to_string(),
        Some("jpg") => "jpeg".to_string(),
        Some(other) => {
            return Err(bad(format!(
                "output_format `{other}` is not supported; use png, jpeg or webp."
            ))
            .param("output_format"))
        }
    };
    let mut body = json!({
        "prompt": prompt,
        "batch_count": n,
        "output_format": format,
    });
    let obj = body.as_object_mut().expect("a JSON object");
    if let Some(size) = text(request, "size")? {
        // OpenAI's "auto": the model's own size.
        if size != "auto" {
            let (w, h) = parse_size(size)?;
            obj.insert("width".into(), json!(w));
            obj.insert("height".into(), json!(h));
        }
    }
    if let Some(neg) = text(request, "negative_prompt")? {
        obj.insert("negative_prompt".into(), json!(neg));
    }
    // Without a seed sd-server uses a fixed 42, so the same prompt and settings always gave the
    // same image. OpenAI's API has no seed: absent means random (-1, which the Python worker
    // reads the same way).
    let seed = match request.get("seed").filter(|s| !s.is_null()) {
        Some(seed) => seed
            .as_i64()
            .ok_or_else(|| bad("seed must be a whole number (-1 = random).").param("seed"))?,
        None => -1,
    };
    obj.insert("seed".into(), json!(seed));
    let mut sample = serde_json::Map::new();
    if let Some(steps) = number(request, "steps")? {
        if !(1.0..=500.0).contains(&steps) || steps.fract() != 0.0 {
            return Err(bad("steps must be a whole number from 1 to 500.").param("steps"));
        }
        sample.insert("sample_steps".into(), json!(steps as u32));
    }
    if let Some(cfg) = number(request, "cfg_scale")? {
        if !(0.0..=100.0).contains(&cfg) {
            return Err(bad("cfg_scale must be between 0 and 100.").param("cfg_scale"));
        }
        sample.insert("guidance".into(), json!({ "txt_cfg": cfg }));
    }
    if let Some(guidance) = number(request, "guidance")? {
        // Distilled guidance (FLUX dev), next to the classifier-free one.
        let g = sample.entry("guidance").or_insert_with(|| json!({}));
        g["distilled_guidance"] = json!(guidance);
    }
    if let Some(s) = text(request, "sampler")? {
        sample.insert("sample_method".into(), json!(s));
    }
    if let Some(s) = text(request, "scheduler")? {
        sample.insert("scheduler".into(), json!(s));
    }
    if !sample.is_empty() {
        obj.insert("sample_params".into(), Value::Object(sample));
    }
    Ok(Plan {
        body,
        format,
        as_url,
    })
}

/// Width and height of a PNG, JPEG or WebP image, read from its header.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| Some(u32::from(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])));
    let le16 = |i: usize| Some(u32::from(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])));
    let le24 = |i: usize| Some(le16(i)? | u32::from(*b.get(i + 2)?) << 16);
    let be32 = |i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?));
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        // Walk the segments to the frame header (SOF0..SOF15 without DHT, JPG and DAC).
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + be16(i + 2)? as usize;
        }
        return None;
    }
    if b.get(0..4) == Some(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return match b.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)),
            b"VP8L" => {
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
            }
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            _ => None,
        };
    }
    None
}

/// Bytes of an image's start decoded to read its header (`image_size`); a JPEG's frame header
/// comes after its EXIF block, which is at most 64 KB.
const HEAD_BYTES: usize = 256 * 1024;

/// An image field of an edit (base64 or a `data:` URL) as bare base64, made in place (the
/// image is not copied), and the decoded start of the image. The rest is decoded by sd-server,
/// which refuses a broken image.
fn image_field(mut value: String, field: &str) -> Result<(String, Vec<u8>), GatewayError> {
    let err = || bad(format!("{field} must be a base64 image or a data: URL.")).param(field);
    if value.starts_with("data:") {
        let at = value.find(";base64,").ok_or_else(err)?;
        value.replace_range(..at + ";base64,".len(), "");
    }
    if value.starts_with(char::is_whitespace) || value.ends_with(char::is_whitespace) {
        value = value.trim().to_string();
    }
    // A whole number of base64 quads (3 bytes each), so the prefix decodes on its own.
    let quads = (HEAD_BYTES / 3 * 4).min(value.len()) / 4 * 4;
    let head = if quads == value.len() {
        B64.decode(&value)
    } else {
        B64.decode(&value[..quads])
    }
    .map_err(|_| err())?;
    if head.is_empty() {
        return Err(err());
    }
    Ok((value, head))
}

/// OpenAI's mask marks the area to edit with transparent pixels, sd.cpp's with white ones. A
/// PNG mask with transparent pixels becomes a grayscale PNG (transparent = white, opaque =
/// black); any other mask (no alpha channel, fully opaque, not a PNG) is in sd.cpp's terms
/// already and gives `None`.
pub fn sd_mask(png: &[u8]) -> Option<Vec<u8>> {
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    if !img.color().has_alpha() {
        return None;
    }
    let la = img.to_luma_alpha8();
    if la.pixels().all(|p| p[1] == u8::MAX) {
        return None;
    }
    let gray = image::GrayImage::from_fn(la.width(), la.height(), |x, y| {
        image::Luma([u8::MAX - la.get_pixel(x, y)[1]])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    gray.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Validates an edit request (`plan`'s fields plus `image`, `mask` and `strength`) and maps
/// it onto `img_gen` (see the module notes). Takes the request to move its images into the
/// job body instead of copying them.
pub fn plan_edit(mut request: Value) -> Result<Plan, GatewayError> {
    let mut p = plan(&request)?;
    let strength = number(&request, "strength")?;
    let not_images =
        || bad("image must be an image (base64 or a data: URL) or a list of them.").param("image");
    let images: Vec<String> = match request.get_mut("image").map(Value::take) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => vec![s],
        Some(Value::Array(a)) => a
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                _ => Err(not_images()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(not_images()),
    };
    if images.is_empty() {
        return Err(bad("The `image` field (the picture to edit) is required.").param("image"));
    }
    if images.len() > MAX_EDIT_IMAGES {
        return Err(bad(format!("An edit takes at most {MAX_EDIT_IMAGES} images.")).param("image"));
    }
    let mut encoded = Vec::with_capacity(images.len());
    let mut first_size = None;
    for (i, image) in images.into_iter().enumerate() {
        let (b64, head) = image_field(image, "image")?;
        let size = image_size(&head);
        if let Some((w, h)) = size.filter(|(w, h)| w.max(h) > SIDE_RANGE.end()) {
            return Err(bad(format!(
                "Image {} is {w}x{h}; the largest side an edit takes is {} pixels. Make it smaller first.",
                i + 1,
                SIDE_RANGE.end()
            ))
            .param("image"));
        }
        if i == 0 {
            first_size = size;
        }
        encoded.push(Value::String(b64));
    }
    let mask = match request.get_mut("mask").map(Value::take) {
        None | Some(Value::Null) => None,
        Some(Value::String(m)) if m.trim().is_empty() => None,
        Some(Value::String(m)) => {
            let (b64, head) = image_field(m, "mask")?;
            // A mask is converted only when it is a PNG: decode the whole of it.
            let converted = if head.starts_with(b"\x89PNG") {
                let bytes = B64.decode(&b64).map_err(|_| {
                    bad("mask must be a base64 image or a data: URL.").param("mask")
                })?;
                sd_mask(&bytes).map(|png| B64.encode(png))
            } else {
                None
            };
            Some(converted.unwrap_or(b64))
        }
        Some(_) => return Err(bad("mask must be a string.").param("mask")),
    };
    let obj = p.body.as_object_mut().expect("a JSON object");
    if !obj.contains_key("width") {
        // The first image's size, made a multiple of 8.
        let fits = first_size
            .map(|(w, h)| (w - w % 8, h - h % 8))
            .filter(|(w, h)| SIDE_RANGE.contains(w) && SIDE_RANGE.contains(h));
        let (w, h) = fits.ok_or_else(|| {
            bad(format!(
                "The size of the image could not be used (a PNG, JPEG or WebP with sides between {} and {} pixels); set `size`, like 1024x1024.",
                SIDE_RANGE.start(),
                SIDE_RANGE.end()
            ))
            .param("size")
        })?;
        obj.insert("width".into(), json!(w));
        obj.insert("height".into(), json!(h));
    }
    if let Some(strength) = strength {
        if !(0.0..=1.0).contains(&strength) {
            return Err(bad("strength must be between 0 and 1.").param("strength"));
        }
        obj.insert("strength".into(), json!(strength));
    }
    if let Some(mask) = mask {
        obj.insert("mask_image".into(), Value::String(mask));
    }
    // sd-server's own edit route does the same: the first image is the init image and every
    // image a reference (one more copy of the first image in the body).
    obj.insert("init_image".into(), encoded[0].clone());
    obj.insert("ref_images".into(), Value::Array(encoded));
    Ok(p)
}

/// Fields of an edit form that are numbers (a form sends everything as text).
const NUMBER_FIELDS: [&str; 6] = ["n", "steps", "cfg_scale", "guidance", "seed", "strength"];

/// Reads OpenAI's multipart edit form (`image` / `image[]` files, `mask`, text fields) into
/// the JSON shape `plan_edit` reads. A file part is encoded as base64; an `image` or `mask`
/// sent as a text part is base64 (or a `data:` URL) already and stays as it is.
async fn read_edit_form(mut mp: Multipart) -> Result<Value, GatewayError> {
    let mut out = serde_json::Map::new();
    let mut images = Vec::new();
    while let Some(field) = mp.next_field().await.map_err(|e| {
        bad(format!(
            "The multipart body could not be read: {}",
            e.body_text()
        ))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        let is_file = field.file_name().is_some()
            || field
                .content_type()
                .is_some_and(|ct| !ct.starts_with("text/"));
        let bytes = field
            .bytes()
            .await
            .map_err(|e| bad(format!("Field {name}: {}", e.body_text())))?;
        let image = if is_file {
            Value::String(B64.encode(&bytes))
        } else {
            Value::String(String::from_utf8_lossy(&bytes).trim().to_string())
        };
        match name.as_str() {
            "image" | "image[]" => images.push(image),
            "mask" => {
                out.insert(name, image);
            }
            _ => {
                let text = String::from_utf8_lossy(&bytes).trim().to_string();
                let value = match serde_json::from_str::<Value>(&text) {
                    Ok(n @ Value::Number(_)) if NUMBER_FIELDS.contains(&name.as_str()) => n,
                    _ => Value::String(text),
                };
                out.insert(name, value);
            }
        }
    }
    if !images.is_empty() {
        out.insert("image".into(), Value::Array(images));
    }
    Ok(Value::Object(out))
}

/// The request body as JSON: an edit may come as a multipart form.
async fn read_request(kind: Kind, headers: &HeaderMap, body: Bytes) -> Result<Value, GatewayError> {
    let multipart = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("multipart/form-data"));
    if kind == Kind::Edits && multipart {
        let mut req = axum::http::Request::new(axum::body::Body::from(body));
        if let Some(ct) = headers.get(header::CONTENT_TYPE) {
            req.headers_mut().insert(header::CONTENT_TYPE, ct.clone());
        }
        let mp = Multipart::from_request(req, &())
            .await
            .map_err(|e| bad(format!("The multipart body could not be read: {e}")))?;
        return read_edit_form(mp).await;
    }
    serde_json::from_slice(&body)
        .map_err(|e| bad(format!("The request body is not valid JSON: {e}")))
}

/// The state of a polled job.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// `queued` / `generating`: ask again.
    Running,
    /// The encoded images (base64), in order.
    Done(Vec<String>),
    /// `failed` / `cancelled`, with sd-server's message.
    Failed(String),
}

/// Reads a `GET /sdcpp/v1/jobs/{id}` answer.
pub fn parse_job(v: &Value) -> Job {
    match v.get("status").and_then(Value::as_str).unwrap_or("") {
        "completed" => {
            let images: Vec<String> = v
                .pointer("/result/images")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|i| i.get("b64_json").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if images.is_empty() {
                Job::Failed("sd-server finished without an image.".into())
            } else {
                Job::Done(images)
            }
        }
        "failed" | "cancelled" => Job::Failed(
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("the generation failed")
                .to_string(),
        ),
        _ => Job::Running,
    }
}

/// Device memory (MB) a failed job needed and had, from sd-server's log lines of that job:
/// `model manager cannot make enough memory available on CUDA0: need 5788.60 MB device / ...,
/// available 3267.70 MB device / ...`. sd-server itself only answers "generate_image returned
/// no results" (measured: SD-Turbo at 1024x1024 on a 4 GB RTX 3050 Ti).
pub fn memory_shortfall(lines: &[String]) -> Option<(u64, u64)> {
    let mb_before = |s: &str, unit: &str| -> Option<u64> {
        let end = s.find(unit)?;
        let num = s[..end].trim_end().rsplit(' ').next()?;
        num.parse::<f64>().ok().map(|v| v.round() as u64)
    };
    lines.iter().rev().find_map(|l| {
        let rest = &l[l.find("cannot make enough memory available")?..];
        let need = mb_before(&rest[rest.find("need ")?..], " MB device")?;
        let avail_at = rest.find(", available ")?;
        let available = mb_before(&rest[avail_at..], " MB device")?;
        Some((need, available))
    })
}

/// The worker body of an edit on a Python engine model, from the checked `img_gen` body
/// `plan_edit` made: the first image, the mask already turned into white = repaint, the canvas
/// size and the sampler settings under the OpenAI route's names.
fn python_edit_body(img_gen: &Value) -> Value {
    let mut out = serde_json::Map::new();
    let mut copy = |to: &str, v: Option<&Value>| {
        if let Some(v) = v.filter(|v| !v.is_null()) {
            out.insert(to.into(), v.clone());
        }
    };
    copy("prompt", img_gen.get("prompt"));
    copy("n", img_gen.get("batch_count"));
    copy("negative_prompt", img_gen.get("negative_prompt"));
    copy("seed", img_gen.get("seed"));
    copy("strength", img_gen.get("strength"));
    copy("image", img_gen.get("init_image"));
    copy("mask", img_gen.get("mask_image"));
    let sample = img_gen.get("sample_params");
    copy("steps", sample.and_then(|s| s.get("sample_steps")));
    copy(
        "cfg_scale",
        sample.and_then(|s| s.pointer("/guidance/txt_cfg")),
    );
    copy("sampler", sample.and_then(|s| s.get("sample_method")));
    copy("scheduler", sample.and_then(|s| s.get("scheduler")));
    if let (Some(w), Some(h)) = (
        img_gen.get("width").and_then(Value::as_u64),
        img_gen.get("height").and_then(Value::as_u64),
    ) {
        out.insert("size".into(), json!(format!("{w}x{h}")));
    }
    Value::Object(out)
}

/// An images request on a Python engine model (a diffusers pipeline): a generation's OpenAI body
/// goes to the worker as it is, an edit as `python_edit_body`; its PNGs come back as `b64_json`
/// or `data:` URLs like sd-server's.
async fn python_generate(
    state: &GwState,
    def: &ModelDefinition,
    kind: Kind,
    request: Value,
    plan: &Plan,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    if plan.format != "png" {
        return Err(bad(format!(
            "output_format `{}` is not available for this model; it makes PNG images.",
            plan.format
        ))
        .param("output_format"));
    }
    let body = serde_json::to_vec(&request)
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))?;
    let answer =
        crate::pyhf::post(state, def, guard, kind.path(), "application/json", body).await?;
    let v: Value = serde_json::from_slice(&answer.body)
        .map_err(|e| GatewayError::new(502, "upstream_error", format!("Python engine: {e}")))?;
    let images: Vec<String> = v
        .get("data")
        .and_then(Value::as_array)
        .map(|d| {
            d.iter()
                .filter_map(|i| i.get("b64_json").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if images.is_empty() {
        return Err(GatewayError::new(
            502,
            "upstream_error",
            "Python engine: the answer has no image.",
        ));
    }
    let created = v.get("created").and_then(Value::as_u64).unwrap_or(0);
    let out = shape(&images, "png", plan.as_url, created);
    guard.finish(200, None);
    let builder = crate::pyhf::lease_headers(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json"),
        def,
        &answer.lease,
    );
    builder
        .body(axum::body::Body::from(out.to_string()))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

/// The OpenAI answer for the finished images.
pub fn shape(images: &[String], format: &str, as_url: bool, created: u64) -> Value {
    let mime = if format == "jpeg" {
        "image/jpeg".to_string()
    } else {
        format!("image/{format}")
    };
    let data: Vec<Value> = images
        .iter()
        .map(|b64| {
            if as_url {
                json!({ "url": format!("data:{mime};base64,{b64}") })
            } else {
                json!({ "b64_json": b64 })
            }
        })
        .collect();
    json!({ "created": created, "data": data, "output_format": format })
}

/// Cancels the sd-server job when the request is dropped before it finished (the client went
/// away): a generation takes minutes on a CPU and nobody wants the result.
struct CancelOnDrop {
    http: reqwest::Client,
    url: String,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            let (http, url) = (self.http.clone(), self.url.clone());
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    let _ = http.post(url).timeout(Duration::from_secs(5)).send().await;
                });
            }
        }
    }
}

/// Handles one image generation or edit request.
pub async fn handle(state: GwState, kind: Kind, headers: HeaderMap, body: Bytes) -> Response {
    let mut guard = RecordGuard::new(
        RequestRecord {
            id: request_id(),
            started_at_ms: unix_ms(),
            endpoint: kind.path().to_string(),
            ..Default::default()
        },
        trace_context(&headers),
    );
    let id = guard.get().id.clone();
    let result = run(&state, kind, &headers, body, &mut guard).await;
    let mut resp = match result {
        Ok(resp) => resp,
        Err(err) => {
            guard.finish(err.status, Some(&err.code));
            err.into_response()
        }
    };
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-harness-request-id", v);
    }
    if let Some(rate) = guard.rate {
        crate::auth::rate_headers(resp.headers_mut(), &rate, false);
    }
    resp
}

async fn run(
    state: &GwState,
    kind: Kind,
    headers: &HeaderMap,
    body: Bytes,
    guard: &mut RecordGuard,
) -> Result<Response, GatewayError> {
    let caller = crate::auth::check(state, headers, false, GatewayEndpoint::Images)?;
    guard.get().client = ClientInfo::new(
        header_str(headers, "user-agent"),
        header_str(headers, "x-harness-client"),
        caller.token_name.clone(),
    );
    let request = read_request(kind, headers, body).await?;
    // The OpenAI body a Python engine model gets as it is (generations only).
    let raw_request = (kind == Kind::Generations).then(|| request.clone());
    let model = text(&request, "model")?.map(str::to_string);
    guard.get().model = model.clone();
    guard.admitted(caller.admit(state)?);
    let plan = match kind {
        Kind::Generations => plan(&request)?,
        Kind::Edits => plan_edit(request)?,
    };

    let scheduler = state.scheduler.clone();
    let settings = scheduler.settings();
    let def = scheduler
        .resolve(model.as_deref())
        .await
        .map_err(|e| caller.sched_error(&e))?;
    {
        let r = guard.get();
        r.alias = Some(def.alias.clone());
        r.definition_id = Some(def.id.clone());
    }
    caller.check_model(&def.alias)?;
    GatewayError::reject_remote(&def)?;
    // A diffusers pipeline of the Python engine answers OpenAI's request itself.
    if let Some(task) = crate::pyhf::task_of(&def) {
        if let Some(e) = python_task_error(&def.alias, &task, kind) {
            return Err(e);
        }
        let request = match kind {
            Kind::Generations => raw_request.unwrap_or_default(),
            Kind::Edits => python_edit_body(&plan.body),
        };
        return python_generate(state, &def, kind, request, &plan, guard).await;
    }
    if !is_image_model(&def) {
        return Err(GatewayError::new(
            400,
            "model_not_image",
            format!(
                "`{}` is not an image model (a diffusion model: safetensors, ckpt or a stable-diffusion.cpp GGUF); {} needs one.",
                def.alias,
                kind.path()
            ),
        )
        .param("model"));
    }

    let lease = match scheduler.acquire_tokens(&def, None).await {
        Ok(lease) => lease,
        Err(e) => {
            guard.get().oom = e.is_oom();
            return Err(GatewayError::from_sched(&e));
        }
    };
    {
        let r = guard.get();
        r.server_id = Some(lease.server_id.clone());
        r.queue_wait_ms = lease.queue_wait.as_millis() as u64;
        r.slots = Some(lease.slots);
        r.waited = lease.waited.map(str::to_string);
        r.backend = lease.backend.clone();
        if let Some(load) = &lease.load {
            r.load_ms = Some(load.ms);
            r.evicted = load.evicted.clone();
            r.oom = load.recovered;
        }
    }

    let upstream_err = |e: reqwest::Error| {
        if e.is_timeout() {
            GatewayError::new(
                504,
                "upstream_timeout",
                "sd-server did not answer in time.".to_string(),
            )
        } else {
            GatewayError::new(502, "upstream_unreachable", format!("sd-server: {e}"))
        }
    };
    // Where this job's log lines start: a failure is explained from those alone.
    let manager = state.scheduler.registry().get(&lease.server_id);
    let log_mark = manager.as_ref().map(|m| m.get_logs_since(u64::MAX).0);
    let started = state
        .http
        .post(format!("{}/sdcpp/v1/img_gen", lease.upstream))
        .json(&plan.body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(upstream_err)?;
    let status = started.status().as_u16();
    let accepted: Value = started.json().await.unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = accepted
            .pointer("/error/message")
            .or_else(|| accepted.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("the request was refused");
        guard.get().status = status;
        return Err(GatewayError::new(
            if status == 400 { 400 } else { 502 },
            if status == 400 {
                "invalid_request"
            } else {
                "upstream_error"
            },
            format!("sd-server: {detail}"),
        ));
    }
    let job_id = accepted
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::new(502, "upstream_error", "sd-server gave no job id."))?
        .to_string();
    let mut cancel = CancelOnDrop {
        http: state.http.clone(),
        url: format!("{}/sdcpp/v1/jobs/{job_id}/cancel", lease.upstream),
        armed: true,
    };

    let limit = match settings.request_timeout_secs {
        0 => None,
        n => Some(Duration::from_secs(u64::from(n))),
    };
    let began = Instant::now();
    let mut wait = Duration::from_millis(150);
    let images = loop {
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(1));
        if limit.is_some_and(|l| began.elapsed() > l) {
            return Err(GatewayError::new(
                504,
                "upstream_timeout",
                "sd-server did not finish the image in time (raise the request timeout in the gateway settings).",
            ));
        }
        let polled = state
            .http
            .get(format!("{}/sdcpp/v1/jobs/{job_id}", lease.upstream))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(upstream_err)?;
        if polled.status().as_u16() == 404 || polled.status().as_u16() == 410 {
            return Err(GatewayError::new(
                502,
                "upstream_error",
                "sd-server lost the job.",
            ));
        }
        let value: Value = polled
            .json()
            .await
            .map_err(|e| GatewayError::new(502, "upstream_error", format!("sd-server: {e}")))?;
        match parse_job(&value) {
            Job::Running => {}
            Job::Done(images) => break images,
            Job::Failed(message) => {
                cancel.armed = false;
                let lines = match (&manager, log_mark) {
                    (Some(m), Some(mark)) => m.get_logs_since(mark).1,
                    _ => Vec::new(),
                };
                return Err(match memory_shortfall(&lines) {
                    Some((need, available)) => GatewayError::new(
                        500,
                        "image_out_of_memory",
                        format!(
                            "The image does not fit in GPU memory at this size: sd-server needs about {need} MB and {available} MB is free. Try a smaller size (SD 1.x and SD-Turbo models are made for 512x512)."
                        ),
                    )
                    .details(json!({ "need_mb": need, "available_mb": available })),
                    None => GatewayError::new(
                        500,
                        "generation_failed",
                        format!("sd-server: {message}"),
                    ),
                });
            }
        }
    };
    cancel.armed = false;
    guard.get().ttft_ms = Some(guard.started.elapsed().as_millis() as u64);
    guard.get().status = 200;

    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = shape(&images, &plan.format, plan.as_url, created);
    guard.finish(200, None);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-harness-model", &def.alias)
        .header("x-harness-server", &lease.server_id)
        .header(
            "x-harness-queue-ms",
            lease.queue_wait.as_millis().to_string(),
        );
    if let Some(load) = &lease.load {
        builder = builder.header("x-harness-load-ms", load.ms.to_string());
        if !load.evicted.is_empty() {
            builder = builder.header("x-harness-evicted", load.evicted.join(","));
        }
    }
    drop(lease);
    builder
        .body(axum::body::Body::from(out.to_string()))
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

/// Why a Python engine model of `task` cannot serve `kind`: only diffusers pipelines draw, and
/// an image-to-image one (InstructPix2Pix, Qwen-Image-Edit) needs a start image.
fn python_task_error(alias: &str, task: &str, kind: Kind) -> Option<GatewayError> {
    let e = match task {
        "text-to-image" => return None,
        "image-to-image" if kind == Kind::Edits => return None,
        "image-to-image" => GatewayError::new(
            400,
            "image_required",
            format!("`{alias}` edits images: send it to /v1/images/edits with an image."),
        ),
        _ => GatewayError::new(
            400,
            "model_not_image",
            format!(
                "`{alias}` is a Python engine model of {task}; {} needs a text-to-image model.",
                kind.path()
            ),
        ),
    };
    Some(e.param("model"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_to_image_python_models_only_edit() {
        assert!(python_task_error("m", "text-to-image", Kind::Generations).is_none());
        assert!(python_task_error("m", "text-to-image", Kind::Edits).is_none());
        assert!(python_task_error("m", "image-to-image", Kind::Edits).is_none());
        let e = python_task_error("m", "image-to-image", Kind::Generations).unwrap();
        assert_eq!((e.status, e.code.as_str()), (400, "image_required"));
        let e = python_task_error("m", "text-classification", Kind::Edits).unwrap();
        assert_eq!(e.code, "model_not_image");
    }

    #[test]
    fn a_job_that_ran_out_of_device_memory_says_how_much_it_needed() {
        let lines = vec![
            "[+9.1s] [stdout] [W] model manager memory on CUDA0: reported free 3267.70 MB / total 4095.50 MB --- model_manager.cpp:1753".to_string(),
            "[+9.1s] [stdout] [W] model manager cannot make enough memory available on CUDA0: need 5788.60 MB device / 5276.60 MB budget, available 3267.70 MB device / 2783.70 MB budget --- model_manager.cpp:1962".to_string(),
            "[+9.1s] [stdout] [E] sampling for image 1/1 failed after 0.03s --- image.cpp:907".to_string(),
        ];
        assert_eq!(memory_shortfall(&lines), Some((5789, 3268)));
        assert_eq!(memory_shortfall(&lines[2..]), None);
        assert_eq!(memory_shortfall(&[]), None);
    }

    #[test]
    fn an_openai_request_maps_onto_the_native_job() {
        let p = plan(&json!({
            "model": "sd-turbo", "prompt": "a red fox", "n": 2, "size": "512x768",
            "response_format": "b64_json", "negative_prompt": "blurry", "steps": 4,
            "cfg_scale": 1.0, "seed": 7, "sampler": "euler_a"
        }))
        .unwrap();
        assert_eq!(
            p.body,
            json!({
                "prompt": "a red fox", "batch_count": 2, "output_format": "png",
                "width": 512, "height": 768, "negative_prompt": "blurry", "seed": 7,
                "sample_params": {
                    "sample_steps": 4, "guidance": {"txt_cfg": 1.0}, "sample_method": "euler_a"
                }
            })
        );
        assert!(!p.as_url && p.format == "png");
    }

    #[test]
    fn a_bare_prompt_leaves_the_model_defaults_alone() {
        let p = plan(&json!({"prompt": "a cat"})).unwrap();
        assert_eq!(
            p.body,
            json!({"prompt": "a cat", "batch_count": 1, "output_format": "png", "seed": -1})
        );
        // OpenAI's "auto" is the same thing.
        assert!(plan(&json!({"prompt": "a cat", "size": "auto"}))
            .unwrap()
            .body
            .get("width")
            .is_none());
        // Distilled guidance sits next to the CFG scale.
        let p = plan(&json!({"prompt": "x", "cfg_scale": 1.0, "guidance": 3.5})).unwrap();
        assert_eq!(
            p.body["sample_params"]["guidance"],
            json!({"txt_cfg": 1.0, "distilled_guidance": 3.5})
        );
    }

    #[test]
    fn bad_requests_name_the_field() {
        let param = |v: Value| {
            plan(&v)
                .unwrap_err()
                .param
                .map(|p| p.to_string())
                .unwrap_or_default()
        };
        assert_eq!(param(json!({})), "prompt");
        assert_eq!(param(json!({"prompt": "  "})), "prompt");
        assert_eq!(param(json!({"prompt": "x", "n": 0})), "n");
        assert_eq!(param(json!({"prompt": "x", "n": 99})), "n");
        assert_eq!(param(json!({"prompt": "x", "size": "512"})), "size");
        assert_eq!(param(json!({"prompt": "x", "size": "500x500"})), "size");
        assert_eq!(param(json!({"prompt": "x", "size": "16x16"})), "size");
        assert_eq!(param(json!({"prompt": "x", "steps": 0})), "steps");
        assert_eq!(param(json!({"prompt": "x", "steps": 2.5})), "steps");
        assert_eq!(param(json!({"prompt": "x", "seed": "a"})), "seed");
        assert_eq!(param(json!({"prompt": "x", "cfg_scale": "a"})), "cfg_scale");
        assert_eq!(
            param(json!({"prompt": "x", "response_format": "html"})),
            "response_format"
        );
        assert_eq!(
            param(json!({"prompt": "x", "output_format": "gif"})),
            "output_format"
        );
        assert_eq!(parse_size("1024x1024").unwrap(), (1024, 1024));
        assert_eq!(parse_size("640X480").unwrap(), (640, 480));
    }

    #[test]
    fn jobs_are_read_by_status() {
        assert_eq!(parse_job(&json!({"status": "queued"})), Job::Running);
        assert_eq!(parse_job(&json!({"status": "generating"})), Job::Running);
        let done = json!({"status": "completed", "result": {"images": [
            {"index": 0, "b64_json": "AAA"}, {"index": 1, "b64_json": "BBB"}]}});
        assert_eq!(
            parse_job(&done),
            Job::Done(vec!["AAA".into(), "BBB".into()])
        );
        let empty = json!({"status": "completed", "result": {"images": []}});
        assert!(matches!(parse_job(&empty), Job::Failed(_)));
        let failed = json!({"status": "failed", "error": {"code": "generation_failed", "message": "out of memory"}});
        assert_eq!(parse_job(&failed), Job::Failed("out of memory".into()));
    }

    /// A PNG header of `w` x `h` (enough for `image_size`).
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    #[test]
    fn image_sizes_come_from_the_header() {
        assert_eq!(image_size(&png(640, 480)), Some((640, 480)));
        // JPEG: an APP0 segment, then SOF0 (height before width).
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
        jpeg.extend([0xFF, 0xC0, 0, 11, 8, 0x01, 0xE0, 0x02, 0x80, 3, 0, 0, 0]);
        assert_eq!(image_size(&jpeg), Some((640, 480)));
        // WebP lossy, lossless and extended.
        let webp = |chunk: &[u8], tail: &[u8]| {
            let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
            b.extend(chunk);
            b.extend(tail);
            b
        };
        let mut vp8 = vec![0u8; 10];
        vp8.extend(640u16.to_le_bytes());
        vp8.extend(480u16.to_le_bytes());
        assert_eq!(image_size(&webp(b"VP8 ", &vp8)), Some((640, 480)));
        let bits: u32 = 639 | (479 << 14);
        let mut vp8l = vec![0u8; 5];
        vp8l.extend(bits.to_le_bytes());
        assert_eq!(image_size(&webp(b"VP8L", &vp8l)), Some((640, 480)));
        let mut vp8x = vec![0u8; 8];
        vp8x.extend(&639u32.to_le_bytes()[..3]);
        vp8x.extend(&479u32.to_le_bytes()[..3]);
        assert_eq!(image_size(&webp(b"VP8X", &vp8x)), Some((640, 480)));
        assert_eq!(image_size(b"GIF89a"), None);
        assert_eq!(image_size(&[0xFF, 0xD8, 0x00]), None);
    }

    #[test]
    fn an_edit_sends_the_image_as_init_and_reference_at_its_own_size() {
        let a = B64.encode(png(641, 480));
        let b = format!("data:image/png;base64,{}", B64.encode(png(64, 64)));
        let p = plan_edit(json!({
            "prompt": "make it night", "image": [a, b], "strength": 0.4,
            "mask": B64.encode(png(641, 480))
        }))
        .unwrap();
        // 641 is not a multiple of 8: the canvas is 640 wide.
        assert_eq!(
            (p.body["width"].clone(), p.body["height"].clone()),
            (json!(640), json!(480))
        );
        assert_eq!(p.body["strength"], json!(0.4));
        assert_eq!(p.body["init_image"], json!(a));
        // A data: URL goes on as plain base64.
        assert_eq!(p.body["ref_images"], json!([a, B64.encode(png(64, 64))]));
        assert_eq!(p.body["mask_image"], json!(B64.encode(png(641, 480))));
        // `size` wins over the image's size; one image may be a bare string.
        let p = plan_edit(json!({"prompt": "x", "image": a, "size": "512x512"})).unwrap();
        assert_eq!(p.body["width"], json!(512));
        assert_eq!(p.body["ref_images"], json!([a]));
        assert!(p.body.get("strength").is_none() && p.body.get("mask_image").is_none());
    }

    #[test]
    fn a_python_edit_gets_the_first_image_mask_size_and_sampler() {
        let a = B64.encode(png(641, 480));
        let b = B64.encode(png(64, 64));
        let p = plan_edit(json!({
            "prompt": "make it night", "image": [a, b], "strength": 0.4, "n": 2,
            "mask": B64.encode(png(641, 480)), "steps": 12, "cfg_scale": 6.5, "seed": 9,
            "sampler": "euler_a", "scheduler": "karras", "negative_prompt": "blur"
        }))
        .unwrap();
        let body = python_edit_body(&p.body);
        assert_eq!(
            body,
            json!({
                "prompt": "make it night", "n": 2, "negative_prompt": "blur", "seed": 9,
                "strength": 0.4, "image": a, "mask": B64.encode(png(641, 480)), "steps": 12,
                "cfg_scale": 6.5, "sampler": "euler_a", "scheduler": "karras", "size": "640x480"
            })
        );
        // Only what was asked for goes on, plus the random seed (the worker reads -1 as random).
        let p = plan_edit(json!({"prompt": "x", "image": b})).unwrap();
        assert_eq!(
            python_edit_body(&p.body),
            json!({"prompt": "x", "n": 1, "seed": -1, "image": b, "size": "64x64"})
        );
    }

    #[test]
    fn bad_edits_name_the_field() {
        let param = |v: Value| {
            plan_edit(v)
                .unwrap_err()
                .param
                .map(|p| p.to_string())
                .unwrap_or_default()
        };
        let ok = B64.encode(png(512, 512));
        assert_eq!(param(json!({"prompt": "x"})), "image");
        assert_eq!(param(json!({"prompt": "x", "image": []})), "image");
        assert_eq!(param(json!({"prompt": "x", "image": 5})), "image");
        assert_eq!(
            param(json!({"prompt": "x", "image": "not base64!"})),
            "image"
        );
        assert_eq!(
            param(json!({"prompt": "x", "image": "data:image/png,abc"})),
            "image"
        );
        assert_eq!(
            param(json!({"prompt": "x", "image": vec![ok.clone(); 17]})),
            "image"
        );
        assert_eq!(
            param(json!({"prompt": "x", "image": ok, "strength": 1.5})),
            "strength"
        );
        assert_eq!(
            param(json!({"prompt": "x", "image": ok, "mask": "%%"})),
            "mask"
        );
        // An image whose size is unknown or too small needs `size`.
        let gif = B64.encode(b"GIF89a....");
        assert_eq!(param(json!({"prompt": "x", "image": gif})), "size");
        assert!(plan_edit(json!({"prompt": "x", "image": gif, "size": "256x256"})).is_ok());
        assert_eq!(
            param(json!({"prompt": "x", "image": B64.encode(png(32, 32))})),
            "size"
        );
        assert_eq!(param(json!({"image": ok})), "prompt");
    }

    /// A real PNG of `w` x 1 pixels with these alpha values (grayscale + alpha).
    fn alpha_png(alphas: &[u8]) -> Vec<u8> {
        let img = image::GrayAlphaImage::from_fn(alphas.len() as u32, 1, |x, _| {
            image::LumaA([0, alphas[x as usize]])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn an_openai_mask_is_turned_into_an_sd_cpp_mask() {
        // Transparent (edit) -> white, opaque (keep) -> black.
        let out = sd_mask(&alpha_png(&[0, 255, 128])).unwrap();
        let gray = image::load_from_memory(&out).unwrap().to_luma8();
        assert_eq!(gray.as_raw(), &vec![255, 0, 127]);
        // Fully opaque, no alpha, or not a PNG: already white = edit, left alone.
        assert_eq!(sd_mask(&alpha_png(&[255, 255])), None);
        let rgb = image::RgbImage::from_pixel(2, 2, image::Rgb([255, 255, 255]));
        let mut no_alpha = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut no_alpha, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(sd_mask(no_alpha.get_ref()), None);
        assert_eq!(sd_mask(b"\xFF\xD8\xFF"), None);
        // Through an edit: the converted mask is what sd-server gets.
        let p = plan_edit(json!({"prompt": "x", "image": B64.encode(png(512, 512)),
                                 "mask": B64.encode(alpha_png(&[0, 255]))}))
        .unwrap();
        let sent = B64.decode(p.body["mask_image"].as_str().unwrap()).unwrap();
        let gray = image::load_from_memory(&sent).unwrap().to_luma8();
        assert_eq!(gray.as_raw(), &vec![255, 0]);
    }

    #[test]
    fn an_image_larger_than_the_largest_side_is_refused() {
        let err = plan_edit(
            json!({"prompt": "x", "image": B64.encode(png(6000, 4000)), "size": "512x512"}),
        )
        .unwrap_err();
        assert_eq!(err.param.as_deref(), Some("image"));
        assert!(err.message.contains("6000x4000"), "{}", err.message);
        // A reference image counts too.
        let refs = json!([B64.encode(png(512, 512)), B64.encode(png(512, 8192))]);
        assert!(plan_edit(json!({"prompt": "x", "image": refs})).is_err());
        assert!(plan_edit(json!({"prompt": "x", "image": B64.encode(png(4096, 3072))})).is_ok());
    }

    #[tokio::test]
    async fn an_openai_edit_form_is_read_into_the_json_fields() {
        let boundary = "XyZ";
        let mut body = Vec::new();
        let mut part = |head: &str, data: &[u8]| {
            body.extend(
                format!("--{boundary}\r\nContent-Disposition: form-data; {head}\r\n\r\n").bytes(),
            );
            body.extend(data);
            body.extend(b"\r\n");
        };
        part(
            r#"name="image[]"; filename="a.png"
Content-Type: image/png"#,
            &png(512, 384),
        );
        part(
            r#"name="image[]"; filename="b.png"
Content-Type: image/png"#,
            &png(64, 64),
        );
        part(r#"name="mask"; filename="m.png""#, &png(512, 384));
        // An image sent as a text part is base64 already: not encoded twice.
        let text_image = B64.encode(png(32, 32));
        part(r#"name="image[]""#, text_image.as_bytes());
        part(r#"name="prompt""#, b"42");
        part(r#"name="n""#, b"2");
        part(r#"name="strength""#, b"0.3");
        part(r#"name="model""#, b"sd-turbo");
        body.extend(format!("--{boundary}--\r\n").bytes());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}")).unwrap(),
        );
        let v = read_request(Kind::Edits, &headers, Bytes::from(body))
            .await
            .unwrap();
        // Numbers are numbers, the prompt stays text even when it looks like one.
        assert_eq!(v["prompt"], json!("42"));
        assert_eq!(
            (v["n"].clone(), v["strength"].clone()),
            (json!(2), json!(0.3))
        );
        assert_eq!(v["model"], json!("sd-turbo"));
        assert_eq!(
            v["image"],
            json!([
                B64.encode(png(512, 384)),
                B64.encode(png(64, 64)),
                text_image
            ])
        );
        let p = plan_edit(v).unwrap();
        assert_eq!(
            (p.body["width"].clone(), p.body["height"].clone()),
            (json!(512), json!(384))
        );
        assert_eq!(p.body["batch_count"], json!(2));
        assert!(p.body.get("mask_image").is_some());
        // JSON works on the edit route too.
        let json_body = Bytes::from(json!({"prompt": "x"}).to_string());
        assert!(
            read_request(Kind::Edits, &HeaderMap::new(), json_body.clone())
                .await
                .is_ok()
        );
        // A generation is always JSON, whatever the header says.
        assert!(read_request(Kind::Generations, &headers, json_body)
            .await
            .is_ok());
    }

    #[test]
    fn the_answer_has_openai_shape() {
        let images = vec!["AAA".to_string()];
        let v = shape(&images, "png", false, 5);
        assert_eq!(v["created"], 5);
        assert_eq!(v["data"][0]["b64_json"], "AAA");
        let v = shape(&images, "jpeg", true, 5);
        assert_eq!(v["data"][0]["url"], "data:image/jpeg;base64,AAA");
    }
}
