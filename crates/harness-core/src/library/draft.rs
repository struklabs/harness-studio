//! Draft model suggestions for speculative decoding.
//!
//! llama.cpp drafts efficiently only when target and draft share a vocabulary
//! (`common_speculative_are_compatible`): same tokenizer type, the same BOS/EOS handling,
//! vocabulary sizes within 128 tokens and identical token texts from id 5 on. We compare the
//! same facts from the GGUF headers: `tokenizer.ggml.model`, `tokenizer.ggml.pre`,
//! `add_bos_token` / `add_eos_token` and the ids they add, the token count, and a hash of the
//! token texts [5, 5 + 32768). Keys verified on real files (`tests/data/gguf-library-hints.json`):
//! the Qwen2/Qwen3 family shares `gpt2` + `qwen2` and the hash; gpt-oss (`gpt-4o`), Gemma 4
//! (`gemma4`) and Qwen3.6 (`qwen35`, 248k tokens) do not match it.
//!
//! A draft must also be much smaller: at most [`MAX_DRAFT_RATIO`] of the target's parameters.

use super::params::ParamInfo;
use crate::domain::ModelKind;
use crate::gguf::TokenizerInfo;
use crate::models::LocalModel;
use serde::{Deserialize, Serialize};

/// llama.cpp's `SPEC_VOCAB_MAX_SIZE_DIFFERENCE`.
pub const MAX_VOCAB_SIZE_DIFFERENCE: u64 = 128;
/// A draft has at most this share of the target's parameters.
pub const MAX_DRAFT_RATIO: f64 = 0.4;

/// Why two tokenizers do not match (for tests and tooltips).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mismatch {
    /// A header without tokenizer facts.
    Unknown,
    TokenizerType,
    PreTokenizer,
    SpecialTokens,
    VocabSize,
    TokenTexts,
}

/// Whether `draft` can draft for `target` token for token.
pub fn tokenizers_compatible(
    target: &TokenizerInfo,
    draft: &TokenizerInfo,
) -> Result<(), Mismatch> {
    if target.model.is_none() || draft.model.is_none() {
        return Err(Mismatch::Unknown);
    }
    if target.model != draft.model {
        return Err(Mismatch::TokenizerType);
    }
    if target.pre.is_some() && draft.pre.is_some() && target.pre != draft.pre {
        return Err(Mismatch::PreTokenizer);
    }
    // The special tokens only matter when the tokenizer adds them.
    let add_bos = |t: &TokenizerInfo| t.add_bos.unwrap_or(false);
    let add_eos = |t: &TokenizerInfo| t.add_eos.unwrap_or(false);
    if add_bos(target) != add_bos(draft)
        || add_eos(target) != add_eos(draft)
        || (add_bos(target) && target.bos_id != draft.bos_id)
        || (add_eos(target) && target.eos_id != draft.eos_id)
    {
        return Err(Mismatch::SpecialTokens);
    }
    match (target.vocab_size, draft.vocab_size) {
        (Some(a), Some(b)) if a.abs_diff(b) <= MAX_VOCAB_SIZE_DIFFERENCE => {}
        _ => return Err(Mismatch::VocabSize),
    }
    match (&target.head_hash, &draft.head_hash) {
        (Some(a), Some(b)) if a != b => Err(Mismatch::TokenTexts),
        _ => Ok(()),
    }
}

fn size_of(model: &LocalModel, params: &ParamInfo) -> f64 {
    params
        .count
        .map(|c| c as f64)
        .unwrap_or(model.size_bytes as f64)
}

/// Whether `draft` is a useful draft for `target`: both complete LLM files, compatible
/// tokenizers, draft at most [`MAX_DRAFT_RATIO`] of the target.
pub fn is_draft_candidate(
    target: &LocalModel,
    target_params: &ParamInfo,
    draft: &LocalModel,
    draft_params: &ParamInfo,
) -> bool {
    if target.id == draft.id
        || target.kind != ModelKind::Llm
        || draft.kind != ModelKind::Llm
        || !draft.is_ready
    {
        return false;
    }
    let (Some(t), Some(d)) = (
        target.gguf.as_ref().and_then(|g| g.tokenizer.as_ref()),
        draft.gguf.as_ref().and_then(|g| g.tokenizer.as_ref()),
    ) else {
        return false;
    };
    // Compare like with like: parameters when both are known, else file sizes.
    let (ts, ds) = if target_params.count.is_some() && draft_params.count.is_some() {
        (size_of(target, target_params), size_of(draft, draft_params))
    } else {
        (target.size_bytes as f64, draft.size_bytes as f64)
    };
    ds > 0.0 && ds <= ts * MAX_DRAFT_RATIO && tokenizers_compatible(t, d).is_ok()
}

/// For every model, the ids of the bigger local LLMs it can draft for.
pub fn draft_targets(models: &[LocalModel], params: &[ParamInfo]) -> Vec<Vec<String>> {
    models
        .iter()
        .zip(params)
        .map(|(draft, dp)| {
            models
                .iter()
                .zip(params)
                .filter(|(target, tp)| is_draft_candidate(target, tp, draft, dp))
                .map(|(target, _)| target.id.clone())
                .collect()
        })
        .collect()
}

/// A suggested draft for one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftSuggestion {
    pub model_id: String,
    pub path: std::path::PathBuf,
    pub name: String,
    pub quantization: String,
    pub size_bytes: u64,
    pub params: ParamInfo,
    /// Draft size / target size (parameters when known).
    pub ratio: f64,
}

/// Local drafts for the model at `target_path`, smallest first.
pub fn suggest_for(target_path: &std::path::Path, models: &[LocalModel]) -> Vec<DraftSuggestion> {
    let key = super::norm(target_path);
    let Some(target) = models.iter().find(|m| super::norm(&m.path) == key) else {
        return Vec::new();
    };
    let tp = super::params::param_info(target);
    let mut out: Vec<DraftSuggestion> = models
        .iter()
        .filter_map(|m| {
            let dp = super::params::param_info(m);
            is_draft_candidate(target, &tp, m, &dp).then(|| DraftSuggestion {
                ratio: size_of(m, &dp) / size_of(target, &tp).max(1.0),
                model_id: m.id.clone(),
                path: m.path.clone(),
                name: m.name.clone(),
                quantization: m.quantization.clone(),
                size_bytes: m.size_bytes,
                params: dp,
            })
        })
        .collect();
    out.sort_by_key(|s| s.size_bytes);
    out
}
