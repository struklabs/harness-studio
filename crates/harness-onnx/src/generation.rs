//! Generation settings of an encoder-decoder model (`generation_config.json`, falling back to
//! `config.json`) and the logits processing of greedy decoding, as transformers applies them:
//! the minimum length, banned n-gram repeats, bad words, forced first and last tokens. Pure code:
//! the decoder session lives in [`crate::decoder`].

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Hard cap on generated tokens, whatever the request or the model's config asks.
pub const MAX_NEW_TOKENS: usize = 256;

/// transformers' `GenerationConfig.max_length` default.
const DEFAULT_MAX_LENGTH: usize = 20;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GenConfig {
    /// The first decoder token (`decoder_start_token_id`).
    pub start_id: u32,
    /// Tokens that end the output.
    pub eos_ids: Vec<u32>,
    /// Decoder length, the start token included (`max_length`).
    pub max_length: usize,
    /// Shortest decoder length before an end token may come (`min_length`).
    pub min_length: usize,
    /// No n-gram of this size repeats (`no_repeat_ngram_size`; 0 is off).
    pub no_repeat_ngram: usize,
    /// Single tokens never generated (`bad_words_ids` of one token, the pad token of Marian).
    pub bad_ids: Vec<u32>,
    /// Token forced right after the start token (`forced_bos_token_id`, BART).
    pub forced_bos: Option<u32>,
    /// Token forced at the last position (`forced_eos_token_id`).
    pub forced_eos: Option<u32>,
}

/// The decoder graph beside an encoder graph: the same precision's merged decoder
/// (`encoder_model_quantized.onnx` takes `decoder_model_merged_quantized.onnx`), then its
/// plain decoder, then the unsuffixed ones; `None` for a file that is no `encoder_model*`.
pub fn decoder_beside(encoder: &Path) -> Option<PathBuf> {
    let stem = encoder.file_stem()?.to_string_lossy().to_string();
    let suffix = stem.strip_prefix("encoder_model")?;
    let dir = encoder.parent()?;
    [
        format!("decoder_model_merged{suffix}.onnx"),
        format!("decoder_model{suffix}.onnx"),
        "decoder_model_merged.onnx".to_string(),
        "decoder_model.onnx".to_string(),
    ]
    .into_iter()
    .map(|n| dir.join(n))
    .find(|p| p.is_file())
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

impl GenConfig {
    /// The settings of the repo at `root`; `None` without a start token. `overrides` (a
    /// `task_specific_params` entry) win over both files.
    pub fn read(root: &Path, overrides: Option<&Value>) -> Option<Self> {
        let config = read_json(&root.join("config.json")).unwrap_or(Value::Null);
        let generation = read_json(&root.join("generation_config.json")).unwrap_or(Value::Null);
        let lookup = |key: &str| {
            overrides
                .and_then(|o| o.get(key))
                .or_else(|| generation.get(key))
                .or_else(|| config.get(key))
                .or_else(|| config.get("decoder").and_then(|d| d.get(key)))
                .filter(|v| !v.is_null())
                .cloned()
        };
        let id = |v: &Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
        let size = |key: &str| lookup(key).and_then(|v| v.as_u64()).map(|n| n as usize);
        let start_id = lookup("decoder_start_token_id")
            .or_else(|| lookup("bos_token_id"))
            .as_ref()
            .and_then(id)?;
        let eos_ids = match lookup("eos_token_id") {
            Some(Value::Array(a)) => a.iter().filter_map(id).collect(),
            Some(v) => id(&v).into_iter().collect(),
            None => Vec::new(),
        };
        let bad_ids = lookup("bad_words_ids")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|w| match w.as_array().map(Vec::as_slice) {
                Some([one]) => id(one),
                _ => None,
            })
            .collect();
        Some(GenConfig {
            start_id,
            eos_ids,
            max_length: size("max_length").unwrap_or(DEFAULT_MAX_LENGTH).max(2),
            min_length: size("min_length").unwrap_or(0),
            no_repeat_ngram: size("no_repeat_ngram_size").unwrap_or(0),
            bad_ids,
            forced_bos: lookup("forced_bos_token_id").as_ref().and_then(id),
            forced_eos: lookup("forced_eos_token_id").as_ref().and_then(id),
        })
    }

    /// Decoder length of one request: the config's `max_length`, or the start token plus
    /// `max_new_tokens`, never more than [`MAX_NEW_TOKENS`] new tokens.
    pub fn length_of(&self, max_new_tokens: Option<usize>) -> usize {
        let new = max_new_tokens
            .unwrap_or(self.max_length.saturating_sub(1))
            .clamp(1, MAX_NEW_TOKENS);
        1 + new
    }
}

/// The next token after `tokens` (the start token first) from the last position's `logits`,
/// with `max_length` the decoder length of this request.
pub fn next_token(cfg: &GenConfig, tokens: &[i64], logits: &mut [f32], max_length: usize) -> u32 {
    let len = tokens.len();
    let force = |logits: &mut [f32], token: u32| {
        if (token as usize) < logits.len() {
            logits.fill(f32::NEG_INFINITY);
            logits[token as usize] = 0.0;
        }
    };
    if len == 1 {
        if let Some(bos) = cfg.forced_bos {
            force(logits, bos);
        }
    }
    if len + 1 == max_length {
        if let Some(eos) = cfg.forced_eos {
            force(logits, eos);
        }
    }
    let mut ban = |token: u32| {
        if let Some(v) = logits.get_mut(token as usize) {
            *v = f32::NEG_INFINITY;
        }
    };
    if len < cfg.min_length {
        for &eos in &cfg.eos_ids {
            ban(eos);
        }
    }
    for &bad in &cfg.bad_ids {
        ban(bad);
    }
    let n = cfg.no_repeat_ngram;
    if n > 0 && len >= n {
        // Every earlier n-gram whose first n-1 tokens equal the last n-1 tokens bans its end.
        let tail = &tokens[len + 1 - n..];
        for start in 0..=len - n {
            if tokens[start..start + n - 1] == *tail {
                ban(tokens[start + n - 1] as u32);
            }
        }
    }
    argmax(logits)
}

/// Index of the largest value (the first one on ties).
pub fn argmax(row: &[f32]) -> u32 {
    let mut best = 0;
    for (i, v) in row.iter().enumerate() {
        if *v > row[best] {
            best = i;
        }
    }
    best as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn greedy_picks_the_first_largest() {
        assert_eq!(argmax(&[0.1, 0.7, 0.7, -1.0]), 1);
        assert_eq!(argmax(&[3.0]), 0);
    }

    #[test]
    fn reads_bart_and_marian_settings() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.json"),
            json!({"max_length": 999}).to_string(),
        )
        .unwrap();
        fs::write(
            dir.path().join("generation_config.json"),
            json!({"decoder_start_token_id": 2, "eos_token_id": 2, "forced_bos_token_id": 0,
                   "forced_eos_token_id": 2, "max_length": 142, "min_length": 56,
                   "no_repeat_ngram_size": 3, "bad_words_ids": [[58100], [1, 2]]})
            .to_string(),
        )
        .unwrap();
        let g = GenConfig::read(dir.path(), None).unwrap();
        assert_eq!(
            g,
            GenConfig {
                start_id: 2,
                eos_ids: vec![2],
                max_length: 142,
                min_length: 56,
                no_repeat_ngram: 3,
                bad_ids: vec![58100],
                forced_bos: Some(0),
                forced_eos: Some(2),
            }
        );
        // A task's own settings win.
        let t = GenConfig::read(dir.path(), Some(&json!({"max_length": 40}))).unwrap();
        assert_eq!(t.max_length, 40);
        // 141 new tokens by default; the request may ask for fewer, never above the cap.
        assert_eq!(g.length_of(None), 142);
        assert_eq!(g.length_of(Some(5)), 6);
        assert_eq!(g.length_of(Some(100_000)), 1 + MAX_NEW_TOKENS);
    }

    #[test]
    fn decoders_pair_with_their_encoder() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let touch = |n: &str| fs::write(d.join(n), b"onnx").unwrap();
        touch("encoder_model_quantized.onnx");
        touch("encoder_model_fp16.onnx");
        // Only an unsuffixed, unmerged decoder: every encoder takes it.
        touch("decoder_model.onnx");
        assert_eq!(
            decoder_beside(&d.join("encoder_model_fp16.onnx")),
            Some(d.join("decoder_model.onnx"))
        );
        touch("decoder_model_merged.onnx");
        touch("decoder_model_merged_quantized.onnx");
        assert_eq!(
            decoder_beside(&d.join("encoder_model_quantized.onnx")),
            Some(d.join("decoder_model_merged_quantized.onnx"))
        );
        assert_eq!(
            decoder_beside(&d.join("encoder_model_fp16.onnx")),
            Some(d.join("decoder_model_merged.onnx"))
        );
        assert_eq!(decoder_beside(&d.join("decoder_model.onnx")), None);
        assert!(decoder_beside(&d.join("encoder_model_int8.onnx")).is_some());
    }

    #[test]
    fn start_end_and_length_come_from_any_config() {
        let dir = tempfile::tempdir().unwrap();
        let write = |n: &str, v: Value| fs::write(dir.path().join(n), v.to_string()).unwrap();
        // No decoder_start_token_id: the BOS token starts; an EOS list; nested decoder config.
        write(
            "config.json",
            json!({"decoder": {"bos_token_id": 7, "eos_token_id": [3, 4], "max_length": 33}}),
        );
        let g = GenConfig::read(dir.path(), None).unwrap();
        assert_eq!(
            (g.start_id, g.eos_ids.clone(), g.max_length),
            (7, vec![3, 4], 33)
        );
        // A top-level config value wins over the nested one, the generation config over both.
        write(
            "config.json",
            json!({"bos_token_id": 8, "max_length": 12, "decoder": {"bos_token_id": 7}}),
        );
        assert_eq!(GenConfig::read(dir.path(), None).unwrap().start_id, 8);
        write(
            "generation_config.json",
            json!({"decoder_start_token_id": 9}),
        );
        let g = GenConfig::read(dir.path(), None).unwrap();
        assert_eq!((g.start_id, g.max_length), (9, 12));
        // No start token anywhere: not a generator.
        write("config.json", json!({}));
        write("generation_config.json", json!({}));
        assert_eq!(GenConfig::read(dir.path(), None), None);
        // No max_length: transformers' 20.
        write(
            "generation_config.json",
            json!({"decoder_start_token_id": 0}),
        );
        assert_eq!(
            GenConfig::read(dir.path(), None).unwrap().length_of(None),
            20
        );
    }

    #[test]
    fn logits_rules_follow_transformers() {
        let cfg = GenConfig {
            start_id: 2,
            eos_ids: vec![2],
            max_length: 10,
            min_length: 3,
            no_repeat_ngram: 2,
            bad_ids: vec![4],
            forced_bos: Some(0),
            forced_eos: Some(2),
        };
        let row = || vec![0.0, 0.1, 9.0, 0.2, 8.0, 0.3];
        // Right after the start token the forced BOS comes.
        assert_eq!(next_token(&cfg, &[2], &mut row(), 10), 0);
        // EOS is held back below min_length, the bad token never comes.
        assert_eq!(next_token(&cfg, &[2, 0], &mut row(), 10), 5);
        assert_eq!(next_token(&cfg, &[2, 0, 5], &mut row(), 10), 2);
        // `5 3` was seen once: after 5 the 3 is banned.
        let mut logits = vec![0.0, 0.0, -9.0, 5.0, 0.0, 1.0];
        assert_eq!(next_token(&cfg, &[2, 0, 5, 3, 1, 5], &mut logits, 10), 5);
        // The last position is the forced EOS.
        assert_eq!(
            next_token(&cfg, &[2, 0, 5, 1, 3, 1, 0, 3, 5], &mut row(), 10),
            2
        );
    }
}
