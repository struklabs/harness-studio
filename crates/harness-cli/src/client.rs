//! Typed Control API client (JSON in, JSON out; errors mapped to exit codes).
//!
//! Responses are handled as `serde_json::Value`: `--json` prints the API's own schema (the
//! OpenAPI document at `/api/harness/v1/openapi.json` describes it), and the human output reads
//! the few fields it shows. Core types that implement `Deserialize` are used where the CLI
//! works with the values (tokens, downloads, benchmark runs).

use std::time::Duration;

use harness_core::control_api::BASE_PATH;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{CliError, CliResult};

#[derive(Clone)]
pub struct ApiClient {
    /// `http://host:port/api/harness/v1`
    pub base: String,
    token: String,
    http: reqwest::Client,
}

/// `host:port`, `http(s)://host:port` or a full base URL → the API base URL.
pub fn normalize_base(raw: &str) -> CliResult<String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(CliError::usage("cli.invalid_argument", "--host is empty"));
    }
    let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{}", raw)
    };
    let base = match with_scheme.find(BASE_PATH) {
        Some(i) => with_scheme[..i].to_string(),
        None => with_scheme,
    };
    // Default port of the Control API when none is given.
    let authority = base.split("://").nth(1).unwrap_or_default();
    let has_port = authority
        .rsplit_once(':')
        .is_some_and(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && !authority.ends_with(']');
    let base = if has_port {
        base
    } else {
        format!("{}:{}", base, harness_core::control_api::DEFAULT_PORT)
    };
    Ok(format!("{}{}", base, BASE_PATH))
}

/// Percent-encodes one path segment (model ids are file paths).
pub fn seg(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

impl ApiClient {
    pub fn new(base: String, token: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(180))
            // A local instance must not be reached through a system proxy.
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self { base, token, http }
    }

    /// `http://host:port` (without the API base path).
    /// The token this client sends (session or named token).
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn origin(&self) -> &str {
        self.base.strip_suffix(BASE_PATH).unwrap_or(&self.base)
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> CliResult<Value> {
        let url = format!("{}{}", self.base, path);
        let mut req = self.http.request(method, &url).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| connection_error(&url, &e))?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|e| connection_error(&url, &e))?;
        let value: Value = if bytes.iter().all(u8::is_ascii_whitespace) {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        if (200..300).contains(&status) {
            Ok(value)
        } else if value.get("error").is_some() {
            Err(CliError::from_api(status, &value))
        } else {
            Err(CliError::new(
                crate::error::exit_for_status(status),
                "api.error",
                format!("HTTP {} from {}", status, url),
            )
            .with("http_status", status))
        }
    }

    pub async fn get(&self, path: &str) -> CliResult<Value> {
        self.request(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> CliResult<Value> {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> CliResult<Value> {
        self.request(Method::PUT, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> CliResult<Value> {
        self.request(Method::DELETE, path, None).await
    }

    pub async fn get_as<T: DeserializeOwned>(&self, path: &str) -> CliResult<T> {
        let v = self.get(path).await?;
        serde_json::from_value(v)
            .map_err(|e| CliError::general("api.unexpected_response", format!("{}: {}", path, e)))
    }
}

fn connection_error(url: &str, e: &reqwest::Error) -> CliError {
    CliError::transport(e, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_forms_normalize_to_the_api_base() {
        assert_eq!(
            normalize_base("127.0.0.1:11500").unwrap(),
            "http://127.0.0.1:11500/api/harness/v1"
        );
        assert_eq!(
            normalize_base("http://box.lan:9000/").unwrap(),
            "http://box.lan:9000/api/harness/v1"
        );
        assert_eq!(
            normalize_base("https://box/api/harness/v1").unwrap(),
            "https://box:11500/api/harness/v1"
        );
        assert_eq!(
            normalize_base("[::1]:12000").unwrap(),
            "http://[::1]:12000/api/harness/v1"
        );
        assert!(normalize_base(" ").is_err());
        assert_eq!(seg("C:\\m\\a b.gguf"), "C%3A%5Cm%5Ca%20b.gguf");
    }
}
