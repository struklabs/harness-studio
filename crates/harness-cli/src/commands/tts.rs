//! `harness speak` and `harness tts ls|catalog|install|rm`: text to speech with a local model
//! and the managed sherpa-onnx runtime. Everything runs in this process (no instance is
//! needed): the models are folders under the model directories, the runtime is one of the
//! registered ones (`harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`).

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use harness_core::tts::{self, catalog, run, AudioFormat, TtsModel};
use serde_json::{json, Value};

use crate::cli::{SpeakArgs, TtsCommand};
use crate::error::{CliError, CliResult};
use crate::output::{bytes, table, Out};

pub async fn command(command: &TtsCommand, out: &Out) -> CliResult<()> {
    match command {
        TtsCommand::Ls => ls(out).await,
        TtsCommand::Catalog => list_catalog(out).await,
        TtsCommand::Install { id } => install(id, out).await,
        TtsCommand::Rm { model } => rm(model, out).await,
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> CliResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CliError::general("cli.io", e.to_string()))?
        .map_err(CliError::core)
}

async fn installed() -> CliResult<Vec<TtsModel>> {
    blocking(|| Ok(tts::installed_models())).await
}

fn model_json(m: &TtsModel) -> Value {
    json!({
        "name": m.name,
        "id": m.id,
        "family": m.family.as_str(),
        "languages": m.languages,
        "quantization": m.quantization,
        "sample_rate": m.sample_rate,
        "size_bytes": m.size_bytes,
        "voices": m.voices.iter().map(|v| json!({
            "id": v.id, "sid": v.sid, "language": v.language, "gender": v.gender,
        })).collect::<Vec<_>>(),
    })
}

async fn ls(out: &Out) -> CliResult<()> {
    let models = installed().await?;
    let value = Value::Array(models.iter().map(model_json).collect());
    out.data(&value, || {
        if models.is_empty() {
            return "No text-to-speech model is installed. See `harness tts catalog`.".to_string();
        }
        let rows: Vec<Vec<String>> = models
            .iter()
            .map(|m| {
                vec![
                    m.name.clone(),
                    m.family.as_str().to_string(),
                    m.languages.join(","),
                    m.quantization.clone(),
                    bytes(m.size_bytes),
                    voice_summary(m),
                ]
            })
            .collect();
        table(
            &["MODEL", "FAMILY", "LANGUAGES", "QUANT", "SIZE", "VOICES"],
            &rows,
        )
    });
    Ok(())
}

/// `af_heart, af_bella, ... (53)` for long lists.
fn voice_summary(m: &TtsModel) -> String {
    let names: Vec<&str> = m.voices.iter().map(|v| v.id.as_str()).collect();
    if names.len() <= 4 {
        names.join(", ")
    } else {
        format!("{}, ... ({})", names[..3].join(", "), names.len())
    }
}

async fn list_catalog(out: &Out) -> CliResult<()> {
    let have = installed().await?;
    let entries = catalog::catalog();
    let value = Value::Array(
        entries
            .iter()
            .map(|e| {
                let mut v = serde_json::to_value(e).unwrap_or_default();
                v["installed"] = json!(have.iter().any(|m| m.name == e.id));
                v
            })
            .collect(),
    );
    out.data(&value, || {
        let rows: Vec<Vec<String>> = entries
            .iter()
            .map(|e| {
                vec![
                    e.id.clone(),
                    e.languages.join(","),
                    bytes(e.size_bytes),
                    e.license.clone(),
                    match (have.iter().any(|m| m.name == e.id), e.recommended) {
                        (true, _) => "installed".to_string(),
                        (false, true) => "recommended".to_string(),
                        _ => String::new(),
                    },
                ]
            })
            .collect();
        table(&["ID", "LANGUAGES", "SIZE", "LICENSE", ""], &rows)
    });
    Ok(())
}

async fn install(id: &str, out: &Out) -> CliResult<()> {
    let entry = catalog::find(id).ok_or_else(|| CliError::core(catalog_not_found(id)))?;
    out.confirm(&format!(
        "Download {} ({}, {}) from the official sherpa-onnx release?",
        entry.id,
        bytes(entry.size_bytes),
        entry.license
    ))?;
    let id = entry.id.clone();
    let task = tokio::spawn(async move { catalog::install(&id).await });
    tokio::pin!(task);
    let model = loop {
        tokio::select! {
            res = &mut task => {
                break res
                    .map_err(|e| CliError::general("cli.io", e.to_string()))?
                    .map_err(CliError::core)?;
            }
            _ = tokio::time::sleep(Duration::from_millis(700)) => {
                if let Some(p) = catalog::install_progress() {
                    out.progress(&format!(
                        "{} {} / {}",
                        p.current_asset,
                        bytes(p.downloaded_bytes),
                        bytes(p.total_bytes)
                    ));
                }
            }
        }
    };
    out.progress_done();
    let value = model_json(&model);
    out.data(&value, || {
        format!(
            "Installed {} in {} ({} voices).",
            model.name,
            model.dir.display(),
            model.voices.len()
        )
    });
    Ok(())
}

fn catalog_not_found(id: &str) -> String {
    harness_core::error_key::err("tts.catalog_not_found")
        .with("id", id)
        .into()
}

async fn rm(model: &str, out: &Out) -> CliResult<()> {
    let models = installed().await?;
    let found = tts::find_model(&models, Some(model)).map_err(CliError::core)?;
    out.confirm(&format!(
        "Remove {} and everything in {}?",
        found.name,
        found.dir.display()
    ))?;
    let (file, name) = (found.model_file.clone(), found.name.clone());
    blocking(move || {
        let roots = harness_core::config::AppSettings::load().all_models_dirs();
        tts::delete_model_dir(&file, &roots)
    })
    .await?;
    out.data(&json!({"removed": name}), || {
        format!("Removed text-to-speech model {name}")
    });
    Ok(())
}

/// The text to read: the argument, or stdin when it is `-` or missing (a terminal is not
/// read: that would hang).
fn text_from(
    arg: Option<&str>,
    stdin: &mut dyn Read,
    stdin_is_terminal: bool,
) -> CliResult<String> {
    match arg {
        Some(t) if t != "-" => Ok(t.to_string()),
        None if stdin_is_terminal => Err(CliError::usage(
            "cli.missing_argument",
            "Give the text to read as an argument, or pipe it in (harness speak - < text.txt)",
        )),
        _ => {
            let mut raw = Vec::new();
            stdin
                .read_to_end(&mut raw)
                .map_err(|e| CliError::general("cli.io", e.to_string()))?;
            Ok(String::from_utf8_lossy(&raw).into_owned())
        }
    }
}

/// Where the audio goes: a file, or stdout for `-`.
#[derive(Debug, PartialEq)]
enum Target {
    File(PathBuf),
    Stdout,
}

fn target_of(output: Option<&PathBuf>, format: AudioFormat) -> Target {
    match output {
        Some(p) if p.as_os_str() == "-" => Target::Stdout,
        Some(p) => Target::File(p.clone()),
        None => Target::File(PathBuf::from(format!("speech.{}", format.extension()))),
    }
}

fn parse_format(s: &str) -> CliResult<AudioFormat> {
    AudioFormat::parse(s).ok_or_else(|| {
        CliError::usage(
            "cli.invalid_value",
            format!(
                "Unknown format \"{s}\" (use {})",
                AudioFormat::ALL.map(AudioFormat::as_str).join(", ")
            ),
        )
    })
}

pub async fn speak(args: &SpeakArgs, out: &Out) -> CliResult<()> {
    let format = parse_format(&args.format)?;
    let target = target_of(args.output.as_ref(), format);
    if target == Target::Stdout && out.json {
        return Err(CliError::usage(
            "cli.invalid_value",
            "--json cannot be combined with -o - (the audio is the output)",
        ));
    }
    if target == Target::Stdout && std::io::stdout().is_terminal() {
        return Err(CliError::usage(
            "cli.invalid_value",
            "Refusing to write audio to a terminal; redirect stdout or give -o FILE",
        ));
    }
    let stdin = std::io::stdin();
    let is_terminal = stdin.is_terminal();
    let text = text_from(args.text.as_deref(), &mut stdin.lock(), is_terminal)?;
    let (text, speed) = tts::check_request(&text, args.speed).map_err(CliError::core)?;
    if !run::format_available(format) {
        return Err(CliError::core(
            harness_core::error_key::err("tts.format_needs_ffmpeg")
                .with("format", format.as_str())
                .to_string(),
        ));
    }

    let models = installed().await?;
    let model = tts::find_model(&models, args.model.as_deref()).map_err(CliError::core)?;
    let runtime = args.runtime.clone();
    let binary = blocking(move || run::resolve_binary(runtime.as_deref())).await?;
    out.info(format!("Reading aloud with {} ...", model.name));
    let speech = run::synthesize(
        &binary,
        &run::SynthesisRequest {
            model,
            voice: args.voice.as_deref(),
            input: &text,
            speed,
            threads: args.threads,
        },
    )
    .await
    .map_err(CliError::core)?;
    let audio = run::encode(speech.wav.clone(), format)
        .await
        .map_err(CliError::core)?;

    match &target {
        Target::Stdout => {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&audio)
                .and_then(|_| stdout.flush())
                .map_err(|e| CliError::general("cli.io", e.to_string()))?;
            out.info(summary(&speech, format, audio.len(), None));
        }
        Target::File(path) => {
            std::fs::write(path, &audio).map_err(|e| {
                CliError::general("cli.io", format!("Could not write {}: {e}", path.display()))
            })?;
            let value = json!({
                "file": path,
                "model": speech.model,
                "voice": speech.voice,
                "format": format.as_str(),
                "sample_rate": speech.sample_rate,
                "duration_secs": speech.duration_secs,
                "bytes": audio.len(),
                "elapsed_ms": speech.elapsed_ms,
            });
            out.data(&value, || summary(&speech, format, audio.len(), Some(path)));
        }
    }
    Ok(())
}

fn summary(s: &run::Speech, format: AudioFormat, size: usize, file: Option<&PathBuf>) -> String {
    format!(
        "{} {:.2} s {} ({}, {} Hz, {}), model {}, voice {}, made in {:.1} s",
        match file {
            Some(p) => format!("Wrote {}:", p.display()),
            None => "Wrote audio:".to_string(),
        },
        s.duration_secs,
        format.as_str(),
        bytes(size as u64),
        s.sample_rate,
        "mono",
        s.model,
        s.voice,
        s.elapsed_ms as f64 / 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> crate::cli::Command {
        let mut v = vec!["harness"];
        v.extend_from_slice(args);
        crate::cli::Cli::try_parse_from(v).unwrap().command
    }

    #[test]
    fn speak_arguments_parse() {
        let crate::cli::Command::Speak(a) = parse(&[
            "speak", "Merhaba", "-m", "kokoro", "--voice", "af_heart", "-s", "1.2", "-f", "mp3",
            "-o", "out.mp3",
        ]) else {
            panic!("not speak")
        };
        assert_eq!(a.text.as_deref(), Some("Merhaba"));
        assert_eq!(a.model.as_deref(), Some("kokoro"));
        assert_eq!(a.voice.as_deref(), Some("af_heart"));
        assert_eq!(a.speed, Some(1.2));
        assert_eq!(a.format, "mp3");
        assert_eq!(a.output, Some(PathBuf::from("out.mp3")));
        let crate::cli::Command::Speak(a) = parse(&["speak"]) else {
            panic!()
        };
        assert!(a.text.is_none() && a.format == "wav" && a.output.is_none());
    }

    #[test]
    fn tts_and_runtime_family_parse() {
        assert!(matches!(
            parse(&["tts", "install", "kokoro-int8-multi-lang-v1_0"]),
            crate::cli::Command::Tts {
                command: TtsCommand::Install { .. }
            }
        ));
        assert!(matches!(
            parse(&["tts", "catalog"]),
            crate::cli::Command::Tts {
                command: TtsCommand::Catalog
            }
        ));
        let crate::cli::Command::Runtime {
            command: crate::cli::RuntimeCommand::Add { family, github, .. },
        } = parse(&[
            "runtime",
            "add",
            "--github",
            "k2-fsa/sherpa-onnx",
            "--family",
            "sherpa-onnx",
        ])
        else {
            panic!("not runtime add")
        };
        assert_eq!(family.as_deref(), Some("sherpa-onnx"));
        assert_eq!(github.as_deref(), Some("k2-fsa/sherpa-onnx"));
    }

    #[test]
    fn speak_and_tts_need_no_instance() {
        assert!(super::super::need(&parse(&["speak", "x"])).is_none());
        assert!(super::super::need(&parse(&["tts", "ls"])).is_none());
    }

    #[test]
    fn text_comes_from_the_argument_or_stdin() {
        let mut empty: &[u8] = b"";
        assert_eq!(text_from(Some("hi"), &mut empty, false).unwrap(), "hi");
        let mut piped: &[u8] = "Şu çocuğun güzel öğüdü\n".as_bytes();
        assert_eq!(
            text_from(Some("-"), &mut piped, false).unwrap(),
            "Şu çocuğun güzel öğüdü\n"
        );
        let mut piped: &[u8] = b"from a pipe";
        assert_eq!(text_from(None, &mut piped, false).unwrap(), "from a pipe");
        let mut tty: &[u8] = b"";
        let e = text_from(None, &mut tty, true).unwrap_err();
        assert_eq!(e.exit, crate::error::Exit::Usage);
    }

    #[test]
    fn output_target_and_format() {
        assert_eq!(
            target_of(None, AudioFormat::Wav),
            Target::File(PathBuf::from("speech.wav"))
        );
        assert_eq!(
            target_of(None, AudioFormat::Opus),
            Target::File(PathBuf::from("speech.opus"))
        );
        assert_eq!(
            target_of(Some(&PathBuf::from("-")), AudioFormat::Wav),
            Target::Stdout
        );
        assert_eq!(
            target_of(Some(&PathBuf::from("a/b.wav")), AudioFormat::Mp3),
            Target::File(PathBuf::from("a/b.wav"))
        );
        assert_eq!(parse_format("MP3").unwrap(), AudioFormat::Mp3);
        let e = parse_format("ogg").unwrap_err();
        assert_eq!(e.exit, crate::error::Exit::Usage);
        assert!(e.message.contains("wav"), "{}", e.message);
    }

    #[test]
    fn tts_keys_have_english_messages() {
        let e = CliError::core(catalog_not_found("nope"));
        assert_eq!(e.code, "tts.catalog_not_found");
        assert!(e.message.contains("nope"), "{}", e.message);
        let e = CliError::core(tts::find_model(&[], None).unwrap_err());
        assert_eq!(e.code, "tts.no_model");
        assert!(e.message.contains("harness tts install"), "{}", e.message);
        let e = CliError::core(tts::check_request("", None).unwrap_err());
        assert_eq!(e.code, "tts.input_empty");
    }
}
