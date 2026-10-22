//! Outbound relay transport. Account sign-in/ticket issuance stays behind `TicketProvider`.
use super::{
    noise::{Channel, Identity},
    *,
};
use futures_util::{SinkExt, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub const RELAY_URL: &str = "wss://relay.struklabs.com:7843/v1/app";
pub const SUBPROTOCOL: &str = "struklabs.app.v1";
pub type TicketFuture<'a> = Pin<Box<dyn Future<Output = Result<RelayTicket, String>> + Send + 'a>>;

/// Issued after StrukLabs account sign-in. Never accept a ticket for another static key.
pub struct RelayTicket {
    pub credential: String,
    pub desktop_id: String,
    pub public_key: [u8; 32],
    pub expires_at: u64,
}
impl RelayTicket {
    fn validate(&self, key: &[u8; 32]) -> Result<(), String> {
        if self.public_key != *key
            || self.expires_at <= control_api::now_secs()
            || self.expires_at > control_api::now_secs() + 60
            || !(16..=4096).contains(&self.credential.len())
            || !(8..=64).contains(&self.desktop_id.len())
            || !self
                .desktop_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ERR_PROTOCOL.into());
        }
        Ok(())
    }
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
            closed: false,
            apps: apps::Applications::shared(),
            cursors: HashMap::new(),
            pending_requests: Vec::new(),
        })
    }

    pub fn receive(&mut self, frame: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let result = self.receive_inner(frame);
        if let Err(error) = result {
            let code = if error == ERR_REVOKED {
                if self.authenticated.is_some() {
                    "auth.revoked"
                } else {
                    "auth.invalid_token"
                }
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
            let v: Value = serde_json::from_slice(&bytes).map_err(|_| ERR_PROTOCOL)?;
            if v["v"] != 1 || v["seq"].as_u64() != Some(self.inbound) {
                return Err(ERR_PROTOCOL.into());
            }
            self.inbound = self.inbound.checked_add(1).ok_or(ERR_PROTOCOL)?;
            let peer = self.channel.peer()?;
            let mut reply = match field(&v, "type")? {
                "pair.request" if self.mode == "pair" => {
                    let (status, body) = self.hub.remote_pair(
                        field(&v, "op")?,
                        &v["body"],
                        peer,
                        self.desktop_key,
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
                _ if self.authenticated.is_some() => {
                    json!({"type":"error","id":v["id"],"code":"protocol.unknown_type","message":"Unsupported request.","retryable":false})
                }
                _ => return Err(ERR_PROTOCOL.into()),
            };
            if self.pending_requests.len() > 32 {
                return Err(ERR_PROTOCOL.into());
            }
            if reply.is_null() {
                return Ok(frames);
            }
            reply["v"] = json!(1);
            reply["seq"] = json!(self.outbound);
            self.outbound = self.outbound.checked_add(1).ok_or(ERR_PROTOCOL)?;
            frames.extend(
                self.channel
                    .send(&serde_json::to_vec(&reply).map_err(|_| ERR_PROTOCOL)?)?,
            );
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
            self.apps.detach(device);
        }
        self.closed = true;
        self.channel.close();
        self.authenticated = None;
    }

    fn encode_reply(&mut self, mut v: Value) -> Result<Vec<Vec<u8>>, String> {
        v["v"] = json!(1);
        v["seq"] = json!(self.outbound);
        self.outbound = self.outbound.checked_add(1).ok_or(ERR_PROTOCOL)?;
        self.channel
            .send(&serde_json::to_vec(&v).map_err(|_| ERR_PROTOCOL)?)
    }

    pub async fn poll(&mut self) -> Result<Vec<Vec<u8>>, String> {
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
                connect_with_apps(RELAY_URL,ticket,identity,tokens.clone(),name,apps.clone()).await
            }=>result,
        };
        if let Err(error) = result {
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
    connect_url(RELAY_URL, ticket, identity, tokens, name).await
}

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
    )
    .await
}

async fn connect_with_apps(
    url: &str,
    ticket: RelayTicket,
    identity: &Identity,
    tokens: TokenStore,
    name: &str,
    apps: Arc<apps::Applications>,
) -> Result<(), String> {
    let mut request = url.into_client_request().map_err(|_| ERR_PROTOCOL)?;
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", SUBPROTOCOL.parse().unwrap());
    let (mut ws, response) = tokio_tungstenite::connect_async_with_config(
        request,
        Some(
            tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(Some(65556))
                .max_frame_size(Some(65556)),
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
    let mut incoming_seq = 0u64;
    let mut last = Instant::now();
    let mut last_ping = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if last.elapsed()>Duration::from_secs(60) { return Err(ERR_PROTOCOL.into()); }
                let mut ended=Vec::new();
                for (sid,(session,_,activity)) in sessions.iter_mut() {
                    if session.check_authorized().is_err() {
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
                for (session,_,_) in sessions.values_mut() {
                    for reply in session.poll().await? {ws.send(Message::Binary(reply.into())).await.map_err(|_|ERR_PROTOCOL)?;}
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
                        let v:Value=serde_json::from_str(&text).map_err(|_|ERR_PROTOCOL)?;
                        if v["v"]!=1 || v["seq"].as_u64()!=Some(incoming_seq) { return Err(ERR_PROTOCOL.into()); }
                        incoming_seq+=1;
                        let sid=field(&v,"session")?;
                        match field(&v,"type")? {
                            "auth.ok" if !authenticated && v["role"]=="desktop" => authenticated=true,
                            "session.incoming" if authenticated => {
                                if sessions.len()>=8 || sessions.contains_key(sid) { return Err(ERR_PROTOCOL.into()); }
                                let mut session=DesktopSession::new(identity,field(&v,"mode")?,sid,&ticket.desktop_id,name,tokens.clone())?;
                                session.apps=apps.clone();
                                sessions.insert(sid.into(),(session,false,Instant::now()));
                            }
                            "transport.fallback" if authenticated => { sessions.get_mut(sid).ok_or(ERR_PROTOCOL)?.1=true; }
                            "peer.left" => { if let Some((mut s,_,_))=sessions.remove(sid) {s.close();} }
                            "ping" => {
                                ws.send(Message::Text(json!({"v":1,"seq":seq,"session":sid,"type":"pong","ts":v["ts"]}).to_string().into())).await.map_err(|_|ERR_PROTOCOL)?; seq+=1;
                            }
                            "pong"|"presence"|"joined"|"ice"|"sdp.offer" => {},
                            "error" => return Err(if matches!(v["code"].as_str(),Some("relay.revoked"|"relay.auth_failed")){ERR_REVOKED}else{ERR_PROTOCOL}.into()),
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
                    Message::Close(_) => return Ok(()),
                    _ => return Err(ERR_PROTOCOL.into()),
                }
            }
        }
    }
}

#[cfg(test)]
mod ticket_tests {
    use super::*;
    #[test]
    fn tickets_are_short_lived_and_bound_to_the_noise_key() {
        let mut ticket = RelayTicket {
            credential: "mock-ticket-no-production-value".into(),
            desktop_id: "desktop_01".into(),
            public_key: [1; 32],
            expires_at: control_api::now_secs() + 60,
        };
        assert!(ticket.validate(&[1; 32]).is_ok());
        assert!(ticket.validate(&[2; 32]).is_err());
        ticket.expires_at = control_api::now_secs();
        assert!(ticket.validate(&[1; 32]).is_err());
        ticket.expires_at = control_api::now_secs() + 61;
        assert!(ticket.validate(&[1; 32]).is_err());
    }
}
