//! `harness transcribe` and `harness asr ls|catalog|install|rm`: speech recognition with a
//! local ONNX model and the managed sherpa-onnx runtime. Everything runs in this process (no
//! instance is needed): the models are folders under the model directories, the runtime is one
//! of the registered ones (`harness runtime add --github k2-fsa/sherpa-onnx --family
//! sherpa-onnx`, the one text to speech uses too).

use std::io::Write;
use std::time::Duration;

use harness_core::asr::{self, catalog, run, AsrModel, Transcript};
use serde_json::{json, Value};

use crate::cli::{AsrCommand, TranscribeArgs};
use crate::error::{CliError, CliResult};
use crate::output::{bytes, table, Out};

pub async fn command(command: &AsrCommand, out: &Out) -> CliResult<()> {
    match command {
        AsrCommand::Ls => ls(out).await,
        AsrCommand::Catalog => list_catalog(out).await,
        AsrCommand::Install { id } => install(id, out).await,
        AsrCommand::Rm { model } => rm(model, out).await,
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

async fn installed() -> CliResult<Vec<AsrModel>> {
    blocking(|| Ok(asr::installed_models())).await
}

fn model_json(m: &AsrModel) -> Value {
    json!({
        "name": m.name,
        "id": m.id,
        "family": m.family.as_str(),
        "languages": m.languages,
        "quantization": m.quantization,
        "size_bytes": m.size_bytes,
    })
}

async fn ls(out: &Out) -> CliResult<()> {
    let models = installed().await?;
    let value = Value::Array(models.iter().map(model_json).collect());
    out.data(&value, || {
        if models.is_empty() {
            return "No speech recognition model is installed. See `harness asr catalog`."
                .to_string();
        }
        let rows: Vec<Vec<String>> = models
            .iter()
            .map(|m| {
                vec![
                    m.name.clone(),
                    m.family.as_str().to_string(),
                    languages_text(&m.languages),
                    m.quantization.clone(),
                    bytes(m.size_bytes),
                ]
            })
            .collect();
        table(&["MODEL", "FAMILY", "LANGUAGES", "QUANT", "SIZE"], &rows)
    });
    Ok(())
}

/// `en,de,fr,... (25)` for long lists, `-` when the folder is not from the catalog.
fn languages_text(languages: &[String]) -> String {
    match languages.len() {
        0 => "-".to_string(),
        1..=6 => languages.join(","),
        n => format!("{},... ({n})", languages[..4].join(",")),
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
                    languages_text(&e.languages),
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
    let entry = catalog::find(id).ok_or_else(|| {
        CliError::core(
            harness_core::error_key::err("stt.catalog_not_found")
                .with("id", id)
                .to_string(),
        )
    })?;
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
        format!("Installed {} in {}.", model.name, model.dir.display())
    });
    Ok(())
}

async fn rm(model: &str, out: &Out) -> CliResult<()> {
    let models = installed().await?;
    let found = asr::find_model(&models, Some(model), false).map_err(CliError::core)?;
    out.confirm(&format!(
        "Remove {} and everything in {}?",
        found.name,
        found.dir.display()
    ))?;
    let (file, name) = (found.model_file.clone(), found.name.clone());
    blocking(move || {
        let roots = harness_core::config::AppSettings::load().all_models_dirs();
        asr::delete_model_dir(&file, &roots)
    })
    .await?;
    out.data(&json!({"removed": name}), || {
        format!("Removed speech recognition model {name}")
    });
    Ok(())
}

/// What `transcribe -f` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
    VerboseJson,
    DiarizedJson,
    Srt,
    Vtt,
}

fn parse_format(s: &str) -> CliResult<Format> {
    Ok(match s.trim().to_ascii_lowercase().as_str() {
        "text" | "txt" => Format::Text,
        "json" => Format::Json,
        "verbose_json" | "verbose-json" => Format::VerboseJson,
        "diarized_json" | "diarized-json" => Format::DiarizedJson,
        "srt" => Format::Srt,
        "vtt" => Format::Vtt,
        _ => {
            return Err(CliError::usage(
                "cli.invalid_value",
                format!(
                "Unknown format \"{s}\" (use text, json, verbose_json, diarized_json, srt, vtt)"
            ),
            ))
        }
    })
}

/// The transcript as the file or stdout gets it.
fn render(t: &Transcript, format: Format) -> String {
    match format {
        Format::Text => format!("{}\n", t.text.trim()),
        Format::Json => format!("{}\n", t.to_json()),
        Format::VerboseJson => format!(
            "{}\n",
            serde_json::to_string_pretty(&t.to_verbose_json("transcribe")).unwrap_or_default()
        ),
        Format::DiarizedJson => format!(
            "{}\n",
            serde_json::to_string_pretty(&t.to_diarized_json("transcribe")).unwrap_or_default()
        ),
        Format::Srt => t.to_srt(),
        Format::Vtt => t.to_vtt(),
    }
}

fn is_resident_asr(path: &std::path::Path) -> bool {
    asr::is_onnx_ctc_file(path) || harness_core::engine::crispasr::is_model(path)
}

pub async fn transcribe(args: &TranscribeArgs, out: &Out) -> CliResult<()> {
    let format = parse_format(&args.format)?;
    // `--json` is the whole result, like everywhere; without -o it replaces the plain text.
    let format = if out.json && args.output.is_none() {
        Format::VerboseJson
    } else {
        format
    };
    let to_stdout = args.output.as_ref().is_none_or(|p| p.as_os_str() == "-");
    let audio = std::fs::read(&args.file).map_err(|e| {
        CliError::general(
            "cli.io",
            format!("Could not read {}: {e}", args.file.display()),
        )
    })?;

    let ctc = blocking({
        let wanted = args.model.clone();
        move || {
            let defs = harness_core::domain::store::Domain::global()
                .definitions()
                .unwrap_or_default();
            let files = harness_core::models::ModelManager.scan_models();
            Ok(wanted
                .as_deref()
                .and_then(|name| {
                    let path = std::path::PathBuf::from(name);
                    if is_resident_asr(&path) {
                        return Some(path);
                    }
                    defs.iter()
                        .find(|d| d.alias == name || d.id == name)
                        .and_then(|d| d.base.local_path())
                        .filter(|p| is_resident_asr(p))
                        .map(std::path::Path::to_path_buf)
                        .or_else(|| {
                            files
                                .iter()
                                .find(|m| {
                                    (m.name == name || m.id == name) && is_resident_asr(&m.path)
                                })
                                .map(|m| m.path.clone())
                        })
                })
                .or_else(|| {
                    if wanted.is_none() && asr::installed_models().is_empty() {
                        files
                            .iter()
                            .find(|m| is_resident_asr(&m.path))
                            .map(|m| m.path.clone())
                    } else {
                        None
                    }
                }))
        }
    })
    .await?;
    if ctc.is_some() && (args.diarize || args.speakers.is_some() || format == Format::DiarizedJson)
    {
        return Err(CliError::usage(
            "cli.invalid_value",
            "Resident ASR models cannot tell speakers apart; --diarize, --speakers and diarized_json need a sherpa-onnx model",
        ));
    }
    let transcript = if let Some(path) = ctc {
        out.info(format!("Transcribing with {} ...", path.display()));
        if harness_core::engine::crispasr::is_model(&path) {
            asr::crispasr::transcribe(
                &path,
                audio,
                args.threads,
                args.runtime.as_deref(),
                args.language.as_deref(),
            )
            .await
            .map_err(CliError::core)?
        } else {
            asr::onnx::transcribe(&path, audio, args.threads)
                .await
                .map_err(CliError::core)?
        }
    } else {
        let models = installed().await?;
        let model =
            asr::find_model(&models, args.model.as_deref(), false).map_err(CliError::core)?;
        let runtime = args.runtime.clone();
        let binary = blocking(move || run::resolve_binary(runtime.as_deref())).await?;
        let vad = catalog::ensure_vad().await.map_err(CliError::core)?;
        if let Some(n) = args
            .speakers
            .filter(|n| !(1..=asr::diarize::MAX_SPEAKERS).contains(n))
        {
            return Err(CliError::usage(
                "cli.invalid_value",
                format!(
                    "--speakers {n} is not between 1 and {}",
                    asr::diarize::MAX_SPEAKERS
                ),
            ));
        }
        let diarize = args.diarize || args.speakers.is_some() || format == Format::DiarizedJson;
        let speaker_models = if diarize {
            Some(
                asr::diarize::ensure_models()
                    .await
                    .map_err(CliError::core)?,
            )
        } else {
            None
        };
        out.info(format!("Transcribing with {} ...", model.name));
        run::transcribe(
            &binary,
            &run::TranscribeRequest {
                model,
                vad: &vad,
                language: args.language.as_deref(),
                threads: args.threads,
                diarize: speaker_models.as_ref().map(|m| run::Diarization {
                    models: m,
                    speakers: args.speakers,
                }),
            },
            audio,
        )
        .await
        .map_err(CliError::core)?
    };

    let text = render(&transcript, format);
    if to_stdout {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(text.as_bytes())
            .and_then(|_| stdout.flush())
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
        out.info(summary(&transcript, None));
    } else if let Some(path) = &args.output {
        std::fs::write(path, text.as_bytes()).map_err(|e| {
            CliError::general("cli.io", format!("Could not write {}: {e}", path.display()))
        })?;
        let value = json!({
            "file": path,
            "model": transcript.model,
            "duration_secs": transcript.duration_secs,
            "segments": transcript.segments.len(),
            "elapsed_ms": transcript.elapsed_ms,
        });
        out.data(&value, || summary(&transcript, Some(path)));
    }
    Ok(())
}

fn summary(t: &Transcript, file: Option<&std::path::PathBuf>) -> String {
    format!(
        "{} {:.1} s of audio, {} segments, model {}, read in {:.1} s",
        match file {
            Some(p) => format!("Wrote {}:", p.display()),
            None => "Done:".to_string(),
        },
        t.duration_secs,
        t.segments.len(),
        t.model,
        t.elapsed_ms as f64 / 1000.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use clap::Parser;
    use harness_core::asr::Segment;

    fn parse(args: &[&str]) -> Command {
        let mut v = vec!["harness"];
        v.extend_from_slice(args);
        crate::cli::Cli::try_parse_from(v).unwrap().command
    }

    #[test]
    fn transcribe_arguments_parse() {
        let Command::Transcribe(a) = parse(&[
            "transcribe",
            "talk.wav",
            "-m",
            "parakeet",
            "-l",
            "ja",
            "-f",
            "srt",
            "-o",
            "talk.srt",
            "--threads",
            "2",
        ]) else {
            panic!("not transcribe")
        };
        assert_eq!(a.file, std::path::PathBuf::from("talk.wav"));
        assert_eq!(a.model.as_deref(), Some("parakeet"));
        assert_eq!(a.language.as_deref(), Some("ja"));
        assert_eq!(a.format, "srt");
        assert_eq!(a.output, Some("talk.srt".into()));
        assert_eq!(a.threads, Some(2));
        let Command::Transcribe(a) = parse(&["transcribe", "a.mp3"]) else {
            panic!()
        };
        assert!(a.model.is_none() && a.format == "text" && a.output.is_none());
        assert!(crate::cli::Cli::try_parse_from(["harness", "transcribe"]).is_err());
    }

    #[test]
    fn asr_commands_parse_and_need_no_instance() {
        assert!(matches!(
            parse(&[
                "asr",
                "install",
                "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"
            ]),
            Command::Asr {
                command: AsrCommand::Install { .. }
            }
        ));
        assert!(matches!(
            parse(&["asr", "catalog"]),
            Command::Asr {
                command: AsrCommand::Catalog
            }
        ));
        assert!(super::super::need(&parse(&["asr", "ls"])).is_none());
        assert!(super::super::need(&parse(&["transcribe", "x.wav"])).is_none());
    }

    #[test]
    fn formats() {
        assert_eq!(parse_format("TXT").unwrap(), Format::Text);
        assert_eq!(parse_format("verbose-json").unwrap(), Format::VerboseJson);
        assert_eq!(parse_format("vtt").unwrap(), Format::Vtt);
        let e = parse_format("docx").unwrap_err();
        assert_eq!(e.exit, crate::error::Exit::Usage);
        assert!(e.message.contains("srt"), "{}", e.message);
    }

    #[test]
    fn the_transcript_is_rendered_per_format() {
        let t = Transcript::new(
            vec![Segment {
                start: 0.0,
                end: 1.5,
                text: "Merhaba dünya.".into(),
                speaker: None,
            }],
            2.0,
            None,
            "m".into(),
            5,
        );
        assert_eq!(render(&t, Format::Text), "Merhaba dünya.\n");
        assert_eq!(render(&t, Format::Json), "{\"text\":\"Merhaba dünya.\"}\n");
        assert!(render(&t, Format::Srt).starts_with("1\n00:00:00,000 --> 00:00:01,500\n"));
        assert!(render(&t, Format::Vtt).starts_with("WEBVTT\n"));
        let v: Value = serde_json::from_str(&render(&t, Format::VerboseJson)).unwrap();
        assert_eq!(v["segments"][0]["end"], 1.5);
    }

    #[test]
    fn languages_are_summarised() {
        assert_eq!(languages_text(&[]), "-");
        assert_eq!(languages_text(&["tr".into(), "en".into()]), "tr,en");
        let many: Vec<String> = (0..25).map(|i| format!("l{i}")).collect();
        assert_eq!(languages_text(&many), "l0,l1,l2,l3,... (25)");
    }

    #[test]
    fn asr_keys_have_english_messages() {
        let e = CliError::core(
            harness_core::error_key::err("stt.catalog_not_found")
                .with("id", "nope")
                .to_string(),
        );
        assert_eq!(e.code, "stt.catalog_not_found");
        assert!(e.message.contains("nope"), "{}", e.message);
        let e = CliError::core(asr::find_model(&[], None, false).unwrap_err());
        assert_eq!(e.code, "stt.no_model");
        assert!(e.message.contains("harness asr install"), "{}", e.message);
    }
}
