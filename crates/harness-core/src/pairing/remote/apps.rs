//! Per-device chat state survives relay/Noise reconnections, with bounded acknowledged replay.
use super::*;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

fn instances() -> &'static Mutex<Vec<std::sync::Weak<Applications>>> {
    static INSTANCES: OnceLock<Mutex<Vec<std::sync::Weak<Applications>>>> = OnceLock::new();
    INSTANCES.get_or_init(Default::default)
}

pub(in crate::pairing) fn revoke_device(device: &str) {
    instances().lock().unwrap().retain(|weak| {
        if let Some(apps) = weak.upgrade() {
            apps.revoke(device);
            true
        } else {
            false
        }
    });
}

pub(crate) fn revoke_token(token_id: &str) {
    instances().lock().unwrap().retain(|weak| {
        if let Some(apps) = weak.upgrade() {
            apps.chats.lock().unwrap().retain(|_, chat| {
                if chat.token_id == token_id {
                    let _ = chat.cancel.send(true);
                    false
                } else {
                    true
                }
            });
            true
        } else {
            false
        }
    });
}

struct Chat {
    events: VecDeque<(u64, Instant, Value, usize)>,
    bytes: usize,
    next: u64,
    done: Option<Value>,
    cancel: watch::Sender<bool>,
    changed: Instant,
    detached: Option<Instant>,
    token_id: String,
    /// Digest of the request (model, messages, params): an identical retry attaches to the run.
    fingerprint: [u8; 32],
}
#[derive(Default)]
pub struct Applications {
    chats: Mutex<HashMap<(String, String), Chat>>,
    limiter: crate::gateway::limits::RateLimiter,
    /// One live session per device: the generation of the session that owns each device.
    live: Mutex<HashMap<String, u64>>,
    generation: std::sync::atomic::AtomicU64,
}

fn request_fingerprint(request: &Value) -> [u8; 32] {
    let canonical =
        json!({"model":request["model"],"messages":request["messages"],"params":request["params"]});
    let mut h = Blake2s256::new();
    h.update(canonical.to_string().as_bytes());
    h.finalize().into()
}

impl Applications {
    pub fn shared() -> Arc<Self> {
        let apps = Arc::new(Self::default());
        let mut list = instances().lock().unwrap();
        list.retain(|w| w.strong_count() > 0);
        list.push(Arc::downgrade(&apps));
        apps
    }
    pub fn shutdown(&self) {
        for (_, chat) in self.chats.lock().unwrap().drain() {
            let _ = chat.cancel.send(true);
        }
    }
    /// Registers a newly authenticated session for `device`. Any older session of the device is
    /// superseded: it no longer owns the device and must end with `session.superseded`.
    pub fn claim_session(&self, device: &str) -> u64 {
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.live.lock().unwrap().insert(device.into(), generation);
        generation
    }
    pub fn is_current(&self, device: &str, generation: u64) -> bool {
        self.live.lock().unwrap().get(device) == Some(&generation)
    }
    /// Ends ownership; true when `generation` was still the current session of the device.
    pub fn release(&self, device: &str, generation: u64) -> bool {
        let mut live = self.live.lock().unwrap();
        if live.get(device) == Some(&generation) {
            live.remove(device);
            true
        } else {
            false
        }
    }
    #[cfg(test)]
    pub(super) fn insert_test_chat(&self, device: &str, id: &str, token_id: &str, request: &Value) {
        let (cancel, _) = watch::channel(false);
        self.chats.lock().unwrap().insert(
            (device.into(), id.into()),
            Chat {
                events: VecDeque::new(),
                bytes: 0,
                next: 0,
                done: None,
                cancel,
                changed: Instant::now(),
                detached: None,
                token_id: token_id.into(),
                fingerprint: request_fingerprint(request),
            },
        );
    }
    pub fn attach(&self, device: &str) {
        for ((d, _), c) in self.chats.lock().unwrap().iter_mut() {
            if d == device {
                c.detached = None;
            }
        }
    }
    pub fn detach(&self, device: &str) {
        for ((d, _), c) in self.chats.lock().unwrap().iter_mut() {
            if d == device && c.detached.is_none() {
                c.detached = Some(Instant::now());
            }
        }
    }
    pub fn revoke(&self, device: &str) {
        self.chats.lock().unwrap().retain(|(d, _), chat| {
            if d == device {
                let _ = chat.cancel.send(true);
                false
            } else {
                true
            }
        });
    }
    pub fn ack(&self, device: &str, id: &str, index: u64) -> Result<(), String> {
        let mut chats = self.chats.lock().unwrap();
        let c = chats
            .get_mut(&(device.into(), id.into()))
            .ok_or(ERR_PROTOCOL)?;
        if index >= c.next {
            return Err(ERR_PROTOCOL.into());
        }
        while c.events.front().is_some_and(|(i, _, _, _)| *i <= index) {
            c.bytes -= c.events.pop_front().unwrap().3;
        }
        Ok(())
    }
    pub fn cancel(&self, device: &str, id: &str) {
        if let Some(c) = self.chats.lock().unwrap().get(&(device.into(), id.into())) {
            let _ = c.cancel.send(true);
        }
    }
    pub fn replay(&self, device: &str, id: &str, last: i64) -> Result<Vec<Value>, String> {
        let mut chats = self.chats.lock().unwrap();
        let c = chats
            .get_mut(&(device.into(), id.into()))
            .ok_or(ERR_PROTOCOL)?;
        Self::expire(c);
        let next = last
            .checked_add(1)
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(ERR_PROTOCOL)?;
        if next > c.next || (next < c.next && c.events.front().is_none_or(|(i, _, _, _)| *i > next))
        {
            return Err(ERR_PROTOCOL.into());
        }
        let mut out: Vec<Value> = c
            .events
            .iter()
            .filter(|(i, _, _, _)| *i >= next)
            .map(|(_, _, v, _)| v.clone())
            .collect();
        if let Some(done) = &c.done {
            out.push(done.clone());
        }
        Ok(out)
    }
    fn expire(c: &mut Chat) {
        while c
            .events
            .front()
            .is_some_and(|(_, at, _, _)| at.elapsed() >= Duration::from_secs(30))
        {
            c.bytes -= c.events.pop_front().unwrap().3;
        }
    }
    pub fn cleanup(&self, tokens: &TokenStore) {
        self.chats.lock().unwrap().retain(|_, c| {
            Self::expire(c);
            if c.detached
                .is_some_and(|t| t.elapsed() >= Duration::from_secs(30))
                || (c.done.is_some() && c.changed.elapsed() > Duration::from_secs(30))
                || tokens.get(&c.token_id).is_err()
            {
                let _ = c.cancel.send(true);
                false
            } else {
                true
            }
        });
    }
    fn emit(&self, device: &str, id: &str, mut event: Value) {
        let mut chats = self.chats.lock().unwrap();
        let Some(c) = chats.get_mut(&(device.into(), id.into())) else {
            return;
        };
        c.changed = Instant::now();
        Self::expire(c);
        if event["type"] == "chat.delta" {
            event["index"] = json!(c.next);
            c.next += 1;
            let bytes = event.to_string().len();
            while c.bytes + bytes > 262144 {
                if let Some((_, _, _, n)) = c.events.pop_front() {
                    c.bytes -= n;
                } else {
                    break;
                }
            }
            c.bytes += bytes;
            c.events
                .push_back((c.next - 1, Instant::now(), event, bytes));
        } else {
            event["last_index"] = json!(c.next as i64 - 1);
            c.done = Some(event);
        }
    }
    /// The relay loop polls retained events; the bounded buffer provides backpressure by
    /// ending unavailable resumes rather than allocating an unbounded transport queue.
    pub fn start(
        self: &Arc<Self>,
        device: &str,
        info: TokenInfo,
        request: Value,
    ) -> Result<(), Value> {
        let error = |code: &str| {
            let message = match code {
                "chat.limit_reached" => "Too many chats are running for this device.",
                "chat.busy" => "This chat id is already used by a different request.",
                _ => "The chat request could not be completed.",
            };
            json!({"type":"error","id":request["id"],"code":code,"message":message,"retryable":true})
        };
        let id = request["id"]
            .as_str()
            .filter(|id| (8..=64).contains(&id.len()))
            .ok_or_else(|| error("protocol.malformed"))?
            .to_string();
        let model = request["model"]
            .as_str()
            .ok_or_else(|| error("protocol.malformed"))?;
        if !control_api::allows(&info.scopes, Scope::Inference)
            || !info.policy.allows_endpoint(GatewayEndpoint::Chat)
            || !info.policy.allows_model(model)
        {
            return Err(error("model.not_allowed"));
        }
        let messages = request["messages"]
            .as_array()
            .filter(|m| !m.is_empty() && m.len() <= 256)
            .ok_or_else(|| error("protocol.malformed"))?;
        let mut chars = 0;
        for message in messages {
            if !matches!(
                message["role"].as_str(),
                Some("system" | "user" | "assistant")
            ) {
                return Err(error("protocol.malformed"));
            }
            // The prompt limit is total UTF-8 bytes (APP_PROTOCOL 6.3), not characters.
            chars += message["content"]
                .as_str()
                .ok_or_else(|| error("protocol.malformed"))?
                .len();
        }
        if chars > 262144 {
            return Err(error("protocol.too_large"));
        }
        if let Some(params) = request.get("params") {
            let params = params
                .as_object()
                .ok_or_else(|| error("protocol.malformed"))?;
            for (key, max) in [("temperature", 2.0), ("top_p", 1.0)] {
                if params
                    .get(key)
                    .is_some_and(|v| v.as_f64().is_none_or(|n| n < 0.0 || n > max))
                {
                    return Err(error("protocol.malformed"));
                }
            }
            if params
                .get("max_tokens")
                .is_some_and(|v| v.as_u64().is_none_or(|n| n == 0 || n > 32768))
                || params.get("seed").is_some_and(|v| v.as_i64().is_none())
            {
                return Err(error("protocol.malformed"));
            }
        }
        let fingerprint = request_fingerprint(&request);
        {
            // An identical retry (same id, same request) attaches to the existing run without
            // consuming rate or concurrency; a different request under the id is `chat.busy`.
            let chats = self.chats.lock().unwrap();
            if let Some(chat) = chats.get(&(device.into(), id.clone())) {
                return if chat.fingerprint == fingerprint && chat.token_id == info.id {
                    Ok(())
                } else {
                    Err(error("chat.busy"))
                };
            }
        }
        let permit = self
            .limiter
            .admit(&info.id, &info.policy.limits)
            .map_err(|_| error("chat.rate_limited"))?;
        let (cancel, mut cancelled) = watch::channel(false);
        {
            let mut chats = self.chats.lock().unwrap();
            if chats.contains_key(&(device.into(), id.clone())) {
                return Err(error("chat.busy"));
            }
            if chats.len() >= 256
                || chats
                    .iter()
                    .filter(|((d, _), c)| d == device && c.done.is_none())
                    .count()
                    >= info.policy.limits.max_concurrent.unwrap_or(1).min(4) as usize
            {
                return Err(error("chat.limit_reached"));
            }
            chats.insert(
                (device.into(), id.clone()),
                Chat {
                    events: VecDeque::new(),
                    bytes: 0,
                    next: 0,
                    done: None,
                    cancel,
                    changed: Instant::now(),
                    detached: None,
                    token_id: info.id.clone(),
                    fingerprint,
                },
            );
        }
        let apps = self.clone();
        let device = device.to_string();
        tokio::spawn(async move {
            let _permit = permit;
            let (tx, mut rx) = mpsc::channel::<String>(4);
            let inference = Self::generate(request, info.policy, tx);
            tokio::pin!(inference);
            let mut watchdog = tokio::time::interval(Duration::from_secs(1));
            let reason = loop {
                tokio::select! {
                    _=cancelled.changed()=>break "cancelled",
                    _=watchdog.tick()=> {
                        let expired=apps.chats.lock().unwrap().get(&(device.clone(),id.clone())).is_none_or(|c|c.detached.is_some_and(|t|t.elapsed()>=Duration::from_secs(30)));
                        if expired {break "cancelled";}
                    },
                    delta=rx.recv(), if !rx.is_closed() || !rx.is_empty()=>if let Some(text)=delta {
                        apps.emit(&device,&id,json!({"type":"chat.delta","id":id,"text":text}));
                    },
                    result=&mut inference=> {
                        while let Ok(text)=rx.try_recv() {apps.emit(&device,&id,json!({"type":"chat.delta","id":id,"text":text}));}
                        break result.unwrap_or("error");
                    }
                }
            };
            apps.emit(
                &device,
                &id,
                json!({"type":"chat.done","id":id,"finish_reason":reason}),
            );
        });
        Ok(())
    }
    async fn generate(
        request: Value,
        policy: TokenPolicy,
        tx: mpsc::Sender<String>,
    ) -> Result<&'static str, String> {
        let scheduler = crate::gateway::scheduler().ok_or(ERR_PROTOCOL)?;
        let def = scheduler
            .resolve(request["model"].as_str())
            .await
            .map_err(|_| ERR_PROTOCOL)?;
        // Definition ids also resolve; authorize the resolved alias before loading anything.
        if !policy.allows_model(&def.alias) {
            return Err(ERR_PROTOCOL.into());
        }
        let lease = scheduler.acquire(&def).await.map_err(|_| ERR_PROTOCOL)?;
        let mut body = json!({"model":def.alias,"messages":request["messages"],"stream":true});
        if let Some(params) = request["params"].as_object() {
            for key in ["temperature", "top_p", "max_tokens", "seed"] {
                if let Some(v) = params.get(key) {
                    body[key] = v.clone();
                }
            }
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ERR_PROTOCOL)?;
        let mut req = http
            .post(format!("{}/v1/chat/completions", lease.upstream))
            .json(&body);
        if let Some(key) = &lease.api_key {
            req = req.bearer_auth(key);
        }
        let response = req
            .send()
            .await
            .map_err(|_| ERR_PROTOCOL)?
            .error_for_status()
            .map_err(|_| ERR_PROTOCOL)?;
        let mut stream = response.bytes_stream();
        let mut pending = Vec::new();
        let mut finish = "stop";
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ERR_PROTOCOL)?;
            if pending.len() + chunk.len() > 1024 * 1024 {
                return Err(ERR_PROTOCOL.into());
            }
            pending.extend_from_slice(&chunk);
            while let Some(n) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = pending.drain(..=n).collect();
                let line = std::str::from_utf8(&line).map_err(|_| ERR_PROTOCOL)?.trim();
                if let Some(data) = line.strip_prefix("data:").map(str::trim) {
                    if data == "[DONE]" {
                        return Ok(finish);
                    }
                    let v: Value = serde_json::from_str(data).map_err(|_| ERR_PROTOCOL)?;
                    if v["choices"][0]["finish_reason"] == "length" {
                        finish = "length";
                    }
                    if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                        let mut part = String::new();
                        for ch in text.chars() {
                            part.push(ch);
                            if part.len() >= 4000 {
                                tx.send(std::mem::take(&mut part))
                                    .await
                                    .map_err(|_| ERR_PROTOCOL)?;
                            }
                        }
                        if !part.is_empty() {
                            tx.send(part).await.map_err(|_| ERR_PROTOCOL)?;
                        }
                    }
                }
            }
        }
        Ok(finish)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(token_id: &str) -> Chat {
        let (cancel, _) = watch::channel(false);
        Chat {
            events: VecDeque::new(),
            bytes: 0,
            next: 0,
            done: None,
            cancel,
            changed: Instant::now(),
            detached: None,
            token_id: token_id.into(),
            fingerprint: [0; 32],
        }
    }

    #[test]
    fn chat_ack_resume_bounds_expiry_and_owner_revoke() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(dir.path());
        let token = tokens.create("Test", &[Scope::Inference]).unwrap();
        let apps = Applications::shared();
        apps.chats
            .lock()
            .unwrap()
            .insert(("device_1".into(), "chat_001".into()), chat(&token.info.id));
        for _ in 0..100 {
            apps.emit(
                "device_1",
                "chat_001",
                json!({"type":"chat.delta","id":"chat_001","text":"x".repeat(4096)}),
            );
        }
        assert!(apps
            .chats
            .lock()
            .unwrap()
            .values()
            .all(|c| c.bytes <= 262144));
        assert!(
            apps.replay("device_1", "chat_001", -1).is_err(),
            "evicted cursor is unavailable"
        );
        assert!(apps.replay("another_device", "chat_001", 90).is_err());
        assert_eq!(apps.replay("device_1", "chat_001", 90).unwrap().len(), 9);
        apps.ack("device_1", "chat_001", 95).unwrap();
        assert!(apps.ack("device_1", "chat_001", 100).is_err());
        assert_eq!(
            apps.replay("device_1", "chat_001", 95).unwrap()[0]["index"],
            96
        );
        apps.detach("device_1");
        apps.chats
            .lock()
            .unwrap()
            .values_mut()
            .for_each(|c| c.detached = Some(Instant::now() - Duration::from_secs(31)));
        apps.cleanup(&tokens);
        assert!(apps.chats.lock().unwrap().is_empty());
        apps.chats
            .lock()
            .unwrap()
            .insert(("device_1".into(), "chat_002".into()), chat(&token.info.id));
        revoke_device("device_1");
        assert!(apps.chats.lock().unwrap().is_empty());
        apps.chats
            .lock()
            .unwrap()
            .insert(("device_1".into(), "chat_003".into()), chat(&token.info.id));
        tokens.revoke(&token.info.id).unwrap();
        assert!(
            apps.chats.lock().unwrap().is_empty(),
            "named-token revocation also drops offline replay state immediately"
        );
    }

    #[test]
    fn chat_record_expiry_is_independent_of_disconnect_and_ack() {
        let apps = Applications::default();
        let mut c = chat("token_01");
        c.next = 2;
        c.bytes = 10;
        c.events.push_back((
            0,
            Instant::now() - Duration::from_secs(31),
            json!({"type":"chat.delta","index":0}),
            5,
        ));
        c.events
            .push_back((1, Instant::now(), json!({"type":"chat.delta","index":1}), 5));
        apps.chats
            .lock()
            .unwrap()
            .insert(("device_1".into(), "chat_001".into()), c);
        assert!(apps.replay("device_1", "chat_001", -1).is_err());
        assert_eq!(apps.replay("device_1", "chat_001", 0).unwrap().len(), 1);
    }
}
