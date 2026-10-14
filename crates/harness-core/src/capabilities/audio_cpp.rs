use super::RuntimeCapabilities;
use std::path::Path;

/// Normalize flags from audio.cpp's bracketed usage synopsis as well as its
/// option table, so flags such as device and threads remain visible in the UI.
pub fn from_help(
    key: crate::capabilities::BinaryKey,
    version: &str,
    help: &str,
) -> RuntimeCapabilities {
    let flags = regex::Regex::new(r"--[a-z][a-z-]*").expect("valid audio.cpp flag pattern");
    let names: std::collections::BTreeSet<_> = flags.find_iter(help).map(|f| f.as_str()).collect();
    let normalized: String = names
        .into_iter()
        .map(|f| format!("{f} N    audio.cpp\n"))
        .collect();
    RuntimeCapabilities::from_outputs(key, version, &normalized, "")
}

pub fn probe_binary(binary: &Path) -> RuntimeCapabilities {
    let (path, mtime_ms, size) = crate::capabilities::BinaryKey::file_id(binary);
    let key = crate::capabilities::BinaryKey {
        path,
        mtime_ms,
        size,
        ..Default::default()
    };
    let help = crate::capabilities::probe_output(binary, "--help").unwrap_or_default();
    let version = crate::capabilities::probe_output(binary, "--version").unwrap_or_default();
    from_help(key, &version, &help)
}
