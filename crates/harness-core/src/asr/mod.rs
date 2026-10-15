//! Speech recognition with sherpa-onnx: the ONNX models whisper.cpp cannot run
//! (NVIDIA Parakeet / NeMo, SenseVoice, Paraformer, Zipformer, Omnilingual, Dolphin, Cohere
//! Transcribe), run by the `sherpa-onnx-vad-with-offline-asr` tool of the sherpa-onnx runtime
//! that already serves text to speech ([`crate::tts`], [`crate::engine::SHERPA_ONNX`]).
//! Streaming transducers (NVIDIA Nemotron ASR streaming, streaming Zipformer) are run by the
//! release's online decoder `sherpa-onnx` on the whole file without the VAD (cut at pauses, a
//! streaming model loses the first words of every piece), and the segments are made from its
//! token times ([`parse_online_json`]).
//!
//! * [`AsrModel`]: a model folder found under the model roots ([`scan`]); the library lists it as
//!   [`crate::domain::ModelKind::Asr`] with the sherpa-onnx runtime (its file is `.onnx`, a
//!   whisper.cpp model is a ggml `.bin`).
//! * [`build_args`]: the command line of one run, a pure function (unit tested); [`run`] starts
//!   the process, [`catalog`] is the curated list of models to download.
//! * [`Transcript`]: text and timed segments, shaped as OpenAI's `json`, `verbose_json`, `text`,
//!   `srt` and `vtt`.
//!
//! sherpa-onnx has no HTTP server for offline models and the tool reads the model on every run,
//! so nothing stays resident: the gateway's `/v1/audio/transcriptions`, the Playground and
//! `harness transcribe` all start one process per request, like text to speech. The VAD tool is
//! used for every file, short or long: Silero VAD cuts the audio at pauses (so a model that
//! cannot take a long recording still can, and every segment has a time), and the model reads
//! one segment at a time.

pub mod catalog;
pub mod crispasr;
pub mod diarize;
pub mod llm;
pub mod onnx;
pub mod run;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error_key::err;
use crate::tts::{dir_size, file_name, files_of, quantization_of};

/// Folder under a model root where Harness puts the speech-recognition models it downloads.
pub const MODELS_SUBDIR: &str = "asr";

/// The model architectures `sherpa-onnx-vad-with-offline-asr` is run with here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AsrFamily {
    /// NVIDIA NeMo transducer (Parakeet TDT): `encoder` + `decoder` + `joiner`.
    NemoTransducer,
    /// icefall transducer (Zipformer, Conformer): `encoder` + `decoder` + `joiner`.
    Transducer,
    /// FunASR Paraformer: one `model.onnx`.
    Paraformer,
    /// SenseVoice (zh, en, ja, ko, yue): one `model.onnx`.
    SenseVoice,
    /// NeMo CTC (Citrinet, Conformer-CTC, FastConformer-CTC): one `model.onnx`.
    NemoCtc,
    /// Meta's Omnilingual ASR (1600 languages, Turkish among them): one CTC `model.onnx`.
    Omnilingual,
    /// Dolphin (Eastern languages) CTC: one `model.onnx`.
    Dolphin,
    /// A streaming transducer (NVIDIA Nemotron ASR streaming, streaming Zipformer): `encoder` +
    /// `decoder` + `joiner`, read by the online tool.
    StreamingTransducer,
    /// Cohere Transcribe (14 languages, punctuation): `encoder` + `decoder`; it needs the
    /// language named ([`cohere_language`]).
    CohereTranscribe,
}

impl AsrFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NemoTransducer => "nemo_transducer",
            Self::Transducer => "transducer",
            Self::Paraformer => "paraformer",
            Self::SenseVoice => "sense_voice",
            Self::NemoCtc => "nemo_ctc",
            Self::Omnilingual => "omnilingual",
            Self::Dolphin => "dolphin",
            Self::StreamingTransducer => "streaming_transducer",
            Self::CohereTranscribe => "cohere_transcribe",
        }
    }

    /// Run by the online decoder `sherpa-onnx` instead of the offline VAD tool.
    pub fn is_streaming(self) -> bool {
        self == Self::StreamingTransducer
    }
}

/// A speech-recognition model folder on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AsrModel {
    /// The main model file's path: the library id.
    pub id: String,
    /// Folder name (`sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`): what a request's `model`
    /// names.
    pub name: String,
    pub family: AsrFamily,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub dir: PathBuf,
    /// `model.onnx`, or the encoder of a transducer.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub model_file: PathBuf,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub decoder: Option<PathBuf>,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub joiner: Option<PathBuf>,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub tokens: PathBuf,
    pub size_bytes: u64,
    /// `int8`, `fp16` or `fp32`.
    pub quantization: String,
    /// Languages it understands, when the catalog knows the folder (empty: not known).
    pub languages: Vec<String>,
}

/// The `.onnx` file of `files` whose name starts with `prefix`: int8 before the rest.
fn pick_onnx(files: &[PathBuf], prefix: &str) -> Option<PathBuf> {
    let mut found: Vec<&PathBuf> = files
        .iter()
        .filter(|p| {
            let n = file_name(p).to_ascii_lowercase();
            n.starts_with(prefix) && n.ends_with(".onnx")
        })
        .collect();
    found.sort_by_key(|p| {
        let n = file_name(p).to_ascii_lowercase();
        (!n.contains("int8"), n)
    });
    found.first().map(|p| (*p).clone())
}

/// Whether `dir` is a speech-recognition model folder this build runs, and which: the files
/// sherpa-onnx's published archives have (`tokens.txt` plus `encoder` / `decoder` / `joiner`, or
/// one `model*.onnx` and a family name in the folder name). A transducer whose folder name says
/// `streaming` is a streaming one; Cohere Transcribe is an `encoder` + `decoder` in a `cohere`
/// folder. Other streaming models (CTC, Paraformer), Whisper (whisper.cpp runs those), Canary,
/// Moonshine and the text-to-speech folders are not recognised.
pub fn detect(dir: &Path) -> Option<AsrModel> {
    let name = file_name(dir);
    let lower = name.to_ascii_lowercase();
    if lower.contains("whisper") {
        return None;
    }
    let streaming = lower.contains("streaming");
    let files = files_of(dir);
    let tokens = files.iter().find(|p| file_name(p) == "tokens.txt")?.clone();
    let transducer = (
        pick_onnx(&files, "encoder"),
        pick_onnx(&files, "decoder"),
        pick_onnx(&files, "joiner"),
    );
    let (family, model_file, decoder, joiner) = if let (Some(e), Some(d), Some(j)) = transducer {
        let nemo = lower.contains("nemo") || lower.contains("parakeet");
        let family = if streaming {
            AsrFamily::StreamingTransducer
        } else if nemo {
            AsrFamily::NemoTransducer
        } else {
            AsrFamily::Transducer
        };
        (family, e, Some(d), Some(j))
    } else if streaming {
        return None;
    } else if let (true, (Some(e), Some(d), None)) = (lower.contains("cohere"), transducer) {
        (AsrFamily::CohereTranscribe, e, Some(d), None)
    } else {
        let model = pick_onnx(&files, "model")?;
        let family = if lower.contains("sense-voice") || lower.contains("sensevoice") {
            AsrFamily::SenseVoice
        } else if lower.contains("paraformer") {
            AsrFamily::Paraformer
        } else if lower.contains("omnilingual") {
            AsrFamily::Omnilingual
        } else if lower.contains("dolphin") {
            AsrFamily::Dolphin
        } else if lower.contains("nemo") && lower.contains("ctc") {
            AsrFamily::NemoCtc
        } else {
            return None;
        };
        (family, model, None, None)
    };
    let languages = catalog::find(&name)
        .map(|e| e.languages)
        .unwrap_or_default();
    Some(AsrModel {
        id: model_file.to_string_lossy().to_string(),
        quantization: quantization_of(&file_name(&model_file)),
        size_bytes: dir_size(dir),
        name,
        family,
        dir: dir.to_path_buf(),
        model_file,
        decoder,
        joiner,
        tokens,
        languages,
    })
}

/// The speech-recognition models under `roots`: folders up to a few levels deep, the models
/// Harness downloads (`<root>/asr/<name>`) and ones the user unpacked anywhere in a root.
pub fn scan(roots: &[PathBuf]) -> Vec<AsrModel> {
    let mut out = crate::tts::scan_folders(roots, detect, |a, b| a.id == b.id);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// [`scan`] of the configured model directories.
pub fn installed_models() -> Vec<AsrModel> {
    scan(&crate::config::AppSettings::load().all_models_dirs())
}

/// HF CTC packages run on the resident ONNX engine.
pub fn is_onnx_ctc_file(path: &Path) -> bool {
    harness_onnx::package::library_task(path) == Some(harness_onnx::package::Task::AsrCtc)
}

/// A sherpa-onnx ASR graph, excluding HF CTC packages served by the ONNX engine.
pub fn is_sherpa_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        && !is_onnx_ctc_file(path)
}

/// The library's view of a model folder.
pub fn to_local_model(model: &AsrModel, root: &Path) -> crate::models::LocalModel {
    crate::models::LocalModel {
        id: model.id.clone(),
        name: model.name.clone(),
        filename: file_name(&model.model_file),
        path: model.model_file.clone(),
        size_bytes: model.size_bytes,
        architecture: Some(model.family.as_str().to_string()),
        context_length: None,
        quantization: model.quantization.clone(),
        is_split: false,
        total_shards: None,
        present_shards: None,
        is_ready: true,
        shard_paths: files_of(&model.dir),
        root_dir: root.to_string_lossy().to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: crate::domain::ModelKind::Asr,
        origin: crate::domain::ModelOrigin::Library,
        task: None,
    }
}

pub(crate) fn library_models_in(
    root: &Path,
    tree: &crate::models::discovery::DirectoryTree,
) -> Vec<crate::models::LocalModel> {
    let mut models = tree.scan(root, &["tokens.txt"], detect, |m| m.id.clone());
    models.sort_by(|a, b| a.name.cmp(&b.name));
    models
        .into_iter()
        .map(|m| to_local_model(&m, root))
        .collect()
}

/// Models of the library roots as library files (`models::ModelManager` adds them to its scan).
pub fn library_models(roots: &[PathBuf]) -> Vec<crate::models::LocalModel> {
    roots
        .iter()
        .flat_map(|root| {
            scan(std::slice::from_ref(root))
                .into_iter()
                .map(|m| to_local_model(&m, root))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Removes a model folder (everything in it); refuses anything that is not a model folder
/// inside one of `roots`.
pub fn delete_model_dir(model_file: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let _mutation = crate::models::LibraryMutation::begin();
    let dir = model_file.parent().ok_or_else(|| {
        String::from(err("stt.model_not_found").with("model", model_file.display()))
    })?;
    if detect(dir).is_none() {
        return Err(err("stt.model_not_found")
            .with("model", model_file.display())
            .into());
    }
    let inside = roots.iter().any(|r| {
        crate::paths::is_within(dir, r)
            && crate::paths::normalize(dir) != crate::paths::normalize(r)
    });
    if !inside {
        return Err(err("library.outside_library")
            .with("path", dir.display())
            .into());
    }
    std::fs::remove_dir_all(dir).map_err(|e| {
        String::from(
            err("fs.remove_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })
}

/// The model a request's `model` field names: the folder name (case does not matter) or the
/// model file's path. `exact_only` skips the unique-part match (`parakeet`) the CLI and the
/// Playground allow; the gateway uses it so that a part of a name never takes a request away
/// from a whisper definition. Nothing named: the first model.
pub fn find_model<'a>(
    models: &'a [AsrModel],
    wanted: Option<&str>,
    exact_only: bool,
) -> Result<&'a AsrModel, String> {
    let wanted = wanted.map(str::trim).filter(|w| !w.is_empty());
    let Some(wanted) = wanted else {
        return models
            .first()
            .ok_or_else(|| String::from(err("stt.no_model")));
    };
    let lower = wanted.to_ascii_lowercase();
    if let Some(m) = models
        .iter()
        .find(|m| m.name.to_ascii_lowercase() == lower || m.id == wanted)
    {
        return Ok(m);
    }
    let partial: Vec<&AsrModel> = if exact_only {
        Vec::new()
    } else {
        models
            .iter()
            .filter(|m| m.name.to_ascii_lowercase().contains(&lower))
            .collect()
    };
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(err("stt.model_not_found").with("model", wanted).into()),
        many => Err(err("stt.model_ambiguous")
            .with("model", wanted)
            .with(
                "candidates",
                many.iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .into()),
    }
}

/// What one run needs besides the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// `silero_vad.onnx`.
    pub vad: PathBuf,
    /// A spoken language (`tr`, `en`), only SenseVoice takes one.
    pub language: Option<String>,
    pub threads: u32,
}

/// Longest speech segment the VAD hands to the model, in seconds (the tool's default is 20;
/// the models here read 25 s as well as 20 and it makes fewer cuts in the middle of a sentence).
pub const MAX_SEGMENT_SECS: u32 = 25;

/// SenseVoice's language names, from a request's `language` (`zh`, `Chinese`, `ja-JP`); `auto`
/// for anything else.
pub fn sense_voice_language(language: Option<&str>) -> &'static str {
    let Some(l) = language.map(|l| l.trim().to_ascii_lowercase()) else {
        return "auto";
    };
    match l.split(['-', '_']).next().unwrap_or_default() {
        "zh" | "chinese" | "mandarin" => "zh",
        "en" | "english" => "en",
        "ja" | "jp" | "japanese" => "ja",
        "ko" | "korean" => "ko",
        "yue" | "cantonese" => "yue",
        _ => "auto",
    }
}

/// Cohere Transcribe's languages (it has no automatic detection).
pub const COHERE_LANGUAGES: [&str; 14] = [
    "en", "fr", "de", "it", "es", "pt", "el", "nl", "pl", "zh", "ja", "ko", "vi", "ar",
];

/// Cohere Transcribe's language for a request's `language` (`de`, `de-DE`, `German`): the
/// two-letter code when it is one of [`COHERE_LANGUAGES`], `None` for another one. No language
/// at all is English.
pub fn cohere_language(language: Option<&str>) -> Option<&'static str> {
    let Some(l) = language
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
    else {
        return Some("en");
    };
    let code = l.split(['-', '_']).next().unwrap_or_default();
    let code = match code {
        "english" => "en",
        "french" => "fr",
        "german" => "de",
        "italian" => "it",
        "spanish" => "es",
        "portuguese" => "pt",
        "greek" => "el",
        "dutch" => "nl",
        "polish" => "pl",
        "chinese" | "mandarin" => "zh",
        "japanese" => "ja",
        "korean" => "ko",
        "vietnamese" => "vi",
        "arabic" => "ar",
        other => other,
    };
    COHERE_LANGUAGES.iter().find(|c| **c == code).copied()
}

fn path_arg(flag: &str, path: &Path) -> String {
    format!("{flag}={}", path.display())
}

/// The command line of one run, without the audio file (the last argument, see [`run`]).
pub fn build_args(model: &AsrModel, params: &Params) -> Vec<String> {
    if model.family.is_streaming() {
        // The online decoder: the whole file, no VAD; one result at the end.
        let mut args = vec![
            path_arg("--tokens", &model.tokens),
            path_arg("--encoder", &model.model_file),
        ];
        if let Some(d) = &model.decoder {
            args.push(path_arg("--decoder", d));
        }
        if let Some(j) = &model.joiner {
            args.push(path_arg("--joiner", j));
        }
        args.push("--enable-endpoint=false".to_string());
        args.push(format!("--num-threads={}", params.threads.max(1)));
        args.push("--print-args=false".to_string());
        return args;
    }
    let mut args = vec![
        path_arg("--silero-vad-model", &params.vad),
        format!("--silero-vad-max-speech-duration={MAX_SEGMENT_SECS}"),
        path_arg("--tokens", &model.tokens),
    ];
    match model.family {
        AsrFamily::NemoTransducer | AsrFamily::Transducer => {
            args.push(path_arg("--encoder", &model.model_file));
            if let Some(d) = &model.decoder {
                args.push(path_arg("--decoder", d));
            }
            if let Some(j) = &model.joiner {
                args.push(path_arg("--joiner", j));
            }
            args.push(format!(
                "--model-type={}",
                if model.family == AsrFamily::NemoTransducer {
                    "nemo_transducer"
                } else {
                    "transducer"
                }
            ));
        }
        AsrFamily::Paraformer => {
            args.push(path_arg("--paraformer", &model.model_file));
            args.push("--model-type=paraformer".to_string());
        }
        AsrFamily::SenseVoice => {
            args.push(path_arg("--sense-voice-model", &model.model_file));
            // Punctuation, casing and numbers as digits.
            args.push("--sense-voice-use-itn=true".to_string());
            args.push(format!(
                "--sense-voice-language={}",
                sense_voice_language(params.language.as_deref())
            ));
        }
        AsrFamily::NemoCtc => {
            args.push(path_arg("--nemo-ctc-model", &model.model_file));
            args.push("--model-type=nemo_ctc".to_string());
        }
        AsrFamily::Omnilingual => args.push(path_arg("--omnilingual-asr-model", &model.model_file)),
        AsrFamily::Dolphin => args.push(path_arg("--dolphin-model", &model.model_file)),
        AsrFamily::StreamingTransducer => unreachable!("built above"),
        AsrFamily::CohereTranscribe => {
            args.push(path_arg("--cohere-transcribe-encoder", &model.model_file));
            if let Some(d) = &model.decoder {
                args.push(path_arg("--cohere-transcribe-decoder", d));
            }
            args.push(format!(
                "--cohere-transcribe-language={}",
                cohere_language(params.language.as_deref()).unwrap_or("en")
            ));
            args.push("--cohere-transcribe-use-punct=true".to_string());
            args.push("--cohere-transcribe-use-itn=true".to_string());
        }
    }
    args.push(format!("--num-threads={}", params.threads.max(1)));
    // The tool echoes its arguments (the model paths) to stderr by default.
    args.push("--print-args=false".to_string());
    args
}

/// One recognised stretch of speech.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Who said it (`A`, `B`...), when the transcript was diarized ([`diarize::label`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// The text of a transcription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
    /// Length of the audio in seconds.
    pub duration_secs: f64,
    /// The language a request asked for, if it did.
    pub language: Option<String>,
    pub model: String,
    /// Wall time of the run (model load included).
    pub elapsed_ms: u64,
}

/// Parses the lines `start -- end: text` of the VAD tool's stdout. `pad` seconds of silence
/// were put in front of the audio ([`run::PAD_SECS`]): times are moved back by it and kept inside
/// `0..=duration`. Segments without text are dropped.
pub fn parse_output(stdout: &str, pad: f64, duration: f64) -> Vec<Segment> {
    static LINE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = LINE.get_or_init(|| {
        regex::Regex::new(r"^\s*(\d+(?:\.\d+)?) -- (\d+(?:\.\d+)?):\s?(.*)$").expect("valid regex")
    });
    let clamp = |t: f64| (t - pad).clamp(0.0, duration.max(0.0));
    stdout
        .lines()
        .filter_map(|line| {
            let c = re.captures(line.trim_end())?;
            let text = c[3].trim().to_string();
            if text.is_empty() {
                return None;
            }
            let (start, end) = (clamp(c[1].parse().ok()?), clamp(c[2].parse().ok()?));
            Some(Segment {
                start,
                end: end.max(start),
                text,
                speaker: None,
            })
        })
        .collect()
}

/// A pause between two words this long (seconds) starts a new segment.
const SEGMENT_PAUSE: f64 = 1.0;
/// After a sentence end, a pause this long starts a new segment.
const SENTENCE_PAUSE: f64 = 0.5;
/// How long the last word piece of a segment is taken to last (the decoder gives start times
/// only).
const TOKEN_SECS: f64 = 0.25;

/// Segments from the online decoder's result: the JSON line (`tokens` and their `timestamps`) it
/// writes to stderr. Pauses are measured between word pieces (the decoder emits punctuation and
/// spaces late, right before the next word): a segment ends at a pause of [`SEGMENT_PAUSE`], at a
/// sentence end followed by [`SENTENCE_PAUSE`], or when it reaches [`MAX_SEGMENT_SECS`]. Times are
/// handled as in [`parse_output`].
pub fn parse_online_json(stderr: &str, pad: f64, duration: f64) -> Vec<Segment> {
    #[derive(Deserialize)]
    struct Online {
        tokens: Vec<String>,
        timestamps: Vec<f64>,
    }
    let Some(result) = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with('{'))
        .find_map(|l| serde_json::from_str::<Online>(l.trim()).ok())
    else {
        return Vec::new();
    };
    let clamp = |t: f64| (t - pad).clamp(0.0, duration.max(0.0));
    let mut out = Vec::new();
    let mut text = String::new();
    // Start time and last word-piece time of the open segment; a sentence end seen after it.
    let mut first: Option<f64> = None;
    let mut last = 0.0;
    let mut sentence_end = false;
    let mut close = |text: &mut String, first: Option<f64>, end: f64| {
        let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let (Some(start), false) = (first, t.is_empty()) {
            let (start, end) = (clamp(start), clamp(end));
            out.push(Segment {
                start,
                end: end.max(start),
                text: t,
                speaker: None,
            });
        }
        text.clear();
    };
    for (tok, at) in result.tokens.iter().zip(result.timestamps.iter().copied()) {
        let word = tok.chars().any(char::is_alphanumeric);
        if word {
            if let Some(start) = first {
                let gap = at - last;
                if gap >= SEGMENT_PAUSE
                    || (sentence_end && gap >= SENTENCE_PAUSE)
                    || at - start >= f64::from(MAX_SEGMENT_SECS)
                {
                    close(&mut text, first, (last + TOKEN_SECS).min(at));
                    first = None;
                }
            }
            first.get_or_insert(at);
            last = at;
            sentence_end = false;
        } else if tok.trim_end().ends_with(['.', '?', '!', '。', '？', '！']) {
            sentence_end = true;
        }
        text.push_str(tok);
    }
    close(&mut text, first, last + TOKEN_SECS);
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xFF00..=0xFFEF)
}

/// The segments as one text: a space between them, none between two CJK characters.
pub fn join_segments(segments: &[Segment]) -> String {
    let mut out = String::new();
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        if let (Some(last), Some(first)) = (out.chars().last(), text.chars().next()) {
            if !(is_cjk(last) && is_cjk(first)) {
                out.push(' ');
            }
        }
        out.push_str(text);
    }
    out
}

/// `00:01:02,500` (SRT) or `00:01:02.500` (WebVTT).
pub fn format_time(secs: f64, separator: char) -> String {
    let total_ms = (secs.max(0.0) * 1000.0).round() as u64;
    let (h, m, s, ms) = (
        total_ms / 3_600_000,
        total_ms / 60_000 % 60,
        total_ms / 1000 % 60,
        total_ms % 1000,
    );
    format!("{h:02}:{m:02}:{s:02}{separator}{ms:03}")
}

impl Transcript {
    pub fn new(
        segments: Vec<Segment>,
        duration_secs: f64,
        language: Option<String>,
        model: String,
        elapsed_ms: u64,
    ) -> Self {
        Self {
            text: join_segments(&segments),
            segments,
            duration_secs,
            language,
            model,
            elapsed_ms,
        }
    }

    /// SubRip subtitles, one cue per segment (`A: text` when diarized).
    pub fn to_srt(&self) -> String {
        let mut out = String::new();
        for (i, s) in self.segments.iter().enumerate() {
            let text = match &s.speaker {
                Some(sp) => format!("{sp}: {}", s.text),
                None => s.text.clone(),
            };
            out.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                i + 1,
                format_time(s.start, ','),
                format_time(s.end, ','),
                text
            ));
        }
        out
    }

    /// WebVTT subtitles, one cue per segment (a `<v A>` voice span when diarized).
    pub fn to_vtt(&self) -> String {
        let mut out = String::from("WEBVTT\n\n");
        for s in &self.segments {
            let text = match &s.speaker {
                Some(sp) => format!("<v {sp}>{}", s.text),
                None => s.text.clone(),
            };
            out.push_str(&format!(
                "{} --> {}\n{}\n\n",
                format_time(s.start, '.'),
                format_time(s.end, '.'),
                text
            ));
        }
        out
    }

    /// OpenAI's `diarized_json`: the text, `duration` and segments with their `speaker`.
    pub fn to_diarized_json(&self, task: &str) -> serde_json::Value {
        serde_json::json!({
            "task": task,
            "duration": (self.duration_secs * 1000.0).round() / 1000.0,
            "text": self.text,
            "segments": self.segments.iter().enumerate().map(|(i, s)| serde_json::json!({
                "type": "transcript.text.segment",
                "id": format!("seg_{i}"),
                "start": (s.start * 1000.0).round() / 1000.0,
                "end": (s.end * 1000.0).round() / 1000.0,
                "text": s.text,
                "speaker": s.speaker,
            })).collect::<Vec<_>>(),
        })
    }

    /// OpenAI's `json`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "text": self.text })
    }

    /// OpenAI's `verbose_json`: the text with `duration` and the timed `segments`.
    pub fn to_verbose_json(&self, task: &str) -> serde_json::Value {
        let mut v = serde_json::json!({
            "task": task,
            "duration": (self.duration_secs * 1000.0).round() / 1000.0,
            "text": self.text,
            "segments": self.segments.iter().enumerate().map(|(i, s)| serde_json::json!({
                "id": i,
                "start": (s.start * 1000.0).round() / 1000.0,
                "end": (s.end * 1000.0).round() / 1000.0,
                "text": s.text,
            })).collect::<Vec<_>>(),
        });
        // Diarized: each segment says who spoke.
        for (seg, s) in v["segments"]
            .as_array_mut()
            .into_iter()
            .flatten()
            .zip(&self.segments)
        {
            if let Some(sp) = &s.speaker {
                seg["speaker"] = serde_json::Value::String(sp.clone());
            }
        }
        if let Some(l) = &self.language {
            v["language"] = serde_json::Value::String(l.clone());
        }
        v
    }
}

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod tests;
