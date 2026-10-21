//! Reference voices for `POST /v1/audio/speech`: voice cloning.
//!
//! A request may carry a short recording of the voice to speak in, either as JSON
//! (`"voice_ref": {"data": "<base64>", "format": "wav"}`) or as a multipart `voice_ref`
//! file. The recording only ever comes from the request body: any field that names a path on the
//! server is refused. Cloning a real person's voice needs their permission, so the request must
//! also say `"voice_consent": true`. The recording is decoded and checked here (at most
//! [`MAX_BYTES`] and [`MAX_SECONDS`]), handed to the engine as a temporary file that is deleted
//! when the request ends ([`TempVoice`]), and never kept.

use std::path::{Path, PathBuf};

use axum::http::{header, HeaderMap};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use harness_core::audio::{self, Container};
use serde_json::{json, Map, Value};

use crate::errors::GatewayError;
use crate::speech::SpeechRequest;

/// Largest reference recording (as sent) in bytes.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
/// Longest reference recording in seconds.
pub const MAX_SECONDS: f64 = 30.0;
/// Shortest reference recording in seconds: less carries too little of a voice.
pub const MIN_SECONDS: f64 = 1.0;
/// Longest transcript of the reference recording in characters.
pub const MAX_TEXT_CHARS: usize = 1000;
/// Sample rate the reference is decoded to when it is not a WAV (Pocket TTS works at 24 kHz).
const DECODE_RATE: u32 = 24_000;

/// Request fields that name a file or address on the server: never accepted for a reference voice.
const PATH_FIELDS: &[&str] = &[
    "ref_audio",
    "ref_audio_path",
    "reference_audio",
    "reference_path",
    "reference_audio_path",
    "voice_path",
    "voice_file",
    "voice_url",
    "speaker_wav",
    "speaker_audio",
    "audio_prompt_path",
    "prompt_audio",
    "prompt_audio_path",
];

fn bad(message: impl Into<String>) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

fn path_refused(field: &str) -> GatewayError {
    bad(format!(
        "`{field}` is not accepted: a reference voice is never read from a server path. Send the recording itself as `voice_ref` (base64 in JSON or a multipart file)."
    ))
    .param(field.to_string())
}

fn present(v: &Option<Value>) -> bool {
    v.as_ref().is_some_and(|v| !v.is_null())
}

/// Whether the request carries reference audio (`voice_ref`, audio.cpp's field name).
pub fn requested(req: &SpeechRequest) -> bool {
    present(&req.voice_ref)
}

/// Whether the request carries `voice_ref` (the cloning field of this API).
pub fn wants_reference(req: &SpeechRequest) -> bool {
    present(&req.voice_ref)
}

/// The request body without the cloning fields: what an engine's own field check reads.
pub fn without_reference(raw: &Value) -> Vec<u8> {
    let mut raw = raw.clone();
    if let Some(object) = raw.as_object_mut() {
        for key in ["voice_ref", "voice_ref_text", "voice_consent"] {
            object.remove(key);
        }
    }
    serde_json::to_vec(&raw).unwrap_or_default()
}

/// Refuses path-like fields anywhere in the request, with or without reference audio.
pub fn check_raw(raw: &Value) -> Result<(), GatewayError> {
    let Some(object) = raw.as_object() else {
        return Ok(());
    };
    for field in PATH_FIELDS {
        if object.get(*field).is_some_and(|v| !v.is_null()) {
            return Err(path_refused(field));
        }
    }
    Ok(())
}

/// The consent gate: a request with reference audio must say `"voice_consent": true`.
pub fn require_consent(req: &SpeechRequest) -> Result<(), GatewayError> {
    if !requested(req) {
        return Ok(());
    }
    if req.voice_consent == Some(true) {
        return Ok(());
    }
    Err(bad(
        "Cloning a voice needs the speaker's permission: set `voice_consent` to true to confirm you have it (or the voice is your own).",
    )
    .param("voice_consent"))
}

/// A checked reference recording.
#[derive(Debug)]
pub struct Reference {
    /// WAV bytes (the upload itself when it was a WAV, else decoded to 24 kHz mono 16-bit).
    pub wav: Vec<u8>,
    pub seconds: f64,
    /// Transcript of the recording, when given.
    pub text: Option<String>,
}

/// The transcript field (`voice_ref_text`): text of at most [`MAX_TEXT_CHARS`] characters.
pub fn reference_text(req: &SpeechRequest) -> Result<Option<String>, GatewayError> {
    let Some(text) = req.voice_ref_text.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if text.chars().count() > MAX_TEXT_CHARS || text.chars().any(|c| c.is_control() && c != '\n') {
        return Err(bad(format!(
            "`voice_ref_text` must be plain text of at most {MAX_TEXT_CHARS} characters."
        ))
        .param("voice_ref_text"));
    }
    Ok((!text.is_empty()).then(|| text.to_string()))
}

/// The raw bytes and the declared format of `voice_ref`, before any audio is read.
pub fn decode_field(value: &Value) -> Result<(Vec<u8>, Option<String>), GatewayError> {
    let field = "voice_ref";
    let object = value.as_object().ok_or_else(|| {
        bad("`voice_ref` must be an object: {\"type\": \"base64\", \"data\": \"<audio base64>\"}. A file path is never accepted.")
            .param(field)
    })?;
    match object.get("type").and_then(Value::as_str) {
        Some("base64") => {}
        _ => {
            return Err(bad(
                "`voice_ref` only accepts {\"type\": \"base64\", \"data\": \"<audio base64>\"}; a server file path is never accepted.",
            )
            .param(field))
        }
    }
    for key in object.keys() {
        match key.as_str() {
            "type" | "data" | "format" => {}
            "path" | "file" | "url" | "uri" | "filename" => return Err(path_refused(key)),
            other => return Err(bad(format!("Unknown `voice_ref` field `{other}`.")).param(field)),
        }
    }
    let data = object
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("`voice_ref.data` must be base64 text.").param(field))?;
    let data = data.strip_prefix("data:audio/wav;base64,").unwrap_or(data);
    if data.len() > MAX_BYTES / 3 * 4 + 8 {
        return Err(too_big());
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| bad("`voice_ref.data` is not valid base64.").param(field))?;
    let format = match object.get("format") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .ok_or_else(|| bad("`voice_ref.format` must be text.").param(field))?
                .to_ascii_lowercase(),
        ),
    };
    Ok((bytes, format))
}

fn too_big() -> GatewayError {
    bad(format!(
        "The reference recording is larger than {} MiB.",
        MAX_BYTES / (1024 * 1024)
    ))
    .param("voice_ref")
}

/// Checks a reference recording and turns it into a WAV the engine reads. Accepts WAV, MP3, FLAC
/// and Ogg Vorbis (the last three through ffmpeg); the declared `format`, when given, must match
/// the file.
pub async fn check_audio(
    bytes: Vec<u8>,
    declared: Option<&str>,
    text: Option<String>,
) -> Result<Reference, GatewayError> {
    let field = "voice_ref";
    if bytes.is_empty() {
        return Err(bad("The reference recording is empty.").param(field));
    }
    if bytes.len() > MAX_BYTES {
        return Err(too_big());
    }
    let container = audio::sniff(&bytes);
    if !matches!(
        container,
        Container::Wav | Container::Mp3 | Container::Flac | Container::OggVorbis
    ) {
        return Err(
            bad("The reference recording must be a wav, mp3, flac or ogg (Vorbis) file.")
                .param(field),
        );
    }
    if let Some(format) = declared {
        let ok = match format {
            "wav" | "wave" => container == Container::Wav,
            "mp3" => container == Container::Mp3,
            "flac" => container == Container::Flac,
            "ogg" | "oga" => container == Container::OggVorbis,
            _ => {
                return Err(bad(format!(
                    "`voice_ref.format` `{format}` is not supported; use wav, mp3, flac or ogg."
                ))
                .param(field))
            }
        };
        if !ok {
            return Err(bad(format!(
                "`voice_ref.format` says {format} but the recording is {}.",
                container.name()
            ))
            .param(field));
        }
    }
    let wav = if container == Container::Wav {
        bytes
    } else {
        // Decode at most one second past the limit: enough to tell a recording is too long.
        audio::to_wav_with(bytes, DECODE_RATE, Some(MAX_SECONDS as u32 + 1))
            .await
            .map_err(|e| {
                if e.starts_with("audio.ffmpeg_missing") {
                    bad("Reading an mp3, flac or ogg reference needs ffmpeg on this machine; send a wav instead.")
                        .param(field)
                } else {
                    bad("The reference recording could not be decoded.").param(field)
                }
            })?
    };
    let seconds = audio::wav_duration_secs(&wav)
        .filter(|s| s.is_finite())
        .ok_or_else(|| bad("The reference recording is not a readable WAV file.").param(field))?;
    if seconds > MAX_SECONDS {
        return Err(bad(format!(
            "The reference recording is longer than {MAX_SECONDS:.0} seconds."
        ))
        .param(field));
    }
    if seconds < MIN_SECONDS {
        return Err(bad(format!(
            "The reference recording is shorter than {MIN_SECONDS:.0} second."
        ))
        .param(field));
    }
    Ok(Reference { wav, seconds, text })
}

/// Everything about cloning in a request, checked: `None` when the request has no reference
/// audio. Path-like fields and a missing consent are refused first.
pub async fn prepare(raw: &Value, req: &SpeechRequest) -> Result<Option<Reference>, GatewayError> {
    check_raw(raw)?;
    require_consent(req)?;
    let Some(value) = req.voice_ref.as_ref().filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let text = reference_text(req)?;
    let (bytes, format) = decode_field(value)?;
    check_audio(bytes, format.as_deref(), text).await.map(Some)
}

/// A multipart speech request as the JSON the rest of the handler reads: text fields as they
/// are, `speed` as a number, `voice_consent` as a boolean and the `voice_ref` file as the
/// base64 object.
pub async fn multipart_to_json(headers: &HeaderMap, body: Bytes) -> Result<Bytes, GatewayError> {
    use axum::extract::{FromRequest, Multipart};
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .cloned()
        .ok_or_else(|| bad("The multipart body needs a Content-Type header."))?;
    let request = axum::http::Request::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .map_err(|e| bad(e.to_string()))?;
    let mut form = Multipart::from_request(request, &()).await.map_err(|e| {
        bad(format!(
            "The multipart body could not be read: {}",
            e.body_text()
        ))
    })?;
    let mut out = Map::new();
    while let Some(field) = form.next_field().await.map_err(|e| {
        bad(format!(
            "The multipart body could not be read: {}",
            e.body_text()
        ))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "voice_ref" {
            let filename = field.file_name().map(str::to_string);
            let bytes = field.bytes().await.map_err(|e| {
                bad(format!(
                    "The multipart body could not be read: {}",
                    e.body_text()
                ))
            })?;
            if bytes.len() > MAX_BYTES {
                return Err(too_big());
            }
            let mut reference = Map::new();
            reference.insert("type".into(), json!("base64"));
            reference.insert("data".into(), json!(STANDARD.encode(&bytes)));
            if let Some(ext) = filename
                .as_deref()
                .and_then(|f| Path::new(f).extension())
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .filter(|e| ["wav", "mp3", "flac", "ogg"].contains(&e.as_str()))
            {
                reference.insert("format".into(), json!(ext));
            }
            out.insert("voice_ref".into(), Value::Object(reference));
            continue;
        }
        let text = field.text().await.map_err(|e| {
            bad(format!(
                "The multipart body could not be read: {}",
                e.body_text()
            ))
        })?;
        let value = match name.as_str() {
            "speed" => text
                .trim()
                .parse::<f64>()
                .ok()
                .map(|n| json!(n))
                .ok_or_else(|| bad("`speed` must be a number.").param("speed"))?,
            "voice_consent" => json!(text.trim().eq_ignore_ascii_case("true")),
            _ => json!(text),
        };
        out.insert(name, value);
    }
    serde_json::to_vec(&Value::Object(out))
        .map(Bytes::from)
        .map_err(|e| GatewayError::new(500, "gateway_error", e.to_string()))
}

/// A reference recording written into an engine's voice folder for one request: the files are
/// removed when this is dropped, however the request ends.
#[derive(Debug)]
pub struct TempVoice {
    wav: PathBuf,
    text: Option<PathBuf>,
    /// The voice name a worker request uses (the file name without extension).
    pub name: String,
}

impl TempVoice {
    pub fn write(dir: &Path, reference: &Reference) -> Result<Self, GatewayError> {
        let fail = |e: String| GatewayError::new(500, "gateway_error", e);
        std::fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
        let random = harness_core::network::generate_api_key().map_err(fail)?;
        let name = format!(
            "{}{random}",
            harness_core::engine::crispasr_tts::CLONE_PREFIX
        );
        let wav = dir.join(format!("{name}.wav"));
        std::fs::write(&wav, &reference.wav).map_err(|e| fail(e.to_string()))?;
        let mut voice = Self {
            wav,
            text: None,
            name,
        };
        if let Some(text) = &reference.text {
            let path = dir.join(format!("{}.txt", voice.name));
            std::fs::write(&path, text).map_err(|e| fail(e.to_string()))?;
            voice.text = Some(path);
        }
        Ok(voice)
    }
}

impl Drop for TempVoice {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.wav);
        if let Some(text) = &self.text {
            let _ = std::fs::remove_file(text);
        }
    }
}

/// The attestation the gateway passes to an engine that wants a consent statement.
pub const ATTESTATION: &str =
    "The API client confirmed the speaker's permission (voice_consent: true) through the Harness gateway";

#[cfg(test)]
mod tests {
    use super::*;

    fn req(v: Value) -> SpeechRequest {
        serde_json::from_value(v).unwrap()
    }

    /// A mono 16-bit WAV of silence, `secs` long.
    fn wav(secs: f64, rate: u32) -> Vec<u8> {
        let samples = (secs * f64::from(rate)) as u32;
        let data = samples * 2;
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend((36 + data).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(rate.to_le_bytes());
        w.extend((rate * 2).to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(data.to_le_bytes());
        w.extend(vec![0u8; data as usize]);
        w
    }

    fn json_ref(bytes: &[u8], format: Option<&str>) -> Value {
        let mut o = json!({"type": "base64", "data": STANDARD.encode(bytes)});
        if let Some(f) = format {
            o["format"] = json!(f);
        }
        o
    }

    #[test]
    fn consent_is_required_with_reference_audio_only() {
        let plain = req(json!({"input": "Hi"}));
        assert!(!requested(&plain));
        require_consent(&plain).unwrap();
        for consent in [json!(null), json!(false)] {
            let r = req(
                json!({"input": "Hi", "voice_ref": {"type": "base64", "data": "AA=="}, "voice_consent": consent}),
            );
            let e = require_consent(&r).unwrap_err();
            assert_eq!(e.status, 400);
            assert_eq!(e.param.as_deref(), Some("voice_consent"));
        }
        let missing = req(json!({"input": "Hi", "voice_ref": {"type": "base64", "data": "AA=="}}));
        assert_eq!(require_consent(&missing).unwrap_err().status, 400);
        // audio.cpp's older field needs the same consent.
        let old = req(json!({"input": "Hi", "voice_ref": {"type": "base64", "data": "AA=="}}));
        assert_eq!(require_consent(&old).unwrap_err().status, 400);
        let ok = req(
            json!({"input": "Hi", "voice_ref": {"type": "base64", "data": "AA=="}, "voice_consent": true}),
        );
        require_consent(&ok).unwrap();
    }

    #[test]
    fn path_like_fields_are_refused() {
        for field in PATH_FIELDS {
            let raw = json!({"input": "Hi", *field: "/etc/hosts"});
            let e = check_raw(&raw).unwrap_err();
            assert_eq!(e.status, 400, "{field}");
        }
        check_raw(&json!({"input": "Hi", "ref_audio": null})).unwrap();
        for value in [
            json!("/etc/hosts"),
            json!("C:\\Windows\\win.ini"),
            json!("file:///etc/hosts"),
            json!(["a"]),
        ] {
            assert_eq!(decode_field(&value).unwrap_err().status, 400);
        }
        for key in ["path", "file", "url", "uri", "filename"] {
            let value = json!({"type": "base64", "data": "AA==", key: "/etc/hosts"});
            assert_eq!(decode_field(&value).unwrap_err().status, 400, "{key}");
        }
        assert_eq!(
            decode_field(&json!({"type": "base64", "data": "/etc/hosts"}))
                .unwrap_err()
                .status,
            400
        );
        assert_eq!(
            decode_field(&json!({"type": "base64", "data": "AA==", "extra": 1}))
                .unwrap_err()
                .status,
            400
        );
    }

    #[tokio::test]
    async fn size_duration_and_format_limits() {
        let ok = check_audio(wav(3.0, 16_000), Some("wav"), None)
            .await
            .unwrap();
        assert!((ok.seconds - 3.0).abs() < 0.01);
        assert!(check_audio(wav(3.0, 16_000), None, None).await.is_ok());
        // Longer than 30 s.
        assert!(check_audio(wav(31.0, 8_000), None, None).await.is_err());
        assert!(check_audio(wav(30.0, 8_000), None, None).await.is_ok());
        // Shorter than a second.
        assert!(check_audio(wav(0.4, 16_000), None, None).await.is_err());
        // Larger than 10 MiB.
        let big = vec![0u8; MAX_BYTES + 1];
        assert_eq!(check_audio(big, None, None).await.unwrap_err().status, 400);
        let data = "A".repeat(MAX_BYTES / 3 * 4 + 12);
        assert_eq!(
            decode_field(&json!({"type": "base64", "data": data}))
                .unwrap_err()
                .status,
            400
        );
        // Empty, unknown container, wrong declared format.
        assert!(check_audio(Vec::new(), None, None).await.is_err());
        assert!(check_audio(b"not audio at all".to_vec(), None, None)
            .await
            .is_err());
        assert!(check_audio(wav(2.0, 16_000), Some("mp3"), None)
            .await
            .is_err());
        assert!(check_audio(wav(2.0, 16_000), Some("aiff"), None)
            .await
            .is_err());
        // Opus and WebM are not on the list.
        let mut opus = b"OggS".to_vec();
        opus.extend(vec![0u8; 40]);
        opus.extend(b"OpusHead");
        assert!(check_audio(opus, None, None).await.is_err());
        // A header that claims more data than the file has is read by what is there.
        let mut cut = wav(40.0, 8_000);
        cut.truncate(44 + 8_000 * 2);
        assert!((check_audio(cut, None, None).await.unwrap().seconds - 1.0).abs() < 0.01);
    }

    #[test]
    fn base64_and_transcript_are_checked() {
        let bytes = wav(2.0, 16_000);
        let (back, format) = decode_field(&json_ref(&bytes, Some("WAV"))).unwrap();
        assert_eq!(back, bytes);
        assert_eq!(format.as_deref(), Some("wav"));
        assert_eq!(
            decode_field(&json!({"type": "base64", "data": "not base64!"}))
                .unwrap_err()
                .status,
            400
        );
        let r = req(json!({"voice_ref_text": "  Hello there.  "}));
        assert_eq!(reference_text(&r).unwrap().as_deref(), Some("Hello there."));
        let long = req(json!({"voice_ref_text": "a".repeat(MAX_TEXT_CHARS + 1)}));
        assert_eq!(reference_text(&long).unwrap_err().status, 400);
        // Wrong types do not even parse (the request is a 400).
        for bad in [
            json!({"voice_ref_text": 5}),
            json!({"voice_consent": "true"}),
            json!({"voice_consent": 1}),
        ] {
            assert!(serde_json::from_value::<SpeechRequest>(bad).is_err());
        }
        assert_eq!(reference_text(&req(json!({}))).unwrap(), None);
    }

    #[tokio::test]
    async fn prepare_orders_the_checks_and_reads_the_reference() {
        let bytes = wav(2.0, 16_000);
        let body = json!({"input": "Hi", "voice_ref": json_ref(&bytes, Some("wav")), "voice_consent": true, "voice_ref_text": "Hi"});
        let r = req(body.clone());
        let reference = prepare(&body, &r).await.unwrap().unwrap();
        assert_eq!(reference.text.as_deref(), Some("Hi"));
        // No consent: refused before the audio is read.
        let mut no_consent = body.clone();
        no_consent["voice_consent"] = json!(false);
        assert_eq!(
            prepare(&no_consent, &req(no_consent.clone()))
                .await
                .unwrap_err()
                .param
                .as_deref(),
            Some("voice_consent")
        );
        // A path next to a valid reference is still refused.
        let mut path = body.clone();
        path["ref_audio"] = json!("/etc/hosts");
        assert_eq!(
            prepare(&path, &req(path.clone())).await.unwrap_err().status,
            400
        );
        let none = json!({"input": "Hi"});
        assert!(prepare(&none, &req(none.clone())).await.unwrap().is_none());
    }

    #[test]
    fn the_temp_voice_is_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let reference = Reference {
            wav: wav(1.0, 8_000),
            seconds: 1.0,
            text: Some("Hi".into()),
        };
        let voice = TempVoice::write(dir.path(), &reference).unwrap();
        assert!(voice
            .name
            .starts_with(harness_core::engine::crispasr_tts::CLONE_PREFIX));
        assert!(voice
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        let files = || std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(files(), 2);
        drop(voice);
        assert_eq!(files(), 0);
    }

    #[tokio::test]
    async fn multipart_becomes_the_json_body() {
        let wav = wav(2.0, 16_000);
        let boundary = "xyz";
        let mut body = Vec::new();
        for (name, value) in [
            ("model", "pocket"),
            ("input", "Hello"),
            ("speed", "1.25"),
            ("voice_consent", "true"),
        ] {
            body.extend(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                    .into_bytes(),
            );
        }
        body.extend(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"voice_ref\"; filename=\"me.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
                .into_bytes(),
        );
        body.extend(&wav);
        body.extend(format!("\r\n--{boundary}--\r\n").into_bytes());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}")
                .parse()
                .unwrap(),
        );
        let json: Value = serde_json::from_slice(
            &multipart_to_json(&headers, Bytes::from(body))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(json["input"], "Hello");
        assert_eq!(json["speed"], 1.25);
        assert_eq!(json["voice_consent"], true);
        assert_eq!(json["voice_ref"]["format"], "wav");
        let r: SpeechRequest = serde_json::from_value(json.clone()).unwrap();
        let reference = prepare(&json, &r).await.unwrap().unwrap();
        assert_eq!(reference.wav, wav);
        // Without consent the same form is refused.
        let mut denied = json.clone();
        denied["voice_consent"] = json!(false);
        assert!(
            prepare(&denied, &serde_json::from_value(denied.clone()).unwrap())
                .await
                .is_err()
        );
    }
}
