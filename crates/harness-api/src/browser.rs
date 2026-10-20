//! Headless browser authentication. This never issues or exposes a Control API bearer token.
use crate::{
    auth::Caller,
    error::{ApiError, ApiResult, Body},
    AppState,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json, Router,
};
use harness_core::control_api::{self, Principal, Scope};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const COOKIE: &str = "__Host-harness";
const TTL: Duration = Duration::from_secs(8 * 3600);

/// Serve-only configuration, deliberately separate from desktop Control API settings.
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserConfig {
    pub ui_dir: PathBuf,
    /// Exact externally reachable origin; required for network binds. No proxy headers trusted.
    pub public_origin: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

struct Session {
    csrf: String,
    expires: Instant,
}
struct Inner {
    password: Option<String>,
    setup: Option<(String, Instant)>,
    sessions: HashMap<String, Session>,
    failures: u32,
    retry_at: Instant,
}

pub struct BrowserAuth {
    inner: Mutex<Inner>,
    password_path: PathBuf,
    pub config: BrowserConfig,
    pub tls: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SessionStatus {
    setup_required: bool,
    pub(crate) authenticated: bool,
    csrf: Option<String>,
}

impl BrowserAuth {
    pub fn open(
        dir: &std::path::Path,
        config: BrowserConfig,
    ) -> Result<(Arc<Self>, Option<String>), String> {
        if config.tls_cert.is_some() != config.tls_key.is_some() {
            return Err("browser.tls_pair_required".into());
        }
        if !config.ui_dir.join("index.html").is_file() {
            return Err("browser.ui_missing".into());
        }
        let password_path = dir.join("config/browser-admin.json");
        let password = match std::fs::read(&password_path) {
            Ok(bytes) => {
                let hash: String = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                PasswordHash::new(&hash).map_err(|e| e.to_string())?;
                Some(hash)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        let code = if password.is_none() {
            Some(control_api::new_session_token()?)
        } else {
            None
        };
        Ok((
            Arc::new(Self {
                tls: config.tls_cert.is_some(),
                config,
                password_path,
                inner: Mutex::new(Inner {
                    password,
                    setup: code
                        .clone()
                        .map(|c| (c, Instant::now() + Duration::from_secs(900))),
                    sessions: HashMap::new(),
                    failures: 0,
                    retry_at: Instant::now(),
                }),
            }),
            code,
        ))
    }

    /// Browser cookies may only act on this exact origin, even when API CORS permits others.
    pub fn same_origin(&self, state: &AppState, headers: &HeaderMap, write: bool) -> bool {
        if headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site")
        {
            return false;
        }
        let Some(origin) = headers.get(header::ORIGIN) else {
            return !write;
        };
        let Ok(origin) = origin.to_str() else {
            return false;
        };
        if let Some(expected) = &self.config.public_origin {
            return origin == expected;
        }
        if !state.loopback {
            return false;
        }
        let scheme = if self.tls { "https" } else { "http" };
        ["127.0.0.1", "localhost", "[::1]"]
            .iter()
            .any(|host| origin == format!("{scheme}://{host}:{}", state.addr.port()))
    }

    pub fn revoke_all(&self) {
        self.inner.lock().unwrap().sessions.clear();
    }

    pub fn session_id(headers: &HeaderMap) -> Option<&str> {
        headers
            .get(header::COOKIE)?
            .to_str()
            .ok()?
            .split(';')
            .find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")))
    }

    pub fn caller(
        &self,
        state: &AppState,
        headers: &HeaderMap,
        method: &Method,
    ) -> ApiResult<Caller> {
        let write = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
        if !self.same_origin(state, headers, write) {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "browser.origin"));
        }
        let inner = self.inner.lock().unwrap();
        let session = Self::session_id(headers)
            .and_then(|id| inner.sessions.get(id))
            .filter(|s| s.expires > Instant::now())
            .ok_or_else(ApiError::unauthorized)?;
        if write
            && !headers
                .get("x-harness-csrf")
                .is_some_and(|v| control_api::ct_eq(v.as_bytes(), session.csrf.as_bytes()))
        {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "browser.csrf"));
        }
        Ok(Caller {
            principal: Principal::Browser,
            scopes: vec![Scope::Admin],
        })
    }

    pub(crate) fn status(&self, headers: &HeaderMap) -> SessionStatus {
        let inner = self.inner.lock().unwrap();
        let session = Self::session_id(headers)
            .and_then(|id| inner.sessions.get(id))
            .filter(|s| s.expires > Instant::now());
        SessionStatus {
            setup_required: inner.password.is_none(),
            authenticated: session.is_some(),
            csrf: session.map(|s| s.csrf.clone()),
        }
    }

    fn sign_in(&self, req: LoginRequest, old: Option<String>) -> ApiResult<(String, String)> {
        // Serialize password work and use a global bounded backoff: spoofed IP/proxy headers
        // cannot bypass it, and attackers cannot queue concurrent Argon2 allocations.
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        if now < inner.retry_at {
            return Err(
                ApiError::new(StatusCode::TOO_MANY_REQUESTS, "browser.rate_limit")
                    .with("seconds", (inner.retry_at - now).as_secs() + 1),
            );
        }
        let valid = if let Some(hash) = &inner.password {
            req.setup_code.is_none()
                && PasswordHash::new(hash).ok().is_some_and(|h| {
                    Argon2::default()
                        .verify_password(req.password.as_bytes(), &h)
                        .is_ok()
                })
        } else {
            inner.setup.as_ref().is_some_and(|(code, expires)| {
                *expires > now
                    && req
                        .setup_code
                        .as_ref()
                        .is_some_and(|given| control_api::ct_eq(code.as_bytes(), given.as_bytes()))
            })
        };
        if !valid {
            inner.failures = (inner.failures + 1).min(6);
            inner.retry_at = now + Duration::from_secs(1 << inner.failures);
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "browser.credentials",
            ));
        }
        if inner.password.is_none() {
            if !(12..=1024).contains(&req.password.len()) {
                return Err(ApiError::bad_request("browser.password_length"));
            }
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|_| ApiError::core("api.random_failed"))?;
            let salt = SaltString::encode_b64(&bytes).map_err(|e| ApiError::core(e.to_string()))?;
            let hash = Argon2::default()
                .hash_password(req.password.as_bytes(), &salt)
                .map_err(|e| ApiError::core(e.to_string()))?
                .to_string();
            let tmp = self.password_path.with_extension("tmp");
            harness_core::instance::write_private_file(&tmp, &serde_json::to_vec(&hash).unwrap())?;
            std::fs::rename(&tmp, &self.password_path)
                .map_err(|e| ApiError::core(e.to_string()))?;
            inner.password = Some(hash);
            inner.setup = None;
        }
        inner.failures = 0;
        inner.retry_at = now;
        inner.sessions.retain(|_, s| s.expires > now);
        if let Some(old) = old {
            inner.sessions.remove(&old);
        }
        // Bound memory; do not silently evict another live browser.
        if inner.sessions.len() >= 64 {
            return Err(ApiError::conflict("browser.session_limit"));
        }
        let id = control_api::new_session_token()?;
        let csrf = control_api::new_session_token()?;
        inner.sessions.insert(
            id.clone(),
            Session {
                csrf: csrf.clone(),
                expires: now + TTL,
            },
        );
        Ok((id, csrf))
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    password: String,
    setup_code: Option<String>,
}

fn browser(s: &AppState) -> ApiResult<Arc<BrowserAuth>> {
    s.browser
        .clone()
        .ok_or_else(|| ApiError::not_found("browser", "disabled"))
}

#[utoipa::path(get, path = "/browser/session", tag = "system", security(()), responses((status = 200, body = SessionStatus)))]
pub async fn session(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<SessionStatus>> {
    Ok(Json(browser(&s)?.status(&headers)))
}

#[utoipa::path(post, path = "/browser/login", tag = "system", security(()), request_body = LoginRequest, responses((status = 200, body = SessionStatus), (status = 401, body = crate::error::ErrorBody), (status = 403, body = crate::error::ErrorBody), (status = 429, body = crate::error::ErrorBody)))]
pub async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Body(req): Body<LoginRequest>,
) -> ApiResult<Response> {
    let auth = browser(&s)?;
    if !auth.tls && !s.loopback {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "browser.https_required",
        ));
    }
    if req.password.len() > 1024 {
        return Err(ApiError::bad_request("browser.password_length"));
    }
    let old = BrowserAuth::session_id(&headers).map(str::to_owned);
    let worker = auth.clone();
    let result = tokio::task::spawn_blocking(move || worker.sign_in(req, old))
        .await
        .map_err(|e| ApiError::core(e.to_string()))?;
    let (id, csrf) = match result {
        Ok(v) => v,
        Err(err) => {
            let retry = err.params.get("seconds").and_then(|s| s.as_u64());
            let mut response = err.into_response();
            if let Some(seconds) = retry {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, seconds.to_string().parse().unwrap());
            }
            return Ok(response);
        }
    };
    let mut response = Json(SessionStatus {
        setup_required: false,
        authenticated: true,
        csrf: Some(csrf),
    })
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!(
            "{COOKIE}={id}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={}",
            TTL.as_secs()
        )
        .parse()
        .unwrap(),
    );
    Ok(response)
}

#[utoipa::path(post, path = "/browser/logout", tag = "system", responses((status = 204)))]
pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let auth = browser(&s)?;
    if let Some(id) = BrowserAuth::session_id(&headers) {
        auth.inner.lock().unwrap().sessions.remove(id);
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!("{COOKIE}=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0")
            .parse()
            .unwrap(),
    );
    Ok(response)
}

#[utoipa::path(delete, path = "/browser/sessions", tag = "system", responses((status = 204)))]
pub async fn revoke(State(s): State<AppState>) -> ApiResult<StatusCode> {
    browser(&s)?.revoke_all();
    Ok(StatusCode::NO_CONTENT)
}

/// Same-origin policy covers login too (including login CSRF); private responses never cache.
pub async fn guard(State(s): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(auth) = &s.browser {
        let write = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
        let path = req.uri().path();
        let browser_request = !req.headers().contains_key(header::AUTHORIZATION)
            || path.ends_with("/browser/login")
            || path.ends_with("/browser/session");
        if browser_request
            && path.starts_with("/api/harness/v1/browser/")
            && !auth.same_origin(&s, req.headers(), write)
        {
            return ApiError::new(StatusCode::FORBIDDEN, "browser.origin").into_response();
        }
    }
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        "frame-ancestors 'none'; object-src 'none'; base-uri 'self'"
            .parse()
            .unwrap(),
    );
    response
}

pub fn static_routes(router: Router, config: &BrowserConfig) -> Router {
    use tower::ServiceExt;
    let dir = config.ui_dir.clone();
    router.fallback(move |req: Request| {
        let service = tower_http::services::ServeDir::new(dir.clone());
        async move {
            if req.uri().path() == control_api::BASE_PATH
                || req
                    .uri()
                    .path()
                    .starts_with(&format!("{}/", control_api::BASE_PATH))
            {
                return ApiError::new(StatusCode::NOT_FOUND, "api.no_route").into_response();
            }
            service
                .oneshot(req)
                .await
                .unwrap()
                .map(axum::body::Body::new)
        }
    })
}

/// Only non-secret launch defaults; never serialize AppSettings to a browser.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BasicSettings {
    default_ctx: u32,
    default_ngl: u32,
}

#[utoipa::path(get, path = "/browser/settings", tag = "system", responses((status = 200, body = BasicSettings)))]
pub async fn settings() -> ApiResult<Json<BasicSettings>> {
    let settings = crate::error::blocking(harness_core::config::AppSettings::load).await?;
    Ok(Json(BasicSettings {
        default_ctx: settings.server.default_ctx,
        default_ngl: settings.server.default_ngl,
    }))
}

#[utoipa::path(put, path = "/browser/settings", tag = "system", request_body = BasicSettings, responses((status = 204)))]
pub async fn save_settings(Body(req): Body<BasicSettings>) -> ApiResult<StatusCode> {
    if req.default_ctx > 2097152 || req.default_ngl > 999 {
        return Err(ApiError::bad_request("browser.invalid_settings"));
    }
    crate::error::blocking(move || {
        let mut settings = harness_core::config::AppSettings::load();
        settings.server.default_ctx = req.default_ctx;
        settings.server.default_ngl = req.default_ngl;
        settings.save()
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}
/// Recheck every cookie-authenticated SSE stream, including logs and telemetry, not just /events.
pub(crate) fn protect_stream(
    response: Response,
    auth: Arc<BrowserAuth>,
    headers: HeaderMap,
) -> Response {
    use futures_util::StreamExt;
    if !response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v.as_bytes().starts_with(b"text/event-stream"))
    {
        return response;
    }
    let (parts, body) = response.into_parts();
    let stream = futures_util::stream::unfold(
        (body.into_data_stream(), auth, headers),
        |(mut body, auth, headers)| async move {
            loop {
                if !auth.status(&headers).authenticated {
                    return None;
                }
                let item = tokio::select! {
                    item = body.next() => item,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                };
                if !auth.status(&headers).authenticated {
                    return None;
                }
                return item.map(|item| (item, (body, auth, headers)));
            }
        },
    );
    Response::from_parts(parts, axum::body::Body::from_stream(stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> (tempfile::TempDir, Arc<BrowserAuth>, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "ui").unwrap();
        let (auth, code) = BrowserAuth::open(
            dir.path(),
            BrowserConfig {
                ui_dir: dir.path().into(),
                ..Default::default()
            },
        )
        .unwrap();
        (dir, auth, code.unwrap())
    }
    fn req(code: Option<String>, password: &str) -> LoginRequest {
        LoginRequest {
            setup_code: code,
            password: password.into(),
        }
    }

    #[test]
    fn setup_is_one_time_private_and_survives_restart() {
        let (dir, auth, code) = auth();
        let (id, csrf) = auth
            .sign_in(req(Some(code.clone()), "test-password-1234"), None)
            .unwrap();
        assert_ne!(id, csrf);
        assert!(auth.inner.lock().unwrap().setup.is_none());
        let path = dir.path().join("config/browser-admin.json");
        assert!(harness_core::instance::is_private_to_current_user(&path).unwrap());
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("$argon2id$"));
        assert!(!saved.contains("test-password-1234"));
        assert!(!saved.contains(&code));
        assert!(auth
            .sign_in(req(Some(code), "test-password-1234"), None)
            .is_err());
        let (reopened, code) = BrowserAuth::open(dir.path(), auth.config.clone()).unwrap();
        assert!(code.is_none());
        assert!(reopened.inner.lock().unwrap().sessions.is_empty());
        assert!(reopened
            .sign_in(req(None, "test-password-1234"), None)
            .is_ok());
    }

    #[test]
    fn setup_expiry_and_backoff_do_not_consume_code() {
        let (_dir, auth, code) = auth();
        assert_eq!(
            auth.sign_in(req(Some("wrong".into()), "test-password-1234"), None)
                .unwrap_err()
                .status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            auth.sign_in(req(Some(code.clone()), "test-password-1234"), None)
                .unwrap_err()
                .status,
            StatusCode::TOO_MANY_REQUESTS
        );
        {
            let mut inner = auth.inner.lock().unwrap();
            inner.retry_at = Instant::now();
            assert!(inner.setup.is_some());
            inner.setup.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        }
        assert_eq!(
            auth.sign_in(req(Some(code), "test-password-1234"), None)
                .unwrap_err()
                .status,
            StatusCode::UNAUTHORIZED
        );
        assert!(auth.inner.lock().unwrap().password.is_none());
    }

    #[test]
    fn session_rotation_expiry_and_revocation() {
        let (_dir, auth, code) = auth();
        let (first, _) = auth
            .sign_in(req(Some(code), "test-password-1234"), None)
            .unwrap();
        let (second, _) = auth
            .sign_in(req(None, "test-password-1234"), Some(first.clone()))
            .unwrap();
        assert!(!auth.inner.lock().unwrap().sessions.contains_key(&first));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{COOKIE}={second}").parse().unwrap(),
        );
        assert!(auth.status(&headers).authenticated);
        auth.inner
            .lock()
            .unwrap()
            .sessions
            .get_mut(&second)
            .unwrap()
            .expires = Instant::now();
        assert!(!auth.status(&headers).authenticated);
        auth.sign_in(req(None, "test-password-1234"), None).unwrap();
        auth.revoke_all();
        assert!(auth.inner.lock().unwrap().sessions.is_empty());
    }
}
