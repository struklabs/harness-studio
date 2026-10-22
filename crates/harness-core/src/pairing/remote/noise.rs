//! Noise channel over whole messages; independent of WSS so WebRTC can supply the same frames.
use super::{decode32, encode, ERR_KEYCHAIN, ERR_PROTOCOL};
use snow::resolvers::{CryptoResolver, DefaultResolver};
use std::time::{Duration, Instant};

pub const SUITE: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
pub const MAX_ENVELOPE: usize = 1024 * 1024;

pub struct Identity {
    secret: [u8; 32],
    pub public: [u8; 32],
}

impl Identity {
    pub fn pairing_link(&self, code: &str, desktop_id: &str) -> Result<String, String> {
        if code.len() != 8
            || !code.bytes().all(|b| b.is_ascii_digit())
            || !(8..=64).contains(&desktop_id.len())
            || !desktop_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ERR_PROTOCOL.into());
        }
        Ok(format!(
            "https://app.struklabs.com/devices/pair#code={code}&desktop={desktop_id}&fp={}",
            super::fingerprint(&self.public)
        ))
    }
    /// Fail closed if the keychain is locked/corrupt; never silently rotate a pinned key.
    pub fn load(store: &dyn crate::secrets::SecretStore) -> Result<Self, String> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().map_err(|_| ERR_KEYCHAIN)?;
        let account = crate::secrets::remote_noise_account();
        let secret = match store.get(&account).map_err(|_| ERR_KEYCHAIN)? {
            Some(value) => decode32(&value).map_err(|_| ERR_KEYCHAIN)?,
            None => {
                let secret = crate::control_api::random_bytes::<32>()?;
                store
                    .set(&account, &encode(&secret))
                    .map_err(|_| ERR_KEYCHAIN)?;
                secret
            }
        };
        Self::from_secret(secret)
    }

    pub fn from_secret(secret: [u8; 32]) -> Result<Self, String> {
        let mut dh = DefaultResolver
            .resolve_dh(&snow::params::DHChoice::Curve25519)
            .ok_or(ERR_PROTOCOL)?;
        dh.set(&secret);
        Ok(Self {
            secret,
            public: dh.pubkey().try_into().map_err(|_| ERR_PROTOCOL)?,
        })
    }

    pub fn responder(&self, mode: &str, session: &str) -> Result<Channel, String> {
        if !matches!(mode, "pair" | "app") {
            return Err(ERR_PROTOCOL.into());
        }
        let sid = session_bytes(session)?;
        let prologue = format!("StrukLabs-App/1|{mode}|{session}");
        let state = snow::Builder::new(SUITE.parse().map_err(|_| ERR_PROTOCOL)?)
            .local_private_key(&self.secret)
            .map_err(|_| ERR_PROTOCOL)?
            .prologue(prologue.as_bytes())
            .map_err(|_| ERR_PROTOCOL)?
            .build_responder()
            .map_err(|_| ERR_PROTOCOL)?;
        Ok(Channel {
            sid,
            state: State::Handshake(Box::new(state)),
            started: None,
            step: 0,
            assembled: Vec::new(),
        })
    }
}

fn session_bytes(session: &str) -> Result<[u8; 16], String> {
    if session.len() != 32
        || !session
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ERR_PROTOCOL.into());
    }
    let mut out = [0; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&session[i * 2..i * 2 + 2], 16).map_err(|_| ERR_PROTOCOL)?;
    }
    Ok(out)
}

enum State {
    Handshake(Box<snow::HandshakeState>),
    Transport(Box<snow::TransportState>),
    Closed,
}

pub struct Channel {
    sid: [u8; 16],
    state: State,
    started: Option<Instant>,
    step: u8,
    assembled: Vec<u8>,
}

pub struct Received {
    pub reply: Option<Vec<u8>>,
    pub envelope: Option<Vec<u8>>,
}

impl Channel {
    pub fn handshake_expired(&self) -> bool {
        matches!(self.state, State::Handshake(_))
            && self
                .started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(10))
    }
    pub fn peer(&self) -> Result<[u8; 32], String> {
        match &self.state {
            State::Transport(s) => s
                .get_remote_static()
                .ok_or(ERR_PROTOCOL)?
                .try_into()
                .map_err(|_| ERR_PROTOCOL.into()),
            _ => Err(ERR_PROTOCOL.into()),
        }
    }

    fn frame(&self, kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(21 + payload.len());
        out.extend_from_slice(&((17 + payload.len()) as u32).to_be_bytes());
        out.push(kind);
        out.extend_from_slice(&self.sid);
        out.extend_from_slice(payload);
        out
    }

    pub fn receive(&mut self, frame: &[u8]) -> Result<Received, String> {
        let result = self.receive_inner(frame);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn receive_inner(&mut self, frame: &[u8]) -> Result<Received, String> {
        if frame.len() < 21
            || frame.len() > 65556
            || u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize != frame.len() - 4
            || frame[5..21] != self.sid
        {
            return Err(ERR_PROTOCOL.into());
        }
        let payload = &frame[21..];
        match &mut self.state {
            State::Handshake(hs) => {
                if self.started.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(10)
                    || frame[4] != 1
                    || payload.len() != if self.step == 0 { 32 } else { 64 }
                {
                    return Err(ERR_PROTOCOL.into());
                }
                let mut plain = [0; 65535];
                if hs
                    .read_message(payload, &mut plain)
                    .map_err(|_| ERR_PROTOCOL)?
                    != 0
                {
                    return Err(ERR_PROTOCOL.into());
                }
                if self.step == 0 {
                    let mut out = [0; 96];
                    let n = hs.write_message(&[], &mut out).map_err(|_| ERR_PROTOCOL)?;
                    self.step = 2;
                    Ok(Received {
                        reply: Some(self.frame(1, &out[..n])),
                        envelope: None,
                    })
                } else {
                    let State::Handshake(hs) = std::mem::replace(&mut self.state, State::Closed)
                    else {
                        unreachable!()
                    };
                    self.state = State::Transport(Box::new(
                        hs.into_transport_mode().map_err(|_| ERR_PROTOCOL)?,
                    ));
                    Ok(Received {
                        reply: None,
                        envelope: None,
                    })
                }
            }
            State::Transport(t) => {
                if frame[4] != 2 || payload.len() < 80 {
                    return Err(ERR_PROTOCOL.into());
                }
                let mut record = vec![0; 65535];
                let n = t
                    .read_message(payload, &mut record)
                    .map_err(|_| ERR_PROTOCOL)?;
                record.truncate(n);
                if n < 64 || n % 64 != 0 || record[0] > 1 {
                    return Err(ERR_PROTOCOL.into());
                }
                let len = u16::from_be_bytes([record[1], record[2]]) as usize;
                if len > 16384
                    || 3 + len > n
                    || record[3 + len..].iter().any(|b| *b != 0)
                    || self.assembled.len() + len > MAX_ENVELOPE
                {
                    return Err(ERR_PROTOCOL.into());
                }
                self.assembled.extend_from_slice(&record[3..3 + len]);
                Ok(Received {
                    reply: None,
                    envelope: if record[0] == 0 {
                        Some(std::mem::take(&mut self.assembled))
                    } else {
                        None
                    },
                })
            }
            State::Closed => Err(ERR_PROTOCOL.into()),
        }
    }

    pub fn send(&mut self, envelope: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        if envelope.is_empty() || envelope.len() > MAX_ENVELOPE {
            return Err(ERR_PROTOCOL.into());
        }
        let result = self.send_inner(envelope);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn send_inner(&mut self, envelope: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut frames = Vec::new();
        let count = envelope.len().div_ceil(16384);
        for (i, data) in envelope.chunks(16384).enumerate() {
            let mut record = vec![0; (3 + data.len()).div_ceil(64) * 64];
            record[0] = u8::from(i + 1 < count);
            record[1..3].copy_from_slice(&(data.len() as u16).to_be_bytes());
            record[3..3 + data.len()].copy_from_slice(data);
            let mut cipher = vec![0; record.len() + 16];
            let State::Transport(t) = &mut self.state else {
                return Err(ERR_PROTOCOL.into());
            };
            let n = t
                .write_message(&record, &mut cipher)
                .map_err(|_| ERR_PROTOCOL)?;
            frames.push(self.frame(2, &cipher[..n]));
        }
        Ok(frames)
    }

    pub fn close(&mut self) {
        self.state = State::Closed;
        self.assembled.clear();
    }
}
