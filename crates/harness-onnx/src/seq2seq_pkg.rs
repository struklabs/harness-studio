//! Text encoder-decoder models (T5, BART, Marian, Pegasus...) as transformers.js / Optimum
//! export them: `encoder_model*.onnx`, a decoder beside it ([`generation::decoder_beside`]), a
//! `tokenizer.json`. The task comes from the config: Marian translates, a model whose only
//! `task_specific_params` entry is `summarization` summarizes, anything else is
//! text2text-generation. Pure code: the sessions live in [`crate::seq2seq_engine`].

use crate::api::ApiError;
use crate::generation::{self, GenConfig};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Encoder-decoder families whose inputs are plain tokenized text. mBART, M2M100 and NLLB
/// need source and target language tokens and are not served yet.
const MODEL_TYPES: &[&str] = &[
    "t5",
    "mt5",
    "umt5",
    "longt5",
    "bart",
    "marian",
    "pegasus",
    "pegasus_x",
    "blenderbot",
    "blenderbot-small",
];

/// Input tokens when neither the tokenizer nor the config names a limit.
const DEFAULT_MAX_INPUT: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub struct Seq2SeqPackage {
    pub encoder_path: PathBuf,
    pub decoder_path: PathBuf,
    pub tokenizer_path: PathBuf,
    pub root: PathBuf,
    /// `summarization`, `translation` or `text2text-generation`.
    pub task: &'static str,
    pub gen: GenConfig,
    /// Longest input in tokens (longer inputs are cut).
    pub max_input: usize,
    /// Display name: the repo folder.
    pub name: String,
    pub notes: Vec<String>,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Whether a `config.json` is a text encoder-decoder this crate serves.
pub fn is_seq2seq_config(config: &Value) -> bool {
    let typed = config
        .get("model_type")
        .and_then(Value::as_str)
        .is_some_and(|t| MODEL_TYPES.contains(&t));
    let generator = config
        .get("architectures")
        .and_then(Value::as_array)
        .is_some_and(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .any(|n| n.ends_with("ForConditionalGeneration") || n == "MarianMTModel")
        });
    typed && generator
}

/// The pipeline task of a seq2seq config and the `task_specific_params` entry it runs with.
fn task_of(config: &Value) -> (&'static str, Option<Value>) {
    if config.get("model_type").and_then(Value::as_str) == Some("marian") {
        return ("translation", None);
    }
    let params = config
        .get("task_specific_params")
        .and_then(Value::as_object);
    let summarization = params.and_then(|p| p.get("summarization"));
    let translates = params.is_some_and(|p| p.keys().any(|k| k.starts_with("translation")));
    match summarization {
        Some(s) if !translates => ("summarization", Some(s.clone())),
        _ => ("text2text-generation", None),
    }
}

/// The folder with `config.json` and `tokenizer.json` (the graph's folder or its parent).
fn find_root(onnx: &Path) -> Option<PathBuf> {
    onnx.ancestors()
        .skip(1)
        .take(2)
        .find(|d| d.join("config.json").is_file() && d.join("tokenizer.json").is_file())
        .map(Path::to_path_buf)
}

impl Seq2SeqPackage {
    /// Reads the repo around an `encoder_model*.onnx`; `None` for any other file or model.
    pub fn discover(onnx_path: &Path) -> Option<Self> {
        let root = find_root(onnx_path)?;
        let config = read_json(&root.join("config.json"))?;
        if !is_seq2seq_config(&config) {
            return None;
        }
        let decoder_path = generation::decoder_beside(onnx_path)?;
        let (task, overrides) = task_of(&config);
        let gen = GenConfig::read(&root, overrides.as_ref())?;
        let tokenizer_limit = read_json(&root.join("tokenizer_config.json"))
            .and_then(|t| t.get("model_max_length").and_then(Value::as_f64))
            .filter(|n| *n >= 1.0 && *n < 1e6)
            .map(|n| n as usize);
        // BART, Marian, Pegasus have learned positions: no input beyond them. T5's are relative,
        // its `n_positions` only a training length.
        let positions = config
            .get("max_position_embeddings")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        let max_input = match (tokenizer_limit, positions) {
            (Some(t), Some(p)) => t.min(p),
            (t, p) => t
                .or(p)
                .or_else(|| {
                    config
                        .get("n_positions")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize)
                })
                .unwrap_or(DEFAULT_MAX_INPUT),
        };
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "onnx-model".to_string());
        let notes = vec![
            format!(
                "task {} ({}), decoder {}",
                task,
                config.get("model_type").and_then(Value::as_str).unwrap_or(""),
                decoder_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
            format!(
                "greedy decoding, max_length {}, min_length {}, no_repeat_ngram {}, input up to {} tokens",
                gen.max_length, gen.min_length, gen.no_repeat_ngram, max_input
            ),
        ];
        Some(Seq2SeqPackage {
            encoder_path: onnx_path.to_path_buf(),
            decoder_path,
            tokenizer_path: root.join("tokenizer.json"),
            root,
            task,
            gen,
            max_input,
            name,
            notes,
        })
    }

    /// The JSON key of the generated text (`summary_text`, `translation_text`,
    /// `generated_text`), as the transformers pipelines answer.
    pub fn output_key(&self) -> &'static str {
        match self.task {
            "summarization" => "summary_text",
            "translation" => "translation_text",
            _ => "generated_text",
        }
    }
}

/// A parsed `/hf` request of a text generator.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Seq2SeqJob {
    pub text: String,
    pub max_new_tokens: Option<usize>,
}

/// `{"inputs": "<text>", "parameters": {"max_new_tokens"}}`.
pub fn parse(body: &Value) -> Result<Seq2SeqJob, ApiError> {
    let text = match body.get("inputs") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(Value::String(_)) => return Err(ApiError::invalid("inputs is empty")),
        Some(_) => return Err(ApiError::invalid("inputs must be one text")),
        None => return Err(ApiError::invalid("inputs is required")),
    };
    let max_new_tokens = match body.pointer("/parameters/max_new_tokens") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| ApiError::invalid("max_new_tokens must be a positive integer"))?
                as usize,
        ),
    };
    Ok(Seq2SeqJob {
        text,
        max_new_tokens,
    })
}

/// The pipeline task of a model file, for the library.
pub fn pipeline_task_of(onnx_path: &Path) -> Option<&'static str> {
    Seq2SeqPackage::discover(onnx_path).map(|p| p.task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn repo(dir: &Path, config: Value, generation: Value) -> PathBuf {
        let onnx = dir.join("onnx");
        fs::create_dir_all(&onnx).unwrap();
        fs::write(dir.join("config.json"), config.to_string()).unwrap();
        fs::write(dir.join("generation_config.json"), generation.to_string()).unwrap();
        fs::write(dir.join("tokenizer.json"), "{}").unwrap();
        for f in [
            "encoder_model_quantized.onnx",
            "decoder_model_merged_quantized.onnx",
        ] {
            fs::write(onnx.join(f), b"onnx").unwrap();
        }
        onnx.join("encoder_model_quantized.onnx")
    }

    #[test]
    fn tasks_follow_the_config() {
        let tmp = tempfile::tempdir().unwrap();
        // distilbart-cnn: summarization, with that entry's lengths.
        let bart = repo(
            &tmp.path().join("distilbart-cnn-6-6"),
            json!({"architectures": ["BartForConditionalGeneration"], "model_type": "bart",
                   "max_position_embeddings": 1024,
                   "task_specific_params": {"summarization": {"max_length": 142, "min_length": 56,
                                                               "no_repeat_ngram_size": 3}}}),
            json!({"decoder_start_token_id": 2, "eos_token_id": 2, "forced_bos_token_id": 0}),
        );
        let p = Seq2SeqPackage::discover(&bart).unwrap();
        assert_eq!((p.task, p.output_key()), ("summarization", "summary_text"));
        assert_eq!(
            (p.gen.max_length, p.gen.min_length, p.gen.no_repeat_ngram),
            (142, 56, 3)
        );
        assert_eq!(p.max_input, 1024);
        assert!(p
            .decoder_path
            .ends_with("decoder_model_merged_quantized.onnx"));
        // t5-small names summarization and translations: a general text2text model.
        let t5 = repo(
            &tmp.path().join("t5-small"),
            json!({"architectures": ["T5ForConditionalGeneration"], "model_type": "t5",
                   "task_specific_params": {"summarization": {}, "translation_en_to_de": {}}}),
            json!({"decoder_start_token_id": 0, "eos_token_id": 1}),
        );
        let p = Seq2SeqPackage::discover(&t5).unwrap();
        assert_eq!((p.task, p.max_input), ("text2text-generation", 512));
        // Every graph of the repo is refused as a text encoder.
        assert!(matches!(
            crate::package::ModelPackage::discover(&t5),
            Err(crate::package::PackageError::TextGenerator(_))
        ));
        // Marian translates; its pad token is never generated.
        let marian = repo(
            &tmp.path().join("opus-mt-en-de"),
            json!({"architectures": ["MarianMTModel"], "model_type": "marian"}),
            json!({"decoder_start_token_id": 58100, "eos_token_id": 0, "bad_words_ids": [[58100]]}),
        );
        let p = Seq2SeqPackage::discover(&marian).unwrap();
        assert_eq!(
            (p.task, p.output_key()),
            ("translation", "translation_text")
        );
        assert_eq!(p.gen.bad_ids, vec![58100]);
        // The decoder graph is part of the model, not one of its own.
        assert!(Seq2SeqPackage::discover(
            &marian.with_file_name("decoder_model_merged_quantized.onnx")
        )
        .is_none());
    }

    #[test]
    fn other_models_are_not_seq2seq() {
        let tmp = tempfile::tempdir().unwrap();
        let gen = json!({"decoder_start_token_id": 0});
        // NLLB needs language tokens; Whisper is speech; a captioner is an image model.
        for (name, config) in [
            (
                "nllb",
                json!({"architectures": ["M2M100ForConditionalGeneration"], "model_type": "m2m_100"}),
            ),
            (
                "whisper",
                json!({"architectures": ["WhisperForConditionalGeneration"], "model_type": "whisper"}),
            ),
            (
                "vit-gpt2",
                json!({"architectures": ["VisionEncoderDecoderModel"], "model_type": "vision-encoder-decoder"}),
            ),
            (
                "t5-encoder",
                json!({"architectures": ["T5EncoderModel"], "model_type": "t5"}),
            ),
        ] {
            let encoder = repo(&tmp.path().join(name), config, gen.clone());
            assert!(Seq2SeqPackage::discover(&encoder).is_none(), "{name}");
        }
    }

    #[test]
    fn requests_take_one_text() {
        let job = parse(&json!({"inputs": "Hello", "parameters": {"max_new_tokens": 8}})).unwrap();
        assert_eq!(
            job,
            Seq2SeqJob {
                text: "Hello".into(),
                max_new_tokens: Some(8)
            }
        );
        assert!(parse(&json!({"inputs": "  "})).is_err());
        assert!(parse(&json!({"inputs": ["a", "b"]})).is_err());
        let tokens = |v: Value| parse(&json!({"inputs": "a", "parameters": {"max_new_tokens": v}}));
        for bad in [json!(0), json!(-1), json!(1.5), json!("5")] {
            assert!(tokens(bad.clone()).is_err(), "{bad} must be refused");
        }
        // Above the cap is accepted here; the decoder clamps it to 256.
        assert_eq!(
            tokens(json!(100_000)).unwrap().max_new_tokens,
            Some(100_000)
        );
    }
}
