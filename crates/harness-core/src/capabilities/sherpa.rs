//! Probe of a `sherpa-onnx-offline-tts` binary (sherpa-onnx).
//!
//! The tool has no `--version`; `--help` prints the usage to stderr and exits 0, then the flag
//! table, one flag per line (long descriptions wrap onto lines that start with a space and
//! never with `--`):
//!
//! ```text
//!   --kokoro-model              : Path to Kokoro model (string, default = "")
//!   --speed                     : Speech speed. Larger=faster. (float, default = 1)
//! ```
//!
//! The version of a build is the release tag it was installed from (the registry records it).

use super::parse::FlagInfo;
use super::{BinaryKey, RuntimeCapabilities, SCHEMA_VERSION};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// Flags of a `--help` text.
pub fn parse_help(text: &str) -> BTreeMap<String, FlagInfo> {
    static LINE: OnceLock<Regex> = OnceLock::new();
    let line_re = LINE.get_or_init(|| {
        Regex::new(r"^\s{1,4}(--[A-Za-z0-9][A-Za-z0-9-]*)\s+:\s?(.*)$").expect("valid regex")
    });
    static DEFAULT: OnceLock<Regex> = OnceLock::new();
    let default_re =
        DEFAULT.get_or_init(|| Regex::new(r"default = ([^)]*)\)").expect("valid regex"));
    let mut out = BTreeMap::new();
    let mut in_options = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "Options:" || trimmed == "Standard options:" {
            in_options = true;
            continue;
        }
        if !in_options {
            continue;
        }
        let Some(c) = line_re.captures(line) else {
            continue;
        };
        let name = c[1].to_string();
        let description = c[2].trim().to_string();
        let default = default_re
            .captures(&description)
            .map(|d| d[1].trim().trim_matches('"').to_string())
            .filter(|d| !d.is_empty());
        out.insert(
            name.clone(),
            FlagInfo {
                canonical: name.clone(),
                aliases: vec![name],
                value_hint: Some("VALUE".to_string()),
                default,
                description,
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

    /// Real output of `sherpa-onnx-offline-tts --help` of v1.13.8 (Windows x64 shared).
    fn help() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/sherpa-onnx-offline-tts-v1.13.8.help.txt"),
        )
        .unwrap()
    }

    #[test]
    fn parses_the_real_flag_table() {
        let flags = parse_help(&help());
        assert!(flags.len() >= 60, "{} flags", flags.len());
        assert_eq!(flags["--speed"].default.as_deref(), Some("1.0"));
        assert_eq!(flags["--num-threads"].default.as_deref(), Some("1"));
        assert_eq!(
            flags["--output-filename"].default.as_deref(),
            Some("./generated.wav")
        );
        // A description that wraps over two lines still yields its flag.
        assert!(flags["--tts-silence-scale"]
            .description
            .starts_with("Duration of the pause"));
        // Example command lines of the usage text are not flags.
        assert!(!flags.contains_key("--vits-model=./vits-piper-en_US-amy-low/en_US-amy-low.onnx"));
        assert!(flags.contains_key("--vits-model") && flags.contains_key("--print-args"));
    }

    #[test]
    fn capabilities_answer_flag_questions() {
        let caps = from_help(BinaryKey::default(), &help());
        assert!(caps.help_parsed);
        for flag in [
            "--kokoro-model",
            "--kokoro-voices",
            "--kokoro-tokens",
            "--kokoro-data-dir",
            "--kokoro-lexicon",
            "--vits-model",
            "--vits-tokens",
            "--vits-data-dir",
            "--sid",
            "--speed",
            "--num-threads",
            "--output-filename",
        ] {
            assert!(caps.has_flag(flag), "{flag}");
        }
        assert!(!caps.has_flag("--host"), "no server in this build");
        assert!(from_help(BinaryKey::default(), "garbage").flags.is_empty());
    }
}
