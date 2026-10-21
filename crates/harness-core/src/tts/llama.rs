//! llama.cpp text to speech: Qwen3-TTS (`general.architecture = qwen3tts`, a projector with
//! `clip.gen.audio.projector_type`) through the runtime's `llama-tts` tool (llama.cpp b10270+;
//! llama-server has no speech endpoint).
//!
//! * One `llama-tts` runs at a time ([`permit`]); others wait. The process is killed when the
//!   request is dropped (the client went away) or its time is up.
//! * The text goes in a file (`-f`), never on the command line (Windows' 32k limit, quoting);
//!   arguments are an array, never a shell line.
//! * A text is spoken in runs of whole sentences ([`groups`]), each with a frame limit (`-n`)
//!   for its length ([`frame_budget`]): the model sometimes misses its end token and talks on
//!   until the limit (llama.cpp #26699). A run that used its whole budget, or a request that ran
//!   out of time, is answered with what was made and `x-harness-truncated: frames` / `time`.
//! * No voice cloning: `--tts-speaker-file` is never passed. The Base model speaks with its own
//!   voice; OpenAI's voice names are accepted for compatibility and mean that voice.
//! * The temporary text and WAV files live in a per-request temporary folder that is removed
//!   when the request ends, whatever the outcome.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::io::AsyncReadExt;
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::domain::ModelDefinition;

/// `general.architecture` of the models `llama-tts` speaks with here.
const ARCHITECTURES: &[&str] = &["qwen3tts"];
/// Codec frames per second of audio (Qwen3-TTS's "12Hz" tokenizer runs at 12.5 a second).
pub const FRAME_RATE: f64 = 12.5;
/// Characters a second of slow speech says; the frame budget leaves [`SLACK`] times that room.
const CHARS_PER_SEC: f64 = 12.0;
const SLACK: f64 = 1.6;
/// A run's budget is never under this many seconds of audio, nor over [`MAX_RUN_SECS`].
const MIN_RUN_SECS: f64 = 6.0;
const MAX_RUN_SECS: f64 = 120.0;
/// Characters of one run of sentences, and of one sentence (longer ones are cut at a space).
pub const MAX_RUN_CHARS: usize = 400;
pub const MAX_SENTENCE_CHARS: usize = 300;
/// Silence between runs.
const PAUSE_SECS: f64 = 0.2;
/// Prompt context `llama-tts` gets (the text of one run and its codec frames fit easily).
const CONTEXT: &str = "4096";

/// `--tts-lang` codes of Qwen3-TTS and their names (llama.cpp's `tts_lang_codes`).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("zh", "chinese"),
    ("en", "english"),
    ("de", "german"),
    ("it", "italian"),
    ("pt", "portuguese"),
    ("es", "spanish"),
    ("ja", "japanese"),
    ("ko", "korean"),
    ("fr", "french"),
    ("ru", "russian"),
];

/// OpenAI's voice names: a client written for OpenAI sends one; the model's own voice speaks.
pub const OPENAI_VOICES: &[&str] = &[
    "alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer", "verse",
];

/// The files of a definition that speaks through `llama-tts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsModel {
    pub model: PathBuf,
    pub mmproj: PathBuf,
}

/// The model and projector when `def` speaks through `llama-tts`: a local GGUF of a known
/// architecture whose projector makes audio.
pub fn model_of(def: &ModelDefinition) -> Option<TtsModel> {
    let model = def.base.local_path()?;
    let mmproj = def.mmproj.as_deref()?;
    let arch = crate::models::cached_metadata(model)?.architecture?;
    if !ARCHITECTURES.contains(&arch.as_str()) {
        return None;
    }
    crate::models::cached_metadata(mmproj)?.gen_audio_projector?;
    Some(TtsModel {
        model: model.to_path_buf(),
        mmproj: mmproj.to_path_buf(),
    })
}

/// Whether a model file speaks through `llama-tts` (its GGUF header names a speaking
/// architecture).
pub fn is_tts_file(path: &Path) -> bool {
    crate::models::cached_metadata(path)
        .and_then(|m| m.architecture)
        .is_some_and(|a| is_tts_architecture(&a))
}

/// The projector that makes audio for `model`: an `mmproj*.gguf` in the same folder whose header
/// has `clip.gen.audio.projector_type`. With several, the one whose name shares the longest start
/// with the model's (after `mmproj-`) wins, then the first by name.
pub fn companion_projector(model: &Path) -> Option<PathBuf> {
    let dir = model.parent()?;
    let stem = model.file_stem()?.to_string_lossy().to_ascii_lowercase();
    let mut found: Vec<(usize, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .is_some_and(|n| n.starts_with("mmproj") && n.ends_with(".gguf"))
        })
        .filter(|p| {
            crate::models::cached_metadata(p).is_some_and(|m| m.gen_audio_projector.is_some())
        })
        .map(|p| {
            let name = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let name = name
                .trim_start_matches("mmproj")
                .trim_start_matches(['-', '_', '.']);
            let shared = name
                .chars()
                .zip(stem.chars())
                .take_while(|(a, b)| a == b)
                .count();
            (shared, p)
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().next().map(|(_, p)| p)
}

/// Whether a GGUF `architecture` speaks through `llama-tts`.
pub fn is_tts_architecture(architecture: &str) -> bool {
    ARCHITECTURES.contains(&architecture)
}

/// Whether a Hub model JSON is a `llama-tts` model: the Hub's GGUF header names a speaking
/// architecture and the repo ships a projector (`mmproj-*.gguf`) next to it.
pub fn is_hub_repo(item: &serde_json::Value) -> bool {
    let arch = item
        .pointer("/gguf/architecture")
        .and_then(|a| a.as_str())
        .is_some_and(is_tts_architecture);
    let mmproj = item
        .get("siblings")
        .and_then(|s| s.as_array())
        .is_some_and(|files| {
            files.iter().any(|f| {
                f.get("rfilename")
                    .and_then(|n| n.as_str())
                    .map(|n| n.rsplit('/').next().unwrap_or(n).to_ascii_lowercase())
                    .is_some_and(|n| n.starts_with("mmproj") && n.ends_with(".gguf"))
            })
        });
    arch && mmproj
}

/// `llama-tts` next to the runtime's `llama-server`.
pub fn binary_next_to(server: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "llama-tts.exe"
    } else {
        "llama-tts"
    };
    let path = server.parent()?.join(name);
    path.is_file().then_some(path)
}

/// The `--tts-lang` code of a request's language (`en`, `EN-us`, `english`); English when
/// none is named. `Err` for one the model does not speak.
pub fn language_code(language: Option<&str>) -> Result<&'static str, String> {
    let Some(l) = language.map(str::trim).filter(|l| !l.is_empty()) else {
        return Ok("en");
    };
    let lower = l.to_ascii_lowercase();
    let base = lower.split(['-', '_']).next().unwrap_or_default();
    LANGUAGES
        .iter()
        .find(|(code, name)| *code == base || *name == lower)
        .map(|(code, _)| *code)
        .ok_or_else(|| {
            format!(
                "This model speaks {}; `{l}` is not one of them.",
                LANGUAGES
                    .iter()
                    .map(|(c, _)| *c)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Whether `voice` is one this model speaks with (none, its own `default`, or an OpenAI name).
pub fn voice_ok(voice: Option<&str>) -> bool {
    voice.is_none_or(|v| {
        let v = v.trim().to_ascii_lowercase();
        v.is_empty() || v == "default" || OPENAI_VOICES.contains(&v.as_str())
    })
}

/// Codec frames a run of `chars` characters may make.
pub fn frame_budget(chars: usize) -> u32 {
    let secs = (chars as f64 / CHARS_PER_SEC * SLACK).clamp(MIN_RUN_SECS, MAX_RUN_SECS);
    (secs * FRAME_RATE).ceil() as u32
}

/// Words a single period does not end (titles and common abbreviations, Turkish and English).
const ABBREVIATIONS: &[&str] = &[
    "dr", "prof", "doç", "doc", "yrd", "av", "sn", "bkz", "bk", "vs", "vb", "örn", "ör", "mr",
    "mrs", "ms", "st", "no", "nr", "mah", "cad", "sok", "apt", "tel", "etc", "vol", "fig",
];

/// Whether the period at byte `at` of `text` ends a sentence: not after an abbreviation, a
/// number ("3. madde") or a one-letter initial ("A. Yılmaz").
fn ends_sentence(text: &str, at: usize) -> bool {
    let word: String = text[..at]
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if word.is_empty() {
        return true;
    }
    let lower = word.to_lowercase();
    !(ABBREVIATIONS.contains(&lower.as_str())
        || word.chars().all(|c| c.is_ascii_digit())
        || word.chars().count() == 1)
}

/// `piece` cut into runs of at most `limit` characters, at the last space before the limit.
fn capped(piece: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = piece.trim();
    while rest.chars().count() > limit {
        let end = rest
            .char_indices()
            .nth(limit)
            .map_or(rest.len(), |(i, _)| i);
        let cut = rest[..end].rfind(' ').filter(|&i| i > 0).unwrap_or(end);
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// The sentences of `text`: cut after `.`, `!`, `?`, `…` (closing quotes and brackets stay with
/// them) followed by a space or the end, after a CJK end mark, and at line breaks; none longer
/// than [`MAX_SENTENCE_CHARS`], none empty.
pub fn sentences(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        let mut end = None;
        if c == '\n' {
            end = Some(at + 1);
        } else if matches!(c, '。' | '！' | '？') {
            end = Some(at + c.len_utf8());
        } else if matches!(c, '.' | '!' | '?' | '…') {
            let mut j = i;
            while j + 1 < chars.len() && matches!(chars[j + 1].1, '.' | '!' | '?' | '…') {
                j += 1;
            }
            while j + 1 < chars.len()
                && matches!(chars[j + 1].1, '"' | '\'' | '”' | '’' | ')' | ']')
            {
                j += 1;
            }
            let after = chars.get(j + 1).map(|(_, c)| *c);
            let single_period = c == '.' && j == i;
            if after.is_none_or(char::is_whitespace) && (!single_period || ends_sentence(text, at))
            {
                end = Some(chars.get(j + 1).map_or(text.len(), |(p, _)| *p));
                i = j;
            }
        }
        if let Some(end) = end {
            parts.push(&text[start..end]);
            start = end;
        }
        i += 1;
    }
    parts.push(&text[start..]);
    let out: Vec<String> = parts
        .iter()
        .flat_map(|p| capped(p, MAX_SENTENCE_CHARS))
        .collect();
    if out.is_empty() {
        vec![text.trim().to_string()]
    } else {
        out
    }
}

/// `text` as runs of whole sentences of at most [`MAX_RUN_CHARS`] characters (a longer
/// sentence is its own run). CJK sentences are joined without a space.
pub fn groups(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = String::new();
    for sentence in sentences(text) {
        if !run.is_empty() && run.chars().count() + 1 + sentence.chars().count() > MAX_RUN_CHARS {
            out.push(std::mem::take(&mut run));
        }
        if !run.is_empty() {
            let cjk =
                run.ends_with(['。', '！', '？']) && !sentence.starts_with(|c: char| c.is_ascii());
            if !cjk {
                run.push(' ');
            }
        }
        run.push_str(&sentence);
    }
    if !run.is_empty() {
        out.push(run);
    }
    out
}

/// `llama-tts`'s arguments for one run.
pub fn args(
    model: &TtsModel,
    prompt_file: &Path,
    output: &Path,
    frames: u32,
    language: &str,
) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    let mut push = |a: &str, v: OsString| {
        out.push(a.into());
        out.push(v);
    };
    push("-m", model.model.clone().into());
    push("-mm", model.mmproj.clone().into());
    push("-f", prompt_file.into());
    push("-o", output.into());
    push("-n", frames.to_string().into());
    push("--tts-lang", language.into());
    push("-c", CONTEXT.into());
    out
}

/// One request at a time: each run loads the model and fills the GPU.
pub async fn permit() -> SemaphorePermit<'static> {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(1))
        .acquire()
        .await
        .expect("the gate is never closed")
}

/// Why the speech is partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The request's time ran out.
    Time,
    /// A run made all the frames its budget allows.
    Frames,
}

impl Cut {
    pub fn as_str(self) -> &'static str {
        match self {
            Cut::Time => "time",
            Cut::Frames => "frames",
        }
    }
}

/// What a request said.
#[derive(Debug)]
pub struct Speech {
    /// Mono 16-bit WAV.
    pub wav: Vec<u8>,
    pub sample_rate: u32,
    pub duration_secs: f64,
    pub cut: Option<Cut>,
    pub elapsed_ms: u64,
}

/// Joins runs' samples with a short pause between them.
pub fn join(runs: &[Vec<i16>], rate: u32) -> Vec<i16> {
    let pause = vec![0i16; (PAUSE_SECS * f64::from(rate)) as usize];
    let mut out = Vec::new();
    for (n, run) in runs.iter().enumerate() {
        if n > 0 {
            out.extend_from_slice(&pause);
        }
        out.extend_from_slice(run);
    }
    out
}

/// Whether a run of `secs` seconds used (within a frame) the whole `budget`.
pub fn at_budget(secs: f64, budget: u32) -> bool {
    secs * FRAME_RATE >= f64::from(budget) - 1.0
}

/// Keep the gate until a cancelled run's process group has been killed and reaped.
struct RunChild {
    child: Option<AsyncGroupChild>,
    permit: Arc<SemaphorePermit<'static>>,
}

impl Drop for RunChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let permit = self.permit.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = child.wait().await;
            });
        }
    }
}

async fn run_child(
    command: &mut tokio::process::Command,
    limit: Option<Duration>,
    permit: Arc<SemaphorePermit<'static>>,
) -> std::io::Result<Option<std::process::Output>> {
    // The group builder's flag only affects Windows in command-group 5.0.1.
    command.kill_on_drop(true);
    crate::orphans::protect(command.as_std_mut());
    let mut group = command.group();
    group.kill_on_drop(true);
    #[cfg(windows)]
    group.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut run = RunChild {
        child: Some(group.spawn()?),
        permit,
    };
    let child = run.child.as_mut().unwrap();
    // Registered for the next start's sweep in case Harness is killed (`orphans`).
    let _registration = child.id().map(crate::orphans::Registration::new);
    let mut stderr = child.inner().stderr.take();
    let wait = async {
        let mut bytes = Vec::new();
        if let Some(stderr) = &mut stderr {
            stderr.read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(std::process::Output {
            status: child.wait().await?,
            stdout: Vec::new(),
            stderr: bytes,
        })
    };
    let out = match limit {
        Some(limit) => match tokio::time::timeout(limit, wait).await {
            Ok(out) => out?,
            Err(_) => {
                // Retain ownership across timeout so the entire group is killed and reaped.
                let child = run.child.as_mut().unwrap();
                child.start_kill()?;
                child.wait().await?;
                run.child.take();
                return Ok(None);
            }
        },
        None => wait.await?,
    };
    run.child.take();
    Ok(Some(out))
}

/// Speaks `text` with `binary`; `limit` is the request's time (None: no limit). Errors are
/// plain messages; `Err` before any audio was made.
pub async fn speak(
    binary: &Path,
    model: &TtsModel,
    text: &str,
    language: &str,
    limit: Option<Duration>,
) -> Result<Speech, String> {
    let dir = tempfile::Builder::new()
        .prefix("harness-llama-tts-")
        .tempdir()
        .map_err(|e| format!("could not create a temporary folder: {e}"))?;
    let permit = Arc::new(permit().await);
    let started = Instant::now();
    let deadline = limit.map(|l| started + l);
    let mut runs = Vec::new();
    let mut rate = 0;
    let mut cut = None;
    for (n, run) in groups(text).iter().enumerate() {
        let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if left.is_some_and(|l| l.is_zero()) {
            cut = Some(Cut::Time);
            break;
        }
        let prompt = dir.path().join(format!("text-{n}.txt"));
        let output = dir.path().join(format!("speech-{n}.wav"));
        tokio::fs::write(&prompt, run.as_bytes())
            .await
            .map_err(|e| format!("could not write the text: {e}"))?;
        let budget = frame_budget(run.chars().count());
        let mut command = tokio::process::Command::new(binary);
        command
            .args(args(model, &prompt, &output, budget, language))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(parent) = binary.parent() {
            command.current_dir(parent);
        }
        let Some(out) = run_child(&mut command, left, permit.clone())
            .await
            .map_err(|e| format!("llama-tts ({}): {e}", binary.display()))?
        else {
            cut = Some(Cut::Time);
            break;
        };
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "llama-tts failed: {}",
                crate::tts::run::failure_detail(&stderr)
            ));
        }
        let wav = tokio::fs::read(&output)
            .await
            .map_err(|e| format!("llama-tts wrote no audio: {e}"))?;
        let (samples, run_rate) =
            crate::asr::llm::pcm16(&wav).ok_or("llama-tts wrote audio that is not 16-bit WAV")?;
        rate = run_rate;
        if at_budget(samples.len() as f64 / f64::from(run_rate), budget) {
            cut.get_or_insert(Cut::Frames);
        }
        runs.push(samples);
    }
    if runs.is_empty() {
        return Err(if cut == Some(Cut::Time) {
            "llama-tts did not finish in time".into()
        } else {
            "llama-tts made no audio".into()
        });
    }
    let samples = join(&runs, rate);
    Ok(Speech {
        duration_secs: samples.len() as f64 / f64::from(rate),
        wav: crate::asr::llm::wav_of(&samples, rate),
        sample_rate: rate,
        cut,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Re-enter this test binary as a stand-in for llama-tts and its descendant.
    #[test]
    fn process_group_fixture() {
        let Some(dir) = std::env::var_os("HARNESS_LLAMA_TTS_FIXTURE") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let descendant = std::env::var_os("HARNESS_LLAMA_TTS_DESCENDANT").is_some();
        let _child = if descendant {
            None
        } else {
            Some(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "tts::llama::tests::process_group_fixture"])
                    .env("HARNESS_LLAMA_TTS_DESCENDANT", "1")
                    .spawn()
                    .unwrap(),
            )
        };
        std::fs::write(
            dir.join(if descendant { "descendant" } else { "parent" }),
            std::process::id().to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(60));
    }

    #[tokio::test]
    async fn timeout_and_cancellation_kill_and_reap_process_groups() {
        for cancel in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let permit = Arc::new(permit().await);
            let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "tts::llama::tests::process_group_fixture"])
                .env("HARNESS_LLAMA_TTS_FIXTURE", dir.path())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            let task = tokio::spawn(async move {
                run_child(
                    &mut command,
                    (!cancel).then_some(Duration::from_secs(3)),
                    permit,
                )
                .await
            });
            let pids = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let (Ok(parent), Ok(descendant)) = (
                        std::fs::read_to_string(dir.path().join("parent")),
                        std::fs::read_to_string(dir.path().join("descendant")),
                    ) {
                        break [parent.parse::<u32>().unwrap(), descendant.parse().unwrap()];
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("fixture processes started");
            if cancel {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            } else {
                assert!(task.await.unwrap().unwrap().is_none());
            }
            // The gate is held by cancellation cleanup until wait() has reaped the child.
            let _permit = tokio::time::timeout(Duration::from_secs(10), super::permit())
                .await
                .expect("cleanup released the gate");
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let mut system = sysinfo::System::new();
                    let pids = pids.map(sysinfo::Pid::from_u32);
                    system.refresh_processes_specifics(
                        sysinfo::ProcessesToUpdate::Some(&pids),
                        true,
                        sysinfo::ProcessRefreshKind::nothing(),
                    );
                    if pids.iter().all(|pid| {
                        system.process(*pid).is_none_or(|process| {
                            process.status() == sysinfo::ProcessStatus::Zombie
                        })
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the entire process group exited");
            #[cfg(unix)]
            {
                let mut status = 0;
                assert_eq!(
                    unsafe { libc::waitpid(pids[0] as i32, &mut status, libc::WNOHANG) },
                    -1
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ECHILD)
                );
            }
        }
    }

    #[test]
    fn languages_are_codes_or_names() {
        assert_eq!(language_code(None), Ok("en"));
        assert_eq!(language_code(Some(" ")), Ok("en"));
        assert_eq!(language_code(Some("DE")), Ok("de"));
        assert_eq!(language_code(Some("pt-BR")), Ok("pt"));
        assert_eq!(language_code(Some("Japanese")), Ok("ja"));
        let err = language_code(Some("tr")).unwrap_err();
        assert!(err.contains("zh, en, de") && err.contains("`tr`"), "{err}");
    }

    #[test]
    fn only_the_models_own_voice_speaks() {
        assert!(voice_ok(None) && voice_ok(Some("")) && voice_ok(Some("default")));
        assert!(voice_ok(Some("Alloy")) && voice_ok(Some("verse")));
        assert!(!voice_ok(Some("Vivian")));
        assert!(!voice_ok(Some("C:/voices/me.wav")));
    }

    #[test]
    fn frame_budgets_grow_with_the_text_within_bounds() {
        assert_eq!(frame_budget(0), 75); // 6 s
        assert_eq!(frame_budget(120), 200); // 16 s
        assert_eq!(frame_budget(MAX_RUN_CHARS), 667); // 53.3 s
        assert_eq!(frame_budget(100_000), 1500); // 120 s
        assert!(at_budget(200.0 / FRAME_RATE, 200));
        assert!(at_budget(199.0 / FRAME_RATE, 200));
        assert!(!at_budget(150.0 / FRAME_RATE, 200));
    }

    #[test]
    fn text_is_cut_into_sentences_and_runs() {
        assert_eq!(
            sentences("Dr. Ali geldi. 3. madde bitti! A. Yılmaz \"evet.\" dedi\nYeni satır"),
            vec![
                "Dr. Ali geldi.",
                "3. madde bitti!",
                // A closing quote stays with its end mark.
                "A. Yılmaz \"evet.\"",
                "dedi",
                "Yeni satır"
            ]
        );
        assert_eq!(
            sentences("今天很好。明天见！"),
            vec!["今天很好。", "明天见！"]
        );
        assert_eq!(
            sentences("Wait... what?! Fine."),
            vec!["Wait...", "what?!", "Fine."]
        );
        assert_eq!(sentences("3.14 is pi"), vec!["3.14 is pi"]);
        let long = "word ".repeat(100);
        let cut = sentences(&long);
        assert!(cut.len() == 2 && cut.iter().all(|s| s.chars().count() <= MAX_SENTENCE_CHARS));
        // Runs of whole sentences under the limit.
        let text = "This sentence has forty characters ok. ".repeat(25);
        let runs = groups(&text);
        assert_eq!(runs.len(), 3);
        assert!(runs.iter().all(|r| r.chars().count() <= MAX_RUN_CHARS));
        assert_eq!(runs.join(" "), text.trim());
        assert_eq!(groups("今天很好。明天见！"), vec!["今天很好。明天见！"]);
        assert_eq!(groups("Short."), vec!["Short."]);
    }

    #[test]
    fn arguments_never_carry_the_text_or_a_speaker() {
        let model = TtsModel {
            model: "m.gguf".into(),
            mmproj: "mm.gguf".into(),
        };
        let a: Vec<String> = args(&model, Path::new("t.txt"), Path::new("o.wav"), 200, "de")
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            a,
            [
                "-m",
                "m.gguf",
                "-mm",
                "mm.gguf",
                "-f",
                "t.txt",
                "-o",
                "o.wav",
                "-n",
                "200",
                "--tts-lang",
                "de",
                "-c",
                "4096"
            ]
        );
        assert!(!a.iter().any(|x| x.contains("speaker") || x == "-p"));
    }

    #[test]
    fn qwen3_tts_gguf_repos_are_recognised() {
        // ggml-org/Qwen3-TTS-12Hz-1.7B-Base-GGUF (2026-10-16): no pipeline_tag, the Hub's GGUF
        // header says `qwen3tts`, model and projector files.
        let item = serde_json::json!({
            "id": "ggml-org/Qwen3-TTS-12Hz-1.7B-Base-GGUF",
            "gguf": {"architecture": "qwen3tts", "total": 1733157888},
            "siblings": [
                {"rfilename": "Qwen3-TTS-12Hz-1.7B-Base-Q4_K_M.gguf"},
                {"rfilename": "mmproj-Qwen3-TTS-12Hz-1.7B-Base-Q8_0.gguf"}
            ]
        });
        assert!(is_hub_repo(&item));
        let mut no_projector = item.clone();
        no_projector["siblings"] = serde_json::json!([{"rfilename": "Qwen3-TTS-Q4_K_M.gguf"}]);
        assert!(!is_hub_repo(&no_projector));
        let mut chat = item.clone();
        chat["gguf"]["architecture"] = "qwen3".into();
        assert!(!is_hub_repo(&chat));
    }

    /// A GGUF header with string keys only.
    fn gguf(path: &Path, keys: &[(&str, &str)]) {
        let mut out = b"GGUF".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend((keys.len() as u64).to_le_bytes());
        for (k, v) in keys {
            out.extend((k.len() as u64).to_le_bytes());
            out.extend(k.as_bytes());
            out.extend(8u32.to_le_bytes());
            out.extend((v.len() as u64).to_le_bytes());
            out.extend(v.as_bytes());
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn the_projector_that_makes_audio_is_found_next_to_the_model() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("Qwen3-TTS-12Hz-1.7B-Base-Q4_K_M.gguf");
        gguf(&model, &[("general.architecture", "qwen3tts")]);
        assert!(is_tts_file(&model));
        assert_eq!(companion_projector(&model), None);
        // A vision projector and a speaking one of another model; the matching one wins.
        gguf(
            &dir.path().join("mmproj-gemma.gguf"),
            &[
                ("general.architecture", "clip"),
                ("clip.projector_type", "gemma3"),
            ],
        );
        let other = dir.path().join("mmproj-Other-TTS-Q8_0.gguf");
        gguf(
            &other,
            &[
                ("general.architecture", "clip"),
                ("clip.gen.audio.projector_type", "x_gen"),
            ],
        );
        assert_eq!(companion_projector(&model), Some(other));
        let own = dir.path().join("mmproj-Qwen3-TTS-12Hz-1.7B-Base-Q8_0.gguf");
        gguf(
            &own,
            &[
                ("general.architecture", "clip"),
                ("clip.audio.projector_type", "qwen3tts_spkenc"),
                ("clip.gen.audio.projector_type", "qwen3tts_gen"),
            ],
        );
        assert_eq!(companion_projector(&model), Some(own.clone()));
        // The speaker encoder of that projector does not make the model one that hears.
        let meta = crate::models::cached_metadata(&own).unwrap();
        assert_eq!(meta.gen_audio_projector.as_deref(), Some("qwen3tts_gen"));
        assert!(!crate::asr::llm::hears(
            meta.audio_projector.as_deref().unwrap()
        ));
        // A definition with both speaks through llama-tts.
        let mut def = ModelDefinition::new(
            "q",
            crate::domain::ModelRef::Local {
                path: model.clone(),
            },
        );
        assert_eq!(model_of(&def), None);
        def.mmproj = Some(own.clone());
        assert_eq!(model_of(&def), Some(TtsModel { model, mmproj: own }));
    }

    #[test]
    fn runs_are_joined_with_a_pause() {
        let joined = join(&[vec![1, 2], vec![3]], 10);
        assert_eq!(joined, vec![1, 2, 0, 0, 3]);
        assert_eq!(join(&[], 10), Vec::<i16>::new());
    }
}
