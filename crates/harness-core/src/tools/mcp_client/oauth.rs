//! Authorization for remote MCP servers, per the MCP authorization spec (2025-06-18): OAuth 2.1
//! Authorization Code + PKCE through the system browser and a 127.0.0.1 loopback redirect.
//!
//! The pieces, in the order a first connection meets them:
//! 1. The server answers `401` with `WWW-Authenticate: Bearer resource_metadata="..."`
//!    ([`parse_www_authenticate`]).
//! 2. Protected resource metadata (RFC 9728) names the authorization server(s)
//!    ([`protected_resource_urls`], [`ProtectedResource`]); without it the server's own origin
//!    is tried, like the 2025-03-26 revision.
//! 3. Authorization server metadata (RFC 8414, then OpenID Connect discovery)
//!    ([`auth_server_urls`], [`AuthServer`]).
//! 4. Dynamic client registration (RFC 7591) when the server offers it.
//! 5. The authorization request carries PKCE `S256` and the `resource` parameter (RFC 8707);
//!    the loopback redirect and PKCE come from [`crate::hf_auth`].
//! 6. The token request repeats `resource`; the session (tokens, client, endpoints) is one
//!    keychain entry per configured server (`mcp-oauth:<name>`), nothing in JSON files.
//! 7. [`bearer`] hands the transports a valid access token and refreshes it before it expires
//!    or after the server rejected it.
//!
//! Not supported: servers that do not offer dynamic client registration (they need a
//! pre-registered client id), and token revocation on sign-out (the session is deleted locally).

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::hf_auth::{self, Pkce};

/// How long the browser sign-in waits for the redirect.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// Refresh this long before the access token expires.
const REFRESH_MARGIN_SECS: u64 = 60;
const CLIENT_NAME: &str = "Harness Studio";
const USER_AGENT: &str = concat!("HarnessStudio/", env!("CARGO_PKG_VERSION"));

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

// --- WWW-Authenticate ---

/// What a `401`'s `WWW-Authenticate` header says (RFC 9728 §5.1, RFC 6750 §3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

/// Reads the auth-params (`key="value"` or `key=value`) of a `WWW-Authenticate` header; the
/// scheme words and parameters it does not know are skipped.
pub fn parse_www_authenticate(header: &str) -> Challenge {
    let mut out = Challenge::default();
    let b = header.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && matches!(b[i], b' ' | b',' | b'\t') {
            i += 1;
        }
        let key_start = i;
        while i < b.len() && !matches!(b[i], b'=' | b' ' | b',' | b'\t') {
            i += 1;
        }
        let key = header[key_start..i].to_ascii_lowercase();
        if i >= b.len() || b[i] != b'=' {
            continue; // a scheme word ("Bearer") or a bare token
        }
        i += 1;
        let value = if i < b.len() && b[i] == b'"' {
            i += 1;
            let mut v = String::new();
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' && i + 1 < b.len() {
                    i += 1;
                }
                // Copy whole characters: only ASCII bytes were tested above.
                let ch_len = header[i..].chars().next().map_or(1, char::len_utf8);
                v.push_str(&header[i..i + ch_len]);
                i += ch_len;
            }
            i += 1; // closing quote
            v
        } else {
            let vs = i;
            while i < b.len() && !matches!(b[i], b',' | b' ' | b'\t') {
                i += 1;
            }
            header[vs..i].to_string()
        };
        let slot = match key.as_str() {
            "resource_metadata" => &mut out.resource_metadata,
            "scope" => &mut out.scope,
            "error" => &mut out.error,
            _ => continue,
        };
        if slot.is_none() && !value.is_empty() {
            *slot = Some(value);
        }
    }
    out
}

// --- discovery ---

/// Protected resource metadata (RFC 9728): the parts a client needs.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProtectedResource {
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// Authorization server metadata (RFC 8414 / OpenID Connect discovery): the parts a client needs.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AuthServer {
    #[serde(default)]
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

impl AuthServer {
    /// PKCE `S256` is required; a server that lists methods without it is refused. One that
    /// lists none is tried anyway (older servers omit the field).
    pub fn supports_s256(&self) -> bool {
        self.code_challenge_methods_supported.is_empty()
            || self
                .code_challenge_methods_supported
                .iter()
                .any(|m| m == "S256")
    }
}

/// Where the protected resource metadata may live: the URL from the challenge first, then the
/// well-known URI with the MCP endpoint's path, then the one at the origin (RFC 9728 §3).
pub fn protected_resource_urls(mcp_url: &str, hint: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = hint.map(str::to_string).into_iter().collect();
    if let Ok(u) = Url::parse(mcp_url) {
        let origin = u.origin().ascii_serialization();
        let path = u.path().trim_end_matches('/');
        if !path.is_empty() {
            out.push(format!(
                "{origin}/.well-known/oauth-protected-resource{path}"
            ));
        }
        out.push(format!("{origin}/.well-known/oauth-protected-resource"));
    }
    dedup(out)
}

/// Where an issuer's metadata may live, in the order the MCP spec gives (RFC 8414 §3.1 with the
/// issuer's path inserted, OpenID Connect with it inserted or appended).
pub fn auth_server_urls(issuer: &str) -> Vec<String> {
    let Ok(u) = Url::parse(issuer) else {
        return Vec::new();
    };
    let origin = u.origin().ascii_serialization();
    let path = u.path().trim_end_matches('/');
    if path.is_empty() {
        return vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ];
    }
    vec![
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
        format!("{origin}{path}/.well-known/openid-configuration"),
    ]
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in v {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// The canonical form of the MCP endpoint used as the `resource` parameter (RFC 8707): no
/// fragment, and no trailing slash when the path is just `/`.
pub fn canonical_resource(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut u) => {
            u.set_fragment(None);
            let s = u.to_string();
            if u.path() == "/" && u.query().is_none() {
                s.trim_end_matches('/').to_string()
            } else {
                s
            }
        }
        Err(_) => url.to_string(),
    }
}

/// What discovery found.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    /// The `resource` parameter value.
    pub resource: String,
    pub server: AuthServer,
    /// The scope to ask for, when the challenge or the resource metadata names one.
    pub scope: Option<String>,
}

async fn get_json(client: &reqwest::Client, url: &str) -> Option<Value> {
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

/// Probes the MCP endpoint, then reads the resource and authorization server metadata.
pub async fn discover(mcp_url: &str) -> Result<Discovery, String> {
    let http = client()?;
    let probe = http
        .post(mcp_url)
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "harness-studio", "version": env!("CARGO_PKG_VERSION")},
            },
        }))
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, mcp_url))?;
    let challenge = probe
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(parse_www_authenticate)
        .fold(Challenge::default(), |mut acc, c| {
            acc.resource_metadata = acc.resource_metadata.or(c.resource_metadata);
            acc.scope = acc.scope.or(c.scope);
            acc
        });

    let mut resource_meta: Option<ProtectedResource> = None;
    for url in protected_resource_urls(mcp_url, challenge.resource_metadata.as_deref()) {
        if let Some(v) = get_json(&http, &url).await {
            if let Ok(m) = serde_json::from_value::<ProtectedResource>(v) {
                resource_meta = Some(m);
                break;
            }
        }
    }
    let issuers: Vec<String> = match &resource_meta {
        Some(m) if !m.authorization_servers.is_empty() => m.authorization_servers.clone(),
        // No resource metadata (the 2025-03-26 way): the MCP server is its own issuer.
        _ => Url::parse(mcp_url)
            .map(|u| vec![u.origin().ascii_serialization()])
            .unwrap_or_default(),
    };
    for issuer in issuers {
        for url in auth_server_urls(&issuer) {
            let Some(v) = get_json(&http, &url).await else {
                continue;
            };
            let Ok(server) = serde_json::from_value::<AuthServer>(v) else {
                continue;
            };
            if !server.supports_s256() {
                return Err(format!(
                    "the authorization server {issuer} does not support PKCE S256"
                ));
            }
            let scope = challenge.scope.clone().or_else(|| {
                resource_meta
                    .as_ref()
                    .filter(|m| !m.scopes_supported.is_empty())
                    .map(|m| m.scopes_supported.join(" "))
            });
            return Ok(Discovery {
                resource: canonical_resource(mcp_url),
                server,
                scope,
            });
        }
    }
    Err(format!(
        "could not find the authorization server of {mcp_url}: no protected resource metadata \
         and no authorization server metadata"
    ))
}

// --- registration, authorization request, tokens ---

#[derive(Debug, Clone, PartialEq)]
pub struct Registered {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// Dynamic client registration (RFC 7591) of a public native client.
async fn register(endpoint: &str, redirect_uri: &str) -> Result<Registered, String> {
    let resp = client()?
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&json!({
            "client_name": CLIENT_NAME,
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, endpoint))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "client registration was refused (HTTP {status}): {}",
            oauth_error_text(&body)
        ));
    }
    let client_id = body
        .get("client_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("the registration answer has no client_id")?
        .to_string();
    Ok(Registered {
        client_id,
        client_secret: body
            .get("client_secret")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

fn oauth_error_text(body: &Value) -> String {
    let e = body.get("error").and_then(Value::as_str).unwrap_or("error");
    match body.get("error_description").and_then(Value::as_str) {
        Some(d) if !d.is_empty() => format!("{e}: {d}"),
        _ => e.to_string(),
    }
}

/// The URL the browser opens.
pub fn authorize_url(
    server: &AuthServer,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
    resource: &str,
    scope: Option<&str>,
) -> Result<String, String> {
    let mut url = Url::parse(&server.authorization_endpoint)
        .map_err(|e| format!("invalid authorization endpoint: {e}"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", resource);
        if let Some(s) = scope.filter(|s| !s.is_empty()) {
            q.append_pair("scope", s);
        }
    }
    Ok(url.into())
}

#[derive(Deserialize)]
struct TokenBody {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

async fn token_request(endpoint: &str, form: &[(&str, &str)]) -> Result<TokenBody, String> {
    let resp = client()?
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, endpoint))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        return serde_json::from_str::<TokenBody>(&text)
            .map_err(|_| "the token endpoint answered without an access token".to_string());
    }
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Err(format!(
        "the token endpoint refused the request (HTTP {status}): {}",
        oauth_error_text(&body)
    ))
}

// --- the stored session ---

/// One server's sign-in as it lives in the keychain.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    /// The MCP endpoint the token is for.
    pub server_url: String,
    pub resource: String,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds; `None` = the server did not say.
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("server_url", &self.server_url)
            .field("client_id", &self.client_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Session {
    fn expired(&self, now: u64) -> bool {
        self.expires_at
            .is_some_and(|e| now + REFRESH_MARGIN_SECS >= e)
    }

    fn apply(&mut self, t: TokenBody, now: u64) {
        self.access_token = t.access_token;
        if t.refresh_token.is_some() {
            self.refresh_token = t.refresh_token;
        }
        self.expires_at = t.expires_in.map(|s| now + s);
        if t.scope.is_some() {
            self.scope = t.scope;
        }
    }
}

fn load_session(server: &str) -> Option<Session> {
    let json = crate::secrets::mcp_oauth_get(server).ok()??;
    serde_json::from_str(&json).ok()
}

fn save_session(server: &str, s: &Session) -> Result<(), String> {
    let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
    crate::secrets::mcp_oauth_set(server, &json)
}

/// The server has a sign-in that was made for this endpoint.
pub fn is_authorized(server: &str, url: &str) -> bool {
    load_session(server)
        .is_some_and(|s| canonical_resource(&s.server_url) == canonical_resource(url))
}

/// Forgets the sign-in (the tokens are not revoked at the server).
pub fn logout(server: &str) {
    crate::secrets::mcp_oauth_delete(server);
}

static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The access token to send to `server` at `url`, refreshed first when it is about to expire or
/// when `rejected` (the token the server just refused) is still the stored one. `None`: no
/// sign-in, or it can no longer be renewed (the server will answer `401`).
pub async fn bearer(server: &str, url: &str, rejected: Option<&str>) -> Option<String> {
    let current = |s: &Session| {
        canonical_resource(&s.server_url) == canonical_resource(url) && !s.access_token.is_empty()
    };
    let s = load_session(server).filter(current)?;
    let stale = |s: &Session| rejected == Some(s.access_token.as_str()) || s.expired(now_secs());
    if !stale(&s) {
        return Some(s.access_token);
    }
    let _one_at_a_time = REFRESH.lock().await;
    // Another request may have renewed it while this one waited.
    let mut s = load_session(server).filter(current)?;
    if !stale(&s) {
        return Some(s.access_token);
    }
    let refresh = s.refresh_token.clone()?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh.as_str()),
        ("client_id", s.client_id.as_str()),
        ("resource", s.resource.as_str()),
    ];
    if let Some(secret) = &s.client_secret {
        form.push(("client_secret", secret));
    }
    let renewed = token_request(&s.token_endpoint, &form).await.ok()?;
    s.apply(renewed, now_secs());
    save_session(server, &s).ok()?;
    Some(s.access_token)
}

// --- the sign-in ---

static PENDING: Mutex<Option<HashMap<String, tokio::sync::oneshot::Sender<()>>>> = Mutex::new(None);

fn register_pending(server: &str) -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut g = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    // A new sign-in replaces (cancels) an older one of the same server.
    if let Some(old) = g
        .get_or_insert_with(HashMap::new)
        .insert(server.to_string(), tx)
    {
        let _ = old.send(());
    }
    rx
}

fn clear_pending(server: &str) {
    let mut g = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = g.as_mut() {
        if m.get(server).is_some_and(|tx| tx.is_closed()) {
            m.remove(server);
        }
    }
}

/// A sign-in of `server` is waiting for the browser.
pub fn login_pending(server: &str) -> bool {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(server))
        .is_some_and(|tx| !tx.is_closed())
}

/// Cancels a waiting sign-in. True when one was waiting.
pub fn cancel_login(server: &str) -> bool {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .and_then(|m| m.remove(server))
        .map(|tx| tx.send(()).is_ok())
        .unwrap_or(false)
}

/// Opens an authorization URL in the system browser: `https://`, or `http://` to a loopback
/// host (a development server).
pub fn open_in_browser(url: &str) -> Result<(), String> {
    let u = Url::parse(url).map_err(|e| format!("invalid authorization URL: {e}"))?;
    let loopback = matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if u.scheme() == "https" || (u.scheme() == "http" && loopback) {
        hf_auth::spawn_browser(url)
    } else {
        Err(format!(
            "refusing to open {}: the authorization URL must be https",
            u.origin().ascii_serialization()
        ))
    }
}

/// Signs `server` in: discovery, client registration, the browser round trip (waiting at most
/// five minutes, or until [`cancel_login`]) and the token exchange. `open` shows the
/// authorization URL to the user. The session replaces an older one.
pub async fn authorize(
    server: &str,
    mcp_url: &str,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    let found = discover(mcp_url).await?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not listen for the redirect on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = hf_auth::redirect_uri(port);
    let registered = match &found.server.registration_endpoint {
        Some(endpoint) => register(endpoint, &redirect).await?,
        None => {
            return Err(
                "the authorization server offers no dynamic client registration, and Harness \
                 has no client id for it"
                    .to_string(),
            )
        }
    };
    let pkce = Pkce::new()?;
    let state = hf_auth::new_state()?;
    let url = authorize_url(
        &found.server,
        &registered.client_id,
        &redirect,
        &state,
        &pkce.challenge,
        &found.resource,
        found.scope.as_deref(),
    )?;
    let cancelled = register_pending(server);
    let opened = open(&url);
    if let Err(e) = opened {
        clear_pending(server);
        return Err(format!("could not open the browser: {e}"));
    }
    let code = tokio::select! {
        r = tokio::time::timeout(
            LOGIN_TIMEOUT,
            hf_auth::wait_for_code(listener, &state, "MCP server"),
        ) => match r {
            Ok(code) => code,
            Err(_) => Err("the sign-in timed out: no answer from the browser within five minutes".to_string()),
        },
        _ = cancelled => Err("the sign-in was cancelled".to_string()),
    };
    clear_pending(server);
    let code = code?;

    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("client_id", registered.client_id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
        ("resource", found.resource.as_str()),
    ];
    if let Some(secret) = &registered.client_secret {
        form.push(("client_secret", secret));
    }
    let tokens = token_request(&found.server.token_endpoint, &form).await?;
    let mut session = Session {
        server_url: mcp_url.to_string(),
        resource: found.resource,
        client_id: registered.client_id,
        client_secret: registered.client_secret,
        token_endpoint: found.server.token_endpoint,
        access_token: String::new(),
        refresh_token: None,
        expires_at: None,
        scope: None,
    };
    session.apply(tokens, now_secs());
    save_session(server, &session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::mcp_client::testserver::{serve, Req, Resp};
    use std::sync::{Arc, Mutex};

    #[test]
    fn www_authenticate_parameters() {
        let c = parse_www_authenticate(
            r#"Bearer realm="mcp", resource_metadata="https://x.example/.well-known/oauth-protected-resource/mcp", scope="files:read files:write", error=invalid_token"#,
        );
        assert_eq!(
            c.resource_metadata.as_deref(),
            Some("https://x.example/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(c.scope.as_deref(), Some("files:read files:write"));
        assert_eq!(c.error.as_deref(), Some("invalid_token"));
        // No parameters, a different scheme, an escaped quote, non-ASCII text.
        assert_eq!(parse_www_authenticate("Bearer"), Challenge::default());
        assert_eq!(
            parse_www_authenticate(r#"Basic realm="x""#),
            Challenge::default()
        );
        let c =
            parse_www_authenticate(r#"Bearer scope="a \"b\" ü", resource_metadata=https://m/r"#);
        assert_eq!(c.scope.as_deref(), Some("a \"b\" ü"));
        assert_eq!(c.resource_metadata.as_deref(), Some("https://m/r"));
    }

    #[test]
    fn metadata_urls_follow_the_rfcs() {
        assert_eq!(
            protected_resource_urls(
                "https://mcp.example/api/mcp",
                Some("https://mcp.example/meta.json")
            ),
            [
                "https://mcp.example/meta.json",
                "https://mcp.example/.well-known/oauth-protected-resource/api/mcp",
                "https://mcp.example/.well-known/oauth-protected-resource",
            ]
        );
        assert_eq!(
            protected_resource_urls("https://mcp.example/", None),
            ["https://mcp.example/.well-known/oauth-protected-resource"]
        );
        assert_eq!(
            auth_server_urls("https://auth.example"),
            [
                "https://auth.example/.well-known/oauth-authorization-server",
                "https://auth.example/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            auth_server_urls("https://auth.example/tenant1"),
            [
                "https://auth.example/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example/.well-known/openid-configuration/tenant1",
                "https://auth.example/tenant1/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            canonical_resource("https://MCP.example/#frag"),
            "https://mcp.example"
        );
        assert_eq!(
            canonical_resource("https://mcp.example/mcp?x=1#f"),
            "https://mcp.example/mcp?x=1"
        );
    }

    #[test]
    fn metadata_documents_parse() {
        let pr: ProtectedResource = serde_json::from_value(json!({
            "resource": "https://mcp.example/mcp",
            "authorization_servers": ["https://auth.example"],
            "scopes_supported": ["a", "b"],
            "bearer_methods_supported": ["header"],
        }))
        .unwrap();
        assert_eq!(pr.authorization_servers, ["https://auth.example"]);
        let a: AuthServer = serde_json::from_value(json!({
            "issuer": "https://auth.example",
            "authorization_endpoint": "https://auth.example/authorize",
            "token_endpoint": "https://auth.example/token",
            "registration_endpoint": "https://auth.example/register",
            "code_challenge_methods_supported": ["S256"],
        }))
        .unwrap();
        assert!(a.supports_s256());
        let plain: AuthServer = serde_json::from_value(json!({
            "authorization_endpoint": "https://a/authorize",
            "token_endpoint": "https://a/token",
            "code_challenge_methods_supported": ["plain"],
        }))
        .unwrap();
        assert!(!plain.supports_s256());
        // Without the endpoints it is not authorization server metadata.
        assert!(serde_json::from_value::<AuthServer>(json!({"issuer": "x"})).is_err());
    }

    #[test]
    fn the_authorization_url_carries_pkce_and_resource() {
        let server = AuthServer {
            issuer: None,
            authorization_endpoint: "https://auth.example/authorize?tenant=t".into(),
            token_endpoint: "https://auth.example/token".into(),
            registration_endpoint: None,
            code_challenge_methods_supported: vec![],
            scopes_supported: vec![],
        };
        let url = authorize_url(
            &server,
            "cid",
            "http://127.0.0.1:5555/callback",
            "st",
            "chal",
            "https://mcp.example/mcp",
            Some("a b"),
        )
        .unwrap();
        let u = Url::parse(&url).unwrap();
        let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["tenant"], "t");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], "chal");
        assert_eq!(q["resource"], "https://mcp.example/mcp");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555/callback");
        assert_eq!(q["scope"], "a b");
    }

    /// What the fake authorization server remembers between its endpoints.
    #[derive(Default)]
    struct AuthState {
        challenge: Option<String>,
        redirect: Option<String>,
        resource_at_authorize: Option<String>,
        /// The access token the MCP endpoint accepts.
        valid_token: Option<String>,
        issued: u32,
        refreshes: u32,
        expires_in: u64,
    }

    /// An MCP server behind OAuth: protected resource metadata, authorization server metadata,
    /// dynamic registration, a token endpoint that checks PKCE, and a JSON-only MCP endpoint
    /// that answers `initialize` / `tools/list` for a valid bearer and `401` otherwise.
    async fn oauth_server(
        state: Arc<Mutex<AuthState>>,
    ) -> crate::tools::mcp_client::testserver::TestServer {
        serve(move |base, req: Req| {
            let mut st = state.lock().unwrap();
            match (req.method.as_str(), req.path()) {
                ("POST", "/mcp") => {
                    let ok = req
                        .header("authorization")
                        .and_then(|h| h.strip_prefix("Bearer "))
                        .is_some_and(|t| st.valid_token.as_deref() == Some(t));
                    if !ok {
                        return Resp::status(401).with_header(
                            "WWW-Authenticate",
                            &format!(
                                r#"Bearer resource_metadata="{base}/.well-known/oauth-protected-resource/mcp", scope="tools""#
                            ),
                        );
                    }
                    let v: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
                    let id = v.get("id").cloned().unwrap_or(Value::Null);
                    match v.get("method").and_then(Value::as_str) {
                        Some("initialize") => Resp::json(200, &json!({"jsonrpc":"2.0","id":id,"result":{
                            "protocolVersion":"2025-06-18","capabilities":{"tools":{}},
                            "serverInfo":{"name":"oauth-fixture","version":"1"}}})),
                        Some("tools/list") => Resp::json(200, &json!({"jsonrpc":"2.0","id":id,"result":{
                            "tools":[{"name":"whoami","description":"d","inputSchema":{"type":"object"}}]}})),
                        Some(_) if id.is_null() => Resp::status(202),
                        _ => Resp::json(200, &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"no"}})),
                    }
                }
                ("GET", "/.well-known/oauth-protected-resource/mcp") => Resp::json(200, &json!({
                    "resource": format!("{base}/mcp"),
                    "authorization_servers": [format!("{base}/auth")],
                })),
                ("GET", "/.well-known/oauth-authorization-server/auth") => Resp::json(200, &json!({
                    "issuer": format!("{base}/auth"),
                    "authorization_endpoint": format!("{base}/auth/authorize"),
                    "token_endpoint": format!("{base}/auth/token"),
                    "registration_endpoint": format!("{base}/auth/register"),
                    "code_challenge_methods_supported": ["S256"],
                })),
                ("POST", "/auth/register") => {
                    let v: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
                    st.redirect = v["redirect_uris"][0].as_str().map(str::to_string);
                    Resp::json(201, &json!({"client_id": "client-1"}))
                }
                ("POST", "/auth/token") => {
                    let grant = req.param("grant_type").unwrap_or_default();
                    if req.param("client_id").as_deref() != Some("client-1")
                        || req.param("resource").as_deref() != Some(format!("{base}/mcp").as_str())
                    {
                        return Resp::json(400, &json!({"error":"invalid_request","error_description":"client or resource"}));
                    }
                    if grant == "authorization_code" {
                        let verifier = req.param("code_verifier").unwrap_or_default();
                        let pkce_ok = st.challenge.as_deref()
                            == Some(crate::hf_auth::pkce_challenge(&verifier).as_str());
                        if req.param("code").as_deref() != Some("the-code")
                            || !pkce_ok
                            || req.param("redirect_uri") != st.redirect
                        {
                            return Resp::json(400, &json!({"error":"invalid_grant","error_description":"code, pkce or redirect"}));
                        }
                    } else if grant == "refresh_token" {
                        if req.param("refresh_token").as_deref() != Some("refresh-1") {
                            return Resp::json(400, &json!({"error":"invalid_grant"}));
                        }
                        st.refreshes += 1;
                    } else {
                        return Resp::json(400, &json!({"error":"unsupported_grant_type"}));
                    }
                    st.issued += 1;
                    let token = format!("access-{}", st.issued);
                    st.valid_token = Some(token.clone());
                    Resp::json(200, &json!({
                        "access_token": token, "token_type": "Bearer",
                        "refresh_token": "refresh-1", "expires_in": st.expires_in,
                    }))
                }
                _ => Resp::status(404),
            }
        })
        .await
    }

    /// The `open` of a test: reads the authorization URL like the authorization server would,
    /// then sends the browser to the loopback redirect with a code.
    fn browser(state: Arc<Mutex<AuthState>>) -> impl FnOnce(&str) -> Result<(), String> {
        move |url: &str| {
            let u = Url::parse(url).unwrap();
            let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
            assert_eq!(q["code_challenge_method"], "S256");
            assert_eq!(q["client_id"], "client-1");
            {
                let mut st = state.lock().unwrap();
                st.challenge = Some(q["code_challenge"].clone());
                st.resource_at_authorize = q.get("resource").cloned();
                assert_eq!(Some(q["redirect_uri"].clone()), st.redirect);
            }
            let redirect = format!("{}?code=the-code&state={}", q["redirect_uri"], q["state"]);
            tokio::spawn(async move {
                let _ = reqwest::get(redirect).await;
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn sign_in_end_to_end_against_a_local_server() {
        let state = Arc::new(Mutex::new(AuthState {
            expires_in: 3600,
            ..Default::default()
        }));
        let server = oauth_server(state.clone()).await;
        let url = format!("{}/mcp", server.base);
        let name = "oauth-e2e";

        // Without a sign-in the server refuses, and the client says authorization is needed.
        assert!(!is_authorized(name, &url));
        let refused = crate::tools::mcp_client::McpConnection::connect(
            name,
            crate::tools::mcp_client::McpTarget::for_server(
                name,
                &crate::mcp::McpServerDef {
                    url: Some(url.clone()),
                    transport: Some("http".into()),
                    ..Default::default()
                },
            )
            .unwrap(),
            crate::tools::mcp_client::ConnectOptions {
                legacy_only: true,
                ..Default::default()
            },
        )
        .await;
        match refused {
            Err(crate::tools::mcp_client::McpError::Http {
                status: 401,
                message,
                ..
            }) => {
                assert!(message.contains("authoriz"), "{message}")
            }
            other => panic!("expected a 401, got {:?}", other.err()),
        }

        authorize(name, &url, browser(state.clone())).await.unwrap();
        assert!(is_authorized(name, &url));
        assert!(!login_pending(name));
        {
            let st = state.lock().unwrap();
            assert_eq!(
                st.resource_at_authorize.as_deref(),
                Some(url.as_str()),
                "RFC 8707 resource"
            );
            assert_eq!(st.issued, 1);
        }
        // The discovery hit the metadata the challenge pointed to.
        assert_eq!(
            server.count("GET", "/.well-known/oauth-protected-resource/mcp"),
            1
        );

        // The stored token opens the connection; the session is not in any JSON.
        let target = crate::tools::mcp_client::McpTarget::for_server(
            name,
            &crate::mcp::McpServerDef {
                url: Some(url.clone()),
                transport: Some("http".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let conn = crate::tools::mcp_client::McpConnection::connect(
            name,
            target,
            crate::tools::mcp_client::ConnectOptions {
                legacy_only: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(conn.tools().len(), 1);
        drop(conn);

        logout(name);
        assert!(!is_authorized(name, &url));
        assert!(bearer(name, &url, None).await.is_none());
    }

    #[tokio::test]
    async fn an_expired_or_rejected_token_is_refreshed_once() {
        let state = Arc::new(Mutex::new(AuthState {
            expires_in: 3600,
            ..Default::default()
        }));
        let server = oauth_server(state.clone()).await;
        let url = format!("{}/mcp", server.base);
        let name = "oauth-refresh";
        authorize(name, &url, browser(state.clone())).await.unwrap();
        let first = bearer(name, &url, None).await.unwrap();
        assert_eq!(first, "access-1");
        assert_eq!(state.lock().unwrap().refreshes, 0);

        // The server rejected the token: one refresh, the new token is stored.
        let second = bearer(name, &url, Some(&first)).await.unwrap();
        assert_eq!(second, "access-2");
        assert_eq!(state.lock().unwrap().refreshes, 1);
        // A late second report of the old token does not refresh again.
        assert_eq!(bearer(name, &url, Some(&first)).await.unwrap(), "access-2");
        assert_eq!(state.lock().unwrap().refreshes, 1);

        // Past its lifetime: refreshed without anybody complaining.
        let mut s = load_session(name).unwrap();
        s.expires_at = Some(now_secs() - 10);
        save_session(name, &s).unwrap();
        assert_eq!(bearer(name, &url, None).await.unwrap(), "access-3");
        assert_eq!(state.lock().unwrap().refreshes, 2);

        // A token meant for another endpoint is never sent here.
        assert!(bearer(name, "https://other.example/mcp", None)
            .await
            .is_none());
        logout(name);
    }

    #[tokio::test]
    async fn a_wrong_state_or_a_server_without_registration_fails_cleanly() {
        let state = Arc::new(Mutex::new(AuthState {
            expires_in: 3600,
            ..Default::default()
        }));
        let server = oauth_server(state.clone()).await;
        let url = format!("{}/mcp", server.base);
        // The browser step is cancelled from outside.
        let name = "oauth-cancel";
        let task = {
            let url = url.clone();
            tokio::spawn(async move { authorize(name, &url, |_| Ok(())).await })
        };
        for _ in 0..200 {
            if login_pending(name) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(cancel_login(name));
        let err = task.await.unwrap().unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
        assert!(!is_authorized(name, &url));

        // No metadata anywhere.
        let bare = serve(|_, _| Resp::status(404)).await;
        let err = discover(&format!("{}/mcp", bare.base)).await.unwrap_err();
        assert!(
            err.contains("could not find the authorization server"),
            "{err}"
        );
    }
}
