//! Running `sherpa-onnx-vad-with-offline-asr` for one request.
//!
//! The registered sherpa-onnx runtime's binary is `sherpa-onnx-offline-tts`; the speech tools
//! of the same release sit next to it ([`tool_binary`]). The audio goes in as a 16-bit mono WAV
//! in the run's own temporary folder (the tool is started there and takes the relative file name,
//! so a user name with characters the Windows ANSI code page lacks cannot break the audio path),
//! with half a second of silence on both sides: Silero VAD needs that run-up to catch speech that
//! starts at the first sample (a synthesized voice does, and so does a clipped recording), and
//! without it the first words are lost. The times in the answer are moved back by that padding.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use command_group::AsyncCommandGroup;
use tokio::sync::Semaphore;

use super::{build_args, parse_output, AsrModel, Params, Transcript};
use crate::audio::{self, Container};
use crate::error_key::err;

/// Silence put before and after the audio, in seconds.
pub const PAD_SECS: f64 = 0.5;
/// Runs at the same time: each one loads its own model copy (Parakeet int8 is 0.7 GB in memory).
const MAX_PARALLEL_RUNS: usize = 2;
/// A run may take this long at the least; longer audio gets more (see [`timeout_for`]).
const MIN_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_TIMEOUT: Duration = Duration::from_secs(3600);

fn permits() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(MAX_PARALLEL_RUNS))
}

/// How long a run on `duration_secs` of audio may take: the models run ten times faster than
/// real time or better on a CPU, so twice the length with a floor is generous.
pub fn timeout_for(duration_secs: f64) -> Duration {
    Duration::from_secs_f64(120.0 + duration_secs.max(0.0) * 2.0).clamp(MIN_TIMEOUT, MAX_TIMEOUT)
}

/// The sherpa-onnx runtime binary for a launch: the chosen runtime id or the newest installed
/// one (the same runtime as text to speech). Blocking.
pub fn resolve_binary(runtime: Option<&str>) -> Result<PathBuf, String> {
    crate::tts::run::resolve_binary(runtime)
}

/// The VAD + ASR tool next to the runtime's own binary: the online one for a streaming model.
pub fn tool_binary(runtime_binary: &Path, family: super::AsrFamily) -> Result<PathBuf, String> {
    let stem = if family.is_streaming() {
        "sherpa-onnx"
    } else {
        "sherpa-onnx-vad-with-offline-asr"
    };
    let name = if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    };
    let tool = runtime_binary.with_file_name(name);
    if tool.is_file() {
        Ok(tool)
    } else {
        Err(err("stt.binary_not_found")
            .with("path", tool.display())
            .into())
    }
}

// --- the audio ---

/// A 16-bit mono PCM WAV: its sample rate and samples (the `data` chunk, cut to what is there,
/// since a WAV that ffmpeg wrote to a pipe declares a length of 0xFFFFFFFF).
pub fn pcm16_mono(wav: &[u8]) -> Option<(u32, &[u8])> {
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut rate = None;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= wav.len() {
            let tag = u16::from_le_bytes(wav[body..body + 2].try_into().ok()?);
            let channels = u16::from_le_bytes(wav[body + 2..body + 4].try_into().ok()?);
            let bits = u16::from_le_bytes(wav[body + 14..body + 16].try_into().ok()?);
            if tag != 1 || channels != 1 || bits != 16 {
                return None;
            }
            rate = Some(u32::from_le_bytes(wav[body + 4..body + 8].try_into().ok()?));
        } else if id == b"data" {
            let end = body.saturating_add(size).min(wav.len());
            let data = &wav[body..end];
            return Some((rate?, &data[..data.len() & !1]));
        }
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    None
}

/// 16-bit mono samples at `rate` as 16 kHz, the rate the online decoder and the speaker tools
/// read (linear interpolation).
pub fn resample_16k(rate: u32, pcm: &[u8]) -> Vec<u8> {
    const RATE: u32 = 16_000;
    let samples: Vec<i16> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect();
    if rate == RATE || samples.is_empty() || rate == 0 {
        return pcm[..samples.len() * 2].to_vec();
    }
    let ratio = f64::from(rate) / f64::from(RATE);
    let len = ((samples.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(len * 2);
    for i in 0..len {
        let pos = i as f64 * ratio;
        let j = pos as usize;
        let frac = pos - j as f64;
        let a = f64::from(samples[j]);
        let b = f64::from(*samples.get(j + 1).unwrap_or(&samples[j]));
        out.extend_from_slice(&((a + (b - a) * frac).round() as i16).to_le_bytes());
    }
    out
}

/// A 16-bit mono WAV of `samples` at `rate`.
fn wav_of(rate: u32, parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(44 + len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(len as u32).to_le_bytes());
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// The audio of a request, ready for the tool.
#[derive(Debug)]
pub struct Prepared {
    /// A WAV file.
    pub wav: Vec<u8>,
    /// Length of the audio itself, padding not counted.
    pub duration_secs: f64,
    /// Seconds of silence put in front of the audio (0 when it could not be padded).
    pub pad_secs: f64,
}

/// A 16-bit mono WAV padded with [`PAD_SECS`] of silence on both sides; `None` for any other
/// audio.
pub fn pad_wav(wav: &[u8]) -> Option<Prepared> {
    let (rate, data) = pcm16_mono(wav)?;
    if rate == 0 {
        return None;
    }
    let silence = vec![0u8; (f64::from(rate) * PAD_SECS) as usize * 2];
    Some(Prepared {
        wav: wav_of(rate, &[&silence, data, &silence]),
        duration_secs: data.len() as f64 / 2.0 / f64::from(rate),
        pad_secs: PAD_SECS,
    })
}

/// Makes `audio` what the tool takes. A 16-bit mono WAV is padded as it is; anything else
/// (stereo or float WAV, MP3, M4A, WebM / Opus...) is converted to 16 kHz mono WAV by ffmpeg
/// first. Without ffmpeg a WAV of another layout goes to the tool unpadded (it reads those), and
/// other containers are refused.
pub async fn prepare(audio: Vec<u8>) -> Result<Prepared, String> {
    if let Some(p) = pad_wav(&audio) {
        return Ok(p);
    }
    let container = audio::sniff(&audio);
    if audio::ffmpeg_path().is_some() {
        let converted = audio::to_wav(audio).await?;
        return pad_wav(&converted).ok_or_else(|| {
            String::from(err("audio.convert_failed").with("error", "not a 16-bit mono WAV"))
        });
    }
    if container == Container::Wav {
        let duration = audio::wav_duration_secs(&audio).unwrap_or(0.0);
        return Ok(Prepared {
            wav: audio,
            duration_secs: duration,
            pad_secs: 0.0,
        });
    }
    Err(err("stt.needs_ffmpeg")
        .with("format", container.name())
        .into())
}

// --- the run ---

fn spawn_failed(binary: &Path, e: std::io::Error) -> String {
    err("stt.spawn_failed")
        .with("path", binary.display())
        .with("error", e)
        .into()
}

/// What `transcribe` needs besides the audio.
#[derive(Debug, Clone)]
pub struct TranscribeRequest<'a> {
    pub model: &'a AsrModel,
    /// `silero_vad.onnx` ([`super::catalog::ensure_vad`]).
    pub vad: &'a Path,
    pub language: Option<&'a str>,
    pub threads: Option<u32>,
    /// Also tell the speakers apart ([`super::diarize`]).
    pub diarize: Option<Diarization<'a>>,
}

/// Speaker diarization of a run.
#[derive(Debug, Clone)]
pub struct Diarization<'a> {
    pub models: &'a super::diarize::Models,
    /// How many speakers there are, when the caller knows.
    pub speakers: Option<u32>,
}

/// Starts `tool` with `args` in `cwd` and waits for it. A timeout drops the child, which kills
/// the process tree.
async fn run_tool(
    tool: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    // sherpa-onnx reads local files only: no network where the OS allows it.
    let mut command = crate::pyenv::isolation::offline_command(tool);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut group = command.group();
    group.kill_on_drop(true);
    #[cfg(windows)]
    group.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = group.spawn().map_err(|e| spawn_failed(tool, e))?;
    tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| String::from(err("stt.timeout").num("seconds", timeout.as_secs())))?
        .map_err(|e| spawn_failed(tool, e))
}

/// Transcribes `audio` (any container [`prepare`] takes) with the tool next to
/// `runtime_binary`.
pub async fn transcribe(
    runtime_binary: &Path,
    req: &TranscribeRequest<'_>,
    audio: Vec<u8>,
) -> Result<Transcript, String> {
    let tool = tool_binary(runtime_binary, req.model.family)?;
    if req.model.family == super::AsrFamily::CohereTranscribe
        && super::cohere_language(req.language).is_none()
    {
        return Err(err("stt.language_unsupported")
            .with("language", req.language.unwrap_or_default())
            .with("languages", super::COHERE_LANGUAGES.join(", "))
            .into());
    }
    let prepared = prepare(audio).await?;
    let dir = tempfile::Builder::new()
        .prefix("harness-asr-")
        .tempdir()
        .map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", std::env::temp_dir().display())
                    .with("error", e),
            )
        })?;
    const INPUT: &str = "input.wav";
    // The online decoder reads 16 kHz only (it does not resample).
    let input = match pcm16_mono(&prepared.wav) {
        Some((rate, pcm)) if req.model.family.is_streaming() && rate != 16_000 => {
            wav_of(16_000, &[&resample_16k(rate, pcm)])
        }
        _ => prepared.wav.clone(),
    };
    tokio::fs::write(dir.path().join(INPUT), &input)
        .await
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", dir.path().join(INPUT).display())
                    .with("error", e),
            )
        })?;
    let params = Params {
        vad: req.vad.to_path_buf(),
        language: req.language.map(str::to_string),
        threads: req.threads.unwrap_or_else(crate::tts::run::default_threads),
    };
    let mut args = build_args(req.model, &params);
    args.push(INPUT.to_string());

    let _permit = permits()
        .acquire()
        .await
        .map_err(|_| String::from(err("stt.run_failed").with("detail", "gate closed")))?;
    let started = Instant::now();
    let out = run_tool(
        &tool,
        &args,
        dir.path(),
        timeout_for(prepared.duration_secs),
    )
    .await?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(err("stt.run_failed")
            .with("detail", crate::tts::run::failure_detail(&stderr))
            .into());
    }
    // The online decoder writes its result to stderr, as JSON with the token times.
    let mut segments = if req.model.family.is_streaming() {
        super::parse_online_json(&stderr, prepared.pad_secs, prepared.duration_secs)
    } else {
        parse_output(
            &String::from_utf8_lossy(&out.stdout),
            prepared.pad_secs,
            prepared.duration_secs,
        )
    };
    if let Some(d) = &req.diarize {
        let turns = diarize_run(runtime_binary, d, &prepared, dir.path(), params.threads).await?;
        super::diarize::label(&mut segments, &turns, prepared.pad_secs);
    }
    Ok(Transcript::new(
        segments,
        prepared.duration_secs,
        req.language
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string),
        req.model.name.clone(),
        started.elapsed().as_millis() as u64,
    ))
}

/// Runs the diarization tool on the prepared (padded) audio, made 16 kHz for it.
async fn diarize_run(
    runtime_binary: &Path,
    d: &Diarization<'_>,
    prepared: &Prepared,
    dir: &Path,
    threads: u32,
) -> Result<Vec<super::diarize::Turn>, String> {
    let tool = super::diarize::tool_binary(runtime_binary)?;
    let (rate, pcm) = pcm16_mono(&prepared.wav).ok_or_else(|| {
        String::from(
            err("stt.run_failed").with("detail", "speaker separation needs a 16-bit mono WAV"),
        )
    })?;
    const INPUT: &str = "speakers.wav";
    let wav = wav_of(super::diarize::RATE, &[&resample_16k(rate, pcm)]);
    tokio::fs::write(dir.join(INPUT), wav).await.map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", dir.join(INPUT).display())
                .with("error", e),
        )
    })?;
    let mut args = super::diarize::build_args(d.models, d.speakers, threads);
    args.push(INPUT.to_string());
    let out = run_tool(&tool, &args, dir, timeout_for(prepared.duration_secs)).await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(err("stt.run_failed")
            .with("detail", crate::tts::run::failure_detail(&stderr))
            .into());
    }
    Ok(super::diarize::parse_turns(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f64) -> Vec<u8> {
        let samples = (f64::from(rate) * secs) as usize;
        let data: Vec<u8> = (0..samples)
            .flat_map(|i| (((i % 50) as i16 - 25) * 100).to_le_bytes())
            .collect();
        wav_of(rate, &[&data])
    }

    #[test]
    fn a_mono_wav_is_padded_on_both_sides() {
        let wav = tone(16_000, 1.0);
        let p = pad_wav(&wav).unwrap();
        assert!((p.duration_secs - 1.0).abs() < 1e-9);
        assert_eq!(p.pad_secs, 0.5);
        assert!((audio::wav_duration_secs(&p.wav).unwrap() - 2.0).abs() < 1e-9);
        let (rate, data) = pcm16_mono(&p.wav).unwrap();
        assert_eq!(rate, 16_000);
        // 0.5 s of silence in front, the samples, 0.5 s behind.
        assert!(data[..16_000].iter().all(|b| *b == 0));
        assert!(data[data.len() - 16_000..].iter().all(|b| *b == 0));
        assert_eq!(
            &data[16_000..16_000 + 64],
            &pcm16_mono(&wav).unwrap().1[..64]
        );
    }

    #[test]
    fn other_layouts_are_not_padded() {
        // Stereo.
        let mut stereo = tone(16_000, 0.1);
        stereo[22] = 2;
        assert!(pad_wav(&stereo).is_none());
        assert!(pad_wav(b"not a wav at all, really, it is not").is_none());
        assert!(pad_wav(&[]).is_none());
    }

    #[test]
    fn a_streamed_wav_with_an_unknown_length_is_read_to_its_end() {
        // ffmpeg writing to a pipe leaves 0xFFFFFFFF as the data length.
        let mut wav = tone(16_000, 0.25);
        let at = wav.windows(4).position(|w| w == b"data").unwrap() + 4;
        wav[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let (_, data) = pcm16_mono(&wav).unwrap();
        assert_eq!(data.len(), 8000);
        assert!((pad_wav(&wav).unwrap().duration_secs - 0.25).abs() < 1e-9);
    }

    #[test]
    fn the_real_test_file_is_padded() {
        let wav =
            std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/audio/hello.wav"))
                .unwrap();
        let p = pad_wav(&wav).unwrap();
        let original = audio::wav_duration_secs(&wav).unwrap();
        assert!((p.duration_secs - original).abs() < 0.01);
        assert!((audio::wav_duration_secs(&p.wav).unwrap() - original - 1.0).abs() < 0.01);
    }

    #[test]
    fn the_timeout_grows_with_the_audio() {
        assert_eq!(timeout_for(10.0), MIN_TIMEOUT);
        assert_eq!(
            timeout_for(3600.0),
            Duration::from_secs(120 + 7200).min(MAX_TIMEOUT)
        );
        assert_eq!(timeout_for(1e9), MAX_TIMEOUT);
        assert_eq!(timeout_for(-5.0), MIN_TIMEOUT);
    }

    #[test]
    fn the_tool_sits_next_to_the_runtime_binary() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join(if cfg!(windows) {
            "sherpa-onnx-offline-tts.exe"
        } else {
            "sherpa-onnx-offline-tts"
        });
        std::fs::write(&runtime, b"x").unwrap();
        let err = tool_binary(&runtime, crate::asr::AsrFamily::Transducer).unwrap_err();
        assert!(err.contains("stt.binary_not_found"), "{err}");
        let tool = dir.path().join(if cfg!(windows) {
            "sherpa-onnx-vad-with-offline-asr.exe"
        } else {
            "sherpa-onnx-vad-with-offline-asr"
        });
        std::fs::write(&tool, b"x").unwrap();
        assert_eq!(
            tool_binary(&runtime, crate::asr::AsrFamily::Transducer).unwrap(),
            tool
        );
    }
}
