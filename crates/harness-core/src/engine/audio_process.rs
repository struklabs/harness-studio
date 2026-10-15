//! Audited v0.9.1 audio processing boundary. No caller paths or arbitrary options.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SECONDS: f64 = 600.0;
pub const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, serde::Serialize)]
pub struct OptionSpec {
    pub name: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub integer: bool,
    pub choices: &'static [&'static str],
}

pub fn options(family: &str) -> Vec<OptionSpec> {
    let n = |name, min, max, default, integer| OptionSpec {
        name,
        min,
        max,
        default,
        integer,
        choices: &[],
    };
    match family {
        "apollo" => vec![
            n("audio_chunk_duration_sec", 0., 600., 0., false),
            n("audio_chunk_overlap_sec", 0., 60., 1., false),
            n("edge_pad_duration_sec", 0., 60., 0., false),
        ],
        "audiosr" => vec![
            n("num_inference_steps", 1., 1000., 50., true),
            n("guidance_scale", 0., 20., 3.5, false),
            n("ddim_eta", 0., 10., 1., false),
            n("audio_chunk_duration_sec", 0.001, 600., 15., false),
            n("audio_chunk_overlap_sec", 0., 60., 2., false),
            n("seed", -1., 2147483647., 42., true),
        ],
        "sam_audio" => vec![
            n("num_inference_steps", 1., 1000., 16., true),
            n("seed", -1., 4294967295., 42., true),
        ],
        "universr" => vec![
            n("audio_chunk_duration_sec", 0., 600., 0., false),
            n("input_sample_rate", 8000., 24000., 16000., true),
            OptionSpec {
                name: "sampler_mode",
                min: 0.,
                max: 0.,
                default: 0.,
                integer: false,
                choices: &["euler", "midpoint", "rk4"],
            },
            n("num_inference_steps", 1., 1000., 4., true),
            n("guidance_scale", 0., 20., 1.5, false),
            n("seed", 0., 4294967295., 42., true),
        ],
        _ => vec![],
    }
}

/// Recognized transport fields and options across processing families.
/// Family-specific admission is checked again after model resolution.
pub fn request_field(key: &str) -> bool {
    matches!(key, "model" | "file" | "audio_base64" | "prompt")
        || ["apollo", "audiosr", "sam_audio", "universr"]
            .iter()
            .any(|family| options(family).iter().any(|spec| spec.name == key))
}

pub fn decode_audio(data: &str) -> Result<Vec<u8>, String> {
    decode_wav(data, MAX_BYTES)
}

pub fn decode_output(data: &str) -> Result<Vec<u8>, String> {
    decode_wav(data, MAX_OUTPUT_BYTES)
}

fn decode_wav(data: &str, limit: usize) -> Result<Vec<u8>, String> {
    let data = data
        .strip_prefix("data:audio/wav;base64,")
        .or_else(|| data.strip_prefix("data:audio/x-wav;base64,"))
        .unwrap_or(data);
    if data.len() > limit.div_ceil(3) * 4 {
        return Err("Audio exceeds the byte limit.".into());
    }
    let audio = STANDARD.decode(data).map_err(|_| "Invalid WAV base64.")?;
    validate_wav_limit(&audio, limit)?;
    Ok(audio)
}

/// Parse complete PCM/float WAV chunks, checking the actual frame count instead
/// of trusting the caller's byte-rate or truncated RIFF/data size declarations.
pub fn validate_wav(audio: &[u8]) -> Result<(u32, u16), String> {
    validate_wav_limit(audio, MAX_BYTES)
}

/// The localized key for a request error of this module (the English text stays the API
/// message). Errors without a key of their own carry their text.
pub fn error_key(message: &str) -> crate::error_key::ErrorKey {
    use crate::error_key::err;
    if message.starts_with("Use a complete PCM") || message == "Audio exceeds the byte limit." {
        err("audio_process.bad_wav")
    } else if message == "This model requires a 44.1 kHz WAV." {
        err("audio_process.needs_44k")
    } else if message.starts_with("SAM Audio accepts at most") {
        err("audio_process.sam_too_long")
    } else {
        err("audio_process.invalid_request").with("message", message)
    }
}

pub fn validate_output(audio: &[u8]) -> Result<(u32, u16), String> {
    validate_wav_limit(audio, MAX_OUTPUT_BYTES)
}

fn validate_wav_limit(audio: &[u8], limit: usize) -> Result<(u32, u16), String> {
    let bad = || "Use a complete PCM or float WAV, at most 32 MiB and 10 minutes.".to_string();
    if audio.len() > limit
        || audio.len() < 44
        || crate::audio::sniff(audio) != crate::audio::Container::Wav
    {
        return Err(bad());
    }
    let u16at = |p| u16::from_le_bytes([audio[p], audio[p + 1]]);
    let u32at = |p| u32::from_le_bytes(audio[p..p + 4].try_into().unwrap());
    if u32at(4) as usize + 8 != audio.len() {
        return Err(bad());
    }
    let mut pos = 12;
    let mut format = None;
    let mut data = None;
    while pos + 8 <= audio.len() {
        let size = u32at(pos + 4) as usize;
        let start = pos + 8;
        let end = start
            .checked_add(size)
            .filter(|e| *e <= audio.len())
            .ok_or_else(bad)?;
        match &audio[pos..pos + 4] {
            b"fmt " if size >= 16 && format.is_none() => {
                let (encoding, channels, rate, align, bits) = (
                    u16at(start),
                    u16at(start + 2),
                    u32at(start + 4),
                    u16at(start + 12),
                    u16at(start + 14),
                );
                if !matches!(encoding, 1 | 3)
                    || !(1..=2).contains(&channels)
                    || !(8000..=192000).contains(&rate)
                    || !(matches!(bits, 8 | 16 | 24 | 32) && (encoding != 3 || bits == 32))
                    || align != channels * (bits / 8)
                    || u32at(start + 8) != rate * u32::from(align)
                {
                    return Err(bad());
                }
                format = Some((rate, channels, align));
            }
            b"data" if data.is_none() => data = Some(size),
            b"fmt " | b"data" => return Err(bad()),
            _ => {}
        }
        pos = end + (size & 1);
    }
    let (rate, channels, align) = format.ok_or_else(bad)?;
    let size = data.ok_or_else(bad)?;
    if pos != audio.len()
        || size == 0
        || size % usize::from(align) != 0
        || size as f64 / f64::from(align) / f64::from(rate) > MAX_SECONDS
    {
        return Err(bad());
    }
    Ok((rate, channels))
}

/// Client options are flat JSON fields (or multipart scalar fields). Rebuild
/// `options` for the worker; never pass through even a nested caller object.
pub fn rebuild(
    model: &str,
    family: &str,
    fields: &Map<String, Value>,
    audio: &[u8],
) -> Result<Value, String> {
    if super::audio_cpp::processing_task(family).is_none() {
        return Err("Unsupported audio processing family.".into());
    }
    let (rate, _) = validate_wav(audio)?;
    if matches!(family, "apollo" | "bs_roformer" | "mel_band_roformer") && rate != 44100 {
        return Err("This model requires a 44.1 kHz WAV.".into());
    }
    if family == "sam_audio"
        && crate::audio::wav_duration_secs(audio).unwrap_or(f64::INFINITY) > 400.
    {
        return Err("SAM Audio accepts at most 400 seconds.".into());
    }
    let specs = options(family);
    let mut opts = Map::new();
    for (key, value) in fields {
        if matches!(key.as_str(), "model" | "audio_base64" | "file") {
            continue;
        }
        if key == "prompt" && family == "sam_audio" {
            continue;
        }
        let spec = specs
            .iter()
            .find(|s| s.name == key)
            .ok_or_else(|| format!("Unknown option `{key}` for {family}."))?;
        if !spec.choices.is_empty() {
            if !value.as_str().is_some_and(|v| spec.choices.contains(&v)) {
                return Err(format!("Invalid `{key}`."));
            }
        } else {
            let n = value
                .as_f64()
                .ok_or_else(|| format!("`{key}` must be a number."))?;
            if !n.is_finite()
                || n < spec.min
                || n > spec.max
                || (spec.integer && value.as_i64().is_none())
            {
                return Err(format!("Invalid `{key}`."));
            }
            if key == "input_sample_rate" && ![8000., 12000., 16000., 24000.].contains(&n) {
                return Err("Invalid input_sample_rate.".into());
            }
        }
        opts.insert(key.clone(), value.clone());
    }
    if family == "universr"
        && !opts.contains_key("input_sample_rate")
        && ![8000, 12000, 16000, 24000].contains(&rate)
    {
        return Err("UniverSR requires input_sample_rate (8000, 12000, 16000, or 24000).".into());
    }
    if matches!(family, "apollo" | "audiosr") {
        let default = if family == "apollo" { 0. } else { 15. };
        let chunk = opts
            .get("audio_chunk_duration_sec")
            .and_then(Value::as_f64)
            .unwrap_or(default);
        let overlap = opts
            .get("audio_chunk_overlap_sec")
            .and_then(Value::as_f64)
            .unwrap_or(if family == "apollo" { 1. } else { 2. });
        if chunk > 0. && overlap > chunk / 2. {
            return Err("Overlap must be at most half the chunk duration.".into());
        }
    }
    let mut request = json!({"audio_base64": STANDARD.encode(audio), "options": opts});
    if family == "sam_audio" {
        let prompt = fields
            .get("prompt")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 1024 && !s.contains('\0'))
            .ok_or("SAM Audio requires a prompt of 1–1024 characters.")?;
        request["text"] = json!(prompt.trim());
    }
    Ok(json!({"model": model, "request": request}))
}

/// Reject traversal and symlinks (including parent symlinks), then canonicalize
/// against an owned worker directory. Never open outside artifacts.
pub fn contained_artifact(root: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Unsafe audio artifact.".into());
    }
    let lexical_root = root.to_path_buf();
    let root = root
        .canonicalize()
        .map_err(|_| "Missing audio output directory.")?;
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let relative = candidate
        .strip_prefix(&root)
        // Windows canonicalization adds a verbatim prefix (\\?\). The
        // worker may return the ordinary absolute spelling of the same root.
        .or_else(|_| candidate.strip_prefix(&lexical_root))
        .map_err(|_| "Unsafe audio artifact.")?;
    let mut checked = root.clone();
    for part in relative.components() {
        checked.push(part);
        if std::fs::symlink_metadata(&checked)
            .map_err(|_| "Missing audio artifact.")?
            .file_type()
            .is_symlink()
        {
            return Err("Unsafe audio artifact.".into());
        }
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| "Missing audio artifact.")?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err("Unsafe audio artifact.".into());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wav(seconds: u32) -> Vec<u8> {
        let size = seconds * 8000 * 2;
        let mut b = b"RIFF".to_vec();
        b.extend((size + 36).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(8000u32.to_le_bytes());
        b.extend(16000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(size.to_le_bytes());
        // A tone, not silence: restoration models reject silent input.
        for i in 0..size / 2 {
            let s = (f32::sin(i as f32 * 440.0 * std::f32::consts::TAU / 8000.0) * 8000.0) as i16;
            b.extend(s.to_le_bytes());
        }
        b
    }

    #[test]
    fn body_is_rebuilt_and_never_forwards_paths_or_caller_model() {
        let fields = json!({"model":"untrusted", "audio_base64":"ignored", "seed":42});
        let body = rebuild("resolved", "audiosr", fields.as_object().unwrap(), &wav(1)).unwrap();
        assert_eq!(body["model"], "resolved");
        assert_eq!(body["request"]["options"], json!({"seed":42}));
        assert_eq!(body["request"].as_object().unwrap().len(), 2);
        assert_eq!(
            decode_audio(body["request"]["audio_base64"].as_str().unwrap()).unwrap(),
            wav(1)
        );
        for field in [
            json!({"unknown":1}),
            json!({"options":{"audio":"secret"}}),
            json!({"seed":{"path":"secret"}}),
            json!({"seed":1.5}),
            json!({"prompt":"ignored"}),
        ] {
            assert!(rebuild("m", "audiosr", field.as_object().unwrap(), &wav(1)).is_err());
        }
        for key in [
            "audio",
            "voice_ref",
            "output",
            "outputs",
            "output_dir",
            "reference_image_path",
            "path",
            "unknown",
            "guidance_scale",
        ] {
            let fields = Map::from_iter([(key.into(), json!("C:/Windows/win.ini"))]);
            let error = rebuild("m", "sam_audio", &fields, &wav(1)).unwrap_err();
            assert!(error.contains(key));
            assert!(!error.contains("C:/Windows/win.ini"));
        }
        assert!(rebuild("m", "speechbrain", &Map::new(), &wav(1)).is_err());
    }

    #[test]
    fn family_prompt_and_option_bounds() {
        for fields in [
            json!({}),
            json!({"prompt":" "}),
            json!({"prompt":"x".repeat(1025)}),
            json!({"prompt":"x", "seed":-2}),
            json!({"prompt":"x", "num_inference_steps":0}),
        ] {
            assert!(rebuild("m", "sam_audio", fields.as_object().unwrap(), &wav(1)).is_err());
        }
        let body = rebuild(
            "m",
            "sam_audio",
            json!({"prompt":" man speaking ", "seed":42})
                .as_object()
                .unwrap(),
            &wav(1),
        )
        .unwrap();
        assert_eq!(body["request"]["text"], "man speaking");
        assert!(body["request"].get("prompt").is_none());
        for fields in [
            json!({"input_sample_rate":44100}),
            json!({"sampler_mode":"path"}),
            json!({"guidance_scale":21}),
        ] {
            assert!(rebuild("m", "universr", fields.as_object().unwrap(), &wav(1)).is_err());
        }
        assert!(rebuild("m", "apollo", &Map::new(), &wav(1)).is_err());
    }

    #[test]
    fn request_errors_have_keys() {
        let key = |m: &str| error_key(m).key();
        assert_eq!(
            key(&validate_wav(b"RIFF").unwrap_err()),
            "audio_process.bad_wav"
        );
        assert_eq!(
            key("Audio exceeds the byte limit."),
            "audio_process.bad_wav"
        );
        assert_eq!(
            key(&rebuild("m", "apollo", &Map::new(), &wav(1)).unwrap_err()),
            "audio_process.needs_44k"
        );
        assert_eq!(
            key("SAM Audio accepts at most 400 seconds."),
            "audio_process.sam_too_long"
        );
        assert_eq!(key("Invalid `x`."), "audio_process.invalid_request");
    }

    #[test]
    fn wav_byte_duration_and_malformed_header_limits() {
        assert!(validate_wav(&wav(600)).is_ok());
        assert!(validate_wav(&wav(601)).is_err());
        assert!(rebuild(
            "m",
            "sam_audio",
            json!({"prompt":"sound"}).as_object().unwrap(),
            &wav(401)
        )
        .is_err());
        assert!(validate_wav(&vec![0; MAX_BYTES + 1]).is_err());
        assert!(decode_audio(&"a".repeat(MAX_BYTES.div_ceil(3) * 4 + 1)).is_err());
        assert!(decode_audio("!").is_err());
        assert!(validate_wav(b"ID3 not WAV").is_err());
        let mut truncated = wav(1);
        truncated.pop();
        assert!(validate_wav(&truncated).is_err());
        let mut rate = wav(601);
        rate[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(validate_wav(&rate).is_err());
        assert_eq!(
            decode_audio(&format!(
                "data:audio/wav;base64,{}",
                STANDARD.encode(wav(1))
            ))
            .unwrap(),
            wav(1)
        );
    }

    #[test]
    fn artifact_containment_rejects_parent_and_sibling_paths() {
        let root = tempfile::tempdir().unwrap();
        let owned = root.path().join("outputs");
        std::fs::create_dir(&owned).unwrap();
        std::fs::write(owned.join("ok.wav"), wav(1)).unwrap();
        std::fs::write(root.path().join("secret.wav"), wav(1)).unwrap();
        assert!(contained_artifact(&owned, Path::new("ok.wav")).is_ok());
        for path in [
            owned.join("../secret.wav"),
            root.path().join("secret.wav"),
            owned.join("missing.wav"),
            owned.clone(),
        ] {
            assert!(contained_artifact(&owned, &path).is_err(), "{path:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn artifact_containment_rejects_symlinks_inside_and_outside() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("ok.wav"), wav(1)).unwrap();
        std::fs::write(other.path().join("secret.wav"), wav(1)).unwrap();
        symlink(root.path().join("ok.wav"), root.path().join("inside.wav")).unwrap();
        symlink(other.path(), root.path().join("outside")).unwrap();
        assert!(contained_artifact(root.path(), Path::new("inside.wav")).is_err());
        assert!(contained_artifact(root.path(), Path::new("outside/secret.wav")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn artifact_containment_rejects_windows_symlinks_when_supported() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("ok.wav"), wav(1)).unwrap();
        if std::os::windows::fs::symlink_file(
            root.path().join("ok.wav"),
            root.path().join("link.wav"),
        )
        .is_ok()
        {
            assert!(contained_artifact(root.path(), Path::new("link.wav")).is_err());
        }
        if std::os::windows::fs::symlink_dir(other.path(), root.path().join("link-dir")).is_ok() {
            std::fs::write(other.path().join("secret.wav"), wav(1)).unwrap();
            assert!(contained_artifact(root.path(), Path::new("link-dir/secret.wav")).is_err());
        }
    }

    /// Explicitly opt in with AUDIOCPP_TEST_BIN, AUDIOCPP_TEST_MODEL, AUDIOCPP_TEST_FAMILY.
    #[tokio::test]
    #[ignore]
    async fn real_audio_cpp_process() {
        use super::super::audio_cpp;
        use crate::process::LaunchConfig;
        let binary = std::env::var_os("AUDIOCPP_TEST_BIN").expect("AUDIOCPP_TEST_BIN");
        let model =
            PathBuf::from(std::env::var_os("AUDIOCPP_TEST_MODEL").expect("AUDIOCPP_TEST_MODEL"));
        let family = std::env::var("AUDIOCPP_TEST_FAMILY").expect("AUDIOCPP_TEST_FAMILY");
        assert!(audio_cpp::processing_task(&family).is_some());
        assert_eq!(
            audio_cpp::metadata(&model)
                .unwrap()
                .audiocpp_family
                .as_deref(),
            Some(family.as_str())
        );
        let dir = tempfile::tempdir().unwrap();
        let port = audio_cpp::random_port().unwrap();
        let config = LaunchConfig {
            model_path: model,
            alias: Some("real-process".into()),
            host: "127.0.0.1".into(),
            port,
            gpu_layers: 0,
            ..Default::default()
        };
        let path = audio_cpp::write_config(&config, dir.path()).unwrap();
        let mut cmd = tokio::process::Command::new(binary);
        cmd.arg("--config")
            .arg(path)
            .args(["--no-ui", "--backend"])
            // AudioSR needs a GPU backend (see audio_cpp::server_config).
            .arg(std::env::var("AUDIOCPP_TEST_BACKEND").unwrap_or_else(|_| "cpu".into()))
            .current_dir(dir.path())
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().unwrap();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}");
        for _ in 0..200 {
            if http
                .get(format!("{url}/health"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            assert!(child.try_wait().unwrap().is_none(), "worker exited");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let mut audio = wav(1);
        if matches!(
            family.as_str(),
            "bs_roformer" | "mel_band_roformer" | "apollo"
        ) {
            audio[24..28].copy_from_slice(&44100u32.to_le_bytes());
            audio[28..32].copy_from_slice(&88200u32.to_le_bytes());
        }
        let fields = if family == "sam_audio" {
            json!({"prompt":"man speaking"})
        } else {
            json!({})
        };
        let body = rebuild("real-process", &family, fields.as_object().unwrap(), &audio).unwrap();
        let response = http
            .post(format!("{url}/v1/tasks/run"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "{}",
            response.text().await.unwrap()
        );
        let value: Value = response.json().await.unwrap();
        if let Some(audio) = value["audio"].as_str() {
            decode_audio(audio).unwrap();
        } else {
            let outputs = value["named_audio_outputs"]
                .as_array()
                .expect("named audio outputs");
            assert!(!outputs.is_empty());
            for output in outputs {
                decode_audio(output["audio"].as_str().unwrap()).unwrap();
            }
        }
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
}
