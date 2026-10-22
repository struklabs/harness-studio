//! Audio files for speech recognition.
//!
//! whisper-server decodes WAV (any rate and channel count), MP3, FLAC and Ogg Vorbis itself
//! (checked with real files on whisper.cpp `b5130`); M4A / MP4 (AAC), WebM (Opus, what a
//! browser's `MediaRecorder` makes) and Ogg Opus answer `400 Invalid request`. The gateway
//! finds the container by its first bytes ([`sniff`]) and converts the others to 16 kHz mono
//! WAV with `ffmpeg` ([`to_wav`]) when one is installed, so an OpenAI client can upload what it
//! has. Without ffmpeg such a file is refused with a message that says so.

use std::path::PathBuf;
use std::time::Duration;

use crate::error_key::err;

/// Longest conversion Harness waits for.
const CONVERT_TIMEOUT: Duration = Duration::from_secs(300);

/// Audio container, from the first bytes of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Wav,
    Mp3,
    Flac,
    /// Ogg with Vorbis audio.
    OggVorbis,
    /// Ogg with Opus audio (`.opus`).
    OggOpus,
    /// Matroska / WebM (a browser recording).
    Webm,
    /// MP4 / M4A (AAC).
    Mp4,
    /// AAC in ADTS frames (`.aac`).
    Aac,
    Unknown,
}

impl Container {
    /// whisper-server reads this container without help.
    pub fn is_native(self) -> bool {
        matches!(self, Self::Wav | Self::Mp3 | Self::Flac | Self::OggVorbis)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::OggVorbis => "ogg",
            Self::OggOpus => "opus",
            Self::Webm => "webm",
            Self::Mp4 => "m4a",
            Self::Aac => "aac",
            Self::Unknown => "unknown",
        }
    }
}

/// The container of `bytes` by its signature.
pub fn sniff(bytes: &[u8]) -> Container {
    let head = |n: usize| &bytes[..bytes.len().min(n)];
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Container::Wav;
    }
    if bytes.starts_with(b"fLaC") {
        return Container::Flac;
    }
    if bytes.starts_with(b"OggS") {
        let page = head(160);
        return if page.windows(8).any(|w| w == b"OpusHead") {
            Container::OggOpus
        } else {
            Container::OggVorbis
        };
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Container::Webm;
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Container::Mp4;
    }
    if bytes.starts_with(b"ID3") {
        return Container::Mp3;
    }
    if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0 {
        // Frame sync: layer bits 00 are AAC (ADTS), anything else MPEG audio (MP3).
        return if bytes[1] & 0x06 == 0 {
            Container::Aac
        } else {
            Container::Mp3
        };
    }
    Container::Unknown
}

/// `ffmpeg` on `PATH` (or next to a known package location on Windows).
pub fn ffmpeg_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    crate::integrations::launch::find_file_on_path(name)
}

/// Converts any audio ffmpeg reads to 16 kHz mono 16-bit WAV (what whisper works on).
pub async fn to_wav(input: Vec<u8>) -> Result<Vec<u8>, String> {
    to_wav_with(input, 16_000, None).await
}

/// [`to_wav`] at `rate` Hz, decoding at most `max_secs` seconds of the input when given (a
/// longer input is cut there, so a caller can reject it by the result's duration).
pub async fn to_wav_with(
    input: Vec<u8>,
    rate: u32,
    max_secs: Option<u32>,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncWriteExt;
    let ffmpeg = ffmpeg_path().ok_or_else(|| String::from(err("audio.ffmpeg_missing")))?;
    let mut cmd = crate::process::hidden_tokio_command(ffmpeg);
    let rate = rate.to_string();
    let limit = max_secs.map(|s| s.to_string());
    let mut args = vec![
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-i",
        "pipe:0",
        "-vn",
        "-ar",
        rate.as_str(),
        "-ac",
        "1",
        "-c:a",
        "pcm_s16le",
    ];
    if let Some(limit) = &limit {
        args.extend(["-t", limit.as_str()]);
    }
    args.extend(["-f", "wav", "pipe:1"]);
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    crate::orphans::protect(cmd.as_std_mut());
    let mut child = cmd
        .spawn()
        .map_err(|e| String::from(err("audio.convert_failed").with("error", e)))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    // Feed the input while the output is read, so neither pipe fills up.
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(&input).await;
        let _ = stdin.shutdown().await;
    });
    let out = tokio::time::timeout(CONVERT_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| String::from(err("audio.convert_timeout")))?
        .map_err(|e| String::from(err("audio.convert_failed").with("error", e)))?;
    let _ = feed.await;
    if !out.status.success() || out.stdout.len() <= 44 {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr
            .lines()
            .next()
            .unwrap_or("no audio decoded")
            .trim()
            .to_string();
        return Err(err("audio.convert_failed").with("error", reason).into());
    }
    Ok(out.stdout)
}

/// Duration of a WAV file in seconds (PCM header; `None` for anything else).
pub fn wav_duration_secs(bytes: &[u8]) -> Option<f64> {
    if sniff(bytes) != Container::Wav {
        return None;
    }
    let mut pos = 12;
    let (mut byte_rate, mut data) = (None, None);
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        if id == b"fmt " && pos + 20 <= bytes.len() {
            byte_rate = Some(u32::from_le_bytes(
                bytes[pos + 16..pos + 20].try_into().ok()?,
            ));
        } else if id == b"data" {
            data = Some(size.min(bytes.len() - pos - 8));
            break;
        }
        pos += 8 + size + (size & 1);
    }
    let (rate, data) = (byte_rate?, data?);
    (rate > 0).then(|| data as f64 / f64::from(rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/audio")
                .join(name),
        )
        .unwrap()
    }

    /// Real files: `hello.wav` is Windows SAPI speech (16 kHz mono), the others ffmpeg encodes of it.
    #[test]
    fn sniffs_real_files() {
        let cases = [
            ("hello.wav", Container::Wav, true),
            ("hello.mp3", Container::Mp3, true),
            ("hello.flac", Container::Flac, true),
            ("hello.ogg", Container::OggVorbis, true),
            ("hello.opus", Container::OggOpus, false),
            ("hello.webm", Container::Webm, false),
            ("hello.m4a", Container::Mp4, false),
        ];
        for (file, want, native) in cases {
            let c = sniff(&data(file));
            assert_eq!(c, want, "{file}");
            assert_eq!(c.is_native(), native, "{file}");
        }
        assert_eq!(sniff(b"plain text, not audio"), Container::Unknown);
        assert_eq!(sniff(&[]), Container::Unknown);
        // Synthetic frame headers: MPEG-1 layer 3 and ADTS AAC share the sync word.
        assert_eq!(sniff(&[0xff, 0xfb, 0x90, 0x00]), Container::Mp3);
        assert_eq!(sniff(&[0xff, 0xf1, 0x50, 0x80]), Container::Aac);
    }

    #[test]
    fn wav_duration() {
        let wav = data("hello.wav");
        let d = wav_duration_secs(&wav).unwrap();
        assert!((d - 5.855).abs() < 0.01, "{d}");
        assert!(wav_duration_secs(&data("hello.mp3")).is_none());
    }

    /// Needs ffmpeg on PATH: every container converts to a WAV whisper reads.
    #[tokio::test]
    #[ignore]
    async fn real_ffmpeg_converts() {
        if ffmpeg_path().is_none() {
            eprintln!("no ffmpeg on PATH, skipping");
            return;
        }
        for file in ["hello.webm", "hello.m4a", "hello.opus", "hello.mp3"] {
            let wav = to_wav(data(file)).await.unwrap();
            assert_eq!(sniff(&wav), Container::Wav, "{file}");
            let secs = wav_duration_secs(&wav).unwrap();
            assert!((secs - 5.9).abs() < 0.3, "{file}: {secs}");
        }
        let e = to_wav(b"not audio at all".to_vec()).await.unwrap_err();
        assert!(e.starts_with("audio.convert_failed"), "{e}");
    }
}
