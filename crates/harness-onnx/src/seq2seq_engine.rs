//! ONNX Runtime sessions of a text encoder-decoder ([`crate::seq2seq_pkg`]): the text is
//! tokenized and encoded once, then [`crate::decoder`] generates the output greedily.

use crate::api::ApiError;
use crate::decoder::{self, Decoder};
use crate::engine::{build_session, LoadOptions};
use crate::seq2seq_pkg::{Seq2SeqJob, Seq2SeqPackage};
use ort::session::Session;
use ort::value::{Tensor, TensorElementType};
use serde_json::{json, Value};
use std::sync::Mutex;
use tokenizers::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

pub struct Seq2SeqEngine {
    encoder: Mutex<Session>,
    decoder: Decoder,
    tokenizer: Tokenizer,
    pub package: Seq2SeqPackage,
    ids: String,
    mask: Option<String>,
    /// `DirectML`, `CUDA` or `CPU`.
    pub provider: String,
}

/// Loads the encoder and the decoder and generates two tokens as a warm-up.
pub fn load(opts: &LoadOptions, package: Seq2SeqPackage) -> Result<Seq2SeqEngine, String> {
    let (session, provider) = build_session(&package.encoder_path, opts.provider, opts.threads)?;
    let mut ids = None;
    let mut mask = None;
    let mut unknown = Vec::new();
    for input in session.inputs() {
        match input.name() {
            "input_ids" => ids = Some(input.name().to_string()),
            "attention_mask" => mask = Some(input.name().to_string()),
            other => unknown.push(other.to_string()),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "the encoder needs inputs this server cannot feed: {}",
            unknown.join(", ")
        ));
    }
    let ids = ids.ok_or("the encoder has no input_ids input")?;
    let mut tokenizer = decoder::load_tokenizer(&package.tokenizer_path)?;
    tokenizer.with_padding(None);
    tokenizer
        .with_truncation(Some(TruncationParams {
            direction: TruncationDirection::Right,
            max_length: package.max_input,
            strategy: TruncationStrategy::LongestFirst,
            stride: 0,
        }))
        .map_err(|e| format!("tokenizer truncation: {}", e))?;
    let decoder = decoder::load(
        opts,
        &package.decoder_path,
        &package.tokenizer_path,
        package.gen.clone(),
    )?;
    let engine = Seq2SeqEngine {
        encoder: Mutex::new(session),
        decoder,
        tokenizer,
        package,
        ids,
        mask,
        provider: provider.to_string(),
    };
    engine
        .handle(&Seq2SeqJob {
            text: "Hello.".into(),
            max_new_tokens: Some(2),
            num_beams: Some(1),
        })
        .map_err(|e| e.message)?;
    Ok(engine)
}

impl Seq2SeqEngine {
    /// One request: tokenize, encode, generate, decode into the pipeline's JSON.
    pub fn handle(&self, job: &Seq2SeqJob) -> Result<Value, ApiError> {
        let encoding = self
            .tokenizer
            .encode(job.text.as_str(), true)
            .map_err(|e| ApiError::invalid(format!("cannot tokenize the input: {}", e)))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&i| i64::from(i)).collect();
        let n = ids.len();
        if n == 0 {
            return Err(ApiError::invalid("the input has no tokens"));
        }
        let err = |e: ort::Error| ApiError::server(format!("encoder: {}", e));
        let mut feeds: Vec<(String, ort::value::DynValue)> = vec![(
            self.ids.clone(),
            Tensor::from_array(([1usize, n], ids))
                .map_err(err)?
                .into_dyn(),
        )];
        if let Some(name) = &self.mask {
            feeds.push((
                name.clone(),
                Tensor::from_array(([1usize, n], vec![1i64; n]))
                    .map_err(err)?
                    .into_dyn(),
            ));
        }
        let (shape, states) = {
            let mut session = self
                .encoder
                .lock()
                .map_err(|_| ApiError::server("the encoder session is poisoned"))?;
            let outputs = session
                .run(feeds)
                .map_err(|e| ApiError::server(format!("encoder failed: {}", e)))?;
            let value = outputs
                .get("last_hidden_state")
                .or_else(|| (outputs.len() == 1).then(|| &outputs[0]))
                .ok_or_else(|| ApiError::server("the encoder has no last_hidden_state"))?;
            match value.dtype().tensor_type() {
                Some(TensorElementType::Float16) => {
                    let (s, d) = value.try_extract_tensor::<half::f16>().map_err(err)?;
                    (
                        s.to_vec(),
                        d.iter().map(|v| v.to_f32()).collect::<Vec<f32>>(),
                    )
                }
                _ => {
                    let (s, d) = value.try_extract_tensor::<f32>().map_err(err)?;
                    (s.to_vec(), d.to_vec())
                }
            }
        };
        let tokens = self
            .decoder
            .generate(&states, &shape, job.max_new_tokens, job.num_beams)?;
        let text = self.decoder.decode(&tokens)?;
        Ok(json!([{ self.package.output_key(): text }]))
    }

    pub fn describe(&self) -> String {
        format!(
            "{} ({}), {}, {}",
            self.package.name,
            self.package.task,
            self.provider,
            self.decoder.describe()
        )
    }
}
