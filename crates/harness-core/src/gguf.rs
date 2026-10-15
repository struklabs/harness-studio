use crate::error_key::err;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

const GGUF_MAGIC: &[u8; 4] = b"GGUF";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GgufMetadata {
    pub version: u32,
    pub tensor_count: u64,
    pub kv_count: u64,
    pub architecture: Option<String>,
    /// audio.cpp model family; only meaningful for architecture = audiocpp.
    #[serde(default)]
    pub audiocpp_family: Option<String>,
    pub model_name: Option<String>,
    /// Provenance URLs that may identify a Hugging Face repository when a split file was
    /// copied into the library by hand.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub repo_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_repo_url: Option<String>,
    pub context_length: Option<u64>,
    pub file_type: Option<u32>,
    pub quantization: String,
    // Architecture hyper-parameters used for memory estimation.
    #[serde(default)]
    pub block_count: Option<u64>,
    #[serde(default)]
    pub embedding_length: Option<u64>,
    #[serde(default)]
    pub head_count: Option<u64>,
    #[serde(default)]
    pub head_count_kv: Option<u64>,
    #[serde(default)]
    pub key_length: Option<u64>,
    #[serde(default)]
    pub value_length: Option<u64>,
    #[serde(default)]
    pub expert_count: Option<u64>,
    #[serde(default)]
    pub expert_used_count: Option<u64>,
    /// `<arch>.feed_forward_length` (largest value of per-layer arrays).
    #[serde(default)]
    pub feed_forward_length: Option<u64>,
    /// `<arch>.expert_feed_forward_length` (MoE experts).
    #[serde(default)]
    pub expert_feed_forward_length: Option<u64>,
    /// `<arch>.attention.sliding_window`: window of sliding-window-attention layers (tokens).
    #[serde(default)]
    pub sliding_window: Option<u64>,
    /// `<arch>.attention.sliding_window_pattern` as a number: every N-th layer is a full
    /// attention layer, the others use the window (llama.cpp `set_swa_pattern(N)`).
    #[serde(default)]
    pub sliding_window_pattern: Option<u64>,
    /// `<arch>.attention.sliding_window_pattern` as a per-layer array: number of layers that
    /// use the window.
    #[serde(default)]
    pub swa_layer_count: Option<u64>,
    /// `<arch>.attention.key_length_swa` / `value_length_swa`: head size of the
    /// sliding-window layers when it differs from the full-attention layers (Gemma 4).
    #[serde(default)]
    pub key_length_swa: Option<u64>,
    #[serde(default)]
    pub value_length_swa: Option<u64>,
    /// KV heads of the sliding-window and of the full-attention layers, from the per-layer
    /// `head_count_kv` and `sliding_window_pattern` arrays (Gemma 4: 8 and 1). `None` when
    /// the file has no per-layer arrays (then `head_count_kv` applies to every layer).
    #[serde(default)]
    pub head_count_kv_swa: Option<u64>,
    #[serde(default)]
    pub head_count_kv_full: Option<u64>,
    /// Layers with attention (a KV cache) of hybrid models whose per-layer `head_count_kv`
    /// array is 0 for the recurrent layers (Jamba, Granite hybrid, Nemotron-H).
    #[serde(default)]
    pub attention_layer_count: Option<u64>,
    /// `<arch>.full_attention_interval`: every N-th layer has attention, the others are
    /// recurrent (Qwen3-Next, Qwen3.5/3.6).
    #[serde(default)]
    pub full_attention_interval: Option<u64>,
    /// `<arch>.ssm.*` of recurrent / hybrid models (Mamba, gated delta net): they keep one
    /// fixed-size state per sequence (= per server slot).
    #[serde(default)]
    pub ssm_conv_kernel: Option<u64>,
    #[serde(default)]
    pub ssm_state_size: Option<u64>,
    #[serde(default)]
    pub ssm_inner_size: Option<u64>,
    #[serde(default)]
    pub ssm_group_count: Option<u64>,
    /// `<arch>.rope.scaling.type` (`yarn`, `linear`, `none`...): RoPE scaling the file declares.
    #[serde(default)]
    pub rope_scaling_type: Option<String>,
    /// `<arch>.rope.scaling.factor`.
    #[serde(default)]
    pub rope_scaling_factor: Option<f64>,
    /// `<arch>.rope.scaling.original_context_length`: context the model was trained on before
    /// the scaling.
    #[serde(default)]
    pub rope_scaling_original_ctx: Option<u64>,
    /// Bytes of `token_embd.weight` (llama.cpp keeps it in host memory for the lookup).
    #[serde(default)]
    pub token_embd_bytes: Option<u64>,
    /// Bytes of `output.weight`; `None` with `token_embd_bytes` set = tied output (llama.cpp
    /// then also uploads a copy of the embeddings as the output layer).
    #[serde(default)]
    pub output_bytes: Option<u64>,
    #[serde(default)]
    pub has_chat_template: bool,
    /// The embedded chat template handles `tools` (native tool/function calling).
    #[serde(default)]
    pub template_supports_tools: bool,
    /// Tokenizer / pooling hints for embedding & reranker models.
    #[serde(default)]
    pub pooling_type: Option<u64>,
    /// True for multimodal projector files (`general.type = mmproj` / clip architecture).
    #[serde(default)]
    pub is_mmproj: bool,
    /// `general.type` ("model", "mmproj", "adapter", "imatrix"...).
    #[serde(default)]
    pub general_type: Option<String>,
    /// `adapter.type` of adapter files ("lora").
    #[serde(default)]
    pub adapter_type: Option<String>,
    /// `clip.has_vision_encoder` / `clip.has_audio_encoder` of projector files.
    #[serde(default)]
    pub has_vision_encoder: bool,
    #[serde(default)]
    pub has_audio_encoder: bool,
    /// Projector type of the audio encoder (`clip.projector_type` of an audio projector, else
    /// `clip.audio.projector_type`): `lfm2a`, `qwen3asr`, `voxtral`, `ultravox`... It picks the
    /// transcription prompt of a chat model that hears (`crate::asr::llm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_projector: Option<String>,
    /// A `<arch>.classifier.output_labels` key exists (classification heads, e.g. rerankers).
    #[serde(default)]
    pub has_classifier_labels: bool,
    /// `<arch>.decision.type` (`vllm-d3`, `decision2`...): a native decision model, served
    /// through llama-server's `/v1/systemone` (llama.cpp lists it with the `decisions` output
    /// modality).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_type: Option<String>,
    /// A sequence-classification head (`cls.output.weight` / `cls.weight` tensors), which is
    /// how BERT/XLM-R rerankers such as bge-reranker-v2-m3 are stored (no `pooling_type` key).
    #[serde(default)]
    pub has_cls_head: bool,
    /// `general.size_label` ("4B", "567M", "32x2.4B"; free text, display only).
    #[serde(default)]
    pub size_label: Option<String>,
    /// `general.basename` (pairs a projector with its model: both carry the same basename).
    #[serde(default)]
    pub basename: Option<String>,
    /// Sum of the element counts of the tensors described in this file (exact; for split
    /// models only this shard's tensors). `None` when the tensor table could not be read.
    #[serde(default)]
    pub param_count: Option<u64>,
    /// `split.count` of split GGUFs (0 or absent for single files).
    #[serde(default)]
    pub split_count: Option<u64>,
    /// The embedded chat template can switch reasoning on (`enable_thinking`,
    /// `reasoning_effort`, gpt-oss channels, or an unconditional `<think>` opener).
    #[serde(default)]
    pub template_supports_reasoning: bool,
    #[serde(default)]
    pub reasoning_control: crate::reasoning::ReasoningControl,
    /// Tokenizer facts used to match draft models with their target.
    #[serde(default)]
    pub tokenizer: Option<TokenizerInfo>,
    /// Distinct ggml tensor types of this file (numeric `ggml_type` ids, sorted). Used to tell
    /// whether a runtime can load the file (fork-only quant types; see `runtimes::quant`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tensor_types: Vec<u32>,
    /// Type 42 whose byte spans match Prism's deprecated group-128 layout rather than
    /// upstream Q2_0's group-64 layout. Only set when the tensor table proves the difference.
    #[serde(default)]
    pub legacy_prism_q2_0: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnownMetadataFixes {
    pub sliding_window: Option<u64>,
    pub override_kv: Option<&'static str>,
}

impl GgufMetadata {
    pub fn known_metadata_fixes(&self) -> KnownMetadataFixes {
        // Google's QAT 1B GGUF ships sliding_window=1024, but its trained window is 512;
        // llama.cpp produces garbage on prompts longer than ~600 tokens without this fix.
        let gemma3_1b = self.architecture.as_deref() == Some("gemma3")
            && self.embedding_length == Some(1152)
            && self.sliding_window == Some(1024);
        KnownMetadataFixes {
            sliding_window: if gemma3_1b {
                Some(512)
            } else {
                self.sliding_window
            },
            override_kv: gemma3_1b.then_some("gemma3.attention.sliding_window=int:512"),
        }
    }
}

/// Tokenizer facts of a GGUF (`tokenizer.ggml.*`), enough to tell whether two models can
/// be target and draft of speculative decoding (see `library::draft`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenizerInfo {
    /// `tokenizer.ggml.model` ("gpt2", "llama", "t5", "gemma4"...).
    #[serde(default)]
    pub model: Option<String>,
    /// `tokenizer.ggml.pre` (pre-tokenizer: "qwen2", "gpt-4o", "default"...).
    #[serde(default)]
    pub pre: Option<String>,
    /// Number of entries in `tokenizer.ggml.tokens`.
    #[serde(default)]
    pub vocab_size: Option<u64>,
    #[serde(default)]
    pub bos_id: Option<u64>,
    #[serde(default)]
    pub eos_id: Option<u64>,
    #[serde(default)]
    pub add_bos: Option<bool>,
    #[serde(default)]
    pub add_eos: Option<bool>,
    /// SHA-256 (hex, first 16 bytes) of the token texts from id [`TOKEN_HASH_START`]
    /// ([`TOKEN_HASH_LEN`] of them; llama.cpp compares token texts from id 5 on). `None` for
    /// smaller vocabularies.
    #[serde(default)]
    pub head_hash: Option<String>,
}

/// First token id compared between target and draft vocabularies (llama.cpp's
/// `SPEC_VOCAB_CHECK_START_TOKEN_ID`).
pub const TOKEN_HASH_START: u64 = 5;
/// Number of token texts hashed into [`TokenizerInfo::head_hash`].
pub const TOKEN_HASH_LEN: u64 = 32_768;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u32)]
enum GgufType {
    Uint8 = 0,
    Int8 = 1,
    Uint16 = 2,
    Int16 = 3,
    Uint32 = 4,
    Int32 = 5,
    Float32 = 6,
    Bool = 7,
    String = 8,
    Array = 9,
    Uint64 = 10,
    Int64 = 11,
    Float64 = 12,
}

impl GgufType {
    fn from_u32(val: u32) -> Result<Self, String> {
        Ok(match val {
            0 => GgufType::Uint8,
            1 => GgufType::Int8,
            2 => GgufType::Uint16,
            3 => GgufType::Int16,
            4 => GgufType::Uint32,
            5 => GgufType::Int32,
            6 => GgufType::Float32,
            7 => GgufType::Bool,
            8 => GgufType::String,
            9 => GgufType::Array,
            10 => GgufType::Uint64,
            11 => GgufType::Int64,
            12 => GgufType::Float64,
            other => return Err(err("gguf.unknown_type").num("id", other).into()),
        })
    }

    /// Byte size of fixed-width types (None for strings/arrays).
    fn fixed_size(self) -> Option<i64> {
        match self {
            GgufType::Uint8 | GgufType::Int8 | GgufType::Bool => Some(1),
            GgufType::Uint16 | GgufType::Int16 => Some(2),
            GgufType::Uint32 | GgufType::Int32 | GgufType::Float32 => Some(4),
            GgufType::Uint64 | GgufType::Int64 | GgufType::Float64 => Some(8),
            GgufType::String | GgufType::Array => None,
        }
    }
}

/// Integer hyper-parameters we care about, matched by key suffix.
const NUMERIC_SUFFIXES: &[&str] = &[
    "context_length",
    "block_count",
    "embedding_length",
    "attention.head_count",
    "attention.head_count_kv",
    "attention.key_length",
    "attention.value_length",
    "expert_count",
    "expert_used_count",
    "pooling_type",
    "feed_forward_length",
    "expert_feed_forward_length",
    "attention.sliding_window",
    "rope.scaling.original_context_length",
    "attention.key_length_swa",
    "attention.value_length_swa",
    "full_attention_interval",
    "ssm.conv_kernel",
    "ssm.state_size",
    "ssm.inner_size",
    "ssm.group_count",
];

/// The Jinja chat template stored in the file (`tokenizer.chat_template`), `None` when the
/// file has none. Skips every other key, so it stays fast on large vocabularies.
pub fn read_gguf_chat_template(path: &Path) -> Result<Option<String>, String> {
    let file = File::open(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).map_err(read_error)?;
    if &magic != GGUF_MAGIC {
        return Err(err("gguf.bad_magic")
            .with("path", path.display())
            .with("found", format!("{:?}", String::from_utf8_lossy(&magic)))
            .into());
    }
    let _version = read_u32(&mut reader)?;
    let _tensor_count = read_u64(&mut reader)?;
    let kv_count = read_u64(&mut reader)?;
    for _ in 0..kv_count {
        let key = read_string(&mut reader)?;
        let val_type = GgufType::from_u32(read_u32(&mut reader)?)?;
        if key == "tokenizer.chat_template" && val_type == GgufType::String {
            return read_string(&mut reader).map(Some);
        }
        skip_value(&mut reader, val_type)?;
    }
    Ok(None)
}

pub fn read_gguf_metadata(path: &Path) -> Result<GgufMetadata, String> {
    let file = File::open(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    // Large buffer + relative seeks: skipping the tokenizer vocab (100k+ strings)
    // no longer costs one syscall per entry.
    let mut reader = BufReader::with_capacity(256 * 1024, file);

    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).map_err(read_error)?;
    if &magic != GGUF_MAGIC {
        return Err(err("gguf.bad_magic")
            .with("path", path.display())
            .with("found", format!("{:?}", String::from_utf8_lossy(&magic)))
            .into());
    }

    let version = read_u32(&mut reader)?;
    let tensor_count = read_u64(&mut reader)?;
    let kv_count = read_u64(&mut reader)?;

    let mut meta = GgufMetadata {
        version,
        tensor_count,
        kv_count,
        ..Default::default()
    };
    let mut numeric: HashMap<String, u64> = HashMap::new();
    let mut general_type: Option<String> = None;
    let mut tok = TokenizerInfo::default();
    let mut has_tokenizer = false;
    let mut kv_complete = true;
    let mut swa_pattern_layers: Option<u64> = None;
    let mut swa_pattern_period: Option<u64> = None;
    // Per-layer arrays of the main architecture (not the projector's).
    let mut swa_pattern: Option<Vec<u64>> = None;
    let mut kv_heads_per_layer: Option<Vec<u64>> = None;
    let mut alignment: Option<u64> = None;
    let mut rope_type: Option<String> = None;
    let mut rope_factor: Option<f64> = None;

    let mut audio_flag = None;
    let mut audio_hint = false;
    let mut audio_projector: Option<String> = None;
    for _ in 0..kv_count {
        let Ok(key) = read_string(&mut reader) else {
            kv_complete = false;
            break;
        };
        let Ok(type_id) = read_u32(&mut reader) else {
            kv_complete = false;
            break;
        };
        let Ok(val_type) = GgufType::from_u32(type_id) else {
            kv_complete = false;
            break;
        };

        match key.as_str() {
            "general.architecture" if val_type == GgufType::String => {
                meta.architecture = read_string(&mut reader).ok()
            }
            "audiocpp.model_spec.family" if val_type == GgufType::String => {
                meta.audiocpp_family = read_string(&mut reader).ok()
            }
            "general.name" if val_type == GgufType::String => {
                meta.model_name = read_string(&mut reader).ok()
            }
            "general.url" if val_type == GgufType::String => {
                meta.url = read_string(&mut reader).ok()
            }
            "general.repo_url" if val_type == GgufType::String => {
                meta.repo_url = read_string(&mut reader).ok()
            }
            "general.source.url" if val_type == GgufType::String => {
                meta.source_url = read_string(&mut reader).ok()
            }
            "general.source.repo_url" if val_type == GgufType::String => {
                meta.source_repo_url = read_string(&mut reader).ok()
            }
            "general.type" if val_type == GgufType::String => {
                general_type = read_string(&mut reader).ok()
            }
            "adapter.type" if val_type == GgufType::String => {
                meta.adapter_type = read_string(&mut reader).ok()
            }
            "clip.has_vision_encoder" if val_type == GgufType::Bool => {
                meta.has_vision_encoder = read_uint(&mut reader, val_type)?.unwrap_or(0) != 0
            }
            "clip.has_audio_encoder" if val_type == GgufType::Bool => {
                audio_flag = Some(read_uint(&mut reader, val_type)?.unwrap_or(0) != 0)
            }
            "clip.projector_type" if val_type == GgufType::String => {
                let kind = read_string(&mut reader)?;
                let audio = matches!(
                    kind.as_str(),
                    "ultravox" | "voxtral" | "qwen2a" | "qwen25o" | "lfm2a" | "qwen3a" | "qwen3asr"
                );
                audio_hint |= audio;
                if audio {
                    audio_projector = Some(kind);
                }
            }
            "clip.audio.projector_type" if val_type == GgufType::String => {
                audio_hint = true;
                let kind = read_string(&mut reader)?;
                audio_projector.get_or_insert(kind);
            }
            k if k.starts_with("clip.audio.") => {
                audio_hint = true;
                skip_value(&mut reader, val_type)?;
            }
            k if val_type == GgufType::String
                && k.strip_suffix(".decision.type")
                    .is_some_and(|arch| !arch.is_empty() && !arch.contains('.')) =>
            {
                meta.decision_type = Some(read_string(&mut reader)?).filter(|t| !t.is_empty())
            }
            k if k.ends_with(".classifier.output_labels") => {
                meta.has_classifier_labels = true;
                skip_value(&mut reader, val_type)?
            }
            "general.file_type" => {
                meta.file_type = read_uint(&mut reader, val_type)?.map(|v| v as u32)
            }
            "tokenizer.chat_template" if val_type == GgufType::String => {
                meta.has_chat_template = true;
                if let Ok(template) = read_string(&mut reader) {
                    meta.template_supports_tools =
                        template.contains("tools") || template.contains("tool_call");
                    meta.template_supports_reasoning = template_supports_reasoning(&template);
                    meta.reasoning_control =
                        crate::reasoning::ReasoningControl::from_template(&template);
                }
            }
            "general.size_label" if val_type == GgufType::String => {
                meta.size_label = read_string(&mut reader).ok()
            }
            "general.basename" if val_type == GgufType::String => {
                meta.basename = read_string(&mut reader).ok()
            }
            "split.count" => meta.split_count = read_uint(&mut reader, val_type)?,
            "general.alignment" => alignment = read_uint(&mut reader, val_type)?,
            "tokenizer.ggml.model" if val_type == GgufType::String => {
                has_tokenizer = true;
                tok.model = read_string(&mut reader).ok()
            }
            "tokenizer.ggml.pre" if val_type == GgufType::String => {
                tok.pre = read_string(&mut reader).ok()
            }
            "tokenizer.ggml.bos_token_id" => tok.bos_id = read_uint(&mut reader, val_type)?,
            "tokenizer.ggml.eos_token_id" => tok.eos_id = read_uint(&mut reader, val_type)?,
            "tokenizer.ggml.add_bos_token" if val_type == GgufType::Bool => {
                tok.add_bos = read_uint(&mut reader, val_type)?.map(|v| v != 0)
            }
            "tokenizer.ggml.add_eos_token" if val_type == GgufType::Bool => {
                tok.add_eos = read_uint(&mut reader, val_type)?.map(|v| v != 0)
            }
            "tokenizer.ggml.tokens" if val_type == GgufType::Array => {
                has_tokenizer = true;
                let (count, hash) = read_token_texts(&mut reader)?;
                tok.vocab_size = Some(count);
                tok.head_hash = hash;
            }
            k if k.ends_with(".rope.scaling.type") && val_type == GgufType::String => {
                if rope_type.is_none() {
                    rope_type = read_string(&mut reader).ok();
                } else {
                    skip_value(&mut reader, val_type)?;
                }
            }
            k if k.ends_with(".rope.scaling.factor")
                && matches!(val_type, GgufType::Float32 | GgufType::Float64) =>
            {
                let v = if val_type == GgufType::Float32 {
                    f32::from_bits(read_u32(&mut reader)?) as f64
                } else {
                    f64::from_bits(read_u64(&mut reader)?)
                };
                rope_factor.get_or_insert(v);
            }
            k if k.ends_with(".attention.sliding_window_pattern") => {
                if val_type == GgufType::Array {
                    let layers = read_uint_array(&mut reader)?;
                    swa_pattern_layers = Some(layers.iter().filter(|&&v| v != 0).count() as u64);
                    swa_pattern = Some(layers);
                } else if let Some(v) = read_uint(&mut reader, val_type)? {
                    swa_pattern_period = Some(v);
                }
            }
            k if k.ends_with(".attention.head_count_kv")
                && val_type == GgufType::Array
                && !k.contains("vision")
                && !k.contains("audio") =>
            {
                let layers = read_uint_array(&mut reader)?;
                if let Some(max) = layers.iter().max() {
                    numeric.insert(key, *max);
                }
                kv_heads_per_layer = Some(layers);
            }
            k if NUMERIC_SUFFIXES
                .iter()
                .any(|s| k.ends_with(&format!(".{}", s))) =>
            {
                let value = if val_type == GgufType::Array {
                    // Per-layer arrays (e.g. head_count_kv on some models): keep the maximum.
                    read_uint_array_max(&mut reader)?
                } else {
                    read_uint(&mut reader, val_type)?
                };
                if let Some(v) = value {
                    numeric.insert(key, v);
                }
            }
            _ => skip_value(&mut reader, val_type)?,
        }
    }

    // Prefer `<arch>.<suffix>`; fall back to any key with that suffix.
    let lookup = |suffix: &str| -> Option<u64> {
        if let Some(arch) = &meta.architecture {
            if let Some(v) = numeric.get(&format!("{}.{}", arch, suffix)) {
                return Some(*v);
            }
        }
        numeric
            .iter()
            .filter(|(k, _)| {
                k.ends_with(&format!(".{}", suffix))
                    && !k.contains("vision")
                    && !k.contains("audio")
            })
            .map(|(_, v)| *v)
            .next()
    };
    meta.context_length = lookup("context_length");
    meta.block_count = lookup("block_count");
    meta.embedding_length = lookup("embedding_length");
    meta.head_count = lookup("attention.head_count");
    meta.head_count_kv = lookup("attention.head_count_kv");
    meta.key_length = lookup("attention.key_length");
    meta.value_length = lookup("attention.value_length");
    meta.expert_count = lookup("expert_count");
    meta.expert_used_count = lookup("expert_used_count");
    meta.pooling_type = lookup("pooling_type");
    meta.feed_forward_length = lookup("feed_forward_length");
    meta.expert_feed_forward_length = lookup("expert_feed_forward_length");
    meta.sliding_window = lookup("attention.sliding_window").filter(|&w| w > 0);
    meta.rope_scaling_type = rope_type;
    meta.rope_scaling_factor = rope_factor;
    meta.rope_scaling_original_ctx = lookup("rope.scaling.original_context_length");
    meta.sliding_window_pattern = swa_pattern_period;
    meta.swa_layer_count = swa_pattern_layers;
    meta.key_length_swa = lookup("attention.key_length_swa");
    meta.value_length_swa = lookup("attention.value_length_swa");
    meta.full_attention_interval = lookup("full_attention_interval").filter(|&n| n > 0);
    meta.ssm_conv_kernel = lookup("ssm.conv_kernel");
    meta.ssm_state_size = lookup("ssm.state_size").filter(|&n| n > 0);
    meta.ssm_inner_size = lookup("ssm.inner_size");
    meta.ssm_group_count = lookup("ssm.group_count");
    if let Some(heads) = &kv_heads_per_layer {
        let attention = heads.iter().filter(|&&h| h > 0).count() as u64;
        if attention < heads.len() as u64 {
            meta.attention_layer_count = Some(attention);
        }
        if let Some(pattern) = swa_pattern.as_ref().filter(|p| p.len() == heads.len()) {
            let max_where = |swa: bool| {
                heads
                    .iter()
                    .zip(pattern)
                    .filter(|(_, &p)| (p != 0) == swa)
                    .map(|(&h, _)| h)
                    .max()
            };
            meta.head_count_kv_swa = max_where(true);
            meta.head_count_kv_full = max_where(false);
        }
    }
    let alignment = alignment.filter(|a| *a > 0).unwrap_or(32);
    meta.is_mmproj =
        general_type.as_deref() == Some("mmproj") || meta.architecture.as_deref() == Some("clip");
    meta.has_audio_encoder = audio_flag.unwrap_or(audio_hint);
    meta.audio_projector = audio_projector.filter(|_| meta.has_audio_encoder);
    meta.general_type = general_type;
    if has_tokenizer {
        meta.tokenizer = Some(tok);
    }
    if kv_complete {
        if let Some(t) = read_tensor_table(&mut reader, tensor_count, alignment) {
            meta.param_count = Some(t.params);
            meta.has_cls_head = t.cls;
            meta.token_embd_bytes = t.token_embd_bytes;
            meta.output_bytes = t.output_bytes;
            meta.tensor_types = t.types;
            meta.legacy_prism_q2_0 = t.legacy_prism_q2_0;
        }
    }

    meta.quantization = match meta.file_type {
        Some(ft) => file_type_to_quantization(ft),
        None => {
            extract_quant_from_filename(path.file_name().and_then(|n| n.to_str()).unwrap_or(""))
        }
    };

    Ok(meta)
}

/// Names of the first `limit` tensors of a GGUF file (the key-value table is skipped). Image
/// generators converted by stable-diffusion.cpp carry no `general.architecture` at all, so
/// their tensor names (`model.diffusion_model.*`, `cond_stage_model.*`) are what tells them
/// apart.
pub fn read_gguf_tensor_names(path: &Path, limit: usize) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).map_err(read_error)?;
    if &magic != GGUF_MAGIC {
        return Err(err("gguf.bad_magic")
            .with("path", path.display())
            .with("found", format!("{:?}", String::from_utf8_lossy(&magic)))
            .into());
    }
    let _version = read_u32(&mut reader)?;
    let tensor_count = read_u64(&mut reader)?;
    let kv_count = read_u64(&mut reader)?;
    for _ in 0..kv_count {
        let _key = read_string(&mut reader)?;
        let val_type = GgufType::from_u32(read_u32(&mut reader)?)?;
        skip_value(&mut reader, val_type)?;
    }
    let mut names = Vec::new();
    for _ in 0..tensor_count.min(limit as u64) {
        let len = read_u64(&mut reader)?;
        if len > 4096 {
            return Err(err("gguf.read_failed").with("error", "tensor name").into());
        }
        let mut name = vec![0u8; len as usize];
        reader.read_exact(&mut name).map_err(read_error)?;
        names.push(String::from_utf8_lossy(&name).to_string());
        let n_dims = read_u32(&mut reader)?;
        if n_dims > 8 {
            return Err(err("gguf.read_failed").with("error", "tensor dims").into());
        }
        // Dimensions, ggml type and data offset.
        seek_rel(&mut reader, 8 * i64::from(n_dims) + 4 + 8)?;
    }
    Ok(names)
}

/// Whether a chat template lets the model reason (a switch or an unconditional opener).
/// Checked on real templates: Qwen3.6 / Gemma 4 (`enable_thinking`), gpt-oss
/// (`reasoning_effort`, `<|channel|>`) -> yes; Qwen3-4B-Instruct-2507 (strips
/// `reasoning_content` from history, never opens `<think>`) and Qwen2.5-Coder -> no.
pub fn template_supports_reasoning(template: &str) -> bool {
    crate::reasoning::ReasoningControl::from_template(template).supported()
}

/// Reads the `tokenizer.ggml.tokens` array (value type already consumed): returns the token
/// count and the hash of the token texts from [`TOKEN_HASH_START`] on.
fn read_token_texts(reader: &mut BufReader<File>) -> Result<(u64, Option<String>), String> {
    use sha2::{Digest, Sha256};
    let item_type = GgufType::from_u32(read_u32(reader)?)?;
    let count = read_u64(reader)?;
    if item_type != GgufType::String {
        for _ in 0..count {
            skip_value(reader, item_type)?;
        }
        return Ok((count, None));
    }
    let end = TOKEN_HASH_START + TOKEN_HASH_LEN;
    let mut hasher = Sha256::new();
    let mut buf = Vec::new();
    for i in 0..count {
        let len = read_u64(reader)?;
        if (TOKEN_HASH_START..end).contains(&i) && len <= 1024 * 1024 {
            buf.resize(len as usize, 0);
            reader.read_exact(&mut buf).map_err(read_error)?;
            hasher.update(len.to_le_bytes());
            hasher.update(&buf);
        } else {
            seek_rel(reader, len as i64)?;
        }
    }
    let hash = (count >= end).then(|| {
        hasher.finalize()[..16]
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    });
    Ok((count, hash))
}

/// Sums the element counts of the tensor table that follows the key/value section.
/// Tensor names of a classification head (llama.cpp `LLM_TENSOR_CLS` / `LLM_TENSOR_CLS_OUT`).
const CLS_HEAD_TENSORS: [&[u8]; 2] = [b"cls.output.weight", b"cls.weight"];

struct TensorSummary {
    params: u64,
    cls: bool,
    token_embd_bytes: Option<u64>,
    output_bytes: Option<u64>,
    /// Distinct ggml types, sorted.
    types: Vec<u32>,
    legacy_prism_q2_0: bool,
}

#[derive(Clone, Copy)]
struct TensorLayout {
    elements: u64,
    ggml_type: u32,
    offset: u64,
}

/// Sum of the tensor element counts, whether a classification head is present and the byte
/// sizes of the embedding and output tensors (from the gaps between data offsets, so no
/// ggml type table is needed).
fn read_tensor_table(
    reader: &mut BufReader<File>,
    tensor_count: u64,
    alignment: u64,
) -> Option<TensorSummary> {
    let mut total: u64 = 0;
    let mut cls = false;
    let mut tensors: Vec<TensorLayout> = Vec::with_capacity(tensor_count.min(100_000) as usize);
    let mut embd_offset = None;
    let mut output_offset = None;
    let mut types: Vec<u32> = Vec::new();
    for _ in 0..tensor_count {
        let name_len = read_u64(reader).ok()?;
        if name_len > 64 * 1024 {
            return None;
        }
        let mut name = Vec::new();
        if name_len <= 32 {
            name = vec![0u8; name_len as usize];
            reader.read_exact(&mut name).ok()?;
            cls |= CLS_HEAD_TENSORS.contains(&name.as_slice());
        } else {
            seek_rel(reader, name_len as i64).ok()?;
        }
        let n_dims = read_u32(reader).ok()?;
        if n_dims == 0 || n_dims > 8 {
            return None;
        }
        let mut elements: u64 = 1;
        for _ in 0..n_dims {
            elements = elements.checked_mul(read_u64(reader).ok()?)?;
        }
        let ggml_type = read_u32(reader).ok()?;
        if !types.contains(&ggml_type) {
            types.push(ggml_type);
        }
        let offset = read_u64(reader).ok()?;
        match name.as_slice() {
            b"token_embd.weight" => embd_offset = Some(offset),
            b"output.weight" => output_offset = Some(offset),
            _ => {}
        }
        tensors.push(TensorLayout {
            elements,
            ggml_type,
            offset,
        });
        total = total.checked_add(elements)?;
    }
    // Data starts at the next multiple of the alignment after the tensor table.
    let table_end = reader.stream_position().ok()?;
    let data_start = table_end.div_ceil(alignment) * alignment;
    let data_len = reader
        .get_ref()
        .metadata()
        .ok()?
        .len()
        .saturating_sub(data_start);
    tensors.sort_unstable_by_key(|t| t.offset);
    types.sort_unstable();
    let size_at = |offset: u64| -> Option<u64> {
        let next = tensors
            .iter()
            .map(|t| t.offset)
            .find(|&o| o > offset)
            .unwrap_or(data_len);
        next.checked_sub(offset)
    };
    // Both layouts used ggml type 42. Upstream Q2_0 stores 64 weights in 18 bytes; the
    // deprecated Prism layout stores 128 in 34. GGUF offsets include alignment padding, so
    // compare aligned spans. Ambiguous small tensors and mixed/inconsistent files stay false.
    let mut official_q2 = 0usize;
    let mut legacy_q2 = 0usize;
    for tensor in tensors.iter().filter(|t| t.ggml_type == 42) {
        let Some(span) = size_at(tensor.offset) else {
            continue;
        };
        let layout_size = |block: u64, bytes: u64| {
            (tensor.elements % block == 0).then(|| {
                let raw = tensor.elements / block * bytes;
                raw.div_ceil(alignment) * alignment
            })
        };
        let official = layout_size(64, 18).is_some_and(|n| n == span);
        let legacy = layout_size(128, 34).is_some_and(|n| n == span);
        match (official, legacy) {
            (true, false) => official_q2 += 1,
            (false, true) => legacy_q2 += 1,
            _ => {}
        }
    }
    Some(TensorSummary {
        params: total,
        cls,
        token_embd_bytes: embd_offset.and_then(size_at),
        output_bytes: output_offset.and_then(size_at),
        types,
        legacy_prism_q2_0: legacy_q2 > 0 && official_q2 == 0,
    })
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(read_error)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, String> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf).map_err(read_error)?;
    Ok(u64::from_le_bytes(buf))
}

/// Reads any integer-typed value as u64 (negative values → None); skips other types.
fn read_uint(reader: &mut BufReader<File>, t: GgufType) -> Result<Option<u64>, String> {
    let mut buf = [0u8; 8];
    Ok(match t {
        GgufType::Uint8 | GgufType::Int8 | GgufType::Bool => {
            reader.read_exact(&mut buf[..1]).map_err(read_error)?;
            Some(buf[0] as u64)
        }
        GgufType::Uint16 | GgufType::Int16 => {
            reader.read_exact(&mut buf[..2]).map_err(read_error)?;
            Some(u16::from_le_bytes([buf[0], buf[1]]) as u64)
        }
        GgufType::Uint32 => Some(read_u32(reader)? as u64),
        GgufType::Int32 => {
            let v = read_u32(reader)? as i32;
            (v >= 0).then_some(v as u64)
        }
        GgufType::Uint64 => Some(read_u64(reader)?),
        GgufType::Int64 => {
            let v = read_u64(reader)? as i64;
            (v >= 0).then_some(v as u64)
        }
        other => {
            skip_value(reader, other)?;
            None
        }
    })
}

/// Number of non-zero items and length of an integer/bool array (value type consumed).
/// A per-layer integer (or bool) array; non-integer items read as 0.
fn read_uint_array(reader: &mut BufReader<File>) -> Result<Vec<u64>, String> {
    let item_type = GgufType::from_u32(read_u32(reader)?)?;
    let count = read_u64(reader)?;
    let mut out = Vec::with_capacity(count.min(4096) as usize);
    for _ in 0..count {
        out.push(read_uint(reader, item_type)?.unwrap_or(0));
    }
    Ok(out)
}

fn read_uint_array_max(reader: &mut BufReader<File>) -> Result<Option<u64>, String> {
    let item_type = GgufType::from_u32(read_u32(reader)?)?;
    let count = read_u64(reader)?;
    let mut max: Option<u64> = None;
    for _ in 0..count {
        if let Some(v) = read_uint(reader, item_type)? {
            max = Some(max.map_or(v, |m| m.max(v)));
        }
    }
    Ok(max)
}

fn read_string<R: Read>(reader: &mut R) -> Result<String, String> {
    let len = read_u64(reader)? as usize;
    if len > 1024 * 1024 {
        return Err(err("gguf.string_too_long").num("len", len).into());
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(read_error)?;
    String::from_utf8(buf).map_err(|e| err("gguf.invalid_utf8").with("error", e).into())
}

fn seek_rel(reader: &mut BufReader<File>, n: i64) -> Result<(), String> {
    reader.seek_relative(n).map_err(read_error)
}

/// A read error in the middle of a GGUF header (truncated or unreadable file).
fn read_error(e: std::io::Error) -> String {
    err("gguf.read_failed").with("error", e).into()
}

fn skip_value(reader: &mut BufReader<File>, val_type: GgufType) -> Result<(), String> {
    if let Some(size) = val_type.fixed_size() {
        return seek_rel(reader, size);
    }
    match val_type {
        GgufType::String => {
            let len = read_u64(reader)? as i64;
            seek_rel(reader, len)
        }
        GgufType::Array => {
            let item_type = GgufType::from_u32(read_u32(reader)?)?;
            let count = read_u64(reader)?;
            match item_type.fixed_size() {
                // Fixed-width arrays are skipped in one jump.
                Some(size) => seek_rel(reader, size.saturating_mul(count as i64)),
                None => {
                    for _ in 0..count {
                        skip_value(reader, item_type)?;
                    }
                    Ok(())
                }
            }
        }
        _ => Ok(()),
    }
}

/// Maps `general.file_type` (llama_ftype) to a quantization label.
pub fn file_type_to_quantization(ft: u32) -> String {
    let s = match ft {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        36 => "TQ1_0",
        37 => "TQ2_0",
        38 => "MXFP4_MOE",
        _ => return format!("Type_{}", ft),
    };
    s.to_string()
}

pub fn extract_quant_from_filename(filename: &str) -> String {
    let upper = filename.to_uppercase();
    // Longer / more specific names first so e.g. "IQ4_XS" isn't reported as something shorter.
    let quants = [
        // PrismML fork types (runtimes::quant FORK_TYPES); PTQ1_0 before its substring TQ1_0.
        "PTQ1_0",
        "PQ2_0",
        "MXFP4_MOE",
        "MXFP4",
        "UD-Q4_K_XL",
        "Q4_K_XL",
        "Q3_K_XL",
        "Q2_K_XL",
        "Q5_K_XL",
        "Q6_K_XL",
        "Q8_K_XL",
        // bartowski's "_L" variants (embedding / output weights kept at Q8_0).
        "Q6_K_L",
        "Q5_K_L",
        "Q4_K_L",
        "Q2_K_L",
        "IQ4_NL",
        "IQ4_XS",
        "IQ3_XXS",
        "IQ3_XS",
        "IQ3_M",
        "IQ3_S",
        "IQ2_XXS",
        "IQ2_XS",
        "IQ2_M",
        "IQ2_S",
        "IQ1_M",
        "IQ1_S",
        "TQ1_0",
        "TQ2_0",
        "Q4_K_M",
        "Q4_K_S",
        "Q5_K_M",
        "Q5_K_S",
        "Q3_K_M",
        "Q3_K_L",
        "Q3_K_S",
        "Q2_K_S",
        "Q2_K",
        // Plain k-quants (cstr naming: `...-q4_k.gguf`) after every suffixed variant.
        "Q3_K",
        "Q4_K",
        "Q5_K",
        "Q6_K",
        "Q8_0",
        // ARM repacked Q4_0 (older llama.cpp builds; now repacked at load time).
        "Q4_0_4_4",
        "Q4_0_4_8",
        "Q4_0_8_8",
        "Q4_0",
        "Q4_1",
        "Q5_0",
        "Q5_1",
        "BF16",
        "FP16",
        "F16",
        "F32",
    ];
    for q in quants {
        if upper.contains(q) {
            return q.trim_start_matches("UD-").to_string();
        }
    }
    "Unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lowercase_plain_quant_suffixes_are_parsed() {
        for (name, quant) in [
            ("data2vec-audio-base-960h-q4_k.gguf", "Q4_K"),
            ("model-q5_k.gguf", "Q5_K"),
            ("model-q3_k.gguf", "Q3_K"),
            ("model-q8_0.gguf", "Q8_0"),
            ("model-f16.gguf", "F16"),
            ("model-q4_k_m.gguf", "Q4_K_M"),
        ] {
            assert_eq!(extract_quant_from_filename(name), quant, "{name}");
        }
    }

    #[test]
    fn prismml_fork_quants_are_parsed() {
        assert_eq!(
            extract_quant_from_filename("Ternary-Bonsai-2-27B-PQ2_0.gguf"),
            "PQ2_0"
        );
        assert_eq!(
            extract_quant_from_filename("Ternary-Bonsai-2-27B-PTQ1_0.gguf"),
            "PTQ1_0"
        );
        assert_eq!(extract_quant_from_filename("model-TQ1_0.gguf"), "TQ1_0");
    }

    #[test]
    fn test_extract_quant_from_filename() {
        assert_eq!(
            extract_quant_from_filename("mistral-7b-instruct-v0.2.Q4_K_M.gguf"),
            "Q4_K_M"
        );
        assert_eq!(
            extract_quant_from_filename("llama-3-8b-instruct.Q8_0.gguf"),
            "Q8_0"
        );
        assert_eq!(
            extract_quant_from_filename("model-00001-of-00004.q5_k_s.gguf"),
            "Q5_K_S"
        );
        assert_eq!(
            extract_quant_from_filename("Qwen3-30B-A3B-UD-Q4_K_XL.gguf"),
            "Q4_K_XL"
        );
        assert_eq!(extract_quant_from_filename("model-IQ3_XXS.gguf"), "IQ3_XXS");
        // Real bartowski/SmolLM2-135M-Instruct-GGUF names.
        for (file, quant) in [
            ("SmolLM2-135M-Instruct-Q6_K_L.gguf", "Q6_K_L"),
            ("SmolLM2-135M-Instruct-Q6_K.gguf", "Q6_K"),
            ("SmolLM2-135M-Instruct-Q5_K_L.gguf", "Q5_K_L"),
            ("SmolLM2-135M-Instruct-Q4_K_L.gguf", "Q4_K_L"),
            ("SmolLM2-135M-Instruct-Q3_K_XL.gguf", "Q3_K_XL"),
            ("SmolLM2-135M-Instruct-Q4_0_4_4.gguf", "Q4_0_4_4"),
            ("SmolLM2-135M-Instruct-Q4_0.gguf", "Q4_0"),
            ("SmolLM2-135M-Instruct-f16.gguf", "F16"),
        ] {
            assert_eq!(extract_quant_from_filename(file), quant, "{file}");
        }
    }

    #[test]
    fn test_file_type_mapping() {
        assert_eq!(file_type_to_quantization(15), "Q4_K_M");
        assert_eq!(file_type_to_quantization(7), "Q8_0");
        assert_eq!(file_type_to_quantization(1), "F16");
        assert_eq!(file_type_to_quantization(28), "IQ2_S");
        assert_eq!(file_type_to_quantization(32), "BF16");
        assert_eq!(file_type_to_quantization(30), "IQ4_XS");
    }

    fn write_kv_str(f: &mut File, k: &str, v: &str) {
        f.write_all(&(k.len() as u64).to_le_bytes()).unwrap();
        f.write_all(k.as_bytes()).unwrap();
        f.write_all(&8u32.to_le_bytes()).unwrap();
        f.write_all(&(v.len() as u64).to_le_bytes()).unwrap();
        f.write_all(v.as_bytes()).unwrap();
    }

    fn write_kv_u32(f: &mut File, k: &str, v: u32) {
        f.write_all(&(k.len() as u64).to_le_bytes()).unwrap();
        f.write_all(k.as_bytes()).unwrap();
        f.write_all(&4u32.to_le_bytes()).unwrap();
        f.write_all(&v.to_le_bytes()).unwrap();
    }

    fn write_tensor(f: &mut File, name: &str, elements: u64, ggml_type: u32, offset: u64) {
        f.write_all(&(name.len() as u64).to_le_bytes()).unwrap();
        f.write_all(name.as_bytes()).unwrap();
        f.write_all(&1u32.to_le_bytes()).unwrap();
        f.write_all(&elements.to_le_bytes()).unwrap();
        f.write_all(&ggml_type.to_le_bytes()).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
    }

    fn q2_layout_file(dir: &Path, name: &str, q2_span: u64) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&3u32.to_le_bytes()).unwrap();
        f.write_all(&2u64.to_le_bytes()).unwrap();
        f.write_all(&2u64.to_le_bytes()).unwrap();
        write_kv_str(&mut f, "general.architecture", "llama");
        write_kv_u32(&mut f, "general.alignment", 32);
        write_tensor(&mut f, "blk.0.attn_q.weight", 4096, 42, 0);
        write_tensor(&mut f, "output_norm.weight", 8, 0, q2_span);
        let pos = f.stream_position().unwrap();
        let data_start = pos.div_ceil(32) * 32;
        f.write_all(&vec![0; (data_start - pos + q2_span + 32) as usize])
            .unwrap();
        path
    }

    #[test]
    fn audio_capability_metadata() {
        for (key, value, expected) in [
            ("clip.projector_type", "ultravox", true),
            ("clip.projector_type", "voxtral", true),
            ("clip.projector_type", "qwen2a", true),
            ("clip.projector_type", "qwen25o", true),
            ("clip.projector_type", "lfm2a", true),
            ("clip.audio.projector_type", "qwen3a", true),
            ("clip.audio.num_mel_bins", "", true),
            ("clip.projector_type", "mlp", false),
            ("clip.gen.audio.projector_type", "lfm2a", false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("projector.gguf");
            let mut f = File::create(&path).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
            f.write_all(&2u64.to_le_bytes()).unwrap();
            write_kv_str(&mut f, "general.architecture", "clip");
            if value.is_empty() {
                write_kv_u32(&mut f, key, 80);
            } else {
                write_kv_str(&mut f, key, value);
            }
            drop(f);
            let meta = read_gguf_metadata(&path).unwrap();
            assert!(meta.is_mmproj);
            assert_eq!(meta.has_audio_encoder, expected, "{key}: {value}");
            assert!(!meta.has_vision_encoder);
        }
        // Explicit flags win over hints, irrespective of key order.
        for flag in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("mixed.gguf");
            let mut f = File::create(&path).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
            f.write_all(&4u64.to_le_bytes()).unwrap();
            write_kv_str(&mut f, "general.architecture", "clip");
            write_kv_str(&mut f, "clip.projector_type", "qwen25o");
            for (key, v) in [
                ("clip.has_audio_encoder", flag),
                ("clip.has_vision_encoder", true),
            ] {
                f.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
                f.write_all(key.as_bytes()).unwrap();
                f.write_all(&7u32.to_le_bytes()).unwrap();
                f.write_all(&[u8::from(v)]).unwrap();
            }
            drop(f);
            let meta = read_gguf_metadata(&path).unwrap();
            assert_eq!(meta.has_audio_encoder, flag);
            assert!(meta.has_vision_encoder);
        }
    }

    #[test]
    fn test_gguf_header_parser() {
        let dir = tempfile::tempdir().unwrap();
        let gguf_file = dir.path().join("test_model.gguf");
        {
            let mut f = File::create(&gguf_file).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&100u64.to_le_bytes()).unwrap();
            f.write_all(&7u64.to_le_bytes()).unwrap();
            write_kv_str(&mut f, "general.architecture", "llama");
            write_kv_u32(&mut f, "llama.context_length", 8192);
            // A vocab-like string array that must be skipped correctly.
            let key = "tokenizer.ggml.tokens";
            f.write_all(&(key.len() as u64).to_le_bytes()).unwrap();
            f.write_all(key.as_bytes()).unwrap();
            f.write_all(&9u32.to_le_bytes()).unwrap();
            f.write_all(&8u32.to_le_bytes()).unwrap();
            f.write_all(&3u64.to_le_bytes()).unwrap();
            for tok in ["a", "bb", "ccc"] {
                f.write_all(&(tok.len() as u64).to_le_bytes()).unwrap();
                f.write_all(tok.as_bytes()).unwrap();
            }
            write_kv_u32(&mut f, "llama.block_count", 32);
            write_kv_u32(&mut f, "clip.vision.block_count", 24);
            write_kv_u32(&mut f, "llama.attention.head_count_kv", 8);
            write_kv_str(
                &mut f,
                "tokenizer.chat_template",
                "{% if tools %}{{ tools }}{% endif %}",
            );
        }

        let meta = read_gguf_metadata(&gguf_file).unwrap();
        assert_eq!(meta.version, 3);
        assert_eq!(meta.tensor_count, 100);
        assert_eq!(meta.architecture.as_deref(), Some("llama"));
        assert_eq!(meta.context_length, Some(8192));
        assert_eq!(meta.block_count, Some(32));
        assert_eq!(meta.head_count_kv, Some(8));
        assert!(meta.has_chat_template);
        assert!(meta.template_supports_tools);
        assert!(!meta.is_mmproj);
        assert_eq!(
            read_gguf_chat_template(&gguf_file).unwrap().as_deref(),
            Some("{% if tools %}{{ tools }}{% endif %}")
        );
    }

    #[test]
    fn chat_template_is_none_when_the_file_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let gguf_file = dir.path().join("plain.gguf");
        let mut f = File::create(&gguf_file).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&3u32.to_le_bytes()).unwrap();
        f.write_all(&0u64.to_le_bytes()).unwrap();
        f.write_all(&1u64.to_le_bytes()).unwrap();
        write_kv_str(&mut f, "general.architecture", "llama");
        drop(f);
        assert_eq!(read_gguf_chat_template(&gguf_file).unwrap(), None);
    }

    #[test]
    fn distinguishes_legacy_prism_and_official_q2_0_by_tensor_spans() {
        let dir = tempfile::tempdir().unwrap();
        // 4096 weights: official group-64 = 64 * 18 = 1152 bytes; legacy Prism group-128
        // = 32 * 34 = 1088 bytes. Both are already aligned to 32 bytes.
        let official = q2_layout_file(dir.path(), "official-Q2_0.gguf", 1152);
        let legacy = q2_layout_file(dir.path(), "legacy-Q2_0.gguf", 1088);
        let official = read_gguf_metadata(&official).unwrap();
        let legacy = read_gguf_metadata(&legacy).unwrap();
        assert_eq!(official.tensor_types, [0, 42]);
        assert!(!official.legacy_prism_q2_0);
        assert!(legacy.legacy_prism_q2_0);
    }

    /// Real-model check: HS_TEST_MODEL=<gguf> cargo test real_gguf -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_real_gguf() {
        let Ok(p) = std::env::var("HS_TEST_MODEL") else {
            return;
        };
        let t = std::time::Instant::now();
        let meta = read_gguf_metadata(Path::new(&p)).unwrap();
        println!("{:?} in {:?}", meta, t.elapsed());
        assert!(meta.block_count.is_some());
    }
    /// Native decision models carry `<arch>.decision.type` (keys of ggml-org/vllm-d3-edge-GGUF
    /// and vllm-sr/Decision-2.0 conversions); a key with a longer prefix is not it.
    #[test]
    fn decision_type_is_read_from_the_architecture_key() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, kvs: &[(&str, &str)]| {
            let path = dir.path().join(name);
            let mut f = File::create(&path).unwrap();
            f.write_all(b"GGUF").unwrap();
            f.write_all(&3u32.to_le_bytes()).unwrap();
            f.write_all(&0u64.to_le_bytes()).unwrap();
            f.write_all(&(kvs.len() as u64).to_le_bytes()).unwrap();
            for (k, v) in kvs {
                write_kv_str(&mut f, k, v);
            }
            path
        };
        let edge = file(
            "edge.gguf",
            &[
                ("general.architecture", "qwen35"),
                ("general.type", "model"),
                ("qwen35.decision.type", "vllm-d3"),
            ],
        );
        assert_eq!(
            read_gguf_metadata(&edge).unwrap().decision_type.as_deref(),
            Some("vllm-d3")
        );
        let chat = file(
            "chat.gguf",
            &[
                ("general.architecture", "qwen3"),
                ("clip.vision.decision.type", "x"),
            ],
        );
        assert_eq!(read_gguf_metadata(&chat).unwrap().decision_type, None);
    }
}
