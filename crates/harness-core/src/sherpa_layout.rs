//! Hub repos in sherpa-onnx's layouts: which of them the shipped sherpa-onnx runtime
//! ([`crate::engine::SHERPA_ONNX`]) runs, and which files of a repo make one model.
//!
//! The ONNX speech repos on the Hub (mostly `csukuangfj/...`, the mirrors of the sherpa-onnx
//! release archives) have no `pipeline_tag`, no library name and no `config.json`; the file
//! names are the only evidence. The decision lives next to the local folder scans
//! ([`crate::tts::classify`], [`crate::asr::classify`]) and is the same function, so that a repo
//! the Hub shows as runnable is one the library recognises after the download.
//!
//! * Text to speech: VITS / Piper (`<voice>.onnx` + `espeak-ng-data`), Kokoro (`voices.bin` and
//!   lexicons), KittenTTS (`voices.bin`, `kitten` in the name), Matcha (acoustic model plus a
//!   vocoder in the same repo).
//! * Speech recognition: transducer (Zipformer, Conformer, NeMo Parakeet TDT), streaming
//!   transducer, Paraformer, SenseVoice, Whisper (`<size>-encoder.onnx`), Moonshine v1, NeMo
//!   CTC, Omnilingual, Dolphin, Cohere Transcribe.
//!
//! Not covered (the verdict stays unsupported): Optimum / transformers.js ONNX exports (a
//! `config.json` with `onnx/encoder_model.onnx`), Moonshine v2 `.ort` files, Matcha without its
//! vocoder, Pocket / Supertonic / ZipVoice TTS, Canary.

use crate::asr::AsrFamily;
use crate::tts::TtsFamily;

/// What a repo is for the sherpa-onnx runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SherpaKind {
    Tts(TtsFamily),
    Asr(AsrFamily),
}

impl SherpaKind {
    /// The Hub task (`pipeline_tag`) of the model, for a repo that has none.
    pub fn task(self) -> &'static str {
        match self {
            Self::Tts(_) => "text-to-speech",
            Self::Asr(_) => "automatic-speech-recognition",
        }
    }
}

/// Last path segment of a repo id (`csukuangfj/kokoro-int8-multi-lang-v1_1` -> the name).
fn repo_name(repo_id: &str) -> &str {
    repo_id.rsplit('/').next().unwrap_or(repo_id)
}

/// The files at the repo root.
fn root_files(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| !f.contains('/') && !f.contains('\\'))
        .cloned()
        .collect()
}

fn has_espeak(files: &[String]) -> bool {
    files.iter().any(|f| f.starts_with("espeak-ng-data/"))
}

/// What the sherpa-onnx runtime makes of the repo `repo_id` with these `files` (repo paths), or
/// `None`. Text to speech is decided first: its folders also have a `tokens.txt` and an `.onnx`.
pub fn classify_repo(repo_id: &str, files: &[String]) -> Option<SherpaKind> {
    let name = repo_name(repo_id);
    let root = root_files(files);
    if let Some(family) = crate::tts::classify(name, &root, has_espeak(files)) {
        return Some(SherpaKind::Tts(family));
    }
    crate::asr::classify(name, &root).map(SherpaKind::Asr)
}

/// Root files that a model needs next to its graphs (the tokens, Kokoro / Kitten voices,
/// lexicons, text-normalisation rules, a Piper voice's JSON, a licence). READMEs, export
/// scripts, sample audio and the other docs stay on the Hub.
fn is_companion(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "tokens.txt"
        || n.ends_with("-tokens.txt")
        || n == "voices.bin"
        || (n.starts_with("lexicon") && n.ends_with(".txt"))
        || n.ends_with(".fst")
        || n.ends_with(".onnx.json")
        || n == "config.json"
        || n.starts_with("license")
}

/// The precision a graph name says.
fn is_int8(name: &str) -> bool {
    name.to_ascii_lowercase().contains("int8")
}

/// The graph `name` without its precision marker, to pair an int8 graph with its fp32 twin.
fn role(name: &str) -> String {
    name.to_ascii_lowercase()
        .replace(".int8", "")
        .replace("-int8", "")
}

/// One downloadable model of a repo: its precision label and the repo paths of its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SherpaVariant {
    /// `INT8`, `FP16` or `FP32`.
    pub quantization: &'static str,
    pub files: Vec<String>,
}

/// The models of a sherpa-onnx repo: one per precision when the repo ships both (an int8 and
/// an fp32 copy of every graph, as the Zipformer and Whisper exports do), else one. Each holds
/// its graphs, the companions of [`is_companion`] and, for text to speech, the whole
/// `espeak-ng-data` folder. A set that is not a complete model on its own (an fp32 copy of
/// only the one graph the int8 export left alone) is dropped. Empty for a repo that is not in
/// a sherpa-onnx layout.
pub fn plan_variants(repo_id: &str, files: &[String]) -> Vec<SherpaVariant> {
    let name = repo_name(repo_id);
    let Some(kind) = classify_repo(repo_id, files) else {
        return Vec::new();
    };
    let root = root_files(files);
    let graphs: Vec<&String> = root
        .iter()
        .filter(|f| f.to_ascii_lowercase().ends_with(".onnx"))
        .collect();
    let companions: Vec<String> = root.iter().filter(|f| is_companion(f)).cloned().collect();
    let espeak: Vec<String> = match kind {
        SherpaKind::Tts(_) => files
            .iter()
            .filter(|f| f.starts_with("espeak-ng-data/"))
            .cloned()
            .collect(),
        SherpaKind::Asr(_) => Vec::new(),
    };
    let int8: Vec<&String> = graphs.iter().copied().filter(|g| is_int8(g)).collect();
    let rest: Vec<&String> = graphs.iter().copied().filter(|g| !is_int8(g)).collect();
    let int8_roles: Vec<String> = int8.iter().map(|g| role(g)).collect();
    let rest_roles: Vec<String> = rest.iter().map(|g| role(g)).collect();
    // The int8 set, plus the graphs the export has no int8 copy of (Moonshine's preprocessor).
    let int8_set: Vec<&String> = int8
        .iter()
        .copied()
        .chain(
            rest.iter()
                .copied()
                .filter(|g| !int8_roles.contains(&role(g))),
        )
        .collect();
    // The fp32 set; an int8-only graph (no twin) cannot be left out.
    let fp32_set: Vec<&String> = rest
        .iter()
        .copied()
        .chain(
            int8.iter()
                .copied()
                .filter(|g| !rest_roles.contains(&role(g))),
        )
        .collect();
    let fp32_is_full = !int8.is_empty() && int8.iter().all(|g| rest_roles.contains(&role(g)));
    let candidates: Vec<(&'static str, Vec<&String>)> = if int8.is_empty() {
        let label = if graphs
            .iter()
            .any(|g| g.to_ascii_lowercase().contains("fp16"))
        {
            "FP16"
        } else {
            "FP32"
        };
        vec![(label, graphs.clone())]
    } else if rest.is_empty() || !fp32_is_full {
        vec![("INT8", graphs.clone())]
    } else {
        vec![("INT8", int8_set), ("FP32", fp32_set)]
    };
    let mut out: Vec<SherpaVariant> = Vec::new();
    for (quantization, set) in candidates {
        let mut variant: Vec<String> = set.into_iter().cloned().collect();
        variant.extend(companions.iter().cloned());
        let names: Vec<String> = variant.clone();
        // Complete on its own: the same layout comes out of these files alone.
        let ok = crate::tts::classify(name, &names, !espeak.is_empty())
            .map(SherpaKind::Tts)
            .or_else(|| crate::asr::classify(name, &names).map(SherpaKind::Asr))
            == Some(kind);
        if !ok {
            continue;
        }
        variant.extend(espeak.iter().cloned());
        out.push(SherpaVariant {
            quantization,
            files: variant,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(files: &[&str]) -> Vec<String> {
        files.iter().map(|f| (*f).to_string()).collect()
    }

    fn espeak() -> Vec<String> {
        vec![
            "espeak-ng-data/en_dict".into(),
            "espeak-ng-data/voices/x".into(),
        ]
    }

    fn with_espeak(files: &[&str]) -> Vec<String> {
        let mut v = s(files);
        v.extend(espeak());
        v
    }

    #[test]
    fn piper_vits_is_text_to_speech() {
        let files = with_espeak(&[
            ".gitattributes",
            "MODEL_CARD",
            "en_US-amy-low.onnx",
            "en_US-amy-low.onnx.json",
            "tokens.txt",
            "vits-piper-en_US.py",
        ]);
        assert_eq!(
            classify_repo("csukuangfj/vits-piper-en_US-amy-low", &files),
            Some(SherpaKind::Tts(TtsFamily::Vits))
        );
        // No phoneme data, no lexicon: not a layout the tool can run.
        assert_eq!(
            classify_repo("x/voice", &s(&["a.onnx", "a.onnx.json", "tokens.txt"])),
            None
        );
    }

    #[test]
    fn kokoro_kitten_and_matcha() {
        let kokoro = with_espeak(&[
            "model.int8.onnx",
            "tokens.txt",
            "voices.bin",
            "lexicon-us-en.txt",
            "lexicon-zh.txt",
            "date-zh.fst",
        ]);
        assert_eq!(
            classify_repo("csukuangfj/kokoro-int8-multi-lang-v1_1", &kokoro),
            Some(SherpaKind::Tts(TtsFamily::Kokoro))
        );
        // A community name without "kokoro" still has the lexicons.
        assert_eq!(
            classify_repo("someone/multi-voice-int8", &kokoro),
            Some(SherpaKind::Tts(TtsFamily::Kokoro))
        );
        let kitten = with_espeak(&["model.fp16.onnx", "tokens.txt", "voices.bin"]);
        assert_eq!(
            classify_repo("csukuangfj/kitten-nano-en-v0_1-fp16", &kitten),
            Some(SherpaKind::Tts(TtsFamily::Kitten))
        );
        // voices.bin of an unknown family is not guessed.
        assert_eq!(classify_repo("someone/voices", &kitten), None);
        let matcha_alone = with_espeak(&["model-steps-3.onnx", "tokens.txt"]);
        assert_eq!(
            classify_repo("csukuangfj/matcha-icefall-en_US-ljspeech", &matcha_alone),
            None,
            "without its vocoder Matcha cannot speak"
        );
        let matcha = with_espeak(&["model-steps-3.onnx", "hifigan_v2.onnx", "tokens.txt"]);
        assert_eq!(
            classify_repo("csukuangfj/matcha-icefall-en_US-ljspeech", &matcha),
            Some(SherpaKind::Tts(TtsFamily::Matcha))
        );
        // Another family's folder is not taken for VITS.
        let pocket = with_espeak(&["lm_main.int8.onnx", "tokens.txt"]);
        assert_eq!(classify_repo("x/sherpa-onnx-pocket-tts", &pocket), None);
    }

    #[test]
    fn speech_recognition_layouts() {
        let whisper = s(&[
            "tiny.en-decoder.int8.onnx",
            "tiny.en-decoder.onnx",
            "tiny.en-encoder.int8.onnx",
            "tiny.en-encoder.onnx",
            "tiny.en-tokens.txt",
            "test_wavs/0.wav",
        ]);
        assert_eq!(
            classify_repo("csukuangfj/sherpa-onnx-whisper-tiny.en", &whisper),
            Some(SherpaKind::Asr(AsrFamily::Whisper))
        );
        let moonshine = s(&[
            "cached_decode.int8.onnx",
            "encode.int8.onnx",
            "preprocess.onnx",
            "uncached_decode.int8.onnx",
            "tokens.txt",
        ]);
        assert_eq!(
            classify_repo("csukuangfj/sherpa-onnx-moonshine-tiny-en-int8", &moonshine),
            Some(SherpaKind::Asr(AsrFamily::Moonshine))
        );
        let zipformer = s(&[
            "encoder-epoch-99-avg-1.int8.onnx",
            "decoder-epoch-99-avg-1.onnx",
            "joiner-epoch-99-avg-1.int8.onnx",
            "tokens.txt",
        ]);
        assert_eq!(
            classify_repo(
                "csukuangfj/sherpa-onnx-zipformer-small-en-2023-06-26",
                &zipformer
            ),
            Some(SherpaKind::Asr(AsrFamily::Transducer))
        );
        assert_eq!(
            classify_repo(
                "csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26",
                &zipformer
            ),
            Some(SherpaKind::Asr(AsrFamily::StreamingTransducer))
        );
        assert_eq!(
            classify_repo("x/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8", &zipformer),
            Some(SherpaKind::Asr(AsrFamily::NemoTransducer))
        );
        let single = s(&["model.int8.onnx", "model.onnx", "tokens.txt", "am.mvn"]);
        assert_eq!(
            classify_repo("csukuangfj/sherpa-onnx-paraformer-en-2024-03-09", &single),
            Some(SherpaKind::Asr(AsrFamily::Paraformer))
        );
        assert_eq!(
            classify_repo(
                "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue",
                &single
            ),
            Some(SherpaKind::Asr(AsrFamily::SenseVoice))
        );
        // A single graph whose family the name does not say is not guessed.
        assert_eq!(classify_repo("x/some-asr", &single), None);
    }

    #[test]
    fn exports_for_other_runtimes_are_not_sherpa_layouts() {
        // An Optimum / transformers.js Whisper export: `onnx/` folder and a transformers config.
        let optimum = s(&[
            "config.json",
            "tokenizer.json",
            "onnx/encoder_model.onnx",
            "onnx/decoder_model_merged.onnx",
        ]);
        assert_eq!(classify_repo("Xenova/whisper-tiny.en", &optimum), None);
        // Moonshine v2 ships `.ort` files.
        let v2 = s(&[
            "decoder_model_merged.ort",
            "encoder_model.ort",
            "tokens.txt",
        ]);
        assert_eq!(
            classify_repo("x/sherpa-onnx-moonshine-tiny-en-quantized", &v2),
            None
        );
        // An embedding model.
        assert_eq!(
            classify_repo(
                "BAAI/bge-small",
                &s(&["model.onnx", "tokenizer.json", "config.json"])
            ),
            None
        );
        // Wav2vec2 CTC packages keep their own path.
        assert_eq!(
            classify_repo(
                "x/wav2vec2",
                &s(&[
                    "model.onnx",
                    "vocab.json",
                    "config.json",
                    "preprocessor_config.json"
                ])
            ),
            None
        );
    }

    #[test]
    fn variants_split_by_precision_and_keep_the_model_complete() {
        let whisper = s(&[
            ".gitattributes",
            "test_wavs/0.wav",
            "tiny.en-decoder.int8.onnx",
            "tiny.en-decoder.onnx",
            "tiny.en-encoder.int8.onnx",
            "tiny.en-encoder.onnx",
            "tiny.en-tokens.txt",
        ]);
        let v = plan_variants("csukuangfj/sherpa-onnx-whisper-tiny.en", &whisper);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].quantization, "INT8");
        assert_eq!(
            v[0].files,
            s(&[
                "tiny.en-decoder.int8.onnx",
                "tiny.en-encoder.int8.onnx",
                "tiny.en-tokens.txt"
            ])
        );
        assert_eq!(v[1].quantization, "FP32");
        assert_eq!(
            v[1].files,
            s(&[
                "tiny.en-decoder.onnx",
                "tiny.en-encoder.onnx",
                "tiny.en-tokens.txt"
            ])
        );
    }

    #[test]
    fn a_graph_without_an_int8_twin_goes_with_the_int8_set_only() {
        let moonshine = s(&[
            "LICENSE",
            "README.md",
            "cached_decode.int8.onnx",
            "encode.int8.onnx",
            "preprocess.onnx",
            "test_wavs/0.wav",
            "tokens.txt",
            "uncached_decode.int8.onnx",
        ]);
        let v = plan_variants("csukuangfj/sherpa-onnx-moonshine-tiny-en-int8", &moonshine);
        assert_eq!(
            v.len(),
            1,
            "an fp32 copy of the preprocessor alone is no model"
        );
        assert_eq!(v[0].quantization, "INT8");
        assert!(v[0].files.contains(&"preprocess.onnx".to_string()));
        assert!(v[0].files.contains(&"LICENSE".to_string()));
        assert!(!v[0].files.iter().any(|f| f.starts_with("test_wavs")));
        assert!(!v[0].files.contains(&"README.md".to_string()));
    }

    #[test]
    fn tts_variant_brings_the_phoneme_data_and_voice_config() {
        let files = with_espeak(&[
            "MODEL_CARD",
            "en_US-amy-low.onnx",
            "en_US-amy-low.onnx.json",
            "tokens.txt",
            "vits-piper-en_US.sh",
        ]);
        let v = plan_variants("csukuangfj/vits-piper-en_US-amy-low", &files);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].quantization, "FP32");
        let f = &v[0].files;
        assert!(f.contains(&"en_US-amy-low.onnx".to_string()));
        assert!(f.contains(&"en_US-amy-low.onnx.json".to_string()));
        assert!(f.contains(&"tokens.txt".to_string()));
        assert!(f.contains(&"espeak-ng-data/en_dict".to_string()));
        assert!(!f.contains(&"vits-piper-en_US.sh".to_string()));
        assert!(plan_variants("x/not-speech", &s(&["model.onnx"])).is_empty());
    }

    #[test]
    fn tasks() {
        assert_eq!(SherpaKind::Tts(TtsFamily::Vits).task(), "text-to-speech");
        assert_eq!(
            SherpaKind::Asr(AsrFamily::Whisper).task(),
            "automatic-speech-recognition"
        );
    }
}
