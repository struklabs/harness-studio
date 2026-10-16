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
/// How far apart two voices must be to count as two speakers (cosine distance; the tool's
/// own default is 0.5). Measured with the models above on the public sherpa-onnx test recordings
/// (two speakers twice, four speakers once): 0.5 split one of the two-speaker recordings into
/// three, and every value from 0.65 to 0.75 found the right number in all three; at 0.8 the
/// four-speaker recording lost a speaker. The middle of that range.
pub const CLUSTER_THRESHOLD: f64 = 0.7;
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
        None => args.push(format!(
            "--clustering.cluster-threshold={CLUSTER_THRESHOLD}"
        )),
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

/// Silence put into the audio where one speaker takes over from another without a pause (see
/// [`Splice`]). Long enough for the voice activity detector (0.5 s) and for the streaming
/// models' pause rule (1 s) to end a segment there.
pub const SPLICE_SILENCE_SECS: f64 = 1.2;
/// A speaker change is cut only when both turns around it last this long (seconds): a short
/// "yes" inside the other voice's sentence is not worth cutting the sentence for.
const SPLICE_MIN_TURN_SECS: f64 = 0.8;
/// Turns closer than this (seconds) count as running into each other; a longer gap is a pause
/// the voice activity detector cuts at anyway.
const SPLICE_MAX_GAP_SECS: f64 = 0.5;

/// The places where the audio gets silence put in before speech recognition. A segment of the
/// recognizer is cut at pauses, so two speakers who answer each other without one ended up in the
/// same segment and the transcript gave the whole segment to whoever spoke more of it. With the
/// silence at each speaker change, every segment has one speaker. The recognized times are put
/// back afterwards ([`Splice::restore`]), so the answer never shows the silence.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Splice {
    /// Seconds into the audio the tool read (padding included), ascending.
    points: Vec<f64>,
}

impl Splice {
    /// A cut at each change between two speakers whose turns run into each other.
    pub fn at_speaker_changes(turns: &[Turn]) -> Self {
        let mut sorted: Vec<&Turn> = turns.iter().collect();
        sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut points: Vec<f64> = Vec::new();
        for pair in sorted.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let long = |t: &Turn| t.end - t.start >= SPLICE_MIN_TURN_SECS;
            if a.speaker == b.speaker
                || !long(a)
                || !long(b)
                || b.start - a.end > SPLICE_MAX_GAP_SECS
            {
                continue;
            }
            let at = (a.end + b.start) / 2.0;
            if points
                .last()
                .is_none_or(|last| at - last >= SPLICE_MIN_TURN_SECS)
            {
                points.push(at);
            }
        }
        Self { points }
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Seconds of silence the cuts add.
    pub fn added_secs(&self) -> f64 {
        self.points.len() as f64 * SPLICE_SILENCE_SECS
    }

    /// 16-bit mono `pcm` at `rate` with the silence put in.
    pub fn apply(&self, rate: u32, pcm: &[u8]) -> Vec<u8> {
        let silence = vec![0u8; (f64::from(rate) * SPLICE_SILENCE_SECS) as usize * 2];
        let mut out = Vec::with_capacity(pcm.len() + silence.len() * self.points.len());
        let mut from = 0;
        for point in &self.points {
            let at = (((point * f64::from(rate)) as usize) * 2).clamp(from, pcm.len() / 2 * 2);
            out.extend_from_slice(&pcm[from..at]);
            out.extend_from_slice(&silence);
            from = at;
        }
        out.extend_from_slice(&pcm[from..]);
        out
    }

    /// The time in the original audio of `t` seconds into the spliced audio (both with the
    /// padding): the silence before `t` is taken out; a time inside a cut is the cut's place.
    pub fn restore(&self, t: f64) -> f64 {
        let mut removed = 0.0;
        for (k, point) in self.points.iter().enumerate() {
            let start = point + k as f64 * SPLICE_SILENCE_SECS;
            removed += (t - start).clamp(0.0, SPLICE_SILENCE_SECS);
        }
        t - removed
    }
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

    fn turn(start: f64, end: f64, speaker: u32) -> Turn {
        Turn {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn a_cut_goes_where_one_long_turn_runs_into_another_speakers() {
        let turns = [
            turn(0.5, 4.0, 0),
            turn(4.0, 9.0, 1),
            // A pause of a second: the voice activity detector cuts there itself.
            turn(10.2, 14.0, 0),
            // Overlapping: the cut goes in the middle of the overlap.
            turn(13.0, 18.0, 1),
            // The same speaker goes on: no cut.
            turn(18.0, 20.0, 1),
            // A short turn on either side: no cut.
            turn(20.0, 20.5, 0),
            turn(20.5, 26.0, 1),
        ];
        let splice = Splice::at_speaker_changes(&turns);
        assert_eq!(splice.points, [4.0, 13.5]);
        assert_eq!(splice.added_secs(), 2.4);
        assert!(Splice::at_speaker_changes(&[turn(0.0, 5.0, 0)]).is_empty());
        assert!(Splice::at_speaker_changes(&[]).is_empty());
        // Turns come in any order.
        let mut shuffled = turns;
        shuffled.reverse();
        assert_eq!(Splice::at_speaker_changes(&shuffled), splice);
    }

    #[test]
    fn the_silence_goes_in_and_the_times_come_back() {
        let splice = Splice {
            points: vec![1.0, 3.0],
        };
        // 4 s of audio at 100 Hz, every sample 1.
        let pcm: Vec<u8> = std::iter::repeat_n(1i16.to_le_bytes(), 400)
            .flatten()
            .collect();
        let spliced = splice.apply(100, &pcm);
        let samples: Vec<i16> = spliced
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        assert_eq!(samples.len(), 400 + 2 * 120);
        // 1 s of audio, 1.2 s of silence, 2 s of audio, 1.2 s of silence, 1 s of audio.
        let runs: Vec<(i16, usize)> = samples
            .chunk_by(|a, b| a == b)
            .map(|r| (r[0], r.len()))
            .collect();
        assert_eq!(runs, [(1, 100), (0, 120), (1, 200), (0, 120), (1, 100)]);
        // Before the first cut nothing moves; after both the 2.4 s are taken out.
        assert_eq!(splice.restore(0.5), 0.5);
        assert_eq!(splice.restore(1.0), 1.0);
        assert!((splice.restore(2.2) - 1.0).abs() < 1e-9);
        assert!((splice.restore(2.7) - 1.5).abs() < 1e-9);
        assert!((splice.restore(5.4) - 3.0).abs() < 1e-9);
        assert!((splice.restore(6.4) - 4.0).abs() < 1e-9);
        // Nothing to cut: the audio is the audio.
        assert_eq!(Splice::default().apply(100, &pcm), pcm);
        assert_eq!(Splice::default().restore(2.5), 2.5);
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
        assert!(args.contains(&"--clustering.cluster-threshold=0.7".to_string()));
        // A known number of speakers replaces the threshold.
        assert!(!build_args(&m, Some(2), 4)
            .iter()
            .any(|a| a.contains("cluster-threshold")));
    }
}
