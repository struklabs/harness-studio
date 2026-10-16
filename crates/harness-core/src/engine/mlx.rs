//! The MLX engines as [`Engine`]s (Apple Silicon, macOS 14 or later), two flavours of one
//! [`MlxEngine`]:
//!
//! * `mlx`: a text-generation model folder in MLX's format (`mlx-community/...`), served by
//!   `python/mlx_worker.py` around mlx-lm's OpenAI-compatible server from the managed `mlx`
//!   environment;
//! * `mlx-vlm`: a vision-language folder (its config has a `vision_config`), served by
//!   `python/mlx_vlm_worker.py` around mlx-vlm's server from the managed `mlx-vlm` environment;
//! * `mflux`: a FLUX.2 [klein] folder in mflux's saved format (`mflux-community/...`), served by
//!   `python/mflux_worker.py` (text to image and image editing) from the managed `mflux`
//!   environment;
//! * `mlx-audio`: a text-to-speech folder of mlx-audio (Qwen3-TTS, Chatterbox Turbo, Soprano,
//!   VoxCPM2, Spark-TTS) or a speech recognition one
//!   (Whisper, Parakeet), served by `python/mlx_audio_worker.py` (Harness's own small server
//!   around mlx-audio's model, which the `mlx-vlm` environment carries; `--task transcription`
//!   for recognition).
//!
//! A launch runs
//!
//! ```text
//! <env python> <env>/<worker>.py --model <dir> --port <port> [--alias <alias>] <catalog>
//! ```
//!
//! The workers listen on 127.0.0.1 only, ask every request but `/health` for the per-run API
//! key (`HARNESS_WORKER_API_KEY`, from the environment), serve only the folder they were started
//! with and never run remote code; the vision worker takes images only inline (`data:` URLs).
//! macOS has no network isolation for them (the Linux Python engine's `unshare` has no
//! counterpart there).
//!
//! An MLX folder is a transformers generator (`config.json` + safetensors) that MLX wrote: its
//! config has MLX's `quantization` (`bits`, `group_size`), or its card says
//! `library_name: mlx`. mistral.rs can't read those weights, so it leaves them to these engines.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::{Engine, MetricsSource, ModelFormat, ReleaseSource, RuntimeKind, ServerEngine};
use crate::capabilities::RuntimeCapabilities;
use crate::error_key::err;
use crate::gguf::GgufMetadata;
use crate::params::{ParamCatalog, RenderContext};
use crate::process::{BuildOutput, LaunchConfig, ServerCaps};
use crate::runtime::ReleaseAsset;
use crate::telemetry::Metrics;
use crate::vram::{
    EstimateSource, FitCategory, FitInput, FitResult, Measurement, MemoryBudget, MemoryEstimate,
};

/// Family id of the MLX engine.
pub const MLX: &str = "mlx";

/// Family id of the MLX image engine (mflux).
pub const MFLUX: &str = "mflux";

/// The managed environment the image worker runs in (`crate::pyenv::envs::MFLUX`).
pub const MFLUX_ENV: &str = "mflux";

/// Family id of the MLX vision engine (mlx-vlm).
pub const MLX_VLM: &str = "mlx-vlm";

/// Family id of the MLX audio engine (mlx-audio: text to speech).
pub const MLX_AUDIO: &str = "mlx-audio";

/// The managed environment the text worker runs in (`crate::pyenv::envs::MLX` on Apple Silicon).
pub const ENV: &str = "mlx";

/// The managed environment the vision and audio workers run in (`crate::pyenv::envs::MLX_VLM`:
/// mlx-vlm brings mlx-audio along).
pub const VLM_ENV: &str = "mlx-vlm";

/// The worker scripts, written next to their environments ([`worker_script`]).
const WORKER: &str = include_str!("../../python/mlx_worker.py");
const VLM_WORKER: &str = include_str!("../../python/mlx_vlm_worker.py");
const AUDIO_WORKER: &str = include_str!("../../python/mlx_audio_worker.py");
const IMAGE_WORKER: &str = include_str!("../../python/mflux_worker.py");

/// The FLUX.2 [klein] models the image engine runs: the base model a conversion's card names
/// (lower case) and mflux's name for it (`ModelConfig.from_name`). The worker has the same table
/// (`MODELS` in `python/mflux_worker.py`).
pub const MFLUX_MODELS: &[(&str, &str)] = &[
    ("black-forest-labs/flux.2-klein-4b", "flux2-klein-4b"),
    (
        "black-forest-labs/flux.2-klein-base-4b",
        "flux2-klein-base-4b",
    ),
    ("black-forest-labs/flux.2-klein-9b", "flux2-klein-9b"),
    ("black-forest-labs/flux.2-klein-9b-kv", "flux2-klein-9b-kv"),
    (
        "black-forest-labs/flux.2-klein-base-9b",
        "flux2-klein-base-9b",
    ),
];

/// The weight folders of an mflux folder, each safetensors shards with their index; the
/// tokenizer is in `tokenizer/`.
pub const MFLUX_PARTS: &[&str] = &["transformer", "text_encoder", "vae"];

/// mlx-audio text-to-speech architectures Harness runs (see [`tts_type`]): the ones checked end
/// to end. OuteTTS, Orpheus, Dia and CSM (`sesame`) load a codec (and CSM its text tokenizer and
/// voice prompts) from the Hub by name: they are companion downloads ([`codecs_of`]) that the
/// worker finds offline. Kokoro (misaki, spaCy data and voices fetched at run time) is not
/// offered.
const TTS_TYPES: &[&str] = &[
    "qwen3_tts",
    "chatterbox_turbo",
    "soprano",
    "voxcpm2",
    "spark",
    "outetts",
    "orpheus",
    "dia",
    "sesame",
];

/// A codec repo an mlx-audio family's loader asks the Hub for by name, with its files and
/// sizes (as the repo lists them). Downloaded once into `<library>/<owner>/<repo>` and shared
/// by every model of the families that use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Codec {
    pub repo: &'static str,
    /// Its folder under a library. The repo's own path, except for a repo that is a model too
    /// (`unsloth/Llama-3.2-1B`, `mlx-community/csm-1b`): deleting that model must not take the
    /// files another model loads.
    pub folder: &'static str,
    /// The commit the files are taken from: an upstream change cannot swap them.
    pub rev: &'static str,
    /// `(file, size, sha256)`; each download is checked against both.
    pub files: &'static [(&'static str, u64, &'static str)],
}

const DAC_24K: Codec = Codec {
    repo: "mlx-community/dac-speech-24khz-1.5kbps",
    folder: "mlx-community/dac-speech-24khz-1.5kbps",
    rev: "d8dd32d0124c3c3d06b905980346994c3fd56de1",
    files: &[
        (
            "config.json",
            391,
            "4b0d4c6929bc741cc0a5f7dde1f376294c4db78886b9173455e19a96fb2f6852",
        ),
        (
            "model.safetensors",
            295_669_897,
            "d8295d38f06edd961ae39d75945564828504bfdf5c995f14c1c536a2ac1abe0c",
        ),
    ],
};
const SNAC_24K: Codec = Codec {
    repo: "mlx-community/snac_24khz",
    folder: "mlx-community/snac_24khz",
    rev: "556af1cd3b1c5f2d294f6aa9bb886245d7b716ac",
    files: &[
        (
            "config.json",
            300,
            "e119b9366d4f5e73c6ca5f31137c4ff361578bbb132953a5203afe037c4012be",
        ),
        (
            "model.safetensors",
            79_404_951,
            "e61ae2f638f56ee07a37592cd5a6a9e7d642560ddc78a76ee4a7f96d6922f1be",
        ),
    ],
};
const DAC_44K: Codec = Codec {
    repo: "mlx-community/descript-audio-codec-44khz",
    folder: "mlx-community/descript-audio-codec-44khz",
    rev: "91dff27c81edbb0992be17ac51d2c7d515411a32",
    files: &[
        (
            "config.json",
            219,
            "c3ad73d9f58d6aa4889856597eea8147c36ea49efc2fcb8efcb9a812b30917ec",
        ),
        (
            "model.safetensors",
            306_644_869,
            "cf9b0711afdc362182afda1d6addfd1b6b6e95509dec85fe237ac9a0b8a7522b",
        ),
    ],
};
/// Kyutai's Mimi codec of CSM: the one tokenizer file of the Moshi repo (not its 15 GB model).
const MIMI: Codec = Codec {
    repo: "kyutai/moshiko-pytorch-bf16",
    folder: "kyutai/moshiko-pytorch-bf16",
    rev: "2bfc9ae6e89079a5cc7ed2a68436010d91a3d289",
    files: &[(
        "tokenizer-e351c8d8-checkpoint125.safetensors",
        384_644_900,
        "09b782f0629851a271227fb9d36db65c041790365f11bbe5d3d59369cf863f50",
    )],
};
/// CSM's text tokenizer: Llama 3.2's, from the ungated copy mlx-audio itself loads (Meta's repo
/// is gated); the tokenizer files only.
const LLAMA32_TOKENIZER: Codec = Codec {
    repo: "unsloth/Llama-3.2-1B",
    folder: "unsloth/Llama-3.2-1B-tokenizer",
    rev: "9535bd9b1d1dea6acafbdc4813b728796aeb28da",
    files: &[
        (
            "config.json",
            889,
            "8f028e2cd88148fad38c7dece460947681adf6178e46e11ece9742aa49b378dd",
        ),
        (
            "special_tokens_map.json",
            459,
            "32f404d626cf7b1b6eea36c241ae7cbd6ec29c777a8701ea48c0fe9ebb94c9b1",
        ),
        (
            "tokenizer.json",
            17_209_920,
            "6b9e4e7fb171f92fd137b777cc2714bf87d11576700a1dcd7a399e7bbe39537b",
        ),
        (
            "tokenizer_config.json",
            50_641,
            "8ca815f697b8f6953d9bad3130ddd6238b99d36c20fde052cf730207ac0926d4",
        ),
    ],
};
/// CSM's voice prompts (a voice is a recorded prompt the model continues): mlx-community's
/// ungated copy of the ones in `sesame/csm-1b` (gated); mlx-audio has their texts.
const CSM_PROMPTS: Codec = Codec {
    repo: "mlx-community/csm-1b",
    folder: "mlx-community/csm-1b-voice-prompts",
    rev: "5bf5ec118cf45fecc7b51198fd9f1a20a5aab65a",
    files: &[
        (
            "prompts/conversational_a.wav",
            2_646_044,
            "356648c1bc6c1da7883004557e9b21a2ef7d01682d8b9d02d6dcb950b348b04f",
        ),
        (
            "prompts/conversational_b.wav",
            2_646_044,
            "c247153011385d33aaeed193adfec562c32182e2facd30cc8cd0b3e820e94afb",
        ),
    ],
};

/// The Hugging Face processor (feature extractor and tokenizer) of a Whisper, for mlx-examples'
/// conversions that ship none (`whisper-large-v3-turbo`, `whisper-small-mlx`: weights and a short
/// config). One per tokenizer: large-v3 and turbo (128 mel bins, 51866 tokens), the other
/// multilingual sizes (51865) and the English-only ones (51864). Folders of their own: the repo
/// is a model too.
const WHISPER_LARGE_V3_PROCESSOR: Codec = Codec {
    repo: "openai/whisper-large-v3",
    folder: "openai/whisper-large-v3-processor",
    rev: "06f233fe06e710322aca913c1bc4249a0d71fce1",
    files: &[
        (
            "added_tokens.json",
            34_648,
            "3c51f66c4c21f9e126970078f11ae77a78c74aee8df606ee9daba86e467108e0",
        ),
        (
            "merges.txt",
            493_869,
            "2df2990a395e35e8dfbc7511e08c12d56018d8d04691e0133e5d63b21e154dc6",
        ),
        (
            "normalizer.json",
            52_666,
            "bf1c507dc8724ca9cf9903640dacfb69dae2f00edee4f21ceba106a7392f26dd",
        ),
        (
            "preprocessor_config.json",
            340,
            "7ccc62c6f2765af1f3b46c00c9b5894426835a05021c8b9c01eecb6dfb542711",
        ),
        (
            "special_tokens_map.json",
            2_072,
            "1c70773c078cb2ca96e0fcff113102f1d3e2b1504272c3bb63b035d4a6700d87",
        ),
        (
            "tokenizer.json",
            2_480_617,
            "6d8cbd7cd0d8d5815e478dac67b85a26bbe77c1f5e0c6d76d1ce2abc0e5f21ca",
        ),
        (
            "tokenizer_config.json",
            282_843,
            "844b642c73a91359722f47b35705f7174686df33d252695d8572cf9ac03a6389",
        ),
        (
            "vocab.json",
            1_036_558,
            "e2aa043ef015641d363d8288e7c241c85e36a5c761fb303598e0710233344387",
        ),
    ],
};
const WHISPER_SMALL_PROCESSOR: Codec = Codec {
    repo: "openai/whisper-small",
    folder: "openai/whisper-small-processor",
    rev: "973afd24965f72e36ca33b3055d56a652f456b4d",
    files: &[
        (
            "added_tokens.json",
            34_604,
            "9715fd2243b6f06a5858b5e32950d2853f73dd5bc201aafcf76f5082a2d8acd1",
        ),
        (
            "merges.txt",
            493_869,
            "2df2990a395e35e8dfbc7511e08c12d56018d8d04691e0133e5d63b21e154dc6",
        ),
        (
            "normalizer.json",
            52_666,
            "bf1c507dc8724ca9cf9903640dacfb69dae2f00edee4f21ceba106a7392f26dd",
        ),
        (
            "preprocessor_config.json",
            184_990,
            "9b5cd03a36fbb8a627c64d98a5b5b126ead95a77720723944487311f0110b666",
        ),
        (
            "special_tokens_map.json",
            2_194,
            "e67ae3a0aaa99abcd9f187138e12db1f65c16a14761c50ef10eef2c174a7a691",
        ),
        (
            "tokenizer.json",
            2_480_466,
            "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566",
        ),
        (
            "tokenizer_config.json",
            282_683,
            "2a4c4281cf9f51ac6ccc406fdc711a087afe6530f671fa7b80953edc498275ce",
        ),
        (
            "vocab.json",
            835_550,
            "8f680bba319e01a653d2e8a5dbc17a9157179e0576e6ce74ce0c06356c6e24f9",
        ),
    ],
};
const WHISPER_TINY_EN_PROCESSOR: Codec = Codec {
    repo: "openai/whisper-tiny.en",
    folder: "openai/whisper-tiny.en-processor",
    rev: "87c7102498dcde7456f24cfd30239ca606ed9063",
    files: &[
        (
            "added_tokens.json",
            34_604,
            "560be47bea388757f8d4cc185c5d82067426cbb6361e38016dd90ddc01ab203a",
        ),
        (
            "merges.txt",
            456_318,
            "1ce1664773c50f3e0cc8842619a93edc4624525b728b188a9e0be33b7726adc5",
        ),
        (
            "normalizer.json",
            52_666,
            "bf1c507dc8724ca9cf9903640dacfb69dae2f00edee4f21ceba106a7392f26dd",
        ),
        (
            "preprocessor_config.json",
            184_990,
            "9b5cd03a36fbb8a627c64d98a5b5b126ead95a77720723944487311f0110b666",
        ),
        (
            "special_tokens_map.json",
            1_830,
            "014f8f802366ed818919550be0ad9e35907327cb9e142e8aaa102420f460bda8",
        ),
        (
            "tokenizer.json",
            2_405_679,
            "5eb60cec1e77aeeb6869a2bb5a8e01a84c3fe5d072d75369343021fe6f5310d0",
        ),
        (
            "tokenizer_config.json",
            805,
            "14f84bdf4b9ecdbd4738ddc81c17a1baedfc02bb93c6e049c951e15a1b40b70d",
        ),
        (
            "vocab.json",
            798_156,
            "3ba3c3109ff33976c4bd966589c11ee14fcaa1f4c9e5e154c2ed7f99d80709e7",
        ),
    ],
};

/// The processor of an mlx-examples Whisper config by its vocabulary (`n_vocab`).
fn whisper_processor_for_vocab(n_vocab: u64) -> Option<Codec> {
    match n_vocab {
        51866 => Some(WHISPER_LARGE_V3_PROCESSOR),
        51865 => Some(WHISPER_SMALL_PROCESSOR),
        51864 => Some(WHISPER_TINY_EN_PROCESSOR),
        _ => None,
    }
}

/// The processor of a Hub Whisper repo by its name (the listing has no config), for OpenAI's
/// sizes only (`whisper-<size>[.en]`): large-v3 and turbo, English-only (`.en`), else the
/// multilingual one. `None` for any other name: a fine-tune (`kotoba-whisper-v2.0`) may have
/// another vocabulary, so it is not guessed.
fn whisper_processor_for_repo(id: &str) -> Option<Codec> {
    let name = id.rsplit('/').next().unwrap_or(id).to_ascii_lowercase();
    let re = regex::Regex::new(
        r"(^|[-_])whisper-(tiny|base|small|medium|large-v3-turbo|large-v3|large-v2|large|turbo)(\.en)?([-_]|$)",
    )
    .ok()?;
    let caps = re.captures(&name)?;
    let size = caps.get(2)?.as_str();
    Some(if size.starts_with("large-v3") || size == "turbo" {
        WHISPER_LARGE_V3_PROCESSOR
    } else if caps.get(3).is_some() {
        WHISPER_TINY_EN_PROCESSOR
    } else {
        WHISPER_SMALL_PROCESSOR
    })
}

/// The bundle id of a Whisper processor plan ([`codec_plan_for_repo`]).
pub const WHISPER_BUNDLE: &str = "mlx-audio:whisper";

/// Whether a config is mlx-examples' Whisper config (`n_mels`, `n_vocab`...; transformers names
/// them `num_mel_bins`, `vocab_size`). Such a folder is an MLX build and has no processor.
fn is_mlx_whisper_config(cfg: &serde_json::Value) -> bool {
    cfg.get("n_mels").is_some_and(|v| v.is_u64())
        && cfg
            .get("n_vocab")
            .and_then(|v| v.as_u64())
            .and_then(whisper_processor_for_vocab)
            .is_some()
}

/// The weights of an mlx-audio folder: its safetensors, else mlx-examples' `weights.npz` (older
/// Whisper conversions), the way mlx-audio's loader looks for them.
fn audio_weight_files(dir: &Path) -> Vec<PathBuf> {
    let files = super::mistralrs::weight_files(dir);
    if !files.is_empty() {
        return files;
    }
    let mut npz: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("npz")))
        .collect();
    npz.sort();
    npz
}

/// The voices of a CSM folder: its [`CSM_PROMPTS`]. The worker lists them too (`CSM_VOICES` in
/// `mlx_audio_worker.py`); `csm_voices_match_the_worker` keeps the two the same.
pub const CSM_VOICES: &[&str] = &["conversational_a", "conversational_b"];

/// The pinned download of a codec file ([`codecs_of`]) and the codec's library folder
/// ([`Codec::folder`]), `None` for other files.
pub fn pinned_codec_file(
    repo: &str,
    file: &str,
) -> Option<(crate::downloader::PinnedFile<'static>, &'static str)> {
    [
        DAC_24K,
        SNAC_24K,
        DAC_44K,
        MIMI,
        LLAMA32_TOKENIZER,
        CSM_PROMPTS,
        WHISPER_LARGE_V3_PROCESSOR,
        WHISPER_SMALL_PROCESSOR,
        WHISPER_TINY_EN_PROCESSOR,
    ]
    .iter()
    .filter(|c| c.repo == repo)
    .flat_map(|c| c.files.iter().map(move |f| (c, f)))
    .find(|(_, (name, _, _))| *name == file)
    .map(|(c, (name, size, sha256))| {
        let file = crate::downloader::PinnedFile {
            repo: c.repo,
            rev: c.rev,
            filename: name,
            size: *size,
            sha256,
        };
        (file, c.folder)
    })
}

/// The codecs a text-to-speech family loads (mlx-audio 0.5.9: OuteTTS `DacInterface`, Orpheus
/// `SNAC.from_pretrained`, Dia `DAC.from_pretrained`, CSM `Mimi.from_pretrained`, its Llama 3.2
/// tokenizer and `default_speaker_prompt`).
pub fn codecs_of(family: &str) -> &'static [Codec] {
    match family {
        "outetts" => &[DAC_24K],
        "orpheus" => &[SNAC_24K],
        "dia" => &[DAC_44K],
        "sesame" => &[MIMI, LLAMA32_TOKENIZER, CSM_PROMPTS],
        _ => &[],
    }
}

/// The library folder of a codec: `<library>/<folder>` with all its files, in the first library
/// that has it.
pub fn codec_dir(codec: &Codec, libraries: &[PathBuf]) -> Option<PathBuf> {
    libraries
        .iter()
        .map(|root| root.join(codec.folder))
        .find(|dir| codec.files.iter().all(|(f, _, _)| dir.join(f).is_file()))
}

/// What an mlx-audio folder loads from elsewhere: a text-to-speech family's codecs
/// ([`codecs_of`]), or the processor of a Whisper without one ([`whisper_processor_of`]).
fn companions_of(dir: &Path) -> Vec<Codec> {
    if let Some(processor) = whisper_processor_of(dir) {
        return vec![processor];
    }
    tts_type(dir)
        .map(|t| codecs_of(&t).to_vec())
        .unwrap_or_default()
}

/// The processor an mlx-examples Whisper folder needs ([`is_mlx_whisper_config`] and no
/// processor of its own); `None` for any other folder.
fn whisper_processor_of(dir: &Path) -> Option<Codec> {
    if has_whisper_processor(|name| dir.join(name).is_file()) {
        return None;
    }
    let cfg = read_json(&dir.join("config.json"))?;
    if !is_mlx_whisper_config(&cfg) {
        return None;
    }
    cfg.get("n_vocab")
        .and_then(|v| v.as_u64())
        .and_then(whisper_processor_for_vocab)
}

/// The codecs (or processor) an mlx-audio folder needs ([`companions_of`]), each with its
/// library folder (`None` when it is not downloaded).
pub fn codec_dirs(dir: &Path, libraries: &[PathBuf]) -> Vec<(Codec, Option<PathBuf>)> {
    companions_of(dir)
        .iter()
        .map(|c| (*c, codec_dir(c, libraries)))
        .collect()
}

/// Whether a Hub repo id names an MLX build: published by `mlx-community`, or "mlx" in its name.
/// The original releases of the same families (`nari-labs/Dia-1.6B`, Orpheus GGUF builds) run on
/// other engines and need no MLX codec.
pub fn is_mlx_repo_id(id: &str) -> bool {
    let (owner, name) = id.split_once('/').unwrap_or(("", id));
    owner.eq_ignore_ascii_case("mlx-community") || name.to_ascii_lowercase().contains("mlx")
}

/// What a Hub repo of a text-to-speech family needs besides itself, as the image bundles' plan
/// ([`crate::image_bundles::BundlePlan`], role `codec`, one component per file); `None` for a
/// repo whose family loads nothing else, and for a repo that is no MLX build
/// ([`is_mlx_repo_id`]): only the MLX engine loads these codecs.
pub fn codec_plan_for_repo(
    repo: &str,
    libraries: &[PathBuf],
) -> Option<crate::image_bundles::BundlePlan> {
    if !is_mlx_repo_id(repo) {
        return None;
    }
    let name = repo.rsplit('/').next().unwrap_or(repo).to_ascii_lowercase();
    let (family, codecs): (&str, Vec<Codec>) = match tts_repo_family(repo) {
        Some(family) => (family, codecs_of(family).to_vec()),
        // The processor of an mlx-examples Whisper conversion. mlx-audio's own conversions
        // (`whisper-large-v3-turbo-asr-fp16`) have theirs.
        None if name.contains("whisper") && !name.contains("-asr") => {
            ("whisper", vec![whisper_processor_for_repo(repo)?])
        }
        None => return None,
    };
    if codecs.is_empty() {
        return None;
    }
    let components = codecs
        .iter()
        .flat_map(|c| {
            let dir = codec_dir(c, libraries);
            c.files
                .iter()
                .map(move |(file, size, _)| crate::image_bundles::ComponentPlan {
                    role: "codec".into(),
                    repo: c.repo.into(),
                    file: (*file).into(),
                    size: *size,
                    path: dir
                        .as_ref()
                        .map(|d| d.join(file).to_string_lossy().to_string()),
                })
        })
        .collect();
    Some(crate::image_bundles::BundlePlan {
        bundle_id: if family == "whisper" {
            WHISPER_BUNDLE.to_string()
        } else {
            format!("mlx-audio:{family}")
        },
        label: repo.rsplit('/').next().unwrap_or(repo).to_string(),
        components,
        defaults: Default::default(),
    })
}

/// mlx-audio speech recognition architectures (`model_type`) Harness runs. A NeMo Parakeet
/// config has no `model_type` but its `target` class (see [`is_parakeet_config`]).
const STT_TYPES: &[&str] = &["whisper", "parakeet", "parakeet_tdt"];

/// What the name of a Hub repo of a [`TTS_TYPES`] model contains (the listing has no config),
/// and its family. Plain Chatterbox (`chatterbox-fp16`) and VoxCPM 1.x are other architectures.
const TTS_REPO_NAMES: &[(&str, &str)] = &[
    ("qwen3-tts", "qwen3_tts"),
    ("chatterbox-turbo", "chatterbox_turbo"),
    ("soprano", "soprano"),
    ("voxcpm2", "voxcpm2"),
    ("spark-tts", "spark"),
    ("outetts", "outetts"),
    ("orpheus", "orpheus"),
    ("csm-1b", "sesame"),
];

/// What the name of a Hub repo of a [`STT_TYPES`] model contains (a NeMo Parakeet repo has no
/// `model_type` for its tags to list).
const STT_REPO_NAMES: &[&str] = &["whisper", "parakeet"];

/// Whether a Hub repo is a speech recognizer of a tested architecture: a [`STT_TYPES`] tag (the
/// Hub lists the config's `model_type` among a repo's tags) or its name ([`STT_REPO_NAMES`]).
/// The folder check after the download is [`is_stt_dir`].
pub fn is_stt_repo(id: &str, tags: &[String]) -> bool {
    let name = id.rsplit('/').next().unwrap_or(id).to_ascii_lowercase();
    tags.iter()
        .any(|t| STT_TYPES.contains(&t.to_ascii_lowercase().as_str()))
        || STT_REPO_NAMES.iter().any(|n| name.contains(n))
}

/// Whether a Hub repo is one of mlx-audio's speech models by its tags (an [`AUDIO_TYPES`]
/// `model_type`, or the `mlx-audio` library tag): never a chat model, whatever task it lacks
/// (`mlx-community/Qwen3-ASR-0.6B-4bit` has none). The folder check is [`is_model_dir`]'s.
pub fn is_audio_repo(tags: &[String]) -> bool {
    tags.iter().any(|t| {
        let t = t.to_ascii_lowercase();
        t == "mlx-audio" || AUDIO_TYPES.contains(&t.as_str())
    })
}

/// The text-to-speech family of a Hub repo by its name: [`TTS_REPO_NAMES`], or `dia` for a name
/// that starts with `dia-` (Nari Labs' Dia; "dia" alone is part of too many words).
pub fn tts_repo_family(id: &str) -> Option<&'static str> {
    let name = id.rsplit('/').next().unwrap_or(id).to_ascii_lowercase();
    TTS_REPO_NAMES
        .iter()
        .find(|(n, _)| name.contains(n))
        .map(|(_, f)| *f)
        .or_else(|| name.starts_with("dia-").then_some("dia"))
}

/// `model_type`s a family's Hub repos must list among their tags (the Hub copies a config's
/// `model_type` there) when they list any: a name alone can mislead. `mlx-community/csm-1b-8bit`
/// is a CSM config whose `model_type` is `sam`, and mlx-audio picks the model by `model_type`.
const TTS_REQUIRED_TYPE_TAGS: &[(&str, &[&str])] = &[("sesame", &["sesame", "csm"])];

/// Whether a Hub repo names a text-to-speech model of a tested architecture: its name
/// ([`tts_repo_family`]), and for a family in [`TTS_REQUIRED_TYPE_TAGS`] its `model_type` tag.
pub fn is_tts_repo(id: &str, tags: &[String]) -> bool {
    let Some(family) = tts_repo_family(id) else {
        return false;
    };
    match TTS_REQUIRED_TYPE_TAGS.iter().find(|(f, _)| *f == family) {
        Some((_, types)) if !tags.is_empty() => tags
            .iter()
            .any(|t| types.contains(&t.to_ascii_lowercase().as_str())),
        _ => true,
    }
}

/// `model_type`s of mlx-audio's speech models (recognition and synthesis): such a folder is
/// never a chat model even when its config looks like one (`WhisperForConditionalGeneration`,
/// `Qwen3TTSForConditionalGeneration`).
const AUDIO_TYPES: &[&str] = &[
    "qwen3_tts",
    "whisper",
    "parakeet",
    "kokoro",
    "chatterbox",
    "chatterbox_turbo",
    "dia",
    "sesame",
    "csm",
    "orpheus",
    "outetts",
    "spark",
    "voxcpm",
    "voxcpm2",
    "vibevoice",
    "voxtral",
    "voxtral_tts",
    "qwen3_asr",
    "qwen2_audio",
    "moonshine",
    "sensevoice",
    "indextts",
    "kitten_tts",
    "pocket_tts",
    "soprano",
    "melotts",
    "bark",
    "higgs_audio",
    "fish_qwen3_omni",
    "granite_speech",
    "canary",
    "nemotron_asr",
    "wav2vec",
    "mms",
];

/// Model card tasks of speech models.
const AUDIO_TASKS: &[&str] = &[
    "text-to-speech",
    "text-to-audio",
    "automatic-speech-recognition",
    "audio-to-audio",
];
/// Flags a custom argument line may never set (Harness owns the model, bind and code policy).
const PROTECTED_FLAGS: &[&str] = &[
    "--model",
    "--host",
    "--port",
    "--alias",
    "--trust-remote-code",
    "--adapter-path",
    "--draft-model",
    "--allowed-origins",
    "--task",
];

/// What a launch without a `max_tokens` parameter gets: mlx-lm's own default (512) cuts
/// ordinary chat answers short.
const DEFAULT_MAX_TOKENS: u32 = 8192;

/// What an MLX engine serves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Flavour {
    /// `mlx`: chat (mlx-lm).
    #[default]
    Text,
    /// `mlx-vlm`: chat with images.
    Vision,
    /// `mlx-audio`: text to speech or speech recognition.
    Audio,
    /// `mflux`: text to image and image editing.
    Image,
}

/// An MLX engine: the text one (`mlx`), the vision one (`mlx-vlm`) or the audio one
/// (`mlx-audio`).
#[derive(Debug, Clone, Copy, Default)]
pub struct MlxEngine {
    flavour: Flavour,
}

impl MlxEngine {
    /// `mlx` (mlx-lm).
    pub const TEXT: MlxEngine = MlxEngine {
        flavour: Flavour::Text,
    };
    /// `mlx-vlm`.
    pub const VISION: MlxEngine = MlxEngine {
        flavour: Flavour::Vision,
    };
    /// `mlx-audio`.
    pub const AUDIO: MlxEngine = MlxEngine {
        flavour: Flavour::Audio,
    };
    /// `mflux`.
    pub const IMAGE: MlxEngine = MlxEngine {
        flavour: Flavour::Image,
    };

    fn env(&self) -> &'static str {
        match self.flavour {
            Flavour::Text => ENV,
            Flavour::Vision | Flavour::Audio => VLM_ENV,
            Flavour::Image => MFLUX_ENV,
        }
    }

    /// Whether `path` is a folder this flavour runs.
    pub fn runs(&self, path: &Path) -> bool {
        match self.flavour {
            Flavour::Text => is_model_dir(path),
            Flavour::Vision => is_vlm_dir(path),
            Flavour::Audio => is_audio_dir(path),
            Flavour::Image => is_mflux_dir(path),
        }
    }
}

/// The parsed catalog of a flavour (`params_mlx.toml`, `params_mlx_vlm.toml`,
/// `params_mlx_audio.toml`).
pub fn catalog(flavour: Flavour) -> &'static ParamCatalog {
    static TEXT: OnceLock<ParamCatalog> = OnceLock::new();
    static VISION: OnceLock<ParamCatalog> = OnceLock::new();
    static AUDIO: OnceLock<ParamCatalog> = OnceLock::new();
    static IMAGE: OnceLock<ParamCatalog> = OnceLock::new();
    let parse = |text: &str| -> ParamCatalog {
        toml::from_str(text).expect("the MLX catalogs are valid (unit tested)")
    };
    match flavour {
        Flavour::Text => TEXT.get_or_init(|| parse(include_str!("../params_mlx.toml"))),
        Flavour::Vision => VISION.get_or_init(|| parse(include_str!("../params_mlx_vlm.toml"))),
        Flavour::Audio => AUDIO.get_or_init(|| parse(include_str!("../params_mlx_audio.toml"))),
        Flavour::Image => IMAGE.get_or_init(|| parse(include_str!("../params_mflux.toml"))),
    }
}

/// Capabilities as documented: every flag of the catalog. The worker has no `--help` to probe
/// (its flags are the catalog's own), and without capabilities nothing would be rendered.
fn documented_caps(flavour: Flavour) -> &'static RuntimeCapabilities {
    static TEXT: OnceLock<RuntimeCapabilities> = OnceLock::new();
    static VISION: OnceLock<RuntimeCapabilities> = OnceLock::new();
    static AUDIO: OnceLock<RuntimeCapabilities> = OnceLock::new();
    static IMAGE: OnceLock<RuntimeCapabilities> = OnceLock::new();
    let cell = match flavour {
        Flavour::Text => &TEXT,
        Flavour::Vision => &VISION,
        Flavour::Audio => &AUDIO,
        Flavour::Image => &IMAGE,
    };
    cell.get_or_init(|| {
        let help: String = catalog(flavour)
            .params
            .iter()
            .map(|p| format!("{} N    {}\n", p.flags.join(", "), p.id))
            .collect();
        RuntimeCapabilities::from_outputs(Default::default(), "", &help, "")
    })
}

/// A flag Harness owns, or an abbreviation of one (Python's argparse expands `--trust` to
/// `--trust-remote-code`; the worker refuses those too).
fn is_protected(arg: &str) -> bool {
    let flag = arg.split('=').next().unwrap_or(arg);
    flag.len() > 2 && flag.starts_with("--") && PROTECTED_FLAGS.iter().any(|p| p.starts_with(flag))
}

// ---- model folders -------------------------------------------------------------------------

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// A field of the model card's front matter (`README.md`).
fn card_field(dir: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("README.md")).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            return Some(value.to_string()).filter(|v| !v.is_empty());
        }
    }
    None
}

/// The `tags:` list of the model card's front matter (`- mlx` lines), lower case.
fn card_tags(dir: &Path) -> Vec<String> {
    card_list(dir, "tags")
}

/// A list field of the model card's front matter, lower case: its `- item` lines, or the one
/// value of `key: value`.
fn card_list(dir: &Path, key: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(dir.join("README.md")) else {
        return Vec::new();
    };
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Vec::new();
    }
    let unquote = |v: &str| {
        v.trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_ascii_lowercase()
    };
    let prefix = format!("{key}:");
    let mut items = Vec::new();
    let mut in_list = false;
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            break;
        }
        if let Some(item) = line.trim_start().strip_prefix("- ").filter(|_| in_list) {
            items.push(unquote(item));
        } else if let Some(value) = line.strip_prefix(&prefix) {
            in_list = value.trim().is_empty();
            if !in_list {
                items.push(unquote(value));
            }
        } else {
            in_list = false;
        }
    }
    items
}

/// Bits per weight of MLX's quantization (`"quantization": {"group_size": 64, "bits": 4}`).
pub fn quant_bits(cfg: &serde_json::Value) -> Option<u64> {
    let q = cfg.get("quantization")?;
    q.get("group_size")?;
    q.get("bits")?.as_u64()
}

/// Whether a folder's weights are MLX's: its config has MLX's quantization or its card says
/// `library_name: mlx`. Platform independent: elsewhere such a folder is simply not run.
pub fn is_mlx_checkpoint(dir: &Path) -> bool {
    read_json(&dir.join("config.json")).is_some_and(|c| quant_bits(&c).is_some())
        || card_field(dir, "library_name").is_some_and(|l| l.eq_ignore_ascii_case("mlx"))
}

/// `model_type` of a folder's config.
fn model_type(dir: &Path) -> Option<String> {
    read_json(&dir.join("config.json"))?
        .get("model_type")?
        .as_str()
        .map(str::to_ascii_lowercase)
}

/// Whether a folder is a speech model by its card (`library_name: mlx-audio`, a speech task) or
/// its `model_type`: never a chat model then.
pub fn is_audio_folder(dir: &Path) -> bool {
    card_field(dir, "library_name").is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio"))
        || card_field(dir, "pipeline_tag").is_some_and(|t| AUDIO_TASKS.contains(&t.as_str()))
        || tts_type(dir).is_some_and(|t| AUDIO_TYPES.contains(&t.as_str()))
}

/// Whether an MLX folder has a vision tower, `None` when `path` is not an MLX chat folder.
fn mlx_vision(path: &Path) -> Option<bool> {
    (super::mistralrs::is_checkpoint_dir(path) && is_mlx_checkpoint(path) && !is_audio_folder(path))
        .then(|| {
            read_json(&path.join("config.json")).is_some_and(|c| c.get("vision_config").is_some())
        })
}

/// The mlx-audio text-to-speech architecture of a folder: its `model_type`, or the family whose
/// config does not say it (the worker's `tts_family` tells them apart the same way):
/// * `spark`: Spark-TTS, a Qwen2 config and a `BiCodec/` codec;
/// * `outetts`: a Qwen3 / Llama config whose card says `library_name: outetts`;
/// * `dia`: an encoder-decoder config with a codebook delay pattern;
/// * `orpheus`: a Llama config whose card says `pipeline_tag: text-to-speech` (any other Llama
///   folder is a chat model).
fn tts_type(dir: &Path) -> Option<String> {
    if dir.join("BiCodec").join("config.yaml").is_file()
        && dir.join("audio_tokenizer_config.yaml").is_file()
    {
        return Some("spark".into());
    }
    if card_field(dir, "library_name").is_some_and(|l| l.eq_ignore_ascii_case("outetts")) {
        return Some("outetts".into());
    }
    let cfg = read_json(&dir.join("config.json"))?;
    if cfg.pointer("/data/delay_pattern").is_some() && cfg.pointer("/model/decoder").is_some() {
        return Some("dia".into());
    }
    let kind = cfg.get("model_type")?.as_str()?.to_ascii_lowercase();
    if kind == "llama" && card_field(dir, "pipeline_tag").is_some_and(|t| t == "text-to-speech") {
        return Some("orpheus".into());
    }
    Some(kind)
}

/// Whether `path` is a folder the `mlx-audio` engine runs: an mlx-audio text-to-speech model
/// of a tested architecture ([`TTS_TYPES`]), with `config.json` and safetensors weights, in
/// MLX's format (its card says `mlx` / `mlx-audio` as library or tag, or its config has MLX's
/// quantization).
pub fn is_tts_dir(path: &Path) -> bool {
    path.is_dir()
        && !super::mistralrs::weight_files(path).is_empty()
        && tts_type(path).is_some_and(|t| TTS_TYPES.contains(&t.as_str()))
        && (card_field(path, "library_name")
            .is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio") || l.eq_ignore_ascii_case("mlx"))
            || read_json(&path.join("config.json")).is_some_and(|c| quant_bits(&c).is_some())
            || card_tags(path)
                .iter()
                .any(|t| t == "mlx" || t == "mlx-audio"))
}

/// Whether a config is NeMo's for a Parakeet (RNNT / TDT / CTC) recognizer: no `model_type`, a
/// `target` of NeMo's ASR models.
fn is_parakeet_config(cfg: &serde_json::Value) -> bool {
    cfg.get("model_type").is_none()
        && cfg
            .get("target")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.starts_with("nemo.collections.asr.models.") && t.contains("EncDec"))
}

/// Files mlx-audio's Whisper needs next to its weights: the Hugging Face processor (feature
/// extractor, and the tokenizer as `tokenizer.json` or `vocab.json`). mlx-examples' older
/// conversions (`mlx-community/whisper-large-v3-turbo`: `weights.safetensors` and a short
/// config) have none: they load, then fail at the first transcription ("Processor not found").
pub fn has_whisper_processor(has: impl Fn(&str) -> bool) -> bool {
    has("preprocessor_config.json") && (has("tokenizer.json") || has("vocab.json"))
}

/// Whether a Hub repo's root files are enough for mlx-audio: a Whisper repo
/// ([`is_stt_repo`] by a `whisper` tag or name) needs [`has_whisper_processor`], or is one of
/// mlx-examples' conversions (`config.json` and `weights.npz` / `weights.safetensors`, or
/// quantized `model.safetensors` with a root tiktoken vocabulary) named after an OpenAI size,
/// whose processor is downloaded with it ([`codec_plan_for_repo`]). A listing without file
/// names is taken as it is.
pub fn stt_repo_files_ok(id: &str, tags: &[String], files: &[String]) -> bool {
    let whisper = id
        .rsplit('/')
        .next()
        .unwrap_or(id)
        .to_ascii_lowercase()
        .contains("whisper")
        || tags.iter().any(|t| t.eq_ignore_ascii_case("whisper"));
    let root = |name: &str| files.iter().any(|f| f == name);
    let mlx_examples = root("config.json")
        && (root("weights.npz")
            || root("weights.safetensors")
            || (root("model.safetensors")
                && files
                    .iter()
                    .any(|f| !f.contains('/') && f.ends_with(".tiktoken"))))
        && whisper_processor_for_repo(id).is_some();
    files.is_empty() || !whisper || has_whisper_processor(root) || mlx_examples
}

/// Whether `path` is a speech recognition folder the `mlx-audio` engine runs: a Whisper or
/// Parakeet model in MLX's format (its card says `library_name: mlx` / `mlx-audio`, its config
/// has MLX's quantization or is mlx-examples' Whisper config), with `config.json` and weights
/// ([`audio_weight_files`]), and for Whisper its processor ([`has_whisper_processor`]) or, for an
/// mlx-examples conversion, one to load from the library ([`whisper_processor_of`]). A
/// transformers Whisper checkpoint (no MLX card) stays the Python engine's.
pub fn is_stt_dir(path: &Path) -> bool {
    if !path.is_dir() || audio_weight_files(path).is_empty() {
        return false;
    }
    let Some(cfg) = read_json(&path.join("config.json")) else {
        return false;
    };
    let model_type = cfg
        .get("model_type")
        .and_then(|t| t.as_str())
        .map(str::to_ascii_lowercase);
    if model_type.as_deref() == Some("whisper")
        && !has_whisper_processor(|name| path.join(name).is_file())
        && !is_mlx_whisper_config(&cfg)
    {
        return false;
    }
    let arch = model_type
        .as_deref()
        .is_some_and(|t| STT_TYPES.contains(&t))
        || is_parakeet_config(&cfg);
    // mlx-audio's own conversions (whisper-large-v3-turbo-asr-fp16) have no `library_name`,
    // only the `mlx` tag.
    arch && (card_field(path, "library_name")
        .is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio") || l.eq_ignore_ascii_case("mlx"))
        || quant_bits(&cfg).is_some()
        || is_mlx_whisper_config(&cfg)
        || card_tags(path)
            .iter()
            .any(|t| t == "mlx" || t == "mlx-audio"))
}

/// Whether `path` is (or sits in) a Spark-TTS folder: its `BiCodec/` codec is next to it. Such a
/// folder speaks, MLX build or not, so neither a chat engine (its config is a plain Qwen2 one,
/// also in the original release's `LLM/` subfolder) nor the Python engine claims it.
pub fn is_spark_dir(path: &Path) -> bool {
    let has_codec = |d: &Path| d.join("BiCodec").join("config.yaml").is_file();
    has_codec(path) || path.parent().is_some_and(has_codec)
}

/// Whether `path` is a folder of the `mlx-audio` engine (text to speech or recognition).
pub fn is_audio_dir(path: &Path) -> bool {
    is_tts_dir(path) || is_stt_dir(path) || is_sts_dir(path)
}

/// mlx-audio audio-to-audio architectures (`model_type`) Harness runs: Mel-Band-RoFormer vocal
/// separation (`mlx_audio.sts.models.mel_roformer`).
const STS_TYPES: &[&str] = &["mel_band_roformer"];

/// Whether `path` is an audio-to-audio folder the `mlx-audio` engine runs ([`STS_TYPES`]): its
/// `config.json` names the architecture, it has safetensors weights and an MLX card
/// (`library_name: mlx` / `mlx-audio`, or the `mlx` tag). The worker separates it with
/// `--task separate` behind the gateway's `/v1/audio/process`.
pub fn is_sts_dir(path: &Path) -> bool {
    path.is_dir()
        && !super::mistralrs::weight_files(path).is_empty()
        && model_type(path).is_some_and(|t| STS_TYPES.contains(&t.as_str()))
        && (card_field(path, "library_name")
            .is_some_and(|l| l.eq_ignore_ascii_case("mlx-audio") || l.eq_ignore_ascii_case("mlx"))
            || card_tags(path)
                .iter()
                .any(|t| t == "mlx" || t == "mlx-audio"))
}

/// Whether a Hub repo is an audio-to-audio model of a tested architecture: its tags list a
/// [`STS_TYPES`] `model_type` (the Hub copies the config's there). The folder check after the
/// download is [`is_sts_dir`].
pub fn is_sts_repo(tags: &[String]) -> bool {
    tags.iter()
        .any(|t| STS_TYPES.contains(&t.to_ascii_lowercase().as_str()))
}

/// Families whose mlx-audio `generate` takes a `speed` (mlx-audio 0.5.9); the worker refuses a
/// speed other than 1 for the others.
const TTS_SPEED_TYPES: &[&str] = &["qwen3_tts", "spark"];

/// Whether an mlx-audio text-to-speech folder speaks at a chosen speed (the Speech panel's
/// slider).
pub fn tts_supports_speed(path: &Path) -> bool {
    tts_type(path).is_some_and(|t| TTS_SPEED_TYPES.contains(&t.as_str()))
}

/// The named voices of an mlx-audio text-to-speech folder (Qwen3-TTS: the keys of
/// `talker_config.spk_id`), sorted; empty when it has none (one default voice).
pub fn tts_voices(path: &Path) -> Vec<String> {
    let Some(config) = read_json(&path.join("config.json")) else {
        return Vec::new();
    };
    // CSM speaks in the voice of a recorded prompt: the two it has.
    if config.get("model_type").and_then(|t| t.as_str()) == Some("sesame") {
        return CSM_VOICES.iter().map(|v| v.to_string()).collect();
    }
    let ids = config
        .get("talker_config")
        .and_then(|t| t.get("spk_id"))
        .or_else(|| config.get("spk_id"))
        .and_then(|v| v.as_object());
    let mut voices: Vec<String> = ids
        .map(|m| m.keys().filter(|k| !k.is_empty()).cloned().collect())
        .unwrap_or_default();
    voices.sort();
    voices
}

/// Whether `path` is a folder the `mlx` engine runs: an MLX text generator (not a vision model).
pub fn is_model_dir(path: &Path) -> bool {
    mlx_vision(path) == Some(false)
}

/// Whether `path` is a folder the `mlx-vlm` engine runs: an MLX vision-language model.
pub fn is_vlm_dir(path: &Path) -> bool {
    mlx_vision(path) == Some(true)
}

/// Whether `path` is an MLX folder of any MLX engine.
pub fn is_any_model_dir(path: &Path) -> bool {
    mlx_vision(path).is_some() || is_audio_dir(path) || is_mflux_dir(path)
}

/// mflux's name of a FLUX.2 [klein] base model repo (`black-forest-labs/FLUX.2-klein-4B` ->
/// `flux2-klein-4b`), `None` for any other repo.
pub fn mflux_model_for_base(base: &str) -> Option<&'static str> {
    let base = base.trim().to_ascii_lowercase();
    MFLUX_MODELS
        .iter()
        .find(|(repo, _)| *repo == base)
        .map(|(_, name)| *name)
}

/// The mflux model of a folder: the first `base_model` of its card that [`MFLUX_MODELS`] knows.
pub fn mflux_model(dir: &Path) -> Option<&'static str> {
    card_list(dir, "base_model")
        .iter()
        .find_map(|b| mflux_model_for_base(b))
}

/// The safetensors of an mflux folder's weight folders ([`MFLUX_PARTS`]).
pub fn mflux_weight_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = MFLUX_PARTS
        .iter()
        .flat_map(|part| {
            std::fs::read_dir(dir.join(part))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "safetensors"))
        })
        .collect();
    files.sort();
    files
}

/// The quantization mflux saved a folder with (`quantization_level` in the metadata of its
/// transformer's index), `None` for anything else (a diffusers folder has no such metadata).
fn mflux_quantization(dir: &Path) -> Option<String> {
    let index = read_json(&dir.join("transformer").join("model.safetensors.index.json"))?;
    let meta = index.get("metadata")?;
    meta.get("mflux_version")?;
    Some(match meta.get("quantization_level") {
        Some(serde_json::Value::String(q)) => q.clone(),
        Some(serde_json::Value::Number(q)) => q.to_string(),
        _ => String::new(),
    })
}

/// Whether `path` is a folder the `mflux` engine runs: a FLUX.2 [klein] model ([`mflux_model`])
/// saved by mflux (`transformer/`, `text_encoder/` and `vae/` with an index each, mflux's
/// metadata in the transformer's, and `tokenizer/`), not a diffusers pipeline.
pub fn is_mflux_dir(path: &Path) -> bool {
    path.is_dir()
        && !path.join("model_index.json").exists()
        && mflux_model(path).is_some()
        && mflux_quantization(path).is_some()
        && path.join("tokenizer").join("tokenizer.json").is_file()
        && MFLUX_PARTS.iter().all(|part| {
            let dir = path.join(part);
            dir.join("model.safetensors.index.json").is_file()
                && std::fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == "safetensors"))
        })
}

/// The flavour that runs an MLX folder, `None` for other paths.
pub fn flavour_of(path: &Path) -> Option<Flavour> {
    match mlx_vision(path) {
        Some(true) => Some(Flavour::Vision),
        Some(false) => Some(Flavour::Text),
        None if is_mflux_dir(path) => Some(Flavour::Image),
        None => is_audio_dir(path).then_some(Flavour::Audio),
    }
}

/// What the files of an MLX chat folder say (chat template, vision tower...); `None` for other
/// paths (speech folders included).
pub fn inspect(dir: &Path) -> Option<super::mistralrs::ModelDirInfo> {
    mlx_vision(dir)?;
    super::mistralrs::inspect_checkpoint(dir)
}

/// The library's view of an MLX folder: a chat model like mistral.rs's, or (speech) a pipeline
/// model like the Python engine's, with MLX's bits as its quantization (`4bit`).
fn to_local_model(dir: &Path, root: &Path) -> Option<crate::models::LocalModel> {
    if is_mflux_dir(dir) {
        return Some(mflux_local_model(dir, root));
    }
    let stt = is_stt_dir(dir);
    // A speech config is often no text generator's (Whisper's MLX port, NeMo's Parakeet,
    // Chatterbox, VoxCPM2): its folder is described by its weights and `model_type` alone.
    let info = match super::mistralrs::inspect_checkpoint(dir) {
        Some(info) => info,
        None if stt || is_tts_dir(dir) || is_sts_dir(dir) => audio_dir_info(dir),
        None => return None,
    };
    let mut model = super::mistralrs::to_local_model(dir, &info, root);
    if is_audio_dir(dir) {
        model.kind = crate::domain::ModelKind::Pipeline;
        model.context_length = None;
        // Spark-TTS keeps its codec and speaker encoder in subfolders (`BiCodec/`,
        // `wav2vec2-large-xlsr-53/`), loaded with the model.
        model.size_bytes = model.size_bytes.max(nested_weight_bytes(dir));
    }
    if stt {
        model.task = Some("automatic-speech-recognition".into());
    } else if is_sts_dir(dir) {
        model.task = Some("audio-to-audio".into());
    }
    if let Some(bits) = read_json(&dir.join("config.json")).and_then(|c| quant_bits(&c)) {
        model.quantization = format!("{bits}bit");
    }
    // A Spark-TTS / OuteTTS config is a plain Qwen2 / Qwen3 / Llama one: the folder is named by
    // its speech family.
    if let Some(family) = is_audio_dir(dir)
        .then(|| tts_type(dir))
        .flatten()
        .filter(|t| matches!(t.as_str(), "spark" | "outetts"))
    {
        model.architecture = Some(family);
    }
    Some(model)
}

/// The library's view of an mflux folder: a text-to-image pipeline model (it edits images too)
/// with mflux's bits as its quantization (`4bit`).
fn mflux_local_model(dir: &Path, root: &Path) -> crate::models::LocalModel {
    let weights = mflux_weight_files(dir);
    let info = super::mistralrs::ModelDirInfo {
        weights_bytes: weights
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum(),
        weights,
        architecture: mflux_model(dir).map(str::to_string),
        complete: true,
        ..Default::default()
    };
    let mut model = super::mistralrs::to_local_model(dir, &info, root);
    model.kind = crate::domain::ModelKind::Pipeline;
    model.context_length = None;
    model.task = Some("text-to-image".into());
    model.quantization = match mflux_quantization(dir).as_deref() {
        Some(q) if !q.is_empty() && q.chars().all(|c| c.is_ascii_digit()) => format!("{q}bit"),
        _ => String::new(),
    };
    model
}

/// Bytes of the safetensors under `dir`, one folder level down included.
fn nested_weight_bytes(dir: &Path) -> u64 {
    let size = |d: &Path| -> u64 {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "safetensors"))
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    };
    let nested: u64 = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| size(&e.path()))
        .sum();
    size(dir) + nested
}

/// What the files of a speech folder say: its weights and `model_type`.
fn audio_dir_info(dir: &Path) -> super::mistralrs::ModelDirInfo {
    let weights = audio_weight_files(dir);
    super::mistralrs::ModelDirInfo {
        weights_bytes: weights
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum(),
        weights,
        // No `model_type`: a Spark-TTS or OuteTTS folder by its codec or card, else Parakeet.
        architecture: model_type(dir)
            .or_else(|| tts_type(dir))
            .or_else(|| Some("parakeet".into())),
        complete: true,
        ..Default::default()
    }
}

pub(crate) fn library_models_in(
    root: &Path,
    tree: &crate::models::discovery::DirectoryTree,
) -> Vec<crate::models::LocalModel> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Vec::new();
    }
    // Spark-TTS and OuteTTS folders are told by their codec or card ([`tts_type`]), not
    // necessarily by a `config.json`.
    tree.scan(
        root,
        &["config.json", "audio_tokenizer_config.yaml", "README.md"],
        |dir| is_any_model_dir(dir).then(|| dir.to_path_buf()),
        |dir| dir.clone(),
    )
    .into_iter()
    .filter_map(|dir| to_local_model(&dir, root))
    .collect()
}

/// MLX model folders under `roots` (a few levels deep: `<root>/<owner>/<repo>`). Only where
/// MLX runs: elsewhere they are not listed (mistral.rs can't run them either).
pub fn library_models(roots: &[PathBuf]) -> Vec<crate::models::LocalModel> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Vec::new();
    }
    roots
        .iter()
        .flat_map(|root| {
            crate::tts::scan_folders(
                std::slice::from_ref(root),
                |dir| is_any_model_dir(dir).then(|| dir.to_path_buf()),
                |a, b| a == b,
            )
            .into_iter()
            .filter_map(|dir| to_local_model(&dir, root))
            .collect::<Vec<_>>()
        })
        .collect()
}

/// Removes an MLX model folder (everything in it: config, tokenizer, weights); refuses anything
/// that is not one strictly inside one of `roots`.
pub fn delete_model_dir(dir: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if !is_any_model_dir(dir) {
        return Err(err("mlx.model_unsupported")
            .with("path", dir.display())
            .into());
    }
    super::mistralrs::remove_folder_within(dir, roots)
}

/// The family that runs an MLX folder (`mlx`, `mlx-vlm`, `mlx-audio`), `None` for other paths.
pub fn family_of(path: &Path) -> Option<&'static str> {
    Some(match flavour_of(path)? {
        Flavour::Text => MLX,
        Flavour::Vision => MLX_VLM,
        Flavour::Audio => MLX_AUDIO,
        Flavour::Image => MFLUX,
    })
}

// ---- the environment -----------------------------------------------------------------------

/// The worker script of this version, written next to its environment (rewritten when it
/// differs, so an app update brings its worker along).
pub fn worker_script(flavour: Flavour) -> Result<PathBuf, String> {
    let (env, name, body) = match flavour {
        Flavour::Text => (ENV, "mlx_worker.py", WORKER),
        Flavour::Vision => (VLM_ENV, "mlx_vlm_worker.py", VLM_WORKER),
        Flavour::Audio => (VLM_ENV, "mlx_audio_worker.py", AUDIO_WORKER),
        Flavour::Image => (MFLUX_ENV, "mflux_worker.py", IMAGE_WORKER),
    };
    let dir = crate::pyenv::envs::env_dir_in(&crate::pyenv::envs::root(), env);
    let path = dir.join(name);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(body) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, body))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(path)
}

/// The interpreter a text launch runs: the `mlx` environment's, when it is installed for this
/// version; else `runtime.family_not_installed: mlx` (the UI offers the install), or why this
/// Mac can't have it.
pub fn binary() -> Result<PathBuf, String> {
    MlxEngine::TEXT.binary()
}

/// [`binary`] of the vision engine (the `mlx-vlm` environment).
pub fn vlm_binary() -> Result<PathBuf, String> {
    MlxEngine::VISION.binary()
}

/// [`binary`] of the audio engine (the `mlx-vlm` environment too).
pub fn audio_binary() -> Result<PathBuf, String> {
    MlxEngine::AUDIO.binary()
}

/// [`binary`] of the image engine (the `mflux` environment).
pub fn mflux_binary() -> Result<PathBuf, String> {
    MlxEngine::IMAGE.binary()
}

impl MlxEngine {
    /// The interpreter of this flavour's environment (see [`binary`]).
    pub fn binary(&self) -> Result<PathBuf, String> {
        let root = crate::pyenv::envs::root();
        let Some(spec) = crate::pyenv::envs::spec(self.env()) else {
            return Err(err("mlx.unsupported_platform").into());
        };
        if let Some(refusal) = crate::pyenv::envs::platform_refusal(spec) {
            return Err(refusal);
        }
        if !crate::pyenv::envs::status_in(&root, spec).ready {
            return Err(format!(
                "{}: {}",
                crate::runtimes::ERR_FAMILY_NOT_INSTALLED,
                self.family()
            ));
        }
        Ok(crate::pyenv::envs::python_in(&root, self.env()))
    }
}

/// The weights a launch of `dir` loads: an mflux folder's parts, else the folder's own
/// safetensors (or mlx-examples' npz).
fn launch_weight_files(dir: &Path) -> Vec<PathBuf> {
    if is_mflux_dir(dir) {
        mflux_weight_files(dir)
    } else {
        audio_weight_files(dir)
    }
}

/// Unified memory a launch takes: the weights and a margin for the KV cache and MLX itself. An
/// image's own share (it grows with the size asked for) is checked by the worker per request.
pub fn vram_need(config: &LaunchConfig) -> u64 {
    let weights: u64 = launch_weight_files(&config.model_path)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    weights + weights / 5 + (300 << 20)
}

impl Engine for MlxEngine {
    fn family(&self) -> &'static str {
        match self.flavour {
            Flavour::Text => MLX,
            Flavour::Vision => MLX_VLM,
            Flavour::Audio => MLX_AUDIO,
            Flavour::Image => MFLUX,
        }
    }

    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Server
    }

    fn release_source(&self) -> ReleaseSource {
        // Not a GitHub family: the interpreter comes from a managed environment.
        ReleaseSource {
            repo: "",
            binary: "python",
        }
    }

    fn release_asset(
        &self,
        _release: &serde_json::Value,
        _query: &crate::hardware::AssetQuery,
    ) -> Option<crate::runtime::ReleaseAssetInfo> {
        None
    }

    fn pick_release_assets(
        &self,
        _assets: &[ReleaseAsset],
        _backends: &[String],
    ) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
        None
    }

    /// Folders are told apart by [`is_model_dir`] / [`is_vlm_dir`] / [`is_tts_dir`], not by a
    /// file format.
    fn supports(&self, _format: ModelFormat) -> bool {
        false
    }

    fn detect_capabilities(&self, binary: &Path) -> Arc<RuntimeCapabilities> {
        Arc::new(self.probe_capabilities(binary))
    }

    fn cached_capabilities(&self, binary: &Path) -> Option<Arc<RuntimeCapabilities>> {
        Some(self.detect_capabilities(binary))
    }

    /// Nothing to probe: the catalog is the worker's own.
    fn probe_capabilities(&self, _binary: &Path) -> RuntimeCapabilities {
        RuntimeCapabilities::default()
    }

    fn param_catalog(&self) -> &'static ParamCatalog {
        catalog(self.flavour)
    }

    fn as_server(&self) -> Option<&dyn ServerEngine> {
        Some(self)
    }
}

impl ServerEngine for MlxEngine {
    fn build_launch(
        &self,
        config: &LaunchConfig,
        _caps: &ServerCaps,
        _runtime: Option<&RuntimeCapabilities>,
    ) -> Result<BuildOutput, String> {
        if !crate::network::is_loopback_host(&config.host) {
            return Err(err("process.mlx_loopback_only")
                .with("host", &config.host)
                .into());
        }
        if !self.runs(&config.model_path) {
            return Err(err("mlx.model_unsupported")
                .with("path", config.model_path.display())
                .into());
        }
        build_args(config, &worker_script(self.flavour)?, self.flavour)
    }

    fn health_path(&self) -> &'static str {
        "/health"
    }

    fn health(&self, status: u16, body: &str) -> bool {
        status == 200
            && serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "ok"))
                .unwrap_or(false)
    }

    fn openai_base(&self, host_url: &str) -> String {
        format!("{}/v1", host_url.trim_end_matches('/'))
    }

    /// No `/metrics`, no slots (liveness comes from `/health`).
    fn metrics_source(&self) -> MetricsSource {
        MetricsSource {
            metrics_path: "",
            slots_path: None,
        }
    }

    fn parse_metrics(&self, _text: &str) -> Metrics {
        Metrics::default()
    }

    /// A Python worker: [`crate::pyenv::python_env`] and the per-run key. It listens on its
    /// loopback port itself (no socket: macOS has no `unshare`).
    fn worker_env(&self, api_key: &str) -> Option<Vec<(&'static str, std::ffi::OsString)>> {
        Some(super::python_hf::worker_env(api_key))
    }

    /// The weights plus MLX's share (KV cache, activations, the interpreter), all in unified
    /// memory: measured on an M1 with Qwen3-4B 4-bit (2.1 GB of weights, 2.7 GB at peak).
    fn estimate_memory(
        &self,
        input: &FitInput,
        _meta: Option<&GgufMetadata>,
        budget: &MemoryBudget,
        _measured: &[Measurement],
    ) -> FitResult {
        let model = if input.model_size_bytes > 0 {
            input.model_size_bytes
        } else {
            input
                .model_path
                .as_deref()
                .map(|p| {
                    launch_weight_files(Path::new(p))
                        .iter()
                        .filter_map(|f| std::fs::metadata(f).ok())
                        .map(|m| m.len())
                        .sum()
                })
                .unwrap_or(0)
        };
        let compute = (300u64 << 20) + model / 5;
        let total = model + compute;
        let estimate = MemoryEstimate {
            ram_bytes: total,
            model_host_bytes: model,
            compute_host_bytes: compute,
            slots: 1,
            exact: false,
            ..Default::default()
        };
        let category = if total * 10 <= budget.ram_bytes * 9 {
            FitCategory::CpuHeavy
        } else {
            FitCategory::OomRisk
        };
        FitResult {
            category,
            vram_percentage: 0,
            ram_percentage: (total * 100).checked_div(budget.ram_bytes).unwrap_or(0) as u32,
            estimate,
            budget: *budget,
            source: EstimateSource::Estimate,
            measured_at: None,
            recommended_gpu_layers: 0,
            slots: None,
            kv: None,
        }
    }
}

/// `--companion owner/repo=folder` for each codec a text-to-speech folder's family loads, found
/// in `libraries`; a codec that is not downloaded fails the launch with its repo named.
fn companion_args(model: &Path, libraries: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    for (codec, dir) in codec_dirs(model, libraries) {
        let dir = dir.ok_or_else(|| err("mlx.codec_missing").with("repo", codec.repo))?;
        args.push("--companion".into());
        args.push(format!("{}={}", codec.repo, dir.to_string_lossy()));
    }
    Ok(args)
}

/// Upper bound of mlx-lm's prompt cache (`--prompt-cache-bytes`), which otherwise only counts
/// entries (10). A chat whose template rewrites earlier turns (Qwen3 drops their thinking) leaves
/// one entry per turn, each the size of the whole conversation's KV: measured on an M1 8 GB,
/// Qwen3-0.6B held 8 entries and 4.35 GB at 4k tokens and ran out of Metal memory. Half of the
/// GPU share that the model leaves, between 256 MiB and 8 GiB; 2 GiB when the budget is unknown.
fn prompt_cache_bytes(need: u64, budget: &crate::vram::MemoryBudget) -> u64 {
    const MIB: u64 = 1 << 20;
    if !budget.unified || budget.vram_bytes == 0 {
        return 2048 * MIB;
    }
    (budget.vram_bytes.saturating_sub(need) / 2).clamp(256 * MIB, 8192 * MIB)
}

/// Longest generation of an audio request (`--max-seconds`) when the user set none: 600 s (about
/// ten minutes of reading), 30 s under the gateway's request timeout, so the worker answers the
/// audio made by then before the gateway gives up on the request (a 504 would lose it all). It
/// bounds transcriptions of the same worker as well (Whisper / Parakeet: a 504 after it), and
/// an image request of the mflux worker (checked between denoising steps: a 504). The
/// gateway timeout is read when the worker starts: a change in Settings applies from its next
/// start.
fn audio_max_seconds(gateway_timeout_secs: u32) -> u32 {
    const DEFAULT: u32 = 600;
    if gateway_timeout_secs == 0 {
        return DEFAULT;
    }
    DEFAULT.min(gateway_timeout_secs.saturating_sub(30).max(5))
}

/// The command line of a launch: the worker script first (the program is the interpreter).
fn build_args(
    config: &LaunchConfig,
    script: &Path,
    flavour: Flavour,
) -> Result<BuildOutput, String> {
    let mut args: Vec<String> = vec![
        script.to_string_lossy().to_string(),
        "--model".into(),
        config.model_path.to_string_lossy().to_string(),
        "--port".into(),
        config.port.to_string(),
    ];
    if let Some(alias) = crate::process::served_alias(config) {
        args.push("--alias".into());
        args.push(alias);
    }
    if flavour == Flavour::Audio && is_stt_dir(&config.model_path) {
        args.push("--task".into());
        args.push("transcription".into());
    } else if flavour == Flavour::Audio && is_sts_dir(&config.model_path) {
        args.push("--task".into());
        args.push("separate".into());
    }
    if flavour == Flavour::Audio && !companions_of(&config.model_path).is_empty() {
        let libraries = crate::config::AppSettings::load().all_models_dirs();
        args.extend(companion_args(&config.model_path, &libraries)?);
    }
    let ctx = RenderContext {
        host: config.host.clone(),
        ..Default::default()
    };
    let rendered = crate::params::render_params(
        catalog(flavour),
        Some(documented_caps(flavour)),
        &config.params,
        &ctx,
    )?;
    args.extend(rendered.args);
    if let Some(custom) = config
        .custom_args
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let custom = crate::process::split_args(custom)?;
        if let Some(bad) = custom.iter().find(|a| is_protected(a)) {
            return Err(err("params.protected_flag").with("flag", bad).into());
        }
        args.extend(custom);
    }
    if flavour == Flavour::Text
        && !args
            .iter()
            .any(|a| a.split('=').next() == Some("--prompt-cache-bytes"))
    {
        let budget = crate::vram::budget(&crate::hardware::detect_hardware());
        args.push("--prompt-cache-bytes".into());
        args.push(prompt_cache_bytes(vram_need(config), &budget).to_string());
    }
    if matches!(flavour, Flavour::Audio | Flavour::Image)
        && !args
            .iter()
            .any(|a| a.split('=').next() == Some("--max-seconds"))
    {
        let gateway = crate::config::AppSettings::load()
            .gateway
            .request_timeout_secs;
        args.push("--max-seconds".into());
        args.push(audio_max_seconds(gateway).to_string());
    }
    if matches!(flavour, Flavour::Text | Flavour::Vision)
        && !args
            .iter()
            .any(|a| a.split('=').next() == Some("--max-tokens"))
    {
        args.push("--max-tokens".into());
        args.push(DEFAULT_MAX_TOKENS.to_string());
    }
    Ok(BuildOutput {
        args,
        skipped: rendered.skipped,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    /// The files of `mlx-community/Qwen3-4B-Instruct-2507-4bit` (config trimmed).
    const MLX_CONFIG: &str = r#"{"architectures":["Qwen3ForCausalLM"],"model_type":"qwen3","max_position_embeddings":262144,"quantization":{"group_size":64,"bits":4,"mode":"affine"},"quantization_config":{"group_size":64,"bits":4,"mode":"affine"}}"#;

    /// `mlx-community/Qwen3-VL-2B-Instruct-4bit` (config trimmed).
    const VLM_CONFIG: &str = r#"{"architectures":["Qwen3VLForConditionalGeneration"],"model_type":"qwen3_vl","vision_config":{"depth":24},"text_config":{"num_hidden_layers":28},"quantization":{"group_size":64,"bits":4,"mode":"affine"}}"#;

    #[test]
    fn catalog_parses() {
        for flavour in [
            Flavour::Text,
            Flavour::Vision,
            Flavour::Audio,
            Flavour::Image,
        ] {
            assert!(!catalog(flavour).params.is_empty());
        }
    }

    #[test]
    fn mlx_folders_are_ours_and_not_mistral_rs() {
        let quantized = folder(&[
            ("config.json", MLX_CONFIG),
            ("model.safetensors", "x"),
            ("tokenizer.json", "{}"),
        ]);
        assert!(is_model_dir(quantized.path()));
        assert!(!super::super::mistralrs::is_model_dir(quantized.path()));
        let m = to_local_model(quantized.path(), quantized.path().parent().unwrap()).unwrap();
        assert_eq!(m.quantization, "4bit");
        assert_eq!(m.kind, crate::domain::ModelKind::Llm);
        assert_eq!(m.architecture.as_deref(), Some("qwen3"));

        // A bf16 conversion has no quantization; its card says MLX.
        let card = folder(&[
            (
                "config.json",
                r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama"}"#,
            ),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: mlx\nlicense: apache-2.0\ntags:\n- mlx\n---\n# x\n",
            ),
        ]);
        assert!(is_model_dir(card.path()));
        assert!(!super::super::mistralrs::is_model_dir(card.path()));

        // A plain transformers checkpoint stays with mistral.rs.
        let plain = folder(&[
            (
                "config.json",
                r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama","quantization_config":{"quant_method":"awq","bits":4}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        assert!(!is_model_dir(plain.path()));
        assert!(super::super::mistralrs::is_model_dir(plain.path()));

        // An MLX vision model is mlx-vlm's: not the text engine's, not mistral.rs's.
        let vlm = folder(&[
            ("config.json", VLM_CONFIG),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: transformers\npipeline_tag: image-text-to-text\ntags:\n- mlx\n---\n",
            ),
        ]);
        assert!(!is_model_dir(vlm.path()));
        assert!(is_vlm_dir(vlm.path()));
        assert!(MlxEngine::VISION.runs(vlm.path()));
        assert!(!MlxEngine::TEXT.runs(vlm.path()));
        assert!(!super::super::mistralrs::is_model_dir(vlm.path()));
        assert!(!super::super::python_hf::is_model_dir(vlm.path()));
        assert!(inspect(vlm.path()).unwrap().vision);
        let m = to_local_model(vlm.path(), vlm.path().parent().unwrap()).unwrap();
        assert_eq!(m.kind, crate::domain::ModelKind::Llm);
        assert_eq!(m.quantization, "4bit");
        // ...and the text folder is not a vision one.
        assert!(!is_vlm_dir(quantized.path()));
        assert!(!inspect(quantized.path()).unwrap().vision);
        assert!(inspect(plain.path()).is_none());
    }

    /// An audio worker runs a request for at most 600 s, 30 s under the gateway's timeout (5 s
    /// at least); a `max_seconds` of the user's wins. Text workers take no such flag.
    #[test]
    fn audio_requests_end_before_the_gateway_gives_up() {
        assert_eq!(audio_max_seconds(0), 600);
        assert_eq!(audio_max_seconds(600), 570);
        assert_eq!(audio_max_seconds(3600), 600);
        assert_eq!(audio_max_seconds(120), 90);
        assert_eq!(audio_max_seconds(10), 5);
        let dir = folder(&[("config.json", MLX_CONFIG), ("model.safetensors", "x")]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            ..Default::default()
        };
        let count = |args: &[String]| args.iter().filter(|a| *a == "--max-seconds").count();
        let out = build_args(&config, Path::new("w.py"), Flavour::Audio).unwrap();
        assert_eq!(count(&out.args), 1, "{:?}", out.args);
        let mut set = config.clone();
        set.params
            .insert("max_seconds".into(), crate::params::ParamValue::Int(42));
        let out = build_args(&set, Path::new("w.py"), Flavour::Audio).unwrap();
        assert_eq!(count(&out.args), 1, "{:?}", out.args);
        assert!(
            out.args.join(" ").contains("--max-seconds 42"),
            "{:?}",
            out.args
        );
        let out = build_args(&config, Path::new("w.py"), Flavour::Text).unwrap();
        assert_eq!(count(&out.args), 0, "{:?}", out.args);
    }

    /// The prompt cache is bounded by half of the GPU share the model leaves (256 MiB..8 GiB),
    /// for text models only, and a `--prompt-cache-bytes` of the user's wins.
    #[test]
    fn the_prompt_cache_is_bounded_by_the_memory_left() {
        const MIB: u64 = 1 << 20;
        let m1_8gb = crate::vram::MemoryBudget {
            vram_bytes: 5461 * MIB,
            unified: true,
            ..Default::default()
        };
        // Qwen3-0.6B-4bit: ~700 MiB needed, ~2.3 GiB for the cache.
        assert_eq!(prompt_cache_bytes(701 * MIB, &m1_8gb), 2380 * MIB);
        assert_eq!(prompt_cache_bytes(5400 * MIB, &m1_8gb), 256 * MIB);
        let big = crate::vram::MemoryBudget {
            vram_bytes: 98_304 * MIB,
            ..m1_8gb
        };
        assert_eq!(prompt_cache_bytes(4096 * MIB, &big), 8192 * MIB);
        let unknown = crate::vram::MemoryBudget {
            vram_bytes: 0,
            ..m1_8gb
        };
        assert_eq!(prompt_cache_bytes(701 * MIB, &unknown), 2048 * MIB);

        let dir = folder(&[("config.json", MLX_CONFIG), ("model.safetensors", "x")]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            ..Default::default()
        };
        let count = |out: &BuildOutput| {
            out.args
                .iter()
                .filter(|a| a.starts_with("--prompt-cache-bytes"))
                .count()
        };
        let out = build_args(&config, Path::new("w.py"), Flavour::Text).unwrap();
        assert_eq!(count(&out), 1, "{:?}", out.args);
        for flavour in [Flavour::Vision, Flavour::Audio] {
            let out = build_args(&config, Path::new("w.py"), flavour).unwrap();
            assert_eq!(count(&out), 0, "{flavour:?}: {:?}", out.args);
        }
        let mut custom = config.clone();
        custom.custom_args = Some("--prompt-cache-bytes=1GB".into());
        let out = build_args(&custom, Path::new("w.py"), Flavour::Text).unwrap();
        assert_eq!(count(&out), 1, "{:?}", out.args);
        assert!(out.args.contains(&"--prompt-cache-bytes=1GB".to_string()));
    }

    #[test]
    fn launch_args_keep_harness_flags_and_a_long_answer_limit() {
        let dir = folder(&[("config.json", MLX_CONFIG), ("model.safetensors", "x")]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11450,
            ..Default::default()
        };
        let out = build_args(&config, Path::new("/env/mlx_worker.py"), Flavour::Text).unwrap();
        assert_eq!(out.args[0], "/env/mlx_worker.py");
        let at = |flag: &str| {
            out.args
                .iter()
                .position(|a| a == flag)
                .map(|i| out.args[i + 1].as_str())
        };
        assert_eq!(at("--port"), Some("11450"));
        assert_eq!(at("--max-tokens"), Some("8192"));
        // The worker binds 127.0.0.1 itself; no host on the command line.
        assert!(at("--host").is_none());

        // The catalog is rendered (the worker has no --help to probe).
        let mut set = config.clone();
        set.params
            .insert("max_tokens".into(), crate::params::ParamValue::Int(512));
        set.params.insert(
            "prompt_cache_size".into(),
            crate::params::ParamValue::Int(4),
        );
        set.params.insert(
            "kv_bits".into(),
            crate::params::ParamValue::Text("8".into()),
        );
        let out = build_args(&set, Path::new("w.py"), Flavour::Text).unwrap();
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        let args = out.args.join(" ");
        assert!(args.contains("--max-tokens 512"), "{args}");
        assert!(args.contains("--prompt-cache-size 4"), "{args}");
        assert!(args.contains("--kv-bits 8"), "{args}");
        assert_eq!(args.matches("--max-tokens").count(), 1, "{args}");

        let mut custom = config.clone();
        custom.custom_args = Some("--max-tokens 100".into());
        let out = build_args(&custom, Path::new("w.py"), Flavour::Text).unwrap();
        assert_eq!(
            out.args.iter().filter(|a| *a == "--max-tokens").count(),
            1,
            "{:?}",
            out.args
        );
        for bad in [
            "--trust-remote-code",
            "--trust",
            "--trust-remote",
            "--host 0.0.0.0",
            "--ho 0.0.0.0",
            "--adapter-path /x",
            "--adapter /x",
            "--model=/etc",
            "--mod /etc",
            "--allowed *",
        ] {
            let mut c = config.clone();
            c.custom_args = Some(bad.into());
            let e = build_args(&c, Path::new("w.py"), Flavour::Text).unwrap_err();
            assert!(e.contains("params.protected_flag"), "{bad}: {e}");
            let e = build_args(&c, Path::new("w.py"), Flavour::Vision).unwrap_err();
            assert!(e.contains("params.protected_flag"), "{bad}: {e}");
        }
    }

    #[test]
    fn vision_launch_args() {
        let dir = folder(&[("config.json", VLM_CONFIG), ("model.safetensors", "x")]);
        let mut config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11451,
            ..Default::default()
        };
        config.params.insert(
            "kv_bits".into(),
            crate::params::ParamValue::Text("8".into()),
        );
        let out = build_args(
            &config,
            Path::new("/env/mlx_vlm_worker.py"),
            Flavour::Vision,
        )
        .unwrap();
        assert_eq!(out.args[0], "/env/mlx_vlm_worker.py");
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        let at = |flag: &str| {
            out.args
                .iter()
                .position(|a| a == flag)
                .map(|i| out.args[i + 1].as_str())
        };
        assert_eq!(at("--max-tokens"), Some("8192"));
        assert_eq!(at("--kv-bits"), Some("8"));
        assert_eq!(MlxEngine::VISION.family(), MLX_VLM);
        assert_eq!(MlxEngine::TEXT.family(), MLX);
        // A text folder is not the vision engine's to launch, nor the reverse.
        let e = MlxEngine::TEXT
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap_err();
        assert!(e.contains("mlx.model_unsupported"), "{e}");
    }

    #[test]
    fn launches_are_loopback_only() {
        let dir = folder(&[("config.json", MLX_CONFIG), ("model.safetensors", "x")]);
        let config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "0.0.0.0".into(),
            port: 11450,
            ..Default::default()
        };
        for engine in [MlxEngine::TEXT, MlxEngine::VISION] {
            let e = engine
                .build_launch(&config, &ServerCaps::default(), None)
                .unwrap_err();
            assert!(e.contains("process.mlx_loopback_only"), "{e}");
        }
    }

    /// The worker's own argument filter, run with a system Python when there is one (it needs
    /// neither MLX nor the environment): abbreviations of Harness's flags and repeats exit.
    #[test]
    fn the_worker_refuses_abbreviations_and_repeats() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("mlx_worker.py");
        std::fs::write(&script, WORKER).unwrap();
        let check = |args: &[&str]| -> Option<(bool, String)> {
            let code = format!(
                "import sys; sys.path.insert(0, {:?}); import mlx_worker as w; print(w._split_args({:?}))",
                dir.path().to_string_lossy(),
                args
            );
            let out = std::process::Command::new(python)
                .args(["-I", "-c", &code])
                .env("HARNESS_WORKER_API_KEY", "k")
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr);
            Some((out.status.success(), text))
        };
        let (ok, text) = check(&["--model", "/m", "--port", "1", "--max-tokens", "9"]).unwrap();
        assert!(ok, "{text}");
        assert!(text.contains("'--host', '127.0.0.1'"), "{text}");
        for bad in [
            &["--trust"][..],
            &["--trust-remote"],
            &["--trust-remote-code"],
            &["--ho", "0.0.0.0"],
            &["--allowed-origins", "*"],
            &["--adapter", "/x"],
            &["--draft", "/x"],
            &["--model", "/m", "--mod", "/etc"],
            &["--model", "/m", "--model", "/etc"],
            &["--port", "1", "--port=2"],
        ] {
            let (ok, text) = check(bad).unwrap();
            assert!(!ok, "{bad:?} was accepted: {text}");
        }
    }

    /// The system's Python 3 (`python3`, or `python` on Windows), `None` when there is none. On
    /// Windows without Python, `python` is the Store's stand-in: it runs and fails ("Python
    /// bulunamadı"), so the version it prints decides, not whether it starts.
    pub(crate) fn system_python() -> Option<&'static str> {
        static FOUND: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
        *FOUND.get_or_init(|| {
            let python = if cfg!(windows) { "python" } else { "python3" };
            let out = std::process::Command::new(python)
                .args(["-I", "-c", "import sys; print(sys.version_info[0])"])
                .output()
                .ok()?;
            (out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "3")
                .then_some(python)
        })
    }

    /// Runs `code` with a system Python (`None` when there is none) next to the vision worker
    /// (it needs neither MLX nor the environment for its checks): `w` is the worker module.
    fn vision_worker_py(code: &str) -> Option<(bool, String)> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_vlm_worker.py"), VLM_WORKER).unwrap();
        let python = system_python()?;
        let code = format!(
            "import sys, json, asyncio; sys.path.insert(0, {:?}); import mlx_vlm_worker as w\n{code}",
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &code])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        Some((out.status.success(), text))
    }

    /// The vision worker's request check: only inline media of the right kind; input_audio is
    /// rewritten into a data: URL; no adapters or other models.
    #[test]
    fn the_vision_worker_takes_only_inline_media() {
        let image = |url: &str| {
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "text", "text": "hi"},
                {"type": "image_url", "image_url": {"url": url, "detail": "auto"}}
            ]}]})
        };
        let audio = |data: &str| {
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "input_audio", "input_audio": {"data": data, "format": "wav"}}
            ]}]})
        };
        let check = |body: &serde_json::Value| {
            vision_worker_py(&format!(
                "e, out = w.check_body({:?}.encode())\nprint(json.dumps([e, out and json.loads(out)]))",
                body.to_string()
            ))
            .map(|(ok, text)| {
                assert!(ok, "{text}");
                serde_json::from_str::<serde_json::Value>(text.trim()).unwrap()
            })
        };
        let Some(ok) = check(&image("data:image/png;base64,iVBORw0K")) else {
            eprintln!("no system python, skipping");
            return;
        };
        assert!(ok[0].is_null(), "{ok}");
        // Base64 audio becomes a data: URL, so mlx-vlm never takes it for a path ("/9j..." is
        // valid base64).
        let out = check(&audio("/9j/AAAA")).unwrap();
        assert!(out[0].is_null(), "{out}");
        assert_eq!(
            out[1]["messages"][0]["content"][0]["input_audio"]["data"],
            "data:audio/wav;base64,/9j/AAAA"
        );
        let out = check(&audio("data:audio/wav;base64,AAAA")).unwrap();
        assert!(out[0].is_null(), "{out}");
        for bad in [
            image("http://example.com/x.png"),
            image("/etc/hosts"),
            image("file:///etc/hosts"),
            image("data:text/plain;base64,AAAA"),
            image("data:image/png,notbase64"),
            image("data:image/png;base64,***"),
            audio("/etc/hosts"),
            audio("http://x"),
            audio("file:///etc/hosts"),
            audio("./a.wav"),
            audio("data:image/png;base64,AAAA"),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA", "detail": "../x"}}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": "png"}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "video_url", "video_url": "/v.mp4"}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "video_url", "video_url": {"url": "data:image/png;base64,AAAA"}}]}]}),
            serde_json::json!({"messages": [{"role": "user", "content": [
                {"type": "file", "file": {"file_id": "x"}}]}]}),
            serde_json::json!({"messages": [], "adapter_path": "/x"}),
            serde_json::json!({"messages": [], "model_path": "/x"}),
        ] {
            let out = check(&bad).unwrap();
            assert!(out[0].is_string(), "{bad} was accepted: {out}");
        }
    }

    /// The vision worker's ASGI guard around a stand-in app: only its three routes, the key,
    /// the body limit, and the (checked) body replayed to the app.
    #[test]
    fn the_vision_worker_guard_lets_through_only_what_it_checked() {
        let script = r#"
seen = []
async def inner(scope, receive, send):
    body = b""
    while True:
        m = await receive()
        body += m.get("body", b"")
        if not m.get("more_body"):
            break
    seen.append((scope["path"], body, dict(scope["headers"]).get(b"content-length")))
    await send({"type": "http.response.start", "status": 200, "headers": []})
    await send({"type": "http.response.body", "body": b"inner"})

app = w.guard(inner, "alias")

async def call(method, path, headers=(), chunks=(b"",), kind="http"):
    sent = []
    queue = [{"type": "http.request", "body": c, "more_body": i < len(chunks) - 1}
             for i, c in enumerate(chunks)]
    async def receive():
        return queue.pop(0) if queue else {"type": "http.disconnect"}
    async def send(m):
        sent.append(m)
    scope = {"type": kind, "method": method, "path": path,
             "headers": [(k.lower().encode(), v.encode()) for k, v in headers]}
    await app(scope, receive, send)
    if kind == "websocket":
        return sent[0]["type"], sent[0].get("code")
    return sent[0]["status"], b"".join(m.get("body", b"") for m in sent[1:])

KEY = [("Authorization", "Bearer k")]
async def main():
    r = {}
    for m, p in [("HEAD", "/v1/models"), ("OPTIONS", "/v1/chat/completions"),
                 ("GET", "/v1/models/"), ("GET", "//v1/models"), ("POST", "/unload"),
                 ("GET", "/v1/chat/completions"), ("POST", "/v1/chat/completions/")]:
        r[m + " " + p] = (await call(m, p, KEY))[0]
    r["ws"] = await call("GET", "/v1/chat/completions", KEY, kind="websocket")
    r["health"] = await call("GET", "/health")
    r["no key"] = (await call("GET", "/v1/models"))[0]
    r["wrong key"] = (await call("GET", "/v1/models", [("Authorization", "Bearer x")]))[0]
    r["models"] = await call("GET", "/v1/models", KEY)
    big = b"x" * (1024 * 1024)
    r["big"] = (await call("POST", "/v1/chat/completions", KEY, [big] * 65))[0]
    r["bad"] = (await call("POST", "/v1/chat/completions", KEY,
                           [b'{"messages":[{"role":"user","content":[{"type":"input_audio",',
                            b'"input_audio":{"data":"/etc/hosts","format":"wav"}}]}]}']))[0]
    good = b'{"messages":[{"role":"user","content":"hi"}]}'
    r["good"] = await call("POST", "/v1/chat/completions", KEY, [good[:10], good[10:]])
    audio = b'{"messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"data":"AAAA"}}]}]}'
    r["audio"] = (await call("POST", "/v1/chat/completions", KEY + [("Content-Length", str(len(audio)))], [audio]))[0]
    return r

r = asyncio.run(main())
for k, v in r.items():
    print(k, "=", v)
print("seen", [(p, b.decode()[:120], cl) for p, b, cl in seen])
"#;
        let Some((ok, text)) = vision_worker_py(script) else {
            eprintln!("no system python, skipping");
            return;
        };
        assert!(ok, "{text}");
        for line in [
            "HEAD /v1/models = 404",
            "OPTIONS /v1/chat/completions = 404",
            "GET /v1/models/ = 404",
            "GET //v1/models = 404",
            "POST /unload = 404",
            "GET /v1/chat/completions = 404",
            "POST /v1/chat/completions/ = 404",
            "ws = ('websocket.close', 1008)",
            "health = (200, b'{\"status\": \"ok\"}')",
            "no key = 401",
            "wrong key = 401",
            "big = 413",
            "bad = 400",
            "good = (200, b'inner')",
            "audio = 200",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
        assert!(text.contains("\"id\": \"alias\""), "{text}");
        // The app saw the body whole, and the rewritten one with its new length.
        assert!(
            text.contains("('/v1/chat/completions', '{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}', b'45')"),
            "{text}"
        );
        assert!(text.contains("data:audio/wav;base64,AAAA"), "{text}");
        assert_eq!(text.matches("('/v1/chat/completions'").count(), 2, "{text}");
    }

    /// `mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit` (config trimmed) and its card.
    const TTS_CONFIG: &str = r#"{"architectures":["Qwen3TTSForConditionalGeneration"],"model_type":"qwen3_tts","quantization":{"group_size":64,"bits":8,"mode":"affine"},"talker_config":{"spk_id":{"serena":3066,"ryan":3061}}}"#;
    const TTS_CARD: &str =
        "---\nlicense: apache-2.0\nlibrary_name: mlx-audio\ntags:\n- mlx\n- text-to-speech\n---\n";

    #[test]
    fn speech_folders_are_mlx_audio_and_never_chat_models() {
        let tts = folder(&[
            ("config.json", TTS_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", TTS_CARD),
            ("speech_tokenizer/model.safetensors", "x"),
        ]);
        assert!(is_tts_dir(tts.path()));
        assert_eq!(tts_voices(tts.path()), ["ryan", "serena"]);
        assert!(tts_supports_speed(tts.path()), "Qwen3-TTS takes a speed");
        assert!(!is_model_dir(tts.path()) && !is_vlm_dir(tts.path()));
        assert!(!super::super::mistralrs::is_model_dir(tts.path()));
        assert_eq!(flavour_of(tts.path()), Some(Flavour::Audio));
        assert_eq!(family_of(tts.path()), Some(MLX_AUDIO));
        assert!(MlxEngine::AUDIO.runs(tts.path()));
        assert!(inspect(tts.path()).is_none());
        let m = to_local_model(tts.path(), tts.path().parent().unwrap()).unwrap();
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.quantization, "8bit");
        // Deleting the model takes its codec folder with it, and nothing beside it.
        let library = tempfile::tempdir().unwrap();
        let model = library
            .path()
            .join("mlx-community/Qwen3-TTS-12Hz-0.6B-Base-8bit");
        let other = library.path().join("mlx-community/other");
        for (dir, name) in [
            (&model, "config.json"),
            (&model, "model.safetensors"),
            (&model, "README.md"),
            (&model, "speech_tokenizer/config.json"),
            (&model, "speech_tokenizer/model.safetensors"),
            (&other, "config.json"),
        ] {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let body = match name {
                "config.json" => TTS_CONFIG,
                "README.md" => TTS_CARD,
                _ => "x",
            };
            std::fs::write(path, body).unwrap();
        }
        delete_model_dir(&model, &[library.path().to_path_buf()]).unwrap();
        assert!(!model.exists() && !model.join("speech_tokenizer").exists());
        assert!(other.join("config.json").is_file());

        // A whisper conversion (an MLX card, a transformers-like config) is a speech model:
        // not a chat model; mlx-audio recognizes with it (see the next test).
        let whisper = folder(&[
            (
                "config.json",
                r#"{"architectures":["WhisperForConditionalGeneration"],"model_type":"whisper"}"#,
            ),
            ("model.safetensors", "x"),
            ("README.md", "---\nlibrary_name: mlx\ntags:\n- mlx\n---\n"),
        ]);
        assert!(is_audio_folder(whisper.path()));
        assert!(!is_model_dir(whisper.path()) && !is_vlm_dir(whisper.path()));
        assert!(!super::super::mistralrs::is_model_dir(whisper.path()));
        // Kokoro (an untested architecture) is not offered.
        let kokoro = folder(&[
            ("config.json", r#"{"model_type":"kokoro","istftnet":{}}"#),
            ("kokoro-v1_0.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: mlx\npipeline_tag: text-to-speech\n---\n",
            ),
        ]);
        assert_eq!(flavour_of(kokoro.path()), None);
        // A chat model whose card names a speech task is not a chat model either.
        let tagged = folder(&[
            ("config.json", MLX_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", "---\npipeline_tag: text-to-speech\n---\n"),
        ]);
        assert_eq!(flavour_of(tagged.path()), None);
        assert!(is_tts_repo(
            "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-bf16",
            &[]
        ));
        assert!(!is_tts_repo("mlx-community/Kokoro-82M-bf16", &[]));
        for repo in [
            "mlx-community/chatterbox-turbo-4bit",
            "mlx-community/Soprano-80M-bf16",
            "mlx-community/VoxCPM2-4bit",
            "mlx-community/Spark-TTS-0.5B-bf16",
            "mlx-community/csm-1b-fp16",
        ] {
            assert!(is_tts_repo(repo, &[]), "{repo}");
        }
        for repo in ["mlx-community/chatterbox-fp16", "mlx-community/VoxCPM1.5"] {
            assert!(!is_tts_repo(repo, &[]), "{repo}");
        }
    }

    /// The text-to-speech families checked end to end beside Qwen3-TTS, as mlx-community ships
    /// them (configs trimmed): each is an mlx-audio folder, never a chat model. Spark-TTS's
    /// config is its Qwen2 language model's: its `BiCodec/` codec makes it a speech folder, also
    /// when quantized.
    #[test]
    fn mlx_audio_runs_the_tested_speech_families() {
        let card = |library: &str| {
            format!("---\nlibrary_name: {library}\npipeline_tag: text-to-speech\n---\n")
        };
        let chatterbox = folder(&[
            (
                "config.json",
                r#"{"model_type":"chatterbox_turbo","quantization":{"group_size":64,"bits":4}}"#,
            ),
            ("model.safetensors", "x"),
            ("README.md", &card("mlx-audio")),
        ]);
        let soprano = folder(&[
            (
                "config.json",
                r#"{"architectures":["SopranoForCausalLM"],"model_type":"soprano"}"#,
            ),
            ("model.safetensors", "x"),
            ("README.md", &card("mlx-audio")),
        ]);
        let voxcpm = folder(&[
            (
                "config.json",
                r#"{"model_type":"voxcpm2","quantization":{"group_size":64,"bits":4}}"#,
            ),
            ("model.safetensors", "x"),
            ("README.md", &card("mlx-audio")),
        ]);
        let spark_files = |quantized: bool| {
            let config = if quantized {
                r#"{"architectures":["Qwen2ForCausalLM"],"model_type":"qwen2","quantization":{"group_size":64,"bits":8}}"#
            } else {
                r#"{"architectures":["Qwen2ForCausalLM"],"model_type":"qwen2"}"#
            };
            folder(&[
                ("config.json", config),
                ("model.safetensors", "x"),
                ("tokenizer.json", "{}"),
                ("audio_tokenizer_config.yaml", "x: 1"),
                ("BiCodec/config.yaml", "x: 1"),
                ("BiCodec/model.safetensors", "x"),
                ("README.md", "---\nlicense: cc-by-nc-sa-4.0\ntags:\n- text-to-speech\n- mlx\nlibrary_tag: spark-tts\n---\n"),
            ])
        };
        let (spark, spark8) = (spark_files(false), spark_files(true));
        for dir in [&chatterbox, &soprano, &voxcpm, &spark, &spark8] {
            let path = dir.path();
            assert!(is_tts_dir(path), "{path:?}");
            assert!(
                !is_stt_dir(path) && !is_model_dir(path) && !is_vlm_dir(path),
                "{path:?}"
            );
            assert!(!super::super::mistralrs::is_model_dir(path), "{path:?}");
            assert!(super::super::python_hf::detect(path).is_none(), "{path:?}");
            assert_eq!(flavour_of(path), Some(Flavour::Audio), "{path:?}");
            assert_eq!(family_of(path), Some(MLX_AUDIO), "{path:?}");
            let m = to_local_model(path, path.parent().unwrap()).unwrap();
            assert_eq!(m.kind, crate::domain::ModelKind::Pipeline, "{path:?}");
        }
        assert_eq!(tts_type(spark.path()).as_deref(), Some("spark"));
        // Its codec's weights count in its size.
        let m = to_local_model(spark.path(), spark.path().parent().unwrap()).unwrap();
        assert_eq!(
            m.size_bytes, 2,
            "model.safetensors and BiCodec/model.safetensors"
        );
        // Without the MLX card or quantization it is not mlx-audio's, but still no chat model:
        // neither is the original release's `LLM/` subfolder (a plain Qwen2 config).
        std::fs::write(
            spark.path().join("README.md"),
            "---\nlicense: cc-by-nc-sa-4.0\n---\n",
        )
        .unwrap();
        let llm = spark.path().join("LLM");
        std::fs::create_dir(&llm).unwrap();
        std::fs::copy(spark.path().join("config.json"), llm.join("config.json")).unwrap();
        std::fs::write(llm.join("model.safetensors"), "x").unwrap();
        assert!(!is_tts_dir(spark.path()));
        for dir in [spark.path(), llm.as_path()] {
            assert!(is_spark_dir(dir), "{dir:?}");
            assert!(!super::super::mistralrs::is_model_dir(dir), "{dir:?}");
            assert_eq!(super::super::python_hf::detect(dir), None, "{dir:?}");
        }
        // Without its codec a Qwen2 folder is what it says.
        std::fs::remove_dir_all(spark.path().join("BiCodec")).unwrap();
        assert!(!is_tts_dir(spark.path()) && !is_spark_dir(spark.path()));
        assert!(super::super::mistralrs::is_model_dir(spark.path()));
    }

    /// The families that load a codec by name (OuteTTS, Orpheus, Dia), as mlx-community ships
    /// them (configs trimmed): mlx-audio folders whose launch names the codec's library folder,
    /// and a launch without the codec says which one is missing. A Llama chat folder stays a
    /// chat model; the Hub plan lists the codec's files with their sizes, found once for all
    /// the models that share it.
    #[test]
    fn codec_families_launch_with_their_codec() {
        let outetts = folder(&[
            ("config.json", r#"{"architectures":["Qwen3ForCausalLM"],"model_type":"qwen3","quantization":{"group_size":64,"bits":4}}"#),
            ("model.safetensors", "x"),
            ("tokenizer.json", "{}"),
            ("README.md", "---\nlicense: apache-2.0\npipeline_tag: text-to-speech\nlibrary_name: outetts\ntags:\n- mlx\n---\n"),
        ]);
        let orpheus = folder(&[
            ("config.json", r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama","quantization":{"group_size":64,"bits":4}}"#),
            ("model.safetensors", "x"),
            ("tokenizer.json", "{}"),
            ("README.md", "---\nlibrary_name: mlx\npipeline_tag: text-to-speech\nbase_model: canopylabs/orpheus-3b-0.1-ft\ntags:\n- mlx\n---\n"),
        ]);
        let dia = folder(&[
            ("config.json", r#"{"data":{"channels":9,"delay_pattern":[0,8,9]},"model":{"encoder":{},"decoder":{}},"quantization":{"group_size":64,"bits":4}}"#),
            ("model.safetensors", "x"),
            ("README.md", "---\nbase_model:\n- nari-labs/Dia-1.6B\npipeline_tag: text-to-speech\nlibrary_name: mlx\n---\n"),
        ]);
        let llama_chat = folder(&[
            (
                "config.json",
                r#"{"architectures":["LlamaForCausalLM"],"model_type":"llama","quantization":{"group_size":64,"bits":4}}"#,
            ),
            ("model.safetensors", "x"),
            ("tokenizer.json", "{}"),
            (
                "README.md",
                "---\nlibrary_name: mlx\npipeline_tag: text-generation\n---\n",
            ),
        ]);
        assert!(is_model_dir(llama_chat.path()) && !is_tts_dir(llama_chat.path()));
        let library = tempfile::tempdir().unwrap();
        let libraries = vec![library.path().to_path_buf()];
        for (dir, family, codec) in [
            (
                &outetts,
                "outetts",
                "mlx-community/dac-speech-24khz-1.5kbps",
            ),
            (&orpheus, "orpheus", "mlx-community/snac_24khz"),
            (&dia, "dia", "mlx-community/descript-audio-codec-44khz"),
        ] {
            let path = dir.path();
            assert_eq!(tts_type(path).as_deref(), Some(family));
            assert!(is_tts_dir(path), "{family}");
            assert!(
                !is_model_dir(path) && !super::super::mistralrs::is_model_dir(path),
                "{family}"
            );
            assert_eq!(flavour_of(path), Some(Flavour::Audio), "{family}");
            let e = companion_args(path, &libraries).unwrap_err();
            assert!(e.contains("mlx.codec_missing") && e.contains(codec), "{e}");
            // The codec downloaded into the library (`<library>/<owner>/<repo>`).
            let folder = library.path().join(codec);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("config.json"), "{}").unwrap();
            assert!(companion_args(path, &libraries).is_err(), "weights missing");
            std::fs::write(folder.join("model.safetensors"), "x").unwrap();
            assert_eq!(
                companion_args(path, &libraries).unwrap(),
                [
                    "--companion".to_string(),
                    format!("{codec}={}", folder.to_string_lossy())
                ]
            );
        }
        // Families that load nothing else launch without a companion.
        let soprano = folder(&[
            ("config.json", r#"{"model_type":"soprano"}"#),
            ("model.safetensors", "x"),
            ("README.md", "---\nlibrary_name: mlx-audio\n---\n"),
        ]);
        assert!(companion_args(soprano.path(), &[]).unwrap().is_empty());

        // The Hub: families by repo name, and the codec as a plan of files with sizes.
        assert_eq!(
            tts_repo_family("mlx-community/OuteTTS-1.0-0.6B-4bit"),
            Some("outetts")
        );
        assert_eq!(
            tts_repo_family("mlx-community/Llama-OuteTTS-1.0-1B-4bit"),
            Some("outetts")
        );
        assert_eq!(
            tts_repo_family("mlx-community/orpheus-3b-0.1-ft-4bit"),
            Some("orpheus")
        );
        assert_eq!(tts_repo_family("mlx-community/Dia-1.6B-4bit"), Some("dia"));
        assert_eq!(tts_repo_family("mlx-community/multimedia-model"), None);
        assert_eq!(tts_repo_family("mlx-community/csm-1b"), Some("sesame"));
        assert_eq!(tts_repo_family("mlx-community/csm-1b-fp16"), Some("sesame"));
        // The Hub's tags carry `model_type`: csm-1b-8bit's config says `sam`, which mlx-audio does
        // not load as CSM. Without tags (an old listing) the name decides.
        let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_tts_repo(
            "mlx-community/csm-1b-fp16",
            &tags(&["mlx", "sesame", "text-to-speech"])
        ));
        assert!(!is_tts_repo(
            "mlx-community/csm-1b-8bit",
            &tags(&["mlx", "sam", "text-to-speech"])
        ));
        assert!(is_tts_repo("mlx-community/csm-1b", &[]));
        // Other families keep going by name: Orpheus is tagged `llama`, its config's model_type.
        assert!(is_tts_repo(
            "mlx-community/orpheus-3b-0.1-ft-4bit",
            &tags(&["mlx", "llama", "4-bit"])
        ));
        let empty = tempfile::tempdir().unwrap();
        let libs = [empty.path().to_path_buf()];
        // Only MLX builds pull a codec: the original Dia, a GGUF Orpheus and an OuteTTS release
        // run elsewhere.
        for repo in [
            "nari-labs/Dia-1.6B",
            "unsloth/orpheus-3b-0.1-ft-GGUF",
            "canopylabs/orpheus-3b-0.1-ft",
            "OuteAI/OuteTTS-1.0-0.6B",
        ] {
            assert!(!is_mlx_repo_id(repo), "{repo}");
            assert!(codec_plan_for_repo(repo, &libs).is_none(), "{repo}");
        }
        assert!(is_mlx_repo_id("someone/orpheus-3b-mlx-4bit"));
        assert!(codec_plan_for_repo("someone/orpheus-3b-mlx-4bit", &libs).is_some());
        let plan = codec_plan_for_repo("mlx-community/Dia-1.6B-4bit", &libs).unwrap();
        assert_eq!(plan.bundle_id, "mlx-audio:dia");
        assert_eq!(plan.missing().len(), 2);
        assert_eq!(
            plan.components.iter().map(|c| c.size).sum::<u64>(),
            219 + 306_644_869
        );
        assert!(plan
            .components
            .iter()
            .all(|c| c.role == "codec" && c.repo == "mlx-community/descript-audio-codec-44khz"));
        // Downloaded for one model, it is there for every model of the family.
        let plan = codec_plan_for_repo("mlx-community/orpheus-3b-0.1-ft-bf16", &libraries).unwrap();
        assert!(plan.complete(), "{plan:?}");
        assert!(codec_plan_for_repo("mlx-community/Soprano-80M-bf16", &libraries).is_none());
        assert!(codec_plan_for_repo("mlx-community/whisper-tiny-asr-fp16", &libraries).is_none());
        // mlx-examples' Whisper conversions: the processor of their tokenizer, by name.
        for (repo, processor) in [
            (
                "mlx-community/whisper-large-v3-turbo",
                "openai/whisper-large-v3",
            ),
            (
                "mlx-community/whisper-large-v3-mlx",
                "openai/whisper-large-v3",
            ),
            ("mlx-community/whisper-small-mlx", "openai/whisper-small"),
            (
                "mlx-community/whisper-tiny.en-8bit",
                "openai/whisper-tiny.en",
            ),
        ] {
            let plan = codec_plan_for_repo(repo, &libraries).unwrap();
            assert_eq!(plan.components.len(), 8, "{repo}");
            assert!(
                plan.components.iter().all(|c| c.repo == processor),
                "{repo}"
            );
            let (file, folder) = pinned_codec_file(processor, "tokenizer.json").unwrap();
            assert_eq!(folder, format!("{processor}-processor"));
            assert_eq!(file.repo, processor);
        }
        // Only OpenAI's size names are mapped: a fine-tune may have another vocabulary.
        for repo in [
            "mlx-community/kotoba-whisper-v2.0",
            "someone/whisper-custom-mlx",
            "mlx-community/whisperx",
        ] {
            assert!(whisper_processor_for_repo(repo).is_none(), "{repo}");
            assert!(codec_plan_for_repo(repo, &libraries).is_none(), "{repo}");
        }
        for (repo, processor) in [
            (
                "mlx-community/distil-whisper-large-v3",
                "openai/whisper-large-v3",
            ),
            ("mlx-community/whisper-turbo", "openai/whisper-large-v3"),
            ("mlx-community/whisper-large-v2-mlx", "openai/whisper-small"),
            (
                "mlx-community/whisper-medium.en-mlx",
                "openai/whisper-tiny.en",
            ),
            ("mlx-community/whisper-tiny", "openai/whisper-small"),
        ] {
            assert_eq!(
                whisper_processor_for_repo(repo).map(|c| c.repo),
                Some(processor),
                "{repo}"
            );
        }
        // The processor of each vocabulary.
        for (vocab, processor) in [
            (51866, "openai/whisper-large-v3"),
            (51865, "openai/whisper-small"),
            (51864, "openai/whisper-tiny.en"),
        ] {
            assert_eq!(
                whisper_processor_for_vocab(vocab).map(|c| c.repo),
                Some(processor)
            );
        }
        assert!(whisper_processor_for_vocab(51867).is_none());
        // The Hub: a tiktoken vocabulary counts at the root only, and an unknown name is not
        // offered without a processor of its own.
        let files = |f: &[&str]| f.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let quantized = files(&["config.json", "model.safetensors", "multilingual.tiktoken"]);
        assert!(stt_repo_files_ok(
            "mlx-community/whisper-large-v3-turbo-4bit",
            &[],
            &quantized
        ));
        assert!(!stt_repo_files_ok(
            "mlx-community/whisper-large-v3-turbo-4bit",
            &[],
            &files(&[
                "config.json",
                "model.safetensors",
                "sub/multilingual.tiktoken"
            ])
        ));
        assert!(!stt_repo_files_ok(
            "mlx-community/kotoba-whisper-v2.0",
            &[],
            &quantized
        ));
        assert!(stt_repo_files_ok(
            "mlx-community/kotoba-whisper-v2.0",
            &[],
            &files(&[
                "config.json",
                "model.safetensors",
                "preprocessor_config.json",
                "tokenizer.json"
            ])
        ));
    }

    /// CSM (`sesame`, `mlx-community/csm-1b-fp16`): a speech folder whose Mimi codec, Llama 3.2
    /// tokenizer and two voice prompts come with it from pinned commits; its voices are those
    /// prompts. Every codec file is pinned (40-hex commit, SHA-256, size as the plan lists it).
    #[test]
    fn csm_brings_mimi_its_tokenizer_and_voice_prompts() {
        let csm = folder(&[
            (
                "config.json",
                r#"{"model_type":"sesame","backbone_flavor":"llama-1B","decoder_flavor":"llama-100M","text_vocab_size":128256,"audio_vocab_size":2051,"audio_num_codebooks":32}"#,
            ),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\npipeline_tag: text-to-speech\nlibrary_name: mlx\n---\n",
            ),
        ]);
        let path = csm.path();
        assert_eq!(tts_type(path).as_deref(), Some("sesame"));
        assert!(is_tts_dir(path));
        assert!(!is_model_dir(path) && !super::super::mistralrs::is_model_dir(path));
        assert_eq!(family_of(path), Some(MLX_AUDIO));
        assert_eq!(tts_voices(path), ["conversational_a", "conversational_b"]);
        // CSM speaks at one speed (the panel hides its slider).
        assert!(!tts_supports_speed(path));
        let library = tempfile::tempdir().unwrap();
        let libraries = vec![library.path().to_path_buf()];
        let e = companion_args(path, &libraries).unwrap_err();
        assert!(e.contains("kyutai/moshiko-pytorch-bf16"), "{e}");

        let plan = codec_plan_for_repo("mlx-community/csm-1b-fp16", &libraries).unwrap();
        assert_eq!(plan.bundle_id, "mlx-audio:sesame");
        let repos: std::collections::BTreeSet<_> =
            plan.components.iter().map(|c| c.repo.as_str()).collect();
        assert_eq!(
            repos.into_iter().collect::<Vec<_>>(),
            [
                "kyutai/moshiko-pytorch-bf16",
                "mlx-community/csm-1b",
                "unsloth/Llama-3.2-1B"
            ]
        );
        // Never Moshi's 15 GB model: the codec file, the tokenizer and the prompts (~410 MB).
        let total: u64 = plan.components.iter().map(|c| c.size).sum();
        assert!(total < 420_000_000, "{total}");
        for c in &plan.components {
            let (pin, _) = pinned_codec_file(&c.repo, &c.file).expect("pinned");
            assert_eq!((pin.size, pin.filename), (c.size, c.file.as_str()));
            assert!(pin.rev.len() == 40 && pin.rev.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(pin.sha256.len() == 64 && pin.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        }
        // The older codecs are pinned as well; anything else is not a codec file.
        for (repo, file) in [
            ("mlx-community/snac_24khz", "model.safetensors"),
            ("mlx-community/dac-speech-24khz-1.5kbps", "config.json"),
            (
                "mlx-community/descript-audio-codec-44khz",
                "model.safetensors",
            ),
        ] {
            assert!(pinned_codec_file(repo, file).is_some(), "{repo}");
        }
        assert!(pinned_codec_file("kyutai/moshiko-pytorch-bf16", "model.safetensors").is_none());
        assert!(pinned_codec_file("mlx-community/csm-1b", "model.safetensors").is_none());

        // The tokenizer and the prompts come from repos that are models too: they get folders of
        // their own, so deleting such a model (`harness rm mlx-community/csm-1b`) keeps them.
        let folder_of = |repo: &str, file: &str| pinned_codec_file(repo, file).unwrap().1;
        assert_eq!(
            folder_of("mlx-community/csm-1b", "prompts/conversational_a.wav"),
            "mlx-community/csm-1b-voice-prompts"
        );
        assert_eq!(
            folder_of("unsloth/Llama-3.2-1B", "tokenizer.json"),
            "unsloth/Llama-3.2-1B-tokenizer"
        );
        // A CSM model in the repo's own folder is not where its prompts are looked for.
        for c in &plan.components {
            let file = library.path().join(&c.repo).join(&c.file);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "x").unwrap();
        }
        assert!(companion_args(path, &libraries).is_err());

        // Downloaded into their folders, they are the worker's companions.
        for c in &plan.components {
            let file = library
                .path()
                .join(folder_of(&c.repo, &c.file))
                .join(&c.file);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "x").unwrap();
        }
        let args = companion_args(path, &libraries).unwrap();
        assert_eq!(
            args.iter().filter(|a| *a == "--companion").count(),
            3,
            "{args:?}"
        );
        let prompts = library.path().join("mlx-community/csm-1b-voice-prompts");
        assert!(
            args.contains(&format!(
                "mlx-community/csm-1b={}",
                prompts.to_string_lossy()
            )),
            "{args:?}"
        );
    }

    /// The worker's `CSM_VOICES` (what it accepts) and Rust's (what the panel lists) are one list,
    /// and each voice is a pinned prompt file.
    #[test]
    fn csm_voices_match_the_worker() {
        let line = AUDIO_WORKER
            .lines()
            .find(|l| l.starts_with("CSM_VOICES = "))
            .expect("CSM_VOICES in the worker");
        let worker: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
        assert_eq!(worker, CSM_VOICES);
        for v in CSM_VOICES {
            let file = format!("prompts/{v}.wav");
            assert!(
                pinned_codec_file("mlx-community/csm-1b", &file).is_some(),
                "{file}"
            );
        }
    }

    /// Speech recognition folders in MLX's format (mlx-audio's Whisper port with a transformers
    /// config, mlx-whisper's own config, NeMo's Parakeet config) are mlx-audio's, launched for
    /// transcription; the same folders without an MLX card stay with the Python engine, and
    /// nothing else of mlx-audio's recognizers is offered.
    #[test]
    fn mlx_recognition_folders_are_mlx_audio_transcription_models() {
        let hf_whisper = r#"{"architectures":["WhisperForConditionalGeneration"],"model_type":"whisper","d_model":384}"#;
        let ported = folder(&[
            ("config.json", hf_whisper),
            ("model.safetensors", "x"),
            ("preprocessor_config.json", "{}"),
            ("tokenizer.json", "{}"),
            (
                "README.md",
                "---\npipeline_tag: automatic-speech-recognition\nlibrary_name: mlx-audio\n---\n",
            ),
        ]);
        let parakeet = folder(&[
            (
                "config.json",
                r#"{"sample_rate":16000,"encoder":{},"decoder":{},"target":"nemo.collections.asr.models.rnnt_bpe_models.EncDecRNNTBPEModel"}"#,
            ),
            ("model.safetensors", "x"),
            ("tokenizer.model", "x"),
            ("README.md", "---\nlibrary_name: mlx\n---\n"),
        ]);
        // mlx-audio's own conversion of large-v3-turbo: no library_name, only the mlx tag.
        let tagged = folder(&[
            ("config.json", hf_whisper),
            ("model.safetensors", "x"),
            ("preprocessor_config.json", "{}"),
            ("vocab.json", "{}"),
            (
                "README.md",
                "---
tags:
- mlx
- speech-to-text
- stt
---
# x
",
            ),
        ]);
        for dir in [&ported, &parakeet, &tagged] {
            let path = dir.path();
            assert!(is_stt_dir(path), "{}", path.display());
            assert!(!is_tts_dir(path));
            assert_eq!(flavour_of(path), Some(Flavour::Audio));
            assert_eq!(family_of(path), Some(MLX_AUDIO));
            assert!(MlxEngine::AUDIO.runs(path) && is_any_model_dir(path));
            assert!(!is_model_dir(path) && !is_vlm_dir(path));
            assert!(!super::super::mistralrs::is_model_dir(path));
            assert_eq!(super::super::python_hf::detect(path), None);
            let m = to_local_model(path, path.parent().unwrap()).unwrap();
            assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
            assert_eq!(m.task.as_deref(), Some("automatic-speech-recognition"));
            assert!(m.is_ready && m.size_bytes == 1);
            assert_eq!(
                super::super::server_for_model(path).map(|e| e.family()),
                Some(MLX_AUDIO)
            );
        }
        assert_eq!(
            to_local_model(ported.path(), ported.path())
                .unwrap()
                .architecture
                .as_deref(),
            Some("whisper")
        );
        // mlx-examples' older layout (`weights.safetensors` or `weights.npz`, a short config, no
        // processor): mlx-audio runs it with the processor of its vocabulary from the library.
        let turbo = folder(&[
            (
                "config.json",
                r#"{"n_mels":128,"n_vocab":51866,"n_audio_ctx":1500,"n_text_layer":4,"model_type":"whisper"}"#,
            ),
            ("weights.safetensors", "x"),
            (
                "README.md",
                "---\nlibrary_name: mlx\npipeline_tag: automatic-speech-recognition\n---\n",
            ),
        ]);
        assert!(is_stt_dir(turbo.path()));
        let library = tempfile::tempdir().unwrap();
        let libraries = vec![library.path().to_path_buf()];
        let e = companion_args(turbo.path(), &libraries).unwrap_err();
        assert!(e.contains("openai/whisper-large-v3"), "{e}");
        let processor = library.path().join("openai/whisper-large-v3-processor");
        std::fs::create_dir_all(&processor).unwrap();
        for (f, _, _) in WHISPER_LARGE_V3_PROCESSOR.files {
            std::fs::write(processor.join(f), "{}").unwrap();
        }
        assert_eq!(
            companion_args(turbo.path(), &libraries).unwrap(),
            [
                "--companion".to_string(),
                format!("openai/whisper-large-v3={}", processor.to_string_lossy())
            ]
        );
        // A folder with its own processor needs none; an unknown vocabulary is not offered.
        assert!(companion_args(ported.path(), &libraries)
            .unwrap()
            .is_empty());
        let odd = folder(&[
            (
                "config.json",
                r#"{"n_mels":80,"n_vocab":12,"model_type":"whisper"}"#,
            ),
            ("weights.safetensors", "x"),
        ]);
        assert!(!is_stt_dir(odd.path()));
        assert!(has_whisper_processor(
            |n| n == "preprocessor_config.json" || n == "vocab.json"
        ));
        assert!(!has_whisper_processor(|n| n == "tokenizer.json"));
        assert_eq!(
            to_local_model(parakeet.path(), parakeet.path())
                .unwrap()
                .architecture
                .as_deref(),
            Some("parakeet")
        );

        // A transformers Whisper checkpoint (no MLX card) is the Python engine's, whatever its
        // other tags; a tag outside the front matter does not count.
        let hf = folder(&[
            ("config.json", hf_whisper),
            ("model.safetensors", "x"),
            (
                "README.md",
                "---\npipeline_tag: automatic-speech-recognition\ntags:\n- audio\n---\ntags:\n- mlx\n",
            ),
        ]);
        assert!(!is_stt_dir(hf.path()));
        assert_eq!(flavour_of(hf.path()), None);
        assert_eq!(
            super::super::python_hf::detect(hf.path()).as_deref(),
            Some("automatic-speech-recognition")
        );
        // Old mlx-whisper weights (`weights.npz`) are mlx-audio's, with a multilingual or an
        // English-only processor by the vocabulary; untested recognizers are not offered.
        for (vocab, repo) in [
            (51865, "openai/whisper-small"),
            (51864, "openai/whisper-tiny.en"),
        ] {
            let npz = folder(&[
                (
                    "config.json",
                    &format!(r#"{{"n_mels":80,"n_vocab":{vocab},"model_type":"whisper"}}"#),
                ),
                ("weights.npz", "x"),
            ]);
            assert!(is_stt_dir(npz.path()), "{vocab}");
            assert_eq!(audio_dir_info(npz.path()).weights.len(), 1);
            // Safetensors beside the npz are the weights, the npz is not counted twice.
            std::fs::write(npz.path().join("weights.safetensors"), "x").unwrap();
            let weights = audio_dir_info(npz.path()).weights;
            assert_eq!(weights.len(), 1);
            assert!(weights[0].ends_with("weights.safetensors"));
            std::fs::remove_file(npz.path().join("weights.safetensors")).unwrap();
            let e = companion_args(npz.path(), &[]).unwrap_err();
            assert!(e.contains(repo), "{e}");
        }
        let moonshine = folder(&[
            ("config.json", r#"{"model_type":"moonshine"}"#),
            ("model.safetensors", "x"),
            ("README.md", "---\nlibrary_name: mlx-audio\n---\n"),
        ]);
        assert!(!is_stt_dir(moonshine.path()));
        assert_eq!(flavour_of(moonshine.path()), None);

        // Launched for transcription; a custom line cannot change the task.
        let mut config = LaunchConfig {
            model_path: ported.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11453,
            ..Default::default()
        };
        config
            .params
            .insert("max_seconds".into(), crate::params::ParamValue::Int(300));
        let script = Path::new("/env/mlx_audio_worker.py");
        let args = build_args(&config, script, Flavour::Audio).unwrap().args;
        let args = args.join(" ");
        assert!(args.contains(" --task transcription"), "{args}");
        assert!(args.contains("--max-seconds 300"), "{args}");
        assert!(!args.contains("--max-tokens"), "{args}");
        config.custom_args = Some("--task speech".into());
        let e = build_args(&config, script, Flavour::Audio).unwrap_err();
        assert!(e.contains("params.protected_flag"), "{e}");
        // A text-to-speech folder is launched without a task (the worker speaks by default).
        let tts = folder(&[
            ("config.json", TTS_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", TTS_CARD),
        ]);
        config.model_path = tts.path().to_path_buf();
        config.custom_args = None;
        let args = build_args(&config, script, Flavour::Audio).unwrap().args;
        assert!(!args.iter().any(|a| a == "--task"), "{args:?}");
    }

    #[test]
    fn audio_launch_args_have_no_answer_limit() {
        let dir = folder(&[
            ("config.json", TTS_CONFIG),
            ("model.safetensors", "x"),
            ("README.md", TTS_CARD),
        ]);
        let mut config = LaunchConfig {
            model_path: dir.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11452,
            ..Default::default()
        };
        config
            .params
            .insert("max_chars".into(), crate::params::ParamValue::Int(900));
        config
            .params
            .insert("max_seconds".into(), crate::params::ParamValue::Int(45));
        let out = build_args(
            &config,
            Path::new("/env/mlx_audio_worker.py"),
            Flavour::Audio,
        )
        .unwrap();
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        let args = out.args.join(" ");
        assert!(
            args.starts_with("/env/mlx_audio_worker.py --model "),
            "{args}"
        );
        assert!(args.contains("--max-chars 900"), "{args}");
        assert!(args.contains("--max-seconds 45"), "{args}");
        assert!(!args.contains("--max-tokens"), "{args}");
        assert_eq!(MlxEngine::AUDIO.family(), MLX_AUDIO);
        assert_eq!(MlxEngine::AUDIO.env(), VLM_ENV);
        let e = MlxEngine::TEXT
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap_err();
        assert!(e.contains("mlx.model_unsupported"), "{e}");
    }

    /// The audio worker's checks and routes around a stand-in speaker (system Python, no MLX).
    #[test]
    fn the_audio_worker_speaks_only_checked_requests() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, asyncio; sys.path.insert(0, {:?}); import mlx_audio_worker as w
spoken = []
def speak(text, voice, speed, language):
    spoken.append((text, voice, speed, language))
    if text == "slow":
        raise w.TooSlow()
    return b"RIFFfake", {{"long run": "time", "drifted": "frames"}}.get(text)
app = w.app_for(speak, "tts", ["serena", "ryan"], 50)
async def call(method, path, body=b"", key=True, kind="http"):
    sent, queue = [], [{{"type": "http.request", "body": body, "more_body": False}}]
    async def receive():
        return queue.pop(0) if queue else {{"type": "http.disconnect"}}
    async def send(m):
        sent.append(m)
    headers = [(b"authorization", b"Bearer k")] if key else []
    await app({{"type": kind, "method": method, "path": path, "headers": headers}}, receive, send)
    if kind == "websocket":
        return sent[0].get("code")
    status = sent[0]["status"]
    cut = dict(sent[0]["headers"]).get(b"x-harness-truncated")
    if cut:
        status = (status, cut.decode())
    return status, b"".join(m.get("body", b"") for m in sent[1:]).decode(errors="replace")
async def main():
    S = "/v1/audio/speech"
    for name, args in [
        ("health", ("GET", "/health", b"", False)),
        ("nokey", ("GET", "/v1/models", b"", False)),
        ("models", ("GET", "/v1/models")),
        ("stt", ("POST", "/v1/audio/transcriptions")),
        ("voices", ("GET", "/v1/audio/voices")),
        ("slash", ("POST", S + "/")),
        ("ok", ("POST", S, b'{{"input":"hello","voice":"ryan","speed":1.5}}')),
        ("openai", ("POST", S, b'{{"input":"hello","voice":"alloy","model":"tts"}}')),
        ("ref", ("POST", S, b'{{"input":"x","ref_audio":"/etc/hosts"}}')),
        ("path", ("POST", S, b'{{"input":"x","voice":"../etc/passwd"}}')),
        ("unknown", ("POST", S, b'{{"input":"x","voice":"bob"}}')),
        ("long", ("POST", S, b'{{"input":"' + b"a" * 60 + b'"}}')),
        ("speed", ("POST", S, b'{{"input":"x","speed":9}}')),
        ("mp3", ("POST", S, b'{{"input":"x","response_format":"mp3"}}')),
        ("empty", ("POST", S, b'{{"input":"  "}}')),
        ("slow", ("POST", S, b'{{"input":"slow"}}')),
        ("cut", ("POST", S, b'{{"input":"long run"}}')),
        ("drifted", ("POST", S, b'{{"input":"drifted"}}')),
    ]:
        print(name, await call(*args))
    print("ws", await call("GET", S, kind="websocket"))
    print("spoken", spoken)
asyncio.run(main())
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "health (200, '{\"status\": \"ok\"}')",
            "nokey (401,",
            "models (200, '{\"object\": \"list\", \"data\": [{\"id\": \"tts\"",
            "\"voices\": [\"serena\", \"ryan\"]",
            "stt (404,",
            "voices (404,",
            "slash (404,",
            "ok (200, 'RIFFfake')",
            "openai (200, 'RIFFfake')",
            "ref (400,",
            "path (400,",
            "unknown (400,",
            "long (400,",
            "speed (400,",
            "mp3 (400,",
            "empty (400,",
            "slow (504,",
            "cut ((200, 'time'), 'RIFFfake')",
            "drifted ((200, 'frames'), 'RIFFfake')",
            "ws 1008",
            "spoken [('hello', 'ryan', 1.5, None), ('hello', None, 1.0, None), ('slow', None, 1.0, None), ('long run', None, 1.0, None), ('drifted', None, 1.0, None)]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// Python that calls an ASGI worker `app` as a browser would (`report(name, app, method,
    /// path)` prints one line per case: status, Access-Control-Allow-Origin, -Allow-Headers).
    /// The workers run with `HARNESS_WORKER_ORIGINS=tauri://localhost` and key `k`.
    pub(crate) const CORS_DRIVER: &str = r#"
import asyncio
async def _call(app, method, path, origin=None, key=True, kind="http"):
    body = b'{"messages": [{"role": "user", "content": "hi"}]}'
    first = {"type": "websocket.connect"} if kind == "websocket" else {
        "type": "http.request", "body": body, "more_body": False}
    sent, queue = [], [first]
    async def receive():
        return queue.pop(0) if queue else {"type": "http.disconnect"}
    async def send(m):
        sent.append(m)
    headers = [(b"authorization", b"Bearer k")] if key else []
    if origin:
        headers.append((b"origin", origin.encode()))
    await app({"type": kind, "method": method, "path": path, "headers": headers}, receive, send)
    if kind == "websocket":
        return sent[0]["type"], sent[0].get("code")
    h = dict(sent[0].get("headers") or [])
    return (sent[0]["status"], h.get(b"access-control-allow-origin", b"-").decode(),
            h.get(b"access-control-allow-headers", b"-").decode())
def report(name, app, method, path):
    T, E = "tauri://localhost", "https://evil.example"
    for label, args in [("preflight", ("OPTIONS", T, False)), ("preflight foreign", ("OPTIONS", E, False)),
                        ("foreign", (method, E)), ("foreign nokey", (method, E, False)),
                        ("allowed nokey", (method, T, False)), ("allowed", (method, T)),
                        ("plain", (method, None)), ("options plain", ("OPTIONS", None, False)),
                        # Browsers send "null", lowercase and no slash: anything else is foreign.
                        ("null", (method, "null")), ("upper", (method, "TAURI://LOCALHOST")),
                        ("slash", (method, T + "/")),
                        ("ws foreign", ("GET", E, True, "websocket"))]:
        print(name, label, asyncio.run(_call(app, args[0], path, *args[1:])))
"#;

    /// What [`CORS_DRIVER`] prints for a worker that answers `path` with 200 (`plain` is the
    /// header a request without Origin gets: the inner app's own, unchanged; `options` the
    /// status of an OPTIONS without Origin, also the worker's own answer).
    pub(crate) fn cors_expected(name: &str, plain: &str, options: u16) -> Vec<String> {
        [
            "preflight (204, 'tauri://localhost', 'authorization, content-type')",
            "preflight foreign (403, '-', '-')",
            "foreign (403, '-', '-')",
            "foreign nokey (403, '-', '-')",
            "allowed nokey (401, 'tauri://localhost', '-')",
            "allowed (200, 'tauri://localhost', '-')",
            "null (403, '-', '-')",
            "upper (403, '-', '-')",
            "slash (403, '-', '-')",
            "ws foreign ('websocket.close', 1008)",
        ]
        .iter()
        .map(|l| format!("{name} {l}"))
        .chain([
            format!("{name} plain (200, '{plain}', '-')"),
            format!("{name} options plain ({options}, "),
        ])
        .collect()
    }

    /// Browsers: the vision and speech workers answer the Harness window's preflight (no key
    /// needed), refuse every other Origin with 403 before the key, keep the key for real
    /// requests, and replace the inner app's own CORS headers.
    #[test]
    fn the_vision_and_audio_workers_answer_only_harness_origins() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_vlm_worker.py"), VLM_WORKER).unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import os, sys; sys.path.insert(0, {:?})
env = dict(os.environ)
import mlx_vlm_worker as v
os.environ.update(env)  # each worker takes its key and origins out of the environment
import mlx_audio_worker as a
{CORS_DRIVER}
async def inner(scope, receive, send):
    await send({{"type": "http.response.start", "status": 200,
                "headers": [(b"access-control-allow-origin", b"*")]}})
    await send({{"type": "http.response.body", "body": b"inner"}})
report("vlm", v.cors(v.guard(inner, "alias")), "POST", "/v1/chat/completions")
report("audio", a.cors(a.app_for(lambda *x: b"RIFF", "tts", [], 50)), "GET", "/v1/models")
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .env("HARNESS_WORKER_ORIGINS", "tauri://localhost")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in cors_expected("vlm", "*", 404)
            .into_iter()
            .chain(cors_expected("audio", "-", 404))
        {
            assert!(text.contains(&line), "{line} missing:\n{text}");
        }
    }

    /// The text worker (mlx-lm's http.server handler, here over a stand-in mlx_lm) follows the
    /// same browser rules over real HTTP.
    #[test]
    fn the_text_worker_answers_only_harness_origins() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_worker.py"), WORKER).unwrap();
        let stub = dir.path().join("mlx_lm");
        std::fs::create_dir_all(&stub).unwrap();
        std::fs::write(stub.join("__init__.py"), "").unwrap();
        // Just enough of mlx_lm.server for the worker: its handler answers 200 with "*" CORS
        // headers (as mlx-lm does), and the server is handed to the test instead of run.
        std::fs::write(
            stub.join("server.py"),
            r#"
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import types
HANDLER = []
class APIHandler(BaseHTTPRequestHandler):
    created = 0
    def _set_cors_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
    def _set_completion_headers(self, status_code=200):
        self.send_response(status_code)
        self.send_header("Content-type", "application/json")
        self._set_cors_headers()
    def _ok(self):
        self._set_completion_headers(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")
    def do_GET(self):
        self._ok()
    def do_POST(self):
        self._ok()
    def do_OPTIONS(self):
        self._set_completion_headers(204)
        self.end_headers()
class ModelProvider:
    def __init__(self, cli_args):
        self.cli_args = cli_args
    def load(self, *a):
        pass
CACHES = []
class LRUPromptCache:
    def __init__(self, size, max_bytes=1 << 63):
        CACHES.append((size, max_bytes))
class ResponseGenerator:
    def __init__(self, *a):
        pass
mx = types.SimpleNamespace(distributed=types.SimpleNamespace(
    init=lambda: types.SimpleNamespace(rank=lambda: 0)))
def _run_http_server(host, port, generator, server_class, handler):
    HANDLER.append(handler)
def main():
    run(None, 0, ModelProvider(types.SimpleNamespace(prompt_cache_size=1, prompt_cache_bytes=12345)))
"#,
        )
        .unwrap();
        let script = format!(
            r#"
import sys, threading, http.client; sys.path.insert(0, {:?})
import mlx_worker as w
from mlx_lm import server
w.main(["--model", "m", "--port", "0"])
# The prompt cache gets its byte limit on every path (mlx-lm's own run gives it none).
assert server.CACHES == [(1, 12345)], server.CACHES
httpd = server.ThreadingHTTPServer(("127.0.0.1", 0), server.HANDLER[0])
threading.Thread(target=httpd.serve_forever, daemon=True).start()
def call(method, path, origin=None, key=True, upgrade=False):
    c = http.client.HTTPConnection("127.0.0.1", httpd.server_port)
    h = {{"Authorization": "Bearer k"}} if key else {{}}
    if upgrade:
        h.update({{"Upgrade": "websocket", "Connection": "Upgrade"}})
    if origin:
        h["Origin"] = origin
    c.request(method, path, body=b"{{}}" if method == "POST" else None, headers=h)
    r = c.getresponse()
    r.read()
    return (r.status, r.getheader("Access-Control-Allow-Origin", "-"),
            r.getheader("Access-Control-Allow-Headers", "-"))
T, E = "tauri://localhost", "https://evil.example"
for label, args in [("preflight", ("OPTIONS", T, False)), ("preflight foreign", ("OPTIONS", E, False)),
                    ("foreign", ("POST", E)), ("foreign nokey", ("POST", E, False)),
                    ("allowed nokey", ("POST", T, False)), ("allowed", ("POST", T)),
                    ("plain", ("POST", None)), ("options plain", ("OPTIONS", None, False)),
                    ("null", ("POST", "null")), ("upper", ("POST", "TAURI://LOCALHOST")),
                    ("slash", ("POST", T + "/")), ("ws foreign", ("GET", E, True, True))]:
    print("text", label, call(args[0], "/v1/chat/completions", *args[1:]))
print("text health foreign", call("GET", "/health", E, False))
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .env(
                "HARNESS_WORKER_ORIGINS",
                "tauri://localhost,http://a.example",
            )
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        // mlx-lm has no websockets: an upgrade from a foreign page is a GET, refused the same.
        for line in cors_expected("text", "-", 204)
            .into_iter()
            .filter(|l| !l.contains("ws foreign"))
            .chain([
                "text health foreign (403, '-', '-')".to_string(),
                "text ws foreign (403, '-', '-')".to_string(),
            ])
        {
            assert!(text.contains(&line), "{line} missing:\n{text}");
        }
    }

    /// A speech request cannot run away: `max_tokens` grows with the text within a floor and a
    /// ceiling, generation streams and stops at the time limit with the audio made so far, and
    /// nothing made by then is `TooSlow` (504). A model without `stream` / `max_tokens` is called
    /// without them.
    #[test]
    fn the_audio_worker_caps_frames_and_time() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, types; sys.path.insert(0, {:?}); import mlx_audio_worker as w
print("budget short", w.frame_budget(8, 1.0))
print("budget long", w.frame_budget(100, 1.0))
print("budget slow", w.frame_budget(80, 0.5))
print("budget cap", w.frame_budget(100000, 1.0))
now = [0.0]
clock = lambda: now[0]
class Talker:
    # Talks forever: one chunk per call, 4 s of (fake) clock each, until closed.
    tokenizer = types.SimpleNamespace(encode=lambda t: t.split())
    def __init__(self):
        self.calls, self.closed = [], False
    def generate(self, text, voice=None, speed=1.0, lang_code="auto", max_tokens=4096, stream=False):
        self.calls.append(dict(voice=voice, max_tokens=max_tokens, stream=stream))
        try:
            while True:
                now[0] += 4.0
                yield types.SimpleNamespace(audio=[0.0] * 10)
        finally:
            self.closed = True
encode = lambda parts, rate: "wav of %d chunks" % len(parts)
t = Talker()
speak = w.make_speak(t, ["ryan"], 24000, 10, encode, clock)
print("runaway", speak("one two three", None, 1.0, None), t.calls, "closed", t.closed)
class Silent:
    # Spends the whole time limit and makes nothing.
    def generate(self, text, max_tokens=4096, stream=False, **kw):
        now[0] += 30
        return iter(())
speak = w.make_speak(Silent(), [], 24000, 10, encode, clock)
try:
    speak("one", None, 1.0, None)
except w.TooSlow:
    print("too slow")
class Plain:
    def generate(self, text, voice=None, speed=1.0):
        yield types.SimpleNamespace(audio=[0.0])
speak = w.make_speak(Plain(), [], 24000, 10, encode, clock)
print("plain", speak("hi", None, 1.0, None))
# Other families count their own tokens per second of audio (Spark after its speaker tokens).
for family in ("chatterbox_turbo", "voxcpm2", "spark"):
    print("budget", family, w.frame_budget(100, 1.0, *w.TOKEN_RATES[family]))
class OneSpeed:
    def __init__(self):
        self.calls = []
    def generate(self, text, max_tokens=512, **kw):
        self.calls.append(dict(kw, max_tokens=max_tokens))
        yield types.SimpleNamespace(audio=[0.0])
one = OneSpeed()
speak = w.make_speak(one, [], 32000, 10, encode, clock, family="soprano")
print("one speed", speak("one two", None, 1.0, None), one.calls)
try:
    speak("one two", None, 1.5, None)
except w.Refused as e:
    print("refused speed:", e)
# A VoiceDesign model is given the voice description with every call.
one = OneSpeed()
speak = w.make_speak(one, [], 24000, 10, encode, clock, instruct=w.DESIGNED_VOICE)
speak("one two", None, 1.0, None)
print("designed", one.calls[0]["instruct"] == w.DESIGNED_VOICE)
# A model that cannot stream is called a sentence at a time; the time limit stops it between
# sentences (each takes 4 s of the 10 here).
class Whole:
    def __init__(self):
        self.said = []
    def generate(self, text, max_tokens=512, **kw):
        self.said.append(text)
        now[0] += 4.0
        yield types.SimpleNamespace(audio=[0.0])
now[0] = 0.0
whole = Whole()
speak = w.make_speak(whole, [], 48000, 10, encode, clock, family="voxcpm2")
print("sentences", speak("One. Two!  Three? Four.", None, 1.0, None), whole.said)
print("split", w.sentences("Hi there.  Bye。Yes"), w.sentences("   "))
# A period after a title, an abbreviation, a number (Turkish ordinals) or an initial does not end
# the sentence; an ellipsis does; a line break always does; a long run is cut at a space.
print("tr", w.sentences("Dr. Ali geldi. 3. madde önemli, vs. bitti… A. Yılmaz yazdı."))
print("lines", w.sentences("- süt\n- ekmek\n\n- yumurta"))
print("line ends", [w.sentences(t) for t in ("Madde 3.\nSonraki satır", "Bkz.\nAli", "Saat 10.\nGeldi.", "Hi Dr.\r\nAli")])
print("quote", w.sentences('He said "Go." Then left.'))
print("cap", [len(p) for p in w.sentences(" ".join(["kelime"] * 100), 120)])
# A streaming model speaks a short text in one call, a text longer than one call's frame ceiling
# (187 tokens at speed 1) in runs of whole sentences that each fit: none of it is dropped.
class Streamer:
    tokenizer = types.SimpleNamespace(encode=lambda t: t.split())
    def __init__(self):
        self.calls = []
    def generate(self, text, voice=None, speed=1.0, max_tokens=4096, stream=False, **kw):
        self.calls.append((text, max_tokens))
        yield types.SimpleNamespace(audio=[0.0])
now[0] = 0.0
s = Streamer()
speak = w.make_speak(s, [], 24000, 1000, encode, clock)
speak("Short one. Two.", None, 1.0, None)
print("streamed short", s.calls)
s.calls.clear()
story = " ".join("Sentence %d has exactly seven words here." % i for i in range(60))
print("streamed pauses", speak(story, None, 1.0, None))
said = " ".join(t for t, _ in s.calls)
print("streamed long", len(s.calls), max(m for _, m in s.calls) < w.frame_budget(10**6, 1.0),
      said == story)
s.calls.clear()
speak(story, None, 0.5, None)
print("streamed slow", len(s.calls))
# A sentence too long for one call at a slow speed (59 tokens; 46 fit at 0.25) is a run of its
# own with the ceiling as its budget.
s.calls.clear()
speak(" ".join(["word"] * 59) + ".", None, 0.25, None)
print("one long sentence", [(len(t.split()), m) for t, m in s.calls])
# Runs keep the text as it was: CJK sentences without spaces between them, an unpunctuated text
# (cut at spaces) with them.
chars = types.SimpleNamespace(tokenizer=types.SimpleNamespace(encode=list))
cjk = "今天天气很好。" * 80
groups = w.sentence_groups(chars, cjk, 1.0)
print("cjk", len(groups), "".join(groups) == cjk, max(len(g) for g in groups) <= 187)
print("mixed", w.sentence_groups(chars, "\u4eca\u5929\u5f88\u597d\u3002Hello there. \u518d\u89c1\u3002\u8c22\u8c22\u3002", 1.0))
plain = " ".join(["word"] * 400)
groups = w.sentence_groups(s, plain, 1.0)
print("unpunctuated", len(groups), " ".join(groups) == plain)
# A call that makes all the audio its budget allows (the sampler kept talking) is "frames".
class Drifter(Streamer):
    def generate(self, text, voice=None, speed=1.0, max_tokens=4096, stream=False, **kw):
        self.calls.append((text, max_tokens))
        yield types.SimpleNamespace(audio=[0.0] * int(max_tokens / w.FRAME_RATE * 24000))
speak = w.make_speak(Drifter(), [], 24000, 1000, lambda parts, rate: len(parts), clock)
print("drifted", speak("Short one. Two.", None, 1.0, None))
# CSM is capped by audio length, not tokens: the budget's length (75 frames = 6 s); a run that
# makes all of it is "frames", one that stops before is whole.
class Csm:
    tokenizer = types.SimpleNamespace(encode=lambda t: t.split())
    def __init__(self, fill):
        self.fill, self.calls = fill, []
    def default_speaker_prompt(self, voice, repo_id="sesame/csm-1b"):
        return (voice, repo_id)
    def generate(self, text, voice=None, speaker=0, max_audio_length_ms=90000, stream=False, **kw):
        self.calls.append((voice, max_audio_length_ms, "max_tokens" in kw))
        yield types.SimpleNamespace(audio=[0.0] * int(max_audio_length_ms / 1000 * 24000 * self.fill))
for fill in (1.0, 0.5):
    csm = Csm(fill)
    speak = w.make_speak(csm, w.CSM_VOICES, 24000, 1000, lambda parts, rate: len(parts), clock,
                         family="sesame")
    print("csm", fill, speak("Hello there.", "conversational_b", 1.0, None), csm.calls)
print("csm prompts", w.csm_prompts(Csm(1.0)).default_speaker_prompt("conversational_a"))
# The weights stay wired up to the GPU's working set; without a Metal device nothing changes.
wired = []
metal = types.SimpleNamespace(device_info=lambda: {{"max_recommended_working_set_size": 5726623744}},
                              set_wired_limit=wired.append)
print("wired", w.wire_weights(metal), wired, w.wire_weights(types.SimpleNamespace()))
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            // UTF-8 output: the test prints CJK sentences that a Windows code page cannot encode.
            .args(["-I", "-X", "utf8", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "budget short 75",
            "budget long 800",
            "budget slow 1280",
            "budget cap 1500",
            // 3 tokens -> the floor; stops after the chunk that passed 10 s (4 + 4 + 4).
            "runaway ('wav of 3 chunks', 'time') [{'voice': 'ryan', 'max_tokens': 75, 'stream': True}] closed True",
            "too slow",
            "plain ('wav of 1 chunks', None)",
            "budget chatterbox_turbo 1600",
            "budget voxcpm2 400",
            "budget spark 3264",
            "one speed ('wav of 1 chunks', None) [{'max_tokens': 94}]",
            "refused speed: this model speaks at one speed; use speed 1",
            "designed True",
            "sentences ('wav of 3 chunks', 'time') ['One.', 'Two!', 'Three?']",
            "split ['Hi there.', 'Bye。', 'Yes'] ['   ']",
            "tr ['Dr. Ali geldi.', '3. madde önemli, vs. bitti…', 'A. Yılmaz yazdı.']",
            "lines ['- süt', '- ekmek', '- yumurta']",
            "line ends [['Madde 3.', 'Sonraki satır'], ['Bkz.', 'Ali'], ['Saat 10.', 'Geldi.'], ['Hi Dr.', 'Ali']]",
            "quote ['He said \"Go.\"', 'Then left.']",
            "cap [118, 118, 118, 118, 118, 104]",
            "streamed short [('Short one. Two.', 75)]",
            // 420 tokens: runs of 26 sentences (182 tokens, under 187), all of the text.
            "streamed long 3 True True",
            // At half speed a call holds half the text: runs of 13 sentences.
            "streamed slow 5",
            // Three runs and a pause between each two; none used its whole budget.
            "streamed pauses ('wav of 5 chunks', None)",
            "one long sentence [(59, 1500)]",
            "cjk 4 True True",
            // A Latin sentence after a CJK one is spoken apart from it (a space), CJK ones not.
            "mixed ['今天很好。 Hello there. 再见。谢谢。']",
            "unpunctuated 3 True",
            "drifted (1, 'frames')",
            "csm 1.0 (1, 'frames') [('conversational_b', 6000.0, False)]",
            "csm 0.5 (1, None) [('conversational_b', 6000.0, False)]",
            "csm prompts ('conversational_a', 'mlx-community/csm-1b')",
            "wired 5726623744 [5726623744] None",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// The worker tells the speech families apart like [`tts_type`], finds a codec only in the
    /// cache of `--companion` links it makes (a malformed or missing companion is refused), and
    /// points a config without `tokenizer_name` at the folder's own tokenizer.
    #[test]
    fn the_audio_worker_finds_codecs_offline_and_its_own_tokenizer() {
        // The links need no privilege on Unix; the worker runs on macOS only.
        if !cfg!(unix) {
            return;
        }
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let model = |name: &str, files: &[(&str, &str)]| {
            let path = dir.path().join(name);
            for (file, body) in files {
                std::fs::create_dir_all(path.join(file).parent().unwrap()).unwrap();
                std::fs::write(path.join(file), body).unwrap();
            }
        };
        model(
            "outetts",
            &[
                ("config.json", r#"{"model_type":"qwen3"}"#),
                ("tokenizer.json", "{}"),
                ("README.md", "---\nlibrary_name: outetts\n---\n"),
            ],
        );
        model("orpheus", &[("config.json", r#"{"model_type":"llama"}"#)]);
        model(
            "dia",
            &[(
                "config.json",
                r#"{"data":{"delay_pattern":[0]},"model":{"decoder":{}}}"#,
            )],
        );
        model(
            "spark",
            &[
                ("config.json", r#"{"model_type":"qwen2"}"#),
                ("BiCodec/config.yaml", "x"),
                ("audio_tokenizer_config.yaml", "x"),
            ],
        );
        model("soprano", &[("config.json", r#"{"model_type":"soprano"}"#)]);
        model(
            "codec",
            &[("config.json", "{}"), ("model.safetensors", "x")],
        );
        let script = format!(
            r#"
import os, sys, tempfile; sys.path.insert(0, {root:?}); import mlx_audio_worker as w
root = {root:?}
for name in ("outetts", "orpheus", "dia", "spark", "soprano"):
    print("family", name, w.tts_family(os.path.join(root, name)))
cache = w.companion_cache(["mlx-community/snac_24khz=" + os.path.join(root, "codec")], tempfile.mkdtemp())
os.environ["HF_HUB_CACHE"] = cache
snap = os.path.join(cache, "models--mlx-community--snac_24khz", "snapshots", "harness")
print("linked", os.path.realpath(snap) == os.path.realpath(os.path.join(root, "codec")),
      open(os.path.join(cache, "models--mlx-community--snac_24khz", "refs", "main")).read())
print("only", sorted(os.listdir(cache)))
for bad in ("snac_24khz=" + root, "a/../b=" + root, "x/y=" + os.path.join(root, "missing"), "x/y"):
    try:
        w.companion_cache([bad], tempfile.mkdtemp())
        print("accepted", bad)
    except ValueError as e:
        print("refused:", e)
load = w.own_tokenizer(lambda path: {{"model_type": "qwen3"}}, os.path.join(root, "outetts"))
print("own", load("x")["tokenizer_name"] == os.path.join(root, "outetts"))
kept = w.own_tokenizer(lambda path: {{"tokenizer_name": "given"}}, os.path.join(root, "outetts"))
print("kept", kept("x")["tokenizer_name"])
none = w.own_tokenizer(lambda path: {{}}, os.path.join(root, "orpheus"))
print("none", "tokenizer_name" in none("x"))
"#,
            root = dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "family outetts outetts",
            "family orpheus orpheus",
            "family dia dia",
            "family spark spark",
            "family soprano soprano",
            "linked True harness",
            "only ['models--mlx-community--snac_24khz']",
            "refused: a companion is owner/repo=folder",
            "refused: a companion folder is missing",
            "own True",
            "kept given",
            "none False",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
        assert_eq!(text.matches("refused:").count(), 4, "{text}");
    }

    /// The transcription app: the key, its one route, a multipart body of known fields only, a
    /// body cap (by Content-Length too), audio decoded from the body (never a path) and the
    /// worker's time limit as a 504.
    #[test]
    fn the_audio_worker_transcribes_only_checked_requests() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, asyncio; sys.path.insert(0, {:?}); import mlx_audio_worker as w
heard = []
def decode(audio):
    if audio == b"junk":
        raise ValueError("not audio")
    return audio
def transcribe(samples, language, task):
    heard.append((samples, language, task))
    if samples == b"slow":
        raise w.TooSlow()
    if samples == b"long":
        raise w.Refused("the audio is longer than 10 seconds")
    return "hello world"
app = w.transcription_app_for(transcribe, decode, "stt")
def form(*fields, boundary="XyZ"):
    body = b""
    for name, value in fields:
        disposition = b'form-data; name="%s"' % name.encode()
        if name == "file":
            disposition += b'; filename="a.wav"'
        body += b"--" + boundary.encode() + b"\r\nContent-Disposition: " + disposition + b"\r\n\r\n" + value + b"\r\n"
    return body + b"--" + boundary.encode() + b"--\r\n"
CT = b"multipart/form-data; boundary=XyZ"
async def call(method, path, body=b"", ct=CT, key=True, length=None):
    sent, queue = [], [{{"type": "http.request", "body": body, "more_body": False}}]
    async def receive():
        return queue.pop(0) if queue else {{"type": "http.disconnect"}}
    async def send(m):
        sent.append(m)
    headers = [(b"content-type", ct)]
    if key:
        headers.append((b"authorization", b"Bearer k"))
    if length is not None:
        headers.append((b"content-length", str(length).encode()))
    await app({{"type": "http", "method": method, "path": path, "headers": headers}}, receive, send)
    return sent[0]["status"], b"".join(m.get("body", b"") for m in sent[1:]).decode(errors="replace")
async def main():
    T = "/v1/audio/transcriptions"
    binary = bytes(range(256)) * 4 + b"\r\n--Xy\r\n"
    for name, args in [
        ("health", ("GET", "/health")),
        ("nokey", ("POST", T, form(("file", b"RIFF")), CT, False)),
        ("models", ("GET", "/v1/models")),
        ("speech", ("POST", "/v1/audio/speech", b'{{"input":"x"}}')),
        ("ok", ("POST", T, form(("file", binary), ("language", b"tr"), ("model", b"stt")))),
        ("translate", ("POST", T, form(("file", b"RIFF"), ("task", b"translate")))),
        ("json", ("POST", T, form(("file", b"RIFF"), ("response_format", b"json")))),
        ("srt", ("POST", T, form(("file", b"RIFF"), ("response_format", b"srt")))),
        ("nofile", ("POST", T, form(("language", b"en")))),
        ("emptyfile", ("POST", T, form(("file", b"")))),
        ("path", ("POST", T, form(("file", b"RIFF"), ("path", b"/etc/hosts")))),
        ("twice", ("POST", T, form(("file", b"RIFF"), ("file", b"RIFF")))),
        ("task", ("POST", T, form(("file", b"RIFF"), ("task", b"summarize")))),
        ("lang", ("POST", T, form(("file", b"RIFF"), ("language", b"../x")))),
        ("json body", ("POST", T, b'{{"file":"/etc/hosts"}}', b"application/json")),
        ("noboundary", ("POST", T, form(("file", b"RIFF")), b"multipart/form-data")),
        ("malformed", ("POST", T, b"--XyZ\r\nno headers--XyZ--\r\n")),
        ("big", ("POST", T, b"x", CT, True, w.MAX_AUDIO_BODY + 1)),
        ("junk", ("POST", T, form(("file", b"junk")))),
        ("slow", ("POST", T, form(("file", b"slow")))),
        ("long", ("POST", T, form(("file", b"long")))),
    ]:
        print(name, await call(*args))
    print("heard", [(s if len(s) < 10 else (len(s), s == binary), l, t) for s, l, t in heard])
asyncio.run(main())
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "health (200, '{\"status\": \"ok\"}')",
            "nokey (401,",
            "models (200, '{\"object\": \"list\", \"data\": [{\"id\": \"stt\"",
            "speech (404,",
            "ok (200, '{\"text\": \"hello world\"}')",
            "translate (200,",
            "json (200,",
            "srt (400,",
            "nofile (400,",
            "emptyfile (400,",
            "path (400,",
            "twice (400,",
            "task (400,",
            "lang (400,",
            "json body (400,",
            "noboundary (400,",
            "malformed (400,",
            "big (413,",
            "junk (400,",
            "slow (504,",
            "long (400,",
            // The body's bytes reach the decoder unchanged (CRLF and boundary-like bytes too).
            "heard [((1032, True), 'tr', 'transcribe'), (b'RIFF', None, 'translate'), (b'RIFF', None, 'transcribe'), (b'slow', None, 'transcribe'), (b'long', None, 'transcribe')]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// A transcription cannot run away: audio over the length limit is refused, Whisper is
    /// stopped before its next 30 s window once the time is up (its `decode`), Parakeet after
    /// its next chunk (`chunk_callback`, with chunking for long audio); Parakeet does not
    /// translate.
    #[test]
    fn the_audio_worker_caps_transcriptions() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, types; sys.path.insert(0, {:?}); import mlx_audio_worker as w
now = [0.0]
clock = lambda: now[0]
class Whisper:
    # Decodes one 30 s window per call; each takes 4 s of (fake) clock.
    def __init__(self):
        self.calls = []
    def decode(self, segment, options=None):
        now[0] += 4.0
        return "w"
    def generate(self, audio, *, verbose=None, language=None, task="transcribe", **kw):
        self.calls.append(dict(language=language, task=task))
        windows = max(1, len(audio) // (30 * w.SAMPLE_RATE))
        return types.SimpleNamespace(text=" " + "".join(self.decode(None) for _ in range(windows)) + " ")
class Parakeet:
    def __init__(self):
        self.calls = []
    def generate(self, audio, *, dtype=None, chunk_duration=None, overlap_duration=None, chunk_callback=None, stream=False, **kw):
        self.calls.append(dict(chunk_duration=chunk_duration))
        step = int((chunk_duration or 1e9) * w.SAMPLE_RATE)
        for end in range(step, len(audio) + step, step):
            now[0] += 4.0
            if chunk_callback:
                chunk_callback(min(end, len(audio)), len(audio))
        return types.SimpleNamespace(text="p")
second = [0.0] * w.SAMPLE_RATE
def run(name, transcribe, samples, language=None, task="transcribe"):
    try:
        print(name, repr(transcribe(samples, language, task)))
    except w.TooSlow:
        print(name, "too slow")
    except w.Refused as e:
        print(name, "refused", e)
whisper = Whisper()
t = w.make_transcribe(whisper, 10, 600, clock)
run("whisper short", t, second * 60, "tr", "translate")
run("whisper runaway", t, second * 300)
run("whisper too long", t, second * 601)
print("whisper calls", whisper.calls)
now[0] = 0.0
parakeet = Parakeet()
t = w.make_transcribe(parakeet, 10, 600, clock)
run("parakeet short", t, second * 60)
run("parakeet runaway", t, second * 600)
run("parakeet translate", t, second, None, "translate")
print("parakeet calls", parakeet.calls)
# Whisper's made-up segments past the end of the audio are dropped; without start times the
# text is kept as it is.
seg = lambda start, text, end=None, score=-0.2: dict(start=start, end=start + 1 if end is None else end, text=text, avg_logprob=score)
heard = types.SimpleNamespace(text=" Hello there. pop pop", segments=[
    seg(0.0, " Hello"), seg(1.2, " there."), seg(2.95, " pop"), seg(4.0, " pop"),
    seg(2.0, " K", end=1.0), seg(2.5, " My?", end=9.0)])
nan = types.SimpleNamespace(segments=[seg(0.0, " Real", score=float("nan")), seg(1.0, " speech.", score=float("nan"))])
print("nan scores", repr(w.heard_text(nan, 3.0)))
print("tail", repr(w.heard_text(heard, 3.0)))
print("no starts", repr(w.heard_text(types.SimpleNamespace(text=" as is ", segments=[dict(text="x")]), 3.0)))
print("no segments", repr(w.heard_text(types.SimpleNamespace(text=" p "), 3.0)))
print("parakeet options", w.stt_options({:?}))
# A Whisper loaded without a processor gets the `openai/whisper-*` companion's; one with its own,
# or a model without the attribute, keeps what it has.
bare = types.SimpleNamespace(_processor=None)
w.whisper_processor(bare, ["kyutai/x=/a", "openai/whisper-small=/lib/p"], lambda d: "proc " + d)
own = types.SimpleNamespace(_processor="own")
w.whisper_processor(own, ["openai/whisper-small=/lib/p"], lambda d: "proc " + d)
other = types.SimpleNamespace()
w.whisper_processor(other, ["openai/whisper-small=/lib/p"], lambda d: "proc " + d)
# The worker resolves the folder (`realpath`: `/lib` is a link on Linux, a drive is added on
# Windows), so the expected value is resolved the same way.
print("processors", bare._processor == "proc " + w.os.path.realpath("/lib/p"), own._processor,
      hasattr(other, "_processor"))
"#,
            dir.path().to_string_lossy(),
            dir.path().join("parakeet").to_string_lossy()
        );
        let parakeet = dir.path().join("parakeet");
        std::fs::create_dir_all(&parakeet).unwrap();
        std::fs::write(
            parakeet.join("config.json"),
            r#"{"target":"nemo.collections.asr.models.rnnt_bpe_models.EncDecRNNTBPEModel"}"#,
        )
        .unwrap();
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "whisper short 'ww'",
            // 10 windows of 4 s: stopped before the fourth (12 s > 10 s).
            "whisper runaway too slow",
            "whisper too long refused the audio is longer than 600 seconds",
            "whisper calls [{'language': 'tr', 'task': 'translate'}, {'language': None, 'task': 'transcribe'}]",
            "parakeet short 'p'",
            "parakeet runaway too slow",
            "parakeet translate refused this model only transcribes",
            "parakeet calls [{'chunk_duration': None}, {'chunk_duration': 120.0}]",
            "parakeet options {'model_type': 'parakeet'}",
            "processors True own False",
            "tail 'Hello there.'",
            "nan scores 'Real speech.'",
            "no starts 'as is'",
            "no segments 'p'",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// A Mel-Band-RoFormer folder (`mlx-community/mel-roformer-*-mlx`) is mlx-audio's
    /// separator: a pipeline model of the audio-to-audio task, launched with `--task separate`;
    /// no chat or Python engine takes it, and without an MLX card it is nobody's.
    #[test]
    fn separation_folders_are_mlx_audio() {
        let card = "---\nlibrary_name: mlx\npipeline_tag: audio-to-audio\ntags:\n- mlx\n- mel_band_roformer\n---\n";
        let config = r#"{"_class":"MelRoFormerConfig","model_type":"mel_band_roformer","dim":192,"sample_rate":44100,"chunk_size":352800,"num_overlap":2}"#;
        let sep = folder(&[
            ("config.json", config),
            ("model.safetensors", "x"),
            ("README.md", card),
        ]);
        assert!(is_sts_dir(sep.path()));
        assert!(is_audio_dir(sep.path()) && !is_tts_dir(sep.path()) && !is_stt_dir(sep.path()));
        assert_eq!(flavour_of(sep.path()), Some(Flavour::Audio));
        assert!(!is_model_dir(sep.path()));
        assert!(!super::super::mistralrs::is_model_dir(sep.path()));
        assert!(super::super::python_hf::detect(sep.path()).is_none());
        assert!(companion_args(sep.path(), &[]).unwrap().is_empty());
        let m = to_local_model(sep.path(), sep.path().parent().unwrap()).unwrap();
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.task.as_deref(), Some("audio-to-audio"));
        let config = LaunchConfig {
            model_path: sep.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 11454,
            ..Default::default()
        };
        let args = build_args(
            &config,
            Path::new("/env/mlx_audio_worker.py"),
            Flavour::Audio,
        )
        .unwrap()
        .args
        .join(" ");
        assert!(args.contains(" --task separate"), "{args}");
        assert!(!args.contains("transcription"), "{args}");
        // No MLX card, or another architecture: not a separator.
        let plain = folder(&[
            ("config.json", r#"{"model_type":"mel_band_roformer"}"#),
            ("model.safetensors", "x"),
        ]);
        assert!(!is_sts_dir(plain.path()));
        let other = folder(&[
            ("config.json", r#"{"model_type":"bs_roformer"}"#),
            ("model.safetensors", "x"),
            ("README.md", card),
        ]);
        assert!(!is_sts_dir(other.path()));
        // The Hub: by the `model_type` tag.
        let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_sts_repo(&tags(&[
            "mlx",
            "mel_band_roformer",
            "audio-to-audio"
        ])));
        assert!(!is_sts_repo(&tags(&["mlx", "audio-to-audio", "sam_audio"])));
    }

    /// The separation worker's request takes the WAV alone (base64, a WAV data URI allowed) and
    /// refuses anything else; the audio is cut into overlapping windows that cover it all.
    #[test]
    fn the_audio_worker_separates_in_windows() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, json; sys.path.insert(0, {:?}); import mlx_audio_worker as w
def req(body):
    try:
        return w.process_request(json.dumps(body).encode())
    except w.Refused as e:
        return "refused: %s" % e
print("plain", req({{"model": "m", "request": {{"audio_base64": "UklGRg=="}}}}))
print("uri", req({{"request": {{"audio_base64": "data:audio/wav;base64,UklGRg=="}}}}))
print("option", req({{"model": "m", "request": {{"audio_base64": "UklGRg==", "seed": 1}}}}))
print("extra", req({{"model": "m", "prompt": "x", "request": {{"audio_base64": "UklGRg=="}}}}))
print("bad", req({{"request": {{"audio_base64": "!!"}}}}))
print("short", w.windows(100, 352800, 2))
print("long", w.windows(10, 4, 2))
print("exact", w.windows(8, 4, 2))
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "plain b'RIFF'",
            "uri b'RIFF'",
            "option refused: the request must hold only audio_base64",
            "extra refused: the body must hold only model and request",
            "bad refused: audio_base64 is not base64",
            "short [(0, 100)]",
            "long [(0, 4), (2, 6), (4, 8), (6, 10)]",
            "exact [(0, 4), (2, 6), (4, 8)]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    /// Uploaded audio is never decoded past the length limit: a container header declaring more
    /// is refused before decoding, a header that says less (or nothing) is caught while
    /// streaming, one step past the limit; only WAV, FLAC, MP3 and Ogg Vorbis are taken, and
    /// other rates are resampled to 16 kHz.
    #[test]
    fn the_audio_worker_caps_decoding() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mlx_audio_worker.py"), AUDIO_WORKER).unwrap();
        let script = format!(
            r#"
import sys, types; sys.path.insert(0, {:?})
# Stand-ins for numpy (lists) and miniaudio (a header and a stream of chunks).
np = types.ModuleType("numpy")
np.float32 = "f"
np.asarray = lambda x, dtype=None: list(x)
np.concatenate = lambda parts: [v for p in parts for v in p]
np.zeros = lambda n, dtype=None: [0.0] * n
sys.modules["numpy"] = np
import mlx_audio_worker as w
w.DECODE_FRAMES = 1000
class Mini:
    SampleFormat = types.SimpleNamespace(FLOAT32="f32")
    def __init__(self, rate, declared, actual):
        self.rate, self.declared, self.actual, self.read, self.closed = rate, declared, actual, 0, False
    def info(self, data):
        return types.SimpleNamespace(sample_rate=self.rate, nchannels=2, num_frames=self.declared)
    wav_get_info = flac_get_info = mp3_get_info = vorbis_get_info = info
    def stream_memory(self, data, fmt, nchannels, rate, frames):
        assert (fmt, nchannels, rate, frames) == ("f32", 1, self.rate, 1000), (fmt, nchannels, rate, frames)
        def gen():
            try:
                while self.read < self.actual:
                    n = min(frames, self.actual - self.read)
                    self.read += n
                    yield [0.0] * n
            finally:
                self.closed = True
        return gen()
resampled = []
def resample(samples, src, dst):
    resampled.append((len(samples), src, dst))
    return samples[: len(samples) * dst // src]
FLAC = b"fLaC" + b"\0" * 20
def run(name, data, mini, limit=10):
    try:
        out = w.decode_capped(data, limit, mini, resample)
        print(name, "ok", len(out), "read", mini.read, "closed", mini.closed)
    except w.Refused as e:
        print(name, "refused", e, "read", mini.read, "closed", mini.closed)
# 30 minutes declared: refused before any decoding.
run("declared", FLAC, Mini(16000, 30 * 60 * 16000, 30 * 60 * 16000))
# The header says 1 s (or nothing) but the stream goes on: stopped one step past 10 s.
run("lying", FLAC, Mini(16000, 16000, 30 * 60 * 16000))
run("unknown length", FLAC, Mini(16000, 0, 30 * 60 * 16000))
run("fits", b"RIFF\0\0\0\0WAVEfmt ", Mini(16000, 5 * 16000, 5 * 16000))
run("44.1k", b"ID3\3\0\0", Mini(44100, 0, 2 * 44100))
run("ogg", b"OggS\0\2" + b"\0" * 8, Mini(16000, 0, 100))
run("opus-like junk", b"\x1a\x45\xdf\xa3webm", Mini(16000, 0, 100))
run("text", b"hello world!", Mini(16000, 0, 100))
print("resampled", resampled)
"#,
            dir.path().to_string_lossy()
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "declared refused the audio is longer than 10 seconds read 0 closed False",
            "lying refused the audio is longer than 10 seconds read 161000 closed True",
            "unknown length refused the audio is longer than 10 seconds read 161000 closed True",
            "fits ok 80000 read 80000 closed True",
            "44.1k ok 32000 read 88200 closed True",
            "ogg ok 100",
            "opus-like junk refused the audio must be WAV, FLAC, MP3 or Ogg Vorbis read 0",
            "text refused the audio must be WAV, FLAC, MP3 or Ogg Vorbis read 0",
            "resampled [(88200, 44100, 16000)]",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }

    #[test]
    fn the_worker_guards_what_harness_relies_on() {
        // The key comes from the environment and is compared in constant time; the bind,
        // remote code and the served folder are fixed.
        assert!(WORKER.contains("os.environ.pop(\"HARNESS_WORKER_API_KEY\""));
        assert!(WORKER.contains("hmac.compare_digest"));
        assert!(WORKER.contains("HOST = \"127.0.0.1\""));
        assert!(WORKER.contains("\"--trust-remote-code\", \"--host\""));
        assert!(WORKER.contains("pinned_load"));
        assert!(WORKER.contains("HF_HUB_OFFLINE"));
        assert!(WORKER.contains("HARNESS_WORKER_API_KEY is required"));
        assert!(WORKER.contains("allow_abbrev"));
        assert!(WORKER.contains("HTTPS_PROXY"));
        assert!(VLM_WORKER.contains("kw[\"trust_remote_code\"] = False"));
        assert!(VLM_WORKER.contains("MLX_TRUST_REMOTE_CODE\"] = \"false\""));
        assert!(VLM_WORKER.contains("allow_abbrev=False"));
        assert!(AUDIO_WORKER.contains("os.environ.pop(\"HARNESS_WORKER_API_KEY\""));
        assert!(AUDIO_WORKER.contains("HOST = \"127.0.0.1\""));
        assert!(AUDIO_WORKER.contains("kw[\"trust_remote_code\"] = False"));
        assert!(AUDIO_WORKER.contains("HF_HUB_OFFLINE"));
    }

    /// `mlx-community/FLUX.2-Klein-4B-3bit`'s layout (weights shortened): the card names the base
    /// model as a list, the transformer's index has mflux's metadata.
    fn mflux_folder(card: &str, extra: &[(&str, &str)]) -> tempfile::TempDir {
        const INDEX: &str =
            r#"{"metadata":{"quantization_level":"3","mflux_version":"0.17.5"},"weight_map":{}}"#;
        let mut files = vec![
            ("README.md", card),
            ("tokenizer/tokenizer.json", "{}"),
            ("tokenizer/tokenizer_config.json", "{}"),
            ("transformer/0.safetensors", "transformer"),
            ("transformer/model.safetensors.index.json", INDEX),
            ("text_encoder/0.safetensors", "text"),
            ("text_encoder/model.safetensors.index.json", INDEX),
            ("vae/0.safetensors", "vae"),
            ("vae/model.safetensors.index.json", INDEX),
        ];
        files.extend_from_slice(extra);
        folder(&files)
    }
    const KLEIN_CARD: &str = "---\nlicense: apache-2.0\npipeline_tag: image-to-image\ntags:\n- flux\n- diffusion-single-file\nbase_model:\n- black-forest-labs/FLUX.2-klein-4B\n---\n\n3-bit\n";

    #[test]
    fn mflux_klein_folders_are_the_image_engines() {
        let klein = mflux_folder(KLEIN_CARD, &[]);
        let dir = klein.path();
        assert!(is_mflux_dir(dir));
        assert_eq!(mflux_model(dir), Some("flux2-klein-4b"));
        assert_eq!(flavour_of(dir), Some(Flavour::Image));
        assert_eq!(family_of(dir), Some(MFLUX));
        assert!(is_any_model_dir(dir) && MlxEngine::IMAGE.runs(dir));
        assert!(!is_model_dir(dir) && !is_vlm_dir(dir) && !is_audio_dir(dir));
        assert!(!MlxEngine::TEXT.runs(dir) && !MlxEngine::AUDIO.runs(dir));
        // The parts' weights, not the tokenizer.
        assert_eq!(mflux_weight_files(dir).len(), 3);
        let m = to_local_model(dir, dir.parent().unwrap()).unwrap();
        assert_eq!(m.kind, crate::domain::ModelKind::Pipeline);
        assert_eq!(m.task.as_deref(), Some("text-to-image"));
        assert_eq!(m.quantization, "3bit");
        assert_eq!(m.architecture.as_deref(), Some("flux2-klein-4b"));
        assert_eq!(
            m.size_bytes,
            ("transformer".len() + "text".len() + "vae".len()) as u64
        );

        // mflux-community's card: `base_model: <repo>` on one line.
        let community = mflux_folder(
            "---\nlicense: other\nbase_model: black-forest-labs/FLUX.2-klein-9B\nlibrary_name: mflux\n---\n",
            &[],
        );
        assert_eq!(mflux_model(community.path()), Some("flux2-klein-9b"));
        assert!(is_mflux_dir(community.path()));

        // Not mflux's: another base model (Qwen-Image-Edit in mflux's format), a diffusers
        // pipeline, a part missing, no mflux metadata, no tokenizer.
        let qwen = mflux_folder("---\nbase_model: Qwen/Qwen-Image-Edit-2511\n---\n", &[]);
        assert!(!is_mflux_dir(qwen.path()));
        let diffusers = mflux_folder(KLEIN_CARD, &[("model_index.json", "{}")]);
        assert!(!is_mflux_dir(diffusers.path()));
        let no_vae = mflux_folder(KLEIN_CARD, &[]);
        std::fs::remove_dir_all(no_vae.path().join("vae")).unwrap();
        assert!(!is_mflux_dir(no_vae.path()));
        let no_meta = mflux_folder(KLEIN_CARD, &[]);
        std::fs::write(
            no_meta
                .path()
                .join("transformer/model.safetensors.index.json"),
            r#"{"metadata":{"total_size":1},"weight_map":{}}"#,
        )
        .unwrap();
        assert!(!is_mflux_dir(no_meta.path()));
        let no_tokenizer = mflux_folder(KLEIN_CARD, &[]);
        std::fs::remove_dir_all(no_tokenizer.path().join("tokenizer")).unwrap();
        assert!(!is_mflux_dir(no_tokenizer.path()));
        assert!(!is_mflux_dir(&dir.join("README.md")));
    }

    #[test]
    fn mflux_launches_get_a_time_limit_and_no_token_limit() {
        let klein = mflux_folder(KLEIN_CARD, &[]);
        let config = LaunchConfig {
            model_path: klein.path().to_path_buf(),
            host: "127.0.0.1".into(),
            port: 18080,
            ..Default::default()
        };
        let args = build_args(&config, Path::new("mflux_worker.py"), Flavour::Image)
            .unwrap()
            .args;
        assert_eq!(args[0], "mflux_worker.py");
        assert!(args.windows(2).any(|w| w[0] == "--max-seconds"), "{args:?}");
        assert!(
            !args.iter().any(|a| a == "--max-tokens" || a == "--task"),
            "{args:?}"
        );
        // Its weights are counted from the parts.
        assert_eq!(
            vram_need(&config),
            ("transformer".len() + "text".len() + "vae".len()) as u64 * 6 / 5 + (300 << 20)
        );
        assert!(MlxEngine::IMAGE
            .build_launch(
                &LaunchConfig {
                    host: "0.0.0.0".into(),
                    ..config.clone()
                },
                &Default::default(),
                None
            )
            .is_err());
    }

    #[test]
    fn mflux_base_models_match_the_workers() {
        for (base, name) in MFLUX_MODELS {
            assert!(
                IMAGE_WORKER.contains(&format!("\"{base}\": \"{name}\"")),
                "{base}"
            );
            assert_eq!(
                mflux_model_for_base(&base.to_ascii_uppercase()),
                Some(*name)
            );
        }
        assert_eq!(
            mflux_model_for_base("black-forest-labs/FLUX.1-Kontext-dev"),
            None
        );
        assert!(IMAGE_WORKER.contains("HOST = \"127.0.0.1\""));
        assert!(IMAGE_WORKER.contains("HF_HUB_OFFLINE"));
    }

    /// The image worker's checks, without MLX: requests, image headers, the memory estimate and
    /// the size it picks, the time limit and the client leaving, and the HTTP app.
    #[test]
    fn the_image_worker_checks_requests_memory_and_time() {
        let Some(python) = system_python() else {
            eprintln!("no system Python 3, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mflux_worker.py"), IMAGE_WORKER).unwrap();
        let script = format!(
            r#"
import asyncio, base64, json, os, struct, sys, threading, types, zlib
sys.path.insert(0, {:?}); import mflux_worker as w

def png(width, height):
    return b"\x89PNG\r\n\x1a\n" + struct.pack(">I", 13) + b"IHDR" + struct.pack(">II", width, height) + b"\x08\x02\x00\x00\x00"

def jpeg(width, height):
    return b"\xff\xd8\xff\xe0" + struct.pack(">H", 16) + b"JFIF\x00" + b"\x00" * 9 + b"\xff\xc0" + struct.pack(">HBHH", 17, 8, height, width) + b"\x03" + b"\x00" * 9

def webp(width, height):
    return b"RIFF" + b"\x00" * 4 + b"WEBPVP8X" + b"\x00" * 8 + (width - 1).to_bytes(3, "little") + (height - 1).to_bytes(3, "little")

print("sizes", w.image_size(png(640, 480)), w.image_size(jpeg(320, 200)), w.image_size(webp(512, 768)), w.image_size(b"GIF89a"))

def check(raw, edit=False):
    try:
        r = w.image_request(json.dumps(raw).encode(), edit)
        return {{k: v for k, v in r.items() if k != "images"}} | {{"images": [s for _, s in r["images"]]}}
    except w.Refused as e:
        return "refused: " + str(e)

b64 = lambda b: base64.b64encode(b).decode()
print("plain", check({{"prompt": " a fox "}}))
print("full", check({{"model": "m", "prompt": "x", "n": 4, "seed": 9, "steps": 50, "size": "768x512", "guidance": 1.5}}))
print("auto", check({{"prompt": "x", "size": "auto", "seed": -1}})["size"])
for name, raw in [("steps", {{"prompt": "x", "steps": 51}}), ("n", {{"prompt": "x", "n": 5}}),
                  ("size16", {{"prompt": "x", "size": "520x520"}}), ("big", {{"prompt": "x", "size": "4096x4096"}}),
                  ("negative", {{"prompt": "x", "negative_prompt": "y"}}), ("empty", {{"prompt": " "}}),
                  ("bool", {{"prompt": "x", "steps": True}}), ("image on generation", {{"prompt": "x", "image": "a"}})]:
    print(name, check(raw))
print("edit", check({{"prompt": "x", "image": [b64(png(600, 400)), "data:image/jpeg;base64," + b64(jpeg(64, 64))], "strength": 0.5}}, True))
print("edit none", check({{"prompt": "x"}}, True))
print("edit five", check({{"prompt": "x", "image": [b64(png(64, 64))] * 5}}, True))
print("edit huge", check({{"prompt": "x", "image": b64(png(5000, 64))}}, True))
print("edit gif", check({{"prompt": "x", "image": b64(b"GIF89a" + b"\x00" * 20)}}, True))
print("edit base64", check({{"prompt": "x", "image": "not base64!"}}, True))

# Fitted on an M1 8 GB: 3.4 GB of weights, 5.71 GB at 512x512.
GB = 1e9
weights, budget = int(3.43 * GB), int(6.44 * GB)
print("need 512", round(w.memory_need(weights, 512, 512) / GB, 1), "need 256", round(w.memory_need(weights, 256, 256) / GB, 1),
      "edit 512", round(w.memory_need(weights, 512, 512, [(512, 512)]) / GB, 1),
      "9b more", w.memory_need(weights, 512, 512, name="flux2-klein-9b") > w.memory_need(weights, 512, 512))
print("budget", w.memory_budget({{"max_recommended_working_set_size": 5726633984, "memory_size": 8589934592}}))
print("fit", w.fit_size(weights, budget, "flux2-klein-4b"), w.fit_size(weights, budget, "flux2-klein-4b", [(600, 400)], (600, 400)),
      w.fit_size(weights, 10 ** 12, "flux2-klein-4b"), w.fit_size(10 ** 12, budget, "flux2-klein-4b"))
try:
    w.check_fits(weights, budget, "flux2-klein-4b", 1024, 1024)
except w.Refused as e:
    print("too big:", e)

# Generation: the size it picks, seeds, default steps, the time limit and the client leaving.
now = [0.0]
clock = lambda: now[0]
calls, folders = [], []
class Image:
    def save(self, buf, format):
        buf.write(b"PNG")
def render(models, req, seed, size, paths):
    calls.append((seed, size, req["steps"], [os.path.basename(p) for p in paths]))
    folders.extend(os.path.dirname(p) for p in paths)
    for _ in range(req["steps"]):
        models.stop.call_in_loop(0, None)
        now[0] += 10
    return Image()
models = types.SimpleNamespace(stop=None)
gone = threading.Event()
gen = w.make_generate(models, weights, budget, "flux2-klein-4b", 100, render, clock)
req = w.image_request(json.dumps({{"prompt": "x", "n": 2, "seed": 4294967295}}).encode(), False)
print("drawn", gen(req, False, gone), calls, models.stop)
calls.clear()
req = w.image_request(json.dumps({{"prompt": "x", "image": b64(png(600, 400)), "steps": 1, "seed": 1}}).encode(), True)
print("edited", gen(req, True, gone), calls, all(not os.path.exists(f) for f in folders))
calls.clear()
base = w.make_generate(models, weights, budget, "flux2-klein-base-4b", 100, render, clock, base_steps=True)
try:
    base(w.image_request(b'{{"prompt": "x", "size": "256x256"}}', False), False, gone)
except w.TooSlow:
    print("base too slow after", len(calls), "image(s) of", calls[0][2], "steps at", now[0])
gone.set()
try:
    gen(w.image_request(b'{{"prompt": "x", "size": "256x256"}}', False), False, gone)
except w.Gone:
    print("gone")
try:
    gen(w.image_request(b'{{"prompt": "x", "size": "1024x1024"}}', False), False, threading.Event())
except w.Refused as e:
    print("refused before drawing:", str(e)[:40])

# HTTP: the key, a refusal, an answer, and a client that leaves.
async def call(app, path, body, headers=None, leave=False):
    sent, msgs = [], [{{"type": "http.request", "body": body, "more_body": False}}]
    async def receive():
        if msgs:
            return msgs.pop(0)
        if leave:
            return {{"type": "http.disconnect"}}
        await asyncio.sleep(3600)
    async def send(m):
        sent.append(m)
    hs = [(b"authorization", b"Bearer k")] if headers is None else headers
    await app({{"type": "http", "method": "POST" if body else "GET", "path": path, "headers": hs}}, receive, send)
    if not sent:
        return "nothing"
    return sent[0]["status"], json.loads(sent[1]["body"])
async def run_now(req, edit, gone):
    if req["prompt"] == "slow":
        await asyncio.sleep(0.05)
        if gone.is_set():
            raise w.Gone()
    return [b"PNG"]
app = w.app_for(None, "klein", run=run_now, clock=lambda: 7)
async def main():
    print("health", await call(app, "/health", b"", headers=[]))
    print("no key", (await call(app, "/v1/images/generations", b'{{"prompt": "x"}}', headers=[]))[0])
    print("models", await call(app, "/v1/models", b""))
    print("bad", await call(app, "/v1/images/generations", b'{{"prompt": "x", "steps": 0}}'))
    print("ok", await call(app, "/v1/images/generations", b'{{"prompt": "x"}}'))
    print("left", await call(app, "/v1/images/edits", json.dumps({{"prompt": "slow", "image": b64(png(64, 64))}}).encode(), leave=True))
    print("other", (await call(app, "/v1/chat/completions", b"{{}}"))[0])
asyncio.run(main())
"#,
            dir.path().to_string_lossy(),
        );
        let out = std::process::Command::new(python)
            .args(["-I", "-c", &script])
            .env("HARNESS_WORKER_API_KEY", "k")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{text}");
        for line in [
            "sizes (640, 480) (320, 200) (512, 768) None",
            "plain {'prompt': 'a fox', 'n': 1, 'seed': None, 'steps': None, 'size': None, 'guidance': None, 'strength': None, 'images': []}",
            "full {'prompt': 'x', 'n': 4, 'seed': 9, 'steps': 50, 'size': (768, 512), 'guidance': 1.5, 'strength': None, 'images': []}",
            "auto None",
            "steps refused: steps must be a whole number from 1 to 50",
            "n refused: n must be a whole number from 1 to 4",
            "size16 refused: size 520x520 is out of range",
            "big refused: size 4096x4096 is out of range",
            "negative refused: unsupported field(s) for this model: negative_prompt",
            "empty refused: prompt is required",
            "bool refused: steps must be a whole number",
            "image on generation refused: unsupported field(s) for this model: image",
            "edit {'prompt': 'x', 'n': 1, 'seed': None, 'steps': None, 'size': None, 'guidance': None, 'strength': 0.5, 'images': [(600, 400), (64, 64)]}",
            "edit none refused: image (the picture to edit) is required",
            "edit five refused: an edit takes at most 4 images",
            "edit huge refused: image 1 is 5000x64; an edit takes at most 4096 pixels a side",
            "edit gif refused: image 1 is not a PNG, JPEG or WebP",
            "edit base64 refused: image 1 is not valid base64",
            "need 512 5.7 need 256 4.7 edit 512 5.8 9b more True",
            "budget 6442450944",
            "fit (512, 512) (592, 400) (1024, 1024) None",
            "too big: an image of 1024x1024 needs about 16.2 GB and this Mac has about 6.4 GB for it; the largest that fits is 512x512",
            "drawn [b'PNG', b'PNG'] [(4294967295, (512, 512), 4, []), (0, (512, 512), 4, [])] None",
            "edited [b'PNG'] [(1, (592, 400), 1, ['image-0.img'])] True",
            "base too slow after 1 image(s) of 28 steps at",
            "gone",
            "refused before drawing: an image of 1024x1024 needs about 16.2",
            "health (200, {'status': 'ok'})",
            "no key 401",
            "models (200, {'object': 'list', 'data': [{'id': 'klein', 'object': 'model', 'owned_by': 'harness'}]})",
            "bad (400, {'error': {'message': 'steps must be a whole number from 1 to 50.'",
            "ok (200, {'created': 7, 'data': [{'b64_json': 'UE5H'}]})",
            "left nothing",
            "other 404",
        ] {
            assert!(text.contains(line), "{line} missing:\n{text}");
        }
    }
}
