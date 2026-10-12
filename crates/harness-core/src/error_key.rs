//! Keyed errors: the core's error strings are i18n keys, not sentences.
//!
//! Core services return `Result<_, String>`. The string is one of
//!
//! * `key` — e.g. `servers.already_active`
//! * `key: detail` — the older form with one free parameter (`models.in_use: default, m-qwen`)
//! * `key {"name":"value",...}` — the key and its parameters as a JSON object
//!
//! where `key` is lowercase `[a-z0-9_]` segments joined by dots. Build them with [`err`]:
//!
//! ```ignore
//! // (not run as a doctest; the unit tests below cover it)
//! use harness_core::error_key::{err, parse};
//! let e: String = err("fs.read_failed").with("path", "C:/x.json").with("error", "denied").into();
//! let (key, params) = parse(&e).unwrap();
//! assert_eq!(key, "fs.read_failed");
//! assert_eq!(params["path"], "C:/x.json");
//! ```
//!
//! The GUI translates the key with its locale files (`src/lib/locales/`, `{name}` placeholders
//! filled from the parameters); the Control API returns `{code, message_key, params}`; the CLI
//! prints the English text. Parameters carry the technical detail (paths, OS error text, ids)
//! untranslated. A parameter can itself be a keyed error string (a nested cause); readers
//! translate it too.

use serde_json::{Map, Value};
use std::fmt;

/// A keyed error under construction.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorKey {
    key: &'static str,
    params: Map<String, Value>,
}

/// Starts a keyed error.
pub fn err(key: &'static str) -> ErrorKey {
    debug_assert!(is_key(key), "not an error key: {key}");
    ErrorKey {
        key,
        params: Map::new(),
    }
}

impl ErrorKey {
    /// Adds a parameter (`{name}` in the message). Values are kept as text.
    pub fn with(mut self, name: &str, value: impl fmt::Display) -> Self {
        self.params
            .insert(name.to_string(), Value::String(value.to_string()));
        self
    }

    /// Adds a numeric parameter.
    pub fn num(mut self, name: &str, value: impl Into<serde_json::Number>) -> Self {
        self.params
            .insert(name.to_string(), Value::Number(value.into()));
        self
    }

    pub fn key(&self) -> &'static str {
        self.key
    }
}

impl fmt::Display for ErrorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.params.is_empty() {
            f.write_str(self.key)
        } else {
            let json = serde_json::to_string(&self.params).map_err(|_| fmt::Error)?;
            write!(f, "{} {}", self.key, json)
        }
    }
}

impl From<ErrorKey> for String {
    fn from(e: ErrorKey) -> Self {
        e.to_string()
    }
}

/// Lowercase `[a-z0-9_]` segments joined by dots (at least two segments).
pub fn is_key(text: &str) -> bool {
    text.contains('.')
        && text.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

/// Splits an error string into its key and parameters; `None` for free text.
/// The `key: detail` form puts the detail in `params.detail`.
pub fn parse(text: &str) -> Option<(&str, Map<String, Value>)> {
    if let Some((key, json)) = text.split_once(' ') {
        if is_key(key) && json.starts_with('{') {
            if let Ok(Value::Object(params)) = serde_json::from_str::<Value>(json) {
                return Some((key, params));
            }
        }
    }
    let (key, detail) = match text.split_once(": ") {
        Some((k, d)) => (k, Some(d)),
        None => (text, None),
    };
    if !is_key(key) {
        return None;
    }
    let mut params = Map::new();
    if let Some(d) = detail {
        params.insert("detail".into(), Value::String(d.to_string()));
    }
    Some((key, params))
}

/// HTTP status of a key (the Control API's status; the CLI derives its exit code from it).
pub fn http_status(code: &str) -> u16 {
    match code {
        "network.lan_without_key" | "network.tools_without_key" => 409,
        "models.in_use"
        | "benchmark.already_running"
        | "servers.already_active"
        | "servers.port_in_use"
        | "servers.port_in_use_by"
        | "servers.port_in_use_at"
        | "servers.port_in_use_server"
        | "servers.alias_in_use"
        | "servers.no_failure"
        | "lora.server_not_running"
        | "router.not_running"
        | "router.not_router"
        | "store.newer_version"
        | "runtime.integrity_unverified"
        | "runtime.in_use"
        | "runtime.id_taken"
        | "runtime.changed"
        | "runtime.quant_unsupported"
        | "runtime.busy"
        | "runtime.stop_servers_first"
        | "runtime.no_backup"
        | "lemonade.busy"
        | "lemonade.in_use"
        | "downloads.variant_ambiguous"
        | "convert.already_running"
        | "convert.job_active"
        | "convert.env_in_use"
        | "compaction.busy"
        | "compaction.stale_path"
        | "compaction.gateway_required" => 409,
        "runtime.no_release" | "runtime.whisper_no_macos" => 404,
        // The tool policy refuses the call (Control API: it cannot ask the user).
        "tools.disabled" | "tools.approval_required" => 403,
        "tools.workdir_invalid" => 400,
        c if c.ends_with(".not_found") || c.ends_with("_not_found") => 404,
        c if c.ends_with(".exists") || c.ends_with(".already_exists") => 409,
        c if c.starts_with("network.") || c.starts_with("api.") || c.starts_with("otel.") => 400,
        _ => 422,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_keys_and_params() {
        assert_eq!(
            err("servers.already_active").to_string(),
            "servers.already_active"
        );
        let e: String = err("fs.write_failed")
            .with("path", "C:\\a b\\c.json")
            .with("error", "Erişim engellendi. (os error 5)")
            .num("port", 8080)
            .into();
        let (key, p) = parse(&e).unwrap();
        assert_eq!(key, "fs.write_failed");
        assert_eq!(p["path"], "C:\\a b\\c.json");
        assert_eq!(p["error"], "Erişim engellendi. (os error 5)");
        assert_eq!(p["port"], 8080);
    }

    #[test]
    fn parses_the_older_forms_and_rejects_free_text() {
        let (k, p) = parse("models.in_use: default, m-qwen").unwrap();
        assert_eq!(k, "models.in_use");
        assert_eq!(p["detail"], "default, m-qwen");
        let (k, p) = parse("runtime.not_found").unwrap();
        assert_eq!(k, "runtime.not_found");
        assert!(p.is_empty());
        assert!(parse("Profil bulunamadı: x").is_none());
        assert!(parse("model.gguf is missing").is_none());
        assert!(parse("Error: x").is_none());
        // A key followed by something that is not a JSON object is the legacy form or text.
        assert!(parse("a.b {not json").is_none());
    }

    #[test]
    fn nested_causes_survive() {
        let inner: String = err("fs.read_failed").with("path", "x").into();
        let outer: String = err("store.load_failed").with("error", &inner).into();
        let (_, p) = parse(&outer).unwrap();
        let (k, p2) = parse(p["error"].as_str().unwrap()).unwrap();
        assert_eq!(k, "fs.read_failed");
        assert_eq!(p2["path"], "x");
    }

    #[test]
    fn statuses() {
        assert_eq!(http_status("models.in_use"), 409);
        assert_eq!(http_status("profiles.not_found"), 404);
        assert_eq!(http_status("api.token_not_found"), 404);
        assert_eq!(http_status("downloads.variant_not_found"), 404);
        assert_eq!(http_status("network.invalid_host"), 400);
        assert_eq!(http_status("fs.read_failed"), 422);
    }
}
