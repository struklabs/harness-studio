//! Layout of a Hugging Face ONNX encoder repo on disk and what it says about serving it.
//!
//! A downloaded repo keeps the Hub's folder layout:
//!
//! ```text
//! <repo>/onnx/model.onnx            (or model_quantized.onnx, model_fp16.onnx...)
//! <repo>/tokenizer.json
//! <repo>/config.json  tokenizer_config.json  sentence_bert_config.json
//! <repo>/modules.json  1_Pooling/config.json  2_Normalize/
//! ```
//!
//! [`ModelPackage::discover`] starts from the `.onnx` file and reads the rest the way
//! sentence-transformers does: the pooling mode from `1_Pooling/config.json` (the path comes
//! from `modules.json`), normalization from a `Normalize` module, the maximum sequence length
//! from `sentence_bert_config.json`, `tokenizer_config.json` and `config.json`. A repo without
//! sentence-transformers files falls back to what the model's `config.json` implies (a
//! `...ForSequenceClassification` head is a reranker, anything else an embedding model with mean
//! pooling), and every such guess can be overridden on the command line.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Largest sequence length served, whatever the model claims (8k-context embedders exist; a
/// longer window only costs memory for inputs nobody sends).
pub const MAX_SEQUENCE_LENGTH: usize = 8192;
/// Used when nothing in the repo names a length.
pub const DEFAULT_SEQUENCE_LENGTH: usize = 512;

/// What the model computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Text to vector (`/v1/embeddings`).
    Embedding,
    /// (query, document) to a relevance logit (`/v1/rerank`): a cross-encoder.
    Rerank,
    /// Text (or a text pair) to labels (`/v1/classify`); an NLI head (an `entailment` label)
    /// also answers `/v1/zero-shot-classify`.
    Classify,
    /// Labels per token, grouped into entities (`/v1/token-classify`): NER, POS.
    TokenClassify,
    /// The most likely tokens for a mask (`/v1/fill-mask`).
    FillMask,
    /// An answer span of a context (`/v1/question-answering`): extractive QA.
    QuestionAnswering,
    /// Raw audio to text through a CTC head.
    AsrCtc,
}

impl Task {
    pub fn as_str(self) -> &'static str {
        match self {
            Task::Embedding => "embedding",
            Task::Rerank => "rerank",
            Task::Classify => "classify",
            Task::TokenClassify => "token-classify",
            Task::FillMask => "fill-mask",
            Task::QuestionAnswering => "question-answering",
            Task::AsrCtc => "asr_ctc",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "embedding" | "embeddings" => Some(Task::Embedding),
            "rerank" | "reranker" | "ranking" | "text-ranking" => Some(Task::Rerank),
            "classify" | "classification" | "text-classification" | "zero-shot-classification" => {
                Some(Task::Classify)
            }
            "token-classify" | "token-classification" | "ner" => Some(Task::TokenClassify),
            "fill-mask" | "fill_mask" => Some(Task::FillMask),
            "question-answering" | "qa" => Some(Task::QuestionAnswering),
            "asr_ctc" => Some(Task::AsrCtc),
            _ => None,
        }
    }
}

/// How token vectors become one text vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pooling {
    /// Mean over the real (non-padding) tokens.
    Mean,
    /// The first token (`[CLS]` / `<s>`).
    Cls,
    /// Element-wise maximum over the real tokens.
    Max,
    /// The last real token (decoder embedders such as Qwen3-Embedding).
    LastToken,
    /// Sum over the real tokens divided by the square root of their count.
    MeanSqrtLen,
    /// Mean weighted by position (SGPT): token `i` (1-based) has weight `i`.
    WeightedMean,
}

impl Pooling {
    pub fn parse(s: &str) -> Option<Self> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_'], "")
            .as_str()
        {
            "mean" | "meantokens" => Some(Pooling::Mean),
            "cls" | "clstoken" => Some(Pooling::Cls),
            "max" | "maxtokens" => Some(Pooling::Max),
            "last" | "lasttoken" => Some(Pooling::LastToken),
            "meansqrtlen" | "meansqrtlentokens" => Some(Pooling::MeanSqrtLen),
            "weightedmean" | "weightedmeantokens" => Some(Pooling::WeightedMean),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Pooling::Mean => "mean",
            Pooling::Cls => "cls",
            Pooling::Max => "max",
            Pooling::LastToken => "last",
            Pooling::MeanSqrtLen => "mean_sqrt_len",
            Pooling::WeightedMean => "weighted_mean",
        }
    }
}

/// Everything the server needs to know about a model besides the ONNX graph itself.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPackage {
    pub onnx_path: PathBuf,
    /// tokenizer.json for text models; vocab.json for CTC packages.
    pub tokenizer_path: PathBuf,
    /// Text tokenizer (or CTC vocabulary) and configuration folder.
    pub root: PathBuf,
    pub task: Task,
    pub pooling: Pooling,
    /// L2-normalize the pooled vector (a `Normalize` module in `modules.json`).
    pub normalize: bool,
    /// Text token limit; zero for CTC audio packages.
    pub max_length: usize,
    /// Embedding size from `1_Pooling/config.json`, when it names one.
    pub dimension: Option<usize>,
    /// Label names of a classification head, by id (`id2label` of config.json).
    pub labels: Vec<String>,
    /// Labels are independent (`problem_type: multi_label_classification`): sigmoid, not
    /// softmax.
    pub multi_label: bool,
    /// Display name: the repo folder (`all-MiniLM-L6-v2`).
    pub name: String,
    /// sentence-transformers modules after pooling that this server cannot run (a `Dense`
    /// projection...). A package with any is refused rather than served with a wrong vector.
    pub unsupported_modules: Vec<String>,
    /// How each decision above was reached, for the startup log.
    pub notes: Vec<String>,
}

impl ModelPackage {
    /// The Hugging Face pipeline task this package serves through `/hf` (`None` for the
    /// embedding and reranking models, which have their own endpoints). An NLI classifier is a
    /// zero-shot classifier.
    pub fn pipeline_task(&self) -> Option<&'static str> {
        match self.task {
            Task::Embedding | Task::Rerank | Task::AsrCtc => None,
            Task::Classify if crate::tasks::nli_indices(&self.labels).is_some() => {
                Some("zero-shot-classification")
            }
            Task::Classify => Some("text-classification"),
            Task::TokenClassify => Some("token-classification"),
            Task::FillMask => Some("fill-mask"),
            Task::QuestionAnswering => Some("question-answering"),
        }
    }
}

/// Why a repo cannot be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageError {
    NotOnnx(PathBuf),
    MissingTokenizer(PathBuf),
    InvalidCtc(String),
    /// An image and text dual encoder (CLIP, SigLIP): a vision package scores it.
    DualEncoder(PathBuf),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageError::InvalidCtc(message) => write!(f, "{message}"),
            PackageError::NotOnnx(p) => write!(f, "{} is not an .onnx file", p.display()),
            PackageError::MissingTokenizer(p) => write!(
                f,
                "no tokenizer.json next to {} (or in its parent folder): download the repo's tokenizer files too",
                p.display()
            ),
            PackageError::DualEncoder(p) => write!(
                f,
                "{} is an image and text model (zero-shot image classification), not a text encoder",
                p.display()
            ),
        }
    }
}

impl std::error::Error for PackageError {}

/// The folders to look in, closest first: the model's own folder, then its parent (`onnx/`).
fn search_dirs(onnx: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = onnx.parent() {
        dirs.push(d.to_path_buf());
        if let Some(p) = d.parent() {
            if !p.as_os_str().is_empty() {
                dirs.push(p.to_path_buf());
            }
        }
    }
    dirs
}

/// The folder that holds `tokenizer.json` for a model file, `None` when there is none.
pub fn find_root(onnx: &Path) -> Option<PathBuf> {
    search_dirs(onnx)
        .into_iter()
        .find(|d| d.join("tokenizer.json").is_file())
}

/// Subfolders that hold a repo's graphs but are not its name.
const GRAPH_FOLDERS: [&str; 3] = ["onnx", "onnx_models", "models"];

/// The readable name of the model file `onnx_path`: its repo folder (`all-MiniLM-L6-v2`), plus the
/// variant when the file is not the repo's main graph (`model_quantized.onnx` is
/// `all-MiniLM-L6-v2-quantized`, `decoder_model_merged.onnx` `<repo>-decoder_model_merged`). A
/// generic file name (`model.onnx`) never names a model on its own. `None` for any other file.
pub fn alias_of(onnx_path: &Path) -> Option<String> {
    if !onnx_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
    {
        return None;
    }
    let repo = match find_root(onnx_path) {
        Some(root) => root.file_name()?.to_string_lossy().to_string(),
        None => {
            // No tokenizer to anchor on: the folder, one level up from an `onnx/` subfolder.
            let dir = onnx_path.parent()?;
            let name = dir.file_name()?.to_string_lossy().to_string();
            if GRAPH_FOLDERS.contains(&name.to_ascii_lowercase().as_str()) {
                dir.parent()?.file_name()?.to_string_lossy().to_string()
            } else {
                name
            }
        }
    };
    let stem = onnx_path.file_stem()?.to_string_lossy().to_string();
    let lower = stem.to_ascii_lowercase();
    let variant = if lower == "model" {
        ""
    } else {
        stem.get("model".len() + 1..)
            .filter(|_| lower.starts_with("model_") || lower.starts_with("model-"))
            .unwrap_or(&stem)
    };
    Some(if variant.is_empty() {
        repo
    } else {
        format!("{repo}-{variant}")
    })
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

#[derive(Debug, Deserialize)]
struct ModuleEntry {
    #[serde(default)]
    path: String,
    #[serde(default, rename = "type")]
    kind: String,
}

/// Pooling settings of a `1_Pooling/config.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolingConfig {
    pub mode: Pooling,
    pub dimension: Option<usize>,
}

/// Reads the pooling mode of a sentence-transformers `Pooling` module config: the newer
/// `pooling_mode` string, or the `pooling_mode_*_tokens` booleans (the first one that is on, in
/// the order CLS, mean, max, last token, mean-sqrt-len, weighted-mean).
pub fn parse_pooling_config(v: &Value) -> Option<PoolingConfig> {
    let dimension = v
        .get("word_embedding_dimension")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    if let Some(s) = v.get("pooling_mode").and_then(Value::as_str) {
        if let Some(mode) = Pooling::parse(s) {
            return Some(PoolingConfig { mode, dimension });
        }
    }
    let on = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mode = if on("pooling_mode_cls_token") {
        Pooling::Cls
    } else if on("pooling_mode_mean_tokens") {
        Pooling::Mean
    } else if on("pooling_mode_max_tokens") {
        Pooling::Max
    } else if on("pooling_mode_lasttoken") {
        Pooling::LastToken
    } else if on("pooling_mode_mean_sqrt_len_tokens") {
        Pooling::MeanSqrtLen
    } else if on("pooling_mode_weightedmean_tokens") {
        Pooling::WeightedMean
    } else {
        return None;
    };
    Some(PoolingConfig { mode, dimension })
}

/// Label names of `id2label`, ordered by id (empty when the config has none).
pub fn labels_from_config(config: &Value) -> Vec<String> {
    let Some(map) = config.get("id2label").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut pairs: Vec<(usize, String)> = map
        .iter()
        .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string())))
        .collect();
    pairs.sort_by_key(|(i, _)| *i);
    pairs.into_iter().map(|(_, l)| l).collect()
}

/// Label names that say nothing (`LABEL_0`, `LABEL_1`, ...): what transformers writes when a
/// head was trained without names.
fn generic_labels(labels: &[String]) -> bool {
    labels.iter().all(|l| {
        l.strip_prefix("LABEL_")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// The task a `config.json` implies, from the head in `architectures`. A sequence-classification
/// head is a cross-encoder (reranker) with one output, or with two outputs whose labels have no
/// names (some cross-encoders are trained as binary classifiers); otherwise a classifier.
pub fn task_from_config(config: &Value) -> Task {
    if crate::asr_ctc::is_ctc_config(config) {
        return Task::AsrCtc;
    }
    let heads: Vec<&str> = config
        .get("architectures")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let has = |suffix: &str| heads.iter().any(|n| n.ends_with(suffix));
    if has("ForSequenceClassification") {
        let outputs = config
            .get("num_labels")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or_else(|| labels_from_config(config).len());
        let labels = labels_from_config(config);
        if outputs <= 1 || (outputs == 2 && generic_labels(&labels)) {
            Task::Rerank
        } else {
            Task::Classify
        }
    } else if has("ForTokenClassification") {
        Task::TokenClassify
    } else if has("ForQuestionAnswering") {
        Task::QuestionAnswering
    } else if has("ForMaskedLM") {
        Task::FillMask
    } else {
        Task::Embedding
    }
}

/// A valid length from a config value: the Hub's "unlimited" sentinel
/// (`model_max_length: 1e30`) and nonsense are ignored.
fn plausible_length(v: Option<&Value>) -> Option<usize> {
    let n = v?.as_f64()?;
    (8.0..=1.0e7).contains(&n).then_some(n as usize)
}

/// Maximum number of tokens served for this repo.
fn max_length_of(
    sbert: Option<&Value>,
    tokenizer_config: Option<&Value>,
    config: Option<&Value>,
    notes: &mut Vec<String>,
) -> usize {
    let mut limit = None;
    let mut from = "default";
    if let Some(n) = plausible_length(sbert.and_then(|v| v.get("max_seq_length"))) {
        limit = Some(n);
        from = "sentence_bert_config.json";
    }
    let model_max = plausible_length(tokenizer_config.and_then(|v| v.get("model_max_length")));
    // The architecture's position table bounds everything; RoBERTa-style models reserve two
    // positions (514 -> 512 tokens).
    let positions = plausible_length(config.and_then(|v| v.get("max_position_embeddings"))).map(
        |n| match config
            .and_then(|v| v.get("model_type"))
            .and_then(Value::as_str)
        {
            Some("roberta") | Some("xlm-roberta") | Some("camembert") | Some("xlm-roberta-xl") => {
                n.saturating_sub(2)
            }
            _ => n,
        },
    );
    if limit.is_none() {
        if let Some(n) = model_max {
            limit = Some(n);
            from = "tokenizer_config.json";
        } else if let Some(n) = positions {
            limit = Some(n);
            from = "config.json";
        }
    }
    let mut n = limit.unwrap_or(DEFAULT_SEQUENCE_LENGTH);
    if let Some(p) = positions {
        n = n.min(p);
    }
    let n = n.clamp(8, MAX_SEQUENCE_LENGTH);
    notes.push(format!("max length {} tokens ({})", n, from));
    n
}

impl ModelPackage {
    /// Reads the repo around `onnx_path`.
    pub fn discover(onnx_path: &Path) -> Result<Self, PackageError> {
        if !onnx_path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        {
            return Err(PackageError::NotOnnx(onnx_path.to_path_buf()));
        }
        if let Some(root) = crate::asr_ctc::find_root(onnx_path) {
            if read_json(&root.join("config.json"))
                .is_some_and(|c| crate::asr_ctc::is_ctc_config(&c))
            {
                let ctc = crate::asr_ctc::CtcPackage::discover(onnx_path)
                    .map_err(PackageError::InvalidCtc)?;
                return Ok(Self {
                    onnx_path: ctc.onnx_path,
                    tokenizer_path: root.join("vocab.json"),
                    name: root
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    root,
                    task: Task::AsrCtc,
                    pooling: Pooling::Mean,
                    normalize: false,
                    max_length: 0,
                    dimension: None,
                    labels: Vec::new(),
                    multi_label: false,
                    unsupported_modules: Vec::new(),
                    notes: vec!["task asr_ctc (config.json architectures)".into()],
                });
            }
        }
        let root = find_root(onnx_path)
            .ok_or_else(|| PackageError::MissingTokenizer(onnx_path.to_path_buf()))?;
        let mut notes = Vec::new();

        let config = read_json(&root.join("config.json"));
        if config
            .as_ref()
            .and_then(crate::vision_pkg::task_from_config)
            == Some(crate::vision_pkg::VisionTask::ZeroShot)
        {
            return Err(PackageError::DualEncoder(onnx_path.to_path_buf()));
        }
        let tokenizer_config = read_json(&root.join("tokenizer_config.json"));
        let sbert = read_json(&root.join("sentence_bert_config.json"));
        let modules: Option<Vec<ModuleEntry>> =
            read_json(&root.join("modules.json")).and_then(|v| serde_json::from_value(v).ok());

        // sentence-transformers modules: Transformer, Pooling, Normalize, and the ones we
        // cannot run.
        let mut pooling_cfg: Option<PoolingConfig> = None;
        let mut normalize = false;
        let mut unsupported = Vec::new();
        let mut is_st = false;
        if let Some(modules) = &modules {
            is_st = true;
            for m in modules {
                let kind = m.kind.rsplit('.').next().unwrap_or("");
                match kind {
                    "Transformer" => {}
                    "Pooling" => {
                        pooling_cfg = read_json(&root.join(&m.path).join("config.json"))
                            .and_then(|v| parse_pooling_config(&v));
                    }
                    "Normalize" => normalize = true,
                    other => unsupported.push(other.to_string()),
                }
            }
        } else if let Some(cfg) = read_json(&root.join("1_Pooling").join("config.json")) {
            // No modules.json but a pooling folder (some re-uploads keep only that).
            pooling_cfg = parse_pooling_config(&cfg);
            normalize = root.join("2_Normalize").is_dir();
            is_st = pooling_cfg.is_some();
        }

        // A sentence-transformers repo is an embedding model; a CrossEncoder repo has no
        // modules.json but a classification head.
        let task = match &config {
            Some(c) if !is_st => task_from_config(c),
            _ => Task::Embedding,
        };
        notes.push(format!(
            "task {}{}",
            task.as_str(),
            if is_st {
                " (sentence-transformers repo)"
            } else if config.is_some() {
                " (config.json architectures)"
            } else {
                " (default)"
            }
        ));

        let pooling = match &pooling_cfg {
            Some(p) => {
                notes.push(format!(
                    "pooling {} (1_Pooling/config.json)",
                    p.mode.as_str()
                ));
                p.mode
            }
            None => {
                if task == Task::Embedding {
                    notes.push("pooling mean (no 1_Pooling/config.json: default)".into());
                }
                Pooling::Mean
            }
        };
        if task == Task::Embedding {
            notes.push(format!(
                "normalize {}",
                if normalize {
                    "yes (Normalize module)"
                } else {
                    "no (no Normalize module)"
                }
            ));
        }
        let max_length = max_length_of(
            sbert.as_ref(),
            tokenizer_config.as_ref(),
            config.as_ref(),
            &mut notes,
        );
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "onnx-model".to_string());
        Ok(ModelPackage {
            onnx_path: onnx_path.to_path_buf(),
            tokenizer_path: root.join("tokenizer.json"),
            root,
            task,
            pooling,
            normalize,
            max_length,
            dimension: pooling_cfg.and_then(|p| p.dimension),
            labels: config.as_ref().map(labels_from_config).unwrap_or_default(),
            multi_label: config
                .as_ref()
                .and_then(|c| c.get("problem_type"))
                .and_then(Value::as_str)
                == Some("multi_label_classification"),
            name,
            unsupported_modules: unsupported,
            notes,
        })
    }
}

/// Precision of an ONNX file from its name, in the vocabulary of the Hub's quantization badges:
/// `model.onnx` is FP32, `model_fp16` FP16, `model_quantized` / `model_int8` / `model_qint8_*`
/// / `model_uint8` INT8, `model_q4` / `model_bnb4` 4-bit. Optimum's `model_O1`..`O3` are
/// graph-optimized FP32, `model_O4` is the mixed FP16 one.
pub fn variant_quantization(file_stem: &str) -> &'static str {
    let s = file_stem.to_ascii_lowercase();
    let has = |needle: &str| s.contains(needle);
    if has("bnb4") || has("q4") || has("int4") {
        "Q4"
    } else if has("int8") || has("quantized") || has("uint8") || has("q8") {
        "INT8"
    } else if has("fp16") || has("float16") || s.ends_with("_o4") {
        "FP16"
    } else {
        "FP32"
    }
}

/// Task of a model file for the library: `Some` when `onnx_path` sits in a repo with a
/// tokenizer (a servable encoder), `None` for any other `.onnx` file (a vision or speech
/// model, a bare graph).
pub fn library_task(onnx_path: &Path) -> Option<Task> {
    ModelPackage::discover(onnx_path).ok().map(|p| p.task)
}

/// Whether the graph uses the `op` operator. Operator types are plain strings in the protobuf,
/// so the file is scanned for the name in chunks (no ONNX parser, no full read into memory).
pub fn graph_uses_op(onnx_path: &Path, op: &str) -> bool {
    use std::io::Read;
    let needle = op.as_bytes();
    let Ok(mut file) = std::fs::File::open(onnx_path) else {
        return false;
    };
    let mut buf = vec![0u8; (1 << 20) + needle.len()];
    // Bytes carried over from the previous chunk, so a name split across two reads is found.
    let mut kept = 0;
    loop {
        let n = match file.read(&mut buf[kept..]) {
            Ok(0) | Err(_) => return false,
            Ok(n) => n,
        };
        let filled = kept + n;
        if buf[..filled].windows(needle.len()).any(|w| w == needle) {
            return true;
        }
        kept = (needle.len() - 1).min(filled);
        buf.copy_within(filled - kept..filled, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn finds_an_operator_name_split_across_reads() {
        let dir = std::env::temp_dir().join(format!("hs-onnx-op-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.onnx");
        // The name straddles the 1 MiB read boundary.
        let mut bytes = vec![0u8; (1 << 20) - 4];
        bytes.extend_from_slice(b"ConvInteger");
        bytes.extend_from_slice(&[0u8; 64]);
        fs::write(&path, &bytes).unwrap();
        assert!(graph_uses_op(&path, "ConvInteger"));
        assert!(!graph_uses_op(&path, "MatMulInteger"));
        assert!(!graph_uses_op(&dir.join("missing.onnx"), "ConvInteger"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn alias_comes_from_the_repo_folder() {
        let a = |p: &str| alias_of(Path::new(p));
        assert_eq!(
            a("/m/sentence-transformers/all-MiniLM-L6-v2/onnx/model.onnx").as_deref(),
            Some("all-MiniLM-L6-v2")
        );
        assert_eq!(
            a("/m/x/all-MiniLM-L6-v2/onnx/model_quantized.onnx").as_deref(),
            Some("all-MiniLM-L6-v2-quantized")
        );
        assert_eq!(
            a("/m/x/repo-a/model_fp16.onnx").as_deref(),
            Some("repo-a-fp16")
        );
        assert_eq!(
            a("/m/x/repo-a/decoder_model_merged.onnx").as_deref(),
            Some("repo-a-decoder_model_merged")
        );
        // Two repos with a `model.onnx` do not collide.
        assert_ne!(a("/m/a/one/model.onnx"), a("/m/b/two/model.onnx"));
        assert_eq!(a("/m/x/model.gguf"), None);
    }

    fn write(dir: &Path, rel: &str, v: Value) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, serde_json::to_vec(&v).unwrap()).unwrap();
    }

    /// The files of sentence-transformers/all-MiniLM-L6-v2 as the Hub serves them.
    fn minilm(dir: &Path) -> PathBuf {
        let repo = dir.join("sentence-transformers").join("all-MiniLM-L6-v2");
        write(
            &repo,
            "modules.json",
            json!([
                {"idx": 0, "name": "0", "path": "", "type": "sentence_transformers.models.Transformer"},
                {"idx": 1, "name": "1", "path": "1_Pooling", "type": "sentence_transformers.models.Pooling"},
                {"idx": 2, "name": "2", "path": "2_Normalize", "type": "sentence_transformers.models.Normalize"}
            ]),
        );
        write(
            &repo,
            "1_Pooling/config.json",
            json!({"word_embedding_dimension": 384, "pooling_mode_cls_token": false,
                   "pooling_mode_mean_tokens": true, "pooling_mode_max_tokens": false,
                   "pooling_mode_mean_sqrt_len_tokens": false}),
        );
        write(
            &repo,
            "sentence_bert_config.json",
            json!({"max_seq_length": 256, "do_lower_case": false}),
        );
        write(
            &repo,
            "config.json",
            json!({"architectures": ["BertModel"], "model_type": "bert", "max_position_embeddings": 512}),
        );
        write(
            &repo,
            "tokenizer_config.json",
            json!({"model_max_length": 512}),
        );
        write(&repo, "tokenizer.json", json!({}));
        let onnx = repo.join("onnx").join("model.onnx");
        fs::create_dir_all(onnx.parent().unwrap()).unwrap();
        fs::write(&onnx, b"x").unwrap();
        onnx
    }

    #[test]
    fn minilm_layout_is_read_like_sentence_transformers() {
        let dir = tempfile::tempdir().unwrap();
        let onnx = minilm(dir.path());
        let p = ModelPackage::discover(&onnx).unwrap();
        assert_eq!(p.task, Task::Embedding);
        assert_eq!(p.pooling, Pooling::Mean);
        assert!(p.normalize);
        assert_eq!(p.max_length, 256);
        assert_eq!(p.dimension, Some(384));
        assert_eq!(p.name, "all-MiniLM-L6-v2");
        assert!(p.unsupported_modules.is_empty());
        assert!(p.tokenizer_path.ends_with("tokenizer.json"));
    }

    #[test]
    fn cls_pooling_without_normalize() {
        let dir = tempfile::tempdir().unwrap();
        let onnx = minilm(dir.path());
        let repo = onnx.parent().unwrap().parent().unwrap().to_path_buf();
        write(
            &repo,
            "1_Pooling/config.json",
            json!({"word_embedding_dimension": 384, "pooling_mode_cls_token": true, "pooling_mode_mean_tokens": false}),
        );
        write(
            &repo,
            "modules.json",
            json!([
                {"idx": 0, "path": "", "type": "sentence_transformers.models.Transformer"},
                {"idx": 1, "path": "1_Pooling", "type": "sentence_transformers.models.Pooling"}
            ]),
        );
        let p = ModelPackage::discover(&onnx).unwrap();
        assert_eq!(p.pooling, Pooling::Cls);
        assert!(!p.normalize);
    }

    #[test]
    fn dense_module_is_reported_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let onnx = minilm(dir.path());
        let repo = onnx.parent().unwrap().parent().unwrap().to_path_buf();
        write(
            &repo,
            "modules.json",
            json!([
                {"idx": 0, "path": "", "type": "sentence_transformers.models.Transformer"},
                {"idx": 1, "path": "1_Pooling", "type": "sentence_transformers.models.Pooling"},
                {"idx": 2, "path": "2_Dense", "type": "sentence_transformers.models.Dense"}
            ]),
        );
        let p = ModelPackage::discover(&onnx).unwrap();
        assert_eq!(p.unsupported_modules, vec!["Dense".to_string()]);
    }

    /// cross-encoder/ms-marco-MiniLM-L-6-v2: no modules.json, a classification head.
    #[test]
    fn cross_encoder_is_a_reranker() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir
            .path()
            .join("cross-encoder")
            .join("ms-marco-MiniLM-L-6-v2");
        write(
            &repo,
            "config.json",
            json!({"architectures": ["BertForSequenceClassification"], "model_type": "bert", "max_position_embeddings": 512}),
        );
        write(
            &repo,
            "tokenizer_config.json",
            json!({"model_max_length": 512}),
        );
        write(&repo, "tokenizer.json", json!({}));
        let onnx = repo.join("onnx").join("model.onnx");
        fs::create_dir_all(onnx.parent().unwrap()).unwrap();
        fs::write(&onnx, b"x").unwrap();
        let p = ModelPackage::discover(&onnx).unwrap();
        assert_eq!(p.task, Task::Rerank);
        assert_eq!(p.max_length, 512);
        assert_eq!(library_task(&onnx), Some(Task::Rerank));
    }

    #[test]
    fn roberta_position_table_loses_two() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("BAAI").join("bge-m3");
        write(
            &repo,
            "config.json",
            json!({"architectures": ["XLMRobertaModel"], "model_type": "xlm-roberta", "max_position_embeddings": 8194}),
        );
        write(
            &repo,
            "tokenizer_config.json",
            json!({"model_max_length": 8192}),
        );
        write(&repo, "tokenizer.json", json!({}));
        let onnx = repo.join("onnx").join("model.onnx");
        fs::create_dir_all(onnx.parent().unwrap()).unwrap();
        fs::write(&onnx, b"x").unwrap();
        let p = ModelPackage::discover(&onnx).unwrap();
        assert_eq!(p.max_length, 8192);
        // No pooling files: mean by default, said in the notes.
        assert_eq!(p.pooling, Pooling::Mean);
        assert!(p.notes.iter().any(|n| n.contains("default")));
    }

    #[test]
    fn unlimited_tokenizer_length_is_ignored() {
        let mut notes = Vec::new();
        let n = max_length_of(
            None,
            Some(&json!({"model_max_length": 1e30})),
            Some(&json!({"max_position_embeddings": 514, "model_type": "roberta"})),
            &mut notes,
        );
        assert_eq!(n, 512);
    }

    #[test]
    fn missing_tokenizer_and_wrong_extension_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let onnx = dir.path().join("m").join("onnx").join("model.onnx");
        fs::create_dir_all(onnx.parent().unwrap()).unwrap();
        fs::write(&onnx, b"x").unwrap();
        assert!(matches!(
            ModelPackage::discover(&onnx),
            Err(PackageError::MissingTokenizer(_))
        ));
        assert!(library_task(&onnx).is_none());
        assert!(matches!(
            ModelPackage::discover(&dir.path().join("a.gguf")),
            Err(PackageError::NotOnnx(_))
        ));
    }

    #[test]
    fn precision_from_the_file_name() {
        for (stem, want) in [
            ("model", "FP32"),
            ("model_O1", "FP32"),
            ("model_O4", "FP16"),
            ("model_fp16", "FP16"),
            ("model_quantized", "INT8"),
            ("model_qint8_avx512_vnni", "INT8"),
            ("model_quint8_avx2", "INT8"),
            ("model_int8", "INT8"),
            ("model_q4", "Q4"),
            ("model_bnb4", "Q4"),
            ("model_q4f16", "Q4"),
        ] {
            assert_eq!(variant_quantization(stem), want, "{stem}");
        }
    }

    #[test]
    fn pooling_mode_strings_and_flags() {
        let c = parse_pooling_config(&json!({"pooling_mode": "lasttoken"})).unwrap();
        assert_eq!(c.mode, Pooling::LastToken);
        let c = parse_pooling_config(
            &json!({"pooling_mode_lasttoken": true, "word_embedding_dimension": 1024}),
        )
        .unwrap();
        assert_eq!((c.mode, c.dimension), (Pooling::LastToken, Some(1024)));
        assert!(parse_pooling_config(&json!({"pooling_mode_cls_token": false})).is_none());
        assert_eq!(
            Pooling::parse("mean_sqrt_len_tokens"),
            Some(Pooling::MeanSqrtLen)
        );
        assert_eq!(Task::parse("Reranker"), Some(Task::Rerank));
    }

    #[test]
    fn sequence_heads_are_rerankers_unless_they_name_their_labels() {
        let seq = |extra: Value| {
            let mut c = json!({"architectures": ["BertForSequenceClassification"]});
            c.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            task_from_config(&c)
        };
        // ms-marco cross-encoders: one output.
        assert_eq!(seq(json!({"num_labels": 1})), Task::Rerank);
        assert_eq!(seq(json!({"id2label": {"0": "LABEL_0"}})), Task::Rerank);
        // A cross-encoder trained as a binary classifier: two unnamed outputs.
        assert_eq!(
            seq(json!({"id2label": {"0": "LABEL_0", "1": "LABEL_1"}})),
            Task::Rerank
        );
        assert_eq!(seq(json!({"num_labels": 2})), Task::Rerank);
        // Named labels, or more than two outputs: a classifier.
        assert_eq!(
            seq(json!({"id2label": {"0": "NEGATIVE", "1": "POSITIVE"}})),
            Task::Classify
        );
        assert_eq!(
            seq(json!({"id2label": {"0": "LABEL_0", "1": "LABEL_1", "2": "LABEL_2"}})),
            Task::Classify
        );
    }
}
