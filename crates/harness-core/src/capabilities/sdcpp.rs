//! Probe of an `sd-server` binary (stable-diffusion.cpp).
//!
//! `sd-server` has no `--version` (an unknown argument prints the usage and exits 0); the first
//! line of `--help` carries the commit (`stable-diffusion.cpp version unknown, commit
//! 1b0ba10`), the release tag the registry records is the real version. Its flag table is
//! `  -m, --model <string>      description`, with the description wrapped onto continuation
//! lines far to the right; the sections are `Svr Options`, `Context Options` and
//! `Default Generation Options`:
//!
//! ```text
//!   --sampling-method                        sampling method, one of [euler, euler_a, heun],
//!                                            default: euler for Flux/SD3/Wan, euler_a otherwise
//!   --cfg-scale <float>                      unconditional guidance scale: (default: 7.0)
//! ```
//!
//! Options that take a value but print no placeholder (`--sampling-method`, `--rng`,
//! `--prediction`, `--seed`...) are told apart from switches by the descriptions' wording, see
//! [`takes_value`].

use super::parse::{FlagInfo, VersionInfo};
use super::{BinaryKey, RuntimeCapabilities, SCHEMA_VERSION};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// Flags that take a value although `--help` prints no `<placeholder>` for them. The table has
/// no other tell: a switch and a value-less-looking option share the same layout.
const VALUE_FLAGS: &[&str] = &[
    "--type",
    "--rng",
    "--sampler-rng",
    "--prediction",
    "--lora-apply-mode",
    "--auto-fit",
    "--batched-cfg",
    "--seed",
    "--sampling-method",
    "--high-noise-sampling-method",
    "--scheduler",
    "--sigmas",
    "--hires-sigmas",
    "--skip-layers",
    "--high-noise-skip-layers",
    "--ref-image",
    "--ref-video",
    "--ref-video-audio",
    "--ref-audio",
    "--audio",
    "--cache-mode",
    "--cache-option",
    "--scm-mask",
    "--scm-policy",
    "--vae-tile-size",
    "--vae-relative-tile-size",
    "--prompt-file",
    "--negative-prompt-file",
    "--log-level",
    "--linear-scale",
    "--attn-scale",
    "--image-preprocess",
    "--extra-sample-args",
    "--extra-tiling-args",
    "--ref-image-args",
    "--hires-upscaler",
    "--hires-sigmas",
    "--tokenizer",
];

/// Whether a flag takes a value: a printed `<placeholder>`, or one of the [`VALUE_FLAGS`].
fn takes_value(canonical: &str, hint: &Option<String>) -> bool {
    hint.is_some() || VALUE_FLAGS.contains(&canonical)
}

/// `one of [a, b, c]` of a description (the list may have been wrapped; the lines are joined).
fn enum_values(description: &str) -> Vec<String> {
    static ONE_OF: OnceLock<Regex> = OnceLock::new();
    let re =
        ONE_OF.get_or_init(|| Regex::new(r"one of \[([^\]]+)\]").expect("valid one-of pattern"));
    re.captures(description)
        .map(|c| {
            c[1].split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// `on|off` of an `--auto-fit` style description.
fn on_off(description: &str) -> Vec<String> {
    if description.trim_start().starts_with("on|off") {
        vec!["on".into(), "off".into()]
    } else {
        Vec::new()
    }
}

/// `(default: 7.0)` / `default: euler for ...` / `(default is ...)`: the first word after it.
fn default_of(description: &str) -> Option<String> {
    static DEFAULT: OnceLock<Regex> = OnceLock::new();
    let re = DEFAULT.get_or_init(|| {
        Regex::new(r"default(?: is|:)\s+(\S+?)[,;)]?(?:\s|$)").expect("valid default pattern")
    });
    re.captures(description)
        .map(|c| c[1].trim_matches(['"', '.']).to_string())
        .filter(|d| !d.is_empty())
}

/// Flags of an `sd-server --help` text.
pub fn parse_help(text: &str) -> BTreeMap<String, FlagInfo> {
    static LINE: OnceLock<Regex> = OnceLock::new();
    // Exactly two spaces of indentation, then the flags, then two or more spaces and the text.
    let line_re = LINE.get_or_init(|| {
        Regex::new(r"^ {2}(-\S.*?)(?:\s{2,}(\S.*))?$").expect("valid flag-line pattern")
    });
    let mut out: BTreeMap<String, FlagInfo> = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            current = None;
            continue;
        }
        if !line.starts_with(' ') {
            if let Some(name) = trimmed.strip_suffix("Options:") {
                section = Some(name.trim().to_ascii_lowercase());
            }
            current = None;
            continue;
        }
        if let Some(c) = line_re.captures(line) {
            let mut aliases: Vec<String> = Vec::new();
            let mut hint: Option<String> = None;
            for token in c[1].split_whitespace() {
                let token = token.trim_end_matches(',');
                if token.starts_with('-') {
                    aliases.push(token.to_string());
                } else if !token.is_empty() {
                    hint = Some(token.to_string());
                }
            }
            if aliases.is_empty() {
                continue;
            }
            let canonical = aliases
                .iter()
                .find(|a| a.starts_with("--"))
                .unwrap_or(&aliases[0])
                .clone();
            let description = c.get(2).map(|m| m.as_str().trim().to_string());
            let value_hint = if takes_value(&canonical, &hint) {
                Some(hint.unwrap_or_else(|| "VALUE".to_string()))
            } else {
                None
            };
            out.insert(
                canonical.clone(),
                FlagInfo {
                    canonical: canonical.clone(),
                    aliases,
                    value_hint,
                    description: description.unwrap_or_default(),
                    section: section.clone(),
                    ..Default::default()
                },
            );
            current = Some(canonical);
        } else if let Some(key) = &current {
            // A wrapped description line.
            if let Some(info) = out.get_mut(key) {
                if !info.description.is_empty() {
                    info.description.push(' ');
                }
                info.description.push_str(trimmed);
            }
        }
    }
    for info in out.values_mut() {
        info.enum_values = enum_values(&info.description);
        if info.enum_values.is_empty() {
            info.enum_values = on_off(&info.description);
        }
        info.default = default_of(&info.description);
        info.deprecated = info.description.to_ascii_lowercase().contains("deprecated");
    }
    out
}

/// The commit of the first help line (`stable-diffusion.cpp version unknown, commit 1b0ba10`).
pub fn parse_version(help: &str) -> VersionInfo {
    let line = help
        .lines()
        .find(|l| l.trim_start().starts_with("stable-diffusion.cpp version"))
        .unwrap_or_default();
    let commit = line
        .rsplit_once("commit ")
        .map(|(_, c)| c.trim().to_string())
        .filter(|c| !c.is_empty());
    // `raw` stays empty: the version shown is the release tag the registry recorded.
    VersionInfo {
        commit,
        ..Default::default()
    }
}

/// Capabilities from the raw `--help` output.
pub fn from_help(key: BinaryKey, help: &str) -> RuntimeCapabilities {
    let flags = parse_help(help);
    let version = parse_version(help);
    let mut caps = RuntimeCapabilities {
        schema_version: SCHEMA_VERSION,
        key: BinaryKey {
            commit: version.commit.clone(),
            ..key
        },
        help_parsed: flags.contains_key("--listen-port") || flags.contains_key("--model"),
        version,
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

    /// Real output of `sd-server` `master-956-1b0ba10` (leejet/stable-diffusion.cpp, Windows
    /// x64 Vulkan build).
    fn help() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/sd-server-master-956.help.txt"),
        )
        .unwrap()
    }

    #[test]
    fn parses_the_real_flag_table() {
        let flags = parse_help(&help());
        assert!(flags.len() >= 120, "{} flags", flags.len());
        let model = &flags["--model"];
        assert_eq!(model.aliases, vec!["-m", "--model"]);
        assert_eq!(model.value_hint.as_deref(), Some("<string>"));
        assert_eq!(model.section.as_deref(), Some("context"));
        let listen = &flags["--listen-ip"];
        assert_eq!(listen.aliases, vec!["-l", "--listen-ip"]);
        assert_eq!(listen.section.as_deref(), Some("svr"));
        assert_eq!(listen.default.as_deref(), Some("127.0.0.1"));
        assert_eq!(flags["--listen-port"].default.as_deref(), Some("1234"));
        // A switch has no value; the value-less looking options do.
        assert!(flags["--offload-to-cpu"].value_hint.is_none());
        assert!(flags["--diffusion-fa"].value_hint.is_none());
        assert!(flags["--vae-tiling"].value_hint.is_none());
        assert!(flags["--sampling-method"].value_hint.is_some());
        assert!(flags["--seed"].value_hint.is_some());
        assert_eq!(flags["--cfg-scale"].default.as_deref(), Some("7.0"));
        assert_eq!(
            flags["--vae-format"].enum_values.len(),
            0,
            "an inline list without `one of []` is not an enum"
        );
        // A wrapped list is joined: the whole sampler list is known.
        let sampling = &flags["--sampling-method"];
        assert!(sampling.enum_values.contains(&"euler_a".to_string()));
        assert!(
            sampling.enum_values.contains(&"lms".to_string()),
            "{sampling:?}"
        );
        assert_eq!(flags["--rng"].enum_values, ["std_default", "cuda", "cpu"]);
        assert_eq!(flags["--auto-fit"].enum_values, ["on", "off"]);
        assert!(flags["--clip-on-cpu"].deprecated);
        assert_eq!(
            flags["--steps"].section.as_deref(),
            Some("default generation")
        );
    }

    #[test]
    fn capabilities_answer_flag_questions() {
        let caps = from_help(BinaryKey::default(), &help());
        assert!(caps.help_parsed);
        assert!(caps.has_flag("-m") && caps.has_flag("--diffusion-model"));
        assert!(caps.has_flag("--listen-port"));
        assert!(!caps.has_flag("--api-key"), "sd-server has no auth");
        assert!(!caps.looks_like_server(), "that test is llama.cpp's");
        assert_eq!(caps.version.commit.as_deref(), Some("1b0ba10"));
        assert!(caps.version.raw.is_empty());
        assert!(!from_help(BinaryKey::default(), "garbage").help_parsed);
    }
}
