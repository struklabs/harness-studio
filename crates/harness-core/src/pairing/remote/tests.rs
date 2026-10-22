use super::*;
use noise::{Identity, SUITE};
use relay::TicketProvider;

const HASH: [u8; 32] = [0x5a; 32];

struct MockTickets;
impl TicketProvider for MockTickets {
    fn ticket(&self, key: [u8; 32]) -> relay::TicketFuture<'_> {
        Box::pin(async move {
            let now = control_api::now_secs();
            let jti = control_api::hex(&control_api::random_bytes::<16>().unwrap());
            Ok(relay::RelayTicket {
                credential: relay::test_credential(
                    &key,
                    "desktop_01",
                    "EdDSA",
                    &jti,
                    now,
                    now + 60,
                ),
                desktop_id: "desktop_01".into(),
                public_key: key,
                expires_at: now + 60,
            })
        })
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn sas_matches_review_c1_derivation_and_excludes_claim_id() {
    // Independent vectors computed with Python hashlib.blake2s over the review's formulas:
    // commit = H("struklabs/pairing/sas-commit/v1" || lp(nc) || lp(Kc) || lp(h) || lp(name))
    // sas    = H("struklabs/pairing/sas/v1" || lp(nc) || lp(nd) || lp(Kc) || lp(Kd) || lp(h) || lp(name))
    assert_eq!(
        encode(&commitment(&[3; 32], &[5; 32], &HASH, "Browser")),
        "yKkCvUivWLDoBXja2Fb9gY4oentLyx4dI50Pl8jeHDU"
    );
    assert_eq!(
        sas(&[3; 32], &[4; 32], &[5; 32], &[6; 32], &HASH, "Browser"),
        "230994"
    );
    // Every committed input changes the commitment and the SAS; claim_id is not a parameter at all.
    let base = commitment(&[3; 32], &[5; 32], &HASH, "Browser");
    assert_ne!(base, commitment(&[3; 32], &[5; 32], &[0x5b; 32], "Browser"));
    assert_ne!(base, commitment(&[3; 32], &[5; 32], &HASH, "Browser2"));
    assert_ne!(base, commitment(&[3; 32], &[7; 32], &HASH, "Browser"));
    assert_ne!(base, commitment(&[8; 32], &[5; 32], &HASH, "Browser"));
    let s = sas(&[3; 32], &[4; 32], &[5; 32], &[6; 32], &HASH, "Browser");
    assert_ne!(
        s,
        sas(
            &[3; 32],
            &[4; 32],
            &[5; 32],
            &[6; 32],
            &[0x5b; 32],
            "Browser"
        )
    );
    assert_ne!(
        s,
        sas(&[3; 32], &[4; 32], &[5; 32], &[6; 32], &HASH, "Other")
    );
    assert_eq!(fingerprint(&[6; 32]), "frhbwND9t8F1dcZ8C9s4GQ");
}

#[test]
fn reveal_burns_the_claim_when_hash_name_or_key_were_spliced_by_a_relay() {
    // A relay that terminates Noise on both legs produces a different handshake hash per leg,
    // so the commitment made over leg 1 cannot open on leg 2. Name rewrites fail the same way.
    for case in ["hash", "name", "key", "ok"] {
        let (_dir, tokens, hub, d, c) = setup();
        let nonce = [3; 32];
        let code = offer(&hub);
        let sent_hash = if case == "hash" { [0x11; 32] } else { HASH };
        let sent_name = if case == "name" { "Evil" } else { "Browser" };
        let committed_key = if case == "key" { [0x42; 32] } else { c.public };
        let commit = commitment(&nonce, &committed_key, &sent_hash, sent_name);
        let (status, claim) = hub.remote_pair(
            "claim",
            &json!({"code":code,"name":"Browser","commit":encode(&commit)}),
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens,
        );
        assert_eq!(status, 201);
        let reveal = json!({"claim_id":claim["claim_id"],"nc":encode(&nonce)});
        let (status, _) = hub.remote_pair(
            "reveal",
            &reveal,
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens,
        );
        assert_eq!(status == 200, case == "ok", "{case}");
        if case != "ok" {
            // burned: a second reveal cannot succeed either
            assert_ne!(
                hub.remote_pair(
                    "reveal",
                    &reveal,
                    c.public,
                    d.public,
                    &HASH,
                    "desktop_01",
                    "Test",
                    &tokens
                )
                .0,
                200
            );
        }
    }
}

#[test]
fn poll_requires_valid_reveal_from_the_same_static_key_and_tokens_fail_closed_without_a_bound_key()
{
    let (_dir, tokens, hub, d, c) = setup();
    let other = Identity::from_secret([11; 32]).unwrap();
    let nonce = [3; 32];
    let claim_for = |code: String| {
        hub.remote_pair(
            "claim",
            &json!({"code":code,"name":"Browser","commit":encode(&commitment(&nonce,&c.public,&HASH,"Browser"))}),
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens,
        )
        .1
    };
    let claim = claim_for(offer(&hub));
    let body = |op: &str| {
        if op == "reveal" {
            json!({"claim_id":claim["claim_id"],"nc":encode(&nonce)})
        } else {
            json!({"claim_id":claim["claim_id"],"poll_secret":claim["poll_secret"]})
        }
    };
    let call = |op: &str, who: &Identity| {
        hub.remote_pair(
            op,
            &body(op),
            who.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens,
        )
        .0
    };
    // poll before any reveal: not found, even for the claiming key with the right secret
    assert_eq!(call("poll", &c), 410);
    // a reveal from another static key is `pairing.not_found`: the claim belongs to its Noise key
    assert_eq!(call("reveal", &other), 410);
    // ... and it does not burn the claim: only the claimer can open (or fail) the commitment
    assert_eq!(call("reveal", &c), 200);

    // fresh claim: valid reveal, then poll from another key is refused and does not consume the token
    let claim = claim_for(offer(&hub));
    let body = |op: &str| {
        if op == "reveal" {
            json!({"claim_id":claim["claim_id"],"nc":encode(&nonce)})
        } else {
            json!({"claim_id":claim["claim_id"],"poll_secret":claim["poll_secret"]})
        }
    };
    let call = |op: &str, who: &Identity| {
        hub.remote_pair(
            op,
            &body(op),
            who.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens,
        )
        .0
    };
    assert_eq!(call("reveal", &c), 200);
    hub.approve(claim["claim_id"].as_str().unwrap(), &tokens)
        .unwrap();
    assert_eq!(
        call("poll", &other),
        410,
        "another session/key cannot take the token"
    );
    assert_eq!(call("poll", &c), 200, "the claiming key gets the token");
    // Delivery is idempotent until acknowledged: the same poll re-delivers, only the claiming
    // key may acknowledge, and after the ack the desktop no longer holds a copy.
    assert_eq!(call("poll", &c), 200, "redelivery until ack");
    assert_eq!(call("ack", &other), 410);
    assert_eq!(call("ack", &c), 200);
    assert_eq!(call("poll", &c), 410);
    assert_eq!(call("ack", &c), 410);

    // fail closed: a token with no recorded Noise key is unusable over Noise, with any key
    let unbound = tokens.create("Unbound", &[Scope::Read]).unwrap();
    assert!(hub
        .remote_authorize(&unbound.token, &c.public, &tokens)
        .is_err());
    assert!(tokens.verify_noise(&unbound.token, &c.public).is_none());
}

#[test]
fn web_noise_vectors_match_every_message_and_handshake_hash() {
    for fixture in [
        include_str!("fixtures/cacophony-xx.json"),
        include_str!("fixtures/snow-xx.json"),
    ] {
        let data: Value = serde_json::from_str(fixture).unwrap();
        for v in data["vectors"].as_array().unwrap() {
            let bytes = |key: &str| unhex(v[key].as_str().unwrap());
            let is = bytes("init_static");
            let ie = bytes("init_ephemeral");
            let ip = bytes("init_prologue");
            let rs = bytes("resp_static");
            let re = bytes("resp_ephemeral");
            let rp = bytes("resp_prologue");
            let mut i = snow::Builder::new(SUITE.parse().unwrap())
                .local_private_key(&is)
                .unwrap()
                .prologue(&ip)
                .unwrap()
                .fixed_ephemeral_key_for_testing_only(&ie)
                .build_initiator()
                .unwrap();
            let mut r = snow::Builder::new(SUITE.parse().unwrap())
                .local_private_key(&rs)
                .unwrap()
                .prologue(&rp)
                .unwrap()
                .fixed_ephemeral_key_for_testing_only(&re)
                .build_responder()
                .unwrap();
            let messages = v["messages"].as_array().unwrap();
            for (n, m) in messages.iter().take(3).enumerate() {
                let (writer, reader) = if n % 2 == 0 {
                    (&mut i, &mut r)
                } else {
                    (&mut r, &mut i)
                };
                let payload = unhex(m["payload"].as_str().unwrap());
                let mut out = vec![0; 65535];
                let len = writer.write_message(&payload, &mut out).unwrap();
                out.truncate(len);
                assert_eq!(control_api::hex(&out), m["ciphertext"].as_str().unwrap());
                let mut plain = vec![0; 65535];
                let len = reader.read_message(&out, &mut plain).unwrap();
                assert_eq!(&plain[..len], payload);
            }
            if let Some(hash) = v["handshake_hash"].as_str() {
                assert_eq!(control_api::hex(i.get_handshake_hash()), hash);
                assert_eq!(control_api::hex(r.get_handshake_hash()), hash);
            }
            let mut i = i.into_transport_mode().unwrap();
            let mut r = r.into_transport_mode().unwrap();
            for (n, m) in messages.iter().enumerate().skip(3) {
                let (writer, reader) = if n % 2 == 0 {
                    (&mut i, &mut r)
                } else {
                    (&mut r, &mut i)
                };
                let payload = unhex(m["payload"].as_str().unwrap());
                let mut out = vec![0; 65535];
                let len = writer.write_message(&payload, &mut out).unwrap();
                out.truncate(len);
                assert_eq!(control_api::hex(&out), m["ciphertext"].as_str().unwrap());
                let mut plain = vec![0; 65535];
                let len = reader.read_message(&out, &mut plain).unwrap();
                assert_eq!(&plain[..len], payload);
            }
        }
    }
}

fn setup() -> (tempfile::TempDir, TokenStore, Arc<Hub>, Identity, Identity) {
    let dir = tempfile::tempdir().unwrap();
    let tokens = TokenStore::at(dir.path());
    let hub = hub_for(&tokens);
    (
        dir,
        tokens,
        hub,
        Identity::from_secret([7; 32]).unwrap(),
        Identity::from_secret([9; 32]).unwrap(),
    )
}
fn offer(hub: &Hub) -> String {
    hub.create_offer(&OfferSpec {
        role: DeviceRole::Client,
        preset: AccessPreset::ChatOnly,
        models: vec![],
        ttl_secs: None,
        acknowledge_admin: false,
    })
    .unwrap()
    .code
    .replace('-', "")
}

#[test]
fn commitment_prevents_adaptive_nonce_or_key_grinding_and_premature_approval() {
    let (_dir, tokens, hub, d, c) = setup();
    let nonce = [3; 32];
    let code = offer(&hub);
    let (status, claim) = hub.remote_pair(
        "claim",
        &json!({"code":code,"name":"Browser","commit":encode(&commitment(&nonce,&c.public,&HASH,"Browser"))}),
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        &tokens,
    );
    assert_eq!(status, 201);
    assert!(
        claim["expires_at"].as_str().unwrap().ends_with('Z'),
        "the browser datetime schema requires UTC Z timestamps"
    );
    assert!(hub.pending().is_empty());
    assert!(hub
        .approve(claim["claim_id"].as_str().unwrap(), &tokens)
        .is_err());
    // Thousands of adaptive candidates cannot open the fixed commitment.
    for n in 0u32..4096 {
        let mut changed = nonce;
        changed[..4].copy_from_slice(&n.to_be_bytes());
        if changed != nonce {
            assert_ne!(
                commitment(&changed, &c.public, &HASH, "Browser"),
                commitment(&nonce, &c.public, &HASH, "Browser")
            );
        }
    }
    let wrong = json!({"claim_id":claim["claim_id"],"nc":encode(&[4;32])});
    assert_eq!(
        hub.remote_pair(
            "reveal",
            &wrong,
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens
        )
        .0,
        403,
        "pairing.invalid_reveal"
    );
    let mut correct = wrong.clone();
    correct["nc"] = json!(encode(&nonce));
    assert_ne!(
        hub.remote_pair(
            "reveal",
            &correct,
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens
        )
        .0,
        200
    );
    assert!(hub.pending().is_empty());
}

#[test]
fn pairing_binds_tokens_to_noise_key_delivers_once_and_revokes_named_device() {
    let (_dir, tokens, hub, d, c) = setup();
    let nonce = [3; 32];
    let code = offer(&hub);
    let (_, claim) = hub.remote_pair(
        "claim",
        &json!({"code":code,"name":"B".repeat(64),"commit":encode(&commitment(&nonce,&c.public,&HASH,&"B".repeat(64)))}),
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        &tokens,
    );
    let reveal = json!({"claim_id":claim["claim_id"],"nc":encode(&nonce)});
    let poll = json!({"claim_id":claim["claim_id"],"poll_secret":claim["poll_secret"]});
    let (status, result) = hub.remote_pair(
        "reveal",
        &reveal,
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        &tokens,
    );
    assert_eq!(status, 200);
    assert_eq!(result, json!({"status":"revealed"}));
    assert_eq!(
        hub.pending()[0].verification,
        sas(
            &nonce,
            &decode32(claim["nd"].as_str().unwrap()).unwrap(),
            &c.public,
            &d.public,
            &HASH,
            &"B".repeat(64)
        )
    );
    assert_eq!(
        hub.remote_pair(
            "poll",
            &poll,
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens
        )
        .0,
        202
    );
    let device = hub
        .approve(claim["claim_id"].as_str().unwrap(), &tokens)
        .unwrap();
    assert_eq!(device.name.len(), 64);
    assert!(
        hub.poll(
            claim["claim_id"].as_str().unwrap(),
            claim["poll_secret"].as_str().unwrap(),
            &tokens
        )
        .is_err(),
        "legacy HTTP polling cannot deliver a remote token"
    );
    let (status, result) = hub.remote_pair(
        "poll",
        &poll,
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        &tokens,
    );
    assert_eq!(status, 200);
    let token = result["token"].as_str().unwrap();
    assert!(hub.remote_authorize(token, &c.public, &tokens).is_ok());
    assert!(hub.remote_authorize(token, &d.public, &tokens).is_err());
    // Redelivery: the same poll returns the same token until the browser acknowledges it.
    let (status, again) = hub.remote_pair(
        "poll",
        &poll,
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        &tokens,
    );
    assert_eq!((status, again["token"].as_str()), (200, Some(token)));
    assert_eq!(
        hub.remote_pair(
            "ack",
            &poll,
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens
        ),
        (200, json!({"status":"acknowledged"}))
    );
    assert_eq!(
        hub.remote_pair(
            "poll",
            &poll,
            c.public,
            d.public,
            &HASH,
            "desktop_01",
            "Test",
            &tokens
        )
        .0,
        410
    );
    hub.revoke(&device.id, &tokens).unwrap();
    assert!(tokens.verify(token).is_none());
}

#[test]
fn identity_is_stable_and_keychain_failure_does_not_rotate_it() {
    let store = crate::secrets::MemoryStore::default();
    let a = Identity::load(&store).unwrap();
    let b = Identity::load(&store).unwrap();
    assert_eq!(a.public, b.public);
    crate::secrets::SecretStore::set(&store, &crate::secrets::remote_noise_account(), "corrupt")
        .unwrap();
    assert!(Identity::load(&store).is_err());
}

fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = ((17 + payload.len()) as u32).to_be_bytes().to_vec();
    out.push(kind);
    out.extend_from_slice(&[0x11; 16]);
    out.extend_from_slice(payload);
    out
}

fn frame_for(sid: u8, kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = frame(kind, payload);
    out[5..21].copy_from_slice(&[sid; 16]);
    out
}

#[test]
fn channel_replay_and_bad_padding_are_fatal() {
    for bad_padding in [false, true] {
        let identity = Identity::from_secret([7; 32]).unwrap();
        let mut channel = identity
            .responder("app", "11111111111111111111111111111111")
            .unwrap();
        let mut client = snow::Builder::new(SUITE.parse().unwrap())
            .local_private_key(&[9; 32])
            .unwrap()
            .prologue(b"StrukLabs-App/1|app|11111111111111111111111111111111")
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut out = [0; 65535];
        let n = client.write_message(&[], &mut out).unwrap();
        let reply = channel
            .receive(&frame(1, &out[..n]))
            .unwrap()
            .reply
            .unwrap();
        client.read_message(&reply[21..], &mut out).unwrap();
        let n = client.write_message(&[], &mut out).unwrap();
        channel.receive(&frame(1, &out[..n])).unwrap();
        let mut client = client.into_transport_mode().unwrap();
        let mut record = [0u8; 256];
        record[2] = 1;
        record[3] = b'x';
        if bad_padding {
            record[255] = 1;
        }
        let n = client.write_message(&record, &mut out).unwrap();
        let wire = frame(2, &out[..n]);
        if bad_padding {
            assert!(channel.receive(&wire).is_err());
        } else {
            assert_eq!(channel.receive(&wire).unwrap().envelope.unwrap(), b"x");
            assert!(channel.receive(&wire).is_err());
        }
        let n = client.write_message(&[0; 256], &mut out).unwrap();
        assert!(
            channel.receive(&frame(2, &out[..n])).is_err(),
            "fatal errors close the cipher state"
        );
    }
}

#[tokio::test]
#[allow(clippy::result_large_err)] // tungstenite's callback fixes this error type.
async fn desktop_mock_relay_test_client_encrypted_pair_and_revoke_end_to_end() {
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::{accept_async, accept_hdr_async, connect_async, tungstenite::Message};
    tokio::time::timeout(Duration::from_secs(15), async {
        let (_dir,tokens,hub,d,c)=setup(); let code=offer(&hub);
        let mut listener=None;
        for port in 12440..=12499 {if let Ok(bound)=tokio::net::TcpListener::bind(("127.0.0.1",port)).await {listener=Some(bound);break;}}
        let listener=listener.expect("one isolated test port must be available");
        let url=format!("ws://{}",listener.local_addr().unwrap());
        let captures=Arc::new(Mutex::new(Vec::<Vec<u8>>::new())); let capture=captures.clone();
        let (ready_tx,ready_rx)=tokio::sync::oneshot::channel();
        let mock=tokio::spawn(async move {
            let (socket,_)=listener.accept().await.unwrap();
            let mut desktop=accept_hdr_async(socket,|_: &tokio_tungstenite::tungstenite::handshake::server::Request,mut response:tokio_tungstenite::tungstenite::handshake::server::Response| {
                response.headers_mut().insert("Sec-WebSocket-Protocol",relay::SUBPROTOCOL.parse().unwrap()); Ok(response)
            }).await.unwrap();
            let auth=desktop.next().await.unwrap().unwrap().into_text().unwrap();
            let auth:Value=serde_json::from_str(&auth).unwrap(); assert_eq!(auth["role"],"desktop");
            desktop.send(Message::Text(json!({"v":1,"seq":0,"session":"00000000000000000000000000000000","type":"auth.ok","role":"desktop","limits":{"max_frame_bytes":65556,"idle_timeout_ms":60000,"ping_interval_ms":20000}}).to_string().into())).await.unwrap();
            // Foreign-session traffic on the multiplexed connection: must be ignored, never fatal.
            desktop.send(Message::Binary(frame_for(0x44,2,&[0;96]).into())).await.unwrap();
            desktop.send(Message::Binary(frame_for(0x44,1,&[0;32]).into())).await.unwrap();
            desktop.send(Message::Text(json!({"v":1,"seq":1,"session":"44444444444444444444444444444444","type":"transport.fallback","reason":"policy"}).to_string().into())).await.unwrap();
            desktop.send(Message::Text(json!({"v":1,"seq":2,"session":"44444444444444444444444444444444","type":"peer.left","reason":"left"}).to_string().into())).await.unwrap();
            ready_tx.send(()).unwrap();
            for (offset,mode,sid) in [(3,"pair","11111111111111111111111111111111"),(6,"app","22222222222222222222222222222222"),(9,"app","33333333333333333333333333333333")] {
            let (socket,_)=listener.accept().await.unwrap(); let mut browser=accept_async(socket).await.unwrap();
            desktop.send(Message::Text(json!({"v":1,"seq":offset,"session":sid,"type":"session.incoming","mode":mode,"peer":"browser_01"}).to_string().into())).await.unwrap();
            desktop.send(Message::Text(json!({"v":1,"seq":offset+1,"session":sid,"type":"transport.fallback","reason":"policy"}).to_string().into())).await.unwrap();
            loop {
                tokio::select! {
                    msg=browser.next()=>match msg {Some(Ok(Message::Binary(bytes)))=>{capture.lock().unwrap().push(bytes.to_vec());desktop.send(Message::Binary(bytes)).await.unwrap();},_=>break},
                    msg=desktop.next()=>match msg {Some(Ok(Message::Binary(bytes)))=>{capture.lock().unwrap().push(bytes.to_vec());browser.send(Message::Binary(bytes)).await.unwrap();},Some(Ok(Message::Text(_)))=>{},_=>break}
                }
            }
            desktop.send(Message::Text(json!({"v":1,"seq":offset+2,"session":sid,"type":"peer.left","reason":"left"}).to_string().into())).await.unwrap();
            }
        });
        let desktop_url=url.clone(); let desktop_tokens=tokens.clone();
        let desktop=tokio::spawn(async move {
            relay::connect_url(&desktop_url,MockTickets.ticket(d.public).await.unwrap(),&d,desktop_tokens,"Test Desktop").await
        });
        ready_rx.await.unwrap();
        let (mut ws,_)=connect_async(&url).await.unwrap();
        let prologue=b"StrukLabs-App/1|pair|11111111111111111111111111111111";
        let mut client=snow::Builder::new(SUITE.parse().unwrap()).local_private_key(&[9;32]).unwrap().prologue(prologue).unwrap().build_initiator().unwrap();
        let mut out=[0;65535];let n=client.write_message(&[],&mut out).unwrap(); ws.send(Message::Binary(frame(1,&out[..n]).into())).await.unwrap();
        let response=ws.next().await.unwrap().unwrap().into_data(); client.read_message(&response[21..],&mut out).unwrap();
        let n=client.write_message(&[],&mut out).unwrap(); ws.send(Message::Binary(frame(1,&out[..n]).into())).await.unwrap();
        let hash=client.get_handshake_hash().to_vec(); let mut transport=client.into_transport_mode().unwrap();
        let nonce=[3;32];
        async fn request(ws:&mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,t:&mut snow::TransportState,sid:u8,v:Value)->Value {
            let data=serde_json::to_vec(&v).unwrap(); let mut record=vec![0;(data.len()+3).div_ceil(256)*256];record[1..3].copy_from_slice(&(data.len() as u16).to_be_bytes());record[3..3+data.len()].copy_from_slice(&data);
            let mut out=vec![0;65535];let n=t.write_message(&record,&mut out).unwrap();ws.send(Message::Binary(frame_for(sid,2,&out[..n]).into())).await.unwrap();
            let bytes=ws.next().await.unwrap().unwrap().into_data();let n=t.read_message(&bytes[21..],&mut out).unwrap();assert!(n>=256); let len=u16::from_be_bytes([out[1],out[2]]) as usize;serde_json::from_slice(&out[3..3+len]).unwrap()
        }
        let claim=request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":0,"type":"pair.request","id":"req_0001","op":"claim","body":{"code":code,"name":"Browser","commit":encode(&commitment(&nonce,&c.public,&hash,"Browser"))}})).await;
        assert_eq!(claim["status"],201);let body=&claim["body"];
        let reveal=json!({"claim_id":body["claim_id"],"nc":encode(&nonce)});
        let poll=json!({"claim_id":body["claim_id"],"poll_secret":body["poll_secret"]});
        assert_eq!(request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":1,"type":"pair.request","id":"req_0002","op":"reveal","body":reveal})).await["status"],200);
        hub.approve(body["claim_id"].as_str().unwrap(),&tokens).unwrap();
        let approved=request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":2,"type":"pair.request","id":"req_0003","op":"poll","body":poll})).await;
        let token=approved["body"]["token"].as_str().unwrap();
        assert_eq!(request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":3,"type":"pair.request","id":"req_0004","op":"ack","body":poll})).await["status"],200); assert!(hub.remote_authorize(token,&c.public,&tokens).is_ok());
        assert!(tokens.verify(token).is_none(),"Noise-bound tokens cannot use ordinary bearer routes");
        for wire in captures.lock().unwrap().iter() { for secret in [token,code.as_str(),"pair.request","Browser"] { assert!(!wire.windows(secret.len()).any(|w|w==secret.as_bytes())); } }
        ws.close(None).await.unwrap();
        // A rejected app session must not take the desktop relay connection offline.
        let (mut rejected,_)=connect_async(&url).await.unwrap();
        let mut bad=snow::Builder::new(SUITE.parse().unwrap()).local_private_key(&[9;32]).unwrap().prologue(b"StrukLabs-App/1|app|22222222222222222222222222222222").unwrap().build_initiator().unwrap();
        let n=bad.write_message(&[],&mut out).unwrap();rejected.send(Message::Binary(frame_for(0x22,1,&out[..n]).into())).await.unwrap();let response=rejected.next().await.unwrap().unwrap().into_data();bad.read_message(&response[21..],&mut out).unwrap();let n=bad.write_message(&[],&mut out).unwrap();rejected.send(Message::Binary(frame_for(0x22,1,&out[..n]).into())).await.unwrap();let mut bad=bad.into_transport_mode().unwrap();
        assert_eq!(request(&mut rejected,&mut bad,0x22,json!({"v":1,"seq":0,"type":"hello","from":"client","token":"invalid-test-token","client":{"name":"Test","version":"1"},"caps":[]})).await["code"],"auth.invalid_token");
        rejected.close(None).await.unwrap();
        // Authenticate a new app session with the same key, then revoke only that token.
        let (mut ws,_)=connect_async(&url).await.unwrap();
        let mut client=snow::Builder::new(SUITE.parse().unwrap()).local_private_key(&[9;32]).unwrap().prologue(b"StrukLabs-App/1|app|33333333333333333333333333333333").unwrap().build_initiator().unwrap();
        let n=client.write_message(&[],&mut out).unwrap();ws.send(Message::Binary(frame_for(0x33,1,&out[..n]).into())).await.unwrap();let response=ws.next().await.unwrap().unwrap().into_data();client.read_message(&response[21..],&mut out).unwrap();let n=client.write_message(&[],&mut out).unwrap();ws.send(Message::Binary(frame_for(0x33,1,&out[..n]).into())).await.unwrap();let mut client=client.into_transport_mode().unwrap();
        assert_eq!(request(&mut ws,&mut client,0x33,json!({"v":1,"seq":0,"type":"hello","from":"client","token":token,"client":{"name":"Test","version":"1"},"caps":[]})).await["type"],"hello");
        let unrelated=tokens.create("Unrelated",&[Scope::Read]).unwrap();
        assert_eq!(request(&mut ws,&mut client,0x33,json!({"v":1,"seq":1,"type":"device.revoke","id":"revoke_01"})).await["type"],"device.revoked");
        assert!(tokens.verify_noise(token,&c.public).is_none());assert!(tokens.verify(&unrelated.token).is_some());
        ws.close(None).await.unwrap();mock.await.unwrap();desktop.abort();
    }).await.unwrap();
}

#[test]
fn frame_checks_are_direction_aware_and_strict() {
    let sid = "11111111111111111111111111111111";
    let new_channel = || {
        Identity::from_secret([7; 32])
            .unwrap()
            .responder("app", sid)
            .unwrap()
    };
    let initiator = || {
        snow::Builder::new(SUITE.parse().unwrap())
            .local_private_key(&[9; 32])
            .unwrap()
            .prologue(format!("StrukLabs-App/1|app|{sid}").as_bytes())
            .unwrap()
            .build_initiator()
            .unwrap()
    };
    // The desktop is the responder: it never accepts a 96-byte "message 2" or a 64-byte
    // "message 3" as the first frame, only the 32-byte message 1.
    for len in [0usize, 31, 33, 64, 96] {
        assert!(
            new_channel().receive(&frame(1, &vec![0; len])).is_err(),
            "first frame len {len}"
        );
    }
    assert!(
        new_channel().receive(&frame(2, &[0; 32])).is_err(),
        "transport kind during handshake"
    );
    // After message 1 the only valid next frame is 64 bytes; 96 and 32 are rejected and close it.
    let mut out = [0; 65535];
    for bad in [32usize, 96] {
        let mut ch = new_channel();
        let mut i = initiator();
        let n = i.write_message(&[], &mut out).unwrap();
        assert_eq!(n, 32);
        let reply = ch.receive(&frame(1, &out[..n])).unwrap().reply.unwrap();
        assert_eq!(reply.len(), 21 + 96);
        assert!(
            ch.receive(&frame(1, &vec![0; bad])).is_err(),
            "message 3 len {bad}"
        );
        assert!(
            ch.receive(&frame(1, &out[..32])).is_err(),
            "closed after a fatal frame"
        );
    }
    // A frame for another session id is fatal for this channel.
    let mut ch = new_channel();
    let mut i = initiator();
    let n = i.write_message(&[], &mut out).unwrap();
    assert!(ch.receive(&frame_for(0x22, 1, &out[..n])).is_err());
    // The length prefix must match and oversize frames are rejected.
    let mut ch = new_channel();
    let mut i = initiator();
    let n = i.write_message(&[], &mut out).unwrap();
    let mut bad = frame(1, &out[..n]);
    bad[3] ^= 1;
    assert!(ch.receive(&bad).is_err());
    assert!(new_channel().receive(&vec![0; 65557]).is_err());
}

// --- Cross-implementation vectors: generated by running struklabs-web `packages/app-protocol`
// (commit recorded in fixtures/web-interop.json) and reproduced here byte for byte. ---

fn web_vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/web-interop.json")).unwrap()
}

fn arr32(v: &Value, key: &str) -> [u8; 32] {
    unhex(v[key].as_str().unwrap()).try_into().unwrap()
}

/// sha256 counter stream: the same deterministic bytes the web generator produced.
fn stream(label: &str, length: usize) -> Vec<u8> {
    use sha2::{Digest as _, Sha256};
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < length {
        out.extend_from_slice(&Sha256::digest(format!("{label}/{i}").as_bytes()));
        i += 1;
    }
    out.truncate(length);
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    control_api::hex(&Sha256::digest(bytes))
}

#[test]
fn interop_fixture_records_its_source_commit() {
    let v = web_vectors();
    assert_eq!(v["source"]["commit"].as_str().unwrap().len(), 40);
    assert!(v["source"]["repository"]
        .as_str()
        .unwrap()
        .ends_with("struklabs-web"));
}

#[test]
fn interop_sas_commit_fingerprint_and_poll_digest_match_the_web_package() {
    let v = web_vectors();
    let list = v["sas"].as_array().unwrap();
    assert!(list.len() >= 10);
    for s in list {
        let name = s["name"].as_str().unwrap();
        let (nc, nd, kc, kd, h) = (
            arr32(s, "nc"),
            arr32(s, "nd"),
            arr32(s, "kc"),
            arr32(s, "kd"),
            arr32(s, "h"),
        );
        assert_eq!(
            encode(&commitment(&nc, &kc, &h, name)),
            s["commit"].as_str().unwrap(),
            "commit for {name}"
        );
        assert_eq!(
            sas(&nc, &nd, &kc, &kd, &h, name),
            s["sas"].as_str().unwrap(),
            "sas for {name}"
        );
        assert_eq!(fingerprint(&kd), s["fingerprint"].as_str().unwrap());
        assert_eq!(
            poll_digest(
                s["claim_id"].as_str().unwrap(),
                s["poll_secret"].as_str().unwrap()
            ),
            s["poll_digest"].as_str().unwrap()
        );
    }
}

#[test]
fn interop_frames_encode_decode_and_reject_like_the_web_package() {
    use frames::{decode_frame, encode_frame, Refusal};
    let v = web_vectors();
    for f in v["frames"].as_array().unwrap() {
        let sid: [u8; 16] = unhex(f["session"].as_str().unwrap()).try_into().unwrap();
        let kind = f["kind"].as_u64().unwrap() as u8;
        let len = f["noise_len"].as_u64().unwrap() as usize;
        // The generator labels the noise bytes `frame<n>`; recover n from the digest.
        let label = (0..6)
            .map(|n| format!("frame{n}"))
            .find(|l| sha256_hex(&stream(l, len)) == f["noise_sha256"].as_str().unwrap())
            .expect("deterministic noise bytes");
        let noise = stream(&label, len);
        let bytes = encode_frame(kind, &sid, &noise).unwrap();
        assert_eq!(bytes.len() as u64, f["frame_len"].as_u64().unwrap());
        assert_eq!(sha256_hex(&bytes), f["frame_sha256"].as_str().unwrap());
        if let Some(hex) = f["frame_hex"].as_str() {
            assert_eq!(control_api::hex(&bytes), hex);
        }
        let (k, s, n) = decode_frame(&bytes).unwrap();
        assert_eq!((k, s, n), (kind, sid, &noise[..]));
    }
    for r in v["frame_rejects"].as_array().unwrap() {
        let bytes = unhex(r["frame_hex"].as_str().unwrap());
        let err = decode_frame(&bytes).expect_err(r["name"].as_str().unwrap());
        if r["web_code"] == "too_large" {
            assert_eq!(err, Refusal::FrameTooLarge);
        }
    }
    // One byte over the largest transport message is refused on both sides.
    assert!(encode_frame(2, &[0; 16], &vec![0; frames::MAX_TRANSPORT_MESSAGE + 1]).is_err());
    assert_eq!(
        frames::MAX_FRAME_BODY,
        v["source"]["limits"]["frameBodyBytes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        frames::MAX_TRANSPORT_MESSAGE,
        v["source"]["limits"]["transportMessageBytes"]
            .as_u64()
            .unwrap() as usize
    );
}

#[test]
fn interop_records_pad_to_256_byte_buckets_and_reassemble_like_the_web_package() {
    use frames::{split_into_records, RecordAssembler};
    let v = web_vectors();
    let limit = |k: &str| v["source"]["limits"][k].as_u64().unwrap() as usize;
    assert_eq!(frames::RECORD_PAD, limit("recordPadBytes"));
    assert_eq!(frames::RECORD_DATA_MAX, limit("recordDataBytes"));
    assert_eq!(frames::MAX_FRAGMENTS, limit("maxRecordFragments"));
    for m in v["records"].as_array().unwrap() {
        let message = stream(
            m["message_label"].as_str().unwrap(),
            m["message_len"].as_u64().unwrap() as usize,
        );
        assert_eq!(sha256_hex(&message), m["message_sha256"].as_str().unwrap());
        let records = split_into_records(&message);
        let expected = m["records"].as_array().unwrap();
        assert_eq!(records.len(), expected.len(), "len {}", message.len());
        let mut assembler = RecordAssembler::default();
        let mut back = None;
        for (record, e) in records.iter().zip(expected) {
            assert_eq!(record.len() % 256, 0);
            assert_eq!(record.len() as u64, e["len"].as_u64().unwrap());
            assert_eq!(sha256_hex(record), e["sha256"].as_str().unwrap());
            if let Some(hex) = e["hex"].as_str() {
                assert_eq!(control_api::hex(record), hex);
            }
            back = assembler.push(record, noise::MAX_ENVELOPE).unwrap();
        }
        assert_eq!(back.unwrap(), message);
    }
    for r in v["record_rejects"].as_array().unwrap() {
        let name = r["name"].as_str().unwrap();
        let mut assembler = RecordAssembler::default();
        if let Some(list) = r["records_hex"].as_array() {
            let rejected = list.iter().any(|h| {
                assembler
                    .push(&unhex(h.as_str().unwrap()), noise::MAX_ENVELOPE)
                    .is_err()
            });
            assert!(rejected, "{name}");
        } else {
            // The web package rejects the same push index: the fragment cap is 128.
            let record = unhex(r["repeat_record_hex"].as_str().unwrap());
            let first_err = (0..200)
                .position(|_| assembler.push(&record, noise::MAX_ENVELOPE).is_err())
                .expect(name);
            assert_eq!(first_err as u64, r["reject_at_index"].as_u64().unwrap());
        }
    }
}

#[test]
fn interop_relay_frame_check_matches_the_web_package_on_every_grid_point() {
    use frames::{validate_relay_frame, Direction};
    let v = web_vectors();
    let grid = v["relay_checks"].as_array().unwrap();
    assert!(grid.len() >= 1000);
    for c in grid {
        let direction = match c[3].as_str() {
            Some("browser->desktop") => Some(Direction::BrowserToDesktop),
            Some("desktop->browser") => Some(Direction::DesktopToBrowser),
            _ => None,
        };
        let got = match validate_relay_frame(
            c[0].as_u64().unwrap() as u8,
            c[1].as_u64().unwrap() as usize,
            c[2].as_u64().unwrap() as usize,
            direction,
        ) {
            Ok(()) => "ok",
            Err(r) => r.code(),
        };
        assert_eq!(got, c[4].as_str().unwrap(), "{c}");
    }
}

fn open_channel(identity: &Identity, sid: &str) -> (noise::Channel, snow::TransportState) {
    let mut channel = identity.responder("app", sid).unwrap();
    let mut client = snow::Builder::new(SUITE.parse().unwrap())
        .local_private_key(&[9; 32])
        .unwrap()
        .prologue(format!("StrukLabs-App/1|app|{sid}").as_bytes())
        .unwrap()
        .build_initiator()
        .unwrap();
    let mut out = [0; 65535];
    let n = client.write_message(&[], &mut out).unwrap();
    let reply = channel
        .receive(&frame(1, &out[..n]))
        .unwrap()
        .reply
        .unwrap();
    client.read_message(&reply[21..], &mut out).unwrap();
    let n = client.write_message(&[], &mut out).unwrap();
    channel.receive(&frame(1, &out[..n])).unwrap();
    (channel, client.into_transport_mode().unwrap())
}

#[test]
fn interop_channel_refuses_old_buckets_and_sends_whole_256_byte_buckets() {
    let identity = Identity::from_secret([7; 32]).unwrap();
    let sid = "11111111111111111111111111111111";
    let mut out = [0; 65535];
    // A 64-byte (old bucket) transport record is not a valid message any more.
    let (mut channel, mut client) = open_channel(&identity, sid);
    let n = client.write_message(&[0u8; 64], &mut out).unwrap();
    assert!(channel.receive(&frame(2, &out[..n])).is_err());
    // Everything the desktop sends is a whole number of 256-byte buckets plus the tag.
    let (mut channel, mut client) = open_channel(&identity, sid);
    for size in [1usize, 253, 254, 16384, 16385, 40000] {
        for wire in channel.send(&vec![b'a'; size]).unwrap() {
            let noise = &wire[21..];
            assert!(noise.len() <= frames::MAX_TRANSPORT_MESSAGE);
            assert_eq!((noise.len() - 16) % 256, 0);
            assert!(frames::validate_relay_frame(
                2,
                noise.len(),
                3,
                Some(frames::Direction::DesktopToBrowser)
            )
            .is_ok());
            client.read_message(noise, &mut out).unwrap();
        }
    }
}

// --- Wire fields, acknowledgement and redelivery windows ---

fn claim_via(hub: &Hub, tokens: &TokenStore, d: &Identity, c: &Identity, nonce: [u8; 32]) -> Value {
    let code = offer(hub);
    let (status, claim) = hub.remote_pair(
        "claim",
        &json!({"code":code,"name":"Browser","commit":encode(&commitment(&nonce,&c.public,&HASH,"Browser"))}),
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        tokens,
    );
    assert_eq!(status, 201);
    claim
}

fn pair_call(
    hub: &Hub,
    tokens: &TokenStore,
    d: &Identity,
    c: &Identity,
    op: &str,
    body: &Value,
) -> (u16, Value) {
    hub.remote_pair(
        op,
        body,
        c.public,
        d.public,
        &HASH,
        "desktop_01",
        "Test",
        tokens,
    )
}

#[test]
fn claim_answer_uses_the_agreed_wire_fields_and_no_key_or_legacy_names() {
    let (_dir, tokens, hub, d, c) = setup();
    let claim = claim_via(&hub, &tokens, &d, &c, [3; 32]);
    let mut keys: Vec<&str> = claim
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "claim_id",
            "expires_at",
            "nd",
            "poll_interval_ms",
            "poll_secret"
        ]
    );
    assert_eq!(decode32(claim["nd"].as_str().unwrap()).unwrap().len(), 32);
    assert_eq!(claim["poll_interval_ms"], 2000);
    // Revealed: `{status:"revealed"}` only, with no desktop-computed code on the wire.
    let reveal = json!({"claim_id":claim["claim_id"],"nc":encode(&[3;32])});
    assert_eq!(
        pair_call(&hub, &tokens, &d, &c, "reveal", &reveal),
        (200, json!({"status":"revealed"}))
    );
}

#[test]
fn key_fields_in_any_body_are_refused_and_never_used() {
    let (_dir, tokens, hub, d, c) = setup();
    let nonce = [3; 32];
    let code = offer(&hub);
    let commit = encode(&commitment(&nonce, &c.public, &HASH, "Browser"));
    for key in [
        "static_pub_c",
        "static_pub_d",
        "noise_static_pub",
        "kc",
        "client_static",
    ] {
        let mut body = json!({"code":code,"name":"Browser","commit":commit});
        body[key] = json!(encode(&c.public));
        let (status, answer) = pair_call(&hub, &tokens, &d, &c, "claim", &body);
        let key_error = answer["error"]["code"].as_str();
        assert_eq!(
            (status, key_error),
            (400, Some("pairing.key_in_body")),
            "{key}"
        );
    }
    // The refusals consumed nothing: the same offer still claims normally.
    let (status, claim) = pair_call(
        &hub,
        &tokens,
        &d,
        &c,
        "claim",
        &json!({"code":code,"name":"Browser","commit":commit}),
    );
    assert_eq!(status, 201);
    let id = claim["claim_id"].clone();
    for (op, mut body) in [
        ("reveal", json!({"claim_id":id,"nc":encode(&nonce)})),
        (
            "poll",
            json!({"claim_id":id,"poll_secret":claim["poll_secret"]}),
        ),
        (
            "ack",
            json!({"claim_id":id,"poll_secret":claim["poll_secret"]}),
        ),
    ] {
        body["static_pub_c"] = json!(encode(&c.public));
        let (status, answer) = pair_call(&hub, &tokens, &d, &c, op, &body);
        assert_eq!(
            (status, answer["error"]["code"].as_str()),
            (400, Some("pairing.key_in_body")),
            "{op}"
        );
    }
    let reveal = json!({"claim_id":id,"nc":encode(&nonce)});
    assert_eq!(pair_call(&hub, &tokens, &d, &c, "reveal", &reveal).0, 200);
}

#[test]
fn invalid_reveal_is_403_burns_the_claim_and_counts_a_strike() {
    let (_dir, tokens, hub, d, c) = setup();
    let claim = claim_via(&hub, &tokens, &d, &c, [3; 32]);
    let bad = json!({"claim_id":claim["claim_id"],"nc":encode(&[4;32])});
    let (status, answer) = pair_call(&hub, &tokens, &d, &c, "reveal", &bad);
    assert_eq!(
        (status, answer["error"]["code"].as_str()),
        (403, Some("pairing.invalid_reveal"))
    );
    let good = json!({"claim_id":claim["claim_id"],"nc":encode(&[3;32])});
    assert_eq!(
        pair_call(&hub, &tokens, &d, &c, "reveal", &good).0,
        410,
        "claim erased"
    );
    assert_eq!(hub.state.lock().unwrap().failures.len(), 1);
}

#[test]
fn token_redelivery_ack_and_windows_follow_the_spec() {
    let (_dir, tokens, hub, d, c) = setup();
    let setup_claim = || {
        let claim = claim_via(&hub, &tokens, &d, &c, [3; 32]);
        let reveal = json!({"claim_id":claim["claim_id"],"nc":encode(&[3;32])});
        assert_eq!(pair_call(&hub, &tokens, &d, &c, "reveal", &reveal).0, 200);
        let poll = json!({"claim_id":claim["claim_id"],"poll_secret":claim["poll_secret"]});
        (claim["claim_id"].as_str().unwrap().to_string(), poll)
    };
    let poll_status = |poll: &Value| pair_call(&hub, &tokens, &d, &c, "poll", poll).0;
    // 1. Pending claims expire 180 s after the claim (not the 300 s legacy TTL).
    let (_, poll) = setup_claim();
    assert_eq!(poll_status(&poll), 202);
    hub.advance_clock(181);
    assert_eq!(poll_status(&poll), 410);
    // 2. An approved claim waits up to 300 s for its first poll, whatever the claim TTL was.
    let (id, poll) = setup_claim();
    hub.approve(&id, &tokens).unwrap();
    hub.advance_clock(299);
    let (status, first) = pair_call(&hub, &tokens, &d, &c, "poll", &poll);
    assert_eq!(status, 200);
    // 3. Redelivery until ack: the same poll returns the same token for 60 s after the first delivery.
    hub.advance_clock(59);
    let (status, again) = pair_call(&hub, &tokens, &d, &c, "poll", &poll);
    assert_eq!((status, &again["token"]), (200, &first["token"]));
    hub.advance_clock(2);
    assert_eq!(poll_status(&poll), 410, "redelivery window over");
    // 4. An unpolled approved claim is wiped after 300 s.
    let (id, poll) = setup_claim();
    hub.approve(&id, &tokens).unwrap();
    hub.advance_clock(301);
    assert_eq!(poll_status(&poll), 410);
    // 5. ack before any delivery is refused; after delivery it wipes the copy at once.
    let (id, poll) = setup_claim();
    hub.approve(&id, &tokens).unwrap();
    assert_eq!(pair_call(&hub, &tokens, &d, &c, "ack", &poll).0, 410);
    assert_eq!(poll_status(&poll), 200);
    assert_eq!(
        pair_call(&hub, &tokens, &d, &c, "ack", &poll),
        (200, json!({"status":"acknowledged"}))
    );
    assert_eq!(poll_status(&poll), 410);
}

#[test]
fn legacy_http_claims_keep_their_delivered_once_behaviour() {
    let (_dir, tokens, hub, _, _) = setup();
    let code = offer(&hub);
    let started = hub
        .claim(&code, "Phone", "android", "noncenoncenonce01")
        .unwrap();
    hub.approve(&started.claim_id, &tokens).unwrap();
    let p = hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .unwrap();
    assert_eq!(p.status, ClaimState::Approved);
    assert!(hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .is_err());
}

// --- Relay session rules ---

#[test]
fn one_live_session_per_device_the_older_one_is_superseded() {
    let apps = apps::Applications::shared();
    let first = apps.claim_session("device_1");
    assert!(apps.is_current("device_1", first));
    let second = apps.claim_session("device_1");
    assert!(!apps.is_current("device_1", first));
    assert!(apps.is_current("device_1", second));
    assert!(
        !apps.release("device_1", first),
        "a superseded session must not detach"
    );
    assert!(apps.is_current("device_1", second));
    assert!(apps.release("device_1", second));
    assert!(
        apps.claim_session("other") > second,
        "generations only grow"
    );
}

#[test]
fn chat_start_reports_limit_reached_and_attaches_identical_retries() {
    let dir = tempfile::tempdir().unwrap();
    let tokens = TokenStore::at(dir.path());
    let info = tokens.create("Chat", &[Scope::Inference]).unwrap().info;
    let apps = apps::Applications::shared();
    let request = json!({"type":"chat.start","id":"chat_0001","model":"m","messages":[{"role":"user","content":"hi"}]});
    apps.insert_test_chat("device_1", "chat_0001", &info.id, &request);
    // Identical retry attaches (no error) without consuming a slot.
    assert!(apps
        .start("device_1", info.clone(), request.clone())
        .is_ok());
    // The same id for a different request is `chat.busy`.
    let mut other = request.clone();
    other["messages"][0]["content"] = json!("different");
    let err = apps.start("device_1", info.clone(), other).unwrap_err();
    assert_eq!(err["code"], "chat.busy");
    // The concurrency limit is `chat.limit_reached` (the token allows one running chat).
    let mut second = request.clone();
    second["id"] = json!("chat_0002");
    let err = apps.start("device_1", info.clone(), second).unwrap_err();
    assert_eq!(err["code"], "chat.limit_reached");
    // The prompt limit counts UTF-8 bytes: 100 000 three-byte characters exceed 262 144 bytes.
    let mut big = request.clone();
    big["id"] = json!("chat_0003");
    big["messages"][0]["content"] = json!("\u{20ac}".repeat(100_000));
    let err = apps.start("device_9", info, big).unwrap_err();
    assert_eq!(err["code"], "protocol.too_large");
}

#[test]
fn desktop_ticket_answer_cannot_redirect_the_relay_and_request_matches_the_platform() {
    let key = [5; 32];
    let now = control_api::now_secs();
    let answer = |url: &str, exp: u64| {
        json!({"ticket":relay::test_credential(&key,"desktop_01","EdDSA","jti-answer-000001",now,exp),
        "expires_at":chrono::DateTime::from_timestamp(exp as i64,0).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Secs,true),
        "relay_url":url})
    };
    let t =
        relay::RelayTicket::from_response("desktop_01", &key, &answer(relay::RELAY_URL, now + 120))
            .unwrap();
    assert!(t.expires_at > now);
    assert!(relay::RelayTicket::from_response(
        "desktop_01",
        &key,
        &answer("wss://evil.example/v1/app", now + 120)
    )
    .is_err());
    assert_eq!(
        relay::desktop_ticket_request("desktop_01", &key),
        json!({"device_id":"desktop_01","noise_static_pub":encode(&key)})
    );
}

#[test]
fn strict_json_matches_every_web_duplicate_key_case() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/duplicate-keys.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for code in ["protocol.malformed", "not_json"] {
            let result = json_strict::parse(case["text"].as_str().unwrap().as_bytes(), code);
            assert_eq!(
                result.is_ok(),
                case["accept"].as_bool().unwrap(),
                "{}: {code}",
                case["name"]
            );
            if let Err(error) = result {
                assert_eq!(error, code);
            }
        }
    }
}

#[test]
fn remote_offer_supersedes_and_burns_after_fifteen_wrong_codes_across_lockouts() {
    let (_dir, tokens, hub, d, c) = setup();
    crate::store::JsonStore::new(hub.dir.join("remote_access_v1.json"))
        .save(&RemoteAccessSettingsV1 { enabled: true })
        .unwrap();
    let old = offer(&hub);
    let current = offer(&hub);
    assert_eq!(hub.offers().len(), 1);
    assert_ne!(old, current);
    let wrong = json!({"code":"malformed", "name":"Browser", "commit":encode(&[1;32])});
    // A superseded code is wrong too, with no indication that it ever existed.
    let mut superseded = wrong.clone();
    superseded["code"] = json!(old);
    assert_eq!(
        pair_call(&hub, &tokens, &d, &c, "claim", &superseded).0,
        403
    );
    for attempt in 1..15 {
        if attempt % 5 == 0 {
            let mut correct = wrong.clone();
            correct["code"] = json!(current);
            assert_eq!(pair_call(&hub, &tokens, &d, &c, "claim", &correct).0, 429);
            hub.advance_clock(61);
        }
        if attempt % 2 == 0 {
            assert_eq!(
                hub.claim("malformed", "Browser", "web", "noncenoncenonce01")
                    .unwrap_err(),
                ERR_INVALID_CODE
            );
        } else {
            assert_eq!(pair_call(&hub, &tokens, &d, &c, "claim", &wrong).0, 403);
        }
        assert_eq!(hub.offers().len(), usize::from(attempt < 14));
    }
    hub.advance_clock(61);
    let mut burned = wrong;
    burned["code"] = json!(current);
    assert_eq!(pair_call(&hub, &tokens, &d, &c, "claim", &burned).0, 403);
    let fresh = offer(&hub);
    burned["code"] = json!(fresh);
    assert_eq!(pair_call(&hub, &tokens, &d, &c, "claim", &burned).0, 201);
}

#[test]
fn four_chat_ceiling_counts_all_runs_for_the_same_token() {
    let dir = tempfile::tempdir().unwrap();
    let tokens = TokenStore::at(dir.path());
    let mut info = tokens.create("Chat", &[Scope::Inference]).unwrap().info;
    info.policy.limits.max_concurrent = Some(8);
    let apps = apps::Applications::shared();
    let request = json!({"type":"chat.start","id":"chat_next","model":"m","messages":[{"role":"user","content":"hi"}]});
    for i in 0..4 {
        apps.insert_test_chat(
            &format!("device_{i}"),
            &format!("chat_{i:04}"),
            &info.id,
            &request,
        );
    }
    assert_eq!(
        apps.start("device_5", info, request).unwrap_err()["code"],
        "chat.limit_reached"
    );
}
