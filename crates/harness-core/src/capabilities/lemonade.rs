//! Probe of a `lemond` binary (Lemonade).
//!
//! `lemond --version` prints `lemond version 2026.41.1`; `--help` lists a handful of options
//! (`--port`, `--host`, `--broadcast`, `--log-file`...) in CLI11's layout, not llama.cpp's.
//! Harness only needs the version and to know the binary runs, so no flag table is built.

use super::{BinaryKey, RuntimeCapabilities, SCHEMA_VERSION};
use std::path::Path;

/// The version in `lemond version 2026.41.1` (the last token of the first line).
pub fn parse_version(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let v = line.split_whitespace().last()?;
    v.chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        .then(|| v.to_string())
}

/// Capabilities from the raw `--version` output.
pub fn from_version(key: BinaryKey, version: &str) -> RuntimeCapabilities {
    let mut caps = RuntimeCapabilities {
        schema_version: SCHEMA_VERSION,
        key,
        help_parsed: parse_version(version).is_some(),
        probed_at: super::now_secs(),
        ..Default::default()
    };
    caps.version.raw = version
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    caps.version.version = parse_version(version);
    caps.key.version = caps.version.version.clone();
    caps
}

/// Probes a binary (one `--version` run). Blocking.
pub fn probe_binary(binary: &Path) -> RuntimeCapabilities {
    let (path, mtime_ms, size) = BinaryKey::file_id(binary);
    let key = BinaryKey {
        path,
        mtime_ms,
        size,
        ..Default::default()
    };
    let text = super::probe_output(binary, "--version").unwrap_or_default();
    from_version(key, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_of_the_real_output() {
        // Output of `lemond.exe --version` of v2026.41.1 (embeddable windows-x64).
        assert_eq!(
            parse_version("lemond version 2026.41.1\n").as_deref(),
            Some("2026.41.1")
        );
        assert_eq!(
            parse_version("\n\nlemond version 9.4.0").as_deref(),
            Some("9.4.0")
        );
        assert_eq!(parse_version("usage: lemond"), None);
        assert_eq!(parse_version(""), None);
        let caps = from_version(BinaryKey::default(), "lemond version 2026.41.1\n");
        assert!(caps.help_parsed);
        assert_eq!(caps.version.version.as_deref(), Some("2026.41.1"));
        assert!(!from_version(BinaryKey::default(), "").help_parsed);
    }
}
