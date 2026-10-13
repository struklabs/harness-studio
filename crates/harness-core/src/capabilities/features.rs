//! Per-feature status with separate evidence levels:
//!
//! - [`CapabilityLevel::Unsupported`]: the runtime has no flag/endpoint for it.
//! - [`CapabilityLevel::FlagDetected`]: the flag is in `--help`. Only means "can be shown".
//! - [`CapabilityLevel::EndpointVerified`]: a real run of this binary answered for it.
//! - [`CapabilityLevel::ModelDependent`]: the runtime has it, but whether it works depends on
//!   the model (GGUF metadata, `/props` of an earlier run).
//! - [`CapabilityLevel::PlatformDependent`]: depends on the backend or devices.
//! - [`CapabilityLevel::Unknown`]: endpoint-only features that no run has checked yet.
//!
//! Reasons are i18n keys (`caps.reason.*`) with parameters, never prose.

use super::RuntimeCapabilities;
use crate::gguf::GgufMetadata;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityLevel {
    Unsupported,
    Unknown,
    FlagDetected,
    EndpointVerified,
    ModelDependent,
    PlatformDependent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reason {
    /// i18n key under `caps.reason.`
    pub key: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

impl Reason {
    fn new(key: &str) -> Self {
        Self {
            key: format!("caps.reason.{key}"),
            params: BTreeMap::new(),
        }
    }
    fn with(mut self, k: &str, v: impl ToString) -> Self {
        self.params.insert(k.to_string(), v.to_string());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureStatus {
    pub id: String,
    pub level: CapabilityLevel,
    /// `Some(true)`: usable as far as we know; `Some(false)`: will not work here;
    /// `None`: can't tell yet.
    pub available: Option<bool>,
    pub reason: Reason,
    /// Spelling of the flag found in this runtime, if any.
    pub flag: Option<String>,
}

/// Every reason key [`feature_status`] can return (checked against the locale files).
pub const REASON_KEYS: &[&str] = &[
    "flag_missing",
    "flag_only",
    "verified",
    "verify_failed",
    "not_probed",
    "endpoint_missing",
    "depends_on_model",
    "model_moe",
    "model_not_moe",
    "model_tools",
    "model_no_tools",
    "model_no_template",
    "needs_mmproj",
    "model_vision",
    "model_audio",
    "model_no_audio",
    "model_no_vision",
    "model_embedding",
    "model_not_embedding",
    "model_rerank",
    "model_not_rerank",
    "template_effort",
    "template_no_effort",
    "swa_model_only",
    "backend_dependent",
    "needs_gpu",
    "needs_multi_gpu",
    "experimental_multi_gpu",
    "platform_io",
    "kv_quant_backend",
];

/// What the feature check knows besides the runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct FeatureContext<'a> {
    pub model: Option<&'a GgufMetadata>,
    /// Model path as used for `/props` observations (`RuntimeCapabilities::models` key).
    pub model_path: Option<&'a str>,
    /// An mmproj file is configured for the launch.
    pub has_mmproj: bool,
}

/// GGUF `pooling_type` values (llama.cpp `enum llama_pooling_type`).
fn pooling_name(p: u64) -> &'static str {
    match p {
        0 => "none",
        1 => "mean",
        2 => "cls",
        3 => "last",
        4 => "rank",
        _ => "unknown",
    }
}

struct Eval<'a> {
    caps: &'a RuntimeCapabilities,
    ctx: FeatureContext<'a>,
}

impl Eval<'_> {
    fn status(
        id: &str,
        level: CapabilityLevel,
        available: Option<bool>,
        reason: Reason,
        flag: Option<&str>,
    ) -> FeatureStatus {
        FeatureStatus {
            id: id.to_string(),
            level,
            available,
            reason,
            flag: flag.map(str::to_string),
        }
    }

    /// Flag-only feature, upgraded to verified when a run checked `verify`.
    fn flag(&self, id: &str, candidates: &[&str], verify: Option<&str>) -> FeatureStatus {
        let Some(flag) = candidates.iter().copied().find(|c| self.caps.has_flag(c)) else {
            return Self::status(
                id,
                CapabilityLevel::Unsupported,
                Some(false),
                Reason::new("flag_missing").with("flag", candidates[0]),
                None,
            );
        };
        if let Some(v) = verify.and_then(|k| self.caps.verification(k)) {
            return if v.ok {
                Self::status(
                    id,
                    CapabilityLevel::EndpointVerified,
                    Some(true),
                    Reason::new("verified"),
                    Some(flag),
                )
            } else {
                Self::status(
                    id,
                    CapabilityLevel::FlagDetected,
                    None,
                    Reason::new("verify_failed")
                        .with("detail", v.detail.clone().unwrap_or_default()),
                    Some(flag),
                )
            };
        }
        Self::status(
            id,
            CapabilityLevel::FlagDetected,
            Some(true),
            Reason::new("flag_only"),
            Some(flag),
        )
    }

    /// Endpoint-only feature (no flag): known only after a run checked it.
    fn endpoint(&self, id: &str, verify: &str) -> FeatureStatus {
        match self.caps.verification(verify) {
            Some(v) if v.ok => Self::status(
                id,
                CapabilityLevel::EndpointVerified,
                Some(true),
                Reason::new("verified"),
                None,
            ),
            Some(v) => Self::status(
                id,
                CapabilityLevel::Unsupported,
                Some(false),
                Reason::new("endpoint_missing")
                    .with("detail", v.detail.clone().unwrap_or_default()),
                None,
            ),
            None => Self::status(
                id,
                CapabilityLevel::Unknown,
                None,
                Reason::new("not_probed"),
                None,
            ),
        }
    }

    fn observation(&self) -> Option<&super::ModelObservation> {
        self.ctx
            .model_path
            .and_then(|p| self.caps.models.get(&super::model_key(p)))
    }

    /// Flag present + a model-specific verdict.
    fn model_dependent(
        &self,
        id: &str,
        candidates: &[&str],
        verdict: Option<(bool, Reason)>,
    ) -> FeatureStatus {
        let base = self.flag(id, candidates, None);
        if base.level == CapabilityLevel::Unsupported {
            return base;
        }
        let (available, reason) = match verdict {
            Some((ok, reason)) => (Some(ok), reason),
            None => (None, Reason::new("depends_on_model")),
        };
        FeatureStatus {
            level: CapabilityLevel::ModelDependent,
            available,
            reason,
            ..base
        }
    }

    /// [`Self::model_dependent`], upgraded to `EndpointVerified` once a run in that mode
    /// answered `verify` (a real `/v1/embeddings` or `/rerank` request), unless the model
    /// itself says no.
    fn verified_model_dependent(
        &self,
        id: &str,
        candidates: &[&str],
        verdict: Option<(bool, Reason)>,
        verify: &str,
    ) -> FeatureStatus {
        let base = self.model_dependent(id, candidates, verdict);
        let model_ok = base.available != Some(false);
        match self.caps.verification(verify) {
            Some(v) if v.ok && model_ok && base.level != CapabilityLevel::Unsupported => {
                FeatureStatus {
                    level: CapabilityLevel::EndpointVerified,
                    available: Some(true),
                    reason: Reason::new("verified"),
                    ..base
                }
            }
            _ => base,
        }
    }

    fn platform(
        &self,
        id: &str,
        candidates: &[&str],
        available: Option<bool>,
        reason: Reason,
    ) -> FeatureStatus {
        let base = self.flag(id, candidates, None);
        if base.level == CapabilityLevel::Unsupported {
            return base;
        }
        FeatureStatus {
            level: CapabilityLevel::PlatformDependent,
            available,
            reason,
            ..base
        }
    }

    fn gpu_count(&self) -> usize {
        self.caps.devices.len()
    }

    fn backends(&self) -> String {
        let b = self.caps.backends();
        if b.is_empty() {
            "CPU".to_string()
        } else {
            b.join(", ")
        }
    }

    fn template_cap(&self, name: &str) -> Option<bool> {
        self.observation()
            .and_then(|o| o.chat_template_caps.get(name).copied())
    }
}

/// Status of every known feature for this runtime (+ optional model).
pub fn feature_status(caps: &RuntimeCapabilities, ctx: FeatureContext<'_>) -> Vec<FeatureStatus> {
    let e = Eval { caps, ctx };
    let model = ctx.model;
    let mut out = Vec::new();

    // Platform / backend dependent.
    out.push(e.platform(
        "flash_attn",
        &["--flash-attn", "-fa"],
        Some(true),
        Reason::new("backend_dependent").with("backends", e.backends()),
    ));
    out.push(e.platform(
        "kv_quant",
        &["--cache-type-k", "-ctk"],
        Some(true),
        Reason::new("kv_quant_backend").with("backends", e.backends()),
    ));
    let gpus = e.gpu_count();
    let split_tensor = caps
        .flag("--split-mode")
        .is_some_and(|f| f.enum_values.iter().any(|v| v == "tensor"));
    out.push(if split_tensor {
        e.platform(
            "split_mode_tensor",
            &["--split-mode"],
            Some(gpus >= 2),
            if gpus >= 2 {
                Reason::new("experimental_multi_gpu").with("count", gpus)
            } else {
                Reason::new("needs_multi_gpu").with("count", gpus)
            },
        )
    } else {
        Eval::status(
            "split_mode_tensor",
            CapabilityLevel::Unsupported,
            Some(false),
            Reason::new("flag_missing").with("flag", "--split-mode tensor"),
            None,
        )
    });
    out.push(e.platform(
        "main_gpu",
        &["--main-gpu", "-mg"],
        Some(gpus >= 1),
        if gpus >= 1 {
            Reason::new("backend_dependent").with("backends", e.backends())
        } else {
            Reason::new("needs_gpu")
        },
    ));
    out.push(e.platform(
        "tensor_split",
        &["--tensor-split", "-ts"],
        Some(gpus >= 2),
        Reason::new("needs_multi_gpu").with("count", gpus),
    ));
    out.push(e.platform(
        "load_mode",
        &["--load-mode", "-lm"],
        Some(true),
        Reason::new("platform_io"),
    ));

    // Plain runtime switches.
    for (id, flags) in [
        ("kv_offload", &["--kv-offload", "--no-kv-offload"][..]),
        ("kv_unified", &["--kv-unified", "--no-kv-unified"][..]),
        (
            "context_shift",
            &["--context-shift", "--no-context-shift"][..],
        ),
        ("cache_reuse", &["--cache-reuse"][..]),
        ("cache_ram", &["--cache-ram"][..]),
        ("fit", &["--fit"][..]),
        ("jinja", &["--jinja", "--no-jinja"][..]),
        ("reasoning_format", &["--reasoning-format"][..]),
        ("reasoning_budget", &["--reasoning-budget"][..]),
        (
            "speculative",
            &["--spec-draft-model", "--model-draft", "-md"][..],
        ),
        ("spec_type", &["--spec-type"][..]),
        ("lora", &["--lora", "--lora-scaled"][..]),
        ("router", &["--models-dir", "--models-path"][..]),
        ("router_models_max", &["--models-max"][..]),
        (
            "router_autoload",
            &["--models-autoload", "--no-models-autoload"][..],
        ),
        ("sleep_idle", &["--sleep-idle-seconds"][..]),
        ("cors", &["--cors-origins"][..]),
        ("tools", &["--tools"][..]),
        ("mcp", &["--mcp-servers-config"][..]),
        ("timeout", &["--timeout", "-to"][..]),
    ] {
        out.push(e.flag(id, flags, None));
    }

    // Verified through a running server.
    out.push(e.flag("metrics", &["--metrics"], Some("metrics")));
    out.push(e.flag("slots", &["--slots", "--no-slots"], Some("slots")));
    // GET /props works without `--props` (that flag only enables POST /props).
    out.push(match caps.verification("props") {
        Some(_) => e.endpoint("props", "props"),
        None => e.flag("props", &["--props"], None),
    });
    out.push(e.flag(
        "lora_live_scale",
        &["--lora-init-without-apply"],
        Some("lora_adapters"),
    ));
    out.push(e.endpoint("anthropic_messages", "anthropic_messages"));
    out.push(e.endpoint("responses_api", "responses_api"));
    out.push(e.endpoint("apply_template", "apply_template"));

    // Model dependent.
    let moe = model.map(|m| {
        let experts = m.expert_count.unwrap_or(0);
        if experts > 1 {
            (true, Reason::new("model_moe").with("experts", experts))
        } else {
            (false, Reason::new("model_not_moe"))
        }
    });
    out.push(e.model_dependent(
        "moe_cpu_offload",
        &["--n-cpu-moe", "-ncmoe", "--cpu-moe"],
        moe,
    ));

    let swa = Some((true, Reason::new("swa_model_only")));
    out.push(e.model_dependent("swa_full", &["--swa-full"], swa));

    let tools_verdict = match (e.template_cap("supports_tools"), model) {
        (Some(true), _) => Some((true, Reason::new("model_tools"))),
        (Some(false), _) => Some((false, Reason::new("model_no_tools"))),
        (None, Some(m)) if !m.has_chat_template => Some((false, Reason::new("model_no_template"))),
        (None, Some(m)) if m.template_supports_tools => Some((true, Reason::new("model_tools"))),
        (None, Some(_)) => Some((false, Reason::new("model_no_tools"))),
        (None, None) => None,
    };
    out.push(e.model_dependent("tool_calling", &["--jinja"], tools_verdict));

    let effort = e.template_cap("supports_reasoning_effort").map(|ok| {
        if ok {
            (true, Reason::new("template_effort"))
        } else {
            (false, Reason::new("template_no_effort"))
        }
    });
    out.push(e.model_dependent("reasoning_effort", &["--reasoning-effort"], effort));

    let vision_obs = e.observation().and_then(|o| {
        o.modalities
            .get("vision")
            .copied()
            .map(|v| (v, o.with_mmproj))
    });
    let vision = match vision_obs {
        Some((true, _)) => Some((true, Reason::new("model_vision"))),
        Some((false, true)) => Some((false, Reason::new("model_no_vision"))),
        _ if !ctx.has_mmproj && (model.is_some() || vision_obs.is_some()) => {
            Some((false, Reason::new("needs_mmproj")))
        }
        _ => None,
    };
    out.push(e.model_dependent("vision", &["--mmproj", "-mm"], vision));
    // Audio input shares the projector flag but has its own modality observation.
    let audio = e
        .observation()
        .and_then(|o| o.modalities.get("audio").copied())
        .map(|v| {
            (
                v,
                Reason::new(if v { "model_audio" } else { "model_no_audio" }),
            )
        });
    out.push(e.model_dependent("audio_input", &["--mmproj", "-mm"], audio));

    // Embedding / rerank follow the file kind (`domain::detect_kind`): bge-reranker-v2-m3 has
    // no `pooling_type` at all, only a classification head.
    use crate::domain::{detect_kind, ModelKind};
    let kind = model.map(|m| detect_kind(Some(m)));
    let pooling = model.and_then(|m| m.pooling_type);
    let embedding = kind.map(|k| match (k, pooling) {
        (ModelKind::Embedding, Some(p)) => (
            true,
            Reason::new("model_embedding").with("pooling", pooling_name(p)),
        ),
        (ModelKind::Embedding, None) => (true, Reason::new("model_embedding")),
        _ => (false, Reason::new("model_not_embedding")),
    });
    let rerank = kind.map(|k| match k {
        ModelKind::Reranker => (true, Reason::new("model_rerank")),
        _ => (false, Reason::new("model_not_rerank")),
    });
    out.push(e.verified_model_dependent(
        "embeddings",
        &["--embeddings", "--embedding"],
        embedding.clone(),
        "embeddings_endpoint",
    ));
    let pooling_verdict = match (&embedding, &rerank) {
        (_, Some((true, r))) => Some((true, r.clone())),
        (v, _) => v.clone(),
    };
    out.push(e.model_dependent("pooling", &["--pooling"], pooling_verdict));
    out.push(e.verified_model_dependent(
        "rerank",
        &["--rerank", "--reranking"],
        rerank,
        "rerank_endpoint",
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::super::{testdata, ModelObservation, Verification};
    use super::*;

    fn get<'a>(list: &'a [FeatureStatus], id: &str) -> &'a FeatureStatus {
        list.iter()
            .find(|f| f.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    }

    #[test]
    fn levels_from_real_runtime() {
        let caps = testdata::caps("b11236");
        let list = feature_status(&caps, FeatureContext::default());

        let fa = get(&list, "flash_attn");
        assert_eq!(fa.level, CapabilityLevel::PlatformDependent);
        assert_eq!(fa.reason.params["backends"], "Vulkan");

        // Two Vulkan devices in the captured output.
        assert_eq!(get(&list, "split_mode_tensor").available, Some(true));
        assert_eq!(
            get(&list, "kv_unified").level,
            CapabilityLevel::FlagDetected
        );
        assert_eq!(get(&list, "kv_unified").reason.key, "caps.reason.flag_only");
        assert_eq!(get(&list, "metrics").level, CapabilityLevel::FlagDetected);
        assert_eq!(get(&list, "responses_api").level, CapabilityLevel::Unknown);
        assert_eq!(
            get(&list, "moe_cpu_offload").level,
            CapabilityLevel::ModelDependent
        );
        assert_eq!(get(&list, "moe_cpu_offload").available, None);
        assert_eq!(
            get(&list, "speculative").flag.as_deref(),
            Some("--spec-draft-model")
        );

        // Every reason is a known key.
        for f in &list {
            let k = f.reason.key.trim_start_matches("caps.reason.");
            assert!(REASON_KEYS.contains(&k), "{}", f.reason.key);
        }
    }

    #[test]
    fn missing_flags_are_unsupported() {
        let mut caps = testdata::caps("b11236");
        caps.flags
            .retain(|k, _| k != "--swa-full" && k != "--split-mode");
        caps.reindex();
        let list = feature_status(&caps, FeatureContext::default());
        let swa = get(&list, "swa_full");
        assert_eq!(swa.level, CapabilityLevel::Unsupported);
        assert_eq!(swa.available, Some(false));
        assert_eq!(swa.reason.params["flag"], "--swa-full");
        assert_eq!(
            get(&list, "split_mode_tensor").level,
            CapabilityLevel::Unsupported
        );
    }

    #[test]
    fn verification_and_model_observations() {
        let mut caps = testdata::caps("b11236");
        let v = |ok| Verification {
            ok,
            detail: Some("HTTP 404".to_string()),
            verified_at: 1,
        };
        caps.endpoint_verified.insert("metrics".into(), v(true));
        caps.endpoint_verified
            .insert("responses_api".into(), v(false));
        caps.models.insert(
            super::super::model_key("m.gguf"),
            ModelObservation {
                modalities: BTreeMap::from([("vision".to_string(), false)]),
                chat_template_caps: BTreeMap::from([
                    ("supports_tools".to_string(), true),
                    ("supports_reasoning_effort".to_string(), false),
                ]),
                with_mmproj: false,
                ..Default::default()
            },
        );
        let meta = GgufMetadata {
            expert_count: Some(128),
            has_chat_template: true,
            ..Default::default()
        };
        let list = feature_status(
            &caps,
            FeatureContext {
                model: Some(&meta),
                model_path: Some("m.gguf"),
                has_mmproj: false,
            },
        );
        assert_eq!(
            get(&list, "metrics").level,
            CapabilityLevel::EndpointVerified
        );
        let resp = get(&list, "responses_api");
        assert_eq!(resp.level, CapabilityLevel::Unsupported);
        assert_eq!(resp.reason.params["detail"], "HTTP 404");
        let moe = get(&list, "moe_cpu_offload");
        assert_eq!(moe.available, Some(true));
        assert_eq!(moe.reason.params["experts"], "128");
        assert_eq!(get(&list, "tool_calling").available, Some(true));
        assert_eq!(get(&list, "reasoning_effort").available, Some(false));
        assert_eq!(get(&list, "vision").reason.key, "caps.reason.needs_mmproj");
        assert_eq!(get(&list, "embeddings").available, Some(false));
    }

    /// bge-reranker-v2-m3 (no `pooling_type`, classification head) and nomic-embed-text-v1.5
    /// header facts; a real `/rerank` / `/v1/embeddings` answer upgrades them to verified.
    #[test]
    fn embedding_and_rerank_follow_the_file_kind() {
        let mut caps = testdata::caps("b11236");
        let bge = GgufMetadata {
            architecture: Some("bert".into()),
            general_type: Some("model".into()),
            has_cls_head: true,
            ..Default::default()
        };
        let nomic = GgufMetadata {
            architecture: Some("nomic-bert".into()),
            pooling_type: Some(1),
            ..Default::default()
        };
        let ctx = |m| FeatureContext {
            model: Some(m),
            model_path: None,
            has_mmproj: false,
        };
        let list = feature_status(&caps, ctx(&bge));
        let rerank = get(&list, "rerank");
        assert_eq!(rerank.level, CapabilityLevel::ModelDependent);
        assert_eq!(rerank.available, Some(true));
        assert_eq!(get(&list, "embeddings").available, Some(false));
        assert_eq!(get(&list, "pooling").available, Some(true));

        caps.endpoint_verified.insert(
            "rerank_endpoint".into(),
            Verification {
                ok: true,
                detail: Some("1 results".into()),
                verified_at: 1,
            },
        );
        caps.endpoint_verified.insert(
            "embeddings_endpoint".into(),
            Verification {
                ok: true,
                detail: Some("768 dims".into()),
                verified_at: 1,
            },
        );
        let list = feature_status(&caps, ctx(&bge));
        assert_eq!(
            get(&list, "rerank").level,
            CapabilityLevel::EndpointVerified
        );
        // The model says no: a verified endpoint does not make a reranker embed.
        assert_eq!(
            get(&list, "embeddings").level,
            CapabilityLevel::ModelDependent
        );
        let list = feature_status(&caps, ctx(&nomic));
        let emb = get(&list, "embeddings");
        assert_eq!(emb.level, CapabilityLevel::EndpointVerified);
        assert_eq!(get(&list, "rerank").available, Some(false));
        let list = feature_status(&caps, FeatureContext::default());
        assert_eq!(
            get(&list, "rerank").level,
            CapabilityLevel::EndpointVerified
        );
    }
}
