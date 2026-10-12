//! Probe of a `whisper-server` binary (whisper.cpp).
//!
//! whisper-server has neither `--version` (an unknown argument prints the usage and exits 0,
//! so the output carries no version) nor `--list-devices`; only `--help` tells what the
//! build can do. Its flag table has a different layout than llama.cpp's:
//!
//! ```text
//!   -t N,      --threads N                 [4      ] number of threads to use during computation
//!              --prompt PROMPT             [       ] initial prompt
//!   --host HOST,                           [127.0.0.1] Hostname/ip-adress for the server
//! ```
//!
//! The version of a build is the release tag it was installed from (the registry records
//! it); a binary of unknown origin has none.

use super::{BinaryKey, RuntimeCapabilities, SCHEMA_VERSION};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use super::parse::FlagInfo;

/// Flags of a whisper-server `--help` text (stdout and stderr together; it prints to stderr).
pub fn parse_help(text: &str) -> BTreeMap<String, FlagInfo> {
    static LINE: OnceLock<Regex> = OnceLock::new();
    let line_re = LINE.get_or_init(|| {
        // `<flags>  [<default>] <description>`; the flags part has no `[`.
        Regex::new(r"^\s+(-[^\[]*?)\s+\[([^\]]*)\]\s?(.*)$").expect("valid regex")
    });
    let mut out = BTreeMap::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Voice Activity Detection") {
            section = Some("vad".into());
            continue;
        }
        let Some(c) = line_re.captures(line) else {
            continue;
        };
        let mut aliases: Vec<String> = Vec::new();
        let mut hint: Option<String> = None;
        for token in c[1].split_whitespace() {
            let token = token.trim_end_matches(',');
            if token.is_empty() {
                continue;
            }
            if token.starts_with('-') {
                aliases.push(token.to_string());
            } else {
                hint = Some(token.to_string());
            }
        }
        if aliases.is_empty() {
            continue;
        }
        // Prefer the long spelling (`--threads`) as the canonical one.
        let canonical = aliases
            .iter()
            .find(|a| a.starts_with("--"))
            .unwrap_or(&aliases[0])
            .clone();
        let default = c[2].trim();
        let default = (!default.is_empty()).then(|| default.to_string());
        out.insert(
            canonical.clone(),
            FlagInfo {
                canonical,
                aliases,
                value_hint: hint,
                default,
                description: c[3].trim().to_string(),
                section: section.clone(),
                ..Default::default()
            },
        );
    }
    out
}

/// Capabilities from the raw `--help` output.
pub fn from_help(key: BinaryKey, help: &str) -> RuntimeCapabilities {
    let flags = parse_help(help);
    let mut caps = RuntimeCapabilities {
        schema_version: SCHEMA_VERSION,
        key,
        help_parsed: !flags.is_empty(),
        flags,
        probed_at: super::now_secs(),
        ..Default::default()
    };
    caps.reindex();
    caps
}

/// Probes a binary (one `--help` run). Blocking.
pub fn probe_binary(binary: &Path) -> RuntimeCapabilities {
    let (path, mtime_ms, size) = BinaryKey::file_id(binary);
    let key = BinaryKey {
        path,
        mtime_ms,
        size,
        ..Default::default()
    };
    let help = super::probe_output(binary, "--help").unwrap_or_default();
    from_help(key, &help)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real output of whisper-server `b5130` (ggml-org/whisper.cpp, Windows x64 CPU build).
    fn help() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/whisper-server-b5130.help.txt"),
        )
        .unwrap()
    }

    #[test]
    fn parses_the_real_flag_table() {
        let flags = parse_help(&help());
        assert!(flags.len() >= 50, "{} flags", flags.len());
        let t = &flags["--threads"];
        assert_eq!(t.aliases, vec!["-t", "--threads"]);
        assert_eq!(t.value_hint.as_deref(), Some("N"));
        assert_eq!(t.default.as_deref(), Some("4"));
        assert!(t.description.starts_with("number of threads"));
        // Switch without a value.
        let ng = &flags["--no-gpu"];
        assert!(ng.value_hint.is_none());
        assert_eq!(ng.aliases, vec!["-ng", "--no-gpu"]);
        // Flag without a short spelling, with a trailing comma in the table.
        let host = &flags["--host"];
        assert_eq!(host.value_hint.as_deref(), Some("HOST"));
        assert_eq!(host.default.as_deref(), Some("127.0.0.1"));
        assert_eq!(flags["--port"].default.as_deref(), Some("8080"));
        // Header with a single space before the value hint and a long flag name.
        let vspd = &flags["--vad-min-speech-duration-ms"];
        assert_eq!(vspd.value_hint.as_deref(), Some("N"));
        assert_eq!(vspd.section.as_deref(), Some("vad"));
        assert_eq!(vspd.default.as_deref(), Some("250"));
        // No section for the main options.
        assert!(flags["--language"].section.is_none());
        assert_eq!(flags["--prompt"].value_hint.as_deref(), Some("PROMPT"));
        assert_eq!(
            flags["--inference-path"].default.as_deref(),
            Some("/inference")
        );
        // The `[default]` marker of --help itself.
        assert_eq!(flags["--help"].default.as_deref(), Some("default"));
    }

    #[test]
    fn capabilities_answer_flag_questions() {
        let caps = from_help(BinaryKey::default(), &help());
        assert!(caps.help_parsed);
        assert!(caps.has_flag("-t"));
        assert!(caps.has_flag("--no-gpu") && caps.has_flag("-ng"));
        assert!(caps.has_flag("--vad"));
        assert!(!caps.has_flag("--api-key"), "whisper-server has no auth");
        assert!(!caps.looks_like_server(), "that test is llama.cpp's");
        assert!(from_help(BinaryKey::default(), "garbage").flags.is_empty());
    }
}
