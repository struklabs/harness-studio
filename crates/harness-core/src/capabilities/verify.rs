//! Best-effort endpoint checks against a running llama-server, and the "effective launch
//! values" (what the server actually used, e.g. the context `--fit` picked).
//!
//! Chat servers get only cheap requests that never run inference: GET `/props`, `/metrics`,
//! `/slots`, `/v1/models`, `/lora-adapters`; POST `/v1/messages/count_tokens`,
//! `/apply-template` (tokenize/template only) and a transport-level POST `/v1/responses`.
//! An invalid gzip body is rejected before the JSON handler, without inference or an
//! exception log. A matching request to a missing route must give 404; servers that
//! reject bodies before routing give an inconclusive result instead of a false positive.
//!
//! Servers started in embedding or rerank mode (`serve_mode`) get one real, tiny request
//! instead of the chat-only checks: POST `/v1/embeddings` with one short input, or POST
//! `/rerank` with one query and one document. A 200 with vectors / scores marks
//! `embeddings_endpoint` / `rerank_endpoint` as verified (b11236: nomic-embed-text-v1.5 and
//! bge-reranker-v2-m3; a server without `--rerank` answers 501).
//!
//! `--props` is never added: it only enables *changing* global properties through
//! POST `/props`, while GET `/props` works without it (checked on build 11236).

use super::{now_secs, ModelObservation, Verification};
use crate::serve_mode::ServeMode;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

/// What one probe round found.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeReport {
    /// Feature id -> result (only endpoints that gave a conclusive answer).
    pub features: BTreeMap<String, Verification>,
    pub metric_names: Vec<String>,
    /// Model path the observation belongs to (`/props.model_path`).
    pub model_path: Option<String>,
    pub model: Option<ModelObservation>,
    pub props: PropsSummary,
}

/// Effective values reported by `/props` and `/v1/models`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PropsSummary {
    /// Per-slot context (`default_generation_settings.n_ctx`).
    pub n_ctx: Option<u64>,
    pub total_slots: Option<u64>,
    pub n_ctx_train: Option<u64>,
    pub model_alias: Option<String>,
    pub build_info: Option<String>,
    pub chat_format: Option<String>,
    pub reasoning_format: Option<String>,
    pub speculative_types: Option<String>,
    #[serde(default)]
    pub modalities: BTreeMap<String, bool>,
}

/// Values parsed from the server log of the run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LogSummary {
    pub n_slots: Option<u64>,
    pub n_ctx_slot: Option<u64>,
    pub kv_unified: Option<bool>,
    /// `offloaded N/M layers to GPU` (printed at higher log verbosity only).
    pub gpu_layers_offloaded: Option<u64>,
    pub gpu_layers_total: Option<u64>,
    /// K/V pair (`q8_0-f16`) the CUDA build has no flash-attention kernel for: it converts
    /// K and V to f16 on every token ([`kv_fa_fallback`]).
    #[serde(default)]
    pub kv_fa_fallback: Option<String>,
    /// `common_fit_params: failed to fit params to free device memory` ([`fit_params_failed`]).
    #[serde(default)]
    pub fit_failed: bool,
}

/// What the server actually runs with, next to what was requested.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct EffectiveLaunchValues {
    pub requested_ctx: u32,
    pub requested_gpu_layers: u32,
    pub requested_slots: Option<u32>,
    /// Per-slot context the server uses.
    pub n_ctx: Option<u64>,
    pub total_slots: Option<u64>,
    pub n_ctx_train: Option<u64>,
    pub kv_unified: Option<bool>,
    pub gpu_layers_offloaded: Option<u64>,
    pub gpu_layers_total: Option<u64>,
    /// Where the GPU layer count comes from: `log` (the `offloaded N/M` line, printed only
    /// at `-lv 4` / `--verbose`) or `derived` (llama.cpp's rule applied to `-ngl`, the
    /// model's `block_count` and the runtime's GPU devices). `None` when unknown.
    #[serde(default)]
    pub gpu_layers_source: Option<String>,
    /// `--fit on` chose the GPU layers (no `-ngl` was passed) and the log did not say how many.
    #[serde(default)]
    pub gpu_layers_by_fit: bool,
    /// The server picked a different context than requested (`-c 0` or `--fit`).
    pub ctx_adjusted: bool,
    pub model_alias: Option<String>,
    pub build_info: Option<String>,
    pub chat_format: Option<String>,
    pub reasoning_format: Option<String>,
    pub speculative_types: Option<String>,
    #[serde(default)]
    pub modalities: BTreeMap<String, bool>,
    /// K/V pair the CUDA build converts to f16 on every token (slow), see [`kv_fa_fallback`].
    #[serde(default)]
    pub kv_fa_fallback: Option<String>,
    /// llama-server could not fit the settings into free device memory and started anyway
    /// ([`fit_params_failed`]): on Windows the driver may spill the excess into shared system
    /// memory.
    #[serde(default)]
    pub fit_failed: bool,
    /// Smaller shared-memory pressure, separate from failure to fit.
    #[serde(default)]
    pub shared_memory_slowdown: bool,
    #[serde(default)]
    pub shared_memory_bytes: Option<u64>,
    /// Unix seconds.
    pub observed_at: u64,
}

impl EffectiveLaunchValues {
    pub fn combine(
        requested_ctx: u32,
        requested_gpu_layers: u32,
        requested_slots: Option<u32>,
        props: &PropsSummary,
        log: &LogSummary,
    ) -> Self {
        let n_ctx = props.n_ctx.or(log.n_ctx_slot);
        let total_slots = props.total_slots.or(log.n_slots);
        // With several slots (and no unified KV) the requested context is split among them.
        let per_slot_requested = match (total_slots, log.kv_unified) {
            (Some(s), Some(false)) if s > 1 => u64::from(requested_ctx) / s,
            _ => u64::from(requested_ctx),
        };
        let ctx_adjusted = match n_ctx {
            Some(n) => {
                requested_ctx == 0 || (n != per_slot_requested && n != u64::from(requested_ctx))
            }
            None => false,
        };
        Self {
            requested_ctx,
            requested_gpu_layers,
            requested_slots,
            n_ctx,
            total_slots,
            n_ctx_train: props.n_ctx_train,
            kv_unified: log.kv_unified,
            gpu_layers_offloaded: log.gpu_layers_offloaded,
            gpu_layers_total: log.gpu_layers_total,
            gpu_layers_source: log.gpu_layers_offloaded.map(|_| "log".to_string()),
            gpu_layers_by_fit: false,
            ctx_adjusted,
            model_alias: props.model_alias.clone(),
            build_info: props.build_info.clone(),
            chat_format: props.chat_format.clone(),
            reasoning_format: props.reasoning_format.clone(),
            speculative_types: props.speculative_types.clone(),
            modalities: props.modalities.clone(),
            kv_fa_fallback: log.kv_fa_fallback.clone(),
            fit_failed: log.fit_failed,
            shared_memory_slowdown: false,
            shared_memory_bytes: None,
            observed_at: now_secs(),
        }
    }
}

/// What decides the GPU layers of a run, for [`EffectiveLaunchValues::apply_gpu_layer_rule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuLayerInput {
    /// `-ngl N` was passed by Harness (and nothing in the custom arguments overrides it).
    Explicit(u32),
    /// `--fit on` without `-ngl`: llama-server picks the count.
    Fit,
    /// Custom arguments set `-ngl` / `--device`, or router mode: not derivable.
    Unknown,
}

impl EffectiveLaunchValues {
    /// Fills the GPU layer count when the log did not print it (llama-server b11299 prints
    /// `load_tensors: offloaded N/M layers to GPU` only at `-lv 4`, and never reports it on
    /// `/props` or `/slots`).
    ///
    /// llama.cpp's rule (`llama_model::load_tensors`): the model has `block_count + 1`
    /// layers (the repeating blocks and the output layer) and the last `min(ngl, n + 1)` go
    /// to the GPU devices. Without any GPU device (a CPU build, or `--list-devices` lists
    /// none) nothing runs on a GPU, whatever the log line says.
    pub fn apply_gpu_layer_rule(
        &mut self,
        input: GpuLayerInput,
        block_count: Option<u64>,
        gpu_devices: Option<usize>,
    ) {
        let total = block_count.map(|n| n + 1).or(self.gpu_layers_total);
        if gpu_devices == Some(0) {
            if let Some(total) = total {
                self.gpu_layers_offloaded = Some(0);
                self.gpu_layers_total = Some(total);
                self.gpu_layers_source = Some("derived".into());
            }
            return;
        }
        if self.gpu_layers_offloaded.is_some() {
            return;
        }
        match input {
            GpuLayerInput::Explicit(ngl) => {
                if let (Some(total), Some(_)) = (total, gpu_devices) {
                    self.gpu_layers_offloaded = Some(u64::from(ngl).min(total));
                    self.gpu_layers_total = Some(total);
                    self.gpu_layers_source = Some("derived".into());
                }
            }
            GpuLayerInput::Fit => {
                self.gpu_layers_by_fit = true;
                self.gpu_layers_total = total;
            }
            GpuLayerInput::Unknown => {}
        }
    }
}

/// Custom arguments that change the GPU layers or devices themselves.
pub fn custom_args_set_gpu_layers(custom_args: Option<&str>) -> bool {
    custom_args.is_some_and(|a| {
        a.split_whitespace().any(|t| {
            let flag = t.split('=').next().unwrap_or(t);
            matches!(
                flag,
                "-ngl" | "--gpu-layers" | "--n-gpu-layers" | "-dev" | "--device" | "-fit" | "--fit"
            )
        })
    })
}

fn bool_map(v: Option<&Value>) -> BTreeMap<String, bool> {
    v.and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
                .collect()
        })
        .unwrap_or_default()
}

fn str_at(v: &Value, ptr: &str) -> Option<String> {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// Parses GET `/props`: effective values + the model-dependent observation.
pub fn parse_props(props: &Value) -> (PropsSummary, Option<String>, ModelObservation) {
    let summary = PropsSummary {
        n_ctx: props
            .pointer("/default_generation_settings/n_ctx")
            .and_then(Value::as_u64),
        total_slots: props.get("total_slots").and_then(Value::as_u64),
        n_ctx_train: None,
        model_alias: str_at(props, "/model_alias"),
        build_info: str_at(props, "/build_info"),
        chat_format: str_at(props, "/default_generation_settings/params/chat_format"),
        reasoning_format: str_at(
            props,
            "/default_generation_settings/params/reasoning_format",
        ),
        speculative_types: str_at(
            props,
            "/default_generation_settings/params/speculative.types",
        ),
        modalities: bool_map(props.get("modalities")),
    };
    let obs = ModelObservation {
        modalities: summary.modalities.clone(),
        chat_template_caps: bool_map(props.get("chat_template_caps")),
        template_reasoning: str_at(props, "/chat_template")
            .map(|t| crate::gguf::template_supports_reasoning(&t)),
        n_ctx_train: None,
        with_mmproj: false,
        observed_at: now_secs(),
    };
    (summary, str_at(props, "/model_path"), obs)
}

/// `n_ctx_train` of the first model in GET `/v1/models`.
pub fn parse_models_n_ctx_train(models: &Value) -> Option<u64> {
    models
        .pointer("/data/0/meta/n_ctx_train")
        .and_then(Value::as_u64)
}

/// Metric names (without `# HELP`/`# TYPE` lines) of a Prometheus text exposition.
pub fn parse_metric_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split([' ', '{']).next())
        .map(str::to_string)
        .collect();
    names.dedup();
    names
}

/// The K/V pair of llama.cpp's CUDA warning `no FlashAttention vector kernel compiled for K/V
/// types q8_0-f16, converting K and V to f16 instead (slow)`: the default CUDA build has no
/// kernel for mixed cache types (measured on b11327, `docs/en/PERFORMANCE.md` "CUDA").
pub fn kv_fa_fallback(line: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"no FlashAttention vector kernel compiled for K/V types (\S+), converting K and V to f16 instead \(slow\)",
        )
        .unwrap()
    });
    re.captures(line)
        .map(|c| c[1].trim_end_matches(',').to_string())
}

/// Server-log line for [`kv_fa_fallback`] (`lang`: UI language): what happens and which
/// pair to use instead.
pub fn kv_fa_fallback_note(pair: &str, lang: &str) -> String {
    if lang == "tr" {
        format!("[Harness Studio Warn] CUDA derlemesinde {pair} anahtar/değer çifti için hızlı dikkat çekirdeği yok: her token'da K ve V f16'ya çevriliyor (üretim yaklaşık %27-38 yavaşlar, geçici bellek gerekir). Eşit bir çift kullanın (q8_0/q8_0 veya q4_0/q4_0) ya da Konuşma hafızası hassasiyetini Otomatik yapın.")
    } else {
        format!("[Harness Studio Warn] The CUDA build has no fast attention kernel for the K/V pair {pair}: K and V are converted to f16 on every token (generation about 27-38% slower, needs a temporary buffer). Use an equal pair (q8_0/q8_0 or q4_0/q4_0) or set conversation memory precision to Automatic.")
    }
}

/// llama-server's `common_fit_params: failed to fit params to free device memory: ...` line:
/// the settings do not fit in free device memory and the server starts anyway.
///
/// "... n_gpu_layers already set by user to N, abort" is not such a failure: llama.cpp only
/// skips its automatic fitting when `-ngl` (or a split, device list, overrides) is given, which
/// the Harness always does. Measured on RTX 3050 Ti 4 GB (b11445, CUDA): the line appeared for a
/// 0.5B model using 605 MiB and for a Llama-3.2-3B Q6_K using 3131 of 3962 MiB, both running
/// fully on the GPU, so it said nothing about memory.
pub fn fit_params_failed(line: &str) -> bool {
    line.contains("common_fit_params: failed to fit params to free device memory")
        && !line.contains("already set by user")
}

/// Server-log line for [`fit_params_failed`] (`lang`: UI language).
/// The driver moved part of a server's device memory into shared system memory: Windows'
/// `GPU Process Memory` counters of the server process. Shared usage is also normal in small
/// amounts (host-visible staging), so only at least 1 GiB and at least 40% of the dedicated
/// usage count. Measured on an RX 7900 GRE (Vulkan, b11445): runs that fit showed 27 MiB
/// (0.5B), 2124 of 9922 MiB (14B, 64k, q4_0 KV) and 1827 of 10246 MiB (gpt-oss-20b, 32k);
/// the 14B with a 64k f16 KV that did not fit showed 12395 shared against 8499 dedicated.
/// On an RTX 3050 Ti Laptop (4 GB, CUDA b11457): fits showed 112 of 2603 MiB (Qwen2.5-3B, 16k) and
/// 658 of 3884 MiB (64k, below 1 GiB); a 98304 f16 KV that did not fit showed 1892 shared against
/// 3868 dedicated (ratio 0.49): half of the dedicated usage was too strict for a 4 GB card, 40% keeps
/// the largest fitting ratio seen (0.21) well below the limit.
/// An integrated GPU has (almost) no dedicated memory and is never counted.
pub fn spilled_to_shared(dedicated_bytes: u64, shared_bytes: u64) -> bool {
    const GIB: u64 = 1 << 30;
    dedicated_bytes >= GIB / 2 && shared_bytes >= GIB && shared_bytes * 5 >= dedicated_bytes * 2
}

/// Mild pressure: ignore staging buffers and integrated GPUs. RTX 3050 Ti measurements
/// showed 112/2603 MiB without slowdown, but 658/3884 MiB with a large prompt slowdown.
/// Require both 256 MiB and 10% so larger cards' ordinary staging stays below the threshold.
pub fn shared_memory_slowdown(dedicated_bytes: u64, shared_bytes: u64) -> bool {
    dedicated_bytes >= 512 * (1 << 20)
        && shared_bytes >= 256 * (1 << 20)
        && shared_bytes >= dedicated_bytes / 10
        && !spilled_to_shared(dedicated_bytes, shared_bytes)
}

pub fn shared_slowdown_note(shared_bytes: u64, lang: &str) -> String {
    let mib = shared_bytes / (1 << 20);
    if lang == "tr" {
        format!("[Harness Studio Warn] Model yavaşlayabilir: {mib} MiB paylaşılan sistem belleği kullanıyor. Özellikle uzun istemlerde bağlamı veya KV hassasiyetini azaltmayı deneyin.")
    } else {
        format!("[Harness Studio Warn] Model may slow down: using {mib} MiB of shared system memory. Try reducing context or KV precision, especially for long prompts.")
    }
}

/// Server-log line for [`spilled_to_shared`] (`lang`: UI language).
pub fn spilled_note(shared_bytes: u64, lang: &str) -> String {
    let gib = format!("{:.1}", shared_bytes as f64 / (1u64 << 30) as f64);
    if lang == "tr" {
        format!("[Harness Studio Warn] Model ekran kartı belleğine sığmadı: sürücü {gib} GB'ı paylaşılan sistem belleğine taşıdı (yavaşlar, RAM dolabilir). Bağlamı, KV hassasiyetini veya GPU katman sayısını (-ngl) düşürün.")
    } else {
        format!("[Harness Studio Warn] The model did not fit in GPU memory: the driver moved {gib} GB into shared system memory (slow, can fill RAM). Lower the context, the KV precision or the GPU layer count (-ngl).")
    }
}

pub fn fit_failed_note(lang: &str) -> String {
    if lang == "tr" {
        "[Harness Studio Warn] llama-server ayarları boş cihaz belleğine sığdıramadı ve yine de başlattı. Windows'ta sürücü fazlalığı paylaşılan sistem belleğine taşıyabilir (yavaşlar, RAM dolabilir). Bağlamı veya GPU katman sayısını (-ngl) düşürün.".to_string()
    } else {
        "[Harness Studio Warn] llama-server could not fit the settings into free device memory and started anyway. On Windows the driver may move the excess into shared system memory (slow, can fill RAM). Lower the context or the GPU layer count (-ngl).".to_string()
    }
}

pub fn parse_log(lines: &[String]) -> LogSummary {
    static INIT: OnceLock<Regex> = OnceLock::new();
    static OFFLOAD: OnceLock<Regex> = OnceLock::new();
    let init = INIT.get_or_init(|| {
        Regex::new(r"n_slots\s*=\s*(\d+),\s*n_ctx_slot\s*=\s*(\d+)(?:,\s*kv_unified\s*=\s*'?(true|false)'?)?")
            .unwrap()
    });
    let offload =
        OFFLOAD.get_or_init(|| Regex::new(r"offloaded\s+(\d+)/(\d+)\s+layers to GPU").unwrap());
    let mut out = LogSummary::default();
    for line in lines {
        if let Some(c) = init.captures(line) {
            out.n_slots = c[1].parse().ok();
            out.n_ctx_slot = c[2].parse().ok();
            out.kv_unified = c.get(3).map(|m| m.as_str() == "true");
        }
        if let Some(c) = offload.captures(line) {
            out.gpu_layers_offloaded = c[1].parse().ok();
            out.gpu_layers_total = c[2].parse().ok();
        }
        if let Some(pair) = kv_fa_fallback(line) {
            out.kv_fa_fallback = Some(pair);
        }
        if fit_params_failed(line) {
            out.fit_failed = true;
        }
    }
    out
}

fn verified(ok: bool, detail: impl Into<Option<String>>) -> Verification {
    Verification {
        ok,
        detail: detail.into(),
        verified_at: now_secs(),
    }
}

/// Where the probe connects.
#[derive(Debug, Clone)]
pub struct ProbeTarget {
    /// `http://127.0.0.1:8080`
    pub base_url: String,
    pub api_key: Option<String>,
    pub router_mode: bool,
    pub has_mmproj: bool,
    pub serve_mode: ServeMode,
}

/// Vector length of the first item of a `/v1/embeddings` response.
pub fn parse_embedding_dims(body: &Value) -> Option<usize> {
    body.pointer("/data/0/embedding")
        .and_then(Value::as_array)
        .map(Vec::len)
        .filter(|&n| n > 0)
}

/// Number of scored documents in a `/rerank` response.
pub fn parse_rerank_results(body: &Value) -> Option<usize> {
    let results = body.get("results").and_then(Value::as_array)?;
    results
        .iter()
        .all(|r| r.get("relevance_score").is_some_and(Value::is_number))
        .then_some(results.len())
        .filter(|&n| n > 0)
}

/// Server-log lines explaining what the probe itself caused (`lang`: UI language).
pub fn probe_log_notes(report: &ProbeReport, lang: &str) -> Vec<String> {
    let mut out = Vec::new();
    if report
        .features
        .get("responses_api")
        .is_some_and(|v| v.ok && v.detail.as_deref() == Some("HTTP 400"))
    {
        out.push(if lang == "tr" {
            "[Harness Studio] Yukarıdaki /v1/responses 400 hatası ('input' is required) beklenen sonuçtur: uç nokta doğrulaması rotanın varlığını boş bir istekle denetler. Gerçek bir hata değildir.".to_string()
        } else {
            "[Harness Studio] The /v1/responses 400 error above ('input' is required) is expected: endpoint verification checks the route with an empty request. It is not a real error.".to_string()
        });
    }
    out
}

/// Runs the checks. Network errors, 401/403 and 5xx other than 501 are inconclusive and
/// leave no record. In router mode nothing model-specific is probed (the router answers
/// for itself, not for a model), so only the router-independent endpoints are checked.
pub async fn probe_server(target: &ProbeTarget) -> ProbeReport {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let base = target.base_url.trim_end_matches('/');
    let auth = |rb: reqwest::RequestBuilder| match target.api_key.as_deref() {
        Some(k) if !k.is_empty() => rb.bearer_auth(k),
        _ => rb,
    };
    let mut report = ProbeReport::default();

    enum Outcome {
        Ok(String),
        Missing(u16),
        Inconclusive,
    }
    async fn send(rb: reqwest::RequestBuilder) -> (Outcome, u16) {
        match rb.send().await {
            Ok(res) => {
                let code = res.status().as_u16();
                match code {
                    200..=299 => (Outcome::Ok(res.text().await.unwrap_or_default()), code),
                    // Route missing, or the feature disabled by a flag.
                    404 | 405 | 501 => (Outcome::Missing(code), code),
                    _ => {
                        let _ = res.text().await;
                        (Outcome::Inconclusive, code)
                    }
                }
            }
            Err(_) => (Outcome::Inconclusive, 0),
        }
    }

    // GET /metrics
    match send(auth(client.get(format!("{base}/metrics")))).await.0 {
        Outcome::Ok(body) => {
            report.metric_names = parse_metric_names(&body);
            report.features.insert(
                "metrics".into(),
                verified(
                    !report.metric_names.is_empty(),
                    Some(format!("{} metrics", report.metric_names.len())),
                ),
            );
        }
        Outcome::Missing(code) => {
            report.features.insert(
                "metrics".into(),
                verified(false, Some(format!("HTTP {code}"))),
            );
        }
        Outcome::Inconclusive => {}
    }

    if target.router_mode {
        return report;
    }

    // GET /props
    match send(auth(client.get(format!("{base}/props")))).await.0 {
        Outcome::Ok(body) => {
            if let Ok(v) = serde_json::from_str::<Value>(&body) {
                let (summary, model_path, mut obs) = parse_props(&v);
                obs.with_mmproj = target.has_mmproj;
                report.props = summary;
                report.model_path = model_path;
                report.model = Some(obs);
                report.features.insert("props".into(), verified(true, None));
            }
        }
        Outcome::Missing(code) => {
            report.features.insert(
                "props".into(),
                verified(false, Some(format!("HTTP {code}"))),
            );
        }
        Outcome::Inconclusive => {}
    }

    // GET /v1/models
    if let Outcome::Ok(body) = send(auth(client.get(format!("{base}/v1/models")))).await.0 {
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            report.props.n_ctx_train = parse_models_n_ctx_train(&v);
            if let Some(obs) = report.model.as_mut() {
                obs.n_ctx_train = report.props.n_ctx_train;
            }
            report
                .features
                .insert("models_endpoint".into(), verified(true, None));
        }
    }

    // Embedding / rerank servers: one real, tiny request for their own endpoint.
    let mode_check = match target.serve_mode {
        ServeMode::Chat | ServeMode::Transcription | ServeMode::Image => None,
        ServeMode::Embedding => Some((
            "embeddings_endpoint",
            client
                .post(format!("{base}/v1/embeddings"))
                .json(&serde_json::json!({"input": "Harness Studio"})),
        )),
        ServeMode::Rerank => Some((
            "rerank_endpoint",
            client
                .post(format!("{base}/rerank"))
                .json(&serde_json::json!({
                    "query": "Harness Studio",
                    "documents": ["Harness Studio"]
                })),
        )),
    };
    if let Some((feature, rb)) = mode_check {
        let v = match send(auth(rb)).await.0 {
            Outcome::Ok(body) => {
                let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                Some(match target.serve_mode {
                    ServeMode::Embedding => match parse_embedding_dims(&v) {
                        Some(n) => verified(true, Some(format!("{n} dims"))),
                        None => verified(false, Some("no vector".to_string())),
                    },
                    _ => match parse_rerank_results(&v) {
                        Some(n) => verified(true, Some(format!("{n} results"))),
                        None => verified(false, Some("no scores".to_string())),
                    },
                })
            }
            Outcome::Missing(c) => Some(verified(false, Some(format!("HTTP {c}")))),
            Outcome::Inconclusive => None,
        };
        if let Some(v) = v {
            report.features.insert(feature.to_string(), v);
        }
    }

    // Simple GET/POST endpoints: 2xx = present, 404/405/501 = absent/disabled.
    let alias = report
        .props
        .model_alias
        .clone()
        .unwrap_or_else(|| "model".to_string());
    let chat = target.serve_mode == ServeMode::Chat;
    let mut checks: Vec<(&str, reqwest::RequestBuilder)> = vec![
        ("slots", client.get(format!("{base}/slots"))),
        ("lora_adapters", client.get(format!("{base}/lora-adapters"))),
        (
            "anthropic_messages",
            client
                .post(format!("{base}/v1/messages/count_tokens"))
                .json(&serde_json::json!({
                    "model": alias,
                    "messages": [{"role": "user", "content": "hi"}]
                })),
        ),
        (
            "apply_template",
            client
                .post(format!("{base}/apply-template"))
                .json(&serde_json::json!({"messages": [{"role": "user", "content": "hi"}]})),
        ),
    ];
    // The chat-only routes say nothing about an embedding/rerank run.
    if !chat {
        checks.retain(|(f, _)| matches!(*f, "slots" | "lora_adapters"));
    }
    for (feature, rb) in checks {
        let (outcome, _) = send(auth(rb)).await;
        let v = match outcome {
            Outcome::Ok(_) => Some(verified(true, None)),
            Outcome::Missing(c) => Some(verified(false, Some(format!("HTTP {c}")))),
            Outcome::Inconclusive => None,
        };
        if let Some(v) = v {
            report.features.insert(feature.to_string(), v);
        }
    }
    if chat {
        // cpp-httplib matches a POST route before decoding its body. Invalid gzip
        // therefore distinguishes a registered route (400, or 415 without gzip
        // support) from a missing route (404), without invoking the JSON handler.
        // Check the missing route too: a proxy decoding before routing cannot
        // establish the endpoint's existence with this method.
        let request = |path: &str| {
            auth(
                client
                    .post(format!("{base}{path}"))
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .header(reqwest::header::CONTENT_ENCODING, "gzip")
                    .body("{}"),
            )
        };
        let (outcome, code) = send(request("/v1/responses")).await;
        let result = match outcome {
            Outcome::Missing(c) => Some(verified(false, Some(format!("HTTP {c}")))),
            Outcome::Inconclusive if matches!(code, 400 | 415) => {
                let (_, missing_code) = send(request("/__harness_missing_responses_probe")).await;
                (missing_code == 404).then(|| {
                    verified(
                        true,
                        Some(format!("HTTP {code} (transport; missing route: 404)")),
                    )
                })
            }
            _ => None,
        };
        if let Some(result) = result {
            report.features.insert("responses_api".into(), result);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::super::testdata::read;
    use super::*;

    #[test]
    fn props_and_models_real_output() {
        let props: Value = serde_json::from_str(&read("b11236-qwen3-4b.props.json")).unwrap();
        let (s, path, obs) = parse_props(&props);
        assert_eq!(s.n_ctx, Some(4096));
        assert_eq!(s.total_slots, Some(2));
        assert_eq!(s.build_info.as_deref(), Some("b11236-f1ea20621"));
        assert_eq!(s.speculative_types.as_deref(), Some("none"));
        assert_eq!(s.modalities.get("vision"), Some(&false));
        assert!(path
            .unwrap()
            .ends_with("Qwen3-4B-Instruct-2507-Q4_K_M.gguf"));
        assert_eq!(obs.chat_template_caps.get("supports_tools"), Some(&true));
        assert_eq!(
            obs.chat_template_caps.get("supports_reasoning_effort"),
            Some(&false)
        );

        let models: Value = serde_json::from_str(&read("b11236-qwen3-4b.v1-models.json")).unwrap();
        assert_eq!(parse_models_n_ctx_train(&models), Some(262144));
    }

    /// Real `/props` of a hybrid Qwen3-0.6B (b11321): the server reports no reasoning flag, but the
    /// template it serves switches thinking; Qwen3-4B-Instruct-2507 (above) has no such switch.
    #[test]
    fn props_template_reasoning_real_output() {
        let hybrid: Value =
            serde_json::from_str(&read("b11321-qwen3-0.6b-hybrid.props.json")).unwrap();
        let (_, _, obs) = parse_props(&hybrid);
        assert_eq!(
            obs.chat_template_caps.get("supports_reasoning_effort"),
            Some(&false)
        );
        assert_eq!(
            obs.chat_template_caps.get("supports_preserve_reasoning"),
            Some(&false)
        );
        assert_eq!(obs.template_reasoning, Some(true));

        let instruct: Value = serde_json::from_str(&read("b11236-qwen3-4b.props.json")).unwrap();
        assert_eq!(parse_props(&instruct).2.template_reasoning, Some(false));
        let none = parse_props(&serde_json::json!({"model_path": "m.gguf"})).2;
        assert_eq!(none.template_reasoning, None);
    }

    #[test]
    fn embedding_and_rerank_real_output() {
        let emb: Value =
            serde_json::from_str(&read("b11236-nomic-embed.v1-embeddings.json")).unwrap();
        assert_eq!(parse_embedding_dims(&emb), Some(768));
        let rerank: Value = serde_json::from_str(&read("b11236-bge-reranker.rerank.json")).unwrap();
        assert_eq!(parse_rerank_results(&rerank), Some(3));
        // The 501 body of an embedding server asked to rerank has no scores.
        let no: Value = serde_json::from_str(&read("b11236-nomic-embed.rerank-501.json")).unwrap();
        assert_eq!(parse_rerank_results(&no), None);
        assert_eq!(parse_embedding_dims(&no), None);

        // The embedding server's /props: same shape as a chat server, no pooling field.
        let props: Value = serde_json::from_str(&read("b11236-nomic-embed.props.json")).unwrap();
        let (s, path, _) = parse_props(&props);
        assert_eq!(s.n_ctx, Some(2048));
        assert!(path.unwrap().ends_with("nomic-embed-text-v1.5.Q8_0.gguf"));
    }

    #[test]
    fn responses_probe_gets_an_explanation() {
        let mut report = ProbeReport::default();
        assert!(probe_log_notes(&report, "tr").is_empty());
        report.features.insert(
            "responses_api".into(),
            verified(true, Some("HTTP 400".to_string())),
        );
        assert!(probe_log_notes(&report, "tr")[0].contains("beklenen"));
        assert!(probe_log_notes(&report, "en")[0].contains("expected"));
        // A route that answered 200 or a missing route leaves no note.
        report.features.insert(
            "responses_api".into(),
            verified(false, Some("HTTP 404".into())),
        );
        assert!(probe_log_notes(&report, "en").is_empty());
    }

    #[test]
    fn metrics_real_output() {
        let names = parse_metric_names(&read("b11236.metrics.txt"));
        assert!(names.contains(&"llamacpp:prompt_tokens_total".to_string()));
        assert!(names.contains(&"llamacpp:requests_processing".to_string()));
        assert!(names.iter().all(|n| !n.starts_with('#')));
    }

    #[test]
    fn log_real_output_and_effective_values() {
        let lines: Vec<String> = read("b11236-qwen3-4b.server-log.txt")
            .lines()
            .map(str::to_string)
            .collect();
        let log = parse_log(&lines);
        assert_eq!(log.n_slots, Some(2));
        assert_eq!(log.n_ctx_slot, Some(4096));
        assert_eq!(log.kv_unified, Some(false));

        let props: Value = serde_json::from_str(&read("b11236-qwen3-4b.props.json")).unwrap();
        let (s, _, _) = parse_props(&props);
        // Launched with -c 8192 -np 2: 4096 per slot is exactly what was asked for.
        let eff = EffectiveLaunchValues::combine(8192, 99, Some(2), &s, &log);
        assert_eq!(eff.n_ctx, Some(4096));
        assert!(!eff.ctx_adjusted);
        // `-c 0` (model default / --fit) always counts as adjusted.
        assert!(EffectiveLaunchValues::combine(0, 99, None, &s, &log).ctx_adjusted);
        // A different context than requested is flagged.
        assert!(EffectiveLaunchValues::combine(65536, 99, Some(2), &s, &log).ctx_adjusted);
    }

    /// The exact stderr line of llama-server b11327 (CUDA 13.4, RTX 3050 Ti Laptop).
    const KV_FALLBACK_LINE: &str = "W ggml_cuda_flash_attn_ext_vec: no FlashAttention vector kernel compiled for K/V types q8_0-f16, converting K and V to f16 instead (slow). Add \"q8_0-f16\" to GGML_CUDA_FA_QUANTS to compile it.";

    #[test]
    fn kv_fa_fallback_line_is_recognized() {
        assert_eq!(
            kv_fa_fallback(KV_FALLBACK_LINE).as_deref(),
            Some("q8_0-f16")
        );
        // With the `[stderr]` prefix Harness adds, and the other measured pair.
        let prefixed = format!(
            "[stderr] {}",
            KV_FALLBACK_LINE.replace("q8_0-f16", "q8_0-q4_0")
        );
        assert_eq!(kv_fa_fallback(&prefixed).as_deref(), Some("q8_0-q4_0"));
        // Unrelated lines.
        assert_eq!(kv_fa_fallback("llama_kv_cache: size = 1152.00 MiB"), None);
        assert_eq!(
            kv_fa_fallback("W ggml_cuda_flash_attn_ext_vec: something else"),
            None
        );
    }

    #[test]
    fn kv_fa_fallback_reaches_log_summary_and_effective_values() {
        let lines = vec![
            "load_tensors: ok".to_string(),
            format!("[stderr] {KV_FALLBACK_LINE}"),
        ];
        let log = parse_log(&lines);
        assert_eq!(log.kv_fa_fallback.as_deref(), Some("q8_0-f16"));
        let eff = EffectiveLaunchValues::combine(4096, 99, None, &PropsSummary::default(), &log);
        assert_eq!(eff.kv_fa_fallback.as_deref(), Some("q8_0-f16"));
        assert_eq!(
            parse_log(&["load_tensors: ok".to_string()]).kv_fa_fallback,
            None
        );
    }

    #[test]
    fn spill_to_shared_memory_uses_measured_thresholds() {
        const MIB: u64 = 1 << 20;
        assert!(spilled_to_shared(8499 * MIB, 12395 * MIB));
        assert!(!spilled_to_shared(741 * MIB, 27 * MIB));
        assert!(!spilled_to_shared(9922 * MIB, 2124 * MIB));
        assert!(!spilled_to_shared(10246 * MIB, 1827 * MIB));
        // RTX 3050 Ti Laptop (CUDA): fits and a real spill.
        assert!(!spilled_to_shared(2603 * MIB, 112 * MIB));
        assert!(!spilled_to_shared(3884 * MIB, 658 * MIB));
        assert!(spilled_to_shared(3868 * MIB, 1892 * MIB));
        // An integrated GPU's memory is all shared.
        assert!(!spilled_to_shared(0, 6000 * MIB));
    }

    #[test]
    fn mild_shared_pressure_is_distinct_from_spill_and_staging() {
        const MIB: u64 = 1 << 20;
        assert!(shared_memory_slowdown(3884 * MIB, 658 * MIB));
        assert!(!shared_memory_slowdown(2603 * MIB, 112 * MIB));
        assert!(!shared_memory_slowdown(4096 * MIB, 300 * MIB));
        assert!(!shared_memory_slowdown(1024 * MIB, 255 * MIB));
        assert!(!shared_memory_slowdown(0, 6000 * MIB));
        assert!(!shared_memory_slowdown(3868 * MIB, 1892 * MIB));
        assert!(shared_slowdown_note(658 * MIB, "tr").contains("658 MiB"));
        assert!(shared_slowdown_note(658 * MIB, "en").contains("may slow down"));
    }

    #[test]
    fn fit_params_failed_line_is_recognized() {
        let skipped = "[stderr] common_fit_params: failed to fit params to free device memory: n_gpu_layers already set by user to 37, abort";
        assert!(!fit_params_failed(skipped));
        assert!(!parse_log(&[skipped.to_string()]).fit_failed);
        let line = "[stderr] common_fit_params: failed to fit params to free device memory: could not fit to free memory";
        assert!(fit_params_failed(line));
        assert!(!fit_params_failed("llama_kv_cache: size = 1152.00 MiB"));
        let log = parse_log(&[line.to_string()]);
        assert!(log.fit_failed);
        let eff = EffectiveLaunchValues::combine(262144, 37, None, &PropsSummary::default(), &log);
        assert!(eff.fit_failed);
        assert!(!parse_log(&["load_tensors: ok".to_string()]).fit_failed);
        assert!(fit_failed_note("tr").contains("-ngl"));
        assert!(fit_failed_note("en").contains("shared system memory"));
    }

    #[test]
    fn kv_fa_fallback_note_names_the_pair_and_the_fix() {
        let tr = kv_fa_fallback_note("q8_0-f16", "tr");
        let en = kv_fa_fallback_note("q8_0-f16", "en");
        assert!(tr.starts_with("[Harness Studio Warn]") && en.starts_with("[Harness Studio Warn]"));
        assert!(
            tr.contains("q8_0-f16") && tr.contains("Eşit bir çift") && tr.contains("q4_0/q4_0")
        );
        assert!(en.contains("q8_0-f16") && en.contains("equal pair") && en.contains("q4_0/q4_0"));
    }

    #[test]
    fn gpu_layers_from_log_or_rule() {
        let s = PropsSummary::default();
        // `-lv 4` log of b11299 (Qwen3-4B, 36 blocks): the line wins.
        let log =
            parse_log(&["0.00.541.399 I load_tensors: offloaded 20/37 layers to GPU".to_string()]);
        let mut eff = EffectiveLaunchValues::combine(4096, 20, None, &s, &log);
        eff.apply_gpu_layer_rule(GpuLayerInput::Explicit(20), Some(36), Some(1));
        assert_eq!(
            (eff.gpu_layers_offloaded, eff.gpu_layers_total),
            (Some(20), Some(37))
        );
        assert_eq!(eff.gpu_layers_source.as_deref(), Some("log"));

        // Default verbosity: derived from -ngl and block_count.
        let quiet = LogSummary::default();
        let mut eff = EffectiveLaunchValues::combine(4096, 999, None, &s, &quiet);
        eff.apply_gpu_layer_rule(GpuLayerInput::Explicit(999), Some(36), Some(2));
        assert_eq!(
            (eff.gpu_layers_offloaded, eff.gpu_layers_total),
            (Some(37), Some(37))
        );
        assert_eq!(eff.gpu_layers_source.as_deref(), Some("derived"));
        let mut eff = EffectiveLaunchValues::combine(4096, 12, None, &s, &quiet);
        eff.apply_gpu_layer_rule(GpuLayerInput::Explicit(12), Some(36), Some(1));
        assert_eq!(eff.gpu_layers_offloaded, Some(12));

        // A CPU build says "offloaded 37/37" too, but nothing is on a GPU.
        let mut eff = EffectiveLaunchValues::combine(4096, 99, None, &s, &log);
        eff.apply_gpu_layer_rule(GpuLayerInput::Explicit(99), Some(36), Some(0));
        assert_eq!(eff.gpu_layers_offloaded, Some(0));

        // `--fit on` without -ngl: unknown count, flagged.
        let mut eff = EffectiveLaunchValues::combine(4096, 99, None, &s, &quiet);
        eff.apply_gpu_layer_rule(GpuLayerInput::Fit, Some(36), Some(1));
        assert_eq!(eff.gpu_layers_offloaded, None);
        assert!(eff.gpu_layers_by_fit);

        // Unknown devices or custom -ngl: nothing derived.
        let mut eff = EffectiveLaunchValues::combine(4096, 99, None, &s, &quiet);
        eff.apply_gpu_layer_rule(GpuLayerInput::Explicit(99), Some(36), None);
        assert_eq!(eff.gpu_layers_offloaded, None);
        assert!(custom_args_set_gpu_layers(Some("--no-mmap -ngl 10")));
        assert!(custom_args_set_gpu_layers(Some("--device=none")));
        assert!(!custom_args_set_gpu_layers(Some("--no-mmap -ngld 10")));
        assert!(!custom_args_set_gpu_layers(None));
    }
}
