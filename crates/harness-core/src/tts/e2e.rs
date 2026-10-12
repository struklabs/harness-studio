//! Real text-to-speech runs, all `#[ignore]`: they download the sherpa-onnx
//! runtime (20 MB), a Turkish Piper voice (21 MB) and Kokoro int8 (132 MB) from the official
//! releases, check each against its SHA-256, and synthesize speech.
//!
//! ```text
//! cargo test -p harness-core --lib tts::e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `HS_TTS_E2E_DIR=<folder>` keeps the downloads between runs (default: a temporary folder).

use std::path::PathBuf;

use super::catalog;
use super::run::{self, SynthesisRequest};
use super::{AudioFormat, Params, TtsFamily, TtsModel};
use crate::engine::SHERPA_ONNX;
use crate::runtimes::github::{GithubInstall, ReleaseQuery};
use crate::runtimes::RuntimeRegistry;

struct Setup {
    _tmp: Option<tempfile::TempDir>,
    binary: PathBuf,
    home: PathBuf,
}

async fn setup() -> Setup {
    let (tmp, base) = match std::env::var("HS_TTS_E2E_DIR")
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
    let binary = match run::resolve_binary_in(&reg) {
        Ok(b) => b,
        Err(_) => {
            let entry = crate::runtimes::github::install(
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
            eprintln!("installed {} {:?}", entry.id, entry.version);
            run::resolve_binary_in(&reg).expect("installed runtime resolves")
        }
    };
    Setup {
        _tmp: tmp,
        binary,
        home: base.join("models").join("tts"),
    }
}

async fn model(s: &Setup, id: &str) -> TtsModel {
    catalog::install_into(id, &s.home)
        .await
        .unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn check_wav(wav: &[u8], min_secs: f64, rate: u32) {
    assert_eq!(crate::audio::sniff(wav), crate::audio::Container::Wav);
    let secs = crate::audio::wav_duration_secs(wav).expect("a duration");
    assert!(secs >= min_secs, "{secs} s");
    assert_eq!(run::wav_sample_rate(wav), Some(rate));
}

async fn speak(
    s: &Setup,
    m: &TtsModel,
    text: &str,
    voice: Option<&str>,
    speed: f32,
) -> run::Speech {
    run::synthesize(
        &s.binary,
        &SynthesisRequest {
            model: m,
            voice,
            input: text,
            speed,
            threads: None,
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"))
}

/// The text must reach the tool as UTF-8 bytes whatever the Windows code page is: the tool
/// echoes it back (`The text is: ...`).
async fn echoed(s: &Setup, m: &TtsModel, text: &str) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let (sid, voice) = m.speaker_id(None).unwrap();
    let args = super::build_args(
        m,
        voice,
        &Params {
            sid,
            speed: 1.0,
            threads: 2,
            output: dir.path().join("o.wav"),
        },
    );
    let arg = run::text_argument(text).unwrap();
    let out = run::run_tool(&s.binary, &args, &arg).await.unwrap();
    assert!(out.status.success());
    let all: Vec<u8> = [out.stdout, out.stderr].concat();
    let marker = b"The text is: ";
    let start = all
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("the tool echoes the text")
        + marker.len();
    let end = all[start..]
        .windows(14)
        .position(|w| w == b". Speaker ID: ")
        .map(|p| start + p)
        .expect("followed by the speaker id");
    all[start..end].to_vec()
}

#[tokio::test]
#[ignore]
async fn real_turkish_and_english_speech() {
    let s = setup().await;
    let tr = model(&s, "vits-piper-tr_TR-fettah-medium-int8").await;
    assert_eq!(tr.family, TtsFamily::Vits);
    assert_eq!(tr.languages, ["tr"]);

    // Turkish letters arrive as UTF-8.
    let text = "Şu çocuğun güzel öğüdü: İstanbul'da ışık yağıyor.";
    let echoed_text = echoed(&s, &tr, text).await;
    assert_eq!(
        String::from_utf8_lossy(&echoed_text),
        text,
        "bytes: {echoed_text:?}"
    );

    let speech = speak(&s, &tr, text, Some("alloy"), 1.0).await;
    check_wav(&speech.wav, 1.5, 22_050);
    eprintln!(
        "tr: {:.2} s of audio in {} ms",
        speech.duration_secs, speech.elapsed_ms
    );

    // A faster voice is a shorter clip.
    let fast = speak(&s, &tr, text, None, 2.0).await;
    assert!(
        fast.duration_secs < speech.duration_secs * 0.75,
        "{} vs {}",
        fast.duration_secs,
        speech.duration_secs
    );

    let kokoro = model(&s, "kokoro-int8-multi-lang-v1_0").await;
    assert_eq!(kokoro.voices.len(), 54);
    let en = speak(
        &s,
        &kokoro,
        "Harness Studio reads this aloud.",
        Some("alloy"),
        1.0,
    )
    .await;
    check_wav(&en.wav, 1.0, 24_000);
    assert_eq!(en.voice, "af_alloy");
    let british = speak(&s, &kokoro, "Good morning to you.", Some("bf_emma"), 1.0).await;
    check_wav(&british.wav, 0.8, 24_000);
    let es = speak(
        &s,
        &kokoro,
        "Hola, esto es una prueba.",
        Some("ef_dora"),
        1.0,
    )
    .await;
    check_wav(&es.wav, 0.8, 24_000);
    // Chinese needs its characters to survive the command line as well.
    let zh = speak(
        &s,
        &kokoro,
        "你好，这是一个测试。",
        Some("zf_xiaoxiao"),
        1.0,
    )
    .await;
    check_wav(&zh.wav, 1.0, 24_000);

    // PCM is the data chunk; mp3 needs ffmpeg.
    let pcm = run::encode(en.wav.clone(), AudioFormat::Pcm).await.unwrap();
    assert!(pcm.len() as f64 > en.duration_secs * 24_000.0 * 2.0 * 0.95);
    if run::format_available(AudioFormat::Mp3) {
        let mp3 = run::encode(en.wav.clone(), AudioFormat::Mp3).await.unwrap();
        assert_eq!(crate::audio::sniff(&mp3), crate::audio::Container::Mp3);
        let flac = run::encode(en.wav.clone(), AudioFormat::Flac)
            .await
            .unwrap();
        assert_eq!(crate::audio::sniff(&flac), crate::audio::Container::Flac);
    } else {
        eprintln!("no ffmpeg on PATH: mp3 / flac skipped");
    }

    // An unknown voice is refused before the tool starts.
    let e = run::synthesize(
        &s.binary,
        &SynthesisRequest {
            model: &kokoro,
            voice: Some("morgan"),
            input: "x",
            speed: 1.0,
            threads: None,
        },
    )
    .await
    .unwrap_err();
    assert!(e.starts_with("tts.voice_not_found"), "{e}");
}
