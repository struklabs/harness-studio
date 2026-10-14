//! Generation settings of an encoder-decoder model (`generation_config.json`, falling back to
//! `config.json`) and the logits processing of greedy decoding, as transformers applies them:
//! the minimum length, banned n-gram repeats, bad words, forced first and last tokens. Pure code:
//! the decoder session lives in [`crate::decoder`].

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Hard cap on generated tokens, whatever the request or the model's config asks.
pub const MAX_NEW_TOKENS: usize = 256;
/// Hard cap on beams.
pub const MAX_BEAMS: usize = 8;
/// Cap on beams × new tokens: decoder rows one request may run (4 beams get 256 tokens, 8 get
/// 128).
pub const MAX_BEAM_TOKENS: usize = 1024;

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
    /// Beam search width (`num_beams`; 1 is greedy), at most [`MAX_BEAMS`].
    pub num_beams: usize,
    /// Finished beams are ranked by `log p / length ^ length_penalty`.
    pub length_penalty: f32,
    /// Stop as soon as `num_beams` beams have finished (`early_stopping: true`).
    pub early_stopping: bool,
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
            num_beams: size("num_beams").unwrap_or(1).clamp(1, MAX_BEAMS),
            length_penalty: lookup("length_penalty")
                .and_then(|v| v.as_f64())
                .map_or(1.0, |v| v as f32),
            early_stopping: lookup("early_stopping").and_then(|v| v.as_bool()) == Some(true),
        })
    }

    /// Decoder length of one request with `beams` beams: the config's `max_length`, or the
    /// start token plus `max_new_tokens`, never more than [`MAX_NEW_TOKENS`] new tokens nor
    /// more than [`MAX_BEAM_TOKENS`] over all beams.
    pub fn length_of(&self, max_new_tokens: Option<usize>, beams: usize) -> usize {
        let cap = MAX_NEW_TOKENS.min(MAX_BEAM_TOKENS / beams.max(1));
        let new = max_new_tokens
            .unwrap_or(self.max_length.saturating_sub(1))
            .clamp(1, cap);
        1 + new
    }
}

/// The next token after `tokens` (the start token first) from the last position's `logits`,
/// with `max_length` the decoder length of this request.
pub fn next_token(cfg: &GenConfig, tokens: &[i64], logits: &mut [f32], max_length: usize) -> u32 {
    process(cfg, tokens, logits, max_length);
    argmax(logits)
}

/// The logits rules of the config applied to the scores of the token after `tokens`:
/// forced BOS / EOS, the minimum length, bad words, banned n-gram repeats.
pub fn process(cfg: &GenConfig, tokens: &[i64], logits: &mut [f32], max_length: usize) {
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
}

/// `log_softmax` of a row, in place.
pub fn log_softmax(row: &mut [f32]) {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        return;
    }
    let sum: f32 = row.iter().map(|v| (v - max).exp()).sum();
    let log_sum = max + sum.ln();
    for v in row.iter_mut() {
        *v -= log_sum;
    }
}

/// Beam search bookkeeping as transformers' `BeamSearchScorer` does it for one input: the live
/// beams, their summed log-probabilities and the finished hypotheses.
#[derive(Debug, Clone)]
pub struct Beams {
    width: usize,
    length_penalty: f32,
    early_stopping: bool,
    /// Live beams (the start token first) and their scores.
    pub seqs: Vec<Vec<i64>>,
    pub scores: Vec<f32>,
    /// Finished hypotheses: `(score, tokens without the end token)`, best first.
    finished: Vec<(f32, Vec<i64>)>,
    done: bool,
}

impl Beams {
    /// `width` beams holding the start token; only the first one counts at the start (the
    /// others are `-inf`, so the first step does not pick the same token `width` times).
    pub fn new(cfg: &GenConfig, width: usize) -> Self {
        let mut scores = vec![f32::NEG_INFINITY; width];
        scores[0] = 0.0;
        Beams {
            width,
            length_penalty: cfg.length_penalty,
            early_stopping: cfg.early_stopping,
            seqs: vec![vec![i64::from(cfg.start_id)]; width],
            scores,
            finished: Vec::new(),
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    fn rank(&self, sum_logprobs: f32, generated: usize) -> f32 {
        sum_logprobs / (generated.max(1) as f32).powf(self.length_penalty)
    }

    fn add(&mut self, score: f32, seq: Vec<i64>) {
        if self.finished.len() < self.width || score > self.worst() {
            self.finished.push((score, seq));
            self.finished
                .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            self.finished.truncate(self.width);
        }
    }

    fn worst(&self) -> f32 {
        self.finished.last().map_or(f32::NEG_INFINITY, |h| h.0)
    }

    /// One step: `logprobs` holds a processed `log_softmax` row per live beam. Returns, for
    /// each new beam, the old beam it continues (the cache rows to keep, in order).
    pub fn step(&mut self, logprobs: &[Vec<f32>], eos: &[u32]) -> Vec<usize> {
        let k = 2 * self.width;
        // Each beam's best 2k tokens are enough to find the best 2k over all beams.
        let mut candidates: Vec<(f32, usize, u32)> = Vec::new();
        for (b, row) in logprobs.iter().enumerate() {
            let base = self.scores[b];
            if !base.is_finite() {
                continue;
            }
            let mut idx: Vec<u32> = (0..row.len() as u32).collect();
            let top = k.min(idx.len());
            if top < idx.len() {
                idx.select_nth_unstable_by(top - 1, |&a, &c| {
                    row[c as usize]
                        .partial_cmp(&row[a as usize])
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                idx.truncate(top);
            }
            for t in idx {
                let s = base + row[t as usize];
                if s.is_finite() {
                    candidates.push((s, b, t));
                }
            }
        }
        candidates.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        candidates.truncate(k);
        let cur = self.seqs[0].len();
        let mut next_seqs = Vec::with_capacity(self.width);
        let mut next_scores = Vec::with_capacity(self.width);
        let mut sources = Vec::with_capacity(self.width);
        for (rank, (score, b, t)) in candidates.iter().enumerate() {
            if eos.contains(t) {
                // An end among the first `width` candidates finishes a hypothesis.
                if rank < self.width {
                    let seq = self.seqs[*b].clone();
                    let ranked = self.rank(*score, cur - 1);
                    self.add(ranked, seq);
                }
                continue;
            }
            let mut seq = self.seqs[*b].clone();
            seq.push(i64::from(*t));
            next_seqs.push(seq);
            next_scores.push(*score);
            sources.push(*b);
            if next_seqs.len() == self.width {
                break;
            }
        }
        // Fewer than `width` continuations (a tiny vocabulary): pad with dead beams.
        while next_seqs.len() < self.width {
            next_seqs.push(self.seqs[0].clone());
            next_scores.push(f32::NEG_INFINITY);
            sources.push(0);
        }
        let best_live = next_scores
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        self.seqs = next_seqs;
        self.scores = next_scores;
        if self.finished.len() >= self.width {
            self.done = self.early_stopping
                // No live beam can still beat the worst kept hypothesis.
                || self.worst() >= self.rank(best_live, cur);
        }
        if !best_live.is_finite() {
            self.done = true;
        }
        sources
    }

    /// The best hypothesis without its start token; live beams count as finished at the end.
    pub fn best(mut self) -> Vec<i64> {
        if !self.done {
            for (seq, score) in std::mem::take(&mut self.seqs)
                .into_iter()
                .zip(self.scores.clone())
            {
                if score.is_finite() {
                    let ranked = self.rank(score, seq.len() - 1);
                    self.add(ranked, seq);
                }
            }
        }
        self.finished
            .into_iter()
            .next()
            .map(|(_, s)| s.into_iter().skip(1).collect())
            .unwrap_or_default()
    }
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
                num_beams: 1,
                length_penalty: 1.0,
                early_stopping: false,
            }
        );
        // A task's own settings win.
        let t = GenConfig::read(dir.path(), Some(&json!({"max_length": 40}))).unwrap();
        assert_eq!(t.max_length, 40);
        // 141 new tokens by default; the request may ask for fewer, never above the cap.
        assert_eq!(g.length_of(None, 1), 142);
        assert_eq!(g.length_of(Some(5), 1), 6);
        assert_eq!(g.length_of(Some(100_000), 1), 1 + MAX_NEW_TOKENS);
        // Beams share the budget: 4 keep 256 new tokens, 8 get 128.
        assert_eq!(g.length_of(Some(100_000), 4), 1 + 256);
        assert_eq!(g.length_of(Some(100_000), 8), 1 + 128);
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
            GenConfig::read(dir.path(), None)
                .unwrap()
                .length_of(None, 1),
            20
        );
    }

    #[test]
    fn beams_find_what_greedy_misses() {
        // Greedy takes 1 (0.6) and ends worse; 2 (0.4) then the end (0.9) is the best sum.
        let cfg = GenConfig {
            start_id: 3,
            eos_ids: vec![0],
            length_penalty: 0.0,
            ..GenConfig::default()
        };
        let ln = |p: f32| p.ln();
        let mut beams = Beams::new(&cfg, 2);
        let first = vec![ln(1e-6), ln(0.6), ln(0.4), ln(1e-6)];
        let from = beams.step(&[first.clone(), first], &[0]);
        assert_eq!(from, vec![0, 0], "both beams grow from the first one");
        assert_eq!(beams.seqs, vec![vec![3, 1], vec![3, 2]]);
        let after_1 = vec![ln(0.3), ln(0.35), ln(0.35), ln(1e-6)];
        let after_2 = vec![ln(0.9), ln(0.05), ln(0.05), ln(1e-6)];
        let from = beams.step(&[after_1, after_2], &[0]);
        assert_eq!(from, vec![0, 0]);
        assert!(!beams.is_done());
        assert_eq!(beams.best(), vec![2]);
    }

    #[test]
    fn log_softmax_normalizes() {
        let mut row = vec![1.0, 2.0, 3.0];
        log_softmax(&mut row);
        let total: f32 = row.iter().map(|v| v.exp()).sum();
        assert!((total - 1.0).abs() < 1e-5);
        assert!(row[2] > row[1] && row[1] > row[0]);
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
            ..GenConfig::default()
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
