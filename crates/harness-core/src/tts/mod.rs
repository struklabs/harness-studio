//! Text to speech: sherpa-onnx's `sherpa-onnx-offline-tts` run once per request.
//!
//! * [`TtsModel`]: a model folder (Kokoro, Piper / VITS) found under the model roots
//!   ([`scan`]); the library lists it as [`crate::domain::ModelKind::Tts`].
//! * [`Voice`]s and how a request's `voice` (an OpenAI name such as `alloy`, a model voice
//!   name or a speaker number) becomes the speaker id ([`TtsModel::resolve_voice`]).
//! * [`build_args`]: the command line of one run, a pure function (unit tested); [`run`]
//!   starts the process, [`catalog`] is the curated list of models to download.
//!
//! sherpa-onnx has no HTTP server, and its TTS tool reads the model on every run (Kokoro int8
//! takes about 1.5 s to load, a Piper voice 0.3 s), so nothing stays resident: the gateway's
//! `/v1/audio/speech`, the Playground and `harness speak` all spawn the tool of the installed
//! `sherpa-onnx` runtime ([`crate::engine::SHERPA_ONNX`]) for the request. The text goes in as
//! the last argument, see [`run::text_argument`] for the Windows code page problem.

pub mod catalog;
pub mod llama;
pub mod run;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error_key::err;

/// OpenAI's limit for `input`; longer text is the caller's to split (the Playground does).
pub const MAX_INPUT_CHARS: usize = 4096;
/// OpenAI's range for `speed`.
pub const MIN_SPEED: f32 = 0.25;
pub const MAX_SPEED: f32 = 4.0;
/// Folder under a model root where Harness puts the models it downloads.
pub const MODELS_SUBDIR: &str = "tts";
/// Model aliases an OpenAI client sends; they mean "the default model".
pub const DEFAULT_MODEL_ALIASES: [&str; 3] = ["tts-1", "tts-1-hd", "gpt-4o-mini-tts"];

/// The model architectures `sherpa-onnx-offline-tts` is run with here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum TtsFamily {
    /// Kokoro (Apache-2.0): many voices, several languages in one model.
    Kokoro,
    /// VITS, which is what Piper voices are (one voice per model, usually one language).
    Vits,
    /// Matcha-TTS: an acoustic model plus a separate vocoder (HiFi-GAN or Vocos).
    Matcha,
    /// KittenTTS: one small model with a `voices.bin`, espeak-ng phonemes.
    Kitten,
}

impl TtsFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kokoro => "kokoro",
            Self::Vits => "vits",
            Self::Matcha => "matcha",
            Self::Kitten => "kitten",
        }
    }
}

/// One speaker of a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Voice {
    /// The name a request uses (`af_heart`, `0`).
    pub id: String,
    /// Speaker id the tool takes (`--sid`).
    pub sid: u32,
    /// Language tag (`en-US`, `tr`); empty when the model does not say.
    pub language: String,
    /// `female`, `male` or empty.
    pub gender: String,
}

/// A text-to-speech model folder on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TtsModel {
    /// The model file's path: the library id.
    pub id: String,
    /// Folder name (`kokoro-int8-multi-lang-v1_0`): what a request's `model` names.
    pub name: String,
    pub family: TtsFamily,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub dir: PathBuf,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub model_file: PathBuf,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub tokens: PathBuf,
    /// `espeak-ng-data` (phonemes), when the model uses it.
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub data_dir: Option<PathBuf>,
    /// Kokoro `voices.bin`.
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub voices_file: Option<PathBuf>,
    /// Matcha's vocoder (`hifigan*.onnx`, `vocos*.onnx`); `model_file` is its acoustic model.
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub vocoder: Option<PathBuf>,
    /// `lexicon*.txt` files of the folder.
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<String>))]
    pub lexicons: Vec<PathBuf>,
    /// Chinese text-normalisation rules (`phone-zh.fst`, `date-zh.fst`, `number-zh.fst`).
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<String>))]
    pub rule_fsts: Vec<PathBuf>,
    pub size_bytes: u64,
    /// `int8`, `fp16` or `fp32`.
    pub quantization: String,
    /// Languages it speaks (`en`, `tr`...).
    pub languages: Vec<String>,
    pub sample_rate: Option<u32>,
    pub voices: Vec<Voice>,
}

/// Families of sherpa-onnx models that have their own folder layout and are not run here.
const OTHER_FAMILIES: [&str; 4] = ["pocket", "supertonic", "zipvoice", "nabra"];

/// Kokoro v1.0's speakers in `voices.bin` order (checked with `--debug` on
/// `kokoro-int8-multi-lang-v1_0`: `n_speakers=54`, `em_santa` is 53).
const KOKORO_V1_VOICES: [&str; 54] = [
    "af_alloy",
    "af_aoede",
    "af_bella",
    "af_heart",
    "af_jessica",
    "af_kore",
    "af_nicole",
    "af_nova",
    "af_river",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_echo",
    "am_eric",
    "am_fenrir",
    "am_liam",
    "am_michael",
    "am_onyx",
    "am_puck",
    "am_santa",
    "bf_alice",
    "bf_emma",
    "bf_isabella",
    "bf_lily",
    "bm_daniel",
    "bm_fable",
    "bm_george",
    "bm_lewis",
    "ef_dora",
    "em_alex",
    "ff_siwis",
    "hf_alpha",
    "hf_beta",
    "hm_omega",
    "hm_psi",
    "if_sara",
    "im_nicola",
    "jf_alpha",
    "jf_gongitsune",
    "jf_nezumi",
    "jf_tebukuro",
    "jm_kumo",
    "pf_dora",
    "pm_alex",
    "pm_santa",
    "zf_xiaobei",
    "zf_xiaoni",
    "zf_xiaoxiao",
    "zf_xiaoyi",
    "zm_yunjian",
    "zm_yunxi",
    "zm_yunxia",
    "zm_yunyang",
    "em_santa",
];

/// Kokoro v0.19 (English only) speakers in order.
const KOKORO_V019_VOICES: [&str; 11] = [
    "af",
    "af_bella",
    "af_nicole",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_michael",
    "bf_emma",
    "bf_isabella",
    "bm_george",
    "bm_lewis",
];

/// OpenAI's voice names, as the Kokoro voices that stand in for them.
const OPENAI_VOICES: [(&str, &str); 11] = [
    ("alloy", "af_alloy"),
    ("ash", "am_adam"),
    ("ballad", "bm_george"),
    ("coral", "af_kore"),
    ("echo", "am_echo"),
    ("fable", "bm_fable"),
    ("nova", "af_nova"),
    ("onyx", "am_onyx"),
    ("sage", "af_sarah"),
    ("shimmer", "af_bella"),
    ("verse", "am_michael"),
];

/// Voice used when a request names none.
const KOKORO_DEFAULT_VOICE: &str = "af_heart";

/// `(language tag, gender)` of a Kokoro voice from its prefix (`af_` = American English female).
fn kokoro_voice_traits(name: &str) -> (&'static str, &'static str) {
    let mut chars = name.chars();
    let (lang, gender) = (chars.next(), chars.next());
    let language = match lang {
        Some('a') => "en-US",
        Some('b') => "en-GB",
        Some('e') => "es",
        Some('f') => "fr",
        Some('h') => "hi",
        Some('i') => "it",
        Some('j') => "ja",
        Some('p') => "pt-BR",
        Some('z') => "zh",
        _ => "",
    };
    let gender = match gender {
        Some('f') => "female",
        Some('m') => "male",
        _ => "",
    };
    (language, gender)
}

/// The espeak-ng / lexicon language of a Kokoro voice.
fn kokoro_lang_arg(voice: &str) -> Option<&'static str> {
    match voice.chars().next() {
        Some('e') => Some("es"),
        Some('f') => Some("fr"),
        Some('h') => Some("hi"),
        Some('i') => Some("it"),
        Some('j') => Some("ja"),
        Some('p') => Some("pt-br"),
        _ => None,
    }
}

fn kokoro_voices(name: &str) -> Vec<Voice> {
    let table: &[&str] = if name.contains("v0_19") {
        &KOKORO_V019_VOICES
    } else if name.contains("v1_0") {
        &KOKORO_V1_VOICES
    } else {
        &[]
    };
    table
        .iter()
        .enumerate()
        .map(|(sid, id)| {
            let (language, gender) = kokoro_voice_traits(id);
            Voice {
                id: (*id).to_string(),
                sid: sid as u32,
                language: language.to_string(),
                gender: gender.to_string(),
            }
        })
        .collect()
}

/// KittenTTS nano v0.1's eight speakers in `voices.bin` order (sherpa-onnx's model page:
/// `expr-voice-2-m` is speaker 0 ... `expr-voice-5-f` is speaker 7).
fn kitten_voices() -> Vec<Voice> {
    ["2-m", "2-f", "3-m", "3-f", "4-m", "4-f", "5-m", "5-f"]
        .iter()
        .enumerate()
        .map(|(sid, v)| Voice {
            id: format!("expr-voice-{v}"),
            sid: sid as u32,
            language: "en-US".to_string(),
            gender: if v.ends_with('m') { "male" } else { "female" }.to_string(),
        })
        .collect()
}

fn kokoro_languages(name: &str) -> Vec<String> {
    let langs: &[&str] = if name.contains("v1_0") {
        &["en", "es", "fr", "hi", "it", "ja", "pt", "zh"]
    } else if name.contains("v1_1") {
        &["en", "zh"]
    } else {
        &["en"]
    };
    langs.iter().map(|l| (*l).to_string()).collect()
}

/// The parts of a Piper `<voice>.onnx.json` the tool needs.
#[derive(Debug, Default, Deserialize)]
struct PiperConfig {
    #[serde(default)]
    audio: PiperAudio,
    #[serde(default)]
    espeak: PiperEspeak,
    #[serde(default)]
    language: PiperLanguage,
    #[serde(default)]
    num_speakers: u32,
    #[serde(default)]
    speaker_id_map: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Default, Deserialize)]
struct PiperAudio {
    #[serde(default)]
    sample_rate: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct PiperEspeak {
    #[serde(default)]
    voice: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PiperLanguage {
    #[serde(default)]
    code: Option<String>,
}

/// The part of an MMS training `config.json` read here.
#[derive(Debug, Deserialize)]
struct MmsConfig {
    data: MmsData,
}

#[derive(Debug, Deserialize)]
struct MmsData {
    sampling_rate: u32,
}

/// MMS models speak at 16 kHz.
const MMS_SAMPLE_RATE: u32 = 16_000;

/// The language of an MMS folder (`vits-mms-deu`): MMS names languages with ISO 639-3 codes;
/// the ones that have a two-letter code get it, like the other models' languages.
fn mms_language(code: &str) -> String {
    let two = match code {
        "deu" => "de",
        "eng" => "en",
        "fra" => "fr",
        "rus" => "ru",
        "spa" => "es",
        "tha" => "th",
        "tur" => "tr",
        "ukr" => "uk",
        other => other,
    };
    two.to_string()
}

/// `tr_TR` / `en-US` / `tr` -> `(tr, TR)`.
fn split_language(code: &str) -> (String, Option<String>) {
    let mut parts = code.split(['_', '-']);
    let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
    let region = parts.next().map(|r| r.to_ascii_uppercase());
    (lang, region)
}

/// Language code in a Piper folder name (`vits-piper-tr_TR-fettah-medium-int8` -> `tr_TR`).
fn language_in_name(name: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?:^|[-_])([a-z]{2,3}_[A-Z]{2})(?:[-_]|$)").ok()?;
    re.captures(name).map(|c| c[1].to_string())
}

pub(crate) fn files_of(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files
}

pub(crate) fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t) if t.is_file() => {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    total
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// The model file of a folder: the only `.onnx`, or of several the int8, then fp16, then the
/// first.
fn pick_onnx(files: &[PathBuf]) -> Option<PathBuf> {
    let mut onnx: Vec<&PathBuf> = files
        .iter()
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        })
        .collect();
    onnx.sort_by_key(|p| {
        let n = file_name(p).to_ascii_lowercase();
        (!n.contains("int8"), !n.contains("fp16"), n)
    });
    onnx.first().map(|p| (*p).clone())
}

pub(crate) fn quantization_of(file: &str) -> String {
    let lower = file.to_ascii_lowercase();
    if lower.contains("int8") {
        "int8".into()
    } else if lower.contains("fp16") {
        "fp16".into()
    } else {
        "fp32".into()
    }
}

/// A Matcha vocoder's file name (it is a separate graph beside the acoustic model).
fn is_vocoder_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".onnx") && (n.contains("hifigan") || n.contains("vocos") || n.contains("vocoder"))
}

/// The family a sherpa-onnx text-to-speech folder is, from the folder (or repo) `name` and the
/// names of the files at its root; `espeak` is whether it has an `espeak-ng-data` folder. One
/// function for the local scan ([`detect`]) and for the Hub listing ([`crate::sherpa_layout`]),
/// so that a repo shown as runnable is one the library recognises after the download.
pub fn classify(name: &str, files: &[String], espeak: bool) -> Option<TtsFamily> {
    let lower = name.to_ascii_lowercase();
    if OTHER_FAMILIES.iter().any(|f| lower.contains(f)) {
        return None;
    }
    let has = |n: &str| files.iter().any(|f| f.eq_ignore_ascii_case(n));
    let onnx: Vec<&String> = files
        .iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".onnx"))
        .collect();
    if !has("tokens.txt") || onnx.is_empty() {
        return None;
    }
    let lexicon = files.iter().any(|f| {
        let n = f.to_ascii_lowercase();
        n.starts_with("lexicon") && n.ends_with(".txt")
    });
    if has("voices.bin") {
        return if lower.contains("kitten") {
            espeak.then_some(TtsFamily::Kitten)
        } else if lower.contains("kokoro") || lexicon {
            Some(TtsFamily::Kokoro)
        } else {
            None
        };
    }
    let matcha_named = lower.contains("matcha")
        || onnx
            .iter()
            .any(|f| f.to_ascii_lowercase().starts_with("model-steps"));
    if matcha_named {
        // An acoustic model alone cannot make a waveform: the vocoder must be there too.
        let vocoder = onnx.iter().any(|f| is_vocoder_file(f));
        return (vocoder && onnx.len() > 1 && (espeak || lexicon)).then_some(TtsFamily::Matcha);
    }
    if espeak || lexicon || lower.starts_with("vits-mms-") {
        return Some(TtsFamily::Vits);
    }
    None
}

/// The Matcha acoustic model and vocoder of a folder's `.onnx` files.
fn pick_matcha(files: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let vocoder = files
        .iter()
        .find(|p| is_vocoder_file(&file_name(p)))?
        .clone();
    let rest: Vec<PathBuf> = files
        .iter()
        .filter(|p| !is_vocoder_file(&file_name(p)))
        .cloned()
        .collect();
    Some((pick_onnx(&rest)?, vocoder))
}

/// Whether `dir` is a text-to-speech model folder, and which: the files sherpa-onnx's
/// published archives have (`tokens.txt` plus the model, and `voices.bin` for Kokoro or Kitten,
/// `espeak-ng-data` / `lexicon.txt` for VITS and Matcha (which also has a vocoder), a
/// `vits-mms-<language>` name for MMS), see [`classify`]. The other sherpa-onnx families
/// (Pocket...) are not recognised.
pub fn detect(dir: &Path) -> Option<TtsModel> {
    let name = file_name(dir);
    let lower = name.to_ascii_lowercase();
    let files = files_of(dir);
    let names: Vec<String> = files.iter().map(|p| file_name(p)).collect();
    let espeak = dir.join("espeak-ng-data").is_dir();
    let class = classify(&name, &names, espeak)?;
    let find = |n: &str| files.iter().find(|p| file_name(p) == n).cloned();
    let tokens = find("tokens.txt")?;
    let (model_file, vocoder) = if class == TtsFamily::Matcha {
        let (m, v) = pick_matcha(&files)?;
        (m, Some(v))
    } else {
        (pick_onnx(&files)?, None)
    };
    let data_dir = Some(dir.join("espeak-ng-data")).filter(|p| p.is_dir());
    let lexicons: Vec<PathBuf> = files
        .iter()
        .filter(|p| {
            let n = file_name(p).to_ascii_lowercase();
            n.starts_with("lexicon") && n.ends_with(".txt")
        })
        .cloned()
        .collect();
    let rule_fsts: Vec<PathBuf> = ["phone-zh.fst", "date-zh.fst", "number-zh.fst"]
        .iter()
        .filter_map(|n| find(n))
        .collect();
    let voices_file = find("voices.bin");
    let quantization = quantization_of(&file_name(&model_file));

    let (family, languages, sample_rate, voices) = if class == TtsFamily::Kokoro {
        (
            TtsFamily::Kokoro,
            kokoro_languages(&lower),
            Some(24_000),
            kokoro_voices(&lower),
        )
    } else if class == TtsFamily::Kitten {
        (
            TtsFamily::Kitten,
            vec!["en".to_string()],
            Some(24_000),
            kitten_voices(),
        )
    } else if class == TtsFamily::Matcha {
        let code = language_in_name(&name);
        let (lang, region) = code.as_deref().map(split_language).unwrap_or_default();
        let tag = match &region {
            Some(r) if !lang.is_empty() => format!("{lang}-{r}"),
            _ => lang.clone(),
        };
        (
            TtsFamily::Matcha,
            if lang.is_empty() {
                Vec::new()
            } else {
                vec![lang]
            },
            None,
            vec![Voice {
                id: name.clone(),
                sid: 0,
                language: tag,
                gender: String::new(),
            }],
        )
    } else if data_dir.is_some() || !lexicons.is_empty() {
        let config_path = model_file.with_extension("onnx.json");
        let config: PiperConfig = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let code = config
            .language
            .code
            .clone()
            .or_else(|| language_in_name(&name))
            .or(config.espeak.voice.clone());
        let (lang, region) = code.as_deref().map(split_language).unwrap_or_default();
        let tag = match &region {
            Some(r) if !lang.is_empty() => format!("{lang}-{r}"),
            _ => lang.clone(),
        };
        let mut voices: Vec<Voice> = if config.speaker_id_map.len() > 1 {
            let mut by_sid: Vec<(&String, &u32)> = config.speaker_id_map.iter().collect();
            by_sid.sort_by_key(|(_, sid)| **sid);
            by_sid
                .into_iter()
                .map(|(id, sid)| Voice {
                    id: id.clone(),
                    sid: *sid,
                    language: tag.clone(),
                    gender: String::new(),
                })
                .collect()
        } else if config.num_speakers > 1 {
            (0..config.num_speakers)
                .map(|sid| Voice {
                    id: sid.to_string(),
                    sid,
                    language: tag.clone(),
                    gender: String::new(),
                })
                .collect()
        } else {
            Vec::new()
        };
        if voices.is_empty() {
            // One speaker: the folder's own voice name (`fettah`).
            let voice = name
                .strip_prefix("vits-piper-")
                .map(|rest| rest.split('-').nth(1).unwrap_or(rest).to_string())
                .unwrap_or_else(|| name.clone());
            voices.push(Voice {
                id: voice,
                sid: 0,
                language: tag.clone(),
                gender: String::new(),
            });
        }
        (
            TtsFamily::Vits,
            if lang.is_empty() {
                Vec::new()
            } else {
                vec![lang]
            },
            config.audio.sample_rate,
            voices,
        )
    } else {
        let code = lower.strip_prefix("vits-mms-")?;
        // MMS (facebook/mms-tts) as sherpa-onnx exports it: letters map straight to tokens, so
        // there is no phoneme data; one speaker; the rate is in the training `config.json`.
        let rate = std::fs::read_to_string(dir.join("config.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<MmsConfig>(&t).ok())
            .map_or(MMS_SAMPLE_RATE, |c| c.data.sampling_rate);
        let lang = mms_language(code);
        (
            TtsFamily::Vits,
            vec![lang.clone()],
            Some(rate),
            vec![Voice {
                id: code.to_string(),
                sid: 0,
                language: lang,
                gender: String::new(),
            }],
        )
    };

    Some(TtsModel {
        id: model_file.to_string_lossy().to_string(),
        name,
        family,
        dir: dir.to_path_buf(),
        model_file,
        tokens,
        data_dir,
        voices_file,
        vocoder,
        lexicons,
        rule_fsts,
        size_bytes: dir_size(dir),
        quantization,
        languages,
        sample_rate,
        voices,
    })
}

/// The model folders under `roots`, up to a few levels deep (the models Harness downloads,
/// `<root>/<subdir>/<name>`, and ones the user unpacked anywhere in a root): every folder
/// `detect` accepts, once, in no particular order. Shared by the speech-recognition scan.
pub(crate) fn scan_folders<T>(
    roots: &[PathBuf],
    detect: impl Fn(&Path) -> Option<T>,
    same: impl Fn(&T, &T) -> bool,
) -> Vec<T> {
    const MAX_DEPTH: usize = 4;
    let mut out: Vec<T> = Vec::new();
    for root in roots {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 0 {
                if let Some(model) = detect(&dir) {
                    if !out.iter().any(|m| same(m, &model)) {
                        out.push(model);
                    }
                    // A model's own sub-folders (espeak-ng-data) hold no other model.
                    continue;
                }
            }
            if depth >= MAX_DEPTH {
                continue;
            }
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let hidden = entry.file_name().to_string_lossy().starts_with('.');
                if entry.file_type().is_ok_and(|t| t.is_dir()) && !hidden {
                    stack.push((path, depth + 1));
                }
            }
        }
    }
    out
}

/// The text-to-speech models under `roots`: folders up to a few levels deep, the models
/// Harness downloads (`<root>/tts/<name>`) and ones the user unpacked anywhere in a root.
pub fn scan(roots: &[PathBuf]) -> Vec<TtsModel> {
    let mut out = scan_folders(roots, detect, |a, b| a.id == b.id);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// [`scan`] of the configured model directories.
pub fn installed_models() -> Vec<TtsModel> {
    scan(&crate::config::AppSettings::load().all_models_dirs())
}

/// The library's view of a model folder.
pub fn to_local_model(model: &TtsModel, root: &Path) -> crate::models::LocalModel {
    let top_files: Vec<PathBuf> = files_of(&model.dir);
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
        shard_paths: top_files,
        root_dir: root.to_string_lossy().to_string(),
        gguf: None,
        source: None,
        mmproj_path: None,
        kind: crate::domain::ModelKind::Tts,
        origin: crate::domain::ModelOrigin::Library,
        task: None,
        vision: false,
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

/// Removes a model folder (everything in it, including `espeak-ng-data`); refuses anything
/// that is not a model folder inside one of `roots`.
pub fn delete_model_dir(model_file: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let _mutation = crate::models::LibraryMutation::begin();
    let dir = model_file.parent().ok_or_else(|| {
        String::from(err("tts.model_not_found").with("model", model_file.display()))
    })?;
    if detect(dir).is_none() {
        return Err(err("tts.model_not_found")
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

impl TtsModel {
    /// A voice of this model by the name a request gives: its own names (`af_heart`, case
    /// does not matter), OpenAI's (`alloy` stands in for `af_alloy` on Kokoro), a speaker
    /// number, or nothing (the model's default). A single-speaker model answers every name
    /// with its only voice: OpenAI clients always send one.
    pub fn resolve_voice(&self, wanted: Option<&str>) -> Result<&Voice, String> {
        let wanted = wanted.map(str::trim).filter(|v| !v.is_empty());
        let default = || {
            self.voices
                .iter()
                .find(|v| v.id == KOKORO_DEFAULT_VOICE)
                .or_else(|| self.voices.first())
        };
        let Some(wanted) = wanted else {
            return default().ok_or_else(|| no_voices(self));
        };
        let lower = wanted.to_ascii_lowercase();
        if let Some(v) = self
            .voices
            .iter()
            .find(|v| v.id.to_ascii_lowercase() == lower)
        {
            return Ok(v);
        }
        if self.family == TtsFamily::Kokoro {
            if let Some((_, kokoro)) = OPENAI_VOICES.iter().find(|(openai, _)| *openai == lower) {
                if let Some(v) = self.voices.iter().find(|v| v.id == *kokoro) {
                    return Ok(v);
                }
            }
        }
        if let Ok(sid) = lower.parse::<u32>() {
            if let Some(v) = self.voices.iter().find(|v| v.sid == sid) {
                return Ok(v);
            }
            // A Kokoro folder whose voice names are not known takes numbers up to the table of
            // its version; unknown tables are not guessed.
        }
        if self.voices.len() == 1 {
            return Ok(&self.voices[0]);
        }
        if self.voices.is_empty() {
            // Names unknown (a Kokoro version this build has no table for): only numbers.
            return Err(err("tts.voice_not_found")
                .with("voice", wanted)
                .with("model", &self.name)
                .with("available", "0, 1, 2...")
                .into());
        }
        Err(err("tts.voice_not_found")
            .with("voice", wanted)
            .with("model", &self.name)
            .with(
                "available",
                self.voices
                    .iter()
                    .map(|v| v.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .into())
    }

    /// The speaker id of a request, for models whose voice names are not known too: a number
    /// is used as given.
    pub fn speaker_id(&self, wanted: Option<&str>) -> Result<(u32, Option<&Voice>), String> {
        match self.resolve_voice(wanted) {
            Ok(v) => Ok((v.sid, Some(v))),
            Err(e) => {
                if self.voices.is_empty() {
                    if let Some(sid) = wanted.and_then(|w| w.trim().parse::<u32>().ok()) {
                        return Ok((sid, None));
                    }
                    if wanted.map(str::trim).filter(|w| !w.is_empty()).is_none() {
                        return Ok((0, None));
                    }
                }
                Err(e)
            }
        }
    }
}

fn no_voices(model: &TtsModel) -> String {
    err("tts.voice_not_found")
        .with("voice", "")
        .with("model", &model.name)
        .with("available", "")
        .into()
}

/// The model a request's `model` field names. Nothing (or an OpenAI alias such as `tts-1`)
/// is the default: the first Kokoro model, else the first model. Otherwise the folder name,
/// case does not matter, then a part of it when that is unambiguous (`kokoro`, `fettah`), or
/// the model file's path.
pub fn find_model<'a>(
    models: &'a [TtsModel],
    wanted: Option<&str>,
) -> Result<&'a TtsModel, String> {
    let wanted = wanted.map(str::trim).filter(|w| !w.is_empty());
    let default = || {
        models
            .iter()
            .find(|m| m.family == TtsFamily::Kokoro)
            .or_else(|| models.first())
            .ok_or_else(|| String::from(err("tts.no_model")))
    };
    let Some(wanted) = wanted else {
        return default();
    };
    let lower = wanted.to_ascii_lowercase();
    if DEFAULT_MODEL_ALIASES.contains(&lower.as_str()) {
        return default();
    }
    if let Some(m) = models
        .iter()
        .find(|m| m.name.to_ascii_lowercase() == lower || m.id == wanted)
    {
        return Ok(m);
    }
    let partial: Vec<&TtsModel> = models
        .iter()
        .filter(|m| m.name.to_ascii_lowercase().contains(&lower))
        .collect();
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(err("tts.model_not_found").with("model", wanted).into()),
        many => Err(err("tts.model_ambiguous")
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
    pub sid: u32,
    pub speed: f32,
    pub threads: u32,
    pub output: PathBuf,
}

fn path_arg(flag: &str, path: &Path) -> String {
    format!("{flag}={}", path.display())
}

/// The command line of one run, without the text (the last argument, see [`run`]).
/// `voice` is the resolved voice (Kokoro picks its language, lexicon and rules by it).
pub fn build_args(model: &TtsModel, voice: Option<&Voice>, params: &Params) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    match model.family {
        TtsFamily::Kokoro => {
            args.push(path_arg("--kokoro-model", &model.model_file));
            if let Some(v) = &model.voices_file {
                args.push(path_arg("--kokoro-voices", v));
            }
            args.push(path_arg("--kokoro-tokens", &model.tokens));
            if let Some(d) = &model.data_dir {
                args.push(path_arg("--kokoro-data-dir", d));
            }
            let id = voice.map(|v| v.id.as_str()).unwrap_or_default();
            let british = id.starts_with('b');
            let chinese = id.starts_with('z');
            // English lexicon of the voice's accent first, then the Chinese one: v1.0 speaks
            // both without a language switch.
            let mut lexicons: Vec<&PathBuf> = Vec::new();
            let accent = if british { "gb-en" } else { "us-en" };
            if let Some(l) = model
                .lexicons
                .iter()
                .find(|l| file_name(l).contains(accent))
            {
                lexicons.push(l);
            }
            if let Some(l) = model.lexicons.iter().find(|l| file_name(l).contains("zh")) {
                lexicons.push(l);
            }
            if !lexicons.is_empty() {
                args.push(format!(
                    "--kokoro-lexicon={}",
                    lexicons
                        .iter()
                        .map(|l| l.display().to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            if let Some(lang) = kokoro_lang_arg(id) {
                args.push(format!("--kokoro-lang={lang}"));
            }
            if chinese && !model.rule_fsts.is_empty() {
                args.push(format!(
                    "--tts-rule-fsts={}",
                    model
                        .rule_fsts
                        .iter()
                        .map(|f| f.display().to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
        }
        TtsFamily::Kitten => {
            args.push(path_arg("--kitten-model", &model.model_file));
            if let Some(v) = &model.voices_file {
                args.push(path_arg("--kitten-voices", v));
            }
            args.push(path_arg("--kitten-tokens", &model.tokens));
            if let Some(d) = &model.data_dir {
                args.push(path_arg("--kitten-data-dir", d));
            }
        }
        TtsFamily::Matcha => {
            args.push(path_arg("--matcha-acoustic-model", &model.model_file));
            if let Some(v) = &model.vocoder {
                args.push(path_arg("--matcha-vocoder", v));
            }
            args.push(path_arg("--matcha-tokens", &model.tokens));
            if let Some(d) = &model.data_dir {
                args.push(path_arg("--matcha-data-dir", d));
            } else if !model.lexicons.is_empty() {
                args.push(format!(
                    "--matcha-lexicon={}",
                    model
                        .lexicons
                        .iter()
                        .map(|l| l.display().to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
        }
        TtsFamily::Vits => {
            args.push(path_arg("--vits-model", &model.model_file));
            args.push(path_arg("--vits-tokens", &model.tokens));
            if let Some(d) = &model.data_dir {
                args.push(path_arg("--vits-data-dir", d));
            } else if let Some(l) = model.lexicons.first() {
                args.push(path_arg("--vits-lexicon", l));
            }
        }
    }
    args.push(format!("--sid={}", params.sid));
    args.push(format!("--speed={}", params.speed));
    args.push(format!("--num-threads={}", params.threads.max(1)));
    args.push(path_arg("--output-filename", &params.output));
    // The tool echoes its arguments to stderr by default, which would put the text of the
    // request into logs and error messages.
    args.push("--print-args=false".to_string());
    args
}

/// Validates `input` and `speed` of a request. Returns the text without surrounding blanks.
pub fn check_request(input: &str, speed: Option<f32>) -> Result<(String, f32), String> {
    let text = input.trim();
    if text.is_empty() {
        return Err(err("tts.input_empty").into());
    }
    let chars = text.chars().count();
    if chars > MAX_INPUT_CHARS {
        return Err(err("tts.input_too_long")
            .num("max", MAX_INPUT_CHARS as u64)
            .num("length", chars as u64)
            .into());
    }
    let speed = speed.unwrap_or(1.0);
    if !speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
        return Err(err("tts.speed_out_of_range")
            .with("min", MIN_SPEED)
            .with("max", MAX_SPEED)
            .into());
    }
    Ok((text.to_string(), speed))
}

/// Audio formats of OpenAI's `response_format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AudioFormat {
    Wav,
    /// Raw 16-bit little-endian mono samples at the model's sample rate.
    Pcm,
    Mp3,
    Opus,
    Aac,
    Flac,
}

impl AudioFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wav" => Some(Self::Wav),
            "pcm" => Some(Self::Pcm),
            "mp3" => Some(Self::Mp3),
            "opus" => Some(Self::Opus),
            "aac" => Some(Self::Aac),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Pcm => "pcm",
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Aac => "aac",
            Self::Flac => "flac",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Pcm => "audio/pcm",
            Self::Mp3 => "audio/mpeg",
            Self::Opus => "audio/ogg",
            Self::Aac => "audio/aac",
            Self::Flac => "audio/flac",
        }
    }

    /// File extension for a saved result.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            other => other.as_str(),
        }
    }

    /// The formats only ffmpeg can make.
    pub fn needs_ffmpeg(self) -> bool {
        !matches!(self, Self::Wav | Self::Pcm)
    }

    pub const ALL: [AudioFormat; 6] = [
        Self::Wav,
        Self::Pcm,
        Self::Mp3,
        Self::Opus,
        Self::Aac,
        Self::Flac,
    ];
}

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod tests;
