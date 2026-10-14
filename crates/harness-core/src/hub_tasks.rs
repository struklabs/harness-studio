//! Model Hub tasks (Hugging Face `pipeline_tag`) and how far this machine's runtime runs them.
//!
//! Two jobs:
//!
//! * [`analyze_repo`] / [`infer_task`]: many GGUF re-uploads (re-quantizations, `gguf-my-repo`
//!   outputs) carry no `pipeline_tag`, so HF's `pipeline_tag=` filter drops them. The listing
//!   already returns each repo's tags, file names, `library_name` and `gguf` header block
//!   (`expand[]`), so the task is inferred from those without any extra request:
//!   `conversational` / `text-generation-inference` / `base_model:quantized:` tags (LLM),
//!   embedding / sentence-transformers tags and names, reranker tags and names, a multimodal
//!   projector file or vision tags and names (VLM), and, when nothing else says, the GGUF
//!   architecture (encoder = embedding, any other = language model). A repo with no `.gguf`
//!   file, a LoRA adapter or an image generator is [`NotRunnable`] instead of a guess.
//! * [`task_support`]: the "Runs here / partial / not supported" table of the Hub task picker.
//!   The baseline is what Harness can do with llama-server at all; the installed runtime's
//!   capability registry ([`crate::capabilities`]) then lowers it when the binary lacks the flag a
//!   task needs (`--embeddings`, `--rerank`, `--mmproj`) and marks it verified when a real run
//!   answered the endpoint.

use serde::{Deserialize, Serialize};

use crate::capabilities::features::{CapabilityLevel, FeatureStatus};
use crate::capabilities::RuntimeCapabilities;
use crate::hub_compat::{ONNX_FAMILY, SDCPP_FAMILY, SHERPA_FAMILY, WHISPER_FAMILY};

/// Tasks [`infer_task`] can produce; the Hub adds untagged repos to a listing only for these.
pub const INFERABLE: &[&str] = &[
    "text-generation",
    "text-to-image",
    "feature-extraction",
    "text-ranking",
    "image-text-to-text",
];

/// Every `pipeline_tag` the Hub picker offers (kept in step with `hub/tasks.ts` by a test).
pub const KNOWN_TASKS: &[&str] = &[
    "image-text-to-text",
    "audio-text-to-text",
    "visual-question-answering",
    "any-to-any",
    "document-question-answering",
    "video-text-to-text",
    "text-generation",
    "feature-extraction",
    "sentence-similarity",
    "text-ranking",
    "translation",
    "summarization",
    "question-answering",
    "text-classification",
    "token-classification",
    "zero-shot-classification",
    "fill-mask",
    "table-question-answering",
    "automatic-speech-recognition",
    "text-to-speech",
    "text-to-audio",
    "audio-to-audio",
    "audio-classification",
    "image-to-text",
    "text-to-image",
    "image-to-image",
    "text-to-video",
    "image-to-video",
    "image-classification",
    "object-detection",
    "image-segmentation",
    "depth-estimation",
    "zero-shot-image-classification",
];

/// Lower-case alphanumeric words of a repo name (`Qwen3-VL-8B_Instruct` -> qwen3, vl, 8b, ...).
fn name_tokens(repo_id: &str) -> Vec<String> {
    repo_id
        .rsplit('/')
        .next()
        .unwrap_or(repo_id)
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// What the Hub's `gguf` metadata block says about a repo (HF reads it from the first GGUF
/// header; `chat_template` is only present when the file has one).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GgufHints {
    pub architecture: Option<String>,
    /// `false` for encoder-only models (BERT family); absent for most decoders.
    pub causal: Option<bool>,
    pub has_chat_template: bool,
}

impl GgufHints {
    /// From the `gguf` object of a model JSON; `None` when the repo has none.
    pub fn from_json(v: Option<&serde_json::Value>) -> Option<Self> {
        let g = v?.as_object()?;
        Some(Self {
            architecture: g
                .get("architecture")
                .and_then(|a| a.as_str())
                .filter(|a| !a.is_empty())
                .map(str::to_lowercase),
            causal: g.get("causal").and_then(|c| c.as_bool()),
            has_chat_template: g
                .get("chat_template")
                .and_then(|c| c.as_str())
                .is_some_and(|c| !c.trim().is_empty()),
        })
    }
}

/// Encoder-only architectures llama.cpp serves as embedding (or, with a head, rerank) models.
const ENCODER_ARCHS: &[&str] = &[
    "bert",
    "nomic-bert",
    "nomic-bert-moe",
    "jina-bert-v2",
    "jina-bert-v3",
    "modern-bert",
    "neo-bert",
    "eurobert",
];

/// GGUF `general.architecture` values of image / video generators (the ComfyUI-GGUF set); not
/// language models, llama-server cannot load them.
const NON_LLM_ARCHS: &[&str] = &[
    "flux",
    "sd1",
    "sd2",
    "sd3",
    "sdxl",
    "aura",
    "ltxv",
    "hyvid",
    "wan",
    "hidream",
    "cosmos",
    "lumina2",
    "qwen_image",
];

/// How a task was found when the repo declares none (shown to the user as "tahmin").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSource {
    /// A tag (task-name tag, `reranker`, `conversational`, ...).
    Tags,
    /// A word of the repo name.
    Name,
    /// A multimodal projector file.
    Files,
    /// The README front matter's `library_name` (`sentence-transformers`).
    Library,
    /// The GGUF header the Hub parsed (`architecture`, `causal`).
    Gguf,
}

/// Why llama-server cannot run a repo, whatever its task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotRunnable {
    /// The repo lists files but none is a `.gguf`.
    NoGguf,
    /// A LoRA adapter: loads on top of a base model, not on its own.
    Adapter,
    /// An image / video generator or diffusion checkpoint.
    NotLanguageModel,
}

/// Everything the Hub shows about a repo's task beyond the listing's own fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoAnalysis {
    /// `pipeline_tag` from the model JSON, else from the README front matter (`cardData`).
    pub pipeline_tag: Option<String>,
    /// Only when `pipeline_tag` is `None`, and never for a repo that is not runnable.
    pub inferred_task: Option<&'static str>,
    pub source: Option<TaskSource>,
    pub not_runnable: Option<NotRunnable>,
    /// A LoRA adapter for a diffusion model: no Harness runtime loads it (an LLM GGUF LoRA is
    /// an adapter too, but llama-server takes it next to its base model).
    pub diffusion_adapter: bool,
    /// The base model a LoRA adapter is meant for ([`adapter_base_model`]).
    pub adapter_base: Option<String>,
}

/// The base model of a LoRA adapter repo from its `base_model:` tags: the `adapter:` relation
/// first, else a plain `base_model:<id>` (not `quantized:` / `finetune:` relations).
pub fn adapter_base_model(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|t| t.strip_prefix("base_model:adapter:"))
        .or_else(|| {
            tags.iter()
                .filter_map(|t| t.strip_prefix("base_model:"))
                .find(|m| !m.contains(':'))
        })
        .map(String::from)
}

fn str_list(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn gguf_file_names(files: &[String]) -> Vec<String> {
    files
        .iter()
        .map(|f| f.rsplit('/').next().unwrap_or(f).to_lowercase())
        .filter(|f| f.ends_with(".gguf"))
        .collect()
}

/// Reads a model JSON (one listing item or `/api/models/<id>`: `siblings`, `tags`, `gguf`,
/// `library_name`, `cardData`) into a task verdict.
///
/// Signals, strongest first: the declared `pipeline_tag` (JSON, else README front matter), then
/// tags / file names / name words, the front matter's `library_name`, and finally the GGUF header
/// (encoder architecture means embedding, any other architecture a language model). A repo that
/// lists files but no `.gguf`, a LoRA adapter, and image generators are marked not runnable
/// instead of getting a guess.
pub fn analyze_repo(item: &serde_json::Value) -> RepoAnalysis {
    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let card = item.get("cardData");
    let text = |key: &str| {
        item.get(key)
            .and_then(|v| v.as_str())
            .or_else(|| card.and_then(|c| c.get(key)).and_then(|v| v.as_str()))
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    let mut pipeline_tag = text("pipeline_tag");
    let library = text("library_name");
    let mut tags = str_list(item.get("tags"));
    for t in str_list(card.and_then(|c| c.get("tags"))) {
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    let files: Vec<String> = item
        .get("siblings")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|f| f.get("rfilename").and_then(|v| v.as_str()))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let gguf = GgufHints::from_json(item.get("gguf"));
    // An upstream text / vision `pipeline_tag` on a repo that is plainly a diffusion model is a
    // wrong tag (calcuis/sd3.5-large-turbo is tagged `image-to-text`).
    if pipeline_tag
        .as_deref()
        .is_some_and(|p| WRONG_FOR_DIFFUSION.contains(&p))
        && strong_diffusion_evidence(id, &tags, &files, gguf.as_ref())
    {
        pipeline_tag = Some("text-to-image".to_string());
    }
    let not_runnable = not_runnable(id, &tags, &files, library.as_deref(), gguf.as_ref());
    let found = if pipeline_tag.is_none()
        && item
            .get("config")
            .is_some_and(crate::hub_compat::is_ctc_config)
    {
        Some(("automatic-speech-recognition", TaskSource::Files))
    } else {
        match (&pipeline_tag, not_runnable) {
            (None, None) => infer(id, &tags, &files, library.as_deref(), gguf.as_ref()),
            // A diffusion header (`flux`, `sdxl`...) is "not a language model" for llama-server, but
            // an untagged repo of one is still a text-to-image model for stable-diffusion.cpp.
            (None, Some(NotRunnable::NotLanguageModel)) => {
                infer(id, &tags, &files, library.as_deref(), gguf.as_ref())
                    .filter(|(t, _)| *t == "text-to-image")
            }
            _ => None,
        }
    };
    let adapter = not_runnable == Some(NotRunnable::Adapter);
    RepoAnalysis {
        diffusion_adapter: adapter
            && is_diffusion_adapter(id, &tags, &files, library.as_deref(), gguf.as_ref()),
        adapter_base: adapter.then(|| adapter_base_model(&tags)).flatten(),
        pipeline_tag,
        inferred_task: found.map(|(t, _)| t),
        source: found.map(|(_, s)| s),
        not_runnable,
    }
}

fn not_runnable(
    repo_id: &str,
    tags: &[String],
    files: &[String],
    library: Option<&str>,
    gguf: Option<&GgufHints>,
) -> Option<NotRunnable> {
    let ggufs = gguf_file_names(files);
    if is_diffusion_adapter(repo_id, tags, files, library, gguf) {
        return Some(NotRunnable::Adapter);
    }
    // File names are only known when the response carried `siblings`.
    // A repo of whisper.cpp models (`ggml-*.bin`) is run by whisper-server, not llama-server.
    let whisper = files.iter().any(|f| {
        let name = f.rsplit('/').next().unwrap_or(f).to_ascii_lowercase();
        name.starts_with("ggml-") && name.ends_with(".bin")
    });
    if !files.is_empty() && ggufs.is_empty() && !whisper {
        return Some(NotRunnable::NoGguf);
    }
    let lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    if lower.iter().any(|t| t == "gguf-my-lora")
        || (name_tokens(repo_id).iter().any(|t| t == "lora")
            && !ggufs.is_empty()
            && ggufs.iter().all(|f| f.contains("lora")))
    {
        return Some(NotRunnable::Adapter);
    }
    if lower.iter().any(|t| t.starts_with("diffusion"))
        || library.is_some_and(|l| l.starts_with("diffusion"))
        || gguf
            .and_then(|g| g.architecture.as_deref())
            .is_some_and(|a| NON_LLM_ARCHS.contains(&a))
    {
        return Some(NotRunnable::NotLanguageModel);
    }
    None
}

/// Declared tasks that a diffusion repo can carry by mistake.
const WRONG_FOR_DIFFUSION: &[&str] = &[
    "image-to-text",
    "text-generation",
    "image-text-to-text",
    "visual-question-answering",
    "any-to-any",
    "text2text-generation",
];

/// Whether a `base_model:` tag (`base_model:quantized:stabilityai/stable-diffusion-3.5-large-turbo`)
/// names a known diffusion model.
fn diffusion_base_model(tags: &[String]) -> Option<String> {
    tags.iter().find_map(|t| {
        let lower = t.to_lowercase();
        let model = lower.strip_prefix("base_model:")?;
        let model = model.rsplit(':').next().unwrap_or(model);
        let name = model.rsplit('/').next().unwrap_or(model);
        let known = model.starts_with("stabilityai/stable-diffusion")
            || model.starts_with("stabilityai/sd")
            || model.starts_with("black-forest-labs/flux")
            || name.contains("stable-diffusion")
            || name.contains("stable_diffusion")
            || name.starts_with("sdxl")
            || name.starts_with("sd3")
            || name.starts_with("sd-turbo")
            || name.starts_with("sdxl-turbo")
            || name.starts_with("sd_xl")
            || name.starts_with("flux")
            || name.starts_with("z-image")
            || name.starts_with("qwen-image");
        known.then(|| model.to_string())
    })
}

/// Evidence strong enough to overrule a contradicting text / vision `pipeline_tag`: diffusion
/// tags, a diffusion base model, a diffusion GGUF header or image-bundle file names; never a
/// repo with a multimodal projector or a language-model header, and never the repo name alone.
fn strong_diffusion_evidence(
    repo_id: &str,
    tags: &[String],
    files: &[String],
    gguf: Option<&GgufHints>,
) -> bool {
    let has_projector = files.iter().any(|f| f.to_lowercase().contains("mmproj"));
    let arch = gguf.and_then(|g| g.architecture.as_deref());
    if has_projector || arch.is_some_and(|a| !NON_LLM_ARCHS.contains(&a)) {
        return false;
    }
    let lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let tokens = name_tokens(repo_id);
    diffusion_base_model(&lower).is_some()
        || diffusion_source(&lower, &tokens, repo_id, files, arch)
            .is_some_and(|s| s != TaskSource::Name)
}

/// A LoRA adapter for a diffusion model (diffusers or ComfyUI): it loads on top of a base
/// checkpoint, so alone it runs nowhere.
fn is_diffusion_adapter(
    repo_id: &str,
    tags: &[String],
    files: &[String],
    library: Option<&str>,
    gguf: Option<&GgufHints>,
) -> bool {
    let lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let lora = lower.iter().any(|t| {
        t == "lora" || t == "template:diffusion-lora" || t.starts_with("base_model:adapter:")
    });
    if !lora || files.iter().any(|f| f.to_lowercase().contains("mmproj")) {
        return false;
    }
    let arch = gguf.and_then(|g| g.architecture.as_deref());
    if arch.is_some_and(|a| !NON_LLM_ARCHS.contains(&a)) {
        return false;
    }
    let tokens = name_tokens(repo_id);
    diffusion_base_model(&lower).is_some()
        || library.is_some_and(|l| l.starts_with("diffusers"))
        || lower.iter().any(|t| t == "template:diffusion-lora")
        || diffusion_source(&lower, &tokens, repo_id, files, arch).is_some()
}

/// The task of a repo that has no `pipeline_tag`, from its tags, file names and name. `None`
/// when nothing points to one (the Hub then simply does not know the task).
///
/// Only the listing's own fields; [`analyze_repo`] adds the GGUF header and the front matter.
pub fn infer_task(repo_id: &str, tags: &[String], files: &[String]) -> Option<&'static str> {
    infer(repo_id, tags, files, None, None).map(|(t, _)| t)
}

/// Tags that name an image generator.
const DIFFUSION_TAGS: &[&str] = &[
    "stable-diffusion",
    "stable-diffusion-xl",
    "stable-diffusion-cpp",
    "stable-diffusion.cpp",
    "sd.cpp",
    "sdcpp",
    "diffusers",
    "sdxl",
    "sd3",
    "sd-turbo",
    "sdxl-turbo",
    "flux",
    "flux.1",
    "z-image",
    "qwen-image",
];

/// Whether a repo is an image generator, from tags, the GGUF architecture, GGUF file names (the
/// curated image bundles of [`crate::image_bundles`] plus a few families) and the repo name.
fn diffusion_source(
    tags: &[String],
    tokens: &[String],
    repo_id: &str,
    files: &[String],
    arch: Option<&str>,
) -> Option<TaskSource> {
    if tags.iter().any(|t| DIFFUSION_TAGS.contains(&t.as_str())) {
        return Some(TaskSource::Tags);
    }
    if arch.is_some_and(|a| {
        NON_LLM_ARCHS.contains(&a) && !matches!(a, "ltxv" | "hyvid" | "wan" | "cosmos")
    }) {
        return Some(TaskSource::Gguf);
    }
    let pair = |toks: &[String], a: &str, b: &str| toks.windows(2).any(|w| w[0] == a && w[1] == b);
    let by_words = |toks: &[String]| {
        toks.iter()
            .any(|t| matches!(t.as_str(), "sdxl" | "sd3" | "flux" | "sdcpp"))
            || pair(toks, "sd", "turbo")
            || pair(toks, "z", "image")
            || pair(toks, "qwen", "image")
            || pair(toks, "stable", "diffusion")
    };
    for f in files {
        let name = f.rsplit('/').next().unwrap_or(f).to_lowercase();
        if !name.ends_with(".gguf") {
            continue;
        }
        let stem: Vec<String> = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect();
        if crate::image_bundles::bundle_for(&name).is_some() || by_words(&stem) {
            return Some(TaskSource::Files);
        }
    }
    let repo_name = repo_id.rsplit('/').next().unwrap_or(repo_id);
    if crate::image_bundles::bundle_for(repo_name).is_some() || by_words(tokens) {
        return Some(TaskSource::Name);
    }
    None
}

/// Order matters: a reranker or embedding model also looks like a plain GGUF LLM upload
/// (`base_model:quantized:`), so the specific signals come first and the LLM rules last. A
/// reranker goes before the task-name tag because HF adds `feature-extraction` to every
/// BERT-type GGUF, rerankers included.
fn infer(
    repo_id: &str,
    tags: &[String],
    files: &[String],
    library: Option<&str>,
    gguf: Option<&GgufHints>,
) -> Option<(&'static str, TaskSource)> {
    let tags: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let has = |names: &[&str]| tags.iter().any(|t| names.contains(&t.as_str()));
    let tokens = name_tokens(repo_id);
    let token = |names: &[&str]| tokens.iter().any(|t| names.contains(&t.as_str()));
    let arch = gguf.and_then(|g| g.architecture.as_deref());

    if has(&["reranker", "reranking", "rerank", "cross-encoder"]) {
        return Some(("text-ranking", TaskSource::Tags));
    }
    if token(&["reranker", "rerank", "reranking"]) {
        return Some(("text-ranking", TaskSource::Name));
    }
    // A tag that already is a task name (HF adds the task as a tag when it knows it).
    if let Some(t) = KNOWN_TASKS.iter().find(|k| tags.iter().any(|t| t == **k)) {
        return Some((t, TaskSource::Tags));
    }
    if has(&[
        "sentence-transformers",
        "text-embeddings-inference",
        "embeddings",
        "embedding",
        "text-embedding",
        "sentence-embeddings",
    ]) {
        return Some(("feature-extraction", TaskSource::Tags));
    }
    if token(&["embed", "embedding", "embeddings"]) {
        return Some(("feature-extraction", TaskSource::Name));
    }
    if library == Some("sentence-transformers") {
        return Some(("feature-extraction", TaskSource::Library));
    }
    if has(&["music-generation", "text-to-music"]) {
        return Some(("text-to-audio", TaskSource::Tags));
    }
    if has(&["tts"]) {
        return Some(("text-to-speech", TaskSource::Tags));
    }
    // Diffusion / video checkpoints that happen to ship GGUFs are not language models.
    if tags.iter().any(|t| t.starts_with("diffusion")) || has(&["i2v", "t2v"]) {
        return None;
    }
    if let Some(source) = diffusion_source(&tags, &tokens, repo_id, files, arch) {
        return Some(("text-to-image", source));
    }
    let has_projector = files.iter().any(|f| {
        let name = f.rsplit('/').next().unwrap_or(f).to_lowercase();
        name.contains("mmproj")
    });
    if has_projector && token(&["ultravox", "voxtral", "omni", "audio", "asr"]) {
        // Audio-family names plus an actual projector companion: conditional chat audio support.
        return Some(("audio-text-to-text", TaskSource::Files));
    }
    if has_projector {
        return Some(("image-text-to-text", TaskSource::Files));
    }
    if has(&["vision", "multimodal", "vlm", "llava"]) {
        return Some(("image-text-to-text", TaskSource::Tags));
    }
    if token(&["vl", "vlm", "llava", "vision", "multimodal"]) {
        return Some(("image-text-to-text", TaskSource::Name));
    }
    // The header beats the weak LLM tags: an encoder is never a chat model.
    if gguf.is_some_and(|g| g.causal == Some(false))
        || arch.is_some_and(|a| ENCODER_ARCHS.contains(&a))
    {
        return Some(("feature-extraction", TaskSource::Gguf));
    }
    // A chat template in a decoder's header: an instruction-tuned language model.
    if arch.is_some() && gguf.is_some_and(|g| g.has_chat_template) {
        return Some(("text-generation", TaskSource::Gguf));
    }
    if has(&[
        "conversational",
        "text-generation-inference",
        "llama-cpp",
        "llama.cpp",
        "gguf-my-repo",
    ]) || tags.iter().any(|t| t.starts_with("base_model:quantized:"))
    {
        return Some(("text-generation", TaskSource::Tags));
    }
    // Any other architecture the header names is a decoder language model, with or without a
    // chat template (a base model still generates text).
    if arch.is_some() {
        return Some(("text-generation", TaskSource::Gguf));
    }
    None
}

// --- support table ---

/// How well a task runs. Ordered: `No < Partial < Yes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    No,
    Partial,
    Yes,
}

/// What a task needs from llama-server, and how far Harness takes it today.
struct Row {
    task: &'static str,
    /// What Harness can do with the task given a capable runtime.
    baseline: Support,
    /// Capability-registry feature id the task depends on (`None` = chat completion only).
    feature: Option<&'static str>,
    /// Other runtime families that run the task, best first: `(family, support when installed)`.
    /// They matter where llama-server cannot (baseline `No`, or its flag is missing) or runs the
    /// task only partly.
    alts: &'static [(&'static str, Support)],
}

const fn row(task: &'static str, baseline: Support, feature: Option<&'static str>) -> Row {
    Row {
        task,
        baseline,
        feature,
        alts: &[],
    }
}

const fn row_alt(
    task: &'static str,
    baseline: Support,
    feature: Option<&'static str>,
    alts: &'static [(&'static str, Support)],
) -> Row {
    Row {
        task,
        baseline,
        feature,
        alts,
    }
}

/// Speech recognition: whisper.cpp, sherpa-onnx, and exported HF CTC packages.
// The ONNX sidecar handles exported HF ForCTC packages only; other ASR graphs
// (Whisper encoder/decoder, transducers) are not covered by this implementation.
const ASR_ALTS: &[(&str, Support)] = &[
    (WHISPER_FAMILY, Yes),
    (SHERPA_FAMILY, Yes),
    (crate::engine::ONNX_FAMILY, Partial),
];
/// Text-to-speech: Kokoro / Piper through sherpa-onnx.
const TTS_ALTS: &[(&str, Support)] = &[(SHERPA_FAMILY, Yes)];
/// Encoder task heads (classification, NER, fill-mask, extractive QA) and vision heads
/// (image classification, detection, segmentation, depth): harness-onnx on `/hf`.
const ONNX_ALTS: &[(&str, Support)] = &[(ONNX_FAMILY, Yes)];
/// Text-to-image and image-to-image (`/v1/images/edits`): stable-diffusion.cpp.
const T2I_ALTS: &[(&str, Support)] = &[(SDCPP_FAMILY, Yes)];
/// Text / image to video: stable-diffusion.cpp's Wan through `/v1/videos`.
const VIDEO_ALTS: &[(&str, Support)] = &[(SDCPP_FAMILY, Yes)];

use Support::{No, Partial, Yes};

/// Baseline per task for llama-server. Multimodal rows need the projector flag; embeddings and
/// reranking their own server modes; the rest of NLP is prompted through chat completion
/// (`Partial`); speech and image / video generators belong to other runtime families (`alts`),
/// `No` while none of them is installed.
const TABLE: &[Row] = &[
    row("image-text-to-text", Yes, Some("vision")),
    row("audio-text-to-text", Partial, Some("audio_input")),
    row("visual-question-answering", Partial, Some("vision")),
    row("any-to-any", Partial, Some("vision")),
    row("document-question-answering", No, Some("vision")),
    row("video-text-to-text", No, Some("vision")),
    row("text-generation", Yes, None),
    row("feature-extraction", Yes, Some("embeddings")),
    row("sentence-similarity", Yes, Some("embeddings")),
    row("text-ranking", Yes, Some("rerank")),
    row("translation", Partial, None),
    row("summarization", Partial, None),
    row_alt("question-answering", Partial, None, ONNX_ALTS),
    row_alt("text-classification", No, None, ONNX_ALTS),
    row_alt("token-classification", No, None, ONNX_ALTS),
    row_alt("zero-shot-classification", No, None, ONNX_ALTS),
    row_alt("fill-mask", No, None, ONNX_ALTS),
    row("table-question-answering", No, None),
    row_alt(
        "automatic-speech-recognition",
        Partial,
        Some("audio_input"),
        ASR_ALTS,
    ),
    row_alt("text-to-speech", No, None, TTS_ALTS),
    row("text-to-audio", No, None),
    row("audio-to-audio", No, None),
    row("audio-classification", No, None),
    row("image-to-text", Partial, Some("vision")),
    row_alt("text-to-image", No, None, T2I_ALTS),
    row_alt("image-to-image", No, None, T2I_ALTS),
    row_alt("text-to-video", No, None, VIDEO_ALTS),
    row_alt("image-to-video", No, None, VIDEO_ALTS),
    row_alt("image-classification", No, None, ONNX_ALTS),
    row_alt("object-detection", No, None, ONNX_ALTS),
    row_alt("image-segmentation", No, None, ONNX_ALTS),
    row_alt("depth-estimation", No, None, ONNX_ALTS),
    row_alt("zero-shot-image-classification", No, None, ONNX_ALTS),
];

/// One task's support on the installed runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSupport {
    /// HF `pipeline_tag`.
    pub task: String,
    /// What the Hub shows.
    pub support: Support,
    /// Support without looking at the runtime.
    pub baseline: Support,
    /// Capability-registry feature the task depends on.
    pub feature: Option<String>,
    /// The runtime's capability registry was read (`false`: baseline only).
    pub runtime_checked: bool,
    /// The runtime lacks what the task needs, so it was lowered to `No`.
    pub runtime_missing: bool,
    /// A real run of this runtime answered the endpoint the task uses.
    pub verified: bool,
    /// Flag spelling that was found in the runtime (`--embeddings`).
    pub flag: Option<String>,
    /// Another installed runtime family serves the task (`support` already counts it).
    pub runtime: Option<String>,
    /// The task would run on a runtime family that is not installed (`support` does not count
    /// it); the UI offers to install it instead of saying the task cannot run.
    pub needs_runtime: Option<String>,
}

/// Support of every known task. `caps`: capabilities of the installed runtime's binary, `None`
/// (or an unreadable `--help`) leaves the baseline. `features` = [`crate::capabilities::features::feature_status`]
/// of that runtime without a model. `installed`: families of the installed runtimes
/// ([`crate::model_registry::installed_families`]). The embedding / reranking rows stay
/// llama-server's (ONNX encoders are judged per repo by `hub_compat`, not per task).
pub fn task_support(
    caps: Option<&RuntimeCapabilities>,
    features: &[FeatureStatus],
    installed: &[String],
) -> Vec<TaskSupport> {
    let known = caps.is_some_and(|c| c.help_parsed);
    let has = |family: &str| installed.iter().any(|f| f == family);
    TABLE
        .iter()
        .map(|r| {
            let status = r
                .feature
                .and_then(|id| features.iter().find(|f| f.id == id));
            let missing = known
                && status.is_some_and(|f| f.level == CapabilityLevel::Unsupported)
                && r.baseline != No;
            let verified = known
                && status.is_some_and(|f| {
                    f.level == CapabilityLevel::EndpointVerified && f.available == Some(true)
                });
            let llama = if missing { No } else { r.baseline };
            // The best alternative family that beats llama-server: installed ones lift the
            // verdict, an absent one only asks for the install.
            let better = r.alts.iter().filter(|(_, s)| *s > llama);
            let alt = better.clone().find(|(f, _)| has(f));
            let wanted = better.clone().next();
            let (support, runtime, needs_runtime) = match (alt, wanted) {
                (Some((f, s)), _) => (*s, Some(f.to_string()), None),
                (None, Some((f, _))) => (llama, None, Some(f.to_string())),
                (None, None) => (llama, None, None),
            };
            TaskSupport {
                task: r.task.to_string(),
                support,
                baseline: r.baseline,
                feature: r.feature.map(str::to_string),
                runtime_checked: known && (r.feature.is_none() || status.is_some()),
                runtime_missing: missing,
                verified,
                flag: status.and_then(|f| f.flag.clone()),
                runtime,
                needs_runtime,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::features::{feature_status, FeatureContext};

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn audio_mmproj_tasks_are_conditional_chat_support() {
        let files = tags(&["Ultravox-Q8_0.gguf", "mmproj-Ultravox-F16.gguf"]);
        assert_eq!(
            infer_task("org/Ultravox-7B-GGUF", &[], &files),
            Some("audio-text-to-text")
        );
        let caps = crate::capabilities::testdata::caps("b11236");
        let features = feature_status(&caps, FeatureContext::default());
        let list = task_support(Some(&caps), &features, &[]);
        let row = support_of(&list, "audio-text-to-text");
        assert_eq!(row.support, Partial);
        assert_eq!(row.feature.as_deref(), Some("audio_input"));
        assert_eq!(row.flag.as_deref(), Some("--mmproj"));
        let asr = support_of(&list, "automatic-speech-recognition");
        assert_eq!(asr.feature.as_deref(), Some("audio_input"));
    }

    #[test]
    fn llm_requantization_without_pipeline_tag() {
        // Tags exactly as the HF listing returned them for unsloth/Qwen3.8-27B-GGUF.
        let t = tags(&[
            "gguf",
            "qwen3_5",
            "unsloth",
            "base_model:Qwen/Qwen3.8-27B",
            "base_model:quantized:Qwen/Qwen3.8-27B",
            "endpoints_compatible",
            "imatrix",
            "conversational",
        ]);
        assert_eq!(
            infer_task("lmstudio-community/Qwen3.8-27B-GGUF", &t, &[]),
            Some("text-generation")
        );
        // Only the quantization relation is known: still an LLM upload.
        let t = tags(&["gguf", "deepseek", "base_model:quantized:deepseek-ai/X"]);
        assert_eq!(
            infer_task("antirez/deepseek-v4.1-flash-gguf", &t, &[]),
            Some("text-generation")
        );
        // A bare GGUF folder gives no evidence.
        assert_eq!(
            infer_task(
                "ggml-org/models-moved",
                &tags(&["gguf", "endpoints_compatible"]),
                &[]
            ),
            None
        );
    }

    #[test]
    fn projector_file_or_vision_name_means_vlm() {
        let t = tags(&[
            "gguf",
            "conversational",
            "base_model:quantized:Qwen/Qwen3.8-27B",
        ]);
        let files = vec!["mmproj-BF16.gguf".to_string(), "m-Q4_K_M.gguf".to_string()];
        assert_eq!(
            infer_task("unsloth/Qwen3.8-27B-GGUF", &t, &files),
            Some("image-text-to-text")
        );
        assert_eq!(
            infer_task(
                "mradermacher/Qwen3-VL-8B-Instruct-abliterated-GGUF",
                &t,
                &[]
            ),
            Some("image-text-to-text")
        );
        // "vl" must be a word of its own: "Evolve" is not a VLM.
        assert_eq!(
            infer_task("x/Evolve-7B-GGUF", &t, &[]),
            Some("text-generation")
        );
    }

    #[test]
    fn embedding_and_reranker_beat_the_llm_rules() {
        let quant = tags(&["gguf", "base_model:quantized:BAAI/bge-m3", "conversational"]);
        assert_eq!(
            infer_task("x/nomic-embed-text-v1.5-GGUF", &quant, &[]),
            Some("feature-extraction")
        );
        assert_eq!(
            infer_task("gpustack/bge-reranker-v2-m3-GGUF", &quant, &[]),
            Some("text-ranking")
        );
        assert_eq!(
            infer_task("x/y", &tags(&["gguf", "sentence-transformers"]), &[]),
            Some("feature-extraction")
        );
    }

    #[test]
    fn non_language_checkpoints_stay_untyped_or_typed_by_tag() {
        // Real tags of DeepBeepMeep/Wan2.1 and Serveurperso/YuE2-GGUF.
        assert_eq!(
            infer_task(
                "DeepBeepMeep/Wan2.1",
                &tags(&[
                    "diffusion-single-file",
                    "onnx",
                    "gguf",
                    "i2v",
                    "base_model:quantized:Wan-AI/Wan2.1-T2V-1.3B"
                ]),
                &[]
            ),
            None
        );
        assert_eq!(
            infer_task(
                "Serveurperso/YuE2-GGUF",
                &tags(&["gguf", "music-generation", "text-to-music"]),
                &[]
            ),
            Some("text-to-audio")
        );
        // A task name as a tag wins.
        assert_eq!(
            infer_task("a/b", &tags(&["gguf", "text-to-image"]), &[]),
            Some("text-to-image")
        );
    }

    /// Model JSON of real repos, captured from `huggingface.co/api/models/<id>` on 2026-10-02
    /// (only the fields Harness reads; `chat_template` cut to 120 characters).
    fn repo(id: &str) -> serde_json::Value {
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../tests/data/hf-repo-signals.json")).unwrap();
        all[id].clone()
    }

    #[test]
    fn untagged_grab_bag_repo_is_read_from_the_gguf_header() {
        // ggml-org/models-moved: tags `gguf` only, no pipeline_tag, nothing in the name; the
        // header says `llama`.
        let a = analyze_repo(&repo("ggml-org/models-moved"));
        assert_eq!(a.pipeline_tag, None);
        assert_eq!(a.inferred_task, Some("text-generation"));
        assert_eq!(a.source, Some(TaskSource::Gguf));
        assert_eq!(a.not_runnable, None);
        let gpt2 = analyze_repo(&repo(
            "RichardErkhov/Josephgflowers_-_GPT2-774M-CINDER-SHOW-MULTI-CHAT-gguf",
        ));
        assert_eq!(gpt2.inferred_task, Some("text-generation"));
        assert_eq!(gpt2.source, Some(TaskSource::Gguf));
    }

    #[test]
    fn untagged_encoder_header_means_embedding() {
        // caliex/all-MiniLM-L6-v2-f16.gguf: no task tag, `architecture: bert`, `causal: false`.
        let a = analyze_repo(&repo("caliex/all-MiniLM-L6-v2-f16.gguf"));
        assert_eq!(a.inferred_task, Some("feature-extraction"));
        assert_eq!(a.source, Some(TaskSource::Gguf));
    }

    #[test]
    fn reranker_tag_beats_the_feature_extraction_tag_hf_adds_to_bert_ggufs() {
        // sinjab/ms-marco-MiniLM-L6-v2-Q8_0-GGUF carries both `reranker` and `feature-extraction`.
        let item = repo("sinjab/ms-marco-MiniLM-L6-v2-Q8_0-GGUF");
        assert!(item["tags"].to_string().contains("reranker"));
        let a = analyze_repo(&item);
        assert_eq!(a.inferred_task, Some("text-ranking"));
        assert_eq!(a.source, Some(TaskSource::Tags));
    }

    #[test]
    fn repo_without_gguf_files_is_not_runnable_instead_of_guessed() {
        // Qwen/Qwen3-0.6B lists safetensors only. The declared task stays; no inference.
        let a = analyze_repo(&repo("Qwen/Qwen3-0.6B"));
        assert_eq!(a.not_runnable, Some(NotRunnable::NoGguf));
        assert_eq!(a.pipeline_tag.as_deref(), Some("text-generation"));
        assert_eq!(a.inferred_task, None);
        // A response without `siblings` says nothing about files.
        let mut item = repo("ggml-org/models-moved");
        item.as_object_mut().unwrap().remove("siblings");
        assert_eq!(analyze_repo(&item).not_runnable, None);
    }

    #[test]
    fn adapters_and_image_models_are_not_runnable() {
        let lora = analyze_repo(&repo(
            "ggml-org/LoRA-Qwen2.5-1.5B-Instruct-abliterated-F16-GGUF",
        ));
        assert_eq!(lora.not_runnable, Some(NotRunnable::Adapter));
        assert_eq!(lora.inferred_task, None, "an adapter is not a text model");
        // DeepBeepMeep/MiniMax-H3: `library_name: diffusion-single-file`, header `qwen3vl`.
        let img = analyze_repo(&repo("DeepBeepMeep/MiniMax-H3"));
        assert_eq!(img.not_runnable, Some(NotRunnable::NotLanguageModel));
        assert_eq!(img.inferred_task, None);
    }

    #[test]
    fn non_llm_architecture_in_the_header_is_not_runnable() {
        // city96/FLUX.1-dev-gguf: header `architecture: flux`; the declared task stays.
        let a = analyze_repo(&repo("city96/FLUX.1-dev-gguf"));
        assert_eq!(a.not_runnable, Some(NotRunnable::NotLanguageModel));
        assert_eq!(a.pipeline_tag.as_deref(), Some("text-to-image"));
    }

    #[test]
    fn declared_task_wins_and_front_matter_fills_a_missing_one() {
        let item = repo("ggml-org/gemma-3-4b-it-GGUF");
        let a = analyze_repo(&item);
        assert_eq!(a.pipeline_tag.as_deref(), Some("image-text-to-text"));
        assert_eq!((a.inferred_task, a.not_runnable), (None, None));
        // The Hub's top-level field missing: the README front matter (`cardData`) still declares it.
        let mut no_top = item.clone();
        no_top.as_object_mut().unwrap().remove("pipeline_tag");
        let a = analyze_repo(&no_top);
        assert_eq!(a.pipeline_tag.as_deref(), Some("image-text-to-text"));
        assert_eq!(a.inferred_task, None);
    }

    #[test]
    fn gguf_hints_read_architecture_causal_and_template() {
        let g = serde_json::json!({"architecture": "Gemma3", "chat_template": "{{ x }}", "causal": true});
        assert_eq!(
            GgufHints::from_json(Some(&g)),
            Some(GgufHints {
                architecture: Some("gemma3".into()),
                causal: Some(true),
                has_chat_template: true
            })
        );
        assert_eq!(GgufHints::from_json(None), None);
        let blank = serde_json::json!({"chat_template": "  "});
        assert!(
            !GgufHints::from_json(Some(&blank))
                .unwrap()
                .has_chat_template
        );
    }

    #[test]
    fn known_tasks_match_the_picker_table() {
        let ids: Vec<&str> = TABLE.iter().map(|r| r.task).collect();
        assert_eq!(ids, KNOWN_TASKS);
        for t in INFERABLE {
            assert!(KNOWN_TASKS.contains(t));
        }
    }

    /// `src/lib/components/hub/tasks.ts` lists the picker's tasks; this table must cover the
    /// same ids so no chip is left without a verdict.
    #[test]
    fn picker_ids_are_all_in_the_table() {
        let ts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../src/lib/components/hub/tasks.ts");
        let text = std::fs::read_to_string(&ts).unwrap();
        let mut ids: Vec<String> = text
            .lines()
            .filter(|l| l.trim_start().starts_with("{ id: '") && l.contains("hf: '"))
            .filter_map(|l| {
                let rest = l.trim_start().strip_prefix("{ id: '")?;
                Some(rest.split('\'').next()?.to_string())
            })
            .collect();
        ids.sort();
        let mut table: Vec<String> = KNOWN_TASKS.iter().map(|s| s.to_string()).collect();
        table.sort();
        assert_eq!(ids, table);
    }

    fn caps_without(flags: &[&str]) -> RuntimeCapabilities {
        let mut caps = crate::capabilities::testdata::caps("b11236");
        caps.flags.retain(|k, _| !flags.contains(&k.as_str()));
        caps.reindex();
        caps
    }

    fn support_of(list: &[TaskSupport], task: &str) -> TaskSupport {
        list.iter().find(|t| t.task == task).cloned().unwrap()
    }

    #[test]
    fn real_runtime_keeps_the_baseline_and_a_missing_flag_lowers_it() {
        let caps = crate::capabilities::testdata::caps("b11236");
        let features = feature_status(&caps, FeatureContext::default());
        let list = task_support(Some(&caps), &features, &[]);
        assert_eq!(list.len(), KNOWN_TASKS.len());
        let emb = support_of(&list, "feature-extraction");
        assert_eq!(emb.support, Yes);
        assert!(emb.runtime_checked && !emb.runtime_missing);
        assert_eq!(emb.flag.as_deref(), Some("--embeddings"));
        assert_eq!(support_of(&list, "text-to-image").support, No);
        assert_eq!(support_of(&list, "translation").support, Partial);

        // A binary without the rerank / mmproj flags cannot run those tasks.
        let caps = caps_without(&["--rerank", "--mmproj"]);
        let features = feature_status(&caps, FeatureContext::default());
        let list = task_support(Some(&caps), &features, &[]);
        let rerank = support_of(&list, "text-ranking");
        assert_eq!(rerank.support, No);
        assert!(rerank.runtime_missing);
        assert_eq!(rerank.baseline, Yes);
        assert_eq!(support_of(&list, "image-text-to-text").support, No);
        assert_eq!(support_of(&list, "text-generation").support, Yes);
        // Already-unsupported tasks are not "missing" because of the binary.
        assert!(!support_of(&list, "text-to-image").runtime_missing);
    }

    #[test]
    fn unknown_runtime_leaves_the_baseline() {
        let list = task_support(None, &[], &[]);
        let rerank = support_of(&list, "text-ranking");
        assert_eq!(rerank.support, Yes);
        assert!(!rerank.runtime_checked && !rerank.runtime_missing && !rerank.verified);
    }

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn wrong_text_pipeline_tag_on_a_diffusion_gguf_is_overruled() {
        let item = serde_json::json!({
            "id": "calcuis/sd3.5-large-turbo",
            "pipeline_tag": "image-to-text",
            "tags": ["gguf", "stable-diffusion", "gguf-comfy", "image-to-text",
                "base_model:stabilityai/stable-diffusion-3.5-large-turbo",
                "base_model:quantized:stabilityai/stable-diffusion-3.5-large-turbo"],
            "siblings": [{"rfilename": "sd3.5_large_turbo-q4_0.gguf"}],
        });
        let a = analyze_repo(&item);
        assert_eq!(a.pipeline_tag.as_deref(), Some("text-to-image"));
        assert_ne!(a.not_runnable, Some(NotRunnable::Adapter));
        // A real VLM with a projector keeps its tag, whatever the base model says.
        let vlm = serde_json::json!({
            "id": "a/b-GGUF",
            "pipeline_tag": "image-to-text",
            "tags": ["gguf", "base_model:quantized:Qwen/Qwen2-VL-7B"],
            "siblings": [{"rfilename": "m.gguf"}, {"rfilename": "mmproj-f16.gguf"}],
        });
        assert_eq!(
            analyze_repo(&vlm).pipeline_tag.as_deref(),
            Some("image-to-text")
        );
        // An LLM header blocks the override even with a stray diffusion tag.
        let llm = serde_json::json!({
            "id": "a/b",
            "pipeline_tag": "text-generation",
            "tags": ["gguf", "diffusers"],
            "gguf": {"architecture": "llama"},
            "siblings": [{"rfilename": "m.gguf"}],
        });
        assert_eq!(
            analyze_repo(&llm).pipeline_tag.as_deref(),
            Some("text-generation")
        );
    }

    #[test]
    fn diffusion_lora_repos_are_adapters() {
        for (id, tag_list) in [
            (
                "prithivMLmods/SD3.5-Large-Turbo-HyperRealistic-LoRA",
                vec![
                    "diffusers",
                    "text-to-image",
                    "lora",
                    "template:diffusion-lora",
                    "base_model:stabilityai/stable-diffusion-3.5-large-turbo",
                    "base_model:adapter:stabilityai/stable-diffusion-3.5-large-turbo",
                ],
            ),
            (
                "shiroppo/sd_xl_turbo_lora",
                vec!["lora", "stable-diffusion-xl"],
            ),
            ("openskyml/lcm-lora-sdxl-turbo", vec!["diffusers", "lora"]),
            (
                "radames/sdxl-turbo-DPO-LoRA",
                vec!["diffusers", "lora", "text-to-image"],
            ),
        ] {
            let item = serde_json::json!({
                "id": id,
                "pipeline_tag": "text-to-image",
                "tags": tag_list,
                "siblings": [{"rfilename": "pytorch_lora_weights.safetensors"}],
            });
            assert_eq!(
                analyze_repo(&item).not_runnable,
                Some(NotRunnable::Adapter),
                "{id}"
            );
        }
        // A plain diffusion checkpoint is not an adapter.
        let ckpt = serde_json::json!({
            "id": "stabilityai/sdxl-turbo",
            "pipeline_tag": "text-to-image",
            "tags": ["diffusers", "text-to-image"],
            "siblings": [{"rfilename": "sd_xl_turbo_1.0.safetensors"}],
        });
        assert_ne!(analyze_repo(&ckpt).not_runnable, Some(NotRunnable::Adapter));
    }

    #[test]
    fn untagged_diffusion_ggufs_are_text_to_image() {
        // Green-Sky/SD-Turbo-GGUF: no pipeline_tag, files sd_turbo-*.gguf.
        let files = s(&["sd_turbo-f16-q8_0.gguf", "sd_turbo-f16-q4_0.gguf"]);
        let quant = tags(&["gguf", "base_model:quantized:stabilityai/sd-turbo"]);
        assert_eq!(
            infer_task("Green-Sky/SD-Turbo-GGUF", &quant, &files),
            Some("text-to-image")
        );
        // stduhpf/SD3.5-Large-Turbo-GGUF-mixed-sdcpp: by name alone, and by tag.
        assert_eq!(
            infer_task("stduhpf/SD3.5-Large-Turbo-GGUF-mixed-sdcpp", &quant, &[]),
            Some("text-to-image")
        );
        assert_eq!(
            infer_task("a/b", &tags(&["gguf", "stable-diffusion"]), &[]),
            Some("text-to-image")
        );
        for (repo, file) in [
            ("a/model-gguf", "flux1-dev-Q4_0.gguf"),
            ("a/z-image-turbo-gguf", "z_image_turbo-Q8_0.gguf"),
            ("a/qwen-image-gguf", "qwen-image-Q4_K_M.gguf"),
            ("a/x", "sdxl-base-1.0-q8_0.gguf"),
        ] {
            assert_eq!(
                infer_task(repo, &quant, &s(&[file])),
                Some("text-to-image"),
                "{repo} {file}"
            );
        }
        // The GGUF header of an untagged repo counts too (flux is a diffusion architecture).
        let item = serde_json::json!({
            "id": "a/b", "tags": ["gguf"], "gguf": {"architecture": "flux"},
            "siblings": [{"rfilename": "m-Q4_0.gguf"}]
        });
        let a = analyze_repo(&item);
        assert_eq!(a.inferred_task, Some("text-to-image"));
        assert_eq!(a.not_runnable, Some(NotRunnable::NotLanguageModel));
    }

    #[test]
    fn language_models_stay_text_generation_next_to_the_diffusion_rules() {
        let chat = tags(&["gguf", "conversational", "base_model:quantized:x/y"]);
        for (repo, file) in [
            (
                "bartowski/Llama-3.2-3B-Instruct-GGUF",
                "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
            ),
            ("unsloth/Qwen3-8B-GGUF", "Qwen3-8B-Q4_K_M.gguf"),
            ("x/Qwen2.5-Coder-7B-GGUF", "qwen2.5-coder-7b-q4_k_m.gguf"),
            ("x/Mistral-Turbo-7B-GGUF", "mistral-turbo-7b.gguf"),
            ("x/Diffusion-LM-GGUF", "model.gguf"),
        ] {
            assert_eq!(
                infer_task(repo, &chat, &s(&[file])),
                Some("text-generation"),
                "{repo}"
            );
        }
    }

    fn installed(list: &[&str]) -> Vec<String> {
        s(list)
    }

    #[test]
    fn onnx_asr_is_only_partial_and_never_endpoint_verified() {
        // Without llama audio input, the conservative CTC alternative is what remains.
        let caps = crate::capabilities::testdata::caps("b11236");
        let mut features = feature_status(&caps, FeatureContext::default());
        for f in features
            .iter_mut()
            .filter(|f| f.id == "vision" || f.id == "audio_input")
        {
            f.level = CapabilityLevel::Unsupported;
            f.available = Some(false);
        }
        let rows = task_support(
            Some(&caps),
            &features,
            &installed(&[crate::engine::ONNX_FAMILY]),
        );
        let asr = support_of(&rows, "automatic-speech-recognition");
        assert_eq!(asr.support, Partial);
        assert_eq!(asr.runtime.as_deref(), Some(crate::engine::ONNX_FAMILY));
        assert!(!asr.verified);
    }

    #[test]
    fn task_support_follows_the_installed_runtime_families() {
        let caps = crate::capabilities::testdata::caps("b11236");
        let features = feature_status(&caps, FeatureContext::default());
        let none = task_support(Some(&caps), &features, &[]);
        // Not installed: still "no", but the card is told which runtime to install.
        let t2i = support_of(&none, "text-to-image");
        assert_eq!(t2i.support, No);
        assert_eq!(t2i.needs_runtime.as_deref(), Some(SDCPP_FAMILY));
        assert_eq!(t2i.runtime, None);
        assert_eq!(
            support_of(&none, "text-to-speech").needs_runtime.as_deref(),
            Some(SHERPA_FAMILY)
        );
        // Encoder pipelines point at ONNX Runtime.
        assert_eq!(
            support_of(&none, "fill-mask").needs_runtime.as_deref(),
            Some(ONNX_FAMILY)
        );
        // Tasks nobody serves have neither.
        let table = support_of(&none, "table-question-answering");
        assert!(table.needs_runtime.is_none() && table.runtime.is_none());
        // Llama rows are untouched.
        let tg = support_of(&none, "text-generation");
        assert_eq!(
            (tg.support, tg.runtime, tg.needs_runtime),
            (Yes, None, None)
        );

        let sd = task_support(Some(&caps), &features, &installed(&[SDCPP_FAMILY]));
        let t2i = support_of(&sd, "text-to-image");
        assert_eq!(t2i.support, Yes);
        assert_eq!(t2i.runtime.as_deref(), Some(SDCPP_FAMILY));
        assert_eq!(t2i.needs_runtime, None);
        assert_eq!(support_of(&sd, "image-to-image").support, Yes);
        assert_eq!(support_of(&sd, "text-to-video").support, Yes);
        assert_eq!(support_of(&sd, "image-to-video").support, Yes);
        // The other families stay missing.
        assert_eq!(support_of(&sd, "text-to-speech").support, No);

        let speech = task_support(
            Some(&caps),
            &features,
            &installed(&[WHISPER_FAMILY, SHERPA_FAMILY]),
        );
        let asr = support_of(&speech, "automatic-speech-recognition");
        assert_eq!(
            (asr.support, asr.runtime.as_deref()),
            (Yes, Some(WHISPER_FAMILY))
        );
        let tts = support_of(&speech, "text-to-speech");
        assert_eq!(
            (tts.support, tts.runtime.as_deref()),
            (Yes, Some(SHERPA_FAMILY))
        );
        // ASR without whisper.cpp: llama's partial verdict stays, with the install hint.
        let asr = support_of(&none, "automatic-speech-recognition");
        assert_eq!(asr.support, Partial);
        assert_eq!(asr.needs_runtime.as_deref(), Some(WHISPER_FAMILY));
    }
}
