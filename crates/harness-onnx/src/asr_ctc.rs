//! Hugging Face CTC packages, WAV preprocessing and greedy speech recognition.
//! Pure helpers are available without the `engine` feature; inference uses the shared
//! ONNX session builder and serializes runs, including on DirectML.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const ARCHITECTURES: &[&str] = &[
    "Wav2Vec2ForCTC",
    "HubertForCTC",
    "WavLMForCTC",
    "Data2VecAudioForCTC",
    "Wav2Vec2ConformerForCTC",
    "UniSpeechForCTC",
    "UniSpeechSatForCTC",
    "SEWForCTC",
];

pub fn is_ctc_config(config: &Value) -> bool {
    config
        .get("architectures")
        .and_then(Value::as_array)
        .is_some_and(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .any(|n| ARCHITECTURES.contains(&n))
        })
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Finds the audio configs beside the graph or in its repo parent.
pub fn find_root(path: &Path) -> Option<PathBuf> {
    let dir = if path.is_dir() { path } else { path.parent()? };
    [Some(dir), dir.parent()]
        .into_iter()
        .flatten()
        .find(|d| d.join("preprocessor_config.json").is_file() && d.join("config.json").is_file())
        .map(Path::to_path_buf)
}

/// Whether `path` is a CTC speech model: a repo whose `config.json` names a `*ForCTC` head.
/// Vision models and audio classifiers have a `preprocessor_config.json` too.
pub fn is_ctc_model(path: &Path) -> bool {
    find_root(path)
        .and_then(|root| read_json(&root.join("config.json")).ok())
        .is_some_and(|config| is_ctc_config(&config))
}

/// Audio and vocabulary metadata; `tokenizer.json` is never needed.
#[derive(Debug, Clone)]
pub struct CtcPackage {
    pub onnx_path: PathBuf,
    pub root: PathBuf,
    pub sampling_rate: u32,
    pub do_normalize: bool,
    pub return_attention_mask: bool,
    pub blank: usize,
    pub vocab: Vec<String>,
    pub special_tokens: HashSet<String>,
    pub word_delimiter_token: String,
    pub do_lower_case: bool,
}

impl CtcPackage {
    /// Accepts a repo directory or a graph path. External `.onnx_data` files stay
    /// alongside the graph and are read by ONNX Runtime during session creation.
    pub fn discover(path: &Path) -> Result<Self, String> {
        let onnx_path = if path.is_dir() {
            [path.join("onnx/model.onnx"), path.join("model.onnx")]
                .into_iter()
                .find(|p| p.is_file())
                .ok_or("CTC package has no model.onnx or onnx/model.onnx")?
        } else {
            path.to_path_buf()
        };
        if !onnx_path.is_file()
            || !onnx_path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("onnx"))
        {
            return Err(format!("{} is not an ONNX graph file", onnx_path.display()));
        }
        let root = find_root(&onnx_path)
            .ok_or("CTC package needs config.json and preprocessor_config.json")?;
        let config = read_json(&root.join("config.json"))?;
        if !is_ctc_config(&config) {
            return Err(
                "not a supported ForCTC architecture: base audio encoders have no CTC logits head"
                    .into(),
            );
        }
        let size = config
            .get("vocab_size")
            .and_then(Value::as_u64)
            .filter(|&n| n > 0 && n <= 1_000_000)
            .ok_or("CTC config needs a valid vocab_size")? as usize;
        let blank = config
            .get("pad_token_id")
            .and_then(Value::as_u64)
            .filter(|&n| n < size as u64)
            .ok_or("CTC config needs pad_token_id within vocab_size")? as usize;
        let pre = read_json(&root.join("preprocessor_config.json"))?;
        if pre.get("feature_size").and_then(Value::as_u64) != Some(1) {
            return Err("CTC raw audio requires feature_size == 1".into());
        }
        let sampling_rate = pre
            .get("sampling_rate")
            .and_then(Value::as_u64)
            .filter(|&n| n > 0 && n <= 384_000)
            .ok_or("invalid CTC sampling_rate")? as u32;
        let vocabulary = read_json(&root.join("vocab.json"))?;
        let vocabulary = vocabulary
            .as_object()
            .ok_or("vocab.json must map tokens to integer IDs")?;
        let mut vocab = vec![String::new(); size];
        let mut seen = HashSet::new();
        for (token, id) in vocabulary {
            let id = id
                .as_u64()
                .filter(|&n| n < size as u64)
                .ok_or("vocab.json ID exceeds config vocab_size")? as usize;
            if !seen.insert(id) || token.is_empty() {
                return Err("duplicate vocabulary ID or empty token".into());
            }
            vocab[id] = token.clone();
        }
        if seen.len() != size {
            return Err("vocab.json does not match config vocab_size".into());
        }
        let mut package = Self {
            onnx_path,
            root,
            sampling_rate,
            do_normalize: pre
                .get("do_normalize")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            return_attention_mask: pre
                .get("return_attention_mask")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            blank,
            vocab,
            special_tokens: HashSet::new(),
            word_delimiter_token: "|".into(),
            do_lower_case: false,
        };
        for name in ["tokenizer_config.json", "special_tokens_map.json"] {
            let path = package.root.join(name);
            if !path.is_file() {
                continue;
            }
            let cfg = read_json(&path)?;
            for key in ["pad_token", "unk_token", "bos_token", "eos_token"] {
                if let Some(token) = cfg.get(key).and_then(token_content) {
                    package.special_tokens.insert(token.into());
                }
            }
            if let Some(tokens) = cfg
                .get("additional_special_tokens")
                .and_then(Value::as_array)
            {
                for token in tokens.iter().filter_map(token_content) {
                    package.special_tokens.insert(token.into());
                }
            }
            if let Some(token) = cfg.get("word_delimiter_token").and_then(token_content) {
                if token.is_empty() {
                    return Err("word_delimiter_token cannot be empty".into());
                }
                package.word_delimiter_token = token.into();
            }
            if let Some(lower) = cfg.get("do_lower_case").and_then(Value::as_bool) {
                package.do_lower_case = lower;
            }
        }
        // Conventional defaults of the HF CTC tokenizer when configs omit them.
        for token in ["<pad>", "<unk>", "<s>", "</s>"] {
            package.special_tokens.insert(token.into());
        }
        Ok(package)
    }

    /// Collapse repeats before removing the configured blank; concatenate character
    /// tokens directly (including CJK), replacing only the word delimiter with spaces.
    pub fn decode(&self, ids: &[usize]) -> String {
        self.decode_with_previous(ids, None).0
    }

    fn decode_with_previous(
        &self,
        ids: &[usize],
        mut previous: Option<usize>,
    ) -> (String, Option<usize>) {
        let mut text = String::new();
        for &id in ids {
            if previous != Some(id) && id != self.blank {
                if let Some(token) = self
                    .vocab
                    .get(id)
                    .filter(|t| !self.special_tokens.contains(*t))
                {
                    text.push_str(&token.replace(&self.word_delimiter_token, " "));
                }
            }
            previous = Some(id);
        }
        if self.do_lower_case {
            text = text.to_lowercase();
        }
        (
            text.split_whitespace().collect::<Vec<_>>().join(" "),
            previous,
        )
    }

    /// Timed segments from the merged frame IDs (`frame_secs` per frame). A segment ends at
    /// a word delimiter or a pause (a blank run of at least 0.4 s) once it is 8 s long, or at
    /// the next character after 15 s, so subtitles never split a word mid-way and scripts
    /// without word delimiters (CJK) still break at pauses.
    pub fn segments(&self, ids: &[usize], frame_secs: f64) -> Vec<Segment> {
        const TARGET: f64 = 8.0;
        const LIMIT: f64 = 15.0;
        const PAUSE: f64 = 0.4;
        let mut out: Vec<Segment> = Vec::new();
        let mut text = String::new();
        let mut start: Option<usize> = None;
        let mut last = 0usize;
        let mut at_boundary = false;
        let mut previous = None;
        let mut flush = |text: &mut String, start: &mut Option<usize>, last: usize| {
            let mut t = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if self.do_lower_case {
                t = t.to_lowercase();
            }
            if let (Some(s), false) = (*start, t.is_empty()) {
                out.push(Segment {
                    id: out.len(),
                    start: s as f64 * frame_secs,
                    end: (last + 1) as f64 * frame_secs,
                    text: t,
                });
            }
            text.clear();
            *start = None;
        };
        for (frame, &id) in ids.iter().enumerate() {
            let new_token = previous != Some(id) && id != self.blank;
            previous = Some(id);
            let Some(token) = new_token
                .then(|| self.vocab.get(id))
                .flatten()
                .filter(|t| !self.special_tokens.contains(*t))
            else {
                continue;
            };
            let piece = token.replace(&self.word_delimiter_token, " ");
            if piece.trim().is_empty() {
                text.push(' ');
                at_boundary = true;
                continue;
            }
            if let Some(s) = start {
                let length = (frame - s) as f64 * frame_secs;
                let paused = (frame - last) as f64 * frame_secs >= PAUSE;
                if (length >= TARGET && (at_boundary || paused)) || length >= LIMIT {
                    flush(&mut text, &mut start, last);
                }
            }
            start.get_or_insert(frame);
            text.push_str(&piece);
            last = frame;
            at_boundary = piece.ends_with(' ');
        }
        flush(&mut text, &mut start, last);
        out
    }
}

fn token_content(v: &Value) -> Option<&str> {
    v.as_str()
        .or_else(|| v.get("content").and_then(Value::as_str))
}

/// Per-utterance zero mean and unit variance, using HF's epsilon.
pub fn normalize(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let mean = samples.iter().map(|&x| f64::from(x)).sum::<f64>() / samples.len() as f64;
    let variance = samples
        .iter()
        .map(|&x| (f64::from(x) - mean).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    let scale = (variance + 1e-7).sqrt();
    for x in samples {
        *x = ((f64::from(*x) - mean) / scale) as f32;
    }
}

/// Windowed sinc resampling with a low-pass cutoff for downsampling. Output length
/// is rounded to the nearest sample; edge kernels are renormalized to preserve DC.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, String> {
    if from == 0 || to == 0 {
        return Err("sampling rates must be positive".into());
    }
    if from == to || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let ratio = f64::from(to) / f64::from(from);
    let len = (samples.len() as f64 * ratio).round() as usize;
    let cutoff = ratio.min(1.0);
    let radius = (16.0 / cutoff).ceil() as i64;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let position = i as f64 / ratio;
        let center = position.floor() as i64;
        let (mut sum, mut weight) = (0.0, 0.0);
        for j in (center - radius).max(0)..=(center + radius).min(samples.len() as i64 - 1) {
            let distance = position - j as f64;
            let x = std::f64::consts::PI * distance * cutoff;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
            let window = 0.5 * (1.0 + (std::f64::consts::PI * distance / radius as f64).cos());
            let w = sinc * window * cutoff;
            sum += f64::from(samples[j as usize]) * w;
            weight += w;
        }
        out.push((sum / weight) as f32);
    }
    Ok(out)
}

/// Decode RIFF WAV PCM16/24/32 or IEEE float32, averaging all channels.
/// WAVE_FORMAT_EXTENSIBLE with these subformats is also accepted.
pub fn read_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    let invalid = || "invalid or unsupported WAV (need PCM16/24/32 or float32)".to_string();
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(invalid());
    }
    // Pipe writers (including ffmpeg) cannot seek back to fill in chunk sizes.
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let declared = if riff_size == 0 {
        bytes.len()
    } else {
        riff_size.saturating_add(8).min(bytes.len())
    };
    if declared < 12 {
        return Err(invalid());
    }
    let mut pos = 12;
    let (mut fmt, mut data) = (None, Vec::new());
    while pos + 8 <= declared {
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let unknown_data_size =
            &bytes[pos..pos + 4] == b"data" && (size == 0 || size > declared - start);
        let end = if unknown_data_size {
            declared
        } else {
            start
                .checked_add(size)
                .filter(|&e| e <= declared)
                .ok_or_else(invalid)?
        };
        let chunk = &bytes[start..end];
        match &bytes[pos..pos + 4] {
            b"fmt " => fmt = Some(chunk),
            b"data" => data.push((chunk, unknown_data_size)),
            _ => {}
        }
        pos = end + (size & 1);
    }
    let fmt = fmt.filter(|f| f.len() >= 16).ok_or_else(invalid)?;
    let u16_at = |offset| u16::from_le_bytes(fmt[offset..offset + 2].try_into().unwrap());
    let mut kind = u16_at(0);
    if kind == 0xfffe {
        if fmt.len() < 40
            || u16_at(16) < 22
            || fmt[26..40] != [0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]
        {
            return Err(invalid());
        }
        kind = u16_at(24);
    }
    let channels = usize::from(u16_at(2));
    let rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
    let bits = u16_at(14);
    if channels == 0
        || rate == 0
        || !matches!((kind, bits), (1, 16 | 24 | 32) | (3, 32))
        || data.is_empty()
    {
        return Err(invalid());
    }
    let width = usize::from(bits / 8);
    let frame = channels * width;
    if usize::from(u16_at(12)) != frame {
        return Err(invalid());
    }
    let mut samples = Vec::new();
    for (data, unknown_size) in data {
        // chunks_exact clamps streaming data to complete interleaved frames.
        if !unknown_size && data.len() % frame != 0 {
            return Err(invalid());
        }
        for row in data.chunks_exact(frame) {
            let mut mono = 0.0f64;
            for raw in row.chunks_exact(width) {
                let value = match (kind, bits) {
                    (1, 16) => f64::from(i16::from_le_bytes(raw.try_into().unwrap())) / 32768.0,
                    (1, 24) => {
                        f64::from(i32::from_le_bytes([0, raw[0], raw[1], raw[2]]) >> 8) / 8388608.0
                    }
                    (1, 32) => {
                        f64::from(i32::from_le_bytes(raw.try_into().unwrap())) / 2147483648.0
                    }
                    (3, 32) => f64::from(f32::from_le_bytes(raw.try_into().unwrap())),
                    _ => unreachable!(),
                };
                if !value.is_finite() {
                    return Err("WAV contains non-finite samples".into());
                }
                mono += value;
            }
            samples.push((mono / channels as f64) as f32);
        }
    }
    if samples.is_empty() {
        return Err("WAV contains no audio samples".into());
    }
    Ok((samples, rate))
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub id: usize,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transcription {
    pub text: String,
    pub duration: f64,
    pub segments: Vec<Segment>,
}

impl Transcription {
    /// Render approximate chunk segments as SRT or WebVTT.
    pub fn subtitles(&self, vtt: bool) -> Result<String, String> {
        if self.segments.is_empty() {
            return Err("subtitles require segments".into());
        }
        let timestamp = |seconds: f64| {
            let ms = (seconds.max(0.0) * 1000.0).round() as u64;
            format!(
                "{:02}:{:02}:{:02}{}{:03}",
                ms / 3_600_000,
                ms / 60_000 % 60,
                ms / 1000 % 60,
                if vtt { '.' } else { ',' },
                ms % 1000
            )
        };
        let mut out = if vtt {
            "WEBVTT\n\n".into()
        } else {
            String::new()
        };
        for (i, segment) in self.segments.iter().enumerate() {
            out.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                i + 1,
                timestamp(segment.start),
                timestamp(segment.end),
                segment.text
            ));
        }
        Ok(out)
    }
}

/// A chunk's input range and the non-overlapping range of audio it owns.
#[derive(Debug, Clone, Copy)]
#[cfg(any(feature = "engine", test))]
struct Chunk {
    start: usize,
    end: usize,
    keep_start: usize,
    keep_end: usize,
}

#[cfg(any(feature = "engine", test))]
fn chunks(len: usize, rate: u32) -> Vec<Chunk> {
    let size = rate as usize * 20;
    let overlap = rate as usize * 2;
    let mut out = Vec::new();
    let mut start = 0;
    while start < len {
        let end = (start + size).min(len);
        out.push(Chunk {
            start,
            end,
            keep_start: if start == 0 { 0 } else { start + overlap / 2 },
            keep_end: if end == len { len } else { end - overlap / 2 },
        });
        if end == len {
            break;
        }
        start = end - overlap;
    }
    out
}

#[cfg(any(feature = "engine", test))]
fn retained_frames(chunk: Chunk, frames: usize) -> std::ops::Range<usize> {
    let scale = frames as f64 / (chunk.end - chunk.start) as f64;
    let begin = ((chunk.keep_start - chunk.start) as f64 * scale).round() as usize;
    let end = ((chunk.keep_end - chunk.start) as f64 * scale).round() as usize;
    begin.min(frames)..end.min(frames)
}

#[cfg(any(feature = "engine", test))]
fn argmax(row: &[f32]) -> usize {
    // HF's argmax chooses the first ID in a tie.
    let mut best = 0;
    for i in 1..row.len() {
        if row[i] > row[best] {
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_ctc_head_makes_a_ctc_model() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("vit");
        std::fs::create_dir_all(repo.join("onnx")).unwrap();
        std::fs::write(repo.join("preprocessor_config.json"), "{}").unwrap();
        let model = repo.join("onnx").join("model.onnx");
        // A vision model has the same two configs.
        std::fs::write(
            repo.join("config.json"),
            json!({"architectures": ["ViTForImageClassification"]}).to_string(),
        )
        .unwrap();
        assert!(!is_ctc_model(&model));
        std::fs::write(
            repo.join("config.json"),
            json!({"architectures": ["Wav2Vec2ForCTC"]}).to_string(),
        )
        .unwrap();
        assert!(is_ctc_model(&model));
    }

    fn package() -> CtcPackage {
        CtcPackage {
            onnx_path: PathBuf::new(),
            root: PathBuf::new(),
            sampling_rate: 16_000,
            do_normalize: true,
            return_attention_mask: false,
            blank: 3,
            vocab: ["a", "|", "<unk>", "<pad>", "你", "好", "<s>", "</s>", "B"]
                .map(String::from)
                .to_vec(),
            special_tokens: ["<pad>", "<unk>", "<s>", "</s>"]
                .map(String::from)
                .into_iter()
                .collect(),
            word_delimiter_token: "|".into(),
            do_lower_case: false,
        }
    }

    #[test]
    fn decoder_blank_repeats_delimiter_specials_and_cjk() {
        let mut p = package();
        assert_eq!(p.decode(&[0, 0, 3, 0]), "aa");
        assert_eq!(p.decode(&[6, 0, 1, 1, 3, 1, 8, 2, 7]), "a B");
        assert_eq!(p.decode(&[4, 4, 5, 3, 4]), "你好你");
        p.do_lower_case = true;
        assert_eq!(p.decode(&[8]), "b");
        p.word_delimiter_token = "B".into();
        assert_eq!(p.decode(&[0, 8, 0]), "a a");
        assert_eq!(p.decode(&[]), "");
        assert_eq!(argmax(&[1.0, 1.0, -1.0]), 0);
        assert_eq!(argmax(&[-2.0, -1.0, 0.0]), 2);
    }

    #[test]
    fn segments_break_at_words_or_pauses_never_inside_a_word() {
        let p = package();
        // "aa" words separated by "|", one character every 0.1 s (5 frames of 20 ms).
        let mut ids = Vec::new();
        for i in 0..240 {
            ids.push(if i % 3 == 2 { 1 } else { 0 });
            ids.extend(std::iter::repeat_n(3, 4));
        }
        let segments = p.segments(&ids, 0.02);
        assert!(segments.len() >= 2, "{segments:?}");
        let joined: Vec<_> = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined.join(" "), p.decode(&ids));
        assert!(segments
            .iter()
            .all(|s| s.text.split(' ').all(|w| w == "aa")));
        assert!(segments.windows(2).all(|w| w[0].end <= w[1].start));
        // CJK without delimiters: a 1 s pause after 9 s of speech starts a new segment.
        let mut cjk = Vec::new();
        for _ in 0..30 {
            cjk.extend([4, 3, 5]);
            cjk.extend(std::iter::repeat_n(3, 12));
        }
        cjk.extend(std::iter::repeat_n(3, 50));
        cjk.extend([4, 5]);
        let segments = p.segments(&cjk, 0.02);
        assert_eq!(segments.len(), 2, "{segments:?}");
        assert_eq!(segments[1].text, "你好");
        assert!(p.segments(&[3, 3], 0.02).is_empty());
    }

    #[test]
    fn normalization_is_per_utterance_and_constant_audio_is_finite() {
        let mut data = [1.0, 2.0, 3.0, 4.0];
        normalize(&mut data);
        assert!(data.iter().sum::<f32>().abs() < 1e-6);
        assert!((data.iter().map(|x| x * x).sum::<f32>() / 4.0 - 1.0).abs() < 1e-6);
        let mut constant = [0.5; 4];
        normalize(&mut constant);
        assert_eq!(constant, [0.0; 4]);
        normalize(&mut []);
    }

    #[test]
    fn resampling_preserves_length_dc_and_filters_aliases() {
        let dc = vec![0.25; 4410];
        let down = resample(&dc, 44100, 16000).unwrap();
        assert_eq!(down.len(), 1600);
        assert!(down.iter().all(|x| (x - 0.25).abs() < 1e-6));
        assert_eq!(resample(&down, 16000, 44100).unwrap().len(), 4410);
        assert_eq!(resample(&dc, 44100, 44100).unwrap(), dc);
        assert!(resample(&dc, 0, 16000).is_err());
        let high = (0..4410)
            .map(|i| (2.0 * std::f32::consts::PI * 12000.0 * i as f32 / 44100.0).sin())
            .collect::<Vec<_>>();
        let filtered = resample(&high, 44100, 16000).unwrap();
        let rms = (filtered[50..1550].iter().map(|x| x * x).sum::<f32>() / 1500.0).sqrt();
        assert!(rms < 0.01, "out-of-band energy {rms}");
    }

    fn wav(kind: u16, bits: u16, channels: u16, rate: u32, samples: &[u8]) -> Vec<u8> {
        let width = channels * bits / 8;
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(width)).to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        out.extend_from_slice(samples);
        out
    }

    #[test]
    fn stereo_44100_pcm16_to_mono_16000() {
        let raw = (0..4410)
            .flat_map(|_| [16384i16.to_le_bytes(), 0i16.to_le_bytes()].concat())
            .collect::<Vec<_>>();
        let (audio, rate) = read_wav(&wav(1, 16, 2, 44100, &raw)).unwrap();
        assert_eq!(rate, 44100);
        assert_eq!(audio.len(), 4410);
        assert!(audio.iter().all(|&x| x == 0.25));
        assert_eq!(resample(&audio, rate, 16000).unwrap().len(), 1600);
    }

    #[test]
    fn wav_formats_and_malformed_input() {
        for (kind, bits, raw) in [
            (1, 16, vec![0, 128, 0, 64]),
            (1, 24, vec![0, 0, 128, 0, 0, 64]),
            (1, 32, vec![0, 0, 0, 128, 0, 0, 0, 64]),
            (
                3,
                32,
                [(-1.0f32).to_le_bytes(), 0.5f32.to_le_bytes()].concat(),
            ),
        ] {
            let bytes = wav(kind, bits, 1, 16000, &raw);
            assert_eq!(read_wav(&bytes).unwrap().0, [-1.0, 0.5]);
            assert_eq!(read_wav(&bytes[..bytes.len() - 1]).unwrap().0, [-1.0]);
        }
        assert!(read_wav(&wav(3, 32, 1, 16000, &f32::NAN.to_le_bytes())).is_err());
        assert!(read_wav(&wav(1, 16, 0, 16000, &[0, 0])).is_err());
        assert!(read_wav(&wav(1, 16, 1, 0, &[0, 0])).is_err());
        assert!(read_wav(&wav(1, 16, 1, 16000, &[0])).is_err());
        assert!(read_wav(b"not wav").is_err());
    }

    #[test]
    fn wav_pipe_lengths_use_remaining_whole_frames() {
        for riff_size in [0, u32::MAX, 1024] {
            for data_size in [0, u32::MAX, 1024] {
                // Two stereo PCM16 frames followed by an incomplete frame.
                let mut bytes = wav(1, 16, 2, 16000, &[0, 64, 0, 0, 0, 128, 0, 0, 42]);
                bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
                bytes[40..44].copy_from_slice(&data_size.to_le_bytes());
                let (audio, rate) = read_wav(&bytes).unwrap();
                assert_eq!(rate, 16000);
                assert_eq!(audio, [0.25, -0.5]);
            }
        }
        // Unknown lengths do not relax validation of format chunks.
        let mut bytes = wav(1, 16, 1, 16000, &[0, 64]);
        bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_wav(&bytes).is_err());
    }

    #[test]
    fn wav_extensible_and_unknown_padded_chunks() {
        let original = wav(1, 16, 1, 16000, &[0, 64]);
        let mut extended = original[..36].to_vec();
        extended[16..20].copy_from_slice(&40u32.to_le_bytes());
        extended[20..22].copy_from_slice(&0xfffeu16.to_le_bytes());
        extended.extend_from_slice(&22u16.to_le_bytes());
        extended.extend_from_slice(&16u16.to_le_bytes());
        extended.extend_from_slice(&1u32.to_le_bytes());
        extended.extend_from_slice(&[
            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
        ]);
        extended.extend_from_slice(b"JUNK");
        extended.extend_from_slice(&1u32.to_le_bytes());
        extended.extend_from_slice(&[42, 0]);
        extended.extend_from_slice(&original[36..]);
        let size = extended.len() as u32 - 8;
        extended[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(read_wav(&extended).unwrap().0, [0.5]);
        extended[44] = 99;
        assert!(read_wav(&extended).is_err());
    }

    #[test]
    fn chunk_boundaries_keep_each_frame_once_and_decode_after_merging() {
        let plan = chunks(400, 10);
        assert_eq!(plan.len(), 3);
        assert_eq!((plan[0].keep_start, plan[0].keep_end), (0, 190));
        assert_eq!((plan[1].keep_start, plan[1].keep_end), (190, 370));
        assert_eq!((plan[2].keep_start, plan[2].keep_end), (370, 400));
        let ids = (0..400)
            .map(|i| if i == 191 { 3 } else { 0 })
            .collect::<Vec<_>>();
        let mut merged = Vec::new();
        let mut previous = None;
        let mut pieces = Vec::new();
        for chunk in plan {
            let output = &ids[chunk.start..chunk.end];
            let retained = &output[retained_frames(chunk, output.len())];
            let (text, last) = package().decode_with_previous(retained, previous);
            pieces.push(text);
            previous = last;
            merged.extend_from_slice(retained);
        }
        assert_eq!(merged, ids);
        assert_eq!(package().decode(&merged), "aa");
        assert_eq!(pieces.concat(), "aa");
        assert_eq!(chunks(200, 10).len(), 1);
        let tiny_tail = chunks(201, 10);
        assert_eq!(tiny_tail[1].keep_end, 201);
        assert_eq!(retained_frames(tiny_tail[0], 100), 0..95);
    }

    fn write(root: &Path, name: &str, value: Value) {
        std::fs::write(root.join(name), value.to_string()).unwrap();
    }

    #[test]
    fn discovery_without_tokenizer_json_and_rejects_base_encoders_and_vocab_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("onnx")).unwrap();
        std::fs::write(root.join("onnx/model.onnx"), b"fixture").unwrap();
        std::fs::write(root.join("onnx/model.onnx_data"), b"external fixture").unwrap();
        write(
            root,
            "preprocessor_config.json",
            json!({"feature_size":1, "sampling_rate":16000, "do_normalize":false, "return_attention_mask":true}),
        );
        write(root, "vocab.json", json!({"a":0,"<blank>":1,"#":2}));
        write(
            root,
            "tokenizer_config.json",
            json!({"word_delimiter_token":"#", "do_lower_case":true, "pad_token":{"content":"<blank>"}}),
        );
        write(
            root,
            "special_tokens_map.json",
            json!({"unk_token":{"content":"a"}}),
        );
        for architecture in ARCHITECTURES {
            write(
                root,
                "config.json",
                json!({"architectures":[architecture],"pad_token_id":1,"vocab_size":3}),
            );
            let p = CtcPackage::discover(root).unwrap();
            assert_eq!(p.blank, 1);
            assert!(!p.do_normalize);
            assert!(p.return_attention_mask && p.do_lower_case);
            assert_eq!(p.word_delimiter_token, "#");
            assert!(p.special_tokens.contains("a") && p.special_tokens.contains("<blank>"));
            assert_eq!(
                crate::package::library_task(&p.onnx_path),
                Some(crate::package::Task::AsrCtc)
            );
        }
        write(
            root,
            "config.json",
            json!({"architectures":["Wav2Vec2Model"],"pad_token_id":1,"vocab_size":3}),
        );
        assert!(CtcPackage::discover(root)
            .unwrap_err()
            .contains("base audio encoders"));
        write(
            root,
            "config.json",
            json!({"architectures":["Wav2Vec2ForCTC"],"pad_token_id":1,"vocab_size":4}),
        );
        assert!(CtcPackage::discover(root)
            .unwrap_err()
            .contains("vocab_size"));
        write(
            root,
            "config.json",
            json!({"architectures":["Wav2Vec2ForCTC"],"pad_token_id":7,"vocab_size":3}),
        );
        assert!(CtcPackage::discover(root)
            .unwrap_err()
            .contains("pad_token_id"));
    }

    #[test]
    fn subtitles_require_segments_and_have_correct_timestamps() {
        let mut t = Transcription {
            text: "a".into(),
            duration: 20.125,
            segments: Vec::new(),
        };
        assert!(t.subtitles(false).is_err());
        t.segments.push(Segment {
            id: 0,
            start: 0.0,
            end: 20.125,
            text: "a".into(),
        });
        assert_eq!(
            t.subtitles(false).unwrap(),
            "1\n00:00:00,000 --> 00:00:20,125\na\n\n"
        );
        assert_eq!(
            t.subtitles(true).unwrap(),
            "WEBVTT\n\n1\n00:00:00.000 --> 00:00:20.125\na\n\n"
        );
    }

    #[cfg(feature = "engine")]
    #[test]
    #[ignore = "needs HARNESS_TEST_CTC_MODEL and HARNESS_TEST_CTC_WAV local files and ONNX Runtime"]
    fn local_export_transcribes() {
        let (Some(model), Some(wav)) = (
            std::env::var_os("HARNESS_TEST_CTC_MODEL"),
            std::env::var_os("HARNESS_TEST_CTC_WAV"),
        ) else {
            return;
        };
        crate::engine::init_runtime(None).unwrap();
        let options = crate::engine::LoadOptions::new(model.into());
        let engine = load(&options).unwrap();
        let audio = std::fs::read(PathBuf::from(wav)).unwrap();
        assert!(!engine.transcribe(&audio).unwrap().text.is_empty());
    }
}

#[cfg(feature = "engine")]
pub use inference::{load, CtcEngine};

#[cfg(feature = "engine")]
mod inference {
    use super::*;
    use crate::api::ApiError;
    use crate::engine::{build_session, LoadOptions, SessionLock};
    use ort::value::{Tensor, TensorElementType, ValueType};

    pub struct CtcEngine {
        pub package: CtcPackage,
        session: SessionLock,
        mask: bool,
        pub provider: String,
    }

    pub fn load(options: &LoadOptions) -> Result<CtcEngine, String> {
        let package = CtcPackage::discover(&options.model)?;
        let (session, provider) =
            build_session(&package.onnx_path, options.provider, options.threads)?;
        let mut values = false;
        let mut mask = false;
        for input in session.inputs() {
            let expected = match input.name() {
                "input_values" => { values = true; TensorElementType::Float32 },
                "attention_mask" => { mask = true; TensorElementType::Int64 },
                other => return Err(format!("unsupported CTC input {other}; expected input_values and optional attention_mask")),
            };
            if !matches!(input.dtype(), ValueType::Tensor { ty, shape, .. } if *ty == expected && shape.len() == 2 && (shape[0] < 0 || shape[0] == 1))
            {
                return Err(format!(
                    "CTC {} must be {expected:?} [B,N] with B=1 or dynamic",
                    input.name()
                ));
            }
        }
        if !values {
            return Err("CTC graph has no input_values".into());
        }
        let output = session
            .outputs()
            .iter()
            .find(|o| o.name() == "logits")
            .ok_or("graph has no logits output: base audio encoders need a ForCTC head")?;
        if !matches!(output.dtype(), ValueType::Tensor { ty: TensorElementType::Float32 | TensorElementType::Float16, shape, .. }
            if shape.len() == 3 && (shape[2] < 0 || shape[2] as usize == package.vocab.len()))
        {
            return Err(
                "CTC logits must be floating point [B,T,V], with V matching vocab_size".into(),
            );
        }
        let engine = CtcEngine {
            package,
            session: SessionLock::new(session),
            mask,
            provider: provider.into(),
        };
        engine
            .run(&vec![0.0; engine.package.sampling_rate as usize])
            .map_err(|e| e.message)?;
        Ok(engine)
    }

    impl CtcEngine {
        fn run(&self, samples: &[f32]) -> Result<Vec<usize>, ApiError> {
            let error = |e: ort::Error| ApiError::server(format!("CTC inference: {e}"));
            let shape = [1, samples.len()];
            let mut feeds = vec![(
                "input_values".to_string(),
                Tensor::from_array((shape, samples.to_vec()))
                    .map_err(error)?
                    .upcast(),
            )];
            if self.mask {
                feeds.push((
                    "attention_mask".into(),
                    Tensor::from_array((shape, vec![1i64; samples.len()]))
                        .map_err(error)?
                        .upcast(),
                ));
            }
            let mut session = self
                .session
                .lock()
                .map_err(|_| ApiError::server("the model session is poisoned"))?;
            let outputs = session.run(feeds).map_err(error)?;
            let logits = outputs
                .get("logits")
                .ok_or_else(|| ApiError::server("CTC logits missing"))?;
            let (shape, data) = if logits.dtype().tensor_type() == Some(TensorElementType::Float16)
            {
                let (s, d) = logits.try_extract_tensor::<half::f16>().map_err(error)?;
                (s.to_vec(), d.iter().map(|x| x.to_f32()).collect::<Vec<_>>())
            } else {
                let (s, d) = logits.try_extract_tensor::<f32>().map_err(error)?;
                (s.to_vec(), d.to_vec())
            };
            if shape.len() != 3
                || shape[0] != 1
                || shape[1] <= 0
                || shape[2] as usize != self.package.vocab.len()
            {
                return Err(ApiError::server(format!("CTC logits shape {shape:?} does not match vocabulary; a ForCTC head is required")));
            }
            if data.iter().any(|x| !x.is_finite()) {
                return Err(ApiError::server("CTC logits contain non-finite values"));
            }
            Ok(data
                .chunks_exact(self.package.vocab.len())
                .map(argmax)
                .collect())
        }

        /// Transcribe WAV bytes. Chunk timestamps are approximate; no word alignment
        /// or language detection is performed. Normalize the complete utterance before
        /// running overlapping 20-second windows.
        pub fn transcribe(&self, wav: &[u8]) -> Result<Transcription, ApiError> {
            let (samples, rate) = read_wav(wav).map_err(ApiError::invalid)?;
            let mut samples =
                resample(&samples, rate, self.package.sampling_rate).map_err(ApiError::invalid)?;
            if samples.is_empty() {
                return Err(ApiError::invalid(
                    "WAV is shorter than one model audio sample",
                ));
            }
            if self.package.do_normalize {
                normalize(&mut samples);
            }
            let rate = self.package.sampling_rate;
            let duration = samples.len() as f64 / f64::from(rate);
            let mut merged = Vec::new();
            for chunk in chunks(samples.len(), rate) {
                let ids = self.run(&samples[chunk.start..chunk.end])?;
                merged.extend_from_slice(&ids[retained_frames(chunk, ids.len())]);
            }
            let frame_secs = if merged.is_empty() {
                0.0
            } else {
                duration / merged.len() as f64
            };
            Ok(Transcription {
                text: self.package.decode(&merged),
                duration,
                segments: self.package.segments(&merged, frame_secs),
            })
        }
    }
}
