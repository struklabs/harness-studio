//! Outbound relay transport. Account sign-in/ticket issuance stays behind `TicketProvider`.
use super::{
    noise::{Channel, Identity},
    *,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::{SinkExt, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub const RELAY_URL: &str = "wss://relay.struklabs.com:7843/v1/app";
const ERR_SUPERSEDED: &str = "pairing.remote_superseded";
const TICKET_GRACE_SECS: u64 = 15;

pub const SUBPROTOCOL: &str = "struklabs.app.v1";
pub type TicketFuture<'a> = Pin<Box<dyn Future<Output = Result<RelayTicket, String>> + Send + 'a>>;

/// Issued after StrukLabs account sign-in. Never accept a ticket for another static key.
pub struct RelayTicket {
    pub credential: String,
    pub desktop_id: String,
    pub public_key: [u8; 32],
    pub expires_at: u64,
}
/// Desktop tickets last at most 300 s (APP_PROTOCOL 7.5) and tolerate 5 s of clock skew.
pub const MAX_DESKTOP_TICKET_SECS: u64 = 300;
const TICKET_SKEW_SECS: u64 = 5;

fn jti_cache() -> &'static Mutex<HashMap<String, u64>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, u64>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn jwt_part(part: &str) -> Result<Value, String> {
    let bytes = URL_SAFE_NO_PAD.decode(part).map_err(|_| ERR_PROTOCOL)?;
    serde_json::from_slice(&bytes).map_err(|_| ERR_PROTOCOL.into())
}

/// Request body of `POST /account/api/relay/desktop-ticket` (struklabs-web platform). The call is
/// made with the desktop's account access token by a [`TicketProvider`]; it is never a Harness token.
pub fn desktop_ticket_request(desktop_id: &str, key: &[u8; 32]) -> Value {
    json!({"device_id":desktop_id,"noise_static_pub":encode(key)})
}

impl RelayTicket {
    /// Builds a ticket from the platform's answer `{ticket, expires_at, relay_url}`. The claims
    /// are still pinned by `validate` before the ticket is ever sent to a relay. The desktop never
    /// calls `POST /account/api/relay/redeem`: that endpoint belongs to the relay (operator
    /// credential), which claims every `jti` there before accepting `auth`.
    pub fn from_response(desktop_id: &str, key: &[u8; 32], answer: &Value) -> Result<Self, String> {
        let expires_at = chrono::DateTime::parse_from_rfc3339(
            answer["expires_at"].as_str().ok_or(ERR_PROTOCOL)?,
        )
        .map_err(|_| ERR_PROTOCOL)?
        .timestamp();
        // The relay URL is fixed (`RELAY_URL`); a platform answer cannot redirect the desktop.
        if answer["relay_url"].as_str() != Some(RELAY_URL) || expires_at <= 0 {
            return Err(ERR_PROTOCOL.into());
        }
        Ok(Self {
            credential: answer["ticket"].as_str().ok_or(ERR_PROTOCOL)?.to_string(),
            desktop_id: desktop_id.to_string(),
            public_key: *key,
            expires_at: expires_at as u64,
        })
    }
    /// Checks the claims of the desktop ticket before it is ever sent to a relay. The relay (and
    /// the platform JWKS) own signature verification; the desktop pins what it can check offline:
    /// algorithm, lifetime, audience, role, device, key binding and single use.
    fn validate(&self, key: &[u8; 32]) -> Result<(), String> {
        let now = control_api::now_secs();
        let parts: Vec<&str> = self.credential.split('.').collect();
        if parts.len() != 3
            || parts[2].is_empty()
            || self.credential.len() > 4096
            || self.public_key != *key
            || !(8..=64).contains(&self.desktop_id.len())
            || !self
                .desktop_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ERR_PROTOCOL.into());
        }
        let header = jwt_part(parts[0])?;
        // Algorithm pinning: EdDSA only. `none`, HS* and anything else are rejected.
        if header["alg"] != "EdDSA" || !header["kid"].is_string() {
            return Err(ERR_PROTOCOL.into());
        }
        let claims = jwt_part(parts[1])?;
        let (Some(iat), Some(exp)) = (claims["iat"].as_u64(), claims["exp"].as_u64()) else {
            return Err(ERR_PROTOCOL.into());
        };
        let jti = claims["jti"].as_str().unwrap_or("");
        let limits = &claims["limits"];
        if !claims["iss"].as_str().is_some_and(|i| i.starts_with("https://"))
            || !claims["sub"].as_str().is_some_and(|s| (8..=128).contains(&s.len()))
            // The signed per-ticket limits of a desktop ticket (APP_PROTOCOL 7.2).
            || limits["sessions"] != 8
            || limits["bytes_per_minute"] != 8_388_608
            || limits["idle_timeout_ms"] != 60_000
        {
            return Err(ERR_PROTOCOL.into());
        }
        if claims["aud"] != "struklabs-relay"
            || claims["role"] != "desktop"
            || claims["device"] != self.desktop_id.as_str()
            || claims["noise_static_pub"] != encode(key).as_str()
            || !claims["mode"].is_null()
            || !(16..=64).contains(&jti.len())
            || exp <= iat
            || exp - iat > MAX_DESKTOP_TICKET_SECS
            || exp != self.expires_at
            || now + TICKET_SKEW_SECS < iat
            || now >= exp
        {
            return Err(ERR_PROTOCOL.into());
        }
        // Single use per process; a provider that replays a ticket is rejected before dialing.
        let mut seen = jti_cache().lock().unwrap();
        seen.retain(|_, until| *until > now);
        if seen.len() >= 1024 || seen.insert(jti.to_string(), exp).is_some() {
            return Err(ERR_PROTOCOL.into());
        }
        Ok(())
    }
}

/// Builds an unsigned, JWT-shaped credential for tests and mocks.
#[cfg(test)]
pub(super) fn test_credential(
    key: &[u8; 32],
    desktop_id: &str,
    alg: &str,
    jti: &str,
    iat: u64,
    exp: u64,
) -> String {
    let h = encode(
        json!({"alg":alg,"kid":"test-kid","typ":"JWT"})
            .to_string()
            .as_bytes(),
    );
    let c = encode(
        json!({"iss":"https://example.test","aud":"struklabs-relay","sub":"pairwise-handle",
        "device":desktop_id,"role":"desktop","noise_static_pub":encode(key),
        "limits":{"sessions":8,"bytes_per_minute":8388608,"idle_timeout_ms":60000},
        "jti":jti,"iat":iat,"exp":exp})
        .to_string()
        .as_bytes(),
    );
    format!("{h}.{c}.c2lnbmF0dXJl")
}

pub trait TicketProvider: Send + Sync {
    fn ticket(&self, desktop_key: [u8; 32]) -> TicketFuture<'_>;
}

/// The account endpoint is not deployed. No credentials are invented or sent to a stub URL.
pub struct UnavailableTickets;
impl TicketProvider for UnavailableTickets {
    fn ticket(&self, _: [u8; 32]) -> TicketFuture<'_> {
        Box::pin(async { Err("pairing.remote_signin_unavailable".into()) })
    }
}

pub struct DesktopSession {
    channel: Channel,
    mode: String,
    hub: Arc<Hub>,
    tokens: TokenStore,
    desktop_key: [u8; 32],
    desktop_id: String,
    desktop_name: String,
    inbound: u64,
    outbound: u64,
    authenticated: Option<(String, String)>,
    /// Ownership generation of the device (one live session per device).
    generation: u64,
    pub closed: bool,
    apps: Arc<apps::Applications>,
    cursors: HashMap<String, i64>,
    pending_requests: Vec<Value>,
}
impl DesktopSession {
    pub fn new(
        identity: &Identity,
        mode: &str,
        session: &str,
        desktop_id: &str,
        desktop_name: &str,
        tokens: TokenStore,
    ) -> Result<Self, String> {
        Ok(Self {
            channel: identity.responder(mode, session)?,
            mode: mode.into(),
            hub: hub_for(&tokens),
            tokens,
            desktop_key: identity.public,
            desktop_id: desktop_id.into(),
            desktop_name: desktop_name.into(),
            inbound: 0,
            outbound: 0,
            authenticated: None,
            generation: 0,
            closed: false,
            apps: apps::Applications::shared(),
            cursors: HashMap::new(),
            pending_requests: Vec::new(),
        })
    }

    /// A newer session authenticated the same device: this one ends with `session.superseded`.
    fn superseded(&self) -> bool {
        self.authenticated
            .as_ref()
            .is_some_and(|(device, _)| !self.apps.is_current(device, self.generation))
    }

    pub fn receive(&mut self, frame: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        if self.superseded() {
            return Ok(self.reject("session.superseded"));
        }
        let result = self.receive_inner(frame);
        if let Err(error) = result {
            let code = if error == ERR_REVOKED {
                if self.authenticated.is_some() {
                    "auth.revoked"
                } else {
                    "auth.invalid_token"
                }
            } else if matches!(
                error.as_str(),
                "protocol.out_of_sequence" | "protocol.version_unsupported"
            ) {
                error.as_str()
            } else {
                "protocol.malformed"
            };
            let frames = self.reject(code);
            return if frames.is_empty() {
                Err(error)
            } else {
                Ok(frames)
            };
        }
        result
    }

    fn reject(&mut self, code: &str) -> Vec<Vec<u8>> {
        let frames=self.encode_reply(json!({"type":"error","code":code,"message":"The secure session was closed.","retryable":false})).unwrap_or_default();
        self.close();
        frames
    }
    fn receive_inner(&mut self, frame: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        if self.closed {
            return Err(ERR_REVOKED.into());
        }
        // Owner revocation applies before every frame, including fragmented envelopes.
        self.check_authorized()?;
        let received = self.channel.receive(frame)?;
        let mut frames = received.reply.into_iter().collect::<Vec<_>>();
        if let Some(bytes) = received.envelope {
            let v = json_strict::parse(&bytes, "protocol.malformed")?;
            if v["v"] != 1 {
                return Err("protocol.version_unsupported".into());
            }
            if v["seq"].as_u64() != Some(self.inbound) {
                return Err("protocol.out_of_sequence".into());
            }
            self.inbound = self.inbound.checked_add(1).ok_or(ERR_PROTOCOL)?;
            let peer = self.channel.peer()?;
            let reply = match field(&v, "type")? {
                "pair.request" if self.mode == "pair" => {
                    let (status, body) = self.hub.remote_pair(
                        field(&v, "op")?,
                        &v["body"],
                        peer,
                        self.desktop_key,
                        &self.channel.handshake_hash()?,
                        &self.desktop_id,
                        &self.desktop_name,
                        &self.tokens,
                    );
                    json!({"type":"pair.response","id":v["id"],"status":status,"body":body})
                }
                "hello"
                    if self.mode == "app" && self.authenticated.is_none() && self.inbound == 1 =>
                {
                    let token = field(&v, "token")?;
                    let (device, info) = self.hub.remote_authorize(token, &peer, &self.tokens)?;
                    let mut resumed = Vec::new();
                    if let Some(items) = v["resume"].as_array() {
                        if items.len() > 4 {
                            return Err(ERR_PROTOCOL.into());
                        }
                        for item in items {
                            let id = field(item, "chat_id")?;
                            let last = item["last_index"]
                                .as_i64()
                                .filter(|n| *n >= -1)
                                .ok_or(ERR_PROTOCOL)?;
                            if self.apps.replay(&device, id, last).is_ok() {
                                self.cursors.insert(id.into(), last);
                                resumed.push(json!({"chat_id":id,"from_index":last+1}));
                            } else {
                                self.pending_requests.push(json!({"type":"error","id":id,"code":"chat.resume_unavailable","message":"The replay buffer is no longer available.","retryable":false}));
                            }
                        }
                    }
                    self.generation = self.apps.claim_session(&device);
                    self.apps.attach(&device);
                    self.authenticated = Some((device, token.into()));
                    json!({"type":"hello","from":"desktop","accepted_v":1,"desktop":{"name":self.desktop_name,"version":env!("CARGO_PKG_VERSION")},
                        "caps":["chat","resume","status","device.revoke"],"resumed":resumed,"limits":{"max_envelope_bytes":1048576,"max_concurrent_chats":info.policy.limits.max_concurrent.unwrap_or(1).min(4),"idle_timeout_ms":60000}})
                }
                "ping" if self.mode == "pair" || self.authenticated.is_some() => {
                    json!({"type":"pong","ts":v["ts"]})
                }
                "device.revoke" if self.authenticated.is_some() => {
                    let (device, _) = self.authenticated.as_ref().unwrap();
                    self.hub.revoke(device, &self.tokens)?;
                    self.apps.revoke(device);
                    self.closed = true;
                    json!({"type":"device.revoked","id":v["id"]})
                }
                "status" | "models.list" if self.authenticated.is_some() => {
                    self.pending_requests.push(v.clone());
                    Value::Null
                }
                "chat.start" if self.authenticated.is_some() => {
                    let (device, token) = self.authenticated.as_ref().unwrap();
                    let (_, info) = self.hub.remote_authorize(token, &peer, &self.tokens)?;
                    match self.apps.start(device, info, v.clone()) {
                        Ok(()) => {
                            self.cursors.insert(field(&v, "id")?.into(), -1);
                            Value::Null
                        }
                        Err(error) => error,
                    }
                }
                "chat.ack" if self.authenticated.is_some() => {
                    self.apps.ack(
                        &self.authenticated.as_ref().unwrap().0,
                        field(&v, "id")?,
                        v["index"].as_u64().ok_or(ERR_PROTOCOL)?,
                    )?;
                    Value::Null
                }
                "chat.cancel" if self.authenticated.is_some() => {
                    self.apps
                        .cancel(&self.authenticated.as_ref().unwrap().0, field(&v, "id")?);
                    Value::Null
                }
                "pong" if self.authenticated.is_some() => Value::Null,
                _ if self.authenticated.is_some() || self.mode == "pair" => Value::Null,
                _ => return Err(ERR_PROTOCOL.into()),
            };
            if self.pending_requests.len() > 32 {
                return Err(ERR_PROTOCOL.into());
            }
            if reply.is_null() {
                return Ok(frames);
            }
            frames.extend(self.encode_reply(reply)?);
        }
        Ok(frames)
    }
    pub fn check_authorized(&self) -> Result<(), String> {
        if self.channel.handshake_expired() {
            return Err(ERR_PROTOCOL.into());
        }
        if let Some((_, token)) = &self.authenticated {
            self.hub
                .remote_authorize(token, &self.channel.peer()?, &self.tokens)?;
        }
        Ok(())
    }
    pub fn close(&mut self) {
        if let Some((device, _)) = &self.authenticated {
            // Only the session that still owns the device detaches its chats; a superseded
            // session must not mark the new session's chats as orphaned.
            if self.apps.release(device, self.generation) {
                self.apps.detach(device);
            }
        }
        self.closed = true;
        self.channel.close();
        self.authenticated = None;
    }

    fn encode_reply(&mut self, mut v: Value) -> Result<Vec<Vec<u8>>, String> {
        v["v"] = json!(1);
        v["seq"] = json!(self.outbound);
        let next = self.outbound.checked_add(1).ok_or(ERR_PROTOCOL)?;
        let bytes = serde_json::to_vec(&v).map_err(|_| ERR_PROTOCOL)?;
        if bytes.len() > noise::MAX_ENVELOPE {
            return Err("protocol.too_large".into());
        }
        let frames = self.channel.send(&bytes)?;
        self.outbound = next;
        Ok(frames)
    }

    pub async fn poll(&mut self) -> Result<Vec<Vec<u8>>, String> {
        if self.superseded() {
            return Ok(self.reject("session.superseded"));
        }
        self.check_authorized()?;
        let Some((device, token)) = self.authenticated.clone() else {
            return Ok(vec![]);
        };
        let (_, info) = self
            .hub
            .remote_authorize(&token, &self.channel.peer()?, &self.tokens)?;
        let mut out = Vec::new();
        for request in std::mem::take(&mut self.pending_requests) {
            let reply = match request["type"].as_str() {
                Some("models.list") => {
                    if !info.policy.allows_endpoint(GatewayEndpoint::Models) {
                        json!({"type":"error","id":request["id"],"code":"scope.denied","message":"Model listing is not allowed.","retryable":false})
                    } else if let Some(scheduler) = crate::gateway::scheduler() {
                        let defs = scheduler.servable().await.map_err(|_| ERR_PROTOCOL)?;
                        let slots = scheduler.snapshot().await;
                        let models:Vec<Value>=defs.into_iter().filter(|d|info.policy.allows_model(&d.alias)).take(256).map(|d|json!({"id":d.alias,"name":d.alias,"context_length":4096,"loaded":slots.iter().any(|s|s.alias==d.alias&&s.port.is_some()),"capabilities":["chat"]})).collect();
                        json!({"type":"models.list","id":request["id"],"models":models})
                    } else {
                        json!({"type":"models.list","id":request["id"],"models":[]})
                    }
                }
                Some("status") => {
                    let slots = if let Some(scheduler) = crate::gateway::scheduler() {
                        scheduler.snapshot().await
                    } else {
                        vec![]
                    };
                    let models: Vec<String> = slots
                        .iter()
                        .filter(|s| s.port.is_some() && info.policy.allows_model(&s.alias))
                        .map(|s| s.alias.clone())
                        .take(64)
                        .collect();
                    let busy = slots
                        .iter()
                        .any(|s| s.in_flight > 0 && info.policy.allows_model(&s.alias));
                    json!({"type":"status","state":if busy{"busy"}else{"idle"},"models_loaded":models,"jobs":[]})
                }
                _ => request,
            };
            out.extend(self.encode_reply(reply)?);
        }
        for (id, last) in self.cursors.clone() {
            match self.apps.replay(&device, &id, last) {
                Ok(events) => {
                    for event in events.into_iter().take(32) {
                        if event["type"] == "chat.done" {
                            self.cursors.remove(&id);
                        } else {
                            self.cursors
                                .insert(id.clone(), event["index"].as_i64().ok_or(ERR_PROTOCOL)?);
                        }
                        out.extend(self.encode_reply(event)?);
                    }
                }
                Err(_) => {
                    self.cursors.remove(&id);
                    out.extend(self.encode_reply(json!({"type":"error","id":id,"code":"chat.resume_unavailable","message":"The replay buffer is no longer available.","retryable":false}))?);
                }
            }
        }
        Ok(out)
    }
}

impl Drop for DesktopSession {
    fn drop(&mut self) {
        self.close();
    }
}

struct AppGuard(Arc<apps::Applications>);
impl Drop for AppGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

/// Reconnect with fresh account tickets and fresh Noise handshakes. The account provider
/// owns sign-in; cancellation drops WSS and all in-memory resume material.
pub async fn run(
    provider: &dyn TicketProvider,
    identity: &Identity,
    tokens: TokenStore,
    name: &str,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    if !cfg!(feature = "remote-access") {
        return Err("pairing.remote_not_built".into());
    }
    let apps = apps::Applications::shared();
    let _guard = AppGuard(apps.clone());
    let mut attempt = 0u32;
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        let result = tokio::select! {
            _=stop.changed()=>return Ok(()),
            result=async {
                let ticket=provider.ticket(identity.public).await?;
                ticket.validate(&identity.public)?;
                connect_with_apps(RELAY_URL,ticket,identity,tokens.clone(),name,apps.clone(),Some(provider)).await
            }=>result,
        };
        if let Err(error) = result {
            if error == ERR_SUPERSEDED {
                return Ok(());
            }
            if error == "pairing.remote_signin_unavailable" || error == ERR_REVOKED {
                return Err(error);
            }
        }
        apps.cleanup(&tokens);
        let cap = (500u64.saturating_mul(1u64 << attempt.min(5))).min(15000);
        let random = u64::from_le_bytes(control_api::random_bytes::<8>()?);
        tokio::select! {_=stop.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_millis(random%cap))=>{}}
        attempt = attempt.saturating_add(1);
    }
}

/// One outbound connection. Caller owns cancellation/retry; dropping this future closes WSS.
/// `transport.fallback` is the seam for a future ordered, reliable WebRTC FrameLink.
pub async fn connect(
    provider: &dyn TicketProvider,
    identity: &Identity,
    tokens: TokenStore,
    name: &str,
) -> Result<(), String> {
    if !cfg!(feature = "remote-access") {
        return Err("pairing.remote_not_built".into());
    }
    let ticket = provider.ticket(identity.public).await?;
    ticket.validate(&identity.public)?;
    connect_with_apps(
        RELAY_URL,
        ticket,
        identity,
        tokens,
        name,
        apps::Applications::shared(),
        Some(provider),
    )
    .await
}

#[cfg(test)]
pub(super) async fn connect_url(
    url: &str,
    ticket: RelayTicket,
    identity: &Identity,
    tokens: TokenStore,
    name: &str,
) -> Result<(), String> {
    connect_with_apps(
        url,
        ticket,
        identity,
        tokens,
        name,
        apps::Applications::shared(),
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn connect_with_apps(
    url: &str,
    ticket: RelayTicket,
    identity: &Identity,
    tokens: TokenStore,
    name: &str,
    apps: Arc<apps::Applications>,
    provider: Option<&dyn TicketProvider>,
) -> Result<(), String> {
    let mut request = url.into_client_request().map_err(|_| ERR_PROTOCOL)?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", SUBPROTOCOL.parse().unwrap());
    let (mut ws, response) = tokio_tungstenite::connect_async_with_config(
        request,
        Some(
            tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(Some(frames::MAX_FRAME))
                .max_frame_size(Some(frames::MAX_FRAME)),
        ),
        false,
    )
    .await
    .map_err(|_| ERR_PROTOCOL)?;
    if response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|h| h.to_str().ok())
        != Some(SUBPROTOCOL)
    {
        return Err(ERR_PROTOCOL.into());
    }
    let zero = "00000000000000000000000000000000";
    let mut seq = 0u64;
    ws.send(Message::Text(json!({"v":1,"seq":seq,"session":zero,"type":"auth","role":"desktop","credential":ticket.credential}).to_string().into())).await.map_err(|_| ERR_PROTOCOL)?;
    seq += 1;
    let mut sessions: HashMap<String, (DesktopSession, bool, Instant)> = HashMap::new();
    let mut authenticated = false;
    let mut expires_at = ticket.expires_at;
    let mut refreshing: Option<TicketFuture<'_>> = None;
    let mut awaiting_refresh: Option<u64> = None;
    let mut retry_refresh_at = 0;
    let auth_started = Instant::now();
    // Retain every used id, including ended sessions (stronger than the relay's 64 tombstones).
    // Replies are sent only for live ids; late signalling/frames never name a dead session.
    let mut used_sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut incoming_seq = 0u64;
    let mut last = Instant::now();
    let mut last_ping = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    loop {
        tokio::select! {
            refreshed = async { refreshing.as_mut().unwrap().await }, if refreshing.is_some() => {
                refreshing = None;
                match refreshed {
                    Ok(fresh) => {
                        fresh.validate(&identity.public)?;
                        if fresh.desktop_id != ticket.desktop_id { return Err(ERR_PROTOCOL.into()); }
                        ws.send(Message::Text(json!({"v":1,"seq":seq,"session":zero,"type":"auth","role":"desktop","credential":fresh.credential}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?;
                        seq += 1;
                        awaiting_refresh = Some(fresh.expires_at);
                    }
                    Err(_) => retry_refresh_at = control_api::now_secs() + 1,
                }
            }
            _ = tick.tick() => {
                let now = control_api::now_secs();
                if (!authenticated && auth_started.elapsed() >= Duration::from_secs(10)) || now >= expires_at.saturating_add(TICKET_GRACE_SECS) { return Err(ERR_PROTOCOL.into()); }
                if authenticated && refreshing.is_none() && awaiting_refresh.is_none() && now >= retry_refresh_at && now >= expires_at.saturating_sub(TICKET_GRACE_SECS) {
                    if let Some(provider) = provider { refreshing = Some(provider.ticket(identity.public)); }
                }
                if last.elapsed()>Duration::from_secs(60) { return Err(ERR_PROTOCOL.into()); }
                let mut ended=Vec::new();
                for (sid,(session,_,activity)) in sessions.iter_mut() {
                    if session.channel.handshake_expired() {
                        session.close();
                        ended.push(sid.clone());
                    } else if session.check_authorized().is_err() {
                        if let Some((device,_))=&session.authenticated {apps.revoke(device);}
                        for reply in session.reject("auth.revoked") {ws.send(Message::Binary(reply.into())).await.map_err(|_|ERR_PROTOCOL)?;}
                        ended.push(sid.clone());
                    } else if activity.elapsed()>Duration::from_secs(60) {session.close();ended.push(sid.clone());}
                }
                for sid in ended {
                    sessions.remove(&sid);
                    ws.send(Message::Text(json!({"v":1,"seq":seq,"session":sid,"type":"leave","reason":"session_closed"}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?;seq+=1;
                }
                apps.cleanup(&tokens);
                let mut finished=Vec::new();
                for (sid,(session,_,_)) in sessions.iter_mut() {
                    for reply in session.poll().await? {ws.send(Message::Binary(reply.into())).await.map_err(|_|ERR_PROTOCOL)?;}
                    if session.closed {finished.push(sid.clone());}
                }
                for sid in finished {
                    sessions.remove(&sid);
                    ws.send(Message::Text(json!({"v":1,"seq":seq,"session":sid,"type":"leave","reason":"session_closed"}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?;seq+=1;
                }
                if last_ping.elapsed()>Duration::from_secs(20) {
                    ws.send(Message::Text(json!({"v":1,"seq":seq,"session":zero,"type":"ping","ts":control_api::now_secs()*1000}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?;
                    seq+=1;
                    last_ping=Instant::now();
                }
            }
            message=ws.next() => {
                let message=message.ok_or(ERR_PROTOCOL)?.map_err(|_|ERR_PROTOCOL)?;
                last=Instant::now();
                match message {
                    Message::Text(text) => {
                        if text.len()>16384 { return Err(ERR_PROTOCOL.into()); }
                        let v=json_strict::parse(text.as_bytes(),"not_json").map_err(|_|ERR_PROTOCOL)?;
                        if v["v"]!=1 || v["seq"].as_u64()!=Some(incoming_seq) { return Err(ERR_PROTOCOL.into()); }
                        incoming_seq+=1;
                        let sid=field(&v,"session")?;
                        match field(&v,"type")? {
                            "auth.ok" if sid==zero && v["role"]=="desktop" => {
                                if authenticated && awaiting_refresh.is_none() { return Err(ERR_PROTOCOL.into()); }
                                authenticated=true;
                                if let Some(exp) = awaiting_refresh.take() { expires_at=exp; }
                            },
                            "session.incoming" if authenticated => {
                                if sessions.len()>=8 || used_sessions.len()>=65536 || sessions.contains_key(sid) || !used_sessions.insert(sid.to_string()) { return Err(ERR_PROTOCOL.into()); }
                                let mut session=DesktopSession::new(identity,field(&v,"mode")?,sid,&ticket.desktop_id,name,tokens.clone())?;
                                session.apps=apps.clone();
                                sessions.insert(sid.into(),(session,false,Instant::now()));
                            }
                            "transport.fallback" if authenticated => { if let Some(owned)=sessions.get_mut(sid) {
                                if owned.1 { return Err(ERR_PROTOCOL.into()); }
                                owned.1=true; owned.0.channel.start_handshake();
                            } }
                            "peer.left" => { if let Some((mut s,_,_))=sessions.remove(sid) {s.close();} }
                            // Session ownership (APP_PROTOCOL 7.5): the desktop speaks only for the
                            // connection (all-zero id) and for sessions the relay created for it;
                            // anything else is dropped and never touches another session.
                            "ping" if sid==zero || sessions.contains_key(sid) => {
                                ws.send(Message::Text(json!({"v":1,"seq":seq,"session":sid,"type":"pong","ts":v["ts"]}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?; seq+=1;
                            }
                            "ping"|"pong"|"presence"|"joined"|"ice"|"sdp.offer" => {},
                            "error" if sid != zero => {
                                // The relay can send peer.left and error for the same ended session.
                                // Never answer either message, even after the relay evicts its tombstone.
                                if let Some((mut session,_,_))=sessions.remove(sid) { session.close(); }
                            }
                            "error" => return Err(match v["code"].as_str() {
                                Some("relay.session_busy") => crate::error_key::err(ERR_SUPERSEDED).into(),
                                Some("relay.revoked"|"relay.auth_failed") => ERR_REVOKED.into(),
                                _ => ERR_PROTOCOL.into(),
                            }),
                            _ => return Err(ERR_PROTOCOL.into()),
                        }
                    }
                    Message::Binary(frame) if authenticated => {
                        if frame.len()<21 { return Err(ERR_PROTOCOL.into()); }
                        let sid=control_api::hex(&frame[5..21]);
                        let Some((session,fallback,activity))=sessions.get_mut(&sid)else{continue;};
                        *activity=Instant::now();
                        let replies=if *fallback {session.receive(&frame)}else{Err(ERR_PROTOCOL.into())};
                        match replies {
                            Ok(replies)=>for reply in replies {ws.send(Message::Binary(reply.into())).await.map_err(|_|ERR_PROTOCOL)?;},
                            Err(_)=>session.close(),
                        }
                        if session.closed {
                            sessions.remove(&sid);
                            ws.send(Message::Text(json!({"v":1,"seq":seq,"session":sid,"type":"leave","reason":"session_closed"}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?;seq+=1;
                        }
                    }
                    Message::Ping(p) => {ws.send(Message::Pong(p)).await.map_err(|_|ERR_PROTOCOL)?;}
                    Message::Pong(_) => {},
                    Message::Close(frame) => return match frame.map(|f| u16::from(f.code)) {
                        Some(4409) => Err(crate::error_key::err(ERR_SUPERSEDED).into()),
                        Some(4403) => Err(ERR_REVOKED.into()),
                        Some(4500 | 4503) => Err(ERR_PROTOCOL.into()),
                        _ => Ok(()),
                    },
                    _ => return Err(ERR_PROTOCOL.into()),
                }
            }
        }
    }
}

#[cfg(test)]
mod ticket_tests {
    use super::*;
    fn ticket(key: [u8; 32], alg: &str, jti: &str, iat: u64, exp: u64) -> RelayTicket {
        RelayTicket {
            credential: test_credential(&key, "desktop_01", alg, jti, iat, exp),
            desktop_id: "desktop_01".into(),
            public_key: key,
            expires_at: exp,
        }
    }

    fn secure_pair() -> (tempfile::TempDir, DesktopSession, snow::TransportState) {
        let dir = tempfile::tempdir().unwrap();
        let identity = Identity::from_secret([7; 32]).unwrap();
        let mut session = DesktopSession::new(
            &identity,
            "pair",
            "11111111111111111111111111111111",
            "desktop_01",
            "Test",
            TokenStore::at(dir.path()),
        )
        .unwrap();
        let mut client = snow::Builder::new(noise::SUITE.parse().unwrap())
            .local_private_key(&[9; 32])
            .unwrap()
            .prologue(b"StrukLabs-App/1|pair|11111111111111111111111111111111")
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut out = [0; 65535];
        let n = client.write_message(&[], &mut out).unwrap();
        let first = frames::encode_frame(1, &[0x11; 16], &out[..n]).unwrap();
        let reply = session.receive(&first).unwrap();
        client.read_message(&reply[0][21..], &mut out).unwrap();
        let n = client.write_message(&[], &mut out).unwrap();
        session
            .receive(&frames::encode_frame(1, &[0x11; 16], &out[..n]).unwrap())
            .unwrap();
        (dir, session, client.into_transport_mode().unwrap())
    }

    fn send_raw(
        session: &mut DesktopSession,
        client: &mut snow::TransportState,
        text: &[u8],
    ) -> Vec<Vec<u8>> {
        let mut replies = Vec::new();
        for record in frames::split_into_records(text) {
            let mut out = vec![0; record.len() + 16];
            let n = client.write_message(&record, &mut out).unwrap();
            replies.extend(
                session
                    .receive(&frames::encode_frame(2, &[0x11; 16], &out[..n]).unwrap())
                    .unwrap(),
            );
        }
        replies
    }

    fn request(
        session: &mut DesktopSession,
        client: &mut snow::TransportState,
        text: &[u8],
    ) -> Value {
        let replies = send_raw(session, client, text);
        decode_reply(client, replies)
    }

    fn decode_reply(client: &mut snow::TransportState, frames: Vec<Vec<u8>>) -> Value {
        let mut assembler = frames::RecordAssembler::default();
        for frame in frames {
            let mut plain = vec![0; 65535];
            let n = client.read_message(&frame[21..], &mut plain).unwrap();
            if let Some(envelope) = assembler.push(&plain[..n], noise::MAX_ENVELOPE).unwrap() {
                return serde_json::from_slice(&envelope).unwrap();
            }
        }
        panic!("missing reply")
    }

    #[test]
    fn sequence_starts_at_zero_and_gaps_or_repeats_are_fatal() {
        for bad in [0, 2] {
            let (_dir, mut session, mut client) = secure_pair();
            assert!(send_raw(
                &mut session,
                &mut client,
                br#"{"v":1,"seq":0,"type":"future.type"}"#
            )
            .is_empty());
            let text = json!({"v":1,"seq":bad,"type":"ping","ts":1}).to_string();
            let error = request(&mut session, &mut client, text.as_bytes());
            assert_eq!(error["code"], "protocol.out_of_sequence");
            assert_eq!(error["seq"], 0);
            assert!(session.closed);
        }
        let (_dir, mut session, mut client) = secure_pair();
        let error = request(
            &mut session,
            &mut client,
            br#"{"v":1,"seq":1,"type":"ping","ts":1}"#,
        );
        assert_eq!(error["code"], "protocol.out_of_sequence");
        assert_eq!(error["seq"], 0);
    }

    #[test]
    fn unknown_type_consumes_sequence_and_duplicate_members_close_the_session() {
        let (_dir, mut session, mut client) = secure_pair();
        assert!(send_raw(
            &mut session,
            &mut client,
            br#"{"v":1,"seq":0,"type":"future.type"}"#
        )
        .is_empty());
        let pong = request(
            &mut session,
            &mut client,
            br#"{"v":1,"seq":1,"type":"ping","ts":1}"#,
        );
        assert_eq!(pong["type"], "pong");
        assert_eq!(pong["seq"], 0);
        let error = request(
            &mut session,
            &mut client,
            br#"{"v":1,"seq":2,"type":"ping","ts":1,"nested":{"a":1,"a":2}}"#,
        );
        assert_eq!(error["code"], "protocol.malformed");
        assert!(session.closed);
    }

    #[test]
    fn oversized_reply_does_not_leave_a_sequence_gap() {
        let (_dir, mut session, mut client) = secure_pair();
        assert_eq!(
            session.encode_reply(
                json!({"type":"status","id":"large_01","payload":"x".repeat(noise::MAX_ENVELOPE)})
            ),
            Err("protocol.too_large".into())
        );
        assert_eq!(session.outbound, 0);
        let pong = request(
            &mut session,
            &mut client,
            br#"{"v":1,"seq":0,"type":"ping","ts":1}"#,
        );
        assert_eq!(pong["seq"], 0);
        assert!(!session.closed);
    }

    #[allow(clippy::result_large_err)]
    async fn accept_test(
        socket: tokio::net::TcpStream,
    ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
        tokio_tungstenite::accept_hdr_async(socket, |_: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            response.headers_mut().insert("Sec-WebSocket-Protocol",SUBPROTOCOL.parse().unwrap());
            Ok(response)
        }).await.unwrap()
    }

    async fn signal(
        ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        seq: u64,
        sid: &str,
        mut value: Value,
    ) {
        value["v"] = json!(1);
        value["seq"] = json!(seq);
        value["session"] = json!(sid);
        ws.send(Message::Text(value.to_string().into()))
            .await
            .unwrap();
    }

    async fn text_message(
        ws: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) -> Value {
        let message = ws.next().await.unwrap().unwrap();
        serde_json::from_str(message.to_text().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn refresh_reauthenticates_the_live_connection_and_preserves_its_session() {
        struct FreshTickets(std::sync::atomic::AtomicUsize);
        impl TicketProvider for FreshTickets {
            fn ticket(&self, key: [u8; 32]) -> TicketFuture<'_> {
                let first = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
                Box::pin(async move {
                    if first {
                        return Err(ERR_PROTOCOL.into());
                    }
                    let now = control_api::now_secs();
                    Ok(ticket(
                        key,
                        "EdDSA",
                        &format!("refresh-{}", encode(&control_api::random_bytes::<16>()?)),
                        now,
                        now + 300,
                    ))
                })
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/v1/app", listener.local_addr().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let identity = Identity::from_secret([7; 32]).unwrap();
        let now = control_api::now_secs();
        let initial = ticket(
            identity.public,
            "EdDSA",
            "initial-refresh-0001",
            now,
            now + 1,
        );
        let zero = "00000000000000000000000000000000";
        let sid = "11111111111111111111111111111111";
        let server = async {
            let mut ws = accept_test(listener.accept().await.unwrap().0).await;
            let auth = text_message(&mut ws).await;
            assert_eq!(auth["seq"], 0);
            assert_eq!(auth["type"], "auth");
            signal(&mut ws, 0, zero, json!({"type":"auth.ok","role":"desktop"})).await;
            signal(
                &mut ws,
                1,
                sid,
                json!({"type":"session.incoming","mode":"pair"}),
            )
            .await;
            signal(
                &mut ws,
                2,
                sid,
                json!({"type":"transport.fallback","reason":"policy"}),
            )
            .await;
            let refresh = text_message(&mut ws).await;
            assert_eq!(refresh["type"], "auth");
            assert_eq!(refresh["seq"], 1);
            assert_ne!(refresh["credential"], auth["credential"]);
            signal(&mut ws, 3, zero, json!({"type":"auth.ok","role":"desktop"})).await;
            let mut client = snow::Builder::new(noise::SUITE.parse().unwrap())
                .local_private_key(&[9; 32])
                .unwrap()
                .prologue(b"StrukLabs-App/1|pair|11111111111111111111111111111111")
                .unwrap()
                .build_initiator()
                .unwrap();
            let mut out = [0; 96];
            let n = client.write_message(&[], &mut out).unwrap();
            ws.send(Message::Binary(
                frames::encode_frame(1, &[0x11; 16], &out[..n])
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
            let reply = ws.next().await.unwrap().unwrap().into_data();
            assert_eq!(reply.len(), 117);
            client.read_message(&reply[21..], &mut out).unwrap();
            ws.close(None).await.unwrap();
        };
        let provider = FreshTickets(std::sync::atomic::AtomicUsize::new(0));
        let connection = connect_with_apps(
            &url,
            initial,
            &identity,
            TokenStore::at(dir.path()),
            "Test",
            apps::Applications::shared(),
            Some(&provider),
        );
        let (_, result) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(server, connection)
        })
        .await
        .unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(provider.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ended_sessions_never_generate_replies_or_twenty_strikes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/v1/app", listener.local_addr().unwrap());
        let identity = Identity::from_secret([7; 32]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let now = control_api::now_secs();
        let initial = ticket(
            identity.public,
            "EdDSA",
            "tombstone-ticket-0001",
            now,
            now + 300,
        );
        let zero = "00000000000000000000000000000000";
        let sid = "11111111111111111111111111111111";
        let server = async {
            let mut ws = accept_test(listener.accept().await.unwrap().0).await;
            text_message(&mut ws).await;
            signal(&mut ws, 0, zero, json!({"type":"auth.ok","role":"desktop"})).await;
            signal(
                &mut ws,
                1,
                sid,
                json!({"type":"session.incoming","mode":"pair"}),
            )
            .await;
            signal(
                &mut ws,
                2,
                sid,
                json!({"type":"peer.left","reason":"bad_frame"}),
            )
            .await;
            signal(
                &mut ws,
                3,
                sid,
                json!({"type":"error","code":"relay.bad_frame","message":"closed"}),
            )
            .await;
            signal(
                &mut ws,
                4,
                sid,
                json!({"type":"transport.fallback","reason":"timeout"}),
            )
            .await;
            signal(&mut ws, 5, sid, json!({"type":"sdp.offer","sdp":"late"})).await;
            signal(
                &mut ws,
                6,
                sid,
                json!({"type":"ice","candidate":"late.local"}),
            )
            .await;
            ws.send(Message::Binary(
                frames::encode_frame(1, &[0x11; 16], &[0; 32])
                    .unwrap()
                    .into(),
            ))
            .await
            .unwrap();
            for seq in 7..27 {
                signal(&mut ws, seq, sid, json!({"type":"ping","ts":seq})).await;
            }
            signal(&mut ws, 27, zero, json!({"type":"ping","ts":42})).await;
            let pong = text_message(&mut ws).await;
            assert_eq!(pong["session"], zero);
            assert_eq!(pong["seq"], 1);
            assert_eq!(pong["ts"], 42);
            ws.close(None).await.unwrap();
        };
        let connection = connect_url(&url, initial, &identity, TokenStore::at(dir.path()), "Test");
        let (_, result) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(server, connection)
        })
        .await
        .unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn superseded_close_stops_but_internal_errors_remain_retryable() {
        for (code, expected) in [
            (4409, ERR_SUPERSEDED),
            (4500, ERR_PROTOCOL),
            (4503, ERR_PROTOCOL),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("ws://{}/v1/app", listener.local_addr().unwrap());
            let identity = Identity::from_secret([7; 32]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let now = control_api::now_secs();
            let initial = ticket(
                identity.public,
                "EdDSA",
                "close-code-ticket001",
                now,
                now + 300,
            );
            let server = async {
                let mut ws = accept_test(listener.accept().await.unwrap().0).await;
                text_message(&mut ws).await;
                ws.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: "test".into(),
                }))
                .await
                .unwrap();
            };
            let (_, result) = tokio::join!(
                server,
                connect_url(&url, initial, &identity, TokenStore::at(dir.path()), "Test")
            );
            assert_eq!(result, Err(expected.into()));
        }
    }

    #[test]
    fn desktop_ticket_pins_alg_lifetime_key_and_rejects_jti_replay() {
        let now = control_api::now_secs();
        let k = [1; 32];
        assert!(ticket(k, "EdDSA", "jti-ok-000000001", now, now + 300)
            .validate(&k)
            .is_ok());
        // key binding
        assert!(ticket(k, "EdDSA", "jti-key-00000001", now, now + 300)
            .validate(&[2; 32])
            .is_err());
        // algorithm pinning
        for alg in ["none", "HS256", "RS256", "eddsa"] {
            assert!(
                ticket(k, alg, "jti-alg-00000001", now, now + 300)
                    .validate(&k)
                    .is_err(),
                "{alg}"
            );
        }
        // lifetime: exp - iat <= 300, expired, not yet valid
        assert!(ticket(k, "EdDSA", "jti-long-0000001", now, now + 301)
            .validate(&k)
            .is_err());
        assert!(ticket(k, "EdDSA", "jti-old-00000001", now - 400, now - 100)
            .validate(&k)
            .is_err());
        assert!(ticket(k, "EdDSA", "jti-future-00001", now + 60, now + 120)
            .validate(&k)
            .is_err());
        // expires_at must match the signed exp
        let mut t = ticket(k, "EdDSA", "jti-exp-00000001", now, now + 100);
        t.expires_at = now + 200;
        assert!(t.validate(&k).is_err());
        // replay: the second use of the same jti fails, even though everything else is valid
        let first = ticket(k, "EdDSA", "jti-replay-000001", now, now + 60);
        assert!(first.validate(&k).is_ok());
        assert!(ticket(k, "EdDSA", "jti-replay-000001", now, now + 60)
            .validate(&k)
            .is_err());
        // not a JWT at all
        let mut bad = ticket(k, "EdDSA", "jti-bad-00000001", now, now + 60);
        bad.credential = "mock-ticket-no-production-value".into();
        assert!(bad.validate(&k).is_err());
    }
}
