//! Running `sherpa-onnx-offline-tts` for one request, and shaping its WAV into the audio
//! format the request asked for.
//!
//! ## The text on Windows
//!
//! The tool takes the text as its last command-line argument and is a `char**` program: Windows
//! hands it the command line in the system's ANSI code page (cp1252, cp1254...), not UTF-8, so
//! "Merhaba dünya" arrives as cp1254 bytes, the tool warns "Non UTF8 encoded string" and the
//! phonemizer reads garbage (Chinese text arrives as `?`). The releases have no flag, file or
//! stdin input for the text. [`text_argument`] therefore builds the argument so that the
//! process's own conversion yields the UTF-8 bytes: the UTF-8 bytes are read *as* ANSI characters
//! (`MultiByteToWideChar`), and the C runtime's conversion back to ANSI returns exactly those
//! bytes. That holds for every single-byte code page (all of Europe, Turkey, Russia, Greece...)
//! and for the UTF-8 code page; double-byte code pages (Japanese, Chinese, Korean Windows) cannot
//! carry it, text with non-ASCII characters is refused there with `tts.windows_codepage`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use command_group::AsyncCommandGroup;
use tokio::sync::Semaphore;

use super::{AudioFormat, Params, TtsModel};
use crate::error_key::err;

/// Longest a single run may take (loading a model plus the longest allowed text).
const RUN_TIMEOUT: Duration = Duration::from_secs(300);
/// Runs at the same time: each one uses several CPU threads and loads its own model copy.
const MAX_PARALLEL_RUNS: usize = 2;

fn permits() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(MAX_PARALLEL_RUNS))
}

/// Threads one run uses: half the cores, between 1 and 4 (more does not speed up the models
/// here, measured on Kokoro int8 and Piper medium).
pub fn default_threads() -> u32 {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    (cores / 2).clamp(1, 4) as u32
}

/// The audio of one request.
#[derive(Debug, Clone)]
pub struct Speech {
    /// 16-bit PCM WAV, as the tool wrote it.
    pub wav: Vec<u8>,
    pub sample_rate: u32,
    pub duration_secs: f64,
    /// Voice and model that made it.
    pub voice: String,
    pub model: String,
    /// Wall time of the run (model load included).
    pub elapsed_ms: u64,
}

/// A request to [`synthesize`]; `input` and `speed` have been through
/// [`super::check_request`].
#[derive(Debug, Clone)]
pub struct SynthesisRequest<'a> {
    pub model: &'a TtsModel,
    pub voice: Option<&'a str>,
    pub input: &'a str,
    pub speed: f32,
    pub threads: Option<u32>,
}

/// The sherpa-onnx runtime binary for a launch: the chosen runtime id or the newest installed
/// one. Blocking.
pub fn resolve_binary(runtime: Option<&str>) -> Result<PathBuf, String> {
    crate::runtimes::resolve_family_binary_in(
        &crate::runtimes::RuntimeRegistry::global(),
        crate::engine::SHERPA_ONNX,
        runtime,
    )
}

/// [`resolve_binary`] of the newest sherpa-onnx runtime in `reg`.
pub fn resolve_binary_in(reg: &crate::runtimes::RuntimeRegistry) -> Result<PathBuf, String> {
    crate::runtimes::resolve_family_binary_in(reg, crate::engine::SHERPA_ONNX, None)
}

// --- the text argument ---

/// Wraps an argument for the Windows command line (the rules of `CommandLineToArgvW` and the C
/// runtime): quotes around it, a backslash run before a quote or the end doubled, quotes escaped.
pub fn quote_wide(units: &[u16]) -> Vec<u16> {
    let (quote, backslash) = (u16::from(b'"'), u16::from(b'\\'));
    let mut out = vec![quote];
    let mut backslashes = 0usize;
    for &u in units {
        if u == backslash {
            backslashes += 1;
            out.push(u);
            continue;
        }
        if u == quote {
            out.extend(std::iter::repeat_n(backslash, backslashes + 1));
        }
        backslashes = 0;
        out.push(u);
    }
    out.extend(std::iter::repeat_n(backslash, backslashes));
    out.push(quote);
    out
}

/// The text as the last command-line argument; see the module comment for why Windows needs
/// more than `arg(text)`. A leading `-` would make the tool read the text as a flag, so a space
/// goes in front of it.
pub fn text_argument(text: &str) -> Result<OsString, String> {
    let text = if text.starts_with('-') {
        format!(" {text}")
    } else {
        text.to_string()
    };
    #[cfg(windows)]
    {
        windows_text_argument(&text)
    }
    #[cfg(not(windows))]
    {
        Ok(OsString::from(text))
    }
}

#[cfg(windows)]
fn windows_text_argument(text: &str) -> Result<OsString, String> {
    use std::os::windows::ffi::OsStringExt;
    let units: Vec<u16> = if text.is_ascii() || ansi_is_utf8() {
        text.encode_utf16().collect()
    } else if let Some(units) = utf8_bytes_as_ansi(text.as_bytes()) {
        units
    } else {
        return Err(err("tts.windows_codepage").into());
    };
    Ok(OsString::from_wide(&quote_wide(&units)))
}

/// The system code page is UTF-8 ("Beta: Use Unicode UTF-8 for worldwide language support").
#[cfg(windows)]
fn ansi_is_utf8() -> bool {
    unsafe { windows::Win32::Globalization::GetACP() == 65001 }
}

/// UTF-16 units that the process's ANSI conversion turns back into `bytes`: each byte read as
/// a character of the active code page. `None` for a double-byte code page, where UTF-8 bytes
/// are not characters.
#[cfg(windows)]
fn utf8_bytes_as_ansi(bytes: &[u8]) -> Option<Vec<u16>> {
    use windows::Win32::Globalization::{
        GetCPInfo, MultiByteToWideChar, CPINFO, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS,
    };
    let mut info = CPINFO::default();
    unsafe { GetCPInfo(CP_ACP, &mut info) }.ok()?;
    if info.MaxCharSize != 1 {
        return None;
    }
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let len = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, None) };
    if len <= 0 {
        return None;
    }
    let mut wide = vec![0u16; len as usize];
    let written = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, Some(&mut wide)) };
    (written == len).then_some(wide)
}

/// What the C runtime's `argv` holds for `units` on Windows: the ANSI conversion. Used by the
/// test that proves [`utf8_bytes_as_ansi`] returns the original bytes.
#[cfg(all(windows, test))]
fn ansi_bytes_of(units: &[u16]) -> Vec<u8> {
    use windows::core::PCSTR;
    use windows::Win32::Globalization::{WideCharToMultiByte, CP_ACP};
    let len = unsafe { WideCharToMultiByte(CP_ACP, 0, units, None, PCSTR::null(), None) };
    let mut out = vec![0u8; len.max(0) as usize];
    let written =
        unsafe { WideCharToMultiByte(CP_ACP, 0, units, Some(&mut out), PCSTR::null(), None) };
    out.truncate(written.max(0) as usize);
    out
}

// --- the run ---

fn spawn_failed(binary: &Path, e: std::io::Error) -> String {
    err("tts.spawn_failed")
        .with("path", binary.display())
        .with("error", e)
        .into()
}

/// The sherpa-onnx tool's stderr without the lines that are not a message (progress dots,
/// source-file prefixes): the last few lines that say what went wrong.
pub(crate) fn failure_detail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("sample="))
        .collect();
    let tail = lines.len().saturating_sub(4);
    lines[tail..].join(" | ")
}

/// Starts the tool with `args` and the text (from [`text_argument`]) as the last argument, and
/// waits for it. A timeout drops the child, which kills the process tree.
pub(crate) async fn run_tool(
    binary: &Path,
    args: &[String],
    text: &std::ffi::OsStr,
) -> Result<std::process::Output, String> {
    // sherpa-onnx reads local files only: no network where the OS allows it.
    let mut command = crate::pyenv::isolation::offline_command(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Already quoted and converted by `text_argument`.
        command.as_std_mut().raw_arg(text);
    }
    #[cfg(not(windows))]
    command.arg(text);
    if let Some(parent) = binary.parent() {
        command.current_dir(parent);
    }
    let mut group = command.group();
    group.kill_on_drop(true);
    #[cfg(windows)]
    group.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = group.spawn().map_err(|e| spawn_failed(binary, e))?;
    tokio::time::timeout(RUN_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| String::from(err("tts.timeout").num("seconds", RUN_TIMEOUT.as_secs())))?
        .map_err(|e| spawn_failed(binary, e))
}

/// Sample rate of a 16-bit PCM WAV (`fmt ` chunk).
pub fn wav_sample_rate(wav: &[u8]) -> Option<u32> {
    if wav.len() < 28 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    while pos + 8 <= wav.len() {
        let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        if &wav[pos..pos + 4] == b"fmt " && pos + 16 <= wav.len() {
            return Some(u32::from_le_bytes(wav[pos + 12..pos + 16].try_into().ok()?));
        }
        pos += 8 + size + (size & 1);
    }
    None
}

/// Runs the tool once and returns its audio. `binary` is the runtime's
/// `sherpa-onnx-offline-tts`.
pub async fn synthesize(binary: &Path, req: &SynthesisRequest<'_>) -> Result<Speech, String> {
    let (sid, voice) = req.model.speaker_id(req.voice)?;
    let voice_name = voice
        .map(|v| v.id.clone())
        .unwrap_or_else(|| sid.to_string());
    let text = text_argument(req.input)?;
    let dir = tempfile::Builder::new()
        .prefix("harness-tts-")
        .tempdir()
        .map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", std::env::temp_dir().display())
                    .with("error", e),
            )
        })?;
    let output = dir.path().join("speech.wav");
    let params = Params {
        sid,
        speed: req.speed,
        threads: req.threads.unwrap_or_else(default_threads),
        output: output.clone(),
    };
    let args = super::build_args(req.model, voice, &params);

    let _permit = permits()
        .acquire()
        .await
        .map_err(|_| String::from(err("tts.run_failed").with("detail", "gate closed")))?;
    let started = Instant::now();
    let out = run_tool(binary, &args, &text).await?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(err("tts.run_failed")
            .with("detail", failure_detail(&stderr))
            .into());
    }
    let wav = tokio::fs::read(&output)
        .await
        .map_err(|_| String::from(err("tts.run_failed").with("detail", failure_detail(&stderr))))?;
    let duration_secs = crate::audio::wav_duration_secs(&wav).unwrap_or(0.0);
    let sample_rate = wav_sample_rate(&wav).unwrap_or(0);
    if duration_secs <= 0.0 || sample_rate == 0 {
        return Err(err("tts.no_audio")
            .with("detail", failure_detail(&stderr))
            .into());
    }
    Ok(Speech {
        wav,
        sample_rate,
        duration_secs,
        voice: voice_name,
        model: req.model.name.clone(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

// --- audio formats ---

/// Raw samples of a 16-bit PCM WAV (the `data` chunk).
pub fn wav_to_pcm(wav: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 12;
    if wav.len() < 12 || &wav[..4] != b"RIFF" {
        return None;
    }
    while pos + 8 <= wav.len() {
        let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        if &wav[pos..pos + 4] == b"data" {
            let end = (pos + 8 + size).min(wav.len());
            return Some(wav[pos + 8..end].to_vec());
        }
        pos += 8 + size + (size & 1);
    }
    None
}

/// ffmpeg arguments that turn a WAV on stdin into `format` on stdout.
pub fn ffmpeg_args(format: AudioFormat) -> Option<Vec<&'static str>> {
    let encode: &[&str] = match format {
        AudioFormat::Mp3 => &["-c:a", "libmp3lame", "-q:a", "4", "-f", "mp3"],
        AudioFormat::Opus => &["-c:a", "libopus", "-b:a", "48k", "-f", "ogg"],
        AudioFormat::Aac => &["-c:a", "aac", "-b:a", "96k", "-f", "adts"],
        AudioFormat::Flac => &["-c:a", "flac", "-f", "flac"],
        AudioFormat::Wav | AudioFormat::Pcm => return None,
    };
    let mut args = vec![
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-i",
        "pipe:0",
        "-vn",
    ];
    args.extend_from_slice(encode);
    args.push("pipe:1");
    Some(args)
}

/// Whether `format` can be made right now (WAV and PCM always; the others need ffmpeg).
pub fn format_available(format: AudioFormat) -> bool {
    !format.needs_ffmpeg() || crate::audio::ffmpeg_path().is_some()
}

/// The WAV of a [`Speech`] in `format`.
pub async fn encode(wav: Vec<u8>, format: AudioFormat) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncWriteExt;
    match format {
        AudioFormat::Wav => return Ok(wav),
        AudioFormat::Pcm => {
            return wav_to_pcm(&wav).ok_or_else(|| {
                String::from(err("tts.encode_failed").with("error", "no data chunk"))
            })
        }
        _ => {}
    }
    let ffmpeg = crate::audio::ffmpeg_path().ok_or_else(|| {
        String::from(err("tts.format_needs_ffmpeg").with("format", format.as_str()))
    })?;
    let args = ffmpeg_args(format).expect("formats other than wav / pcm have ffmpeg arguments");
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd
        .spawn()
        .map_err(|e| String::from(err("tts.encode_failed").with("error", e)))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(&wav).await;
        let _ = stdin.shutdown().await;
    });
    let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| String::from(err("tts.encode_failed").with("error", "timeout")))?
        .map_err(|e| String::from(err("tts.encode_failed").with("error", e)))?;
    let _ = feed.await;
    if !out.status.success() || out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(err("tts.encode_failed")
            .with("error", stderr.lines().next().unwrap_or("no output").trim())
            .into());
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn quoted(s: &str) -> String {
        String::from_utf16(&quote_wide(&units(s))).unwrap()
    }

    #[test]
    fn arguments_are_quoted_for_the_windows_command_line() {
        assert_eq!(quoted("hello world"), r#""hello world""#);
        assert_eq!(quoted(r#"say "hi""#), r#""say \"hi\"""#);
        // A backslash run before a quote is doubled and the quote escaped; one before the end
        // is doubled so it does not escape the closing quote.
        assert_eq!(quoted(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quoted(r"C:\dir\"), r#""C:\dir\\""#);
        assert_eq!(quoted(r"C:\dir\file"), r#""C:\dir\file""#);
        assert_eq!(quoted(""), r#""""#);
    }

    #[test]
    fn a_leading_dash_is_not_a_flag() {
        let arg = text_argument("--version is fine to say").unwrap();
        let arg = arg.to_string_lossy().to_string();
        assert!(!arg.trim_start_matches(['"', ' ']).is_empty());
        assert!(!arg.starts_with("--") && !arg.starts_with("\"--"), "{arg}");
        assert!(text_argument("- one")
            .unwrap()
            .to_string_lossy()
            .contains("- one"));
    }

    #[cfg(not(windows))]
    #[test]
    fn text_is_passed_as_is_off_windows() {
        assert_eq!(
            text_argument("Merhaba dünya, 你好").unwrap(),
            OsString::from("Merhaba dünya, 你好")
        );
    }

    /// The Windows round trip: the process's ANSI conversion of the argument yields the UTF-8
    /// bytes of the text (the active code page of this machine decides whether it can).
    #[cfg(windows)]
    #[test]
    fn utf8_bytes_survive_the_ansi_conversion() {
        let text = "Merhaba dünya, çok güzel: ğ Ğ ş Ş ı İ ö Ö ü Ü ç Ç — “quotes” ñ é ß";
        match utf8_bytes_as_ansi(text.as_bytes()) {
            Some(wide) => {
                assert_eq!(ansi_bytes_of(&wide), text.as_bytes());
            }
            None => {
                // A double-byte code page: refused, never mangled.
                assert!(text_argument(text).is_err());
            }
        }
        assert_eq!(
            text_argument("ascii only").unwrap().to_string_lossy(),
            "\"ascii only\""
        );
    }

    #[test]
    fn wav_helpers_read_the_header() {
        // 8 kHz mono 16-bit, 4 samples.
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 8).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&8u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 2, 0, 3, 0, 4, 0]);
        assert_eq!(wav_sample_rate(&wav), Some(8000));
        assert_eq!(wav_to_pcm(&wav).unwrap(), vec![1, 0, 2, 0, 3, 0, 4, 0]);
        assert!((crate::audio::wav_duration_secs(&wav).unwrap() - 0.0005).abs() < 1e-9);
        assert_eq!(wav_sample_rate(b"not a wav file at all, really"), None);
        assert!(wav_to_pcm(b"junk").is_none());
    }

    #[test]
    fn failure_detail_keeps_the_last_message_lines() {
        let stderr = "Number of threads: 4\n\nsample=100, progress=0.5\nfoo.cc:Validate:12 Please provide --vits-model\nbar.cc:Run:3 Failed\n";
        assert_eq!(
            failure_detail(stderr),
            "Number of threads: 4 | foo.cc:Validate:12 Please provide --vits-model | bar.cc:Run:3 Failed"
        );
        assert_eq!(failure_detail(""), "");
    }

    #[test]
    fn ffmpeg_arguments_per_format() {
        assert!(ffmpeg_args(AudioFormat::Wav).is_none());
        assert!(ffmpeg_args(AudioFormat::Pcm).is_none());
        for (format, codec) in [
            (AudioFormat::Mp3, "libmp3lame"),
            (AudioFormat::Opus, "libopus"),
            (AudioFormat::Aac, "aac"),
            (AudioFormat::Flac, "flac"),
        ] {
            let args = ffmpeg_args(format).unwrap();
            assert!(args.contains(&codec), "{format:?}");
            assert_eq!(args.first(), Some(&"-hide_banner"));
            assert_eq!(args.last(), Some(&"pipe:1"));
            assert!(args.windows(2).any(|w| w == ["-i", "pipe:0"]));
        }
    }

    #[test]
    fn threads_are_bounded() {
        assert!((1..=4).contains(&default_threads()));
    }
}
