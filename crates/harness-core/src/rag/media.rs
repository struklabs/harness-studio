//! Audio, video and images in knowledge bases.
//!
//! * **Audio and video** are transcribed by the base's speech-recognition model through the
//!   gateway (`/v1/audio/transcriptions`, `verbose_json`), so every ASR engine Harness serves
//!   works. The segments keep their times: a chunk of the transcript carries the time span it
//!   covers ([`crate::rag::chunk::Chunk::t_start_ms`]) and a citation shows it (`1:05-1:32`).
//!   Video is reduced to its audio track with ffmpeg (low priority) first.
//! * **Images** are read by the base's OCR model; the file name and pixel size are indexed too,
//!   so a picture without text can still be found by its name.
//! * The details (length, language, size, model) are kept as [`crate::state::DocMeta`].
//!
//! The stages run apart (see [`crate::rag::kb`]): all files are transcribed or recognized
//! first, all chunks are embedded after, so the speech model and the embedding model are never
//! loaded alternately. Every request is background priority and the embedding model of a base
//! is a CPU model, so a run never takes the GPU from a chat.

use super::chunk::{self, Chunk};
use super::client;
use super::extract::PageText;
use super::ocr::OcrModel;
use super::Endpoint;
use crate::error_key::err;
use crate::state::DocMeta;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Largest audio or video file indexed.
pub const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;
/// Largest image file indexed.
pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

const AUDIO: &[&str] = &["wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "wma"];
const VIDEO: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Video,
    Image,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
            MediaKind::Image => "image",
        }
    }

    pub fn max_bytes(self) -> u64 {
        match self {
            MediaKind::Image => MAX_IMAGE_BYTES,
            _ => MAX_MEDIA_BYTES,
        }
    }
}

/// Media kind from the file extension.
pub fn media_kind(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let ext = ext.as_str();
    if AUDIO.contains(&ext) {
        Some(MediaKind::Audio)
    } else if VIDEO.contains(&ext) {
        Some(MediaKind::Video)
    } else if IMAGE.contains(&ext) {
        Some(MediaKind::Image)
    } else {
        None
    }
}

/// The speech-recognition model of a knowledge base.
#[derive(Debug, Clone)]
pub struct AsrModel {
    pub endpoint: Endpoint,
    pub alias: String,
}

/// One timed piece of a transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
    pub segments: Vec<Segment>,
}

fn secs_to_ms(v: &Value) -> Option<u64> {
    v.as_f64()
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| (s * 1000.0).round() as u64)
}

/// A transcript from the gateway's `verbose_json` (or plain `json`) answer.
pub fn parse_transcript(v: &Value) -> Transcript {
    let segments: Vec<Segment> = v["segments"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let text = s["text"].as_str()?.trim().to_string();
                    if text.is_empty() {
                        return None;
                    }
                    let start = secs_to_ms(&s["start"]).unwrap_or(0);
                    let end = secs_to_ms(&s["end"]).unwrap_or(start).max(start);
                    Some(Segment {
                        start_ms: start,
                        end_ms: end,
                        text,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let text = v["text"]
        .as_str()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });
    Transcript {
        text,
        language: v["language"]
            .as_str()
            .map(str::to_string)
            .filter(|l| !l.is_empty()),
        duration_ms: secs_to_ms(&v["duration"]).or_else(|| segments.last().map(|s| s.end_ms)),
        segments,
    }
}

/// Chunks of a timed transcript: consecutive segments are packed up to
/// [`chunk::TARGET_CHARS`]; each chunk spans the times of its first and last segment.
pub fn chunk_segments(segments: &[Segment]) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut cur: Option<(u64, u64, String)> = None;
    for s in segments {
        match cur.as_mut() {
            Some((_, end, text))
                if text.chars().count() + s.text.chars().count() < chunk::TARGET_CHARS =>
            {
                text.push(' ');
                text.push_str(&s.text);
                *end = s.end_ms;
            }
            _ => {
                if let Some((a, b, text)) = cur.take() {
                    out.push(timed(text, a, b));
                }
                cur = Some((s.start_ms, s.end_ms, s.text.clone()));
            }
        }
    }
    if let Some((a, b, text)) = cur {
        out.push(timed(text, a, b));
    }
    out
}

fn timed(text: String, start: u64, end: u64) -> Chunk {
    Chunk {
        text,
        t_start_ms: Some(start),
        t_end_ms: Some(end),
        ..Default::default()
    }
}

/// `1:05` or `1:02:03` for a time in milliseconds.
pub fn format_time(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `1:05-1:32`, or just `1:05` when the span is under a second.
pub fn format_span(start_ms: u64, end_ms: Option<u64>) -> String {
    match end_ms {
        Some(e) if e / 1000 > start_ms / 1000 => {
            format!("{}-{}", format_time(start_ms), format_time(e))
        }
        _ => format_time(start_ms),
    }
}

/// Pixel size of a PNG, JPEG, BMP or lossy/lossless WebP file (from its header).
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    if bytes.len() > 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(&bytes[16..20]), be32(&bytes[20..24])));
    }
    if bytes.len() > 26 && bytes.starts_with(b"BM") {
        let w = i32::from_le_bytes(bytes[18..22].try_into().ok()?);
        let h = i32::from_le_bytes(bytes[22..26].try_into().ok()?);
        return Some((w.unsigned_abs(), h.unsigned_abs()));
    }
    if bytes.len() > 30 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        match &bytes[12..16] {
            b"VP8X" => {
                let w = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
                let h = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
                return Some((w, h));
            }
            b"VP8 " => {
                let w = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff;
                let h = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff;
                return Some((w as u32, h as u32));
            }
            b"VP8L" => {
                let b = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
                return Some((1 + (b & 0x3fff), 1 + ((b >> 14) & 0x3fff)));
            }
            _ => return None,
        }
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((w, h));
            }
            if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                i += 2;
                continue;
            }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            i += 2 + len;
        }
    }
    None
}

fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    }
}

/// What a media file gives the index.
#[derive(Debug, Clone)]
pub struct MediaDoc {
    pub chunks: Vec<Chunk>,
    pub content: String,
    pub meta: DocMeta,
    pub size: u64,
    pub sha256: String,
}

/// SHA-256 of a file, read in pieces (media files are large).
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let read_failed = |e: std::io::Error| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    };
    let mut f = std::fs::File::open(path).map_err(read_failed)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(read_failed)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Runs ffmpeg at a low priority and without a window.
async fn ffmpeg_low(args: &[std::ffi::OsString]) -> Result<std::process::Output, String> {
    let ffmpeg =
        crate::audio::ffmpeg_path().ok_or_else(|| String::from(err("audio.ffmpeg_missing")))?;
    let mut cmd = crate::process::hidden_tokio_command(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        cmd.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    crate::orphans::protect(cmd.as_std_mut());
    tokio::time::timeout(std::time::Duration::from_secs(1800), cmd.output())
        .await
        .map_err(|_| String::from(err("audio.convert_timeout")))?
        .map_err(|e| String::from(err("audio.convert_failed").with("error", e)))
}

/// The audio track of a video as 16 kHz mono WAV in a temporary file.
async fn video_audio(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let out = dir.join("audio.wav");
    let args: Vec<std::ffi::OsString> = vec![
        "-i".into(),
        path.as_os_str().to_owned(),
        "-vn".into(),
        "-ar".into(),
        "16000".into(),
        "-ac".into(),
        "1".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-y".into(),
        out.as_os_str().to_owned(),
    ];
    let res = ffmpeg_low(&args).await?;
    if !res.status.success() || !out.is_file() {
        let reason = String::from_utf8_lossy(&res.stderr)
            .lines()
            .next()
            .unwrap_or("no audio track")
            .trim()
            .to_string();
        return Err(err("audio.convert_failed").with("error", reason).into());
    }
    Ok(out)
}

/// Transcribes (audio, video) or reads (image) one media file. `size` and `sha256` are those
/// of the source file.
pub async fn extract_media(
    path: &Path,
    kind: MediaKind,
    asr: Option<&AsrModel>,
    ocr: Option<&OcrModel>,
    cancel: &AtomicBool,
) -> Result<MediaDoc, String> {
    let name = super::extract::file_name(path);
    let owned = path.to_path_buf();
    let (size, sha256) = super::blocking_or_cancel(cancel, move || {
        let size = std::fs::metadata(&owned)
            .map_err(|e| {
                String::from(
                    err("fs.read_failed")
                        .with("path", owned.display())
                        .with("error", e),
                )
            })?
            .len();
        Ok((size, sha256_file(&owned)?))
    })
    .await?;
    if size > kind.max_bytes() {
        return Err(err("rag.media_too_large")
            .with("name", &name)
            .num("max_mib", kind.max_bytes() / (1024 * 1024))
            .into());
    }
    let mut meta = DocMeta {
        media: kind.as_str().into(),
        ..Default::default()
    };
    let (chunks, content) = match kind {
        MediaKind::Audio | MediaKind::Video => {
            let asr =
                asr.ok_or_else(|| String::from(err("rag.media_no_asr").with("name", &name)))?;
            // A video is reduced to its audio track first (temporary file, removed after).
            let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
            let (send_path, send_name) = if kind == MediaKind::Video {
                (video_audio(path, tmp.path()).await?, format!("{name}.wav"))
            } else {
                (path.to_path_buf(), name.clone())
            };
            let bytes = tokio::fs::read(&send_path).await.map_err(|e| {
                String::from(
                    err("fs.read_failed")
                        .with("path", send_path.display())
                        .with("error", e),
                )
            })?;
            if kind == MediaKind::Audio {
                meta.duration_ms =
                    crate::audio::wav_duration_secs(&bytes).map(|s| (s * 1000.0).round() as u64);
            }
            let answer = tokio::select! {
                r = client::transcribe(&asr.endpoint, &asr.alias, &send_name, &bytes) => r?,
                _ = super::wait_cancelled(cancel) => return Err(super::CANCELLED.into()),
            };
            drop(bytes);
            let t = parse_transcript(&answer);
            if t.text.trim().is_empty() {
                return Err(err("rag.media_no_speech").with("name", &name).into());
            }
            meta.duration_ms = t.duration_ms.or(meta.duration_ms);
            meta.language = t.language.clone();
            meta.text_model = Some(asr.alias.clone());
            let chunks = if t.segments.is_empty() {
                chunk::chunk_pages(&[PageText {
                    number: None,
                    text: t.text.clone(),
                }])
            } else {
                meta.segments = Some(t.segments.len());
                chunk_segments(&t.segments)
            };
            (chunks, t.text)
        }
        MediaKind::Image => {
            let bytes = tokio::fs::read(path).await.map_err(|e| {
                String::from(
                    err("fs.read_failed")
                        .with("path", path.display())
                        .with("error", e),
                )
            })?;
            let dims = image_size(&bytes);
            meta.width = dims.map(|d| d.0);
            meta.height = dims.map(|d| d.1);
            let ocr =
                ocr.ok_or_else(|| String::from(err("rag.media_no_ocr").with("name", &name)))?;
            meta.text_model = Some(ocr.alias.clone());
            let read = tokio::select! {
                r = ocr.recognize_image(bytes, mime_of(path)) => r?,
                _ = super::wait_cancelled(cancel) => return Err(super::CANCELLED.into()),
            };
            let mut text = read.trim().to_string();
            let label = match dims {
                Some((w, h)) => format!("Image file {name} ({w}x{h} pixels)."),
                None => format!("Image file {name}."),
            };
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&label);
            let chunks = chunk::chunk_pages(&[PageText {
                number: None,
                text: text.clone(),
            }]);
            (chunks, text)
        }
    };
    if chunks.is_empty() {
        return Err(err("rag.media_no_speech").with("name", &name).into());
    }
    Ok(MediaDoc {
        chunks,
        content,
        meta,
        size,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(media_kind(Path::new("a/Talk.WAV")), Some(MediaKind::Audio));
        assert_eq!(media_kind(Path::new("clip.mkv")), Some(MediaKind::Video));
        assert_eq!(media_kind(Path::new("scan.JPEG")), Some(MediaKind::Image));
        assert_eq!(media_kind(Path::new("notes.md")), None);
    }

    #[test]
    fn verbose_json_gives_timed_segments_and_the_length() {
        let v = json!({
            "text": " Merhaba dünya. Bu bir deneme.",
            "language": "turkish",
            "duration": 7.25,
            "segments": [
                {"id": 0, "start": 0.0, "end": 2.5, "text": " Merhaba dünya."},
                {"id": 1, "start": 2.5, "end": 7.2, "text": " Bu bir deneme."},
                {"id": 2, "start": 7.2, "end": 7.25, "text": "  "}
            ]
        });
        let t = parse_transcript(&v);
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[1].start_ms, 2500);
        assert_eq!(t.duration_ms, Some(7250));
        assert_eq!(t.language.as_deref(), Some("turkish"));
        // Without a duration the end of the last segment is the length.
        let t = parse_transcript(&json!({"segments": [{"start": 1.0, "end": 3.0, "text": "x"}]}));
        assert_eq!((t.duration_ms, t.text.as_str()), (Some(3000), "x"));
        // A plain `json` answer has text only.
        let t = parse_transcript(&json!({"text": "only text"}));
        assert!(t.segments.is_empty() && t.text == "only text");
    }

    #[test]
    fn segments_are_packed_with_their_time_span() {
        let seg = |i: u64, text: &str| Segment {
            start_ms: i * 10_000,
            end_ms: i * 10_000 + 9_000,
            text: text.into(),
        };
        let short = vec![seg(0, "one"), seg(1, "two"), seg(2, "three")];
        let c = chunk_segments(&short);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].t_start_ms, c[0].t_end_ms), (Some(0), Some(29_000)));
        assert_eq!(c[0].text, "one two three");
        let long: Vec<Segment> = (0..30).map(|i| seg(i, &"word ".repeat(40))).collect();
        let c = chunk_segments(&long);
        assert!(c.len() > 3);
        assert!(c.iter().all(|c| c.text.chars().count() <= chunk::MAX_CHARS));
        assert!(c.windows(2).all(|w| w[0].t_end_ms < w[1].t_start_ms));
        assert_eq!(c.last().unwrap().t_end_ms, Some(29 * 10_000 + 9_000));
        assert!(chunk_segments(&[]).is_empty());
    }

    #[test]
    fn times_are_shown_as_minutes_and_hours() {
        assert_eq!(format_time(5_000), "0:05");
        assert_eq!(format_time(65_000), "1:05");
        assert_eq!(format_time(3_723_000), "1:02:03");
        assert_eq!(format_span(65_000, Some(92_000)), "1:05-1:32");
        assert_eq!(format_span(65_000, Some(65_400)), "1:05");
        assert_eq!(format_span(65_000, None), "1:05");
    }

    #[test]
    fn image_sizes_are_read_from_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        png.extend_from_slice(&[0; 8]);
        assert_eq!(image_size(&png), Some((640, 480)));
        // JPEG: SOI, APP0 (len 16), SOF0 with 300 rows and 200 columns.
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        jpg.extend_from_slice(&[0; 14]);
        jpg.extend_from_slice(&[0xff, 0xc0, 0, 17, 8, 1, 44, 0, 200, 3, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(image_size(&jpg), Some((200, 300)));
        assert_eq!(image_size(b"not an image at all, no"), None);
    }
}
