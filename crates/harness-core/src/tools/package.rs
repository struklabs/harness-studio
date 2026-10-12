//! The tool package format: a folder with `manifest.json`, the code, an
//! optional test and an optional README. A tool written by hand, written by a model and taken
//! from the community is the same folder.
//!
//! ```json
//! {
//!   "name": "url_title",
//!   "version": "1.0.0",
//!   "description": "What the model reads to decide when to call it.",
//!   "input_schema": { "type": "object", "properties": {}, "required": [] },
//!   "output": "What the result text is.",
//!   "permissions": {
//!     "network": ["example.com", "*.wikipedia.org"],
//!     "fs": { "read": ["."], "write": ["out"] },
//!     "shell": false
//!   },
//!   "runtime": "js",
//!   "entry": "index.js",
//!   "test": "test.js"
//! }
//! ```
//!
//! * `network` lists the host names the code may contact through `harness.http` (`*.x.com` also
//!   admits subdomains). Empty = no network.
//! * `fs.read` / `fs.write` list folders, relative to the chat workspace, the code may read /
//!   write through `harness.fs`. Empty = none; `"."` is the whole workspace.
//! * `shell` must be `false` or absent: this engine never starts a process.
//!
//! Everything here is checked before a package is installed and again when it is loaded; every
//! failure is a keyed error (`tools.package.*`, see [`crate::error_key`]).

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::PermissionClass;
use crate::error_key::err;

pub const MANIFEST_FILE: &str = "manifest.json";
/// Largest manifest, entry or test file read.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_DESCRIPTION: usize = 1024;
const MAX_SCOPES: usize = 16;
const MAX_DOMAINS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsScope {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    /// Allowed host names (`example.com`, `*.example.com`).
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub fs: FsScope,
    /// Always `false` for the JS engine.
    #[serde(default)]
    pub shell: bool,
}

pub const CATEGORIES: &[&str] = &["file", "web", "calculation", "datetime", "text", "other"];

pub fn default_category() -> String {
    "other".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(default = "default_category")]
    pub category: String,
    pub description: String,
    pub input_schema: Value,
    /// What the result text contains (for people and for the model that reads the listing).
    pub output: String,
    #[serde(default)]
    pub permissions: Permissions,
    pub runtime: String,
    pub entry: String,
    #[serde(default)]
    pub test: Option<String>,
}

impl Manifest {
    /// The permission class the tool is registered with: writing files is dangerous; network
    /// access and reading files are restricted; a tool that asks for neither is safe.
    pub fn class(&self) -> PermissionClass {
        let p = &self.permissions;
        if !p.fs.write.is_empty() {
            PermissionClass::Dangerous
        } else if !p.network.is_empty() || !p.fs.read.is_empty() {
            PermissionClass::Restricted
        } else {
            PermissionClass::Safe
        }
    }

    /// The tool works inside the chat workspace (it may read or write files there).
    pub fn uses_workspace(&self) -> bool {
        !self.permissions.fs.read.is_empty() || !self.permissions.fs.write.is_empty()
    }
}

/// A package folder that passed validation.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub manifest: Manifest,
    pub dir: PathBuf,
    pub readme: Option<String>,
}

/// `[a-z][a-z0-9_]{1,47}`: usable as a tool name for the models (function names) and as a
/// folder name on every file system.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && (2..=48).contains(&name.len())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn valid_version(v: &str) -> bool {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    v.len() <= 64
        && parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && v[core.len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.'))
}

/// A relative path without `..`, drive prefixes or a root, made of ordinary names. Judged the
/// same on every OS: `\` and a `:` (`C:/Windows`, which Linux would take as a folder name) are
/// refused everywhere, so a package valid on one system is valid on all.
fn safe_relative(raw: &str) -> bool {
    !raw.is_empty()
        && !raw.contains(['\0', '\\', ':'])
        && Path::new(raw)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn valid_domain(d: &str) -> bool {
    let host = d.strip_prefix("*.").unwrap_or(d);
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// Does `host` fall under one of the allowed `domains`? `example.com` admits only that host,
/// `*.example.com` admits its subdomains (not `example.com` itself).
pub fn domain_allowed(domains: &[String], host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    domains.iter().any(|d| match d.strip_prefix("*.") {
        Some(suffix) => host
            .strip_suffix(suffix)
            .is_some_and(|rest| rest.ends_with('.') && rest.len() > 1),
        None => host == *d,
    })
}

fn bad(key: &'static str, detail: impl std::fmt::Display) -> String {
    err(key).with("detail", detail).into()
}

/// Canonicalize only model-authored HTTP(S) URLs; installed manifests stay strict.
/// URL paths and credentials are not permission scopes. Validate the resulting hosts
/// separately, including duplicate detection, before accepting a proposal or manifest.
pub(super) fn normalize_model_network(network: &mut [String]) -> Result<(), String> {
    for value in network {
        let host = if value.contains("://") {
            let url = reqwest::Url::parse(value)
                .map_err(|_| bad("tools.package.network_invalid", "invalid network URL"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_some_and(|h| h.contains('*'))
            {
                return Err(bad("tools.package.network_invalid", "invalid network URL"));
            }
            url.host_str()
                .ok_or_else(|| bad("tools.package.network_invalid", "missing URL host"))?
                .to_string()
        } else {
            value.clone()
        };
        let bare = host.strip_prefix("*.").unwrap_or(&host);
        if bare.eq_ignore_ascii_case("localhost")
            || bare.to_ascii_lowercase().ends_with(".localhost")
            || bare
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok()
        {
            return Err(bad("tools.package.network_invalid", "IP or localhost host"));
        }
        *value = host;
    }
    Ok(())
}

/// Checks the permission lists of a manifest: host names and workspace folders.
pub fn check_permissions(p: &Permissions) -> Result<(), String> {
    let net = &p.network;
    if net.len() > MAX_DOMAINS || net.iter().any(|d| !valid_domain(d)) {
        let first = net.iter().find(|d| !valid_domain(d)).cloned();
        return Err(bad(
            "tools.package.network_invalid",
            first.unwrap_or_else(|| format!("more than {MAX_DOMAINS} hosts")),
        ));
    }
    if net.iter().collect::<BTreeSet<_>>().len() != net.len() {
        return Err(bad("tools.package.network_invalid", "duplicate host"));
    }
    for scopes in [&p.fs.read, &p.fs.write] {
        if scopes.len() > MAX_SCOPES {
            return Err(bad("tools.package.fs_invalid", "too many folders"));
        }
        if let Some(s) = scopes.iter().find(|s| !safe_relative(s)) {
            return Err(bad("tools.package.fs_invalid", s));
        }
    }
    Ok(())
}

/// Parses and validates a manifest.
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let m: Manifest =
        serde_json::from_str(text).map_err(|e| bad("tools.package.manifest_invalid", e))?;
    if !CATEGORIES.contains(&m.category.as_str()) {
        return Err(bad("tools.package.manifest_invalid", "unknown category"));
    }
    if !valid_name(&m.name) {
        return Err(bad("tools.package.name_invalid", &m.name));
    }
    if !valid_version(&m.version) {
        return Err(bad("tools.package.version_invalid", &m.version));
    }
    if m.description.trim().is_empty() || m.description.chars().count() > MAX_DESCRIPTION {
        return Err(err("tools.package.description_invalid").into());
    }
    if m.output.trim().is_empty() {
        return Err(err("tools.package.output_missing").into());
    }
    if m.input_schema.get("type").and_then(Value::as_str) != Some("object")
        || m.input_schema
            .get("properties")
            .is_some_and(|p| !p.is_object())
        || m.input_schema.get("required").is_some_and(|r| {
            r.as_array()
                .is_none_or(|a| a.iter().any(|n| !n.is_string()))
        })
    {
        return Err(err("tools.package.input_schema_invalid").into());
    }
    if m.runtime != "js" {
        return Err(bad("tools.package.runtime_unsupported", &m.runtime));
    }
    if m.permissions.shell {
        return Err(err("tools.package.shell_not_allowed").into());
    }
    for file in std::iter::once(&m.entry).chain(m.test.as_ref()) {
        if !safe_relative(file) || file == MANIFEST_FILE {
            return Err(bad("tools.package.file_invalid", file));
        }
    }
    check_permissions(&m.permissions)?;
    Ok(m)
}

fn read_text(path: &Path, key: &'static str, shown: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|_| bad(key, shown))?;
    if !meta.is_file() {
        return Err(bad(key, shown));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(bad("tools.package.file_too_large", shown));
    }
    std::fs::read_to_string(path).map_err(|e| bad(key, format!("{shown}: {e}")))
}

/// Loads and validates the package in `dir`: the manifest, that the entry (and the test, when
/// declared) exist inside it and are not symlinks leaving it, and the README when there is one.
pub fn load_dir(dir: &Path) -> Result<Package, String> {
    let text = read_text(
        &dir.join(MANIFEST_FILE),
        "tools.package.manifest_missing",
        MANIFEST_FILE,
    )?;
    let manifest = parse_manifest(&text)?;
    let root = std::fs::canonicalize(dir).map_err(|e| bad("tools.package.manifest_missing", e))?;
    for file in std::iter::once(&manifest.entry).chain(manifest.test.as_ref()) {
        let real = std::fs::canonicalize(dir.join(file))
            .map_err(|_| bad("tools.package.file_missing", file))?;
        if !real.starts_with(&root) || !real.is_file() {
            return Err(bad("tools.package.file_invalid", file));
        }
    }
    let readme = ["README.md", "readme.md"]
        .iter()
        .find_map(|n| read_text(&dir.join(n), "tools.package.file_missing", n).ok());
    Ok(Package {
        manifest,
        dir: dir.to_path_buf(),
        readme,
    })
}

/// The test file's text, `None` when the manifest declares none.
pub fn read_test(pkg: &Package) -> Result<Option<String>, String> {
    pkg.manifest
        .test
        .as_ref()
        .map(|t| read_text(&pkg.dir.join(t), "tools.package.file_missing", t))
        .transpose()
}

/// The entry file's text.
pub fn read_entry(pkg: &Package) -> Result<String, String> {
    read_text(
        &pkg.dir.join(&pkg.manifest.entry),
        "tools.package.file_missing",
        &pkg.manifest.entry,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    type Edit = Box<dyn Fn(&mut Value)>;

    fn base() -> Value {
        json!({
            "name": "url_title",
            "version": "1.2.3",
            "description": "Fetches a page and returns its title.",
            "input_schema": {"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]},
            "output": "The page title.",
            "permissions": {"network": ["example.com", "*.wikipedia.org"], "fs": {"read": ["."], "write": []}},
            "runtime": "js",
            "entry": "index.js"
        })
    }

    fn key_of(v: &Value) -> String {
        let e = parse_manifest(&v.to_string()).unwrap_err();
        crate::error_key::parse(&e).unwrap().0.to_string()
    }

    #[test]
    fn a_complete_manifest_parses_and_gets_its_class() {
        let m = parse_manifest(&base().to_string()).unwrap();
        assert_eq!(m.name, "url_title");
        assert_eq!(m.class(), PermissionClass::Restricted);
        assert!(m.uses_workspace());
        let mut v = base();
        v["permissions"]["fs"]["write"] = json!(["out"]);
        assert_eq!(
            parse_manifest(&v.to_string()).unwrap().class(),
            PermissionClass::Dangerous
        );
        let mut v = base();
        v["permissions"] = json!({});
        let m = parse_manifest(&v.to_string()).unwrap();
        assert_eq!(m.class(), PermissionClass::Safe);
        assert!(!m.uses_workspace());
    }

    #[test]
    fn every_defect_has_its_own_key() {
        let cases: Vec<(&str, Edit)> = vec![
            (
                "tools.package.name_invalid",
                Box::new(|v| v["name"] = json!("Bad Name")),
            ),
            (
                "tools.package.name_invalid",
                Box::new(|v| v["name"] = json!("../x")),
            ),
            (
                "tools.package.version_invalid",
                Box::new(|v| v["version"] = json!("1.0")),
            ),
            (
                "tools.package.description_invalid",
                Box::new(|v| v["description"] = json!("  ")),
            ),
            (
                "tools.package.output_missing",
                Box::new(|v| v["output"] = json!("")),
            ),
            (
                "tools.package.input_schema_invalid",
                Box::new(|v| v["input_schema"] = json!({"type": "string"})),
            ),
            (
                "tools.package.input_schema_invalid",
                Box::new(|v| v["input_schema"]["required"] = json!("url")),
            ),
            (
                "tools.package.runtime_unsupported",
                Box::new(|v| v["runtime"] = json!("python")),
            ),
            (
                "tools.package.shell_not_allowed",
                Box::new(|v| v["permissions"]["shell"] = json!(true)),
            ),
            (
                "tools.package.file_invalid",
                Box::new(|v| v["entry"] = json!("../index.js")),
            ),
            (
                "tools.package.file_invalid",
                Box::new(|v| v["entry"] = json!("/etc/passwd")),
            ),
            (
                "tools.package.file_invalid",
                Box::new(|v| v["test"] = json!("manifest.json")),
            ),
            (
                "tools.package.network_invalid",
                Box::new(|v| v["permissions"]["network"] = json!(["https://example.com"])),
            ),
            (
                "tools.package.network_invalid",
                Box::new(|v| v["permissions"]["network"] = json!(["*"])),
            ),
            (
                "tools.package.network_invalid",
                Box::new(|v| v["permissions"]["network"] = json!(["a.com", "a.com"])),
            ),
            (
                "tools.package.fs_invalid",
                Box::new(|v| v["permissions"]["fs"]["read"] = json!(["../secret"])),
            ),
            (
                "tools.package.fs_invalid",
                Box::new(|v| v["permissions"]["fs"]["write"] = json!(["C:/Windows"])),
            ),
            (
                "tools.package.manifest_invalid",
                Box::new(|v| v["surprise"] = json!(1)),
            ),
            (
                "tools.package.manifest_invalid",
                Box::new(|v| {
                    v.as_object_mut().unwrap().remove("entry");
                }),
            ),
        ];
        for (key, edit) in cases {
            let mut v = base();
            edit(&mut v);
            assert_eq!(key_of(&v), key, "{v}");
        }
        assert!(parse_manifest("not json").is_err());
    }

    #[test]
    fn domains_match_exactly_or_by_wildcard() {
        let d = vec!["example.com".to_string(), "*.wikipedia.org".to_string()];
        assert!(domain_allowed(&d, "example.com"));
        assert!(domain_allowed(&d, "EXAMPLE.com."));
        assert!(domain_allowed(&d, "en.wikipedia.org"));
        assert!(!domain_allowed(&d, "wikipedia.org"));
        assert!(!domain_allowed(&d, "evil-example.com"));
        assert!(!domain_allowed(&d, "example.com.evil.io"));
        assert!(!domain_allowed(&d, "xwikipedia.org"));
        assert!(!domain_allowed(&[], "example.com"));
    }

    #[test]
    fn load_dir_checks_the_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            crate::error_key::parse(&load_dir(dir.path()).unwrap_err())
                .unwrap()
                .0,
            "tools.package.manifest_missing"
        );
        std::fs::write(dir.path().join(MANIFEST_FILE), base().to_string()).unwrap();
        assert_eq!(
            crate::error_key::parse(&load_dir(dir.path()).unwrap_err())
                .unwrap()
                .0,
            "tools.package.file_missing"
        );
        std::fs::write(dir.path().join("index.js"), "function run(a){return 1}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# hi").unwrap();
        let pkg = load_dir(dir.path()).unwrap();
        assert_eq!(pkg.readme.as_deref(), Some("# hi"));
        assert!(read_entry(&pkg).unwrap().contains("run"));
        std::fs::write(
            dir.path().join("index.js"),
            vec![b'a'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(
            crate::error_key::parse(&read_entry(&pkg).unwrap_err())
                .unwrap()
                .0,
            "tools.package.file_too_large"
        );
    }
}
