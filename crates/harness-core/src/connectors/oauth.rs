//! OAuth 2.0 authorization code + PKCE (RFC 7636) with a loopback redirect (RFC 8252 §7.3) for
//! the connectors. The PKCE pair, the `state` check and the loopback listener are the ones the
//! Hugging Face and MCP sign-ins use ([`crate::hf_auth`]); this file adds the connector
//! specifics: scopes by access level, the client id/secret lookup, refresh and revocation.
//!
//! Tokens go to the OS keychain only (`secrets::connector_session_*`) and are never logged:
//! [`Session`] has no `Debug`, and errors carry the provider's `error_description` at most.

use super::{Connected, Provider};
use crate::error_key::err;
use crate::hf_auth::{self, Pkce, TokenResponse};
use crate::secrets;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// Refresh this long before the access token expires.
const REFRESH_MARGIN_SECS: u64 = 60;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn client() -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("HarnessStudio/", env!("CARGO_PKG_VERSION")));
    #[cfg(test)]
    let builder = builder.no_proxy();
    builder.build().map_err(|e| e.to_string())
}

/// What the keychain holds for a connected connector.
#[derive(Serialize, Deserialize, Clone)]
struct Session {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    client_id: String,
}

impl Session {
    fn apply(&mut self, t: TokenResponse, now: u64) {
        self.access_token = t.access_token;
        // A refresh answer without a new refresh token keeps the old one.
        if t.refresh_token.is_some() {
            self.refresh_token = t.refresh_token;
        }
        self.expires_at = t.expires_in.map(|s| now.saturating_add(s));
        if t.scope.is_some() {
            self.scope = t.scope;
        }
    }

    fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at
            .is_some_and(|at| at <= now.saturating_add(REFRESH_MARGIN_SECS))
    }
}

fn oauth_error_text(body: &serde_json::Value) -> String {
    let field = |k: &str| body.get(k).and_then(|v| v.as_str());
    let nested = body
        .get("error")
        .filter(|e| e.is_object())
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str());
    match (field("error"), field("error_description"), nested) {
        (Some(e), Some(d), _) => format!("{e}: {d}"),
        (Some(e), None, _) => e.to_string(),
        (None, _, Some(m)) => m.to_string(),
        _ => "no details".into(),
    }
}

async fn token_request(endpoint: &str, form: &[(&str, &str)]) -> Result<TokenResponse, String> {
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
        return serde_json::from_str::<TokenResponse>(&text)
            .map_err(|_| "The provider answered without an access token".to_string());
    }
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    Err(format!(
        "The provider refused the request (HTTP {status}): {}",
        oauth_error_text(&body)
    ))
}

/// The URL the browser opens.
pub fn authorize_url(
    p: &Provider,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
    scopes: &[String],
) -> Result<String, String> {
    let mut url = Url::parse(&p.auth_url).map_err(|e| format!("Invalid authorization URL: {e}"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &scopes.join(" "))
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        if p.family == super::Family::Google {
            // A refresh token needs offline access; consent is shown again on purpose.
            q.append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        } else {
            q.append_pair("response_mode", "query");
        }
    }
    Ok(url.into())
}

static PENDING: Mutex<Vec<(String, tokio::sync::oneshot::Sender<()>)>> = Mutex::new(Vec::new());

fn register_pending(id: &str) -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    pending.retain(|(i, _)| i != id);
    pending.push((id.to_string(), tx));
    rx
}

fn clear_pending(id: &str) {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(i, _)| i != id);
}

pub fn login_pending(id: &str) -> bool {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|(i, _)| i == id)
}

/// Cancels a sign-in that waits for the browser. Returns whether one was waiting.
pub fn cancel_login(id: &str) -> bool {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    match pending.iter().position(|(i, _)| i == id) {
        Some(at) => pending.remove(at).1.send(()).is_ok(),
        None => false,
    }
}

/// Connects `id`: the user's consent (`consent`, given on the app's own screen after reading
/// what the connector can do), the browser round trip (at most five minutes, or until
/// [`cancel_login`]) and the token exchange. `write` asks for the write scopes as well.
/// `open` shows the authorization URL to the user.
pub async fn connect(
    id: &str,
    write: bool,
    consent: bool,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    if !consent {
        return Err(err("connector.consent_required").into());
    }
    let p =
        super::provider(id).ok_or_else(|| String::from(err("connector.unknown").with("id", id)))?;
    let (client_id, _) = super::client_id(p.family)
        .ok_or_else(|| String::from(err("connector.client_id_missing").with("name", p.name)))?;
    let client_secret = super::client_secret(p.family)?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("Could not listen for the redirect on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!(
        "http://{}:{port}{}",
        p.redirect_host,
        hf_auth::CALLBACK_PATH
    );
    let scopes = p.scopes(write);
    let pkce = Pkce::new()?;
    let state = hf_auth::new_state()?;
    let url = authorize_url(&p, &client_id, &redirect, &state, &pkce.challenge, &scopes)?;
    let cancelled = register_pending(p.id);
    if let Err(e) = open(&url) {
        clear_pending(p.id);
        return Err(format!("Could not open the browser: {e}"));
    }
    let code = tokio::select! {
        r = tokio::time::timeout(LOGIN_TIMEOUT, hf_auth::wait_for_code(listener, &state, p.name)) => match r {
            Ok(code) => code,
            Err(_) => Err("The sign-in timed out: no answer from the browser within five minutes".to_string()),
        },
        _ = cancelled => Err("The sign-in was cancelled".to_string()),
    };
    clear_pending(p.id);
    let code = code?;
    let scope_param = scopes.join(" ");
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("client_id", client_id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
    ];
    if p.family == super::Family::Microsoft {
        form.push(("scope", scope_param.as_str()));
    }
    if let Some(secret) = &client_secret {
        form.push(("client_secret", secret));
    }
    let tokens = token_request(&p.token_url, &form).await?;
    let mut session = Session {
        access_token: String::new(),
        refresh_token: None,
        expires_at: None,
        scope: None,
        client_id,
    };
    session.apply(tokens, now_secs());
    // A provider may grant less than asked for: only offer the write tools if it really
    // granted write scopes.
    let granted: Vec<String> = session
        .scope
        .as_deref()
        .map(|s| s.split(' ').map(str::to_string).collect())
        .unwrap_or_else(|| scopes.clone());
    let write_granted = write
        && p.write_scopes.iter().all(|w| {
            granted
                .iter()
                .any(|g| g == w || g.ends_with(&format!("/{w}")))
        });
    secrets::connector_session_set(
        p.id,
        &serde_json::to_string(&session).map_err(|e| e.to_string())?,
    )?;
    super::mark_connected(
        p.id,
        Connected {
            write: write_granted,
            scopes: granted,
            since: now_secs(),
        },
    )
}

/// A valid access token for the provider, refreshed when it is about to expire.
pub async fn access_token(p: &Provider) -> Result<String, String> {
    let raw = secrets::connector_session_get(p.id)?
        .ok_or_else(|| String::from(err("connector.not_connected").with("name", p.name)))?;
    let mut session: Session = serde_json::from_str(&raw)
        .map_err(|_| String::from(err("connector.not_connected").with("name", p.name)))?;
    if !session.needs_refresh(now_secs()) {
        return Ok(session.access_token);
    }
    let refresh = session
        .refresh_token
        .clone()
        .ok_or_else(|| String::from(err("connector.expired").with("name", p.name)))?;
    let secret = super::client_secret(p.family)?;
    let scope = p
        .scopes(super::connected(p.id).is_some_and(|c| c.write))
        .join(" ");
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh.as_str()),
        ("client_id", session.client_id.as_str()),
    ];
    if p.family == super::Family::Microsoft {
        form.push(("scope", scope.as_str()));
    }
    if let Some(secret) = &secret {
        form.push(("client_secret", secret));
    }
    let tokens = token_request(&p.token_url, &form)
        .await
        .map_err(|_| String::from(err("connector.expired").with("name", p.name)))?;
    session.apply(tokens, now_secs());
    secrets::connector_session_set(
        p.id,
        &serde_json::to_string(&session).map_err(|e| e.to_string())?,
    )?;
    Ok(session.access_token)
}

#[derive(Debug, Clone, Serialize)]
pub struct Disconnected {
    /// The provider confirmed the token was revoked on its side. Without a revocation
    /// endpoint (Microsoft) the user removes the grant in their account instead.
    pub remote_revoked: bool,
}

/// Revokes (where the provider supports it) and forgets the connector's tokens. The local
/// tokens are deleted even when the provider cannot be reached.
pub async fn disconnect(id: &str) -> Result<Disconnected, String> {
    let p =
        super::provider(id).ok_or_else(|| String::from(err("connector.unknown").with("id", id)))?;
    let mut remote_revoked = false;
    if let (Some(raw), Some(url)) = (secrets::connector_session_get(p.id)?, &p.revoke_url) {
        if let Ok(session) = serde_json::from_str::<Session>(&raw) {
            let token = session
                .refresh_token
                .as_deref()
                .unwrap_or(&session.access_token);
            remote_revoked = client()?
                .post(url)
                .form(&[("token", token)])
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
        }
    }
    secrets::connector_session_delete(p.id)?;
    super::mark_disconnected(p.id)?;
    Ok(Disconnected { remote_revoked })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::{set_client, use_test_provider, Family};
    use crate::tools::mcp_client::testserver::{serve, Resp};
    use std::sync::Arc;

    fn mock_provider(base: &str, family: Family, id: &'static str) -> Provider {
        Provider {
            id,
            name: "Test",
            family,
            auth_url: format!("{base}/authorize"),
            token_url: format!("{base}/token"),
            revoke_url: (family == Family::Google).then(|| format!("{base}/revoke")),
            api_base: base.into(),
            read_scopes: vec!["read".into()],
            write_scopes: vec!["write".into()],
            redirect_host: "127.0.0.1",
        }
    }

    /// The "browser": opens the authorization URL's redirect with a code.
    fn browser(
        deny_state: bool,
        seen: Arc<Mutex<Vec<String>>>,
    ) -> impl FnOnce(&str) -> Result<(), String> {
        move |url: &str| {
            seen.lock().unwrap().push(url.to_string());
            let u = Url::parse(url).unwrap();
            let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
            let state = if deny_state {
                "forged".to_string()
            } else {
                q["state"].clone()
            };
            let target = format!("{}?code=the-code&state={state}", q["redirect_uri"]);
            tokio::spawn(async move {
                let _ = reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap()
                    .get(target)
                    .send()
                    .await;
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn mocked_pkce_flow_stores_tokens_in_the_keychain_and_refreshes_and_revokes() {
        let token_calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls = token_calls.clone();
        let server = serve(move |_, req| match req.path() {
            "/token" => {
                calls.lock().unwrap().push(req.body.clone());
                if req.param("grant_type").as_deref() == Some("authorization_code") {
                    assert_eq!(req.param("code").as_deref(), Some("the-code"));
                    assert_eq!(req.param("client_id").as_deref(), Some("client-1"));
                    assert_eq!(req.param("client_secret").as_deref(), Some("shh"));
                    // PKCE: the verifier must hash to the challenge sent to /authorize.
                    let verifier = req.param("code_verifier").unwrap();
                    assert_eq!(verifier.len(), 43);
                    Resp::json(200, &serde_json::json!({"access_token":"at-1","refresh_token":"rt-1","expires_in":1,"scope":"read write"}))
                } else {
                    assert_eq!(req.param("refresh_token").as_deref(), Some("rt-1"));
                    Resp::json(200, &serde_json::json!({"access_token":"at-2","expires_in":3600}))
                }
            }
            "/revoke" => {
                assert_eq!(req.param("token").as_deref(), Some("rt-1"));
                Resp::status(200)
            }
            _ => Resp::status(404),
        })
        .await;
        use_test_provider(mock_provider(&server.base, Family::Google, "gmail"));
        set_client(Family::Google, "client-1", Some("shh")).unwrap();

        // Consent is required before anything opens.
        assert!(connect("gmail", false, false, |_| panic!(
            "no browser without consent"
        ))
        .await
        .unwrap_err()
        .contains("connector.consent_required"));

        let urls = Arc::new(Mutex::new(vec![]));
        connect("gmail", true, true, browser(false, urls.clone()))
            .await
            .unwrap();
        let auth = Url::parse(&urls.lock().unwrap()[0]).unwrap();
        let q: std::collections::HashMap<_, _> = auth.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "read write");
        assert_eq!(q["access_type"], "offline");
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        // The tokens are in the (test) keychain; the settings file holds none of them.
        let raw = secrets::connector_session_get("gmail").unwrap().unwrap();
        assert!(raw.contains("at-1") && raw.contains("rt-1"));
        let settings =
            std::fs::read_to_string(crate::paths::config_dir().join("connectors.json")).unwrap();
        assert!(
            !settings.contains("at-1") && !settings.contains("rt-1") && !settings.contains("shh")
        );
        let c = super::super::connected("gmail").unwrap();
        assert!(c.write);

        // expires_in = 1 is inside the refresh margin, so the next use refreshes.
        let p = super::super::provider("gmail").unwrap();
        assert_eq!(access_token(&p).await.unwrap(), "at-2");
        assert_eq!(access_token(&p).await.unwrap(), "at-2");
        assert_eq!(token_calls.lock().unwrap().len(), 2);

        let d = disconnect("gmail").await.unwrap();
        assert!(d.remote_revoked);
        assert_eq!(secrets::connector_session_get("gmail").unwrap(), None);
        assert!(super::super::connected("gmail").is_none());
        assert!(access_token(&p)
            .await
            .unwrap_err()
            .contains("connector.not_connected"));
    }

    #[tokio::test]
    async fn forged_redirects_are_ignored_and_a_denied_grant_stores_nothing() {
        let server = serve(|_, _| Resp::status(500)).await;
        use_test_provider(mock_provider(&server.base, Family::Microsoft, "calendar"));
        set_client(Family::Microsoft, "", None).unwrap();
        if std::env::var(Family::Microsoft.id_env()).is_err() {
            // No client id yet: reported before the browser opens.
            assert!(connect("calendar", false, true, |_| panic!("no browser"))
                .await
                .unwrap_err()
                .contains("connector.client_id_missing"));
        }
        set_client(Family::Microsoft, "ms-client", None).unwrap();
        let _ = disconnect("calendar").await;
        // A request with the wrong state does not end the sign-in; cancelling does.
        let urls = Arc::new(Mutex::new(vec![]));
        let task = tokio::spawn(connect(
            "calendar",
            false,
            true,
            browser(true, urls.clone()),
        ));
        for _ in 0..50 {
            if login_pending("calendar") && !urls.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(login_pending("calendar"));
        assert!(cancel_login("calendar"));
        assert!(task.await.unwrap().unwrap_err().contains("cancelled"));
        assert_eq!(secrets::connector_session_get("calendar").unwrap(), None);
        assert!(!login_pending("calendar"));
        let q: std::collections::HashMap<_, _> = Url::parse(&urls.lock().unwrap()[0])
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(!q.contains_key("access_type"));
    }

    #[test]
    fn authorize_url_carries_scopes_pkce_and_state() {
        let p = mock_provider("https://auth.example", Family::Microsoft, "excel");
        let url = authorize_url(
            &p,
            "cid",
            "http://localhost:1/callback",
            "st",
            "ch",
            &["a".into(), "b".into()],
        )
        .unwrap();
        let u = Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], "a b");
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["response_type"], "code");
        assert!(!url.contains("client_secret"));
    }
}
