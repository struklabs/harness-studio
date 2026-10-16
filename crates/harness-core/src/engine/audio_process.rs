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
        // RE-USE restores speech at any sample rate; only the chunking is a choice.
        "reuse" => vec![
            n("audio_chunk_duration_sec", 0., 600., 0., false),
            n("audio_chunk_overlap_sec", 0., 60., 1., false),
        ],
        // Speaker turns: how sure a speaker must be, and how short a turn may be (a frame is
        // 10 ms).
        "nemotron_3_diar" | "sortformer_diar" => vec![
            n("speaker_threshold", 0., 1., 0.5, false),
            n("speaker_min_frames", 0., 10000., 0., true),
            n("speaker_pad_frames", 0., 1000., 0., true),
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
        || OPTION_FAMILIES
            .iter()
            .any(|family| options(family).iter().any(|spec| spec.name == key))
}

/// Families that take options.
const OPTION_FAMILIES: &[&str] = &[
    "apollo",
    "audiosr",
    "sam_audio",
    "universr",
    "reuse",
    "nemotron_3_diar",
    "sortformer_diar",
];

/// Whether a multipart text field is a number of some family (the others stay text).
pub fn numeric_option(name: &str) -> bool {
    OPTION_FAMILIES.iter().any(|family| {
        options(family)
            .iter()
            .any(|spec| spec.name == name && spec.choices.is_empty())
    })
}

/// The packages that read 44.1 kHz audio only (the worker refuses another rate).
fn needs_44k(family: &str) -> bool {
    matches!(
        family,
        "apollo" | "bs_roformer" | "mel_band_roformer" | "htdemucs" | "htdemucs_6stems"
    )
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
    } else if message.starts_with("Sortformer accepts at most") {
        err("audio_process.diar_too_long")
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
    if needs_44k(family) && rate != 44100 {
        return Err("This model requires a 44.1 kHz WAV.".into());
    }
    // The diarization models read 16 kHz mono only; any other WAV is converted first.
    let converted;
    let audio = if super::audio_cpp::is_diarization(family) {
        converted = diarization_wav(audio)?;
        &converted[..]
    } else {
        audio
    };
    if family == "sam_audio"
        && crate::audio::wav_duration_secs(audio).unwrap_or(f64::INFINITY) > 400.
    {
        return Err("SAM Audio accepts at most 400 seconds.".into());
    }
    // Sortformer v1 builds a graph that grows with the square of the audio: 2 GB of GPU memory
    // for 16 s, 5 GB for 34 s, 12 GB for 57 s (Nemotron 3 Diarization stays at 250 MB).
    if family == "sortformer_diar"
        && crate::audio::wav_duration_secs(audio).unwrap_or(f64::INFINITY) > SORTFORMER_MAX_SECS
    {
        return Err("Sortformer accepts at most 60 seconds.".into());
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
    if matches!(family, "apollo" | "audiosr" | "reuse") {
        let default = if family == "audiosr" { 15. } else { 0. };
        let chunk = opts
            .get("audio_chunk_duration_sec")
            .and_then(Value::as_f64)
            .unwrap_or(default);
        let overlap = opts
            .get("audio_chunk_overlap_sec")
            .and_then(Value::as_f64)
            .unwrap_or(if family == "audiosr" { 2. } else { 1. });
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

/// Longest audio a Sortformer v1 request may carry (seconds).
pub const SORTFORMER_MAX_SECS: f64 = 60.;

/// Sample rate the diarization models read.
pub const DIARIZATION_RATE: u32 = 16_000;

/// The samples of a validated WAV as mono floats (channels averaged) and its rate.
fn mono_samples(audio: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let (rate, channels) = validate_wav(audio)?;
    let u16at = |p: usize| u16::from_le_bytes([audio[p], audio[p + 1]]);
    let (mut pos, mut bits, mut encoding, mut data) = (12, 0, 0, &audio[..0]);
    while pos + 8 <= audio.len() {
        let size = u32::from_le_bytes(audio[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &audio[pos + 8..pos + 8 + size];
        match &audio[pos..pos + 4] {
            b"fmt " => (encoding, bits) = (u16at(pos + 8), u16at(pos + 22)),
            b"data" => data = body,
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    let width = usize::from(bits / 8);
    let sample = |b: &[u8]| -> f32 {
        match (encoding, bits) {
            (3, _) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            (_, 8) => (f32::from(b[0]) - 128.) / 128.,
            (_, 16) => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.,
            (_, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.,
            _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.,
        }
    };
    let frame = width * usize::from(channels);
    let mono = data
        .chunks_exact(frame)
        .map(|f| f.chunks_exact(width).map(sample).sum::<f32>() / f32::from(channels))
        .collect();
    Ok((rate, mono))
}

/// `samples` at `from` Hz as `to` Hz: windowed-sinc interpolation, low-passed below the lower
/// of the two Nyquist frequencies so that a higher input rate does not fold into the band.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    const LOBES: f64 = 16.;
    let step = f64::from(from) / f64::from(to);
    let cutoff = (1. / step).min(1.);
    let reach = LOBES / cutoff;
    let out_len = (samples.len() as f64 / step).floor() as usize;
    let sinc = |x: f64| {
        if x.abs() < 1e-9 {
            1.
        } else {
            (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
        }
    };
    (0..out_len)
        .map(|i| {
            let t = i as f64 * step;
            let first = (t - reach).ceil().max(0.) as usize;
            let last = ((t + reach).floor() as usize).min(samples.len() - 1);
            let mut sum = 0.;
            for (j, v) in samples.iter().enumerate().take(last + 1).skip(first) {
                let d = j as f64 - t;
                let window = 0.5 * (1. + (std::f64::consts::PI * d / reach).cos());
                sum += f64::from(*v) * cutoff * sinc(d * cutoff) * window;
            }
            sum as f32
        })
        .collect()
}

/// A WAV as the diarization models read it: 16 kHz, mono, 16-bit.
pub fn diarization_wav(audio: &[u8]) -> Result<Vec<u8>, String> {
    let (rate, mono) = mono_samples(audio)?;
    let pcm: Vec<u8> = resample(&mono, rate, DIARIZATION_RATE)
        .iter()
        .flat_map(|v| ((v.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes())
        .collect();
    if pcm.is_empty() {
        return Err("Use a complete PCM or float WAV, at most 32 MiB and 10 minutes.".into());
    }
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + pcm.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(DIARIZATION_RATE.to_le_bytes());
    wav.extend((DIARIZATION_RATE * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    Ok(wav)
}

/// Most speaker turns a diarization answer may carry.
const MAX_TURNS: usize = 100_000;
/// Turns of one speaker closer than this (seconds) are one turn: the models cut a turn at every
/// breath, and Nemotron 3 Diarization even emits runs of 10 ms turns inside one stretch of speech.
const MERGE_GAP_SECS: f64 = 0.3;
/// A turn that is still shorter than this (seconds) after merging is a decoding artifact.
const MIN_TURN_SECS: f64 = 0.08;

/// One stretch of one speaker, in seconds.
struct Turn<'a> {
    start: f64,
    end: f64,
    speaker: &'a str,
    /// Confidence times seconds, summed over the merged turns (`None`: the model gave none).
    weighted: Option<f64>,
}

/// Turns in time order, a speaker's neighbours closer than [`MERGE_GAP_SECS`] joined, the
/// remaining ones shorter than [`MIN_TURN_SECS`] dropped. Only the last emitted turn is
/// considered for merging: in an overlapping A, B, A sequence, B separates the two A turns,
/// so they remain separate even when their times overlap or their gap is small.
fn merge_turns(mut turns: Vec<Turn<'_>>) -> Vec<Turn<'_>> {
    turns.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut merged: Vec<Turn<'_>> = Vec::with_capacity(turns.len());
    for turn in turns {
        match merged.last_mut().filter(|m| m.speaker == turn.speaker) {
            Some(last) if turn.start - last.end <= MERGE_GAP_SECS => {
                last.weighted = match (last.weighted, turn.weighted) {
                    (Some(a), Some(b)) => Some(a + b),
                    _ => None,
                };
                last.end = last.end.max(turn.end);
            }
            _ => merged.push(turn),
        }
    }
    merged.retain(|t| t.end - t.start >= MIN_TURN_SECS);
    merged
}

/// The answer of a diarization worker (`speaker_turns` in samples at `sample_rate`, the rate of
/// the audio it read) as seconds, with speakers named `A`, `B`... in the order they first
/// speak. `None` for anything malformed.
pub fn diarization_result(model: &str, value: &Value, duration_secs: f64) -> Option<Value> {
    let rate = value
        .get("sample_rate")
        .map_or(Some(f64::from(DIARIZATION_RATE)), Value::as_f64)
        .filter(|r| r.is_finite() && *r >= 1000.)?;
    // audio.cpp answers with the timing alone, and no `speaker_turns`, when it finds no speech.
    let list: &[Value] = match value.get("speaker_turns") {
        None if value.get("timing").is_some_and(Value::is_object) => &[],
        turns => turns?.as_array()?,
    };
    if list.len() > MAX_TURNS {
        return None;
    }
    let mut turns = Vec::with_capacity(list.len());
    for turn in list {
        let (start, end) = (
            turn.get("start_sample")?.as_f64()? / rate,
            turn.get("end_sample")?.as_f64()? / rate,
        );
        let speaker = turn.get("speaker_id")?.as_str()?;
        if !(start.is_finite() && end.is_finite() && 0. <= start && start <= end)
            || speaker.is_empty()
            || speaker.len() > 64
            || speaker.chars().any(char::is_control)
        {
            return None;
        }
        // Sortformer v1 reports 0 for every turn: no confidence, rather than a certain zero (a
        // decoded turn passes a threshold, so it is never really 0).
        let confidence = turn
            .get("confidence")
            .and_then(Value::as_f64)
            .filter(|c| *c > 0.);
        turns.push(Turn {
            start,
            end,
            speaker,
            weighted: confidence.map(|c| c.clamp(0., 1.) * (end - start)),
        });
    }
    let round = |v: f64| (v * 1000.).round() / 1000.;
    let mut order: Vec<&str> = Vec::new();
    let segments: Vec<Value> = merge_turns(turns)
        .into_iter()
        .map(|t| {
            let index = order
                .iter()
                .position(|s| *s == t.speaker)
                .unwrap_or_else(|| {
                    order.push(t.speaker);
                    order.len() - 1
                });
            let mut segment = json!({
                "start": round(t.start),
                "end": round(t.end),
                "speaker": crate::asr::diarize::speaker_name(index),
                "speaker_id": t.speaker,
            });
            if let Some(w) = t.weighted {
                let secs = t.end - t.start;
                segment["confidence"] = json!(round(if secs > 0. { w / secs } else { 0. }));
            }
            segment
        })
        .collect();
    Some(json!({
        "model": model,
        "task": "diarization",
        "duration": round(duration_secs),
        "speakers": order.len(),
        "segments": segments,
        "outputs": [],
    }))
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

    /// A 16-bit WAV of a sine tone.
    fn tone(rate: u32, channels: u16, hz: f32, seconds: f32) -> Vec<u8> {
        let frames = (rate as f32 * seconds) as usize;
        let mut pcm = Vec::new();
        for i in 0..frames {
            let v = (f32::sin(i as f32 * hz * std::f32::consts::TAU / rate as f32) * 16000.) as i16;
            for _ in 0..channels {
                pcm.extend(v.to_le_bytes());
            }
        }
        let align = channels * 2;
        let mut b = b"RIFF".to_vec();
        b.extend((36 + pcm.len() as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(channels.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * u32::from(align)).to_le_bytes());
        b.extend(align.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((pcm.len() as u32).to_le_bytes());
        b.extend(pcm);
        b
    }

    fn rms(audio: &[u8]) -> f64 {
        let (_, samples) = mono_samples(audio).unwrap();
        (samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn reuse_takes_any_rate_and_only_chunk_options() {
        // RE-USE is sample-rate independent; Apollo still wants 44.1 kHz.
        let body = rebuild("m", "reuse", &Map::new(), &wav(1)).unwrap();
        assert_eq!(body["request"]["options"], json!({}));
        let fields = json!({"audio_chunk_duration_sec": 10, "audio_chunk_overlap_sec": 2});
        assert!(rebuild("m", "reuse", fields.as_object().unwrap(), &wav(1)).is_ok());
        for fields in [
            json!({"audio_chunk_duration_sec": 10, "audio_chunk_overlap_sec": 6}),
            json!({"edge_pad_duration_sec": 1}),
            json!({"seed": 1}),
            json!({"audio_chunk_duration_sec": 601}),
        ] {
            assert!(rebuild("m", "reuse", fields.as_object().unwrap(), &wav(1)).is_err());
        }
        assert!(request_field("audio_chunk_overlap_sec"));
        assert!(numeric_option("audio_chunk_overlap_sec"));
        assert!(!numeric_option("sampler_mode"));
        assert!(!numeric_option("prompt"));
    }

    #[test]
    fn demucs_and_the_roformers_read_44k_only() {
        for family in [
            "htdemucs",
            "htdemucs_6stems",
            "bs_roformer",
            "mel_band_roformer",
        ] {
            let err = rebuild("m", family, &Map::new(), &wav(1)).unwrap_err();
            assert_eq!(err, "This model requires a 44.1 kHz WAV.", "{family}");
            assert!(rebuild("m", family, &Map::new(), &tone(44100, 2, 440., 0.2)).is_ok());
        }
    }

    #[test]
    fn diarization_audio_is_made_16k_mono_whatever_it_was() {
        for (rate, channels) in [(8000, 1), (16000, 1), (44100, 2), (48000, 1), (22050, 2)] {
            let input = tone(rate, channels, 300., 1.0);
            let body = rebuild("m", "nemotron_3_diar", &Map::new(), &input).unwrap();
            let sent = decode_audio(body["request"]["audio_base64"].as_str().unwrap()).unwrap();
            assert_eq!(
                validate_wav(&sent).unwrap(),
                (16000, 1),
                "{rate} {channels}"
            );
            let samples = mono_samples(&sent).unwrap().1.len();
            assert!((samples as i64 - 16000).abs() <= 1, "{rate}: {samples}");
            // The tone survives (a sine of amplitude 16000/32768 has an RMS of 0.345).
            let level = rms(&sent);
            assert!((level - 0.345).abs() < 0.02, "{rate} {channels}: {level}");
        }
    }

    #[test]
    fn downsampling_filters_what_the_new_rate_cannot_hold() {
        // 10 kHz is above the 8 kHz Nyquist limit of 16 kHz audio: a plain interpolation folds
        // it down to 6 kHz at full strength, the filtered conversion removes it.
        let converted = diarization_wav(&tone(44100, 1, 10000., 1.0)).unwrap();
        assert!(rms(&converted) < 0.03, "{}", rms(&converted));
        // 3 kHz passes.
        let converted = diarization_wav(&tone(44100, 1, 3000., 1.0)).unwrap();
        assert!(rms(&converted) > 0.3, "{}", rms(&converted));
    }

    #[test]
    fn diarization_options_are_bounded() {
        for family in ["nemotron_3_diar", "sortformer_diar"] {
            let fields =
                json!({"speaker_threshold": 0.7, "speaker_min_frames": 5, "speaker_pad_frames": 2});
            let body = rebuild("m", family, fields.as_object().unwrap(), &wav(1)).unwrap();
            assert_eq!(body["request"]["options"], fields);
            assert!(body["request"].get("text").is_none());
            for fields in [
                json!({"speaker_threshold": 1.5}),
                json!({"speaker_min_frames": -1}),
                json!({"speaker_min_frames": 1.5}),
                json!({"speaker_pad_frames": 100000}),
                json!({"num_speakers": 2}),
                json!({"return_frame_probabilities": true}),
            ] {
                assert!(rebuild("m", family, fields.as_object().unwrap(), &wav(1)).is_err());
            }
        }
    }

    #[test]
    fn speaker_turns_become_seconds_with_speakers_named_in_order() {
        let worker = json!({
            "speaker_turns": [
                {"start_sample": 32000, "end_sample": 48000, "speaker_id": "speaker_1", "confidence": 0.9},
                {"start_sample": 0, "end_sample": 16000, "speaker_id": "speaker_0", "confidence": 1.0},
                {"start_sample": 56000, "end_sample": 64000, "speaker_id": "speaker_0"},
            ],
            "timing": {"wall_ms": 12.5, "path": "never return"},
        });
        let result = diarization_result("m", &worker, 4.0).unwrap();
        assert_eq!(result["speakers"], 2);
        assert_eq!(result["duration"], 4.0);
        assert_eq!(result["outputs"], json!([]));
        assert!(!result.to_string().contains("never return"));
        // Time order, and the speaker who speaks first is A whatever the worker numbered them.
        assert_eq!(
            result["segments"],
            json!([
                {"start": 0.0, "end": 1.0, "speaker": "A", "speaker_id": "speaker_0", "confidence": 1.0},
                {"start": 2.0, "end": 3.0, "speaker": "B", "speaker_id": "speaker_1", "confidence": 0.9},
                {"start": 3.5, "end": 4.0, "speaker": "A", "speaker_id": "speaker_0"},
            ])
        );
        // The sample rate the worker reports is the one the times are read at.
        let worker = json!({"sample_rate": 8000, "speaker_turns": [
            {"start_sample": 8000, "end_sample": 16000, "speaker_id": "s"}]});
        assert_eq!(
            diarization_result("m", &worker, 2.0).unwrap()["segments"][0]["end"],
            2.0
        );
        // A model without confidences (Sortformer v1 writes 0) gives none.
        let worker = json!({"speaker_turns": [
            {"start_sample": 0, "end_sample": 16000, "speaker_id": "SPEAKER_00", "confidence": 0}]});
        let result = diarization_result("m", &worker, 1.0).unwrap();
        assert!(result["segments"][0].get("confidence").is_none());
        // No speech at all is an answer, not an error.
        let silent = diarization_result("m", &json!({"speaker_turns": []}), 1.0).unwrap();
        assert_eq!(silent["speakers"], 0);
        assert_eq!(silent["segments"], json!([]));
        // audio.cpp leaves the list out when there is no speech; a non-list is still malformed.
        let silent = diarization_result("m", &json!({"timing": {"wall_ms": 1.0}}), 1.0).unwrap();
        assert_eq!(silent["speakers"], 0);
        assert!(diarization_result("m", &json!({"speaker_turns": 3}), 1.0).is_none());
    }

    #[test]
    fn breath_gaps_and_ten_millisecond_fragments_are_merged() {
        // Nemotron 3 Diarization on the four-speaker test recording: a turn, then 10 ms
        // pieces of the same stretch of speech.
        let turn = |start: u32, end: u32, id: &str| json!({"start_sample": start, "end_sample": end, "speaker_id": id, "confidence": 0.9});
        let worker = json!({"speaker_turns": [
            turn(9600, 66560, "speaker_0"),
            turn(67680, 67840, "speaker_0"),
            turn(69120, 108960, "speaker_0"),
            turn(112320, 172160, "speaker_1"),
            // Same speaker after a real pause: a new turn.
            turn(354560, 397600, "speaker_0"),
            // A lone 10 ms blip of another speaker is dropped.
            turn(400000, 400160, "speaker_2"),
            // Another speaker in between keeps two turns of one speaker apart.
            turn(500000, 510000, "speaker_1"),
            turn(510100, 512000, "speaker_0"),
            turn(512100, 520000, "speaker_1"),
        ]});
        let result = diarization_result("m", &worker, 33.0).unwrap();
        let spans: Vec<(f64, f64, &str)> = result["segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["start"].as_f64().unwrap(),
                    s["end"].as_f64().unwrap(),
                    s["speaker"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            spans,
            [
                (0.6, 6.81, "A"),
                (7.02, 10.76, "B"),
                (22.16, 24.85, "A"),
                (31.25, 31.875, "B"),
                (31.881, 32.0, "A"),
                (32.006, 32.5, "B"),
            ]
        );
        // A merged turn carries the confidence of its parts, weighted by their length.
        let worker = json!({"speaker_turns": [
            {"start_sample": 0, "end_sample": 16000, "speaker_id": "a", "confidence": 1.0},
            {"start_sample": 16000, "end_sample": 48000, "speaker_id": "a", "confidence": 0.7},
        ]});
        let result = diarization_result("m", &worker, 3.0).unwrap();
        assert_eq!(result["segments"].as_array().unwrap().len(), 1);
        assert_eq!(result["segments"][0]["confidence"], 0.8);
    }

    #[test]
    fn a_malformed_diarization_answer_is_refused() {
        for worker in [
            json!({}),
            json!({"speaker_turns": "x"}),
            json!({"speaker_turns": [{"start_sample": 5, "end_sample": 1, "speaker_id": "a"}]}),
            json!({"speaker_turns": [{"start_sample": -1, "end_sample": 1, "speaker_id": "a"}]}),
            json!({"speaker_turns": [{"start_sample": 0, "end_sample": 1}]}),
            json!({"speaker_turns": [{"start_sample": 0, "end_sample": 1, "speaker_id": ""}]}),
            json!({"speaker_turns": [{"start_sample": 0, "end_sample": 1, "speaker_id": "a\u{0}b"}]}),
            json!({"sample_rate": 0, "speaker_turns": []}),
            json!({"sample_rate": "16000", "speaker_turns": []}),
        ] {
            assert!(diarization_result("m", &worker, 1.0).is_none(), "{worker}");
        }
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
        assert_eq!(
            key(&rebuild("m", "sortformer_diar", &Map::new(), &wav(61)).unwrap_err()),
            "audio_process.diar_too_long"
        );
        // Nemotron has no such limit, and a minute is still fine.
        assert!(rebuild("m", "sortformer_diar", &Map::new(), &wav(60)).is_ok());
        assert!(rebuild("m", "nemotron_3_diar", &Map::new(), &wav(61)).is_ok());
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
