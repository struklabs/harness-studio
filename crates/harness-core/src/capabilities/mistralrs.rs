//! Probe of a `mistralrs` binary (mistral.rs).
//!
//! mistral.rs is one clap program with subcommands; Harness runs `mistralrs serve`, so the flags
//! that matter are the ones `mistralrs serve --help` lists. clap prints them in one of two
//! layouts, both handled here:
//!
//! ```text
//! Options:
//!   -m, --model-id <MODEL_ID>  Model ID (Hugging Face id or a local directory)
//!   -p, --port <PORT>          Port to serve on [default: 1234]
//!       --cpu                  Run on the CPU only
//!
//!   -m, --model-id <MODEL_ID>
//!           Model ID (Hugging Face id or a local directory)
//!
//!           [default: ...]
//! ```
//!
//! `mistralrs --version` prints `mistralrs <version>` (clap's own format); [`parse_version`]
//! takes the version from it.

use super::parse::{FlagInfo, VersionInfo};
use super::{BinaryKey, RuntimeCapabilities, SCHEMA_VERSION};
use std::collections::BTreeMap;
use std::path::Path;

/// `[default: X]`, `[possible values: a, b]`, `[env: X=]` of a clap description.
fn bracket_field(description: &str, name: &str) -> Option<String> {
    let start = description.find(&format!("[{name}: "))? + name.len() + 3;
    let rest = &description[start..];
    let end = rest.find(']')?;
    Some(rest[..end].trim().to_string()).filter(|s| !s.is_empty())
}

/// Indentation of a line (spaces; a tab counts as 4).
fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Flags of a clap `--help` text (`mistralrs serve --help`).
pub fn parse_help(text: &str) -> BTreeMap<String, FlagInfo> {
    let mut out: BTreeMap<String, FlagInfo> = BTreeMap::new();
    let mut section: Option<String> = None;
    // The flag whose description continues on the next, deeper indented lines.
    let mut current: Option<(String, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = indent_of(line);
        if indent == 0 {
            current = None;
            if let Some(name) = trimmed.strip_suffix(':') {
                section = Some(name.trim().to_ascii_lowercase());
            }
            continue;
        }
        // A flag header: a short indentation and a leading dash.
        if indent <= 6 && trimmed.starts_with('-') {
            let (head, description) = match trimmed.find("  ") {
                Some(i) => (&trimmed[..i], trimmed[i..].trim()),
                None => (trimmed, ""),
            };
            let mut aliases: Vec<String> = Vec::new();
            let mut hint: Option<String> = None;
            for token in head.split_whitespace() {
                let token = token.trim_end_matches(',');
                if token.starts_with('-') && !token.starts_with("-<") {
                    // `--isq <ISQ>` and `--isq=<ISQ>`.
                    let (flag, value) = token.split_once('=').unwrap_or((token, ""));
                    aliases.push(flag.to_string());
                    if !value.is_empty() {
                        hint = Some(value.to_string());
                    }
                } else if !token.is_empty() {
                    hint = Some(match hint {
                        Some(h) => format!("{h} {token}"),
                        None => token.to_string(),
                    });
                }
            }
            if aliases.is_empty() {
                current = None;
                continue;
            }
            let canonical = aliases
                .iter()
                .find(|a| a.starts_with("--"))
                .unwrap_or(&aliases[0])
                .clone();
            out.insert(
                canonical.clone(),
                FlagInfo {
                    canonical: canonical.clone(),
                    aliases,
                    value_hint: hint,
                    description: description.to_string(),
                    section: section.clone(),
                    ..Default::default()
                },
            );
            current = Some((canonical, indent));
            continue;
        }
        // A description line of the flag above (deeper indented).
        if let Some((key, header_indent)) = &current {
            if indent > *header_indent {
                if let Some(info) = out.get_mut(key) {
                    if !info.description.is_empty() {
                        info.description.push(' ');
                    }
                    info.description.push_str(trimmed);
                }
            }
        }
    }
    for info in out.values_mut() {
        info.default = bracket_field(&info.description, "default");
        info.env = bracket_field(&info.description, "env").map(|e| {
            e.split_once('=')
                .map_or(e.as_str(), |(name, _)| name)
                .to_string()
        });
        info.enum_values = bracket_field(&info.description, "possible values")
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        info.deprecated = info.description.to_ascii_lowercase().contains("deprecated");
    }
    out
}

/// The version of `mistralrs --version` output (`mistralrs 0.7.0`, `mistralrs-cli v0.6.0`):
/// the first word of the first line that starts with a digit (a leading `v` dropped).
pub fn parse_version(text: &str) -> VersionInfo {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let version = line
        .split_whitespace()
        .map(|w| w.strip_prefix('v').unwrap_or(w))
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(String::from);
    VersionInfo {
        raw: if version.is_some() {
            line.to_string()
        } else {
            String::new()
        },
        version,
        ..Default::default()
    }
}

/// Capabilities from the raw `--version` and `serve --help` outputs. The help counts as parsed
/// only when it is the `serve` table (it has `--model-id`).
pub fn from_outputs(key: BinaryKey, version: &str, help: &str) -> RuntimeCapabilities {
    let flags = parse_help(help);
    let version = parse_version(version);
    let mut caps = RuntimeCapabilities {
        schema_version: SCHEMA_VERSION,
        key: BinaryKey {
            version: version.version.clone(),
            ..key
        },
        help_parsed: flags.contains_key("--model-id"),
        version,
        flags,
        probed_at: super::now_secs(),
        ..Default::default()
    };
    caps.reindex();
    caps
}

/// Probes a binary (`--version` and `serve --help`). Blocking.
pub fn probe_binary(binary: &Path) -> RuntimeCapabilities {
    let (path, mtime_ms, size) = BinaryKey::file_id(binary);
    let key = BinaryKey {
        path,
        mtime_ms,
        size,
        ..Default::default()
    };
    let version = super::probe_output(binary, "--version").unwrap_or_default();
    let help = super::probe_output_args(binary, &["serve", "--help"]).unwrap_or_default();
    from_outputs(key, &version, &help)
}

/// The `serve` options Harness relies on, in clap's layout (written from the mistral.rs
/// documentation, not captured from a build).
#[cfg(test)]
pub(crate) fn tests_help() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/mistralrs-serve.help.txt"),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help() -> String {
        tests_help()
    }

    #[test]
    fn parses_both_clap_layouts() {
        let flags = parse_help(&help());
        let model = &flags["--model-id"];
        assert_eq!(model.aliases, vec!["-m", "--model-id"]);
        assert_eq!(model.value_hint.as_deref(), Some("<MODEL_ID>"));
        assert_eq!(flags["--port"].default.as_deref(), Some("1234"));
        assert_eq!(flags["--host"].default.as_deref(), Some("0.0.0.0"));
        assert!(flags["--cpu"].value_hint.is_none());
        assert!(flags["--no-ui"].value_hint.is_none());
        assert_eq!(flags["--paged-attn"].enum_values, ["auto", "on", "off"]);
        assert_eq!(flags["--max-seqs"].default.as_deref(), Some("32"));
        assert_eq!(flags["--max-seq-len"].default.as_deref(), Some("4096"));
        // The long layout: description and default on the following lines.
        let isq = &flags["--isq"];
        assert_eq!(isq.value_hint.as_deref(), Some("<ISQ>"));
        assert!(isq.description.contains("In-situ quantization"), "{isq:?}");
        assert_eq!(flags["--dtype"].default.as_deref(), Some("auto"));
        assert_eq!(flags["--help"].aliases, vec!["-h", "--help"]);

        let compact = "Options:\n  -p, --port <PORT>          Port [default: 1234]\n      --cpu                  CPU only\n      --token <T>            Token [env: HF_TOKEN=]\n";
        let f = parse_help(compact);
        assert_eq!(f["--port"].default.as_deref(), Some("1234"));
        assert_eq!(f["--token"].env.as_deref(), Some("HF_TOKEN"));
        assert!(f["--cpu"].value_hint.is_none());
        assert_eq!(f["--cpu"].description, "CPU only");
    }

    #[test]
    fn version_from_the_clap_line() {
        assert_eq!(
            parse_version("mistralrs 0.7.0\n").version.as_deref(),
            Some("0.7.0")
        );
        assert_eq!(
            parse_version("mistralrs-cli v0.6.1-alpha\n")
                .version
                .as_deref(),
            Some("0.6.1-alpha")
        );
        let v = parse_version("\n  mistralrs 0.8.2 (3f2515e)\n");
        assert_eq!(v.version.as_deref(), Some("0.8.2"));
        assert_eq!(v.raw, "mistralrs 0.8.2 (3f2515e)");
        assert!(parse_version("error: unexpected argument")
            .version
            .is_none());
        assert!(parse_version("").raw.is_empty());
    }

    #[test]
    fn capabilities_need_the_serve_table() {
        let caps = from_outputs(BinaryKey::default(), "mistralrs 0.7.0", &help());
        assert!(caps.help_parsed);
        assert!(caps.has_flag("-m") && caps.has_flag("--isq"));
        assert!(!caps.looks_like_server(), "that test is llama.cpp's");
        assert_eq!(caps.version.version.as_deref(), Some("0.7.0"));
        assert_eq!(caps.key.version.as_deref(), Some("0.7.0"));
        // The top-level help (subcommands only) does not count.
        let top = "Usage: mistralrs <COMMAND>\n\nCommands:\n  serve  Start a server\n  run    Interactive\n\nOptions:\n  -h, --help     Print help\n  -V, --version  Print version\n";
        assert!(!from_outputs(BinaryKey::default(), "", top).help_parsed);
    }
}
