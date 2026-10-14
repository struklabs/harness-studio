//! Hugging Face audio classifiers as transformers.js / Optimum export them: the head is read
//! from `config.json` (`*ForAudioClassification`, `*ForSequenceClassification` of a speech
//! encoder), the features from `preprocessor_config.json`. Two front ends: the raw waveform
//! (wav2vec2, HuBERT, WavLM: `Wav2Vec2FeatureExtractor`) and Kaldi log-mel filter banks (the
//! Audio Spectrogram Transformer: `ASTFeatureExtractor`). Pure code: the session lives in
//! [`crate::audio_engine`].

use crate::package::labels_from_config;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Speech encoders whose `*ForSequenceClassification` head takes audio, not text.
const SPEECH_MODEL_TYPES: &[&str] = &[
    "wav2vec2",
    "wav2vec2-conformer",
    "hubert",
    "wavlm",
    "data2vec-audio",
    "unispeech",
    "unispeech-sat",
    "sew",
    "sew-d",
];

/// What the graph takes as `input_values`.
#[derive(Debug, Clone, PartialEq)]
pub enum Features {
    /// The waveform at `sampling_rate`, zero-mean unit-variance when `normalize`.
    Raw { normalize: bool },
    /// Kaldi log-mel filter banks (25 ms Hann frames every 10 ms), padded or cut to
    /// `max_length` frames, then `(x - mean) / (2 * std)` when `normalize`.
    Fbank {
        mel_bins: usize,
        max_length: usize,
        mean: f32,
        std: f32,
        normalize: bool,
    },
}

#[derive(Debug, Clone)]
pub struct AudioPackage {
    pub onnx_path: PathBuf,
    pub root: PathBuf,
    pub labels: Vec<String>,
    pub sampling_rate: u32,
    pub features: Features,
    /// Display name: the repo folder.
    pub name: String,
    pub notes: Vec<String>,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Whether a `config.json` is an audio classifier.
pub fn is_audio_classifier(config: &Value) -> bool {
    let heads: Vec<&str> = config
        .get("architectures")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if heads.iter().any(|n| n.ends_with("ForAudioClassification")) {
        return true;
    }
    let speech = config
        .get("model_type")
        .and_then(Value::as_str)
        .is_some_and(|t| SPEECH_MODEL_TYPES.contains(&t));
    speech
        && heads
            .iter()
            .any(|n| n.ends_with("ForSequenceClassification"))
}

/// The front end of a `preprocessor_config.json`; `None` for one this crate cannot compute.
pub fn parse_features(v: &Value) -> Option<Features> {
    let kind = v
        .get("feature_extractor_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let normalize = v.get("do_normalize").and_then(Value::as_bool);
    match kind {
        "Wav2Vec2FeatureExtractor" => Some(Features::Raw {
            normalize: normalize.unwrap_or(true),
        }),
        "ASTFeatureExtractor" => {
            let f32_of = |k: &str, d: f32| v.get(k).and_then(Value::as_f64).map_or(d, |x| x as f32);
            let usize_of = |k: &str, d: usize| {
                v.get(k)
                    .and_then(Value::as_u64)
                    .map_or(d, |x| x as usize)
                    .max(1)
            };
            Some(Features::Fbank {
                mel_bins: usize_of("num_mel_bins", 128),
                max_length: usize_of("max_length", 1024),
                mean: f32_of("mean", -4.267_739_3),
                std: f32_of("std", 4.568_997_4),
                normalize: normalize.unwrap_or(true),
            })
        }
        _ => None,
    }
}

/// The folder that holds `preprocessor_config.json` for a model file (its folder or parent).
fn find_root(onnx: &Path) -> Option<PathBuf> {
    onnx.ancestors()
        .skip(1)
        .take(2)
        .find(|d| d.join("preprocessor_config.json").is_file())
        .map(Path::to_path_buf)
}

impl AudioPackage {
    /// Reads the repo around `onnx_path`; `None` when it is not an audio classifier this
    /// crate serves.
    pub fn discover(onnx_path: &Path) -> Option<Self> {
        if !onnx_path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        {
            return None;
        }
        let root = find_root(onnx_path)?;
        let config = read_json(&root.join("config.json"))?;
        if !is_audio_classifier(&config) {
            return None;
        }
        let processor = read_json(&root.join("preprocessor_config.json"))?;
        let features = parse_features(&processor)?;
        let sampling_rate = processor
            .get("sampling_rate")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .unwrap_or(16_000);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "onnx-model".to_string());
        Some(AudioPackage {
            onnx_path: onnx_path.to_path_buf(),
            root,
            labels: labels_from_config(&config),
            sampling_rate,
            notes: vec![
                "task audio-classification (config.json architectures)".into(),
                format!("features {:?} at {} Hz", features, sampling_rate),
            ],
            features,
            name,
        })
    }
}

/// In-place radix-2 FFT of `re` + i`im` (length a power of two).
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

fn mel(freq: f64) -> f64 {
    1127.0 * (1.0 + freq / 700.0).ln()
}

/// Kaldi's triangular mel filters over the first `fft_size / 2` FFT bins (20 Hz to Nyquist).
fn mel_banks(mel_bins: usize, fft_size: usize, rate: u32) -> Vec<Vec<f64>> {
    let nyquist = f64::from(rate) / 2.0;
    let (low, high) = (mel(20.0), mel(nyquist));
    let delta = (high - low) / (mel_bins + 1) as f64;
    let bin_width = f64::from(rate) / fft_size as f64;
    (0..mel_bins)
        .map(|m| {
            let left = low + m as f64 * delta;
            let (center, right) = (left + delta, left + 2.0 * delta);
            (0..fft_size / 2)
                .map(|i| {
                    let f = mel(bin_width * i as f64);
                    let up = (f - left) / (center - left);
                    let down = (right - f) / (right - center);
                    up.min(down).max(0.0)
                })
                .collect()
        })
        .collect()
}

/// `torchaudio.compliance.kaldi.fbank` as the AST feature extractor calls it: 25 ms frames every
/// 10 ms (edges snipped), DC removed, pre-emphasis 0.97, a symmetric Hann window, a 512-point
/// power spectrum, `mel_bins` mel energies, natural log floored at f32 epsilon. Rows are frames.
pub fn kaldi_fbank(samples: &[f32], rate: u32, mel_bins: usize) -> Vec<Vec<f32>> {
    let frame = (f64::from(rate) * 0.025).round() as usize;
    let shift = (f64::from(rate) * 0.010).round() as usize;
    if frame == 0 || shift == 0 || samples.len() < frame {
        return Vec::new();
    }
    let fft_size = frame.next_power_of_two();
    let banks = mel_banks(mel_bins, fft_size, rate);
    let window: Vec<f64> = (0..frame)
        .map(|n| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / (frame - 1) as f64).cos())
        .collect();
    let floor = f64::from(f32::EPSILON);
    let frames = 1 + (samples.len() - frame) / shift;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let chunk = &samples[f * shift..f * shift + frame];
        let mean = chunk.iter().map(|&x| f64::from(x)).sum::<f64>() / frame as f64;
        let centered: Vec<f64> = chunk.iter().map(|&x| f64::from(x) - mean).collect();
        let mut re = vec![0.0; fft_size];
        let mut im = vec![0.0; fft_size];
        for i in 0..frame {
            let prev = centered[i.saturating_sub(1)];
            re[i] = (centered[i] - 0.97 * prev) * window[i];
        }
        fft(&mut re, &mut im);
        let power: Vec<f64> = (0..fft_size / 2)
            .map(|i| re[i] * re[i] + im[i] * im[i])
            .collect();
        out.push(
            banks
                .iter()
                .map(|bank| {
                    let e: f64 = bank.iter().zip(&power).map(|(w, p)| w * p).sum();
                    e.max(floor).ln() as f32
                })
                .collect(),
        );
    }
    out
}

/// The AST input: `max_length` rows of `mel_bins` (zero rows pad a short clip, a long one is
/// cut), normalized with the dataset statistics.
pub fn ast_input(
    samples: &[f32],
    rate: u32,
    mel_bins: usize,
    max_length: usize,
    mean: f32,
    std: f32,
    normalize: bool,
) -> Vec<f32> {
    let bank = kaldi_fbank(samples, rate, mel_bins);
    let mut out = vec![0.0f32; max_length * mel_bins];
    for (row, values) in bank.iter().take(max_length).enumerate() {
        out[row * mel_bins..(row + 1) * mel_bins].copy_from_slice(values);
    }
    if normalize {
        for x in &mut out {
            *x = (*x - mean) / (std * 2.0);
        }
    }
    out
}

/// The pipeline task of a model file, for the library.
pub fn pipeline_task_of(onnx_path: &Path) -> Option<&'static str> {
    AudioPackage::discover(onnx_path).map(|_| "audio-classification")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn heads_of_audio_classifiers() {
        assert!(is_audio_classifier(
            &json!({"architectures": ["ASTForAudioClassification"]})
        ));
        assert!(is_audio_classifier(&json!({
            "architectures": ["Wav2Vec2ForSequenceClassification"], "model_type": "wav2vec2"
        })));
        // A text classifier has the same head name.
        assert!(!is_audio_classifier(&json!({
            "architectures": ["BertForSequenceClassification"], "model_type": "bert"
        })));
        assert!(!is_audio_classifier(
            &json!({"architectures": ["Wav2Vec2ForCTC"], "model_type": "wav2vec2"})
        ));
    }

    #[test]
    fn front_ends_of_common_extractors() {
        assert_eq!(
            parse_features(
                &json!({"feature_extractor_type": "Wav2Vec2FeatureExtractor", "do_normalize": false})
            ),
            Some(Features::Raw { normalize: false })
        );
        assert_eq!(
            parse_features(&json!({
                "feature_extractor_type": "ASTFeatureExtractor", "max_length": 1024,
                "mean": -4.2677393, "num_mel_bins": 128, "std": 4.5689974
            })),
            Some(Features::Fbank {
                mel_bins: 128,
                max_length: 1024,
                mean: -4.267_739_3,
                std: 4.568_997_4,
                normalize: true
            })
        );
        assert_eq!(
            parse_features(&json!({"feature_extractor_type": "WhisperFeatureExtractor"})),
            None
        );
    }

    #[test]
    fn fft_matches_a_direct_transform() {
        let n = 16;
        let x: Vec<f64> = (0..n).map(|i| ((i * 7) % 5) as f64 - 2.0).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0; n]);
        fft(&mut re, &mut im);
        for k in 0..n {
            let (mut r, mut i) = (0.0, 0.0);
            for (t, v) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (k * t) as f64 / n as f64;
                r += v * a.cos();
                i += v * a.sin();
            }
            assert!((re[k] - r).abs() < 1e-9 && (im[k] - i).abs() < 1e-9);
        }
    }

    #[test]
    fn fbank_frames_and_tone_energy() {
        let rate = 16_000;
        // One second of 1 kHz: 98 frames; the mel bin around 1 kHz holds the most energy.
        let tone: Vec<f32> = (0..rate)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let bank = kaldi_fbank(&tone, rate, 128);
        assert_eq!(bank.len(), 98);
        assert_eq!(bank[0].len(), 128);
        let row = &bank[50];
        let peak = (0..128).max_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap();
        let low = mel(20.0);
        let delta = (mel(8000.0) - low) / 129.0;
        let center_hz = |m: usize| 700.0 * (((low + (m + 1) as f64 * delta) / 1127.0).exp() - 1.0);
        assert!(
            (center_hz(peak) - 1000.0).abs() < 60.0,
            "peak at {} Hz",
            center_hz(peak)
        );
        // Silence sits at the floor.
        let silent = kaldi_fbank(&vec![0.0; 800], rate, 128);
        assert!((silent[0][0] - f32::EPSILON.ln()).abs() < 1e-4);
        // Too short for one frame.
        assert!(kaldi_fbank(&[0.0; 100], rate, 128).is_empty());
    }

    #[test]
    fn ast_input_pads_and_normalizes() {
        let x = ast_input(&vec![0.0; 1600], 16_000, 4, 10, -1.0, 2.0, true);
        assert_eq!(x.len(), 40);
        // A padding row is 0 before normalization: (0 - -1) / 4.
        assert!((x[39] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn discovers_an_ast_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("Xenova").join("ast-finetuned-audioset");
        fs::create_dir_all(repo.join("onnx")).unwrap();
        fs::write(
            repo.join("config.json"),
            json!({"architectures": ["ASTForAudioClassification"],
                   "id2label": {"0": "Speech", "1": "Music"}})
            .to_string(),
        )
        .unwrap();
        fs::write(
            repo.join("preprocessor_config.json"),
            json!({"feature_extractor_type": "ASTFeatureExtractor", "sampling_rate": 16000})
                .to_string(),
        )
        .unwrap();
        let model = repo.join("onnx").join("model_quantized.onnx");
        fs::write(&model, b"onnx").unwrap();
        let p = AudioPackage::discover(&model).unwrap();
        assert_eq!(p.labels, vec!["Speech", "Music"]);
        assert_eq!(p.sampling_rate, 16_000);
        assert_eq!(p.name, "ast-finetuned-audioset");
        assert_eq!(pipeline_task_of(&model), Some("audio-classification"));
    }
}
