use super::*;
use noise::{Identity, SUITE};
use relay::TicketProvider;

struct MockTickets;
impl TicketProvider for MockTickets {
    fn ticket(&self, key: [u8; 32]) -> relay::TicketFuture<'_> {
        Box::pin(async move {
            Ok(relay::RelayTicket {
                credential: "mock-ticket-no-production-value".into(),
                desktop_id: "desktop_01".into(),
                public_key: key,
                expires_at: control_api::now_secs() + 60,
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
fn sas_commitment_and_fingerprint_match_web_noble_primitives() {
    // Computed with the read-only browser package's pinned @noble/hashes/blake2.js.
    assert_eq!(
        sas("claim_0001", &[3; 32], &[4; 32], &[5; 32], &[6; 32]),
        "222245"
    );
    assert_eq!(
        encode(&commitment(&[3; 32], &[5; 32])),
        "1spWyxt4F9IBzlxa3WdpxA_V1gM2cUq0ZXKgq6T95dE"
    );
    assert_eq!(fingerprint(&[6; 32]), "frhbwND9t8F1dcZ8C9s4GQ");
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
        &json!({"code":code,"name":"Browser","commit":encode(&commitment(&nonce,&c.public))}),
        c.public,
        d.public,
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
                commitment(&changed, &c.public),
                commitment(&nonce, &c.public)
            );
        }
    }
    let wrong = json!({"claim_id":claim["claim_id"],"poll_secret":claim["poll_secret"],"nonce_c":encode(&[4;32]),"static_pub_c":encode(&c.public)});
    assert_eq!(
        hub.remote_pair(
            "reveal",
            &wrong,
            c.public,
            d.public,
            "desktop_01",
            "Test",
            &tokens
        )
        .0,
        400
    );
    let mut correct = wrong.clone();
    correct["nonce_c"] = json!(encode(&nonce));
    assert_ne!(
        hub.remote_pair(
            "reveal",
            &correct,
            c.public,
            d.public,
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
        &json!({"code":code,"name":"B".repeat(64),"commit":encode(&commitment(&nonce,&c.public))}),
        c.public,
        d.public,
        "desktop_01",
        "Test",
        &tokens,
    );
    let reveal = json!({"claim_id":claim["claim_id"],"poll_secret":claim["poll_secret"],"nonce_c":encode(&nonce),"static_pub_c":encode(&c.public)});
    let (status, result) = hub.remote_pair(
        "reveal",
        &reveal,
        c.public,
        d.public,
        "desktop_01",
        "Test",
        &tokens,
    );
    assert_eq!(status, 200);
    assert_eq!(
        result["verification_code"],
        sas(
            claim["claim_id"].as_str().unwrap(),
            &nonce,
            &decode32(claim["nonce_d"].as_str().unwrap()).unwrap(),
            &c.public,
            &d.public
        )
    );
    assert_eq!(
        hub.remote_pair(
            "poll",
            &reveal,
            c.public,
            d.public,
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
        &reveal,
        c.public,
        d.public,
        "desktop_01",
        "Test",
        &tokens,
    );
    assert_eq!(status, 200);
    let token = result["token"].as_str().unwrap();
    assert!(hub.remote_authorize(token, &c.public, &tokens).is_ok());
    assert!(hub.remote_authorize(token, &d.public, &tokens).is_err());
    assert_eq!(
        hub.remote_pair(
            "poll",
            &reveal,
            c.public,
            d.public,
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
        let mut record = [0u8; 64];
        record[2] = 1;
        record[3] = b'x';
        if bad_padding {
            record[63] = 1;
        }
        let n = client.write_message(&record, &mut out).unwrap();
        let wire = frame(2, &out[..n]);
        if bad_padding {
            assert!(channel.receive(&wire).is_err());
        } else {
            assert_eq!(channel.receive(&wire).unwrap().envelope.unwrap(), b"x");
            assert!(channel.receive(&wire).is_err());
        }
        let n = client.write_message(&[0; 64], &mut out).unwrap();
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
            ready_tx.send(()).unwrap();
            for (offset,mode,sid) in [(1,"pair","11111111111111111111111111111111"),(4,"app","22222222222222222222222222222222"),(7,"app","33333333333333333333333333333333")] {
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
        let mut transport=client.into_transport_mode().unwrap();
        let nonce=[3;32];
        async fn request(ws:&mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,t:&mut snow::TransportState,sid:u8,v:Value)->Value {
            let data=serde_json::to_vec(&v).unwrap(); let mut record=vec![0;(data.len()+3).div_ceil(64)*64];record[1..3].copy_from_slice(&(data.len() as u16).to_be_bytes());record[3..3+data.len()].copy_from_slice(&data);
            let mut out=vec![0;65535];let n=t.write_message(&record,&mut out).unwrap();ws.send(Message::Binary(frame_for(sid,2,&out[..n]).into())).await.unwrap();
            let bytes=ws.next().await.unwrap().unwrap().into_data();let n=t.read_message(&bytes[21..],&mut out).unwrap();assert!(n>=64); let len=u16::from_be_bytes([out[1],out[2]]) as usize;serde_json::from_slice(&out[3..3+len]).unwrap()
        }
        let claim=request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":0,"type":"pair.request","id":"req_0001","op":"claim","body":{"code":code,"name":"Browser","commit":encode(&commitment(&nonce,&c.public))}})).await;
        assert_eq!(claim["status"],201);let body=&claim["body"];
        let reveal=json!({"claim_id":body["claim_id"],"poll_secret":body["poll_secret"],"nonce_c":encode(&nonce),"static_pub_c":encode(&c.public)});
        assert_eq!(request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":1,"type":"pair.request","id":"req_0002","op":"reveal","body":reveal})).await["status"],200);
        hub.approve(body["claim_id"].as_str().unwrap(),&tokens).unwrap();
        let approved=request(&mut ws,&mut transport,0x11,json!({"v":1,"seq":2,"type":"pair.request","id":"req_0003","op":"poll","body":reveal})).await;
        let token=approved["body"]["token"].as_str().unwrap(); assert!(hub.remote_authorize(token,&c.public,&tokens).is_ok());
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
