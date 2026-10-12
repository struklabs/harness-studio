//! Which ggml tensor types a runtime can load, and what to suggest when a model uses one the
//! chosen runtime can't (e.g. PrismML's Bonsai 2 files: `PQ2_0` / `PTQ1_0`, which only
//! PrismML's llama.cpp fork knows).
//!
//! Sources of truth:
//! * The type table is `enum ggml_type` of `ggml/include/ggml.h`: upstream ggml-org/llama.cpp
//!   (`GGML_TYPE_COUNT = 43`) and the PrismML fork (`PQ2_0 = 142`, `PTQ1_0 = 143`,
//!   `GGML_TYPE_COUNT = 144`).
//! * The official build refuses ids outside its table at load (b11236: `tensor 'output.weight'
//!   has invalid ggml type 142. should be in [0, 43)`).
//! * What a specific build can quantize to (and so load) is read from the `llama-quantize
//!   --help` next to its `llama-server` (`  141  or  PQ2_0   :  2.13 bpw ...`): the fork lists
//!   `PQ2_0` / `PTQ1_0`, b11236 and b11026 don't.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Upstream `GGML_TYPE_COUNT`: the official build refuses ids at or above it.
pub const UPSTREAM_TYPE_COUNT: u32 = 43;
/// Synthetic id used in advice for type 42 files whose byte layout proves they are the
/// deprecated Prism group-128 format. It is not written to GGUF files.
pub const LEGACY_PRISM_Q2_0: u32 = u32::MAX;

/// `enum ggml_type` of upstream ggml (removed ids 4, 5, 31-33 and 36-38 left out).
const UPSTREAM_TYPES: &[(u32, &str)] = &[
    (0, "F32"),
    (1, "F16"),
    (2, "Q4_0"),
    (3, "Q4_1"),
    (6, "Q5_0"),
    (7, "Q5_1"),
    (8, "Q8_0"),
    (9, "Q8_1"),
    (10, "Q2_K"),
    (11, "Q3_K"),
    (12, "Q4_K"),
    (13, "Q5_K"),
    (14, "Q6_K"),
    (15, "Q8_K"),
    (16, "IQ2_XXS"),
    (17, "IQ2_XS"),
    (18, "IQ3_XXS"),
    (19, "IQ1_S"),
    (20, "IQ4_NL"),
    (21, "IQ3_S"),
    (22, "IQ2_S"),
    (23, "IQ4_XS"),
    (24, "I8"),
    (25, "I16"),
    (26, "I32"),
    (27, "I64"),
    (28, "F64"),
    (29, "IQ1_M"),
    (30, "BF16"),
    (34, "TQ1_0"),
    (35, "TQ2_0"),
    (39, "MXFP4"),
    (40, "NVFP4"),
    (41, "Q1_0"),
    (42, "Q2_0"),
];

/// Upstream types newer than the ones every build of the last year loads. A build supports
/// them when its `llama-quantize` lists the name (checked: b11026, b11236 and the PrismML
/// fork b10709 list `Q1_0` and `Q2_0`).
const RECENT_UPSTREAM: &[&str] = &["Q1_0", "Q2_0"];

/// A fork that adds tensor types of its own.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ForkHint {
    /// Stable id (`prismml`); also the suggested runtime id.
    pub id: String,
    pub name: String,
    /// GitHub `owner/repo` that publishes the release archives.
    pub repo: String,
    pub url: String,
}

/// Types only a fork knows: `(ggml id, name, fork id)`.
const FORK_TYPES: &[(u32, &str, &str)] = &[(142, "PQ2_0", "prismml"), (143, "PTQ1_0", "prismml")];

pub fn fork_hint(id: &str) -> Option<ForkHint> {
    match id {
        "prismml" => Some(ForkHint {
            id: "prismml".into(),
            name: "PrismML llama.cpp (Bonsai)".into(),
            repo: "PrismML-Eng/llama.cpp".into(),
            url: "https://github.com/PrismML-Eng/llama.cpp".into(),
        }),
        "prismml-legacy" => Some(ForkHint {
            id: "prismml-legacy".into(),
            name: "PrismML llama.cpp prism-b9601 (legacy Q2_0)".into(),
            repo: "PrismML-Eng/llama.cpp · prism-b9601".into(),
            url: "https://github.com/PrismML-Eng/llama.cpp/releases/tag/prism-b9601-68faa14".into(),
        }),
        _ => None,
    }
}

/// Name of a ggml type id (`None` = unknown to both tables).
pub fn type_name(id: u32) -> Option<&'static str> {
    if id == LEGACY_PRISM_Q2_0 {
        return Some("PRISM_Q2_0_LEGACY");
    }
    UPSTREAM_TYPES
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
        .or_else(|| {
            FORK_TYPES
                .iter()
                .find(|(i, _, _)| *i == id)
                .map(|(_, n, _)| *n)
        })
}

/// A tensor type every llama.cpp build of the last year loads (nothing to check).
fn is_baseline(id: u32) -> bool {
    type_name(id).is_some_and(|n| !RECENT_UPSTREAM.contains(&n)) && id < UPSTREAM_TYPE_COUNT
}

/// Quant type names of `llama-quantize --help` (`  40  or  Q1_0 : ...`).
pub fn parse_quantize_help(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(num), Some(or), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if or != "or" || num.parse::<u32>().is_err() {
            continue;
        }
        let name = name.trim_end_matches(':').to_string();
        if !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !out.contains(&name)
        {
            out.push(name);
        }
    }
    out
}

/// `llama-quantize` next to a `llama-server` binary.
pub fn quantize_binary_for(server: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "llama-quantize.exe"
    } else {
        "llama-quantize"
    };
    let p = server.parent()?.join(name);
    p.is_file().then_some(p)
}

/// Quant names the build of `server` knows (from its `llama-quantize --help`; `None` when
/// there is no `llama-quantize` or it didn't run). Cached by path + mtime. Blocking.
pub fn detect_quant_types(server: &Path) -> Option<Vec<String>> {
    type Key = (PathBuf, Option<std::time::SystemTime>);
    static CACHE: Mutex<Option<HashMap<Key, Option<Vec<String>>>>> = Mutex::new(None);
    let quantize = quantize_binary_for(server)?;
    let key = (
        quantize.clone(),
        std::fs::metadata(&quantize)
            .ok()
            .and_then(|m| m.modified().ok()),
    );
    if let Some(hit) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key)) {
        return hit.clone();
    }
    let mut cmd = crate::process::quiet_command(&quantize);
    cmd.arg("--help")
        .current_dir(quantize.parent().unwrap_or(Path::new(".")));
    let found = crate::runtime::run_command_with_timeout(
        &mut cmd,
        crate::runtime::PROBE_TIMEOUT,
        crate::runtime::PROBE_MAX_BYTES,
    )
    .ok()
    .map(|out| {
        parse_quantize_help(&format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    })
    .filter(|v| !v.is_empty());
    CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, found.clone());
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Support {
    Supported,
    Unsupported,
    /// Nothing reliable is known (no `llama-quantize` to ask, or a type no table knows).
    Unknown,
}

/// What a runtime knows about its tensor types.
#[derive(Debug, Clone, Default)]
pub struct RuntimeQuantInfo {
    /// `llama-quantize --help` names (`None` = not detected).
    pub quant_types: Option<Vec<String>>,
    /// The official llama.cpp runtime (refuses ids >= [`UPSTREAM_TYPE_COUNT`]).
    pub official: bool,
    /// The frozen Prism v5 line (final release `prism-b9601`) that reads the deprecated
    /// group-128 layout stored under type id 42.
    pub legacy_prism_q2_0: bool,
}

/// Whether a runtime loads a tensor type.
pub fn support_of(rt: &RuntimeQuantInfo, id: u32) -> Support {
    if id == LEGACY_PRISM_Q2_0 {
        return if rt.legacy_prism_q2_0 {
            Support::Supported
        } else {
            Support::Unsupported
        };
    }
    if is_baseline(id) {
        return Support::Supported;
    }
    let name = type_name(id);
    if let (Some(name), Some(list)) = (name, rt.quant_types.as_ref()) {
        if list.iter().any(|q| q.eq_ignore_ascii_case(name)) {
            return Support::Supported;
        }
        // Only names some build is known to list are proof of absence.
        if RECENT_UPSTREAM.contains(&name) || FORK_TYPES.iter().any(|(_, n, _)| *n == name) {
            return Support::Unsupported;
        }
    }
    if rt.official && id >= UPSTREAM_TYPE_COUNT {
        return Support::Unsupported;
    }
    Support::Unknown
}

/// One tensor type of a model file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TensorType {
    pub id: u32,
    /// `None` = no table knows the id.
    pub name: Option<String>,
}

impl TensorType {
    pub fn of(id: u32) -> Self {
        Self {
            id,
            name: type_name(id).map(str::to_string),
        }
    }

    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => format!("{} ({})", n, self.id),
            None => format!("#{}", self.id),
        }
    }
}

/// Aggregate support of a runtime for every non-baseline type of a model.
pub fn model_support(rt: &RuntimeQuantInfo, types: &[u32]) -> (Support, Vec<TensorType>) {
    let mut missing = Vec::new();
    let mut unknown = false;
    for &id in types {
        match support_of(rt, id) {
            Support::Supported => {}
            Support::Unsupported => missing.push(TensorType::of(id)),
            Support::Unknown => unknown = true,
        }
    }
    let support = if !missing.is_empty() {
        Support::Unsupported
    } else if unknown {
        Support::Unknown
    } else {
        Support::Supported
    };
    (support, missing)
}

/// Types of a model that aren't baseline (the ones worth showing).
pub fn special_types(types: &[u32]) -> Vec<TensorType> {
    types
        .iter()
        .copied()
        .filter(|&id| !is_baseline(id))
        .map(TensorType::of)
        .collect()
}

/// The fork known to add one of these types.
pub fn fork_for(types: &[u32]) -> Option<ForkHint> {
    if types.contains(&LEGACY_PRISM_Q2_0) {
        return fork_hint("prismml-legacy");
    }
    types.iter().find_map(|&id| {
        FORK_TYPES
            .iter()
            .find(|(i, _, _)| *i == id)
            .and_then(|(_, _, fork)| fork_hint(fork))
    })
}

/// Tensor types used for runtime advice, including the layout-only legacy Prism marker.
pub fn model_types(types: &[u32], legacy_prism_q2_0: bool) -> Vec<u32> {
    let mut out = types.to_vec();
    if legacy_prism_q2_0 {
        out.retain(|id| *id != 42);
        out.push(LEGACY_PRISM_Q2_0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `llama-quantize --help` excerpt of PrismML b10709 (win-vulkan-x64).
    const FORK_HELP: &str = "usage: llama-quantize [--help] ...\n\nAllowed quantization types:\n   40  or  Q1_0    :  1.125 bpw quantization\n   41  or  Q2_0    :  2.125 bpw quantization\n  141  or  PQ2_0   :  2.13 bpw quantization (group 128, Prism)\n  143  or  PTQ1_0  :  1.75 bpw ternarization (group 128, Prism)\n   2  or  Q4_0   :  4.34G, +0.4685 ppl @ Llama-3-8B\n  15  or  Q4_K_M  :  4.58G, +0.1754 ppl @ Llama-3-8B\n   0  or  F32    : 26.00G              @ 7B\n          COPY    : only copy tensors, no quantizing\n";

    #[test]
    fn parses_quantize_help() {
        let names = parse_quantize_help(FORK_HELP);
        assert_eq!(
            names,
            ["Q1_0", "Q2_0", "PQ2_0", "PTQ1_0", "Q4_0", "Q4_K_M", "F32"]
        );
    }

    #[test]
    fn prism_types_need_the_fork() {
        // Tensor types read from the real Ternary-Bonsai-2-27B-PQ2_0.gguf: F32, BF16, PQ2_0.
        let types = [0, 30, 142];
        let official = RuntimeQuantInfo {
            quant_types: Some(vec!["Q1_0".into(), "Q2_0".into(), "Q4_K_M".into()]),
            official: true,
            legacy_prism_q2_0: false,
        };
        let (s, missing) = model_support(&official, &types);
        assert_eq!(s, Support::Unsupported);
        assert_eq!(missing, vec![TensorType::of(142)]);
        assert_eq!(missing[0].label(), "PQ2_0 (142)");
        // Official build without llama-quantize still refuses ids >= 43.
        let bare = RuntimeQuantInfo {
            quant_types: None,
            official: true,
            legacy_prism_q2_0: false,
        };
        assert_eq!(model_support(&bare, &types).0, Support::Unsupported);
        let fork = RuntimeQuantInfo {
            quant_types: Some(parse_quantize_help(FORK_HELP)),
            official: false,
            legacy_prism_q2_0: false,
        };
        assert_eq!(model_support(&fork, &types).0, Support::Supported);
        assert_eq!(fork_for(&types).unwrap().repo, "PrismML-Eng/llama.cpp");
        assert_eq!(special_types(&types), vec![TensorType::of(142)]);
    }

    #[test]
    fn common_quants_need_nothing() {
        let rt = RuntimeQuantInfo::default();
        assert_eq!(model_support(&rt, &[0, 1, 12, 14]).0, Support::Supported);
        assert!(special_types(&[0, 12, 14]).is_empty());
        // Q1_0 is upstream but recent: unknown without a quantize list, known with one.
        assert_eq!(model_support(&rt, &[41]).0, Support::Unknown);
        let old = RuntimeQuantInfo {
            quant_types: Some(vec!["Q4_K_M".into()]),
            official: true,
            legacy_prism_q2_0: false,
        };
        assert_eq!(model_support(&old, &[41]).0, Support::Unsupported);
        // An id nobody knows is unknown for a custom runtime.
        assert_eq!(model_support(&rt, &[99]).0, Support::Unknown);
        assert!(fork_for(&[41]).is_none());
    }

    #[test]
    fn legacy_prism_q2_0_only_accepts_the_frozen_runtime_line() {
        let types = model_types(&[0, 42], true);
        let current_prism = RuntimeQuantInfo {
            quant_types: Some(parse_quantize_help(FORK_HELP)),
            official: false,
            legacy_prism_q2_0: false,
        };
        assert_eq!(
            model_support(&current_prism, &types).0,
            Support::Unsupported
        );
        let legacy = RuntimeQuantInfo {
            legacy_prism_q2_0: true,
            ..current_prism
        };
        assert_eq!(model_support(&legacy, &types).0, Support::Supported);
        let hint = fork_for(&types).unwrap();
        assert_eq!(hint.id, "prismml-legacy");
        assert!(hint.url.ends_with("/prism-b9601-68faa14"));
    }
}
