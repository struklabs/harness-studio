//! Speech recognition with a chat model that hears: a llama.cpp model whose projector has an
//! audio encoder (LFM2.5-Audio, Qwen3-ASR, Voxtral, Ultravox, Qwen2.5/3-Omni...). The gateway
//! serves `/v1/audio/transcriptions` with them by sending the audio as an `input_audio` part of a
//! chat completion; this module holds the model-independent parts of that:
//!
//! * which definitions hear ([`projector_of`]),
//! * the instruction per projector family ([`instruction`]; unknown families get a generic one),
//! * cleaning the answer ([`clean`]: Qwen3-ASR's `language X<asr_text>` prefix, reasoning),
//! * cutting long audio into windows ([`windows`]): a chat model repeats itself on a minute of
//!   speech (measured: LFM2.5-Audio wrote one sentence 61 times for a 66 s recording that said
//!   it 10 times), so the audio goes in pieces of at most [`WINDOW_SECS`], cut at the quietest
//!   moment near the end of each piece, each with a token limit for its length ([`max_tokens`]).
//!   The pieces also give the segment times of `verbose_json`, `srt` and `vtt`.

use std::path::Path;

use crate::domain::ModelDefinition;

/// Longest piece of audio one request carries.
pub const WINDOW_SECS: f64 = 25.0;
/// The cut is searched in the last seconds of a window.
const SEARCH_SECS: f64 = 5.0;
/// Energy is compared over frames of this length.
const FRAME_SECS: f64 = 0.1;

/// The audio projector type of a definition that hears: its `mmproj` is a llama.cpp projector
/// with an audio encoder. `None` for every other definition (also a projector without audio).
pub fn projector_of(def: &ModelDefinition) -> Option<String> {
    let base = def.base.local_path()?;
    if crate::engine::ModelFormat::of(base) != crate::engine::ModelFormat::Gguf {
        return None;
    }
    let mmproj = def.mmproj.as_deref()?;
    let meta = crate::models::cached_metadata(mmproj)?;
    meta.has_audio_encoder.then(|| {
        meta.audio_projector
            .clone()
            .unwrap_or_else(|| "audio".into())
    })
}

/// What one transcription request says besides the audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub system: Option<String>,
    pub user: Option<String>,
}

/// Families whose model only transcribes: a translation request is refused instead of returning
/// the transcript as if it were English.
pub fn translates(projector: &str) -> bool {
    !matches!(projector, "lfm2a" | "qwen3asr")
}

/// The instruction for `projector`'s family. `language` (ISO 639-1 or a name) and `prompt`
/// (OpenAI's context text: names, spelling) are added where the family takes them.
pub fn instruction(
    projector: &str,
    translate: bool,
    language: Option<&str>,
    prompt: Option<&str>,
) -> Instruction {
    let language = language.map(str::trim).filter(|l| !l.is_empty());
    let prompt = prompt.map(str::trim).filter(|p| !p.is_empty());
    match projector {
        // LFM2.5-Audio is trained on the system prompt "Perform ASR." (Liquid AI's model card);
        // other wording is not guaranteed to keep it in transcription mode.
        "lfm2a" => Instruction {
            system: Some("Perform ASR.".into()),
            user: None,
        },
        // Qwen3-ASR takes context (names, terms) in the system message and answers
        // `language X<asr_text>...`; the language hint goes with the context.
        "qwen3asr" => {
            let mut system = Vec::new();
            if let Some(l) = language {
                system.push(format!("Language: {l}."));
            }
            if let Some(p) = prompt {
                system.push(p.to_string());
            }
            Instruction {
                system: (!system.is_empty()).then(|| system.join("\n")),
                user: None,
            }
        }
        _ => {
            let mut text = if translate {
                "Translate the speech into English. Answer with the translation only.".to_string()
            } else {
                "Transcribe the audio verbatim. Answer with the transcript only.".to_string()
            };
            if let Some(l) = language {
                text.push_str(&format!(" The speech is in {l}."));
            }
            if let Some(p) = prompt {
                text.push_str(&format!(" Context: {p}"));
            }
            Instruction {
                system: None,
                user: Some(text),
            }
        }
    }
}

/// The chat messages of one window: the instruction and the audio (`format`: `wav` or `mp3`).
pub fn messages(instruction: &Instruction, audio_base64: &str, format: &str) -> serde_json::Value {
    let mut content = Vec::new();
    if let Some(text) = &instruction.user {
        content.push(serde_json::json!({ "type": "text", "text": text }));
    }
    content.push(serde_json::json!({
        "type": "input_audio",
        "input_audio": { "data": audio_base64, "format": format },
    }));
    let mut messages = Vec::new();
    if let Some(system) = &instruction.system {
        messages.push(serde_json::json!({ "role": "system", "content": system }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": content }));
    serde_json::Value::Array(messages)
}

/// Token limit of a window of `secs` seconds: room for fast speech in a token-hungry language,
/// not for a model that repeats itself.
pub fn max_tokens(secs: f64) -> u32 {
    (secs.max(1.0) * 12.0).ceil() as u32 + 48
}

/// The transcript of an answer and the language the model named, if any: reasoning blocks and
/// Qwen3-ASR's `language English<asr_text>` prefix are removed.
pub fn clean(answer: &str) -> (String, Option<String>) {
    let mut text = answer.to_string();
    while let (Some(start), Some(end)) = (text.find("<think>"), text.find("</think>")) {
        if end < start {
            break;
        }
        text.replace_range(start..end + "</think>".len(), "");
    }
    let mut language = None;
    if let Some(pos) = text.find("<asr_text>") {
        let head = text[..pos].trim();
        if let Some(l) = head.strip_prefix("language") {
            let l = l.trim();
            if !l.is_empty() && !l.eq_ignore_ascii_case("none") {
                language = Some(l.to_string());
            }
        }
        text = text[pos + "<asr_text>".len()..].to_string();
    }
    (text.trim().to_string(), language)
}

/// 16-bit PCM of a WAV: mono samples (channels averaged) and the sample rate. `None` for other
/// encodings (float, 24-bit...), which go through ffmpeg first.
pub fn pcm16(wav: &[u8]) -> Option<(Vec<i16>, u32)> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= wav.len() {
            let tag = u16::from_le_bytes([wav[body], wav[body + 1]]);
            let channels = u16::from_le_bytes([wav[body + 2], wav[body + 3]]);
            let rate = u32::from_le_bytes(wav[body + 4..body + 8].try_into().ok()?);
            let bits = u16::from_le_bytes([wav[body + 14], wav[body + 15]]);
            fmt = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = fmt?;
            // 1 = PCM; 0xFFFE = WAVE_FORMAT_EXTENSIBLE (PCM sub-format for 16-bit files).
            if !(tag == 1 || tag == 0xFFFE) || bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
            let end = (body + size).min(wav.len());
            let frame = usize::from(channels) * 2;
            let samples = wav[body..end]
                .chunks_exact(frame)
                .map(|f| {
                    let sum: i32 = f
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|s| i32::from(i16::from_le_bytes(*s)))
                        .sum();
                    (sum / i32::from(channels)) as i16
                })
                .collect();
            return Some((samples, rate));
        }
        pos = body + size + (size & 1);
    }
    None
}

/// A mono 16-bit WAV of `samples`.
pub fn wav_of(samples: &[i16], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Sample ranges of at most [`WINDOW_SECS`]: each cut is at the quietest frame of the last
/// [`SEARCH_SECS`] of its window, so words are rarely split. Short audio is one window.
pub fn windows(samples: &[i16], rate: u32) -> Vec<(usize, usize)> {
    let rate = rate.max(1) as usize;
    let max = (WINDOW_SECS * rate as f64) as usize;
    let search = (SEARCH_SECS * rate as f64) as usize;
    let frame = ((FRAME_SECS * rate as f64) as usize).max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while samples.len() - start > max {
        let end = start + max;
        let from = end - search;
        let mut best = (u64::MAX, end);
        let mut pos = from;
        while pos + frame <= end {
            let energy: u64 = samples[pos..pos + frame]
                .iter()
                .map(|s| (i64::from(*s) * i64::from(*s)) as u64)
                .sum();
            if energy < best.0 {
                best = (energy, pos + frame / 2);
            }
            pos += frame;
        }
        out.push((start, best.1));
        start = best.1;
    }
    if start < samples.len() || out.is_empty() {
        out.push((start, samples.len()));
    }
    out
}

/// Whether a file is a llama.cpp projector with an audio encoder (the library's pairing and the
/// Transcribe panel's list use the same test).
pub fn is_audio_projector(path: &Path) -> bool {
    crate::models::cached_metadata(path).is_some_and(|m| m.has_audio_encoder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_follow_the_family() {
        let lfm = instruction("lfm2a", false, Some("en"), Some("Harness"));
        assert_eq!(lfm.system.as_deref(), Some("Perform ASR."));
        assert_eq!(lfm.user, None);
        let qwen = instruction("qwen3asr", false, Some("tr"), Some("Harness Studio"));
        assert_eq!(
            qwen.system.as_deref(),
            Some("Language: tr.\nHarness Studio")
        );
        assert_eq!(qwen.user, None);
        assert_eq!(instruction("qwen3asr", false, None, None).system, None);
        let other = instruction("voxtral", false, Some("de"), None);
        assert_eq!(other.system, None);
        assert_eq!(
            other.user.as_deref(),
            Some("Transcribe the audio verbatim. Answer with the transcript only. The speech is in de.")
        );
        let tr = instruction("ultravox", true, None, Some("  "));
        assert!(tr
            .user
            .unwrap()
            .starts_with("Translate the speech into English."));
        assert!(!translates("lfm2a") && !translates("qwen3asr") && translates("voxtral"));
    }

    #[test]
    fn messages_put_the_audio_last() {
        let m = messages(&instruction("lfm2a", false, None, None), "QUJD", "wav");
        assert_eq!(m[0]["role"], "system");
        assert_eq!(m[1]["content"][0]["type"], "input_audio");
        assert_eq!(m[1]["content"][0]["input_audio"]["format"], "wav");
        let m = messages(&instruction("x", false, None, None), "QUJD", "mp3");
        assert_eq!(m.as_array().unwrap().len(), 1);
        assert_eq!(m[0]["content"][0]["type"], "text");
        assert_eq!(m[0]["content"][1]["input_audio"]["data"], "QUJD");
    }

    #[test]
    fn answers_are_cleaned() {
        assert_eq!(
            clean("language English<asr_text>The quick brown fox."),
            ("The quick brown fox.".into(), Some("English".into()))
        );
        assert_eq!(clean("language None<asr_text>"), (String::new(), None));
        assert_eq!(
            clean("<think>hmm</think>\n Hello there. "),
            ("Hello there.".into(), None)
        );
        assert_eq!(clean("Plain text."), ("Plain text.".into(), None));
    }

    #[test]
    fn token_limits_grow_with_the_window() {
        assert_eq!(max_tokens(0.2), 60);
        assert_eq!(max_tokens(25.0), 348);
    }

    #[test]
    fn wav_round_trip_and_stereo_mixdown() {
        let samples = [0i16, 1000, -1000, i16::MAX];
        let wav = wav_of(&samples, 16000);
        assert_eq!(pcm16(&wav), Some((samples.to_vec(), 16000)));
        assert_eq!(crate::audio::wav_duration_secs(&wav), Some(4.0 / 16000.0));
        // Stereo: channels averaged.
        let mut stereo = wav_of(&[100, 300, -50, 50], 8000);
        stereo[22] = 2; // channels
        assert_eq!(pcm16(&stereo), Some((vec![200, 0], 8000)));
        // 32-bit float: not read here.
        let mut float = wav.clone();
        float[20] = 3;
        assert_eq!(pcm16(&float), None);
        assert_eq!(pcm16(b"not a wav"), None);
    }

    #[test]
    fn long_audio_is_cut_at_the_quietest_moment() {
        let rate = 1000;
        // 60 s of noise with silence from 22.0 to 22.3 s and from 45.0 to 45.3 s.
        let mut s: Vec<i16> = (0..60 * rate)
            .map(|i| if i % 2 == 0 { 8000 } else { -8000 })
            .collect();
        for quiet in [22_000..22_300, 45_000..45_300] {
            for x in &mut s[quiet] {
                *x = 0;
            }
        }
        let w = windows(&s, rate as u32);
        assert_eq!(w.len(), 3, "{w:?}");
        assert!((22_000..22_300).contains(&w[0].1), "{w:?}");
        assert!((45_000..45_300).contains(&w[1].1), "{w:?}");
        assert_eq!(w[2].1, s.len());
        assert!(w
            .iter()
            .all(|(a, b)| (b - a) as f64 <= WINDOW_SECS * rate as f64));
        // Short audio: one window; empty audio: one empty window.
        assert_eq!(windows(&s[..5000], rate as u32), vec![(0, 5000)]);
        assert_eq!(windows(&[], rate as u32), vec![(0, 0)]);
    }
}
