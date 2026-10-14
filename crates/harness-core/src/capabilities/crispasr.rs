//! CrispASR's CLI flag table, cached with the installed binary identity.
use super::{BinaryKey, FlagInfo, RuntimeCapabilities, SCHEMA_VERSION};
use std::path::Path;

pub fn from_help(key: BinaryKey, help: &str) -> RuntimeCapabilities {
    let mut caps = RuntimeCapabilities {
        schema_version: SCHEMA_VERSION,
        key,
        probed_at: super::now_secs(),
        ..Default::default()
    };
    for line in help.lines().map(str::trim).filter(|l| l.starts_with('-')) {
        let aliases: Vec<String> = line
            .split_whitespace()
            .map(|s| s.trim_end_matches(','))
            .take_while(|s| s.starts_with('-'))
            .map(str::to_string)
            .collect();
        if let Some(name) = aliases.last().cloned() {
            caps.flags.insert(
                name.clone(),
                FlagInfo {
                    canonical: name,
                    aliases,
                    description: line.into(),
                    ..Default::default()
                },
            );
        }
    }
    caps.help_parsed = !caps.flags.is_empty();
    caps.reindex();
    caps
}

pub fn probe_binary(binary: &Path) -> RuntimeCapabilities {
    let (path, mtime_ms, size) = BinaryKey::file_id(binary);
    from_help(
        BinaryKey {
            path,
            mtime_ms,
            size,
            ..Default::default()
        },
        &super::probe_output(binary, "--help").unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_server_flags() {
        let caps = super::from_help(Default::default(), "  --server  Run HTTP server\n  --host HOST  Bind address\n  -m, --model FILE  Load model\n  --api-keys KEYS  Keys");
        assert!(caps.has_flag("--server"));
        assert!(caps.has_flag("-m"));
        assert!(caps.has_flag("--api-keys"));
    }
}
