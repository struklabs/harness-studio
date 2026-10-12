//! Real speech recognition, all `#[ignore]`: they download the sherpa-onnx runtime
//! (20 MB), the Kokoro and Piper voices of the text-to-speech tests, Silero VAD and speech
//! recognition models from the official releases, check each against its SHA-256, make speech
//! with the voices and read it back.
//!
//! ```text
//! cargo test -p harness-core --lib asr::e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `HS_ASR_E2E_DIR=<folder>` keeps the downloads between runs (default: a temporary folder).
//! `HS_ASR_E2E_MODELS=parakeet,sense-voice,omnilingual` picks the models (default: the
//! smallest, Omnilingual).

use std::path::PathBuf;

use super::run::{self, TranscribeRequest};
use super::{catalog, AsrFamily, AsrModel};
use crate::engine::SHERPA_ONNX;
use crate::runtimes::github::{GithubInstall, ReleaseQuery};
use crate::runtimes::RuntimeRegistry;
use crate::tts;

struct Setup {
    _tmp: Option<tempfile::TempDir>,
    binary: PathBuf,
    base: PathBuf,
}

async fn setup() -> Setup {
    let (tmp, base) = match std::env::var("HS_ASR_E2E_DIR")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(dir) => (None, PathBuf::from(dir)),
        None => {
            let t = tempfile::tempdir().unwrap();
            let p = t.path().to_path_buf();
            (Some(t), p)
        }
    };
    let reg = RuntimeRegistry::with_paths(base.join("runtimes.json"), base.join("runtimes"));
    let binary = match tts::run::resolve_binary_in(&reg) {
        Ok(b) => b,
        Err(_) => {
            crate::runtimes::github::install(
                &reg,
                &GithubInstall {
                    query: ReleaseQuery {
                        family: SHERPA_ONNX.into(),
                        repo: crate::engine::SHERPA_ONNX_REPO.into(),
                        ..Default::default()
                    },
                    name: Some("sherpa-onnx".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("sherpa-onnx installs from the official release");
            tts::run::resolve_binary_in(&reg).expect("installed runtime resolves")
        }
    };
    Setup {
        _tmp: tmp,
        binary,
        base,
    }
}

async fn voice(s: &Setup, model: &str, voice: Option<&str>, text: &str) -> Vec<u8> {
    let m = tts::catalog::install_into(model, &s.base.join("models/tts"))
        .await
        .unwrap_or_else(|e| panic!("{model}: {e}"));
    tts::run::synthesize(
        &s.binary,
        &tts::run::SynthesisRequest {
            model: &m,
            voice,
            input: text,
            speed: 1.0,
            threads: None,
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"))
    .wav
}

async fn asr_model(s: &Setup, id: &str) -> AsrModel {
    catalog::install_into(id, &s.base.join("models/asr"))
        .await
        .unwrap_or_else(|e| panic!("{id}: {e}"))
}

async fn read(s: &Setup, m: &AsrModel, wav: Vec<u8>, language: Option<&str>) -> super::Transcript {
    let vad = catalog::ensure_vad_in(&s.base.join("models/asr"))
        .await
        .unwrap();
    run::transcribe(
        &s.binary,
        &TranscribeRequest {
            model: m,
            vad: &vad,
            language,
            threads: None,
        },
        wav,
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"))
}

/// How many of the words of `expected` are in `got` (case and punctuation do not matter).
fn word_hits(expected: &str, got: &str) -> (usize, usize) {
    let norm = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect()
    };
    let (want, have) = (norm(expected), norm(got));
    (want.iter().filter(|w| have.contains(w)).count(), want.len())
}

const ENGLISH: &str = "The quick brown fox jumps over the lazy dog. Speech recognition runs locally on this computer. Harness Studio can transcribe long recordings without sending anything to the cloud.";
const TURKISH: &str = "Merhaba, bu bir deneme konuşmasıdır. Yapay zekâ ile ses tanıma artık tamamen bilgisayarınızda çalışıyor.";
const OMNILINGUAL: &str = "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-v2-int8-2026-02-05";

fn chosen() -> Vec<String> {
    std::env::var("HS_ASR_E2E_MODELS")
        .unwrap_or_else(|_| "omnilingual".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect()
}

/// English speech made by Kokoro, read back by each chosen model; Turkish speech made by Piper,
/// read back by Omnilingual.
#[tokio::test]
#[ignore]
async fn real_speech_read_back() {
    let s = setup().await;
    let english = voice(&s, "kokoro-int8-multi-lang-v1_0", Some("af_heart"), ENGLISH).await;
    let mut ran = 0;
    for pick in chosen() {
        let entry = catalog::catalog()
            .into_iter()
            .find(|e| e.id.contains(&pick))
            .unwrap_or_else(|| panic!("no catalog model matches {pick}"));
        let m = asr_model(&s, &entry.id).await;
        assert_eq!(m.family, entry.family);
        let t = read(&s, &m, english.clone(), Some("en")).await;
        let (hit, total) = word_hits(ENGLISH, &t.text);
        eprintln!(
            "{}: {hit}/{total} words in {} ms ({} segments): {}",
            m.name,
            t.elapsed_ms,
            t.segments.len(),
            t.text
        );
        assert!(hit * 10 >= total * 7, "{hit}/{total}: {}", t.text);
        assert!(t.duration_secs > 8.0 && !t.segments.is_empty());
        assert!(t.segments[0].start < 1.0, "{:?}", t.segments[0]);
        assert!(t.segments.last().unwrap().end <= t.duration_secs + 0.01);
        assert!(t.to_srt().starts_with("1\n00:00:00,"));
        ran += 1;
    }
    assert!(ran > 0);

    if chosen().iter().any(|p| p == "omnilingual") {
        let omni = asr_model(&s, OMNILINGUAL).await;
        assert_eq!(omni.family, AsrFamily::Omnilingual);
        let turkish = voice(&s, "vits-piper-tr_TR-fettah-medium-int8", None, TURKISH).await;
        let t = read(&s, &omni, turkish, Some("tr")).await;
        let (hit, total) = word_hits(TURKISH, &t.text);
        eprintln!("tr: {hit}/{total} words: {}", t.text);
        assert!(hit * 10 >= total * 6, "{hit}/{total}: {}", t.text);
    }
}

/// A recording of well over a minute (the English text eight times, with pauses) is cut at the
/// pauses and read in order; every segment has a time.
#[tokio::test]
#[ignore]
async fn real_long_recording_is_segmented() {
    let s = setup().await;
    let wav = voice(&s, "kokoro-int8-multi-lang-v1_0", Some("af_heart"), ENGLISH).await;
    let (rate, samples) = run::pcm16_mono(&wav).expect("Kokoro writes 16-bit mono");
    let silence = vec![0u8; rate as usize * 2 * 2];
    let mut long = Vec::new();
    for _ in 0..8 {
        long.extend_from_slice(samples);
        long.extend_from_slice(&silence);
    }
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + long.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    for v in [1u16, 1] {
        file.extend_from_slice(&v.to_le_bytes());
    }
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 2).to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(long.len() as u32).to_le_bytes());
    file.extend_from_slice(&long);

    let omni = asr_model(&s, OMNILINGUAL).await;
    let t = read(&s, &omni, file, None).await;
    eprintln!(
        "{:.0} s of audio, {} segments, {} ms",
        t.duration_secs,
        t.segments.len(),
        t.elapsed_ms
    );
    assert!(t.duration_secs > 60.0);
    assert!(t.segments.len() >= 8, "{} segments", t.segments.len());
    assert!(t.segments.windows(2).all(|w| w[0].end <= w[1].start + 0.01));
    let (hit, total) = word_hits(&ENGLISH.repeat(8), &t.text);
    assert!(hit * 10 >= total * 7, "{hit}/{total}");
}
