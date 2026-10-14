//! ONNX Runtime session of an audio classifier ([`crate::audio_pkg`]): one clip per graph run,
//! `input_values` in (`attention_mask` too when the graph asks for it), `logits` out.

use crate::api::ApiError;
use crate::audio_pkg::{self, AudioPackage, Features};
use crate::engine::{build_session, LoadOptions};
use crate::vision;
use base64::Engine as _;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::Value;
use std::sync::Mutex;

/// Longest clip read for a raw-waveform model (an AST reads its first ~10 s anyway).
const MAX_SECONDS: usize = 600;

/// A parsed `/hf` request of an audio classifier.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioJob {
    /// A WAV file (PCM 16/24/32 or float32, any rate and channel count).
    pub audio: Vec<u8>,
    pub top_k: Option<usize>,
}

/// `{"inputs": "<data URL | base64 of a WAV>", "parameters": {"top_k"}}`; `inputs` may also be
/// `{"audio": ...}`.
pub fn parse(body: &Value) -> Result<AudioJob, ApiError> {
    let input = body
        .get("inputs")
        .ok_or_else(|| ApiError::invalid("inputs is required"))?;
    let audio = match input {
        Value::String(s) => s.as_str(),
        Value::Object(o) => o
            .get("audio")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("inputs.audio must be a string"))?,
        _ => {
            return Err(ApiError::invalid(
                "inputs must be one WAV file (a data URL or base64)",
            ))
        }
    };
    let s = audio.trim();
    if s.starts_with("http://") || s.starts_with("https://") {
        return Err(ApiError::invalid(
            "audio URLs are not fetched: send the WAV as a data URL or base64",
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
    let audio = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| ApiError::invalid(format!("the audio is not valid base64: {}", e)))?;
    let top_k = match body.pointer("/parameters/top_k") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| ApiError::invalid("top_k must be a positive integer"))?
                as usize,
        ),
    };
    Ok(AudioJob { audio, top_k })
}

pub struct AudioEngine {
    session: Mutex<Session>,
    pub package: AudioPackage,
    input: String,
    /// `attention_mask` (all ones: one clip, no padding).
    mask: Option<String>,
    output: String,
    /// `DirectML`, `CUDA` or `CPU`.
    pub provider: String,
}

/// Loads an audio classifier and runs one second of silence through it.
pub fn load(opts: &LoadOptions, package: AudioPackage) -> Result<AudioEngine, String> {
    let (session, provider) = build_session(&package.onnx_path, opts.provider, opts.threads)?;
    let mut input = None;
    let mut mask = None;
    let mut unknown = Vec::new();
    for i in session.inputs() {
        match i.name() {
            "input_values" => input = Some(i.name().to_string()),
            "attention_mask" => mask = Some(i.name().to_string()),
            other => unknown.push(other.to_string()),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "the graph needs inputs this server cannot feed: {} (supported: input_values, attention_mask)",
            unknown.join(", ")
        ));
    }
    let input = input.ok_or("the graph has no input_values input (not an audio model)")?;
    let names: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
    let output = match names.as_slice() {
        n if n.contains(&"logits") => "logits".to_string(),
        [only] => only.to_string(),
        n => {
            return Err(format!(
                "the graph has no logits output (has: {})",
                n.join(", ")
            ))
        }
    };
    let engine = AudioEngine {
        session: Mutex::new(session),
        package,
        input,
        mask,
        output,
        provider: provider.to_string(),
    };
    let rate = engine.package.sampling_rate as usize;
    engine
        .classify(&vec![0.0; rate], None)
        .map_err(|e| e.message)?;
    Ok(engine)
}

impl AudioEngine {
    /// One request: decode and resample the WAV, compute the features, run, rank the labels.
    pub fn handle(&self, job: &AudioJob) -> Result<Value, ApiError> {
        let (samples, rate) = crate::asr_ctc::read_wav(&job.audio).map_err(ApiError::invalid)?;
        let target = self.package.sampling_rate;
        if samples.len() / rate.max(1) as usize > MAX_SECONDS {
            return Err(ApiError::invalid(format!(
                "the clip is longer than {} minutes",
                MAX_SECONDS / 60
            )));
        }
        let samples =
            crate::asr_ctc::resample(&samples, rate, target).map_err(ApiError::invalid)?;
        if samples.is_empty() {
            return Err(ApiError::invalid("the audio is empty"));
        }
        self.classify(&samples, job.top_k)
    }

    fn classify(&self, samples: &[f32], top_k: Option<usize>) -> Result<Value, ApiError> {
        let rate = self.package.sampling_rate;
        let (values, shape): (Vec<f32>, Vec<usize>) = match &self.package.features {
            Features::Raw { normalize } => {
                let mut v = samples.to_vec();
                if *normalize {
                    crate::asr_ctc::normalize(&mut v);
                }
                let n = v.len();
                (v, vec![1, n])
            }
            Features::Fbank {
                mel_bins,
                max_length,
                mean,
                std,
                normalize,
            } => (
                audio_pkg::ast_input(
                    samples,
                    rate,
                    *mel_bins,
                    *max_length,
                    *mean,
                    *std,
                    *normalize,
                ),
                vec![1, *max_length, *mel_bins],
            ),
        };
        let err = |e: ort::Error| ApiError::server(format!("tensor: {}", e));
        let mut feeds: Vec<(String, ort::value::DynTensor)> = Vec::new();
        if let Some(name) = &self.mask {
            let len = shape[1];
            feeds.push((
                name.clone(),
                Tensor::from_array(([1usize, len], vec![1i64; len]))
                    .map_err(err)?
                    .upcast(),
            ));
        }
        feeds.push((
            self.input.clone(),
            Tensor::from_array((shape, values)).map_err(err)?.upcast(),
        ));
        let mut session = self
            .session
            .lock()
            .map_err(|_| ApiError::server("the model session is poisoned"))?;
        let outputs = session
            .run(feeds)
            .map_err(|e| ApiError::server(format!("inference failed: {}", e)))?;
        let value = outputs
            .get(&self.output)
            .ok_or_else(|| ApiError::server(format!("output {} missing", self.output)))?;
        let (shape, data) = value
            .try_extract_tensor::<f32>()
            .map_err(|e| ApiError::server(format!("output: {}", e)))?;
        let classes = *shape.last().unwrap_or(&0) as usize;
        let row = data
            .get(..classes)
            .ok_or_else(|| ApiError::server(format!("unexpected logits shape {:?}", shape)))?;
        Ok(vision::labels_response(&vision::classify(
            row,
            &self.package.labels,
            false,
            top_k,
        )))
    }

    pub fn describe(&self) -> String {
        format!(
            "{} (audio-classification), {}, {} labels",
            self.package.name,
            self.provider,
            self.package.labels.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_take_a_wav_and_top_k() {
        let job =
            parse(&json!({"inputs": "data:audio/wav;base64,UklGRg==", "parameters": {"top_k": 3}}))
                .unwrap();
        assert_eq!(job.audio, b"RIFF");
        assert_eq!(job.top_k, Some(3));
        assert_eq!(
            parse(&json!({"inputs": {"audio": "UklGRg=="}}))
                .unwrap()
                .audio,
            b"RIFF"
        );
        assert!(parse(&json!({"inputs": "https://example.com/a.wav"})).is_err());
        assert!(parse(&json!({"inputs": "UklGRg==", "parameters": {"top_k": 0}})).is_err());
        assert!(parse(&json!({"inputs": ["a"]})).is_err());
    }
}
