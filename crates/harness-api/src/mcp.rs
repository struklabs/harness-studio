//! `POST /api/harness/v1/mcp`: the Harness MCP server over streamable HTTP.
//!
//! * The route sits behind the same guard as every other endpoint: Bearer token, loopback
//!   `Host` check. Any valid token may connect (`read`); each tool then needs its own scope
//!   (`harness_mcp::tools`), taken from the caller's token, and `tools/list` only shows the
//!   tools that token allows.
//! * Tools run by calling this API in process: the request goes through the same router,
//!   guard and handlers as an HTTP client's, with the caller's own token, so scopes and core
//!   rules are enforced twice and identically.
//! * Stateless: no `Mcp-Session-Id`, no GET stream (GET/DELETE → 405). Requests are answered
//!   with `application/json`; notifications with 202. Modern (2026-07-28) requests must mirror
//!   `MCP-Protocol-Version`, `Mcp-Method` and `Mcp-Name` (`harness_mcp::protocol`).
//! * `Origin`: absent (non-browser clients) or one of the configured CORS origins; anything
//!   else is 403 (DNS rebinding / drive-by pages).

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use harness_core::control_api::{Principal, BASE_PATH};
use harness_mcp::protocol::{check_http_headers, error_reply, HttpHeaders, PARSE_ERROR};
use harness_mcp::{Backend, BackendError, BackendResult, BoxFuture, McpServer, Method, Transport};
use serde_json::Value;
use tower::ServiceExt;

use crate::auth::Caller;
use crate::AppState;

/// Calls the Control API router in process with the MCP caller's credentials.
struct InProcess {
    router: Router,
    authorization: HeaderValue,
    host: HeaderValue,
}

impl Backend for InProcess {
    fn request<'a>(
        &'a self,
        method: Method,
        path: &'a str,
        body: Option<Value>,
    ) -> BoxFuture<'a, BackendResult<Value>> {
        Box::pin(async move {
            let bytes = match &body {
                Some(b) => serde_json::to_vec(b).unwrap_or_default(),
                None => Vec::new(),
            };
            let mut builder = Request::builder()
                .method(method.as_str())
                .uri(format!("{}{}", BASE_PATH, path))
                .header(header::AUTHORIZATION, self.authorization.clone())
                .header(header::HOST, self.host.clone());
            if body.is_some() {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
            }
            let req = builder
                .body(Body::from(bytes))
                .map_err(|e| BackendError::new("mcp.internal", e.to_string()))?;
            let resp = self
                .router
                .clone()
                .oneshot(req)
                .await
                .map_err(|e| BackendError::new("mcp.internal", e.to_string()))?;
            let status = resp.status().as_u16();
            let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024)
                .await
                .map_err(|e| BackendError::new("mcp.internal", e.to_string()))?;
            let value: Value = if bytes.iter().all(u8::is_ascii_whitespace) {
                Value::Null
            } else {
                serde_json::from_slice(&bytes)
                    .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
            };
            if (200..300).contains(&status) {
                Ok(value)
            } else {
                let mut e = BackendError::from_api(status, &value);
                if value.get("error").is_none() {
                    e = BackendError::new("api.error", format!("HTTP {} from {}", status, path));
                    e.status = Some(status);
                }
                Err(e)
            }
        })
    }
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

fn json_response(status: u16, body: &Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_vec(body).unwrap_or_default(),
    )
        .into_response()
}

fn origin_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = header_str(headers, "origin") else {
        return true;
    };
    state.cors_origins.iter().any(|o| {
        o.trim_end_matches('/')
            .eq_ignore_ascii_case(origin.trim_end_matches('/'))
    })
}

/// `POST /mcp`: one JSON-RPC request or notification.
pub async fn post(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !origin_allowed(&state, &headers) {
        return json_response(
            403,
            &serde_json::json!({"jsonrpc": "2.0", "error": {"code": -32600,
                "message": "Forbidden: Origin not allowed"}}),
        );
    }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            let r = error_reply(
                &Value::Null,
                PARSE_ERROR,
                &format!("Parse error: {}", e),
                None,
                400,
            );
            return json_response(r.status, &r.body);
        }
    };
    let http = HttpHeaders {
        protocol_version: header_str(&headers, "mcp-protocol-version"),
        method: header_str(&headers, "mcp-method"),
        name: header_str(&headers, "mcp-name"),
    };
    if let Err(r) = check_http_headers(&http, &msg) {
        return json_response(r.status, &r.body);
    }
    let authorization = headers
        .get(header::AUTHORIZATION)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static(""));
    let host = HeaderValue::from_str(&format!("127.0.0.1:{}", state.addr.port()))
        .unwrap_or_else(|_| HeaderValue::from_static("127.0.0.1"));
    let backend = InProcess {
        router: crate::routes::router(state.clone(), &[]),
        authorization,
        host,
    };
    let label = match &caller.principal {
        Principal::Session => "mcp".to_string(),
        Principal::Browser => "mcp:browser".to_string(),
        Principal::Named { name, .. } => format!("mcp:{name}"),
    };
    let server = McpServer::new(Arc::new(backend), caller.scopes.clone(), Transport::Http)
        .with_caller_label(label);
    match server.handle(&msg).await {
        Some(reply) => json_response(reply.status, &reply.body),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// GET / DELETE: no standalone SSE stream and no sessions in this server.
pub async fn not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        "",
    )
        .into_response()
}
