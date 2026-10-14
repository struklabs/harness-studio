//! The text decoder of an encoder-decoder model (an image captioner, T5 / BART / Marian):
//! greedy decoding over the encoder's hidden states with the logits rules of
//! [`crate::generation`], using the KV cache when the graph has one.
//!
//! A merged decoder (`decoder_model_merged.onnx`, Optimum / transformers.js) takes
//! `use_cache_branch` and `past_key_values.{i}[.decoder|.encoder].{key,value}` and answers
//! `present.*`: the first step runs with an empty cache and the whole prefix, every later step
//! with the cache and the newest token only. The cross-attention (`.encoder.`) cache is computed
//! once, on the first step; the cached branch answers it empty, so the first values are fed
//! again. A decoder without past inputs gets the whole sequence every step.

use crate::api::ApiError;
use crate::engine::{build_session, LoadOptions};
use crate::generation::{self, GenConfig};
use ort::session::Session;
use ort::value::{DynValue, Tensor, TensorElementType};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokenizers::Tokenizer;

pub struct Decoder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    path: PathBuf,
    pub gen: GenConfig,
    ids: String,
    encoder_states: String,
    /// The graph takes half-precision encoder states.
    states_f16: bool,
    /// `attention_mask` / `encoder_attention_mask` when the graph asks for them (all ones).
    mask: Option<String>,
    encoder_mask: Option<String>,
    /// `use_cache_branch` of a merged decoder.
    cache_branch: Option<String>,
    /// `(past input, present output)` names, in input order.
    cache: Vec<(String, String)>,
    /// The empty cache of the first step: `[1, heads, 0, head_dim]` per input, with its type.
    cache_shape: (usize, usize, bool),
    logits: String,
}

/// A cache tensor kept across steps (the cross-attention cache of the first step).
enum Stored {
    F32(Vec<usize>, Vec<f32>),
    F16(Vec<usize>, Vec<half::f16>),
}

impl Stored {
    fn of(value: &DynValue) -> Result<Self, ort::Error> {
        Ok(match value.dtype().tensor_type() {
            Some(TensorElementType::Float16) => {
                let (s, d) = value.try_extract_tensor::<half::f16>()?;
                Stored::F16(s.iter().map(|x| *x as usize).collect(), d.to_vec())
            }
            _ => {
                let (s, d) = value.try_extract_tensor::<f32>()?;
                Stored::F32(s.iter().map(|x| *x as usize).collect(), d.to_vec())
            }
        })
    }

    fn value(&self) -> Result<DynValue, ort::Error> {
        Ok(match self {
            Stored::F32(s, d) => Tensor::from_array((s.clone(), d.clone()))?.into_dyn(),
            Stored::F16(s, d) => Tensor::from_array((s.clone(), d.clone()))?.into_dyn(),
        })
    }

    /// The batch rows `rows` of the tensor, in that order (a beam's cache follows its beam).
    fn gather(&self, rows: &[usize]) -> Stored {
        fn pick<T: Copy>(shape: &[usize], data: &[T], rows: &[usize]) -> (Vec<usize>, Vec<T>) {
            let size: usize = shape.iter().skip(1).product();
            let mut out = Vec::with_capacity(size * rows.len());
            for &r in rows {
                out.extend_from_slice(&data[r * size..(r + 1) * size]);
            }
            let mut shape = shape.to_vec();
            shape[0] = rows.len();
            (shape, out)
        }
        match self {
            Stored::F32(s, d) => {
                let (s, d) = pick(s, d, rows);
                Stored::F32(s, d)
            }
            Stored::F16(s, d) => {
                let (s, d) = pick(s, d, rows);
                Stored::F16(s, d)
            }
        }
    }
}

/// Reads a `tokenizer.json`, mending what the tokenizers crate cannot read: transformers.js
/// Marian exports carry a `Precompiled` normalizer without its charsmap (`null`). The charsmap
/// is SentencePiece's `nmt_nfkc` table, rebuilt here from its rules: NFKC, the whitespace
/// controls (tab, line feed, carriage return, vertical tab, form feed) as spaces, the other C0
/// controls, DEL and the zero-width characters removed.
pub fn load_tokenizer(path: &Path) -> Result<Tokenizer, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let mut json: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    if let Some(normalizer) = json.get_mut("normalizer") {
        mend_empty_precompiled(normalizer);
    }
    let bytes = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
    Tokenizer::from_bytes(bytes).map_err(|e| format!("cannot read {}: {}", path.display(), e))
}

/// SentencePiece's `nmt_nfkc` as tokenizers normalizers.
fn nmt_nfkc() -> serde_json::Value {
    serde_json::json!({"type": "Sequence", "normalizers": [
        {"type": "NFKC"},
        {"type": "Replace", "pattern": {"Regex": "[\\t\\n\\r\\x{0B}\\x{0C}]"}, "content": " "},
        {"type": "Replace",
         "pattern": {"Regex": "[\\x{00}-\\x{08}\\x{0E}-\\x{1F}\\x{7F}\\x{200B}-\\x{200D}\\x{FEFF}]"},
         "content": ""}
    ]})
}

/// A `Precompiled` normalizer with a `null` charsmap becomes [`nmt_nfkc`], at any depth of
/// `Sequence`s.
fn mend_empty_precompiled(normalizer: &mut serde_json::Value) {
    let empty = normalizer.get("type").and_then(|t| t.as_str()) == Some("Precompiled")
        && normalizer
            .get("precompiled_charsmap")
            .is_none_or(|c| c.is_null());
    if empty {
        *normalizer = nmt_nfkc();
    } else if let Some(list) = normalizer
        .get_mut("normalizers")
        .and_then(|l| l.as_array_mut())
    {
        for n in list.iter_mut() {
            mend_empty_precompiled(n);
        }
    }
}

/// Loads a decoder graph and its tokenizer.
pub fn load(
    opts: &LoadOptions,
    decoder_path: &Path,
    tokenizer_path: &Path,
    gen: GenConfig,
) -> Result<Decoder, String> {
    let (session, _) = build_session(decoder_path, opts.provider, opts.threads)?;
    let tokenizer = load_tokenizer(tokenizer_path)?;
    let mut ids = None;
    let mut encoder_states = None;
    let mut states_f16 = false;
    let mut mask = None;
    let mut encoder_mask = None;
    let mut cache_branch = None;
    let mut past = Vec::new();
    let mut cache_shape = None;
    let mut unknown = Vec::new();
    for input in session.inputs() {
        let name = input.name().to_string();
        match name.as_str() {
            "input_ids" => ids = Some(name),
            "encoder_hidden_states" => {
                states_f16 = input.dtype().tensor_type() == Some(TensorElementType::Float16);
                encoder_states = Some(name)
            }
            "attention_mask" | "decoder_attention_mask" => mask = Some(name),
            "encoder_attention_mask" => encoder_mask = Some(name),
            "use_cache_branch" => cache_branch = Some(name),
            n if n.starts_with("past_key_values.") => {
                if cache_shape.is_none() {
                    let f16 = input.dtype().tensor_type() == Some(TensorElementType::Float16);
                    if let Some([_, heads, _, dim]) = input.dtype().tensor_shape().map(|s| &s[..]) {
                        if *heads > 0 && *dim > 0 {
                            cache_shape = Some((*heads as usize, *dim as usize, f16));
                        }
                    }
                }
                past.push(name)
            }
            _ => unknown.push(name),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "the decoder needs inputs this server cannot feed: {}",
            unknown.join(", ")
        ));
    }
    let ids = ids.ok_or("the decoder has no input_ids input")?;
    let encoder_states = encoder_states.ok_or("the decoder has no encoder_hidden_states input")?;
    let outputs: Vec<String> = session
        .outputs()
        .iter()
        .map(|o| o.name().to_string())
        .collect();
    let logits = outputs
        .iter()
        .find(|n| n.as_str() == "logits")
        .cloned()
        .ok_or("the decoder has no logits output")?;
    // `past_key_values.3.decoder.key` answers as `present.3.decoder.key`.
    let mut cache = Vec::new();
    for p in &past {
        let present = p.replacen("past_key_values", "present", 1);
        if !outputs.contains(&present) {
            return Err(format!("the decoder has {} but no {} output", p, present));
        }
        cache.push((p.clone(), present));
    }
    let cache_shape = match (cache.is_empty(), cache_shape) {
        (true, _) => (0, 0, false),
        (false, Some(s)) => s,
        (false, None) => return Err("the decoder's cache inputs have no fixed head shape".into()),
    };
    Ok(Decoder {
        session: Mutex::new(session),
        tokenizer,
        path: decoder_path.to_path_buf(),
        gen,
        ids,
        encoder_states,
        states_f16,
        mask,
        encoder_mask,
        cache_branch,
        cache,
        cache_shape,
        logits,
    })
}

impl Decoder {
    /// Greedy decoding over the encoder states `states` (`[1, tokens, dim]`): the generated
    /// tokens, the start token and the end token left out.
    pub fn generate(
        &self,
        states: &[f32],
        state_shape: &[i64],
        max_new_tokens: Option<usize>,
        num_beams: Option<usize>,
    ) -> Result<Vec<u32>, ApiError> {
        let width = num_beams
            .unwrap_or(self.gen.num_beams)
            .clamp(1, generation::MAX_BEAMS);
        if width > 1 {
            return self.beam_search(states, state_shape, max_new_tokens, width);
        }
        let max_length = self.gen.length_of(max_new_tokens, 1);
        let err = |e: ort::Error| ApiError::server(format!("decoder: {}", e));
        let shape: Vec<usize> = state_shape.iter().map(|d| *d as usize).collect();
        let enc_len = *shape
            .get(1)
            .ok_or_else(|| ApiError::server("unexpected encoder output shape"))?;
        // The encoder states go in every step (copied: one image is ~150k values).
        let half: Vec<half::f16> = if self.states_f16 {
            states.iter().map(|v| half::f16::from_f32(*v)).collect()
        } else {
            Vec::new()
        };
        let states_value = || -> Result<DynValue, ApiError> {
            Ok(if self.states_f16 {
                Tensor::from_array((shape.clone(), half.clone()))
                    .map_err(err)?
                    .into_dyn()
            } else {
                Tensor::from_array((shape.clone(), states.to_vec()))
                    .map_err(err)?
                    .into_dyn()
            })
        };
        let mut tokens: Vec<i64> = vec![i64::from(self.gen.start_id)];
        // Self-attention cache of the last step, in `cache` order (`None` for cross-attention).
        let mut past: Vec<Option<DynValue>> = Vec::new();
        // Cross-attention cache of the first step, by `cache` index.
        let mut cross: Vec<Option<Stored>> = Vec::new();
        let mut session = self
            .session
            .lock()
            .map_err(|_| ApiError::server("the decoder session is poisoned"))?;
        while tokens.len() < max_length {
            let cached = !self.cache.is_empty() && !past.is_empty();
            // With the cache only the newest token goes in.
            let step: Vec<i64> = if cached {
                vec![*tokens.last().unwrap_or(&0)]
            } else {
                tokens.clone()
            };
            let mut feeds: Vec<(String, DynValue)> = Vec::new();
            feeds.push((
                self.ids.clone(),
                Tensor::from_array(([1usize, step.len()], step))
                    .map_err(err)?
                    .into_dyn(),
            ));
            feeds.push((self.encoder_states.clone(), states_value()?));
            if let Some(name) = &self.mask {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([1usize, tokens.len()], vec![1i64; tokens.len()]))
                        .map_err(err)?
                        .into_dyn(),
                ));
            }
            if let Some(name) = &self.encoder_mask {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([1usize, enc_len], vec![1i64; enc_len]))
                        .map_err(err)?
                        .into_dyn(),
                ));
            }
            if let Some(name) = &self.cache_branch {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([1usize], vec![cached]))
                        .map_err(err)?
                        .into_dyn(),
                ));
            }
            if cached {
                for (i, ((name, _), value)) in self.cache.iter().zip(past.drain(..)).enumerate() {
                    let value = match (value, &cross[i]) {
                        (_, Some(stored)) => stored.value().map_err(err)?,
                        (Some(v), None) => v,
                        (None, None) => return Err(ApiError::server("decoder cache lost")),
                    };
                    feeds.push((name.clone(), value));
                }
            } else if !self.cache.is_empty() {
                let (heads, dim, f16) = self.cache_shape;
                for (name, _) in &self.cache {
                    let empty: DynValue = if f16 {
                        Tensor::from_array(([1usize, heads, 0, dim], Vec::<half::f16>::new()))
                            .map_err(err)?
                            .into_dyn()
                    } else {
                        Tensor::from_array(([1usize, heads, 0, dim], Vec::<f32>::new()))
                            .map_err(err)?
                            .into_dyn()
                    };
                    feeds.push((name.clone(), empty));
                }
            }
            let mut outputs = session
                .run(feeds)
                .map_err(|e| ApiError::server(format!("decoder failed: {}", e)))?;
            let next = {
                let value = outputs
                    .get(&self.logits)
                    .ok_or_else(|| ApiError::server("decoder logits missing"))?;
                let mut row: Vec<f32> = match value.dtype().tensor_type() {
                    Some(TensorElementType::Float16) => {
                        let (s, d) = value.try_extract_tensor::<half::f16>().map_err(err)?;
                        last_row(s, d)?.iter().map(|v| v.to_f32()).collect()
                    }
                    _ => {
                        let (s, d) = value.try_extract_tensor::<f32>().map_err(err)?;
                        last_row(s, d)?.to_vec()
                    }
                };
                generation::next_token(&self.gen, &tokens, &mut row, max_length)
            };
            let first = cross.is_empty();
            for (name, present) in &self.cache {
                let value = outputs
                    .remove(present.as_str())
                    .ok_or_else(|| ApiError::server(format!("{} missing", present)))?;
                if name.contains(".encoder.") {
                    if first {
                        cross.push(Some(Stored::of(&value).map_err(err)?));
                    }
                    past.push(None);
                } else {
                    if first {
                        cross.push(None);
                    }
                    past.push(Some(value));
                }
            }
            if self.gen.eos_ids.contains(&next) {
                break;
            }
            tokens.push(i64::from(next));
        }
        Ok(tokens.iter().skip(1).map(|t| *t as u32).collect())
    }

    /// Beam search with `width` beams as one batch: every row gets the encoder states, the
    /// self-attention cache follows its beam from step to step, the cross-attention cache of
    /// the first step stays (its rows are the same for every beam).
    fn beam_search(
        &self,
        states: &[f32],
        state_shape: &[i64],
        max_new_tokens: Option<usize>,
        width: usize,
    ) -> Result<Vec<u32>, ApiError> {
        let max_length = self.gen.length_of(max_new_tokens, width);
        let err = |e: ort::Error| ApiError::server(format!("decoder: {}", e));
        let mut shape: Vec<usize> = state_shape.iter().map(|d| *d as usize).collect();
        let enc_len = *shape
            .get(1)
            .ok_or_else(|| ApiError::server("unexpected encoder output shape"))?;
        shape[0] = width;
        let mut repeated = Vec::with_capacity(states.len() * width);
        for _ in 0..width {
            repeated.extend_from_slice(states);
        }
        let half: Vec<half::f16> = if self.states_f16 {
            repeated.iter().map(|v| half::f16::from_f32(*v)).collect()
        } else {
            Vec::new()
        };
        let states_value = || -> Result<DynValue, ApiError> {
            Ok(if self.states_f16 {
                Tensor::from_array((shape.clone(), half.clone()))
                    .map_err(err)?
                    .into_dyn()
            } else {
                Tensor::from_array((shape.clone(), repeated.clone()))
                    .map_err(err)?
                    .into_dyn()
            })
        };
        let mut beams = generation::Beams::new(&self.gen, width);
        // Cache by `cache` index: self-attention of the last step, cross-attention of the first.
        let mut past: Vec<Option<Stored>> = Vec::new();
        let mut session = self
            .session
            .lock()
            .map_err(|_| ApiError::server("the decoder session is poisoned"))?;
        while beams.seqs[0].len() < max_length && !beams.is_done() {
            let cur = beams.seqs[0].len();
            let cached = !self.cache.is_empty() && !past.is_empty();
            let step_len = if cached { 1 } else { cur };
            let step: Vec<i64> = beams
                .seqs
                .iter()
                .flat_map(|s| s[s.len() - step_len..].iter().copied())
                .collect();
            let mut feeds: Vec<(String, DynValue)> = vec![
                (
                    self.ids.clone(),
                    Tensor::from_array(([width, step_len], step))
                        .map_err(err)?
                        .into_dyn(),
                ),
                (self.encoder_states.clone(), states_value()?),
            ];
            if let Some(name) = &self.mask {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([width, cur], vec![1i64; width * cur]))
                        .map_err(err)?
                        .into_dyn(),
                ));
            }
            if let Some(name) = &self.encoder_mask {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([width, enc_len], vec![1i64; width * enc_len]))
                        .map_err(err)?
                        .into_dyn(),
                ));
            }
            if let Some(name) = &self.cache_branch {
                feeds.push((
                    name.clone(),
                    Tensor::from_array(([1usize], vec![cached]))
                        .map_err(err)?
                        .into_dyn(),
                ));
            }
            if cached {
                for ((name, _), stored) in self.cache.iter().zip(&past) {
                    let stored = stored
                        .as_ref()
                        .ok_or_else(|| ApiError::server("decoder cache lost"))?;
                    feeds.push((name.clone(), stored.value().map_err(err)?));
                }
            } else if !self.cache.is_empty() {
                let (heads, dim, f16) = self.cache_shape;
                for (name, _) in &self.cache {
                    let empty: DynValue = if f16 {
                        Tensor::from_array(([width, heads, 0, dim], Vec::<half::f16>::new()))
                            .map_err(err)?
                            .into_dyn()
                    } else {
                        Tensor::from_array(([width, heads, 0, dim], Vec::<f32>::new()))
                            .map_err(err)?
                            .into_dyn()
                    };
                    feeds.push((name.clone(), empty));
                }
            }
            let outputs = session
                .run(feeds)
                .map_err(|e| ApiError::server(format!("decoder failed: {}", e)))?;
            let rows: Vec<Vec<f32>> = {
                let value = outputs
                    .get(&self.logits)
                    .ok_or_else(|| ApiError::server("decoder logits missing"))?;
                let (s, data): (Vec<i64>, Vec<f32>) = match value.dtype().tensor_type() {
                    Some(TensorElementType::Float16) => {
                        let (s, d) = value.try_extract_tensor::<half::f16>().map_err(err)?;
                        (s.to_vec(), d.iter().map(|v| v.to_f32()).collect())
                    }
                    _ => {
                        let (s, d) = value.try_extract_tensor::<f32>().map_err(err)?;
                        (s.to_vec(), d.to_vec())
                    }
                };
                let vocab = *s.last().unwrap_or(&0) as usize;
                if vocab == 0 || data.len() != width * step_len * vocab {
                    return Err(ApiError::server(format!(
                        "unexpected decoder logits shape {:?}",
                        s
                    )));
                }
                (0..width)
                    .map(|b| {
                        let at = (b * step_len + step_len - 1) * vocab;
                        let mut row = data[at..at + vocab].to_vec();
                        generation::log_softmax(&mut row);
                        generation::process(&self.gen, &beams.seqs[b], &mut row, max_length);
                        row
                    })
                    .collect()
            };
            let sources = beams.step(&rows, &self.gen.eos_ids);
            let first = past.is_empty();
            let mut next = Vec::with_capacity(self.cache.len());
            for (i, (name, present)) in self.cache.iter().enumerate() {
                if name.contains(".encoder.") && !first {
                    next.push(past[i].take());
                    continue;
                }
                let value = outputs
                    .get(present.as_str())
                    .ok_or_else(|| ApiError::server(format!("{} missing", present)))?;
                let stored = Stored::of(value).map_err(err)?;
                next.push(Some(if name.contains(".encoder.") {
                    stored
                } else {
                    stored.gather(&sources)
                }));
            }
            past = next;
        }
        drop(session);
        Ok(beams.best().into_iter().map(|t| t as u32).collect())
    }

    /// The text of generated tokens, special tokens left out.
    pub fn decode(&self, ids: &[u32]) -> Result<String, ApiError> {
        self.tokenizer
            .decode(ids, true)
            .map_err(|e| ApiError::server(format!("cannot decode the output: {}", e)))
    }

    pub fn describe(&self) -> String {
        format!(
            "decoder {} ({})",
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            if self.cache.is_empty() {
                "no KV cache".to_string()
            } else {
                format!("KV cache, {} tensors", self.cache.len())
            }
        )
    }
}

/// The logits of the last position of `[1, seq, vocab]`.
fn last_row<'a, T>(shape: &[i64], data: &'a [T]) -> Result<&'a [T], ApiError> {
    let vocab = *shape
        .last()
        .ok_or_else(|| ApiError::server("decoder logits have no shape"))? as usize;
    if vocab == 0 || data.len() < vocab {
        return Err(ApiError::server(format!(
            "unexpected decoder logits shape {:?}",
            shape
        )));
    }
    Ok(&data[data.len() - vocab..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_empty_precompiled_normalizer_becomes_nmt_nfkc() {
        let mut n = json!({"type": "Precompiled", "precompiled_charsmap": null});
        mend_empty_precompiled(&mut n);
        assert_eq!(n, nmt_nfkc());
        // Nested sequences are mended too; a real charsmap stays.
        let mut n = json!({"type": "Sequence", "normalizers": [
            {"type": "Sequence", "normalizers": [{"type": "Precompiled", "precompiled_charsmap": null}]},
            {"type": "Precompiled", "precompiled_charsmap": "AAAA"}]});
        mend_empty_precompiled(&mut n);
        assert_eq!(n["normalizers"][0]["normalizers"][0], nmt_nfkc());
        assert_eq!(n["normalizers"][1]["type"], "Precompiled");
    }

    #[test]
    fn a_marian_style_tokenizer_loads_and_folds_like_sentencepiece() {
        // Metaspace splits on spaces only: a line feed left in place would glue "a" and "b".
        let json = json!({
            "version": "1.0", "truncation": null, "padding": null, "added_tokens": [],
            "normalizer": {"type": "Precompiled", "precompiled_charsmap": null},
            "pre_tokenizer": {"type": "Metaspace", "replacement": "\u{2581}", "prepend_scheme": "always", "split": true},
            "post_processor": null, "decoder": null,
            "model": {"type": "WordLevel", "vocab": {"\u{2581}a": 0, "\u{2581}b": 1, "\u{2581}fi": 2, "<unk>": 3},
                      "unk_token": "<unk>"}
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokenizer.json");
        std::fs::write(&path, json.to_string()).unwrap();
        let t = load_tokenizer(&path).unwrap();
        let ids = |s: &str| t.encode(s, false).unwrap().get_ids().to_vec();
        assert_eq!(ids("a b"), vec![0, 1]);
        assert_eq!(ids("a\nb"), vec![0, 1]);
        assert_eq!(ids("a\tb"), vec![0, 1]);
        assert_eq!(ids("a\rb"), vec![0, 1]);
        assert_eq!(ids("a\u{200B}"), vec![0]);
        assert_eq!(ids("\u{FB01}"), vec![2], "the fi ligature folds to fi");
    }
}
