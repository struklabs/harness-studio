//! Images in and out of a vision graph: request parsing, decoding, the image processor
//! (resize, crop, rescale, normalize to NCHW) and the post-processing of each head
//! (labels, boxes, masks, depth maps) in the shapes of the Hugging Face pipelines.
//!
//! No ONNX Runtime here; `vision_engine` runs the graph.

use crate::api::ApiError;
use crate::tasks::{sigmoid, softmax, LabelScore};
use crate::vision_pkg::{Preprocess, Resample, Size, VisionTask};
use base64::Engine as _;
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, RgbImage};
use serde_json::{json, Value};

/// Largest decoded image side: bigger ones are refused (memory), not silently cropped.
pub const MAX_SIDE: u32 = 8192;

/// A parsed `/hf` request of a vision model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VisionJob {
    /// The encoded image (PNG, JPEG, WebP, BMP, GIF...).
    pub image: Vec<u8>,
    pub top_k: Option<usize>,
    pub threshold: Option<f32>,
}

/// The bytes of an image given as a data URL or as plain base64.
pub fn image_bytes(s: &str) -> Result<Vec<u8>, ApiError> {
    let s = s.trim();
    if s.starts_with("http://") || s.starts_with("https://") {
        return Err(ApiError::invalid(
            "image URLs are not fetched: send the image as a data URL or base64",
        ));
    }
    let data = match s.strip_prefix("data:") {
        Some(rest) => {
            let (meta, data) = rest
                .split_once(',')
                .ok_or_else(|| ApiError::invalid("a data URL has no comma"))?;
            if !meta.ends_with(";base64") {
                return Err(ApiError::invalid("only base64 data URLs are supported"));
            }
            data
        }
        None => s,
    };
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| ApiError::invalid(format!("the image is not valid base64: {}", e)))
}

/// `{"inputs": "<data URL | base64>", "parameters": {"top_k", "threshold"}}`; `inputs` may
/// also be `{"image": ...}`.
pub fn parse(body: &Value) -> Result<VisionJob, ApiError> {
    let input = body
        .get("inputs")
        .ok_or_else(|| ApiError::invalid("inputs is required"))?;
    let image = match input {
        Value::String(s) => s.as_str(),
        Value::Object(o) => o
            .get("image")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("inputs.image must be a string"))?,
        _ => {
            return Err(ApiError::invalid(
                "inputs must be one image (a data URL or base64)",
            ))
        }
    };
    let params = body.get("parameters").cloned().unwrap_or(Value::Null);
    let top_k = match params.get("top_k") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| ApiError::invalid("top_k must be a positive integer"))?
                as usize,
        ),
    };
    let threshold = match params.get("threshold") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|f| (0.0..=1.0).contains(f))
                .ok_or_else(|| ApiError::invalid("threshold must be between 0 and 1"))?
                as f32,
        ),
    };
    Ok(VisionJob {
        image: image_bytes(image)?,
        top_k,
        threshold,
    })
}

/// Decodes an image (any orientation tag is ignored, as PIL does by default).
pub fn decode(bytes: &[u8]) -> Result<RgbImage, ApiError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| ApiError::invalid(format!("cannot read the image: {}", e)))?;
    if img.width() == 0 || img.height() == 0 {
        return Err(ApiError::invalid("the image is empty"));
    }
    if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        return Err(ApiError::invalid(format!(
            "the image is {}x{}; the largest side may be {}",
            img.width(),
            img.height(),
            MAX_SIDE
        )));
    }
    Ok(img.to_rgb8())
}

fn filter(r: Resample) -> FilterType {
    match r {
        Resample::Nearest => FilterType::Nearest,
        Resample::Bilinear => FilterType::Triangle,
        Resample::Bicubic => FilterType::CatmullRom,
    }
}

/// `round(value / multiple) * multiple`, at least one multiple (DPT's
/// `constrain_to_multiple_of`).
fn to_multiple(value: f64, multiple: u32) -> u32 {
    let m = multiple.max(1) as f64;
    ((value / m).round() * m).max(m) as u32
}

/// The size `(width, height)` an image of `(w, h)` is resized to.
pub fn target_size(size: Size, w: u32, h: u32) -> (u32, u32) {
    match size {
        Size::Exact { width, height } => (width, height),
        Size::ShortestEdge { short, long } => {
            let (min, max) = (w.min(h) as f64, w.max(h) as f64);
            let mut short = short as f64;
            if let Some(long) = long {
                if max / min * short > long as f64 {
                    short = (long as f64 * min / max).round();
                }
            }
            if w <= h {
                (short as u32, (short * h as f64 / w as f64) as u32)
            } else {
                ((short * w as f64 / h as f64) as u32, short as u32)
            }
        }
        Size::KeepAspect {
            width,
            height,
            multiple,
        } => {
            let mut sw = width as f64 / w as f64;
            let mut sh = height as f64 / h as f64;
            // Scale as little as possible: both edges take the factor closer to 1.
            if (1.0 - sw).abs() < (1.0 - sh).abs() {
                sh = sw;
            } else {
                sw = sh;
            }
            (
                to_multiple(sw * w as f64, multiple),
                to_multiple(sh * h as f64, multiple),
            )
        }
        Size::Divisor(d) => {
            let d = d.max(1);
            ((w / d).max(1) * d, (h / d).max(1) * d)
        }
    }
}

/// The processed pixels, `[1, 3, height, width]` row-major, and that height and width.
pub fn preprocess(img: &RgbImage, p: &Preprocess) -> (Vec<f32>, u32, u32) {
    let mut img = match p.resize {
        Some(size) => {
            let (w, h) = target_size(size, img.width(), img.height());
            let (w, h) = (w.max(1), h.max(1));
            if (w, h) == img.dimensions() {
                img.clone()
            } else {
                image::imageops::resize(img, w, h, filter(p.resample))
            }
        }
        None => img.clone(),
    };
    if let Some((cw, ch)) = p.crop {
        let (w, h) = img.dimensions();
        if cw <= w && ch <= h {
            let x = (w - cw) / 2;
            let y = (h - ch) / 2;
            img = image::imageops::crop_imm(&img, x, y, cw, ch).to_image();
        }
    }
    let (w, h) = img.dimensions();
    let plane = (w * h) as usize;
    let mut out = vec![0f32; 3 * plane];
    for (i, px) in img.pixels().enumerate() {
        for c in 0..3 {
            let src = if p.bgr { 2 - c } else { c };
            let mut v = px.0[src] as f32;
            if let Some(f) = p.rescale {
                v *= f;
            }
            if p.normalize {
                v = (v - p.mean[c]) / p.std[c];
            }
            out[c * plane + i] = v;
        }
    }
    (out, w, h)
}

/// Label `i`: its `id2label` name, or `LABEL_i`.
pub fn label(labels: &[String], i: usize) -> String {
    labels
        .get(i)
        .cloned()
        .unwrap_or_else(|| format!("LABEL_{}", i))
}

/// `image-classification`: the `top_k` (5 by default) labels of one row of logits; softmax,
/// or sigmoid for multi-label and single-logit heads.
pub fn classify(
    logits: &[f32],
    labels: &[String],
    multi_label: bool,
    top_k: Option<usize>,
) -> Vec<LabelScore> {
    let scores = if multi_label || logits.len() == 1 {
        logits.iter().map(|&x| sigmoid(x)).collect()
    } else {
        softmax(logits)
    };
    let mut out: Vec<LabelScore> = scores
        .into_iter()
        .enumerate()
        .map(|(i, score)| LabelScore {
            label: label(labels, i),
            score,
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(top_k.unwrap_or(5).min(out.len()));
    out
}

/// A detected object, in pixels of the original image.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub score: f32,
    pub label: String,
    pub xmin: i64,
    pub ymin: i64,
    pub xmax: i64,
    pub ymax: i64,
}

/// `object-detection` of a DETR-style head: `logits` `[queries, classes + 1]` (the last class
/// is "no object") and `boxes` `[queries, 4]` as normalized `(cx, cy, w, h)`. Objects scoring
/// at least `threshold` (0.5 by default), best first.
pub fn detect(
    logits: &[f32],
    boxes: &[f32],
    classes_plus_one: usize,
    labels: &[String],
    threshold: Option<f32>,
    width: u32,
    height: u32,
) -> Vec<Detection> {
    let threshold = threshold.unwrap_or(0.5);
    let k = classes_plus_one.max(2);
    let (w, h) = (width as f32, height as f32);
    let mut out = Vec::new();
    for (q, row) in logits.chunks_exact(k).enumerate() {
        let probs = softmax(row);
        let Some((best, &score)) = probs[..k - 1]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
        else {
            continue;
        };
        if score < threshold {
            continue;
        }
        let Some(b) = boxes.get(q * 4..q * 4 + 4) else {
            continue;
        };
        let (cx, cy, bw, bh) = (b[0], b[1], b[2], b[3]);
        let px = |v: f32, size: f32| (v * size).round().clamp(0.0, size) as i64;
        out.push(Detection {
            score,
            label: label(labels, best),
            xmin: px(cx - bw / 2.0, w),
            ymin: px(cy - bh / 2.0, h),
            xmax: px(cx + bw / 2.0, w),
            ymax: px(cy + bh / 2.0, h),
        });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// Bilinear sample of a `[h, w]` plane at fractional `(x, y)` (half-pixel centres, as
/// `torch.nn.functional.interpolate(align_corners=False)`).
fn sample(plane: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x = (x - 0.5).clamp(0.0, (w - 1) as f32);
    let y = (y - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |xx: usize, yy: usize| plane[yy * w + xx];
    let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
    let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Semantic segmentation: `logits` `[classes, h, w]` upsampled (bilinear) to the image and
/// the best class per pixel. Returns the class map `[height * width]`.
pub fn segment_classes(
    logits: &[f32],
    classes: usize,
    h: usize,
    w: usize,
    width: u32,
    height: u32,
) -> Vec<u16> {
    let plane = h * w;
    let (sx, sy) = (w as f32 / width as f32, h as f32 / height as f32);
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        let fy = (y as f32 + 0.5) * sy;
        for x in 0..width {
            let fx = (x as f32 + 0.5) * sx;
            let mut best = (0usize, f32::NEG_INFINITY);
            for c in 0..classes {
                let v = sample(&logits[c * plane..(c + 1) * plane], w, h, fx, fy);
                if v > best.1 {
                    best = (c, v);
                }
            }
            out.push(best.0 as u16);
        }
    }
    out
}

/// A mask as a base64 PNG (8-bit gray), the format of the Inference API.
pub fn png_base64(img: &GrayImage) -> Result<String, ApiError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(img.clone())
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| ApiError::server(format!("cannot encode the PNG: {}", e)))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

/// `image-segmentation` response: one entry per class present, largest area first,
/// `[{"score": null, "label", "mask": <base64 PNG>}]`.
pub fn segment_response(
    classes: &[u16],
    labels: &[String],
    width: u32,
    height: u32,
) -> Result<Value, ApiError> {
    let mut counts: std::collections::BTreeMap<u16, usize> = Default::default();
    for &c in classes {
        *counts.entry(c).or_default() += 1;
    }
    let mut order: Vec<(u16, usize)> = counts.into_iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out = Vec::with_capacity(order.len());
    for (c, _) in order {
        let mask = GrayImage::from_fn(width, height, |x, y| {
            image::Luma([if classes[(y * width + x) as usize] == c {
                255
            } else {
                0
            }])
        });
        out.push(json!({
            "score": null,
            "label": label(labels, c as usize),
            "mask": png_base64(&mask)?,
        }));
    }
    Ok(Value::Array(out))
}

/// `depth-estimation`: `depth` `[h, w]` resized (bilinear) to the image, scaled to 0..255 by
/// its maximum as the pipeline does, as a base64 PNG; with the raw range.
pub fn depth_response(
    depth: &[f32],
    h: usize,
    w: usize,
    width: u32,
    height: u32,
) -> Result<Value, ApiError> {
    let (sx, sy) = (w as f32 / width as f32, h as f32 / height as f32);
    let mut values = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            values.push(sample(
                depth,
                w,
                h,
                (x as f32 + 0.5) * sx,
                (y as f32 + 0.5) * sy,
            ));
        }
    }
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let min = values.iter().copied().fold(f32::INFINITY, f32::min);
    let scale = if max > 0.0 { 255.0 / max } else { 0.0 };
    let img = GrayImage::from_fn(width, height, |x, y| {
        let v = values[(y * width + x) as usize] * scale;
        image::Luma([v.clamp(0.0, 255.0) as u8])
    });
    Ok(json!({
        "depth": png_base64(&img)?,
        "width": width,
        "height": height,
        "min": min,
        "max": max,
    }))
}

/// `image-classification` and `object-detection` responses.
pub fn labels_response(list: &[LabelScore]) -> Value {
    crate::hf_api::label_scores(list)
}

pub fn detections_response(list: &[Detection]) -> Value {
    Value::Array(
        list.iter()
            .map(|d| {
                json!({
                    "score": d.score,
                    "label": d.label,
                    "box": {"xmin": d.xmin, "ymin": d.ymin, "xmax": d.xmax, "ymax": d.ymax},
                })
            })
            .collect(),
    )
}

/// Human name of a task for errors.
pub fn task_name(t: VisionTask) -> &'static str {
    t.pipeline_task()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision_pkg::parse_preprocess;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbImage::from_fn(w, h, |x, _| {
            image::Rgb([(x * 255 / w.max(1)) as u8, 0, 255])
        });
        let mut buf = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn requests_take_data_urls_and_base64() {
        let bytes = png(2, 2);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let job = parse(
            &json!({"inputs": format!("data:image/png;base64,{b64}"), "parameters": {"top_k": 3}}),
        )
        .unwrap();
        assert_eq!(job.image, bytes);
        assert_eq!(job.top_k, Some(3));
        let job =
            parse(&json!({"inputs": {"image": b64}, "parameters": {"threshold": 0.9}})).unwrap();
        assert_eq!(job.threshold, Some(0.9));
        assert!(parse(&json!({"inputs": "https://x/cat.png"})).is_err());
        assert!(parse(&json!({"inputs": "data:image/png,abc"})).is_err());
        assert!(parse(&json!({"inputs": ["a"]})).is_err());
        assert!(parse(&json!({"inputs": b64, "parameters": {"threshold": 2}})).is_err());
    }

    #[test]
    fn resize_rules_match_transformers() {
        // DETR: short edge 800, long edge at most 1333.
        let detr = Size::ShortestEdge {
            short: 800,
            long: Some(1333),
        };
        assert_eq!(target_size(detr, 640, 480), (1066, 800));
        assert_eq!(target_size(detr, 2000, 500), (1332, 333));
        // Depth Anything: 518, multiple of 14, aspect kept (640x480 -> 686x518).
        let da = Size::KeepAspect {
            width: 518,
            height: 518,
            multiple: 14,
        };
        assert_eq!(target_size(da, 640, 480), (686, 518));
        assert_eq!(target_size(Size::Divisor(32), 650, 481), (640, 480));
    }

    #[test]
    fn preprocessing_normalizes_to_nchw() {
        let p = parse_preprocess(
            &json!({"size": 4, "image_mean": [0.5,0.5,0.5], "image_std": [0.5,0.5,0.5]}),
        );
        let img = decode(&png(8, 6)).unwrap();
        let (data, w, h) = preprocess(&img, &p);
        assert_eq!((w, h), (4, 4));
        assert_eq!(data.len(), 3 * 16);
        // Blue is 255 everywhere: (1 - 0.5) / 0.5 = 1; green 0: -1.
        assert!(data[32..].iter().all(|v| (v - 1.0).abs() < 1e-5));
        assert!(data[16..32].iter().all(|v| (v + 1.0).abs() < 1e-5));
    }

    #[test]
    fn heads_are_post_processed() {
        let labels = vec!["cat".to_string(), "dog".to_string(), "bird".to_string()];
        let top = classify(&[2.0, 1.0, 0.0], &labels, false, Some(2));
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].label, "cat");
        assert!(top[0].score > top[1].score);
        // Two queries over 2 classes + no-object: the first is a confident dog, the second
        // is "no object".
        let logits = [0.0, 8.0, 0.0, 0.0, 0.0, 8.0];
        let boxes = [0.5, 0.5, 0.5, 0.5, 0.1, 0.1, 0.1, 0.1];
        let found = detect(&logits, &boxes, 3, &labels, None, 200, 100);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "dog");
        assert_eq!(
            (found[0].xmin, found[0].ymin, found[0].xmax, found[0].ymax),
            (50, 25, 150, 75)
        );
        // Two classes on a 2x1 grid: left is class 0, right class 1.
        let seg = [5.0, 0.0, 0.0, 5.0];
        let classes = segment_classes(&seg, 2, 1, 2, 4, 2);
        assert_eq!(classes, vec![0, 0, 1, 1, 0, 0, 1, 1]);
        let resp = segment_response(&classes, &labels, 4, 2).unwrap();
        assert_eq!(resp.as_array().unwrap().len(), 2);
        assert!(resp[0]["mask"].as_str().unwrap().len() > 10);
        let depth = depth_response(&[1.0, 2.0, 3.0, 4.0], 2, 2, 4, 4).unwrap();
        assert_eq!(depth["max"], 4.0);
        assert_eq!(depth["width"], 4);
    }
}
