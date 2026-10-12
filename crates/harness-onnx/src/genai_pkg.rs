//! Layout of an onnxruntime-genai model folder (pure: no runtime, no tokenizer).
//!
//! A text-generation ONNX repo made for onnxruntime-genai keeps everything one run needs in a
//! single folder:
//!
//! ```text
//! <repo>/<variant>/genai_config.json      model type, decoder file, context length, providers
//! <repo>/<variant>/model.onnx  model.onnx.data
//! <repo>/<variant>/tokenizer.json  tokenizer_config.json  [chat_template.jinja]
//! ```
//!
//! `microsoft/Phi-3-mini-4k-instruct-onnx` has one such folder per target
//! (`directml/directml-int4-awq-block-128`, `cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4`,
//! `cuda/cuda-int4-...`); smaller community repos put a single model at the repo root.
//! [`GenaiPackage::discover`] starts from the decoder's `.onnx` file (what the library lists) or
//! from the folder itself.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// File that marks a genai model folder.
pub const CONFIG_FILE: &str = "genai_config.json";

/// What `genai_config.json` and the folder say about a model.
#[derive(Debug, Clone, PartialEq)]
pub struct GenaiPackage {
    /// The folder (what `OgaCreateConfig` takes).
    pub dir: PathBuf,
    /// The decoder graph (`model.decoder.filename`).
    pub onnx_path: PathBuf,
    /// `model.type` (`phi3`, `qwen3`, `llama`...).
    pub model_type: String,
    /// `model.context_length` (the tokens a conversation may fill), 0 when absent.
    pub context_length: usize,
    /// Execution providers the config asks for, lower case (`dml`, `cuda`...); empty is CPU.
    pub providers: Vec<String>,
    /// Decoder shape, for the memory estimate of the KV cache (0 when the config omits it).
    pub layers: usize,
    pub kv_heads: usize,
    pub head_size: usize,
    /// `search` defaults of the config.
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<i64>,
    /// `tokenizer_config.json` or `chat_template.jinja` is there.
    pub has_chat_template: bool,
    /// Whether the config describes a model this server can chat with: a text decoder without
    /// vision or audio sub-models.
    pub text_only: bool,
}

impl GenaiPackage {
    /// Reads the genai model around `path`: a `.onnx` file, a `genai_config.json`, or the
    /// folder. `None` when there is no `genai_config.json` there (an encoder repo, a bare graph).
    pub fn discover(path: &Path) -> Option<Self> {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()?.to_path_buf()
        };
        let config: Value = serde_json::from_str(
            std::fs::read_to_string(dir.join(CONFIG_FILE))
                .ok()?
                .trim_start_matches('\u{feff}'),
        )
        .ok()?;
        let model = config.get("model")?;
        let decoder = model.get("decoder")?;
        let filename = decoder
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("model.onnx");
        // A `.onnx` that is not the decoder (a vision or embedding graph of the folder) is not
        // this package's model.
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        {
            let given = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            let wanted = Path::new(filename)
                .file_name()?
                .to_string_lossy()
                .to_ascii_lowercase();
            if given != wanted {
                return None;
            }
        }
        let providers = providers_of(decoder);
        let search = config.get("search");
        let num = |k: &str| search.and_then(|s| s.get(k)).and_then(Value::as_f64);
        let dim = |k: &str| decoder.get(k).and_then(Value::as_u64).unwrap_or(0) as usize;
        let has_chat_template = dir.join("chat_template.jinja").is_file()
            || std::fs::read_to_string(dir.join("tokenizer_config.json"))
                .ok()
                .is_some_and(|t| t.contains("\"chat_template\""));
        Some(GenaiPackage {
            onnx_path: dir.join(filename),
            model_type: model
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            context_length: model
                .get("context_length")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            providers,
            layers: dim("num_hidden_layers"),
            kv_heads: {
                let kv = dim("num_key_value_heads");
                if kv > 0 {
                    kv
                } else {
                    dim("num_attention_heads")
                }
            },
            head_size: dim("head_size"),
            temperature: num("temperature"),
            top_p: num("top_p"),
            top_k: num("top_k").map(|k| k as i64),
            has_chat_template,
            text_only: model.get("vision").is_none() && model.get("speech").is_none(),
            dir,
        })
    }

    /// Bytes of KV cache per token (fp16 keys and values of every layer); 0 when unknown.
    pub fn kv_bytes_per_token(&self) -> u64 {
        (2 * 2 * self.layers * self.kv_heads * self.head_size) as u64
    }

    /// Bytes of the decoder graph and its external weights (`model.onnx.data`).
    pub fn weight_bytes(&self) -> u64 {
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let mut total = size(&self.onnx_path);
        let name = self
            .onnx_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        if let Some(name) = name {
            for suffix in [".data", "_data"] {
                total += size(&self.onnx_path.with_file_name(format!("{name}{suffix}")));
            }
        }
        total
    }

    /// The provider the config itself runs on, in the vocabulary of `--provider`.
    pub fn config_provider(&self) -> &'static str {
        match self.providers.first().map(String::as_str) {
            Some("dml") | Some("directml") => "directml",
            Some("cuda") | Some("nvtensorrtrtx") | Some("trt") => "cuda",
            _ => "cpu",
        }
    }
}

/// The providers `session_options.provider_options` names (`[{"dml": {}}]`), lower case.
fn providers_of(decoder: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let options = decoder
        .get("session_options")
        .and_then(|s| s.get("provider_options"))
        .and_then(Value::as_array);
    for entry in options.into_iter().flatten() {
        if let Some(map) = entry.as_object() {
            out.extend(map.keys().map(|k| k.to_ascii_lowercase()));
        }
    }
    out
}

/// Folder names that group the variants of a repo by target (`directml/`, `cpu_and_mobile/`).
const TARGET_GROUPS: [&str; 9] = [
    "directml",
    "dml",
    "cpu",
    "cpu_and_mobile",
    "cuda",
    "gpu",
    "npu",
    "rocm",
    "mobile",
];

/// A readable, stable name for the model in `dir`: the repo folder plus the variant folder when
/// the layout is `<repo>/<target group>/<variant>` (`Phi-3-mini-4k-instruct-onnx-directml-int4-awq-block-128`),
/// else the folder itself (a model at the repo root is named after the repo).
pub fn display_name(dir: &Path) -> Option<String> {
    let folder = dir.file_name()?.to_string_lossy().to_string();
    let parent = dir
        .parent()?
        .file_name()
        .map(|n| n.to_string_lossy().to_string());
    let grand = dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().to_string());
    match (parent, grand) {
        (Some(p), Some(repo)) if TARGET_GROUPS.contains(&p.to_ascii_lowercase().as_str()) => {
            Some(format!("{repo}-{folder}"))
        }
        _ => Some(folder),
    }
}

/// [`display_name`] of the model whose decoder is `onnx_path`; `None` for any other file.
pub fn alias_of(onnx_path: &Path) -> Option<String> {
    display_name(&GenaiPackage::discover(onnx_path)?.dir)
}

/// Whether `onnx_path` is the decoder of a genai folder that the sidecar can chat with (the
/// library lists it as a chat model, `serve_mode` serves it as one).
pub fn is_chat_model(onnx_path: &Path) -> bool {
    GenaiPackage::discover(onnx_path).is_some_and(|p| p.text_only)
}

/// The tokens of a genai folder name's precision, for the library: `directml-int4-awq-block-128`
/// is INT4, `cuda-fp16` FP16, `cpu-int4-rtn-block-32-acc-level-4` INT4.
pub fn precision_of(folder_name: &str) -> &'static str {
    let s = folder_name.to_ascii_lowercase();
    if s.contains("int4") || s.contains("q4") || s.contains("4bit") {
        "INT4"
    } else if s.contains("int8") || s.contains("q8") {
        "INT8"
    } else if s.contains("fp16") || s.contains("f16") {
        "FP16"
    } else if s.contains("bf16") {
        "BF16"
    } else {
        "FP32"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(dir: &Path, config: Value) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(CONFIG_FILE), serde_json::to_vec(&config).unwrap()).unwrap();
        std::fs::write(dir.join("model.onnx"), b"x").unwrap();
        dir.join("model.onnx")
    }

    fn phi3_dml() -> Value {
        json!({
            "model": {
                "type": "phi3", "context_length": 4096,
                "decoder": {
                    "filename": "model.onnx",
                    "num_hidden_layers": 32, "num_key_value_heads": 8, "head_size": 96,
                    "session_options": {"provider_options": [{"dml": {}}]}
                }
            },
            "search": {"temperature": 0.6, "top_p": 0.9, "top_k": 40}
        })
    }

    #[test]
    fn reads_the_decoder_context_and_provider() {
        let d = tempfile::tempdir().unwrap();
        let onnx = folder(&d.path().join("directml").join("directml-int4"), phi3_dml());
        let p = GenaiPackage::discover(&onnx).unwrap();
        assert_eq!(p.model_type, "phi3");
        assert_eq!(p.context_length, 4096);
        assert_eq!(p.providers, vec!["dml".to_string()]);
        assert_eq!(p.config_provider(), "directml");
        assert_eq!(
            (p.temperature, p.top_p, p.top_k),
            (Some(0.6), Some(0.9), Some(40))
        );
        assert!(p.text_only && !p.has_chat_template);
        // 2 (key, value) x 2 bytes x 32 layers x 8 heads x 96.
        assert_eq!(p.kv_bytes_per_token(), 2 * 2 * 32 * 8 * 96);
        assert_eq!(p.weight_bytes(), 1);
        assert!(is_chat_model(&onnx));
        // The folder and the config file name the same package.
        assert_eq!(GenaiPackage::discover(onnx.parent().unwrap()).unwrap(), p);
        assert_eq!(
            GenaiPackage::discover(&onnx.parent().unwrap().join(CONFIG_FILE)).unwrap(),
            p
        );
    }

    #[test]
    fn empty_provider_options_mean_the_cpu() {
        let d = tempfile::tempdir().unwrap();
        let onnx = folder(
            d.path(),
            json!({"model": {"type": "qwen3", "decoder": {"filename": "model.onnx",
                "session_options": {"provider_options": []}}}}),
        );
        let p = GenaiPackage::discover(&onnx).unwrap();
        assert_eq!(p.config_provider(), "cpu");
        assert_eq!(p.context_length, 0);
    }

    #[test]
    fn other_graphs_and_encoder_repos_are_not_genai_models() {
        let d = tempfile::tempdir().unwrap();
        let onnx = folder(d.path(), phi3_dml());
        let vision = d.path().join("vision.onnx");
        std::fs::write(&vision, b"x").unwrap();
        assert!(GenaiPackage::discover(&vision).is_none());
        let enc = d.path().join("enc");
        std::fs::create_dir_all(&enc).unwrap();
        std::fs::write(enc.join("model.onnx"), b"x").unwrap();
        assert!(GenaiPackage::discover(&enc.join("model.onnx")).is_none());
        assert!(!is_chat_model(&enc.join("model.onnx")));
        assert!(is_chat_model(&onnx));
    }

    #[test]
    fn vision_models_are_not_chat_models_here() {
        let d = tempfile::tempdir().unwrap();
        let mut c = phi3_dml();
        c["model"]["vision"] = json!({"filename": "vision.onnx"});
        let onnx = folder(d.path(), c);
        assert!(!is_chat_model(&onnx));
    }

    #[test]
    fn chat_template_files_are_noticed() {
        let d = tempfile::tempdir().unwrap();
        let onnx = folder(d.path(), phi3_dml());
        std::fs::write(d.path().join("chat_template.jinja"), "{{ x }}").unwrap();
        assert!(GenaiPackage::discover(&onnx).unwrap().has_chat_template);
    }

    #[test]
    fn names_follow_the_repo_layout() {
        let d = tempfile::tempdir().unwrap();
        let ms = folder(
            &d.path()
                .join("Phi-3-mini-4k-instruct-onnx")
                .join("directml")
                .join("directml-int4-awq-block-128"),
            phi3_dml(),
        );
        assert_eq!(
            alias_of(&ms).as_deref(),
            Some("Phi-3-mini-4k-instruct-onnx-directml-int4-awq-block-128")
        );
        let root = folder(&d.path().join("Qwen3-0.6B-ONNX-GenAI"), phi3_dml());
        assert_eq!(alias_of(&root).as_deref(), Some("Qwen3-0.6B-ONNX-GenAI"));
        assert_eq!(alias_of(&d.path().join("x.onnx")), None);
    }

    #[test]
    fn precision_from_folder_names() {
        assert_eq!(precision_of("directml-int4-awq-block-128"), "INT4");
        assert_eq!(precision_of("cuda-fp16"), "FP16");
        assert_eq!(precision_of("cpu-int4-rtn-block-32-acc-level-4"), "INT4");
        assert_eq!(precision_of("model"), "FP32");
    }
}
