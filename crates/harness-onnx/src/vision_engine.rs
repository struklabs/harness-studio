//! ONNX Runtime session of a vision model ([`crate::vision_pkg`]): one image per graph run,
//! `pixel_values` in (`pixel_mask` too when the graph asks for it), the head's outputs out.

use crate::api::ApiError;
use crate::engine::{build_session, finish_profile, finite_output, LoadOptions, Provider};
use crate::vision::{self, VisionJob};
use crate::vision_pkg::{VisionPackage, VisionTask};
use ort::session::Session;
use ort::value::{Tensor, TensorElementType};
use serde_json::Value;
use std::sync::Mutex;
use tokenizers::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

/// An output tensor: shape and f32 values.
type Output = (Vec<i64>, Vec<f32>);

/// Default sentence of a zero-shot label (the transformers pipeline's).
const DEFAULT_TEMPLATE: &str = "This is a photo of {}.";

/// The text inputs of a zero-shot graph.
struct TextInputs {
    tokenizer: Tokenizer,
    ids: String,
    mask: Option<String>,
    pad_id: i64,
    max_length: usize,
    pad_to_max: bool,
    canonicalize: bool,
    sigmoid: bool,
}

/// Token ids and attention mask of a batch of labels, padded to one length: `(ids, mask, rows,
/// length)`.
type TextBatch = (Vec<i64>, Vec<i64>, usize, usize);

pub struct VisionEngine {
    session: Mutex<Session>,
    pub package: VisionPackage,
    pixel_input: String,
    /// The graph takes half-precision pixels (`model_fp16.onnx`).
    pixel_f16: bool,
    /// `pixel_mask` and its fixed `(height, width)` when the export pinned one (DETR from
    /// transformers.js is traced at 64x64; an all-ones mask means the same at any size).
    mask_input: Option<(String, Option<(usize, usize)>)>,
    /// Outputs read, in the order the task needs them.
    outputs: Vec<String>,
    /// The tokenizer and text inputs of a zero-shot model.
    text: Option<TextInputs>,
    /// The text decoder of a captioner.
    decoder: Option<crate::decoder::Decoder>,
    /// `DirectML`, `CUDA` or `CPU`.
    pub provider: String,
}

/// The outputs a head needs, by the names transformers.js / Optimum exports use.
fn pick_outputs(names: &[&str], task: VisionTask) -> Result<Vec<String>, String> {
    let want: &[&str] = match task {
        VisionTask::Classify | VisionTask::Segment => &["logits"],
        VisionTask::Foreground => &["output_image"],
        VisionTask::PromptSegment => &["logits"],
        VisionTask::Detect => &["logits", "pred_boxes"],
        VisionTask::Depth => &["predicted_depth"],
        VisionTask::ZeroShot => &["logits_per_image"],
        VisionTask::ZeroShotDetect => &["logits", "pred_boxes"],
        // `pooler_output` joins when the graph has it (for `pool: true`).
        VisionTask::Features => {
            return if names.contains(&"last_hidden_state") {
                let mut out = vec!["last_hidden_state".to_string()];
                if names.contains(&"pooler_output") {
                    out.push("pooler_output".to_string());
                }
                Ok(out)
            } else if let [only] = names {
                Ok(vec![only.to_string()])
            } else {
                Err(format!(
                    "an image feature graph needs a last_hidden_state output (has: {})",
                    names.join(", ")
                ))
            };
        }
        VisionTask::Caption => &["last_hidden_state"],
    };
    if want.iter().all(|w| names.contains(w)) {
        return Ok(want.iter().map(|s| s.to_string()).collect());
    }
    // A single-output graph with another name (`output`, `depth`...) still works for the
    // one-output heads.
    match (task, names) {
        (VisionTask::ZeroShot, _) => Err(format!(
            "a zero-shot image graph needs a logits_per_image output (has: {})",
            names.join(", ")
        )),
        (VisionTask::ZeroShotDetect, _) => Err(format!(
            "an open-vocabulary detection graph needs logits and pred_boxes outputs (has: {})",
            names.join(", ")
        )),
        (VisionTask::Detect, _) => Err(format!(
            "an object-detection graph needs logits and pred_boxes outputs (has: {})",
            names.join(", ")
        )),
        (_, [only]) => Ok(vec![only.to_string()]),
        _ => Err(format!(
            "the graph has none of the expected outputs {:?} (has: {})",
            want,
            names.join(", ")
        )),
    }
}

/// Whether open-vocabulary detection outputs agree: `logits` `[patches, queries]` with one
/// query per label, `pred_boxes` `[patches, 4]`.
fn open_detection_fits(logits: usize, boxes: usize, queries: usize) -> bool {
    queries > 0
        && boxes.is_multiple_of(4)
        && logits.is_multiple_of(queries)
        && logits / queries == boxes / 4
}

/// Loads a vision model and runs one warm-up image through it.
pub fn load(opts: &LoadOptions, package: VisionPackage) -> Result<VisionEngine, String> {
    let (session, provider) = build_session(&package.onnx_path, opts.provider, opts.threads)?;
    let mut pixel = None;
    let mut mask_input = None;
    let mut ids_input = None;
    let mut attention_input = None;
    let mut unknown = Vec::new();
    let zero_shot = package.text.is_some();
    for input in session.inputs() {
        let name = input.name().to_string();
        match name.as_str() {
            "pixel_values" | "input_image"
                if name == "pixel_values" || package.task == VisionTask::Foreground =>
            {
                pixel = Some((
                    name,
                    input.dtype().tensor_type() == Some(TensorElementType::Float16),
                ))
            }
            "pixel_mask" => {
                let fixed = match input.dtype().tensor_shape().map(|s| &s[..]) {
                    Some([_, h, w]) if *h > 0 && *w > 0 => Some((*h as usize, *w as usize)),
                    _ => None,
                };
                mask_input = Some((name, fixed))
            }
            "input_ids" if zero_shot => ids_input = Some(name),
            "attention_mask" if zero_shot => attention_input = Some(name),
            _ => unknown.push(name),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "the graph needs inputs this server cannot feed: {} (supported: pixel_values, pixel_mask)",
            unknown.join(", ")
        ));
    }
    let (pixel_input, pixel_f16) =
        pixel.ok_or("the graph has no pixel_values input (not an image model)")?;
    let names: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
    let outputs = pick_outputs(&names, package.task)?;
    let text = match &package.text {
        Some(side) => {
            let ids = ids_input.ok_or("a zero-shot image graph needs an input_ids input")?;
            let mut tokenizer = Tokenizer::from_file(&side.tokenizer_path)
                .map_err(|e| format!("cannot read {}: {}", side.tokenizer_path.display(), e))?;
            let pad_id = side
                .pad_token
                .as_deref()
                .and_then(|t| tokenizer.token_to_id(t))
                .or_else(|| tokenizer.get_padding().map(|p| p.pad_id))
                .unwrap_or(0);
            tokenizer.with_padding(None);
            tokenizer
                .with_truncation(Some(TruncationParams {
                    direction: TruncationDirection::Right,
                    max_length: side.max_length,
                    strategy: TruncationStrategy::LongestFirst,
                    stride: 0,
                }))
                .map_err(|e| format!("tokenizer truncation: {}", e))?;
            Some(TextInputs {
                tokenizer,
                ids,
                mask: attention_input,
                pad_id: i64::from(pad_id),
                max_length: side.max_length,
                pad_to_max: side.pad_to_max,
                canonicalize: side.canonicalize,
                sigmoid: side.sigmoid,
            })
        }
        None => None,
    };
    let decoder = match &package.caption {
        Some(side) => Some(crate::decoder::load(
            opts,
            &side.decoder_path,
            &side.tokenizer_path,
            side.gen.clone(),
        )?),
        None => None,
    };
    let engine = VisionEngine {
        session: Mutex::new(session),
        package,
        pixel_input,
        pixel_f16,
        mask_input,
        outputs,
        text,
        decoder,
        provider: provider.to_string(),
    };
    let warm = image::RgbImage::from_pixel(64, 48, image::Rgb([128, 128, 128]));
    let job = VisionJob {
        candidate_labels: vec!["a cat".into(), "a dog".into()],
        max_new_tokens: Some(2),
        ..VisionJob::default()
    };
    if let Err(e) = engine.infer(&warm, &job) {
        finish_profile(&engine.session);
        if engine.provider != "CPU" {
            eprintln!(
                "harness-onnx: {} warm-up failed: {} (falling back to the CPU)",
                engine.provider, e.message
            );
            let mut cpu = opts.clone();
            cpu.provider = Provider::Cpu;
            let package = engine.package.clone();
            drop(engine);
            return load(&cpu, package);
        }
        return Err(e.message);
    }
    finish_profile(&engine.session);
    Ok(engine)
}

impl VisionEngine {
    /// Runs the graph on processed pixels; returns each output as shape and f32 values.
    fn run(
        &self,
        pixels: Vec<f32>,
        w: u32,
        h: u32,
        text: Option<TextBatch>,
    ) -> Result<Vec<Output>, ApiError> {
        let err = |e: ort::Error| ApiError::server(format!("tensor: {}", e));
        let rows = if self.package.task == VisionTask::PromptSegment {
            text.as_ref().map_or(1, |t| t.2)
        } else {
            1
        };
        let pixels = if rows > 1 {
            pixels.repeat(rows)
        } else {
            pixels
        };
        let shape = [rows, 3, h as usize, w as usize];
        let input: ort::value::DynTensor = if self.pixel_f16 {
            let half: Vec<half::f16> = pixels.into_iter().map(half::f16::from_f32).collect();
            Tensor::from_array((shape, half)).map_err(err)?.upcast()
        } else {
            Tensor::from_array((shape, pixels)).map_err(err)?.upcast()
        };
        let mut feeds: Vec<(String, ort::value::DynTensor)> =
            vec![(self.pixel_input.clone(), input)];
        if let Some((name, fixed)) = &self.mask_input {
            let (mh, mw) = fixed.unwrap_or((h as usize, w as usize));
            let ones = vec![1i64; mh * mw];
            feeds.push((
                name.clone(),
                Tensor::from_array(([1usize, mh, mw], ones))
                    .map_err(err)?
                    .upcast(),
            ));
        }
        if let (Some(inputs), Some((ids, mask, rows, len))) = (&self.text, text) {
            feeds.push((
                inputs.ids.clone(),
                Tensor::from_array(([rows, len], ids))
                    .map_err(err)?
                    .upcast(),
            ));
            if let Some(name) = &inputs.mask {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([rows, len], mask))
                        .map_err(err)?
                        .upcast(),
                ));
            }
        }
        let mut session = self
            .session
            .lock()
            .map_err(|_| ApiError::server("the model session is poisoned"))?;
        let outputs = session
            .run(feeds)
            .map_err(|e| ApiError::server(format!("inference failed: {}", e)))?;
        let mut out = Vec::with_capacity(self.outputs.len());
        for name in &self.outputs {
            let value = outputs
                .get(name)
                .ok_or_else(|| ApiError::server(format!("output {} missing", name)))?;
            let err = |e: ort::Error| ApiError::server(format!("output: {}", e));
            out.push(match value.dtype().tensor_type() {
                Some(TensorElementType::Float16) => {
                    let (s, d) = value.try_extract_tensor::<half::f16>().map_err(err)?;
                    (s.to_vec(), d.iter().map(|v| v.to_f32()).collect())
                }
                _ => {
                    let (s, d) = value.try_extract_tensor::<f32>().map_err(err)?;
                    (s.to_vec(), d.to_vec())
                }
            });
            finite_output(name, &out.last().expect("output just added").1)?;
        }
        Ok(out)
    }

    /// One request: decode, preprocess, run, post-process into the pipeline's JSON.
    pub fn handle(&self, job: &VisionJob) -> Result<Value, ApiError> {
        let img = vision::decode(&job.image)?;
        self.infer(&img, job)
    }

    fn infer(&self, img: &image::RgbImage, job: &VisionJob) -> Result<Value, ApiError> {
        let (width, height) = img.dimensions();
        let (pixels, w, h) = vision::preprocess(img, &self.package.preprocess);
        let batch = match &self.text {
            Some(inputs) => Some(self.encode_labels(inputs, job)?),
            None => None,
        };
        let outputs = self.run(pixels, w, h, batch)?;
        let labels = &self.package.labels;
        let bad = |what: &str, shape: &[i64]| {
            ApiError::server(format!("unexpected {} shape {:?}", what, shape))
        };
        match self.package.task {
            VisionTask::Classify => {
                let (shape, data) = &outputs[0];
                let classes = *shape.last().ok_or_else(|| bad("logits", shape))? as usize;
                let row = data.get(..classes).ok_or_else(|| bad("logits", shape))?;
                Ok(vision::labels_response(&vision::classify(
                    row,
                    labels,
                    self.package.multi_label,
                    job.top_k,
                )))
            }
            VisionTask::Detect => {
                let (shape, logits) = &outputs[0];
                let (_, boxes) = &outputs[1];
                let k = *shape.last().ok_or_else(|| bad("logits", shape))? as usize;
                let mut found = if self.package.focal_detection {
                    vision::detect_focal(logits, boxes, k, labels, job.threshold, (width, height))
                } else {
                    vision::detect(logits, boxes, k, labels, job.threshold, width, height)
                };
                if let Some(k) = job.top_k {
                    found.truncate(k);
                }
                Ok(vision::detections_response(&found))
            }
            VisionTask::Segment => {
                let (shape, data) = &outputs[0];
                let [_, c, hh, ww] = shape.as_slice() else {
                    return Err(bad("logits", shape));
                };
                let classes = vision::segment_classes(
                    data,
                    *c as usize,
                    *hh as usize,
                    *ww as usize,
                    width,
                    height,
                );
                vision::segment_response(&classes, labels, width, height)
            }
            VisionTask::Foreground | VisionTask::PromptSegment => {
                let (shape, data) = &outputs[0];
                let (maps, hh, ww) = match shape.as_slice() {
                    [n, 1, hh, ww] | [n, hh, ww] => (*n as usize, *hh as usize, *ww as usize),
                    _ => return Err(bad("mask logits", shape)),
                };
                let names = if self.package.task == VisionTask::Foreground {
                    vec!["foreground".to_string()]
                } else {
                    job.candidate_labels.clone()
                };
                if maps != names.len() || data.len() != maps * hh * ww {
                    return Err(bad("mask logits", shape));
                }
                vision::binary_masks_response(data, hh, ww, &names, width, height, job.threshold)
            }
            VisionTask::Depth => {
                let (shape, data) = &outputs[0];
                let (hh, ww) = match shape.as_slice() {
                    [_, hh, ww] | [_, 1, hh, ww] => (*hh as usize, *ww as usize),
                    _ => return Err(bad("predicted_depth", shape)),
                };
                vision::depth_response(data, hh, ww, width, height)
            }
            VisionTask::ZeroShot => {
                let (shape, data) = &outputs[0];
                let n = job.candidate_labels.len();
                // One image: a row of one score per label.
                if data.len() != n {
                    return Err(bad("logits_per_image", shape));
                }
                let row = &data[..];
                let multi = self.text.as_ref().is_some_and(|t| t.sigmoid);
                Ok(vision::labels_response(&vision::classify(
                    row,
                    &job.candidate_labels,
                    multi,
                    Some(job.top_k.unwrap_or(n)),
                )))
            }
            VisionTask::Features => {
                let (shape, data) = match (job.pool, outputs.get(1)) {
                    (true, Some(pooled)) => pooled,
                    (true, None) if self.package.cls_pool => {
                        // [1, tokens, dim]: the first token of the one image.
                        let (shape, data) = &outputs[0];
                        let dim = *shape
                            .last()
                            .ok_or_else(|| bad("last_hidden_state", shape))?;
                        let cls = data
                            .get(..dim as usize)
                            .ok_or_else(|| bad("last_hidden_state", shape))?;
                        return Ok(vision::nested(&[1, dim as usize], cls));
                    }
                    (true, None) => return Err(ApiError::invalid(
                        "this model has no pooler_output; ask without pool for its hidden states",
                    )),
                    (false, _) => &outputs[0],
                };
                let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
                Ok(vision::nested(&dims, data))
            }
            VisionTask::ZeroShotDetect => {
                let (shape, logits) = &outputs[0];
                let (_, boxes) = &outputs[1];
                if !open_detection_fits(logits.len(), boxes.len(), job.candidate_labels.len()) {
                    return Err(bad("logits", shape));
                }
                // OWLv2 boxes are relative to the padded square.
                let (fw, fh) = if self.package.preprocess.pad_square {
                    let side = width.max(height);
                    (side, side)
                } else {
                    (width, height)
                };
                let mut found = vision::detect_open(
                    logits,
                    boxes,
                    &job.candidate_labels,
                    job.threshold,
                    (fw, fh),
                    (width, height),
                );
                if let Some(k) = job.top_k {
                    found.truncate(k);
                }
                Ok(vision::detections_response(&found))
            }
            VisionTask::Caption => {
                let (shape, data) = &outputs[0];
                let decoder = self
                    .decoder
                    .as_ref()
                    .ok_or_else(|| ApiError::server("the captioner has no decoder"))?;
                let ids = decoder.generate(data, shape, job.max_new_tokens, None)?;
                let text = decoder.decode(&ids)?;
                Ok(serde_json::json!([{ "generated_text": text }]))
            }
        }
    }

    /// The labels of a zero-shot request in their sentences, tokenized and padded.
    fn encode_labels(&self, inputs: &TextInputs, job: &VisionJob) -> Result<TextBatch, ApiError> {
        if job.candidate_labels.is_empty() {
            return Err(ApiError::invalid(
                "parameters.candidate_labels needs at least one label",
            ));
        }
        // A detector's queries are the labels themselves (the pipeline puts no sentence around).
        let template = match self.package.task {
            VisionTask::ZeroShotDetect | VisionTask::PromptSegment => "{}",
            _ => job
                .hypothesis_template
                .as_deref()
                .unwrap_or(DEFAULT_TEMPLATE),
        };
        let sentences: Vec<String> = job
            .candidate_labels
            .iter()
            .map(|l| {
                let s = template.replace("{}", l);
                if inputs.canonicalize {
                    canonical(&s)
                } else {
                    s
                }
            })
            .collect();
        let encoded = inputs
            .tokenizer
            .encode_batch(sentences, true)
            .map_err(|e| ApiError::server(format!("tokenizer: {}", e)))?;
        let rows: Vec<Vec<i64>> = encoded
            .iter()
            .map(|e| e.get_ids().iter().map(|&i| i64::from(i)).collect())
            .collect();
        Ok(pad_batch(
            &rows,
            inputs.pad_id,
            inputs.pad_to_max.then_some(inputs.max_length),
        ))
    }

    pub fn describe(&self) -> String {
        format!(
            "{} ({}), {}, outputs {}{}",
            self.package.name,
            self.package.task.pipeline_task(),
            self.provider,
            self.outputs.join(", "),
            self.decoder
                .as_ref()
                .map(|d| format!(", {}", d.describe()))
                .unwrap_or_default()
        )
    }
}

/// SigLIP's text canonicalization: lower case, no ASCII punctuation, single spaces.
fn canonical(text: &str) -> String {
    let kept: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_ascii_punctuation())
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pads token rows to the longest (or to `fixed`) with `pad`; the mask marks real tokens.
fn pad_batch(rows: &[Vec<i64>], pad: i64, fixed: Option<usize>) -> TextBatch {
    let len = fixed
        .unwrap_or_else(|| rows.iter().map(Vec::len).max().unwrap_or(0))
        .max(1);
    let mut ids = Vec::with_capacity(rows.len() * len);
    let mut mask = Vec::with_capacity(rows.len() * len);
    for row in rows {
        let real = row.len().min(len);
        ids.extend_from_slice(&row[..real]);
        ids.extend(std::iter::repeat_n(pad, len - real));
        mask.extend(std::iter::repeat_n(1, real));
        mask.extend(std::iter::repeat_n(0, len - real));
    }
    (ids, mask, rows.len(), len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_detection_outputs_must_match_the_labels() {
        // 3 patches x 2 labels, 3 boxes.
        assert!(open_detection_fits(6, 12, 2));
        // The graph scored 3 queries, the request named 2.
        assert!(!open_detection_fits(9, 12, 2));
        // 6 logits as 2 labels is 3 patches, but there are 4 boxes.
        assert!(!open_detection_fits(6, 16, 2));
        assert!(!open_detection_fits(6, 13, 2));
        assert!(!open_detection_fits(0, 0, 0));
    }

    #[test]
    fn siglip_sentences_are_canonical() {
        assert_eq!(
            canonical("This is a photo of  a T-shirt."),
            "this is a photo of a tshirt"
        );
    }

    #[test]
    fn labels_pad_to_the_longest_or_a_fixed_length() {
        let rows = vec![vec![1, 5, 2], vec![1, 7, 8, 9, 2]];
        let (ids, mask, n, len) = pad_batch(&rows, 0, None);
        assert_eq!((n, len), (2, 5));
        assert_eq!(ids, vec![1, 5, 2, 0, 0, 1, 7, 8, 9, 2]);
        assert_eq!(mask, vec![1, 1, 1, 0, 0, 1, 1, 1, 1, 1]);
        let (ids, mask, _, len) = pad_batch(&rows, 9, Some(4));
        assert_eq!(len, 4);
        assert_eq!(ids, vec![1, 5, 2, 9, 1, 7, 8, 9]);
        assert_eq!(mask, vec![1, 1, 1, 0, 1, 1, 1, 1]);
    }

    #[test]
    fn outputs_by_head() {
        assert_eq!(
            pick_outputs(
                &["last_hidden_state", "pooler_output"],
                VisionTask::Features
            )
            .unwrap(),
            vec!["last_hidden_state", "pooler_output"]
        );
        assert_eq!(
            pick_outputs(&["last_hidden_state"], VisionTask::Features).unwrap(),
            vec!["last_hidden_state"]
        );
        assert!(pick_outputs(&["a", "b"], VisionTask::Features).is_err());
        assert_eq!(
            pick_outputs(&["logits", "pred_boxes"], VisionTask::Detect).unwrap(),
            vec!["logits", "pred_boxes"]
        );
        assert!(pick_outputs(&["logits"], VisionTask::Detect).is_err());
        assert_eq!(
            pick_outputs(&["predicted_depth"], VisionTask::Depth).unwrap(),
            vec!["predicted_depth"]
        );
        assert_eq!(
            pick_outputs(&["output"], VisionTask::Classify).unwrap(),
            vec!["output"]
        );
        assert!(pick_outputs(&["a", "b"], VisionTask::Segment).is_err());
        assert_eq!(
            pick_outputs(
                &[
                    "logits_per_image",
                    "logits_per_text",
                    "text_embeds",
                    "image_embeds"
                ],
                VisionTask::ZeroShot
            )
            .unwrap(),
            vec!["logits_per_image"]
        );
    }
}
