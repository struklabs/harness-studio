//! ONNX Runtime session of a vision model ([`crate::vision_pkg`]): one image per graph run,
//! `pixel_values` in (`pixel_mask` too when the graph asks for it), the head's outputs out.

use crate::api::ApiError;
use crate::engine::{build_session, LoadOptions};
use crate::vision::{self, VisionJob};
use crate::vision_pkg::{VisionPackage, VisionTask};
use ort::session::Session;
use ort::value::{Tensor, TensorElementType};
use serde_json::Value;
use std::sync::Mutex;

/// An output tensor: shape and f32 values.
type Output = (Vec<i64>, Vec<f32>);

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
    /// `DirectML`, `CUDA` or `CPU`.
    pub provider: String,
}

/// The outputs a head needs, by the names transformers.js / Optimum exports use.
fn pick_outputs(names: &[&str], task: VisionTask) -> Result<Vec<String>, String> {
    let want: &[&str] = match task {
        VisionTask::Classify | VisionTask::Segment => &["logits"],
        VisionTask::Detect => &["logits", "pred_boxes"],
        VisionTask::Depth => &["predicted_depth"],
    };
    if want.iter().all(|w| names.contains(w)) {
        return Ok(want.iter().map(|s| s.to_string()).collect());
    }
    // A single-output graph with another name (`output`, `depth`...) still works for the
    // one-output heads.
    match (task, names) {
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

/// Loads a vision model and runs one warm-up image through it.
pub fn load(opts: &LoadOptions, package: VisionPackage) -> Result<VisionEngine, String> {
    let (session, provider) = build_session(&package.onnx_path, opts.provider, opts.threads)?;
    let mut pixel = None;
    let mut mask_input = None;
    let mut unknown = Vec::new();
    for input in session.inputs() {
        let name = input.name().to_string();
        match name.as_str() {
            "pixel_values" => {
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
    let engine = VisionEngine {
        session: Mutex::new(session),
        package,
        pixel_input,
        pixel_f16,
        mask_input,
        outputs,
        provider: provider.to_string(),
    };
    let warm = image::RgbImage::from_pixel(64, 48, image::Rgb([128, 128, 128]));
    engine
        .infer(&warm, &VisionJob::default())
        .map_err(|e| e.message)?;
    Ok(engine)
}

impl VisionEngine {
    /// Runs the graph on processed pixels; returns each output as shape and f32 values.
    fn run(&self, pixels: Vec<f32>, w: u32, h: u32) -> Result<Vec<Output>, ApiError> {
        let err = |e: ort::Error| ApiError::server(format!("tensor: {}", e));
        let shape = [1usize, 3, h as usize, w as usize];
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
        let outputs = self.run(pixels, w, h)?;
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
                let found = vision::detect(logits, boxes, k, labels, job.threshold, width, height);
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
            VisionTask::Depth => {
                let (shape, data) = &outputs[0];
                let (hh, ww) = match shape.as_slice() {
                    [_, hh, ww] | [_, 1, hh, ww] => (*hh as usize, *ww as usize),
                    _ => return Err(bad("predicted_depth", shape)),
                };
                vision::depth_response(data, hh, ww, width, height)
            }
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{} ({}), {}, outputs {}",
            self.package.name,
            self.package.task.pipeline_task(),
            self.provider,
            self.outputs.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_by_head() {
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
    }
}
