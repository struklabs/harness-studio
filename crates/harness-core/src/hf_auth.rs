//! "Sign in with Hugging Face": OAuth 2.0 Authorization Code + PKCE (RFC 7636) with a loopback
//! redirect (RFC 8252 §7.3), the device code flow (RFC 8628, for terminals) and token refresh.
//!
//! Hugging Face needs a registered OAuth app: a **public** app (no client secret) whose redirect
//! URIs include the port-less `http://127.0.0.1/callback` (any port is accepted at request
//! time). The client id comes from Settings (`hf_oauth_client_id`), the
//! `HARNESS_HF_OAUTH_CLIENT_ID` environment variable, or the same variable at build time, in
//! that order; without one only the "paste an access token" path is available.
//!
//! Storage: the access token lives in the keychain account the pasted token uses
//! (`huggingface_token`), so every Hub call and download uses it unchanged; the OAuth session
//! (access + refresh token, expiry, client id, cached profile) is a second keychain entry
//! (`huggingface_oauth`). Nothing here is written to a JSON file or printed: token types
//! redact themselves in `Debug`, errors only carry the server's `error_description`.
//!
//! Hugging Face has no token revocation endpoint: signing out deletes both entries, and the
//! grant itself is revoked at <https://huggingface.co/settings/connected-applications>.

use crate::config::AppSettings;
use crate::error_key::err;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const AUTHORIZE_URL: &str = "https://huggingface.co/oauth/authorize";
pub const TOKEN_URL: &str = "https://huggingface.co/oauth/token";
pub const DEVICE_URL: &str = "https://huggingface.co/oauth/device";
pub const WHOAMI_URL: &str = "https://huggingface.co/api/whoami-v2";
/// Where the user revokes an OAuth grant (there is no revocation endpoint).
pub const CONNECTED_APPS_URL: &str = "https://huggingface.co/settings/connected-applications";
/// Where the user manages pasted access tokens.
pub const TOKENS_URL: &str = "https://huggingface.co/settings/tokens";
/// Profile, private repos (read) and public gated repos the user was granted.
pub const SCOPES: &str = "openid profile read-repos gated-repos";
/// Redirect URI to register on the OAuth app (the port is chosen per login).
pub const REGISTERED_REDIRECT_URI: &str = "http://127.0.0.1/callback";
pub const CALLBACK_PATH: &str = "/callback";
pub const CLIENT_ID_ENV: &str = "HARNESS_HF_OAUTH_CLIENT_ID";
const BUILD_CLIENT_ID: Option<&str> = option_env!("HARNESS_HF_OAUTH_CLIENT_ID");

/// How long the browser login waits for the redirect.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// Refresh this long before the access token expires.
const REFRESH_MARGIN_SECS: u64 = 120;
const USER_AGENT: &str = "HarnessStudio/0.2.0";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// --- PKCE / state ---

/// Base64url without padding (RFC 4648 §5), as PKCE requires.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// `code_challenge` for a `code_verifier` (method S256).
pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|e| String::from(err("hf_auth.random_failed").with("error", e)))?;
    Ok(base64url(&buf))
}

/// A fresh PKCE pair: 32 random bytes → a 43-character verifier (RFC 7636 §4.1).
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl fmt::Debug for Pkce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pkce { .. }")
    }
}

impl Pkce {
    pub fn new() -> Result<Self, String> {
        let verifier = random_token(32)?;
        let challenge = pkce_challenge(&verifier);
        Ok(Self {
            verifier,
            challenge,
        })
    }
}

/// A random `state` value (CSRF protection for the redirect).
pub fn new_state() -> Result<String, String> {
    random_token(24)
}

/// Constant-time comparison (the state must not leak through timing).
pub fn states_match(expected: &str, got: &str) -> bool {
    let (a, b) = (expected.as_bytes(), got.as_bytes());
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// --- client id ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientIdSource {
    Settings,
    Env,
    Build,
}

/// A plausible OAuth client id: printable ASCII without spaces, at most 256 characters (plain
/// UUID-style ids and CIMD URLs both fit).
pub fn valid_client_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_graphic())
}

/// The client id to use: Settings, then the environment, then the build.
pub fn client_id(settings: &AppSettings) -> Option<(String, ClientIdSource)> {
    let from_settings = settings.hf_oauth_client_id.trim();
    if valid_client_id(from_settings) {
        return Some((from_settings.to_string(), ClientIdSource::Settings));
    }
    if let Ok(v) = std::env::var(CLIENT_ID_ENV) {
        if valid_client_id(v.trim()) {
            return Some((v.trim().to_string(), ClientIdSource::Env));
        }
    }
    BUILD_CLIENT_ID
        .map(str::trim)
        .filter(|v| valid_client_id(v))
        .map(|v| (v.to_string(), ClientIdSource::Build))
}

// --- authorization request / redirect ---

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

pub fn authorize_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<String, String> {
    let mut url = reqwest::Url::parse(AUTHORIZE_URL)
        .map_err(|e| String::from(err("hf.url_build_failed").with("error", e)))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

/// What a request to the loopback listener means.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    /// The authorization code (state verified).
    Code(String),
    /// The user declined or the server refused (state verified): `error`, `error_description`.
    Denied(String, Option<String>),
    /// A request to the callback path whose `state` is missing or wrong: rejected, the login
    /// keeps waiting (a stray or forged request must not end it).
    StateMismatch,
    /// Any other path (favicon...).
    Other,
}

/// Parses the request target of a loopback request (`/callback?code=..&state=..`).
pub fn parse_callback(target: &str, expected_state: &str) -> Callback {
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::Other;
    };
    if url.path() != CALLBACK_PATH {
        return Callback::Other;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" if code.is_none() => code = Some(v.into_owned()),
            "state" if state.is_none() => state = Some(v.into_owned()),
            "error" if error.is_none() => error = Some(v.into_owned()),
            "error_description" if description.is_none() => description = Some(v.into_owned()),
            _ => {}
        }
    }
    if !state.is_some_and(|s| states_match(expected_state, &s)) {
        return Callback::StateMismatch;
    }
    match (error, code) {
        (Some(e), _) => Callback::Denied(e, description),
        (None, Some(c)) if !c.is_empty() => Callback::Code(c),
        _ => Callback::Denied("invalid_request".into(), None),
    }
}

/// The request target from an HTTP/1.x request line (`GET /callback?... HTTP/1.1`).
pub fn request_target(head: &str) -> Option<&str> {
    let line = head.lines().next()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    (method == "GET" && version.starts_with("HTTP/1.") && target.starts_with('/')).then_some(target)
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
<style>body{{font-family:system-ui,sans-serif;max-width:32rem;margin:15vh auto;padding:0 1rem;\
line-height:1.5;color:#1f2328;background:#fff}}@media(prefers-color-scheme:dark){{body{{color:#e6e6e6;\
background:#141414}}}}h1{{font-size:1.25rem}}</style></head><body><h1>{title}</h1>{body}</body></html>"
    )
}

fn http_response(status: &str, html: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
Cache-Control: no-store\r\nConnection: close\r\n\r\n{html}",
        html.len()
    )
}

async fn read_head(stream: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let read = async {
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
                break;
            }
        }
        Some(())
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .ok()??;
    String::from_utf8(buf).ok()
}

/// Serves the loopback redirect until a request with the right state arrives.
pub(crate) async fn wait_for_code(
    listener: tokio::net::TcpListener,
    expected_state: &str,
    provider: &str,
) -> Result<String, String> {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            continue;
        };
        let Some(head) = read_head(&mut stream).await else {
            continue;
        };
        let outcome = request_target(&head)
            .map(|t| parse_callback(t, expected_state))
            .unwrap_or(Callback::Other);
        let (status, html) = match &outcome {
            Callback::Code(_) => (
                "200 OK",
                page(
                    "Harness Studio",
                    &format!("<p>{provider} onayı alındı. Bu sekmeyi kapatıp uygulamaya dönebilirsiniz; giriş orada tamamlanır.</p><p>{provider} authorization received. You can close this tab and return to the app, where the sign-in completes.</p>"),
                ),
            ),
            Callback::Denied(..) => (
                "200 OK",
                page(
                    "Harness Studio",
                    "<p>Giriş tamamlanmadı. Uygulamaya dönüp yeniden deneyebilirsiniz.</p>\
<p>Sign-in was not completed. Return to the app to try again.</p>",
                ),
            ),
            Callback::StateMismatch => (
                "400 Bad Request",
                page(
                    "Harness Studio",
                    "<p>Bu istek geçerli bir giriş oturumuna ait değil.</p>\
<p>This request does not belong to an active sign-in.</p>",
                ),
            ),
            Callback::Other => ("404 Not Found", page("404", "")),
        };
        let _ = stream
            .write_all(http_response(status, &html).as_bytes())
            .await;
        let _ = stream.shutdown().await;
        match outcome {
            Callback::Code(code) => return Ok(code),
            Callback::Denied(e, d) => {
                return Err(err("hf_auth.denied")
                    .with("error", d.map(|d| format!("{e}: {d}")).unwrap_or(e))
                    .into())
            }
            Callback::StateMismatch | Callback::Other => {}
        }
    }
}

// --- tokens ---

/// A token endpoint response.
#[derive(Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("scope", &self.scope)
            .finish()
    }
}

#[derive(Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| crate::http_error::transport(&e, TOKEN_URL))
}

/// Posts a form to the token endpoint. `Ok(Err(oauth_error))` for an OAuth error response.
async fn token_request(
    form: &[(&str, &str)],
) -> Result<Result<TokenResponse, (String, Option<String>)>, String> {
    let res = client()?
        .post(TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, TOKEN_URL))?;
    let status = res.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(rate_limited(res.headers()));
    }
    let body = res
        .bytes()
        .await
        .map_err(|e| crate::http_error::transport(&e, TOKEN_URL))?;
    if status.is_success() {
        return serde_json::from_slice::<TokenResponse>(&body)
            .map(Ok)
            .map_err(|_| err("hf_auth.token_failed").with("error", status).into());
    }
    match serde_json::from_slice::<OAuthError>(&body) {
        Ok(e) => Ok(Err((e.error, e.error_description))),
        Err(_) => Err(err("hf_auth.token_failed").with("error", status).into()),
    }
}

fn oauth_error_text(error: &str, description: Option<&str>) -> String {
    match description {
        Some(d) if !d.is_empty() => format!("{error}: {d}"),
        _ => error.to_string(),
    }
}

pub async fn exchange_code(
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    token_request(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ])
    .await?
    .map_err(|(e, d)| {
        err("hf_auth.token_failed")
            .with("error", oauth_error_text(&e, d.as_deref()))
            .into()
    })
}

pub async fn refresh_tokens(client_id: &str, refresh_token: &str) -> Result<TokenResponse, String> {
    token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ])
    .await?
    .map_err(|(e, d)| {
        err("hf_auth.refresh_failed")
            .with("error", oauth_error_text(&e, d.as_deref()))
            .into()
    })
}

/// `hf.rate_limited` with the server's `Retry-After` (seconds) when it sent one.
pub fn rate_limited(headers: &reqwest::header::HeaderMap) -> String {
    let retry = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(60);
    err("hf.rate_limited").num("retry", retry).into()
}

// --- profile ---

/// The signed-in Hugging Face account.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HfUser {
    pub name: String,
    #[serde(default)]
    pub fullname: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

fn absolute_avatar(url: &str) -> Option<String> {
    let url = url.trim();
    if url.starts_with("https://") {
        Some(url.to_string())
    } else if url.starts_with('/') {
        Some(format!("https://huggingface.co{url}"))
    } else {
        None
    }
}

/// Parses a `whoami-v2` response.
pub fn parse_whoami(v: &serde_json::Value) -> Option<HfUser> {
    let name = v.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(HfUser {
        name: name.to_string(),
        fullname: v
            .get("fullname")
            .and_then(|f| f.as_str())
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(String::from),
        avatar_url: v
            .get("avatarUrl")
            .and_then(|a| a.as_str())
            .and_then(absolute_avatar),
    })
}

/// The account a token belongs to. `hf_auth.token_invalid` for a rejected token.
pub async fn whoami(token: &str) -> Result<HfUser, String> {
    let res = client()?
        .get(WHOAMI_URL)
        .bearer_auth(token.trim())
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, WHOAMI_URL))?;
    let status = res.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(err("hf_auth.token_invalid").into());
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(rate_limited(res.headers()));
    }
    if !status.is_success() {
        return Err(err("hf_auth.whoami_failed").with("error", status).into());
    }
    let v: serde_json::Value = res
        .json()
        .await
        .map_err(|e| crate::http_error::transport(&e, WHOAMI_URL))?;
    parse_whoami(&v).ok_or_else(|| err("hf_auth.whoami_failed").with("error", "name").into())
}

// --- session record ---

/// The OAuth session kept in the keychain (`huggingface_oauth`).
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthSession {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds; None = the server gave no lifetime.
    #[serde(default)]
    pub expires_at: Option<u64>,
    pub client_id: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub user: HfUser,
}

impl fmt::Debug for OAuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthSession")
            .field("expires_at", &self.expires_at)
            .field("client_id", &self.client_id)
            .field("scope", &self.scope)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl OAuthSession {
    fn from_tokens(t: TokenResponse, client_id: &str, user: HfUser, now: u64) -> Self {
        Self {
            access_token: t.access_token,
            refresh_token: t.refresh_token.filter(|r| !r.is_empty()),
            expires_at: t.expires_in.map(|s| now + s),
            client_id: client_id.to_string(),
            scope: t.scope,
            user,
        }
    }

    /// Applies a refresh response (the refresh token rotates when the server sends a new one).
    pub fn apply_refresh(&mut self, t: TokenResponse, now: u64) {
        self.access_token = t.access_token;
        if let Some(r) = t.refresh_token.filter(|r| !r.is_empty()) {
            self.refresh_token = Some(r);
        }
        self.expires_at = t.expires_in.map(|s| now + s);
        if t.scope.is_some() {
            self.scope = t.scope;
        }
    }

    pub fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at
            .is_some_and(|at| at <= now.saturating_add(REFRESH_MARGIN_SECS))
    }
}

fn read_session() -> Result<Option<OAuthSession>, String> {
    Ok(crate::secrets::hf_oauth_get()?.and_then(|raw| serde_json::from_str(&raw).ok()))
}

/// Saves the session and makes its access token the Hugging Face token.
fn save_session(session: &OAuthSession) -> Result<(), String> {
    let raw = serde_json::to_string(session)
        .map_err(|e| String::from(err("json.serialize_failed").with("error", e)))?;
    crate::secrets::hf_oauth_set(&raw)?;
    crate::secrets::hf_set(&session.access_token)?;
    AppSettings::cache_hf_token(Some(session.access_token.clone()));
    Ok(())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| String::from(err("task.join_failed").with("error", e)))?
}

static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The OAuth access token, refreshed first when it is about to expire. `Ok(None)` without an
/// OAuth session.
async fn oauth_token() -> Result<Option<String>, String> {
    let _guard = REFRESH_LOCK.lock().await;
    let Some(mut session) = blocking(read_session).await? else {
        return Ok(None);
    };
    if session.needs_refresh(now_secs()) {
        if let Some(refresh) = session.refresh_token.clone() {
            let tokens = refresh_tokens(&session.client_id, &refresh).await?;
            session.apply_refresh(tokens, now_secs());
            let saved = session.clone();
            blocking(move || save_session(&saved)).await?;
        }
    }
    let token = session.access_token;
    let cached = token.clone();
    let _ = blocking(move || {
        if AppSettings::load().hf_token.as_deref() != Some(cached.as_str()) {
            AppSettings::cache_hf_token(Some(cached));
        }
        Ok(())
    })
    .await;
    Ok(Some(token))
}

/// The Hugging Face token for a Hub request or download: the OAuth access token (refreshed
/// when needed) or the pasted token. A failed refresh keeps the old token (the request then
/// fails with a keyed 401/403 error instead of silently going anonymous).
pub async fn current_token() -> Option<String> {
    match oauth_token().await {
        Ok(Some(token)) => return Some(token),
        Ok(None) => {}
        Err(e) => eprintln!("[Harness Studio] {e}"),
    }
    // Read the keychain rather than the in-memory copy, so a `harness hf login/logout` in
    // another process is seen at once; a keychain that cannot be read falls back to the copy.
    tokio::task::spawn_blocking(|| match crate::secrets::hf_try_get() {
        Ok(token) => {
            if AppSettings::load().hf_token != token {
                AppSettings::cache_hf_token(token.clone());
            }
            token
        }
        Err(_) => AppSettings::load().hf_token,
    })
    .await
    .ok()
    .flatten()
}

// --- status ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HfAuthKind {
    None,
    Token,
    Oauth,
}

/// What the Settings page and `harness hf whoami` show. Never carries a token.
#[derive(Debug, Clone, Serialize)]
pub struct HfAuthStatus {
    pub kind: HfAuthKind,
    /// Cached profile of the OAuth account (a pasted token's account comes from [`whoami`]).
    pub user: Option<HfUser>,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
    /// A client id is configured, so "Sign in with Hugging Face" can run.
    pub oauth_available: bool,
    pub client_id_source: Option<ClientIdSource>,
    /// The client id itself (public information, shown in Settings).
    pub client_id: Option<String>,
    pub registered_redirect_uri: &'static str,
    pub scopes: &'static str,
    pub login_pending: bool,
}

/// Current sign-in state (reads the keychain; blocking). A keychain that cannot be read is an
/// error (`secrets.hf_read_failed`), not "signed out".
pub fn status() -> Result<HfAuthStatus, String> {
    let settings = AppSettings::load();
    let session = read_session()?;
    let token_set = crate::secrets::hf_try_get()?.is_some();
    let client = client_id(&settings);
    let kind = match (&session, token_set) {
        (Some(_), _) => HfAuthKind::Oauth,
        (None, true) => HfAuthKind::Token,
        (None, false) => HfAuthKind::None,
    };
    Ok(HfAuthStatus {
        kind,
        user: session.as_ref().map(|s| s.user.clone()),
        expires_at: session.as_ref().and_then(|s| s.expires_at),
        scope: session.as_ref().and_then(|s| s.scope.clone()),
        oauth_available: client.is_some(),
        client_id_source: client.as_ref().map(|(_, s)| *s),
        client_id: client.map(|(id, _)| id),
        registered_redirect_uri: REGISTERED_REDIRECT_URI,
        scopes: SCOPES,
        login_pending: login_pending(),
    })
}

/// Signs out: deletes the OAuth session and the token (blocking). The grant stays valid on
/// huggingface.co until revoked there ([`CONNECTED_APPS_URL`]).
pub fn logout() -> Result<(), String> {
    crate::secrets::hf_oauth_delete()?;
    AppSettings::clear_hf_token()
}

// --- login flows ---

static PENDING: Mutex<Option<tokio::sync::oneshot::Sender<()>>> = Mutex::new(None);

pub fn login_pending() -> bool {
    PENDING
        .lock()
        .map(|p| p.as_ref().is_some_and(|tx| !tx.is_closed()))
        .unwrap_or(false)
}

/// Cancels a running browser or device login. True when one was running.
pub fn cancel_login() -> bool {
    PENDING
        .lock()
        .ok()
        .and_then(|mut p| p.take())
        .map(|tx| tx.send(()).is_ok())
        .unwrap_or(false)
}

fn register_pending() -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Ok(mut p) = PENDING.lock() {
        // A new login replaces (cancels) an older one.
        if let Some(old) = p.replace(tx) {
            let _ = old.send(());
        }
    }
    rx
}

/// Stores a fresh OAuth login: profile first (a token that cannot even read the profile is
/// not kept), then the keychain entries.
async fn finish_login(client_id: &str, tokens: TokenResponse) -> Result<HfAuthStatus, String> {
    let user = whoami(&tokens.access_token).await?;
    let session = OAuthSession::from_tokens(tokens, client_id, user, now_secs());
    blocking(move || {
        save_session(&session)?;
        status()
    })
    .await
}

/// Browser login: binds `127.0.0.1:<random port>`, calls `open` with the authorization URL,
/// waits (at most five minutes, or until [`cancel_login`]) for the redirect, exchanges the
/// code and stores the session.
pub async fn login_with_browser(
    client_id: &str,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<HfAuthStatus, String> {
    if !valid_client_id(client_id) {
        return Err(err("hf_auth.no_client_id").into());
    }
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| String::from(err("hf_auth.bind_failed").with("error", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| String::from(err("hf_auth.bind_failed").with("error", e)))?
        .port();
    let pkce = Pkce::new()?;
    let state = new_state()?;
    let redirect = redirect_uri(port);
    let url = authorize_url(client_id, &redirect, &state, &pkce.challenge)?;
    let cancelled = register_pending();
    open(&url).map_err(|e| String::from(err("hf_auth.browser_failed").with("error", e)))?;

    let code = tokio::select! {
        r = tokio::time::timeout(LOGIN_TIMEOUT, wait_for_code(listener, &state, "Hugging Face")) => {
            match r {
                Ok(code) => code,
                Err(_) => Err(err("hf_auth.timeout").into()),
            }
        }
        _ = cancelled => Err(err("hf_auth.cancelled").into()),
    };
    let result = match code {
        Ok(code) => match exchange_code(client_id, &code, &pkce.verifier, &redirect).await {
            Ok(tokens) => finish_login(client_id, tokens).await,
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    clear_pending_if_done();
    result
}

fn clear_pending_if_done() {
    if let Ok(mut p) = PENDING.lock() {
        // Our receiver is gone once the login returned: drop the stale sender.
        if p.as_ref().is_some_and(|tx| tx.is_closed()) {
            *p = None;
        }
    }
}

/// A device authorization (RFC 8628): the user opens `verification_uri` and enters
/// `user_code`.
#[derive(Clone, Deserialize, Serialize)]
pub struct DeviceCode {
    #[serde(skip_serializing)]
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    #[serde(default = "default_device_expiry")]
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_device_expiry() -> u64 {
    600
}

fn default_interval() -> u64 {
    5
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

pub async fn device_start(client_id: &str) -> Result<DeviceCode, String> {
    if !valid_client_id(client_id) {
        return Err(err("hf_auth.no_client_id").into());
    }
    let res = client()?
        .post(DEVICE_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[("client_id", client_id), ("scope", SCOPES)])
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, DEVICE_URL))?;
    let status = res.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(rate_limited(res.headers()));
    }
    let body = res
        .bytes()
        .await
        .map_err(|e| crate::http_error::transport(&e, DEVICE_URL))?;
    if !status.is_success() {
        let text = serde_json::from_slice::<OAuthError>(&body)
            .map(|e| oauth_error_text(&e.error, e.error_description.as_deref()))
            .unwrap_or_else(|_| status.to_string());
        return Err(err("hf_auth.device_failed").with("error", text).into());
    }
    serde_json::from_slice(&body)
        .map_err(|e| String::from(err("hf_auth.device_failed").with("error", e)))
}

/// Polls the token endpoint until the user approves the device code (or it expires / is
/// declined / [`cancel_login`] is called), then stores the session.
pub async fn device_finish(client_id: &str, device: &DeviceCode) -> Result<HfAuthStatus, String> {
    let mut cancelled = register_pending();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(device.expires_in.max(30));
    let mut interval = device.interval.clamp(1, 60);
    let result = loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
            _ = &mut cancelled => break Err(err("hf_auth.cancelled").into()),
        }
        if tokio::time::Instant::now() >= deadline {
            break Err(err("hf_auth.device_expired").into());
        }
        let response = token_request(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", &device.device_code),
            ("client_id", client_id),
        ])
        .await;
        match response {
            Ok(Ok(tokens)) => break finish_login(client_id, tokens).await,
            Ok(Err((e, _))) if e == "authorization_pending" => {}
            Ok(Err((e, _))) if e == "slow_down" => interval = (interval + 5).min(60),
            Ok(Err((e, _))) if e == "expired_token" => {
                break Err(err("hf_auth.device_expired").into())
            }
            Ok(Err((e, d))) => {
                break Err(err("hf_auth.denied")
                    .with("error", oauth_error_text(&e, d.as_deref()))
                    .into())
            }
            // Rate limited: wait longer and keep polling.
            Err(e) if e.starts_with("hf.rate_limited") => interval = (interval * 2).min(60),
            Err(e) => break Err(e),
        }
    };
    drop(cancelled);
    clear_pending_if_done();
    result
}

/// Stores a pasted access token after checking it against `whoami-v2` (CLI `hf login
/// --with-token`; replaces an OAuth session).
pub async fn login_with_token(token: &str) -> Result<HfUser, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(err("secrets.token_empty").into());
    }
    let user = whoami(&token).await?;
    blocking(move || AppSettings::set_hf_token(&token)).await?;
    Ok(user)
}

/// Opens a URL in the default browser (CLI; the GUI uses its opener plugin).
pub fn open_in_browser(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(err("hf_auth.browser_failed").with("error", url).into());
    }
    spawn_browser(url)
}

/// Opens a URL in the default browser without looking at its scheme (the caller vetted it).
pub(crate) fn spawn_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let result = crate::process::hidden_command("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
    #[cfg(target_os = "macos")]
    let result = crate::process::hidden_command("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = crate::process::hidden_command("xdg-open").arg(url).spawn();
    reap(result)
}

fn reap(result: std::io::Result<std::process::Child>) -> Result<(), String> {
    let mut child = result.map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_appendix_b() {
        // RFC 7636 Appendix B: the verifier's octets, the verifier and its S256 challenge.
        let octets: [u8; 32] = [
            116, 24, 223, 180, 151, 153, 224, 37, 79, 250, 96, 125, 216, 173, 187, 186, 22, 212,
            37, 77, 105, 214, 191, 240, 91, 88, 5, 88, 83, 132, 141, 121,
        ];
        let verifier = base64url(&octets);
        assert_eq!(verifier, "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            pkce_challenge(&verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn base64url_rfc4648_vectors_without_padding() {
        // RFC 4648 §10 test vectors, padding removed (the alphabets agree on these).
        for (input, expected) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64url(input.as_bytes()), expected);
        }
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn fresh_pkce_and_state_are_random_and_well_formed() {
        let a = Pkce::new().unwrap();
        let b = Pkce::new().unwrap();
        assert_eq!(a.verifier.len(), 43);
        assert_ne!(a.verifier, b.verifier);
        assert_eq!(a.challenge, pkce_challenge(&a.verifier));
        assert!(a
            .verifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_eq!(format!("{a:?}"), "Pkce { .. }");
        let s1 = new_state().unwrap();
        assert_eq!(s1.len(), 32);
        assert_ne!(s1, new_state().unwrap());
    }

    #[test]
    fn state_comparison() {
        assert!(states_match("abc", "abc"));
        assert!(!states_match("abc", "abd"));
        assert!(!states_match("abc", "ab"));
        assert!(!states_match("", ""));
    }

    #[test]
    fn callback_parsing_validates_state() {
        let s = "st4te";
        assert_eq!(
            parse_callback("/callback?code=abc123&state=st4te", s),
            Callback::Code("abc123".into())
        );
        // Percent-encoded values are decoded.
        assert_eq!(
            parse_callback("/callback?state=st4te&code=a%2Fb%3D", s),
            Callback::Code("a/b=".into())
        );
        assert_eq!(
            parse_callback("/callback?code=abc&state=other", s),
            Callback::StateMismatch
        );
        assert_eq!(
            parse_callback("/callback?code=abc", s),
            Callback::StateMismatch
        );
        assert_eq!(
            parse_callback(
                "/callback?error=access_denied&error_description=User+denied&state=st4te",
                s
            ),
            Callback::Denied("access_denied".into(), Some("User denied".into()))
        );
        // An error with a wrong state is ignored like any forged request.
        assert_eq!(
            parse_callback("/callback?error=access_denied&state=x", s),
            Callback::StateMismatch
        );
        assert_eq!(
            parse_callback("/callback?state=st4te", s),
            Callback::Denied("invalid_request".into(), None)
        );
        assert_eq!(parse_callback("/favicon.ico", s), Callback::Other);
        assert_eq!(
            parse_callback("/callback/x?code=a&state=st4te", s),
            Callback::Other
        );
    }

    #[test]
    fn request_line_parsing() {
        assert_eq!(
            request_target("GET /callback?code=1&state=2 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            Some("/callback?code=1&state=2")
        );
        assert_eq!(request_target("POST /callback HTTP/1.1\r\n\r\n"), None);
        assert_eq!(request_target("GET http://evil/ HTTP/1.1\r\n\r\n"), None);
        assert_eq!(request_target("garbage"), None);
    }

    #[test]
    fn authorize_url_carries_pkce_and_state() {
        let url = authorize_url("my-client", &redirect_uri(49282), "st", "ch").unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some("huggingface.co"));
        assert_eq!(parsed.path(), "/oauth/authorize");
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "my-client");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:49282/callback");
        assert_eq!(q["scope"], SCOPES);
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["code_challenge_method"], "S256");
    }

    #[test]
    fn client_id_validation_and_precedence() {
        assert!(valid_client_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(valid_client_id(
            "https://example.org/.well-known/oauth-cimd"
        ));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("has space"));
        assert!(!valid_client_id(&"x".repeat(257)));
        let settings = AppSettings {
            hf_oauth_client_id: "  from-settings ".into(),
            ..AppSettings::default()
        };
        assert_eq!(
            client_id(&settings),
            Some(("from-settings".into(), ClientIdSource::Settings))
        );
    }

    #[test]
    fn session_refresh_timing_and_rotation() {
        let tokens = |access: &str, refresh: Option<&str>, expires: Option<u64>| TokenResponse {
            access_token: access.into(),
            expires_in: expires,
            refresh_token: refresh.map(String::from),
            scope: Some(SCOPES.into()),
        };
        let user = HfUser {
            name: "u".into(),
            ..Default::default()
        };
        let mut s =
            OAuthSession::from_tokens(tokens("a1", Some("r1"), Some(3600)), "c", user, 1000);
        assert_eq!(s.expires_at, Some(4600));
        assert!(!s.needs_refresh(1000));
        assert!(s.needs_refresh(4600 - REFRESH_MARGIN_SECS));
        // No new refresh token: the old one is kept.
        s.apply_refresh(tokens("a2", None, Some(3600)), 5000);
        assert_eq!(s.access_token, "a2");
        assert_eq!(s.refresh_token.as_deref(), Some("r1"));
        s.apply_refresh(tokens("a3", Some("r2"), None), 6000);
        assert_eq!(s.refresh_token.as_deref(), Some("r2"));
        assert!(!s.needs_refresh(u64::MAX - 1));
        // Debug output never shows tokens.
        let dbg = format!("{s:?} {:?}", tokens("secret-a", Some("secret-r"), None));
        assert!(!dbg.contains("secret") && !dbg.contains("a3") && !dbg.contains("r2"));
    }

    #[test]
    fn whoami_parsing() {
        let v = serde_json::json!({
            "type": "user", "name": "julien", "fullname": "Julien C",
            "avatarUrl": "/avatars/abc.svg"
        });
        let u = parse_whoami(&v).unwrap();
        assert_eq!(u.name, "julien");
        assert_eq!(u.fullname.as_deref(), Some("Julien C"));
        assert_eq!(
            u.avatar_url.as_deref(),
            Some("https://huggingface.co/avatars/abc.svg")
        );
        let v = serde_json::json!({"name": "x", "avatarUrl": "javascript:alert(1)"});
        assert_eq!(parse_whoami(&v).unwrap().avatar_url, None);
        assert!(parse_whoami(&serde_json::json!({"error": "x"})).is_none());
    }

    #[tokio::test]
    async fn loopback_listener_ignores_forged_requests_and_returns_the_code() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server =
            tokio::spawn(
                async move { wait_for_code(listener, "good-state", "Hugging Face").await },
            );
        async fn get(port: u16, target: &str) -> String {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            s.write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        }
        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/callback?code=evil&state=bad")
            .await
            .starts_with("HTTP/1.1 400"));
        assert!(get(port, "/callback?code=the-code&state=good-state")
            .await
            .starts_with("HTTP/1.1 200"));
        assert_eq!(server.await.unwrap().unwrap(), "the-code");
    }

    #[tokio::test]
    async fn loopback_listener_reports_a_denial() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server =
            tokio::spawn(async move { wait_for_code(listener, "s", "Hugging Face").await });
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        s.write_all(b"GET /callback?error=access_denied&state=s HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let e = server.await.unwrap().unwrap_err();
        assert!(e.starts_with("hf_auth.denied"), "{e}");
    }

    #[test]
    fn cancel_without_login_is_a_no_op() {
        assert!(!cancel_login());
    }

    /// Real network: the token endpoint rejects an unknown client/code with an OAuth error
    /// (no credentials involved).
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn token_endpoint_rejects_a_bogus_code() {
        let verifier = Pkce::new().unwrap().verifier;
        let e = exchange_code(
            "harness-test-no-such-client",
            "bogus",
            &verifier,
            &redirect_uri(1),
        )
        .await
        .unwrap_err();
        assert!(e.starts_with("hf_auth.token_failed"), "{e}");
    }

    /// Real network: whoami rejects a token that does not exist.
    #[tokio::test]
    #[ignore] // network: cargo test -- --ignored
    async fn whoami_rejects_an_invalid_token() {
        let e = whoami("hf_this_token_does_not_exist_000000000")
            .await
            .unwrap_err();
        assert_eq!(e, "hf_auth.token_invalid");
    }
}
