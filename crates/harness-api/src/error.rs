//! Error responses: `{"error": {"code", "message_key", "params"}}` with a matching HTTP status.
//!
//! Core services return `String` errors that are i18n keys (`harness_core::error_key`):
//! `key`, `key: detail` or `key {"name": "value"}`. The key becomes `code` / `message_key`
//! and its parameters `params` (`key: detail` gives `params.detail`); a parameter can itself be
//! a keyed error string (a nested cause). The few remaining free-text errors (raw OS or
//! library text without context) become `core.error` with the text in `params.message`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable machine code (`api.unauthorized`, `network.lan_without_key`, `fs.read_failed`...).
    /// `core.error` only for the rare core errors that are free text.
    pub code: String,
    /// i18n key of the message (Harness Studio locale files; same as `code`).
    pub message_key: String,
    /// Values for the message's `{name}` placeholders (`path`, `error`, `detail`, `message`,
    /// `scope`...). Technical detail (paths, OS error text) is untranslated; a string value may
    /// itself be a keyed error (`key {"name": "value"}` or `key: detail`), a nested cause.
    #[schema(value_type = Object)]
    pub params: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub params: Map<String, Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &str) -> Self {
        Self {
            status,
            code: code.to_string(),
            params: Map::new(),
        }
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.params.insert(key.to_string(), value.into());
        self
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "api.unauthorized")
    }

    pub fn forbidden_scope(scope: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, "api.insufficient_scope").with("scope", scope)
    }

    pub fn not_found(what: &str, id: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, "api.not_found")
            .with("resource", what)
            .with("id", id)
    }

    pub fn bad_request(code: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }

    pub fn conflict(code: &str) -> Self {
        Self::new(StatusCode::CONFLICT, code)
    }

    /// Maps a core error string (see the module docs).
    pub fn core(error: impl Into<String>) -> Self {
        let text = error.into();
        match harness_core::error_key::parse(&text) {
            Some((key, params)) => Self {
                status: StatusCode::from_u16(harness_core::error_key::http_status(key))
                    .unwrap_or(StatusCode::UNPROCESSABLE_ENTITY),
                code: key.to_string(),
                params,
            },
            None => Self::new(StatusCode::UNPROCESSABLE_ENTITY, "core.error").with("message", text),
        }
    }
}

impl From<String> for ApiError {
    fn from(e: String) -> Self {
        Self::core(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                message_key: self.code.clone(),
                code: self.code,
                params: self.params,
            },
        };
        (self.status, axum::Json(body)).into_response()
    }
}

/// Runs blocking core work off the async threads.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "api.internal")
            .with("message", e.to_string())
    })
}

// --- Extractors whose rejections use the error format ---

use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

/// JSON body (`api.invalid_body` on a malformed or mistyped body).
pub struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Body(v)),
            Err(r) => {
                Err(ApiError::new(r.status(), "api.invalid_body").with("message", r.body_text()))
            }
        }
    }
}

/// Optional JSON body: none (or empty) = `None`.
pub struct OptBody<T>(pub Option<T>);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for OptBody<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        let bytes = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(|r| {
                ApiError::new(r.status(), "api.invalid_body").with("message", r.body_text())
            })?;
        if bytes.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(OptBody(None));
        }
        serde_json::from_slice(&bytes)
            .map(|v| OptBody(Some(v)))
            .map_err(|e| {
                ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "api.invalid_body")
                    .with("message", e.to_string())
            })
    }
}

/// Query string (`api.invalid_query`).
pub struct Q<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for Q<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|q| Q(q.0))
            .map_err(|r| {
                ApiError::new(r.status(), "api.invalid_query").with("message", r.body_text())
            })
    }
}

/// Path parameters (`api.invalid_path`).
pub struct P<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for P<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|p| P(p.0))
            .map_err(|r| {
                ApiError::new(r.status(), "api.invalid_path").with("message", r.body_text())
            })
    }
}

/// A body object with `id` forced to `id` (create: empty = assigned by the core; update: the
/// path id), then typed. Lets clients leave `id` out.
pub fn with_id<T: DeserializeOwned>(mut value: Value, id: &str) -> ApiResult<T> {
    let Some(obj) = value.as_object_mut() else {
        return Err(
            ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "api.invalid_body")
                .with("message", "JSON object expected"),
        );
    };
    obj.insert("id".into(), Value::String(id.to_string()));
    serde_json::from_value(value).map_err(|e| {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "api.invalid_body")
            .with("message", e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_errors_map_to_codes() {
        let e = ApiError::core("network.lan_without_key");
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.code, "network.lan_without_key");
        let e = ApiError::core("models.in_use: default, m-qwen");
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.params["detail"], "default, m-qwen");
        let e = ApiError::core("Profil bulunamadı: x");
        assert_eq!(e.code, "core.error");
        assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(e.params["message"], "Profil bulunamadı: x");
        // Text that merely contains a dot is not a key.
        assert_eq!(ApiError::core("Dosya a.gguf yok").code, "core.error");
        assert_eq!(
            ApiError::core("network.key_generation_failed: os").params["detail"],
            "os"
        );
        let e = ApiError::core(String::from(
            harness_core::error_key::err("fs.not_found").with("path", "x"),
        ));
        assert_eq!(e.status, StatusCode::NOT_FOUND);
        assert_eq!(e.code, "fs.not_found");
        assert_eq!(e.params["path"], "x");
    }
}
