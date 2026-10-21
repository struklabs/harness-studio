//! Device pairing on the gateway: the public claim routes a new device uses, the device's
//! self-report, and the mobile web chat page. The owner's side (offers, approval, devices,
//! audit, revocation) is on the Control API. Protocol: `harness_core::pairing`.
//!
//! * `GET /m`: the chat page for a phone browser (static; it pairs and chats with the
//!   routes below and `/v1/chat/completions`, with the token it stores in the browser).
//! * `POST /v1/harness/pair/claim`: `{code, device_name, platform, client_nonce}`; public.
//! * `GET /v1/harness/pair/claim/{id}`: poll, with the `x-harness-pair-secret` header.
//! * `POST /v1/harness/device/report`: a paired device's self-report; its own token.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_core::pairing::{self, DeviceReport};
use serde::Deserialize;

use crate::errors::GatewayError;
use crate::{auth, GwState};

const CHAT_PAGE: &str = include_str!("mobile_chat.html");

pub fn routes() -> Router<GwState> {
    Router::new()
        .route("/m", get(chat_page))
        .route("/v1/harness/pair/claim", post(claim))
        .route("/v1/harness/pair/claim/{id}", get(poll))
        .route("/v1/harness/device/report", post(report))
}

fn pairing_error(key: &str) -> GatewayError {
    let (status, message) = match key {
        pairing::ERR_INVALID_CODE => (403, "The pairing code is wrong, used or expired."),
        pairing::ERR_LOCKED => (429, "Too many wrong codes. Wait a minute and try again."),
        pairing::ERR_NOT_FOUND => (404, "This pairing request does not exist (any more)."),
        pairing::ERR_TOO_MANY => (429, "Too many pairing requests are waiting."),
        pairing::ERR_DEVICE_NAME => (400, "The device name is empty or too long."),
        pairing::ERR_NONCE => (400, "The client nonce is malformed."),
        _ => (400, "Pairing failed."),
    };
    let code = if key.starts_with("pairing.") {
        key
    } else {
        "pairing.failed"
    };
    GatewayError::new(status, code, message)
}

async fn chat_page() -> Response {
    let mut resp = axum::response::Html(CHAT_PAGE).into_response();
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
             connect-src 'self'; img-src data:; base-uri 'none'; form-action 'none'; \
             frame-ancestors 'none'",
        ),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    resp
}

#[derive(Deserialize)]
struct ClaimBody {
    code: String,
    device_name: String,
    #[serde(default)]
    platform: String,
    client_nonce: String,
}

async fn claim(State(s): State<GwState>, Json(body): Json<ClaimBody>) -> Response {
    let hub = pairing::hub_for(&s.tokens);
    match tokio::task::spawn_blocking(move || {
        hub.claim(
            &body.code,
            &body.device_name,
            &body.platform,
            &body.client_nonce,
        )
    })
    .await
    {
        Ok(Ok(started)) => Json(started).into_response(),
        Ok(Err(key)) => pairing_error(&key).into_response(),
        Err(_) => pairing_error("pairing.failed").into_response(),
    }
}

async fn poll(State(s): State<GwState>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let secret = headers
        .get("x-harness-pair-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let tokens = s.tokens.clone();
    let hub = pairing::hub_for(&tokens);
    match tokio::task::spawn_blocking(move || hub.poll(&id, &secret, &tokens)).await {
        Ok(Ok(answer)) => {
            let mut resp = Json(answer).into_response();
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            resp
        }
        Ok(Err(key)) => pairing_error(&key).into_response(),
        Err(_) => pairing_error("pairing.failed").into_response(),
    }
}

async fn report(
    State(s): State<GwState>,
    headers: HeaderMap,
    Json(body): Json<DeviceReport>,
) -> Response {
    let caller = match auth::whoami(&s, &headers) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    let Some(token_id) = caller.token_id else {
        return GatewayError::new(
            403,
            "pairing.not_a_device",
            "Only a paired device's own token can report.",
        )
        .into_response();
    };
    let hub = pairing::hub_for(&s.tokens);
    match tokio::task::spawn_blocking(move || hub.report(&token_id, body)).await {
        Ok(Ok(true)) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => GatewayError::new(
            403,
            "pairing.not_a_device",
            "Only a paired device's own token can report.",
        )
        .into_response(),
        _ => pairing_error("pairing.failed").into_response(),
    }
}
