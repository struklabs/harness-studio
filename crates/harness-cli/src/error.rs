//! Exit codes and the CLI error type.
//!
//! Every failure ends up as a [`CliError`]: a stable machine `code` (the Control API / core
//! error key when there is one), an English message for humans and the exit code. With
//! `--json` the error is printed to stderr as `{"error": {"code", "message", "params"}}`.

use serde_json::{Map, Value};

/// Fixed exit codes (docs/en/CLI.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Ok = 0,
    General = 1,
    Usage = 2,
    NotFound = 3,
    Conflict = 4,
    OutOfMemory = 5,
    Connection = 6,
}

impl Exit {
    pub fn code(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone)]
pub struct CliError {
    pub exit: Exit,
    /// Stable machine code (`api.not_found`, `cli.no_instance`, `core.error`...).
    pub code: String,
    /// Human-readable text (English; core free-text errors keep their original text).
    pub message: String,
    pub params: Box<Map<String, Value>>,
}

pub type CliResult<T> = Result<T, CliError>;

impl CliError {
    pub fn transport(error: &reqwest::Error, url: &str) -> Self {
        let mut error = Self::core(harness_core::http_error::transport(error, url));
        error.exit = Exit::Connection;
        error
    }

    pub fn new(exit: Exit, code: &str, message: impl Into<String>) -> Self {
        Self {
            exit,
            code: code.to_string(),
            message: message.into(),
            params: Box::default(),
        }
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.params.insert(key.to_string(), value.into());
        self
    }

    pub fn usage(code: &str, message: impl Into<String>) -> Self {
        Self::new(Exit::Usage, code, message)
    }

    pub fn general(code: &str, message: impl Into<String>) -> Self {
        Self::new(Exit::General, code, message)
    }

    pub fn not_found(what: &str, id: &str) -> Self {
        Self::new(
            Exit::NotFound,
            "cli.not_found",
            format!("{} not found: {}", what, id),
        )
        .with("resource", what)
        .with("id", id)
    }

    /// A core error string (`harness_core::error_key`: `"key"`, `"key: detail"`,
    /// `"key {json}"` or free text), as the Control API maps them (harness-api `error.rs`).
    pub fn core(text: impl Into<String>) -> Self {
        let text = text.into();
        match harness_core::error_key::parse(&text) {
            Some((key, params)) => Self::from_code(
                exit_for_status(harness_core::error_key::http_status(key)),
                key,
                params,
            ),
            None => Self::from_code(
                Exit::General,
                "core.error",
                Map::from_iter([("message".to_string(), Value::String(text))]),
            ),
        }
    }

    /// An error with a known code: the message comes from [`crate::messages`].
    pub fn from_code(exit: Exit, code: &str, params: Map<String, Value>) -> Self {
        let exit = if crate::messages::is_out_of_memory(code, &params) {
            Exit::OutOfMemory
        } else {
            exit
        };
        Self {
            exit,
            message: crate::messages::render(code, &params),
            code: code.to_string(),
            params: Box::new(params),
        }
    }

    /// The Control API error body `{"error": {"code", "message_key", "params"}}` with its
    /// HTTP status.
    pub fn from_api(status: u16, body: &Value) -> Self {
        let err = &body["error"];
        let code = err["code"].as_str().unwrap_or("api.error").to_string();
        let params = err["params"].as_object().cloned().unwrap_or_default();
        Self::from_code(exit_for_status(status), &code, params).with("http_status", status)
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "error": {
                "code": self.code,
                "message": self.message,
                "exit_code": self.exit.code(),
                "params": self.params,
            }
        })
    }
}

pub fn exit_for_status(status: u16) -> Exit {
    match status {
        200..=299 => Exit::Ok,
        400 => Exit::Usage,
        404 => Exit::NotFound,
        409 => Exit::Conflict,
        507 => Exit::OutOfMemory,
        502..=504 => Exit::Connection,
        _ => Exit::General,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transport_errors_preserve_connection_exit_and_hide_raw_detail() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        drop(listener);
        let raw = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap_err();
        let error = CliError::transport(&raw, &url);
        assert_eq!(error.exit, Exit::Connection);
        assert_eq!(error.code, "http.connect_failed_local");
        assert!(error.message.contains(&url));
        assert!(!error.message.contains(&raw.to_string()));
        assert!(error.params.contains_key("transport_detail"));
    }

    #[test]
    fn core_keys_map_to_exit_codes() {
        let e = CliError::core("models.in_use: default");
        assert_eq!(e.exit, Exit::Conflict);
        assert_eq!(e.code, "models.in_use");
        assert_eq!(e.params["detail"], "default");
        assert!(e.message.contains("in use"), "{}", e.message);
        assert_eq!(
            CliError::core("definitions.alias_not_found").exit,
            Exit::NotFound
        );
        let e = CliError::core("Profil bulunamadı: x");
        assert_eq!(e.code, "core.error");
        assert_eq!(e.exit, Exit::General);
        assert_eq!(e.message, "Profil bulunamadı: x");
    }

    #[test]
    fn api_bodies_map_to_exit_codes() {
        let body = serde_json::json!({"error": {"code": "api.not_found", "message_key": "api.not_found",
            "params": {"resource": "server", "id": "x"}}});
        let e = CliError::from_api(404, &body);
        assert_eq!(e.exit, Exit::NotFound);
        assert_eq!(e.message, "server not found: x");
        let body = serde_json::json!({"error": {"code": "api.unauthorized", "params": {}}});
        assert_eq!(CliError::from_api(401, &body).exit, Exit::General);
        let body = serde_json::json!({"error": {"code": "servers.already_active", "params": {}}});
        assert_eq!(CliError::from_api(409, &body).exit, Exit::Conflict);
    }
}
