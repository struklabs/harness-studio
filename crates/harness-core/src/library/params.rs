//! Parameter count of a local model.
//!
//! Exact when the GGUF tensor table of every part could be read (sum of tensor element
//! counts: this is what llama.cpp reports as `n_params`). Otherwise estimated from
//! `general.size_label`, then from the architecture (`block_count`, `embedding_length`,
//! vocabulary), then from the file size and the quantization's bits per weight.
//! Verified on real headers (`tests/data/gguf-library-hints.json`).

use crate::gguf::GgufMetadata;
use crate::models::LocalModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ParamInfo {
    /// Number of parameters (weights).
    pub count: Option<u64>,
    /// `general.size_label` as written by the converter ("4B", "32x2.4B", "large").
    pub label: Option<String>,
    /// `count` is an estimate (shown as "≈").
    pub estimated: bool,
}

pub fn param_info(model: &LocalModel) -> ParamInfo {
    let meta = model.gguf.as_ref();
    let label = meta.and_then(|m| m.size_label.clone());
    if let Some(count) = exact_count(model) {
        return ParamInfo {
            count: Some(count),
            label,
            estimated: false,
        };
    }
    let count = meta
        .and_then(|m| m.size_label.as_deref())
        .and_then(parse_size_label)
        .or_else(|| meta.and_then(estimate_from_shape))
        .or_else(|| estimate_from_size(model.size_bytes, &model.quantization));
    ParamInfo {
        count,
        label,
        estimated: count.is_some(),
    }
}

/// Sum over all parts; `None` if a part is missing or its tensor table is unreadable.
fn exact_count(model: &LocalModel) -> Option<u64> {
    if !model.is_split {
        return model.gguf.as_ref()?.param_count;
    }
    if !model.is_ready {
        return None;
    }
    let mut total: u64 = 0;
    for (i, part) in model.shard_paths.iter().enumerate() {
        let count = if i == 0 && part == &model.path {
            model.gguf.as_ref()?.param_count?
        } else {
            crate::models::cached_metadata(part)?.param_count?
        };
        total = total.checked_add(count)?;
    }
    Some(total)
}

/// "4B" -> 4e9, "0.6B" -> 6e8, "567M" -> 5.67e8, "300M", "1.5T". Mixture labels ("32x2.4B")
/// and words ("large") are not counts.
pub fn parse_size_label(label: &str) -> Option<u64> {
    let l = label.trim();
    let (num, unit) = l.split_at(l.len().checked_sub(1)?);
    let mult = match unit.to_ascii_uppercase().as_str() {
        "K" => 1e3,
        "M" => 1e6,
        "B" => 1e9,
        "T" => 1e12,
        _ => return None,
    };
    let n: f64 = num.parse().ok()?;
    (n > 0.0).then(|| (n * mult).round() as u64)
}

/// Dense transformer approximation: per block 4·d² (attention) + 3·d·ff (gated FFN, ff ≈
/// 8/3·d → 8·d²), plus the token embedding (vocab·d; many models tie the output to it).
pub fn estimate_from_shape(m: &GgufMetadata) -> Option<u64> {
    let layers = m.block_count? as f64;
    let d = m.embedding_length? as f64;
    let vocab = m
        .tokenizer
        .as_ref()
        .and_then(|t| t.vocab_size)
        .unwrap_or(32_000) as f64;
    let experts = m.expert_count.unwrap_or(1).max(1) as f64;
    let per_block = 4.0 * d * d + 8.0 * d * d * experts;
    Some((layers * per_block + vocab * d).round() as u64)
}

/// Last resort: bytes · 8 / bits per weight of the quantization.
pub fn estimate_from_size(size_bytes: u64, quant: &str) -> Option<u64> {
    let bpw = match quant.to_ascii_uppercase().as_str() {
        "F32" => 32.0,
        "F16" | "BF16" | "FP16" => 16.0,
        "Q8_0" => 8.5,
        "Q6_K" | "Q6_K_L" | "Q6_K_XL" => 6.56,
        "Q5_K_M" | "Q5_K_S" | "Q5_K_L" | "Q5_K_XL" | "Q5_0" | "Q5_1" => 5.6,
        "Q4_K_M" | "Q4_K_S" | "Q4_K_L" | "Q4_0" | "Q4_0_4_4" | "Q4_0_4_8" | "Q4_0_8_8" | "Q4_1"
        | "IQ4_NL" | "IQ4_XS" | "Q4_K_XL" => 4.8,
        "MXFP4_MOE" | "MXFP4" => 4.25,
        "Q3_K_M" | "Q3_K_S" | "Q3_K_L" | "Q3_K_XL" | "IQ3_M" | "IQ3_S" | "IQ3_XS" | "IQ3_XXS" => {
            3.9
        }
        "Q2_K" | "Q2_K_S" | "Q2_K_L" | "Q2_K_XL" | "IQ2_M" | "IQ2_S" | "IQ2_XS" | "IQ2_XXS" => 2.9,
        _ => return None,
    };
    (size_bytes > 0).then(|| (size_bytes as f64 * 8.0 / bpw).round() as u64)
}
