//! Who spoke when: speaker diarization with sherpa-onnx's
//! `sherpa-onnx-offline-speaker-diarization` (pyannote segmentation 3.0 finds the speech of each
//! speaker, a 3D-Speaker CAM++ embedding tells the voices apart, clustering groups them), so a
//! transcript can say which speaker said each segment ([`label`]).
//!
//! The two models are small (7 MB and 28 MB), language independent and downloaded once into the
//! hidden `asr/.diarization/` folder with their SHA-256 checked (GitHub publishes no digest for
//! these assets; the pins were computed from the release files). The tool reads 16 kHz audio
//! only, so the request's audio is resampled for it ([`super::run::resample_16k`]).

use std::path::{Path, PathBuf};

use super::Segment;
use crate::error_key::err;

/// pyannote segmentation 3.0 (MIT, CNRS), as sherpa-onnx packs it.
pub const SEGMENTATION_ID: &str = "sherpa-onnx-pyannote-segmentation-3-0";
pub const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
pub const SEGMENTATION_SHA256: &str =
    "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488";
/// 3D-Speaker CAM++ (Apache-2.0, Alibaba), trained on Chinese and English speech; the voice
/// features it compares do not depend on the language.
pub const EMBEDDING_FILE: &str = "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";
pub const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";
pub const EMBEDDING_SHA256: &str =
    "aa3cfc16963a10586a9393f5035d6d6b57e98d358b347f80c2a30bf4f00ceba2";
pub const EMBEDDING_SIZE: u64 = 28_281_164;
/// Folder under the speech-recognition models home (hidden: the model scan skips it).
const DIR: &str = ".diarization";
/// Most speakers a request may name.
pub const MAX_SPEAKERS: u32 = 32;
/// The rate the tool reads.
pub const RATE: u32 = 16_000;

/// The two model files.
#[derive(Debug, Clone, PartialEq)]
pub struct Models {
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
}

/// One stretch of one speaker, in seconds of the audio the tool read.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start: f64,
    pub end: f64,
    pub speaker: u32,
}

/// The segmentation model in an unpacked folder.
fn segmentation_in(dir: &Path) -> Option<PathBuf> {
    let model = dir.join("model.onnx");
    model.is_file().then_some(model)
}

/// Downloads the models into `home/.diarization` when they are not there yet.
pub async fn ensure_models_in(home: &Path) -> Result<Models, String> {
    let dir = home.join(DIR);
    let segmentation = crate::tts::catalog::install_archive(
        super::catalog::downloader(),
        &crate::tts::catalog::Archive {
            id: SEGMENTATION_ID,
            url: SEGMENTATION_URL,
            sha256: SEGMENTATION_SHA256,
        },
        &dir,
        segmentation_in,
        "stt.archive_not_a_model",
    )
    .await?;
    let embedding = dir.join(EMBEDDING_FILE);
    if !std::fs::metadata(&embedding).is_ok_and(|m| m.len() == EMBEDDING_SIZE) {
        let dl = super::catalog::downloader();
        let _busy = dl.try_busy()?;
        let result = dl
            .download_to_file(
                EMBEDDING_URL,
                &embedding,
                EMBEDDING_FILE,
                Some(EMBEDDING_SHA256),
            )
            .await;
        dl.clear_download_progress();
        result?;
    }
    Ok(Models {
        segmentation,
        embedding,
    })
}

/// [`ensure_models_in`] the speech-recognition models folder of the settings.
pub async fn ensure_models() -> Result<Models, String> {
    ensure_models_in(&super::catalog::models_home()).await
}

/// The diarization tool next to the runtime's own binary.
pub fn tool_binary(runtime_binary: &Path) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "sherpa-onnx-offline-speaker-diarization.exe"
    } else {
        "sherpa-onnx-offline-speaker-diarization"
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

/// The command line, without the audio file. `speakers`: how many there are, when the caller
/// knows (else the clustering decides).
pub fn build_args(models: &Models, speakers: Option<u32>, threads: u32) -> Vec<String> {
    let mut args = vec![
        format!(
            "--segmentation.pyannote-model={}",
            models.segmentation.display()
        ),
        format!("--embedding.model={}", models.embedding.display()),
        format!("--segmentation.num-threads={}", threads.max(1)),
        format!("--embedding.num-threads={}", threads.max(1)),
        "--print-args=false".to_string(),
    ];
    match speakers {
        Some(n) => args.push(format!("--clustering.num-clusters={n}")),
        None => args.push("--clustering.cluster-threshold=0.5".to_string()),
    }
    args
}

/// Reads the tool's lines `start -- end speaker_NN`.
pub fn parse_turns(stdout: &str) -> Vec<Turn> {
    stdout
        .lines()
        .filter_map(|line| {
            let (times, speaker) = line.trim().rsplit_once(' ')?;
            let speaker = speaker.strip_prefix("speaker_")?.parse().ok()?;
            let (start, end) = times.split_once(" -- ")?;
            Some(Turn {
                start: start.trim().parse().ok()?,
                end: end.trim().parse().ok()?,
                speaker,
            })
        })
        .collect()
}

/// OpenAI's speaker names: `A`, `B`, ... `Z`, then `A2`, `B2`...
pub fn speaker_name(index: usize) -> String {
    let letter = (b'A' + (index % 26) as u8) as char;
    match index / 26 {
        0 => letter.to_string(),
        round => format!("{letter}{}", round + 1),
    }
}

/// Gives each segment the speaker who talks most during it. `pad` seconds of silence were put in
/// front of the audio the tool read ([`super::run::PAD_SECS`]); segment times are without it. A
/// segment no turn overlaps takes the nearest turn's speaker. Speakers are named in the order
/// they first speak.
pub fn label(segments: &mut [Segment], turns: &[Turn], pad: f64) {
    if turns.is_empty() {
        return;
    }
    let mut order: Vec<u32> = Vec::new();
    for s in segments.iter_mut() {
        let overlap = |t: &Turn| ((t.end - pad).min(s.end) - (t.start - pad).max(s.start)).max(0.0);
        let mut talk: Vec<(u32, f64)> = Vec::new();
        for t in turns {
            let o = overlap(t);
            if o > 0.0 {
                match talk.iter_mut().find(|(sp, _)| *sp == t.speaker) {
                    Some((_, total)) => *total += o,
                    None => talk.push((t.speaker, o)),
                }
            }
        }
        let speaker = talk
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(sp, _)| *sp)
            .unwrap_or_else(|| {
                let mid = (s.start + s.end) / 2.0;
                let distance = |t: &Turn| {
                    let (a, b) = (t.start - pad, t.end - pad);
                    if mid < a {
                        a - mid
                    } else if mid > b {
                        mid - b
                    } else {
                        0.0
                    }
                };
                turns
                    .iter()
                    .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                    .map(|t| t.speaker)
                    .unwrap_or(0)
            });
        let index = match order.iter().position(|sp| *sp == speaker) {
            Some(i) => i,
            None => {
                order.push(speaker);
                order.len() - 1
            }
        };
        s.speaker = Some(speaker_name(index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64) -> Segment {
        Segment {
            start,
            end,
            text: "x".into(),
            speaker: None,
        }
    }

    #[test]
    fn the_tool_output_is_read() {
        let out =
            "Started\n1.583 -- 3.406 speaker_00\n9.346 -- 11.472 speaker_01\nDuration : 16.000 s\n";
        assert_eq!(
            parse_turns(out),
            vec![
                Turn {
                    start: 1.583,
                    end: 3.406,
                    speaker: 0
                },
                Turn {
                    start: 9.346,
                    end: 11.472,
                    speaker: 1
                },
            ]
        );
    }

    #[test]
    fn each_segment_gets_the_speaker_who_talks_most_during_it() {
        // Turns on the padded audio (0.5 s in front); the second speaker starts first.
        let turns = vec![
            Turn {
                start: 0.5,
                end: 3.5,
                speaker: 1,
            },
            Turn {
                start: 3.6,
                end: 6.5,
                speaker: 0,
            },
            Turn {
                start: 6.5,
                end: 7.0,
                speaker: 1,
            },
        ];
        let mut segs = vec![seg(0.0, 3.0), seg(3.2, 6.4), seg(5.9, 6.5), seg(9.0, 9.5)];
        label(&mut segs, &turns, 0.5);
        let names: Vec<_> = segs.iter().map(|s| s.speaker.clone().unwrap()).collect();
        // speaker 1 speaks first: A; speaker 0: B. The third segment overlaps 0 for 0.1 s and 1
        // for 0.5 s; the last one overlaps nothing and takes the nearest turn (speaker 1).
        assert_eq!(names, ["A", "B", "A", "A"]);
        // No turns: nothing is labelled.
        let mut segs = vec![seg(0.0, 1.0)];
        label(&mut segs, &[], 0.5);
        assert_eq!(segs[0].speaker, None);
    }

    #[test]
    fn speakers_are_named_like_openai_names_them() {
        assert_eq!(speaker_name(0), "A");
        assert_eq!(speaker_name(25), "Z");
        assert_eq!(speaker_name(26), "A2");
    }

    #[test]
    fn the_command_line_names_the_models_and_the_speakers() {
        let m = Models {
            segmentation: PathBuf::from("seg/model.onnx"),
            embedding: PathBuf::from("emb.onnx"),
        };
        let args = build_args(&m, Some(3), 4);
        assert!(args.contains(&"--clustering.num-clusters=3".to_string()));
        assert!(args
            .iter()
            .any(|a| a.starts_with("--segmentation.pyannote-model=")));
        let args = build_args(&m, None, 4);
        assert!(args.contains(&"--clustering.cluster-threshold=0.5".to_string()));
    }
}
