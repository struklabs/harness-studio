//! Parsers for the text a llama-server binary prints for `--version`, `--help` and
//! `--list-devices`. They only look at the layout llama.cpp's `common/arg.cpp` prints
//! (flag header at column 0, indented continuation lines), never at specific flag names,
//! so new flags of future builds show up without code changes.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VersionInfo {
    /// First line of the `--version` output, verbatim.
    pub raw: String,
    /// `0.5.0-dev` on current builds; the build number on old ones (`version: 4589 (hash)`).
    pub version: Option<String>,
    pub build: Option<u64>,
    pub commit: Option<String>,
    /// `built with Clang 20.1.8 for Windows x86_64`
    pub built_with: Option<String>,
}

/// One flag of `--help`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FlagInfo {
    /// First long (`--x`) spelling that is not a `--no-` negation; else the first spelling.
    pub canonical: String,
    /// Every spelling on the header line (including `canonical` and negations), in order.
    pub aliases: Vec<String>,
    /// Negative spellings (`--no-x`, `-no-x`).
    #[serde(default)]
    pub negations: Vec<String>,
    /// Value placeholder (`N`, `FNAME`, `[on|off|auto]`), `None` for switches.
    pub value_hint: Option<String>,
    /// The value is optional (`[on|off|auto]`).
    #[serde(default)]
    pub value_optional: bool,
    /// Allowed values when the help lists them (hint, "allowed values:" or a `- x:` list).
    #[serde(default)]
    pub enum_values: Vec<String>,
    pub env: Option<String>,
    pub default: Option<String>,
    /// Description with continuation lines joined by single spaces.
    pub description: String,
    /// `common`, `sampling`, `speculative`, `example-specific`, ...
    pub section: Option<String>,
    /// Marked `(DEPRECATED)` in the help.
    #[serde(default)]
    pub deprecated: bool,
    /// Still listed but "the argument has been removed"; never emitted.
    #[serde(default)]
    pub removed: bool,
}

/// A backend device from `--list-devices`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Device {
    /// `Vulkan0`, `CUDA1`, ...
    pub id: String,
    /// `Vulkan`, `CUDA`, `ROCm`, `Metal`, `SYCL`, ...
    pub backend: String,
    pub name: String,
    pub total_mib: Option<u64>,
    pub free_mib: Option<u64>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

pub fn parse_version(text: &str) -> VersionInfo {
    static VERSION: OnceLock<Regex> = OnceLock::new();
    static BUILD: OnceLock<Regex> = OnceLock::new();
    static COMMIT: OnceLock<Regex> = OnceLock::new();
    let mut info = VersionInfo::default();
    for line in text.lines().map(str::trim) {
        if line.starts_with("built with") {
            info.built_with = Some(line.to_string());
            continue;
        }
        let Some(caps) = re(&VERSION, r"^version:\s*(\S+)(?:\s*\(([^)]*)\))?").captures(line)
        else {
            continue;
        };
        info.raw = line.to_string();
        let version = caps[1].to_string();
        let inner = caps.get(2).map_or("", |m| m.as_str());
        if let Some(b) = re(&BUILD, r"build\s+(\d+)").captures(inner) {
            info.build = b[1].parse().ok();
        }
        if let Some(c) = re(&COMMIT, r"commit\s+([0-9a-fA-F]+)").captures(inner) {
            info.commit = Some(c[1].to_string());
        }
        // Old format: `version: 4589 (a1b2c3d4)` = build number + commit hash.
        if info.build.is_none() && version.bytes().all(|b| b.is_ascii_digit()) {
            info.build = version.parse().ok();
            let hash = inner.trim();
            if !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                info.commit = Some(hash.to_string());
            }
        }
        info.version = Some(version);
    }
    info
}

pub fn parse_list_devices(text: &str) -> Vec<Device> {
    static WITH_MEM: OnceLock<Regex> = OnceLock::new();
    static PLAIN: OnceLock<Regex> = OnceLock::new();
    let with_mem = re(
        &WITH_MEM,
        r"^(\S+?):\s+(.*?)\s*\((\d+)\s*MiB,\s*(\d+)\s*MiB free\)\s*$",
    );
    let plain = re(&PLAIN, r"^([A-Za-z][\w.\[\]:-]*?\d+):\s+(.+?)\s*$");
    let mut devices = Vec::new();
    let mut in_list = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Available devices") {
            in_list = true;
            continue;
        }
        if !in_list || line.is_empty() {
            continue;
        }
        let (id, name, total, free) = if let Some(c) = with_mem.captures(line) {
            (
                c[1].to_string(),
                c[2].to_string(),
                c[3].parse().ok(),
                c[4].parse().ok(),
            )
        } else if let Some(c) = plain.captures(line) {
            (c[1].to_string(), c[2].to_string(), None, None)
        } else {
            continue;
        };
        let backend = id
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .to_string();
        devices.push(Device {
            id,
            backend,
            name,
            total_mib: total,
            free_mib: free,
        });
    }
    devices
}

/// Splits a flag header line into (flag spellings, value hint, description start).
fn parse_header(line: &str) -> Option<(Vec<String>, Option<String>, String)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut flags = Vec::new();
    let mut hint = None;
    loop {
        // One flag token.
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b',' {
            i += 1;
        }
        let token = &line[start..i];
        if !token.starts_with('-') || token.len() < 2 {
            return None;
        }
        flags.push(token.to_string());
        if i < bytes.len() && bytes[i] == b',' {
            i += 1;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            if i >= bytes.len() {
                // Header wrapped after a comma: not a layout llama.cpp prints.
                break;
            }
            continue;
        }
        // After the last flag: " HINT" (single space) or "   description" (2+ spaces).
        let spaces = line[i..].bytes().take_while(|b| *b == b' ').count();
        if spaces == 1 {
            let rest = &line[i + 1..];
            let end = rest.find("  ").unwrap_or(rest.len());
            let h = rest[..end].trim();
            if !h.is_empty() {
                hint = Some(h.to_string());
            }
            i = i + 1 + end;
        }
        break;
    }
    let description = line.get(i..).unwrap_or("").trim().to_string();
    Some((flags, hint, description))
}

/// Values of a hint such as `[on|off|auto]`, `{none,layer,row}`, `<0|1>` or `a,b,c`.
fn hint_enum(hint: &str) -> Vec<String> {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    let token = re(&TOKEN, r"^[a-z0-9][a-z0-9_+.\-]*$");
    let inner = hint
        .trim()
        .trim_start_matches(['[', '{', '<'])
        .trim_end_matches([']', '}', '>']);
    if inner.contains("..") {
        return Vec::new();
    }
    let parts: Vec<&str> = inner
        .split(['|', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() >= 2 && parts.iter().all(|p| token.is_match(p)) {
        parts.into_iter().map(str::to_string).collect()
    } else {
        Vec::new()
    }
}

fn finish_flag(
    flags: Vec<String>,
    hint: Option<String>,
    lines: &[String],
    section: Option<String>,
) -> FlagInfo {
    static ENV: OnceLock<Regex> = OnceLock::new();
    static DEFAULT: OnceLock<Regex> = OnceLock::new();
    static BULLET: OnceLock<Regex> = OnceLock::new();
    static ALLOWED: OnceLock<Regex> = OnceLock::new();

    let joined = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let env_re = re(&ENV, r"\(env:\s*([A-Za-z0-9_]+)\)");
    let env = env_re.captures(&joined).map(|c| c[1].to_string());
    let without_env = env_re.replace_all(&joined, "").trim().to_string();
    let default = re(&DEFAULT, r"default:\s*([^()]*)")
        .captures(&without_env)
        .map(|c| {
            c[1].trim()
                .trim_end_matches([',', ';', '.'])
                .trim()
                .trim_matches('\'')
                .trim_matches('"')
                .to_string()
        })
        .filter(|d| !d.is_empty());

    let mut enum_values = hint.as_deref().map(hint_enum).unwrap_or_default();
    if enum_values.is_empty() {
        if let Some(c) = re(&ALLOWED, r"allowed values:\s*([a-z0-9_,\s.+\-]+)").captures(&joined) {
            enum_values = c[1]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && !s.contains(' '))
                .collect();
        }
    }
    if enum_values.is_empty() {
        let bullet = re(&BULLET, r"^-\s+([a-z0-9][a-z0-9_+\-]*)(?:\s+\(default\))?:");
        enum_values = lines
            .iter()
            .filter_map(|l| bullet.captures(l.trim()).map(|c| c[1].to_string()))
            .collect();
    }

    let is_negation = |f: &str| f.starts_with("--no-") || f.starts_with("-no-");
    let canonical = flags
        .iter()
        .find(|f| f.starts_with("--") && !is_negation(f))
        .or_else(|| flags.iter().find(|f| !is_negation(f)))
        .unwrap_or(&flags[0])
        .clone();
    let negations = flags.iter().filter(|f| is_negation(f)).cloned().collect();
    let value_optional = hint.as_deref().is_some_and(|h| h.starts_with('['));
    let lower = joined.to_lowercase();

    FlagInfo {
        canonical,
        negations,
        value_optional,
        enum_values,
        env,
        default,
        deprecated: joined.contains("DEPRECATED"),
        removed: lower.contains("argument has been removed"),
        description: without_env,
        section,
        aliases: flags,
        value_hint: hint,
    }
}

/// A flag entry being collected: spellings, value hint, description lines, section.
type PendingFlag = (Vec<String>, Option<String>, Vec<String>, Option<String>);

/// Parses `--help` into canonical flag -> info. Lines that are not part of the flag
/// table (banners, section titles, blank lines) are ignored.
pub fn parse_help(text: &str) -> BTreeMap<String, FlagInfo> {
    static SECTION: OnceLock<Regex> = OnceLock::new();
    let section_re = re(&SECTION, r"^-{3,}\s*(.*?)\s*params\s*-{3,}$");
    let mut out = BTreeMap::new();
    let mut section: Option<String> = None;
    let mut current: Option<PendingFlag> = None;

    let flush = |cur: &mut Option<PendingFlag>, out: &mut BTreeMap<String, FlagInfo>| {
        if let Some((flags, hint, lines, sec)) = cur.take() {
            let info = finish_flag(flags, hint, &lines, sec);
            // A spelling listed twice keeps its first definition.
            out.entry(info.canonical.clone()).or_insert(info);
        }
    };

    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(c) = section_re.captures(line.trim()) {
            flush(&mut current, &mut out);
            section = Some(c[1].trim().to_string()).filter(|s| !s.is_empty());
            continue;
        }
        if line.starts_with('-') {
            if let Some((flags, hint, desc)) = parse_header(line) {
                flush(&mut current, &mut out);
                current = Some((flags, hint, vec![desc], section.clone()));
                continue;
            }
        }
        if line.starts_with(char::is_whitespace) {
            if let Some((_, _, lines, _)) = current.as_mut() {
                lines.push(line.to_string());
                continue;
            }
        }
        // Anything else at column 0 ends the current entry.
        flush(&mut current, &mut out);
    }
    flush(&mut current, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
    }

    #[test]
    fn version_real_outputs() {
        let v = parse_version(&data("llama-server-b11236.version.txt"));
        assert_eq!(v.version.as_deref(), Some("0.5.0-dev"));
        assert_eq!(v.build, Some(11236));
        assert_eq!(v.commit.as_deref(), Some("f1ea20621"));
        assert!(v.built_with.as_deref().unwrap().contains("Clang"));
        let v = parse_version(&data("llama-server-b11026.version.txt"));
        assert_eq!(v.build, Some(11026));
        assert_eq!(v.commit.as_deref(), Some("b49650adb"));
    }

    #[test]
    fn version_old_format() {
        // Layout printed by llama.cpp builds before the semantic version was added.
        let v = parse_version("version: 4589 (1a2b3c4d)\nbuilt with MSVC 19.40 for x64\n");
        assert_eq!(v.build, Some(4589));
        assert_eq!(v.commit.as_deref(), Some("1a2b3c4d"));
        assert_eq!(v.version.as_deref(), Some("4589"));
        assert!(parse_version("").build.is_none());
    }

    #[test]
    fn devices_real_outputs() {
        for build in ["b11236", "b11026"] {
            let d = parse_list_devices(&data(&format!("llama-server-{build}.list-devices.txt")));
            assert_eq!(d.len(), 2, "{build}");
            assert_eq!(d[0].id, "Vulkan0");
            assert_eq!(d[0].backend, "Vulkan");
            assert_eq!(d[0].name, "AMD Radeon RX 7900 GRE");
            assert_eq!(d[0].total_mib, Some(16368));
            assert!(d[0].free_mib.is_some());
            assert_eq!(d[1].id, "Vulkan1");
            assert_eq!(d[1].name, "AMD Radeon(TM) Graphics");
        }
        assert!(parse_list_devices("Available devices:\n").is_empty());
    }

    #[test]
    fn help_real_outputs_both_builds() {
        for build in ["b11236", "b11026"] {
            let flags = parse_help(&data(&format!("llama-server-{build}.help.txt")));
            assert!(flags.len() > 250, "{build}: {}", flags.len());

            let fa = &flags["--flash-attn"];
            assert_eq!(fa.aliases, vec!["-fa", "--flash-attn"]);
            assert_eq!(fa.value_hint.as_deref(), Some("[on|off|auto]"));
            assert!(fa.value_optional);
            assert_eq!(fa.enum_values, vec!["on", "off", "auto"]);
            assert_eq!(fa.env.as_deref(), Some("LLAMA_ARG_FLASH_ATTN"));
            assert_eq!(fa.default.as_deref(), Some("auto"));
            assert_eq!(fa.section.as_deref(), Some("common"));

            let kvo = &flags["--kv-offload"];
            assert_eq!(
                kvo.aliases,
                vec!["-kvo", "--kv-offload", "-nkvo", "--no-kv-offload"]
            );
            assert_eq!(kvo.negations, vec!["--no-kv-offload"]);
            assert!(kvo.value_hint.is_none());

            let ctk = &flags["--cache-type-k"];
            assert!(ctk.enum_values.contains(&"q8_0".to_string()));
            assert!(ctk.enum_values.contains(&"iq4_nl".to_string()));
            assert_eq!(ctk.default.as_deref(), Some("f16"));

            let sm = &flags["--split-mode"];
            assert_eq!(sm.enum_values, vec!["none", "layer", "row", "tensor"]);

            let lm = &flags["--load-mode"];
            assert_eq!(
                lm.enum_values,
                vec!["auto", "none", "mmap", "mlock", "mmap+mlock", "dio"]
            );

            let md = &flags["--spec-draft-model"];
            assert!(md.aliases.contains(&"-md".to_string()));
            assert!(md.aliases.contains(&"--model-draft".to_string()));
            assert_eq!(md.value_hint.as_deref(), Some("FNAME"));
            assert_eq!(md.section.as_deref(), Some("speculative"));

            let st = &flags["--spec-type"];
            assert!(st.enum_values.contains(&"draft-simple".to_string()));
            assert!(st.enum_values.contains(&"ngram-mod".to_string()));

            let rf = &flags["--reasoning-format"];
            assert_eq!(rf.enum_values, vec!["none", "deepseek", "deepseek-legacy"]);
            assert_eq!(rf.default.as_deref(), Some("auto"));

            let draft_max = flags
                .values()
                .find(|f| f.aliases.iter().any(|a| a == "--draft-max"))
                .unwrap();
            assert_eq!(draft_max.canonical, "--draft");
            assert!(draft_max.removed);
            assert!(!flags["--spec-draft-n-max"].removed);
            assert!(flags["--defrag-thold"].deprecated);

            let ts = &flags["--tensor-split"];
            assert!(ts.enum_values.is_empty(), "{:?}", ts.enum_values);
            assert!(flags["--device"].enum_values.is_empty());
            assert!(flags["--poll"].enum_values.is_empty());

            let pooling = &flags["--pooling"];
            assert_eq!(
                pooling.enum_values,
                vec!["none", "mean", "cls", "last", "rank"]
            );

            assert_eq!(flags["--fit"].enum_values, vec!["on", "off"]);
            assert_eq!(
                flags["--numa"].enum_values,
                vec!["distribute", "isolate", "numactl"]
            );
            assert!(flags.contains_key("--models-dir"));
            assert!(flags.contains_key("--props"));
            assert!(flags["--tools"].enum_values.is_empty());
            assert_eq!(
                flags["--ctx-size"].default.as_deref(),
                Some("0, 0 = loaded from model")
            );
            // Section banners never become flags.
            assert!(!flags.keys().any(|k| k.starts_with("---")));
            // Descriptions don't swallow the next flag.
            assert!(!flags["--threads"].description.contains("--threads-batch"));
        }
    }

    #[test]
    fn help_old_bare_switch_layout() {
        let help = "----- common params -----\n\n-fa,   --flash-attn                     enable Flash Attention (default: disabled)\n                                        (env: LLAMA_ARG_FLASH_ATTN)\n--no-mmap                               do not memory-map model\n--models-path PATH   dir";
        let flags = parse_help(help);
        let fa = &flags["--flash-attn"];
        assert!(fa.value_hint.is_none());
        assert_eq!(fa.default.as_deref(), Some("disabled"));
        assert_eq!(fa.env.as_deref(), Some("LLAMA_ARG_FLASH_ATTN"));
        assert_eq!(flags["--no-mmap"].canonical, "--no-mmap");
        assert_eq!(flags["--models-path"].value_hint.as_deref(), Some("PATH"));
    }
}
