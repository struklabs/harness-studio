use super::*;
use crate::control_api::allows;

fn setup() -> (tempfile::TempDir, TokenStore, Arc<Hub>) {
    let dir = tempfile::tempdir().unwrap();
    let tokens = TokenStore::at(dir.path());
    let hub = hub(dir.path());
    (dir, tokens, hub)
}

fn spec(preset: AccessPreset) -> OfferSpec {
    OfferSpec {
        role: DeviceRole::Client,
        preset,
        models: Vec::new(),
        ttl_secs: None,
        acknowledge_admin: false,
    }
}

const NONCE: &str = "0123456789abcdef0123456789abcdef";

fn pair(hub: &Hub, tokens: &TokenStore, preset: AccessPreset) -> (DeviceView, String) {
    let offer = hub.create_offer(&spec(preset)).unwrap();
    let started = hub.claim(&offer.code, "Pixel", "android", NONCE).unwrap();
    let device = hub.approve(&started.claim_id, tokens).unwrap();
    let poll = hub
        .poll(&started.claim_id, &started.poll_secret, tokens)
        .unwrap();
    (device, poll.token.unwrap())
}

#[test]
fn full_pairing_flow_delivers_the_token_once_after_approval() {
    let (_d, tokens, hub) = setup();
    let offer = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    assert_eq!(offer.code.len(), 9);

    let started = hub
        .claim(&offer.code, " Pixel 9 ", "android", NONCE)
        .unwrap();
    assert_eq!(started.verification.len(), 6);
    // Before approval: pending, no token, and the device is not registered.
    let poll = hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .unwrap();
    assert_eq!(poll.status, ClaimState::Pending);
    assert!(poll.token.is_none());
    assert!(hub.list(&tokens).unwrap().is_empty());
    assert_eq!(tokens.count(), 0);

    let pending = hub.pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "Pixel 9");
    assert_eq!(pending[0].verification, started.verification);

    let device = hub.approve(&started.claim_id, &tokens).unwrap();
    assert_eq!(device.name, "Pixel 9");
    assert_eq!(device.reachability, Reachability::Never);

    let poll = hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .unwrap();
    assert_eq!(poll.status, ClaimState::Approved);
    let token = poll.token.unwrap();
    assert!(tokens.verify(&token).is_some());
    // The token is handed out once.
    assert_eq!(
        hub.poll(&started.claim_id, &started.poll_secret, &tokens)
            .unwrap_err(),
        ERR_NOT_FOUND
    );
    let events: Vec<String> = hub.audit_log(10).into_iter().map(|e| e.event).collect();
    assert!(events.contains(&"approved".to_string()));
    assert!(events.contains(&"claim_pending".to_string()));
}

#[test]
fn code_is_single_use_and_wrong_codes_lock_out() {
    let (_d, _tokens, hub) = setup();
    let offer = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    hub.claim(&offer.code, "A", "", NONCE).unwrap();
    // Used code.
    assert_eq!(
        hub.claim(&offer.code, "B", "", NONCE).unwrap_err(),
        ERR_INVALID_CODE
    );
    let fresh = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    for _ in 0..4 {
        assert_eq!(
            hub.claim("0000-0000", "X", "", NONCE).unwrap_err(),
            ERR_INVALID_CODE
        );
    }
    // Five failures lock claims, even with the right code.
    assert_eq!(
        hub.claim(&fresh.code, "C", "", NONCE).unwrap_err(),
        ERR_LOCKED
    );
    hub.advance_clock(FAIL_WINDOW_SECS + 1);
    assert!(hub.claim(&fresh.code, "C", "", NONCE).is_ok());
}

#[test]
fn offers_and_claims_expire() {
    let (_d, tokens, hub) = setup();
    let offer = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    hub.advance_clock(OFFER_TTL_SECS + 1);
    assert_eq!(
        hub.claim(&offer.code, "A", "", NONCE).unwrap_err(),
        ERR_INVALID_CODE
    );
    let offer = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    let started = hub.claim(&offer.code, "A", "", NONCE).unwrap();
    hub.advance_clock(CLAIM_TTL_SECS + 1);
    assert!(hub.pending().is_empty());
    assert_eq!(
        hub.approve(&started.claim_id, &tokens).unwrap_err(),
        ERR_EXPIRED
    );
    let poll = hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .unwrap();
    assert_eq!(poll.status, ClaimState::Expired);
}

#[test]
fn deny_never_issues_a_token_and_poll_needs_the_secret() {
    let (_d, tokens, hub) = setup();
    let offer = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    let started = hub.claim(&offer.code, "A", "", NONCE).unwrap();
    assert_eq!(
        hub.poll(&started.claim_id, "wrong", &tokens).unwrap_err(),
        ERR_NOT_FOUND
    );
    hub.deny(&started.claim_id).unwrap();
    assert_eq!(
        hub.approve(&started.claim_id, &tokens).unwrap_err(),
        ERR_NOT_PENDING
    );
    let poll = hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .unwrap();
    assert_eq!(poll.status, ClaimState::Denied);
    assert!(poll.token.is_none());
    assert_eq!(tokens.count(), 0);
}

#[test]
fn claim_input_is_validated() {
    let (_d, _tokens, hub) = setup();
    let offer = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    assert_eq!(
        hub.claim(&offer.code, "  ", "", NONCE).unwrap_err(),
        ERR_DEVICE_NAME
    );
    assert_eq!(
        hub.claim(&offer.code, "A", "", "short").unwrap_err(),
        ERR_NONCE
    );
    // The code accepts typing variants: spaces, no dash.
    assert!(hub
        .claim(&offer.code.replace('-', " "), "A", "", NONCE)
        .is_ok());
}

#[test]
fn admin_needs_an_acknowledgement() {
    let (_d, tokens, hub) = setup();
    assert_eq!(
        hub.create_offer(&spec(AccessPreset::Admin)).unwrap_err(),
        ERR_ADMIN_ACK
    );
    let mut s = spec(AccessPreset::Admin);
    s.acknowledge_admin = true;
    assert!(hub.create_offer(&s).is_ok());
    let (device, _) = pair(&hub, &tokens, AccessPreset::ChatOnly);
    assert_eq!(
        hub.set_access(&device.id, AccessPreset::Admin, &[], false, &tokens)
            .unwrap_err(),
        ERR_ADMIN_ACK
    );
}

#[test]
fn presets_map_to_scopes_and_policy() {
    let chat = AccessPreset::ChatOnly;
    assert!(allows(&chat.scopes(), Scope::Inference));
    assert!(!allows(&chat.scopes(), Scope::Read));
    assert!(!allows(&chat.scopes(), Scope::Admin));
    let p = chat.policy(&["qwen*".into()]);
    assert!(p.allows_endpoint(GatewayEndpoint::Chat));
    assert!(p.allows_endpoint(GatewayEndpoint::Models));
    assert!(!p.allows_endpoint(GatewayEndpoint::Embeddings));
    assert!(!p.allows_endpoint(GatewayEndpoint::Images));
    assert!(p.allows_model("Qwen3-4B"));
    assert!(!p.allows_model("llama"));

    let gw = AccessPreset::GatewayModels.policy(&[]);
    assert!(gw.allows_endpoint(GatewayEndpoint::Embeddings));
    assert!(gw.allows_endpoint(GatewayEndpoint::Images));
    assert!(!gw.allows_endpoint(GatewayEndpoint::Metrics));
    assert!(gw.allows_model("anything"));
    assert!(!allows(&AccessPreset::GatewayModels.scopes(), Scope::Admin));
    assert!(allows(&AccessPreset::Admin.scopes(), Scope::ModelsWrite));
}

#[test]
fn issued_token_carries_the_scope_of_its_preset() {
    let (_d, tokens, hub) = setup();
    let mut s = spec(AccessPreset::ChatOnly);
    s.models = vec!["coder*".into()];
    let offer = hub.create_offer(&s).unwrap();
    let started = hub.claim(&offer.code, "Pixel", "android", NONCE).unwrap();
    hub.approve(&started.claim_id, &tokens).unwrap();
    let poll = hub
        .poll(&started.claim_id, &started.poll_secret, &tokens)
        .unwrap();
    let (info, scopes) = tokens.verify(&poll.token.unwrap()).unwrap();
    assert_eq!(scopes, vec![Scope::Inference]);
    assert_eq!(info.policy.models, vec!["coder*"]);
    assert!(info.policy.allows_endpoint(GatewayEndpoint::Chat));
    assert!(!info.policy.allows_endpoint(GatewayEndpoint::Audio));
    assert_eq!(poll.models, vec!["coder*"]);
}

#[test]
fn revoke_kills_the_token_and_revoke_all_clears_everything() {
    let (_d, tokens, hub) = setup();
    let (a, token_a) = pair(&hub, &tokens, AccessPreset::ChatOnly);
    let (_b, token_b) = pair(&hub, &tokens, AccessPreset::GatewayModels);
    let plain = tokens.create("my script", &[Scope::Read]).unwrap();
    assert_eq!(hub.list(&tokens).unwrap().len(), 2);

    hub.revoke(&a.id, &tokens).unwrap();
    assert!(tokens.verify(&token_a).is_none());
    assert!(tokens.verify(&token_b).is_some());
    assert_eq!(
        hub.revoke(&a.id, &tokens).unwrap_err(),
        ERR_DEVICE_NOT_FOUND
    );

    // An open offer and a pending claim disappear too.
    let open = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    let offer2 = hub.create_offer(&spec(AccessPreset::ChatOnly)).unwrap();
    let pending = hub.claim(&offer2.code, "Late", "", NONCE).unwrap();
    assert_eq!(hub.revoke_all(&tokens).unwrap(), 1);
    assert!(tokens.verify(&token_b).is_none());
    assert!(hub.list(&tokens).unwrap().is_empty());
    assert!(hub.pending().is_empty());
    assert_eq!(
        hub.claim(&open.code, "X", "", NONCE).unwrap_err(),
        ERR_INVALID_CODE
    );
    assert_eq!(
        hub.approve(&pending.claim_id, &tokens).unwrap_err(),
        ERR_NOT_FOUND
    );
    // Tokens that pairing did not make stay.
    assert!(tokens.verify(&plain.token).is_some());
    let events: Vec<String> = hub.audit_log(50).into_iter().map(|e| e.event).collect();
    assert!(events.contains(&"revoked".to_string()));
    assert!(events.contains(&"revoke_all".to_string()));
}

#[test]
fn token_revoked_elsewhere_drops_the_device() {
    let (_d, tokens, hub) = setup();
    let (device, _) = pair(&hub, &tokens, AccessPreset::ChatOnly);
    let infos = tokens.list().unwrap();
    tokens.revoke(&infos[0].id).unwrap();
    assert!(hub.list(&tokens).unwrap().is_empty());
    assert!(hub
        .audit_log(10)
        .iter()
        .any(|e| e.event == "token_removed" && e.device_id.as_deref() == Some(&device.id)));
}

#[test]
fn set_access_changes_the_live_token() {
    let (_d, tokens, hub) = setup();
    let (device, token) = pair(&hub, &tokens, AccessPreset::ChatOnly);
    let updated = hub
        .set_access(
            &device.id,
            AccessPreset::GatewayModels,
            &["a*".into()],
            false,
            &tokens,
        )
        .unwrap();
    assert_eq!(updated.preset, AccessPreset::GatewayModels);
    assert_eq!(updated.models, vec!["a*"]);
    let (info, _) = tokens.verify(&token).unwrap();
    assert!(info.policy.allows_endpoint(GatewayEndpoint::Embeddings));
}

#[test]
fn reports_presence_and_missing_components() {
    let (_d, tokens, hub) = setup();
    let (device, token) = pair(&hub, &tokens, AccessPreset::ChatOnly);
    let (info, _) = tokens.verify(&token).unwrap();
    assert!(!hub
        .report("not-a-device-token", DeviceReport::default())
        .unwrap());
    let ok = hub
        .report(
            &info.id,
            DeviceReport {
                models: vec!["m1".into(), "  ".into()],
                components: vec!["chat_client".into()],
                vram_free_bytes: Some(8 << 30),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(ok);
    let view = &hub.list(&tokens).unwrap()[0];
    assert_eq!(view.id, device.id);
    assert_eq!(view.reachability, Reachability::Online);
    assert_eq!(view.report.as_ref().unwrap().models, vec!["m1"]);
    assert!(view.missing_components.is_empty());
    hub.advance_clock(ONLINE_WINDOW_SECS + 5);
    assert_eq!(
        hub.list(&tokens).unwrap()[0].reachability,
        Reachability::Offline
    );
    hub.touch(&info.id);
    assert_eq!(
        hub.list(&tokens).unwrap()[0].reachability,
        Reachability::Online
    );
}

#[test]
fn role_components_are_minimal() {
    assert_eq!(DeviceRole::Client.components(), ["chat_client"]);
    assert_eq!(DeviceRole::Storage.components(), ["model_storage"]);
    assert!(DeviceRole::InferenceHost.components().contains(&"runtime"));
    assert_eq!(
        DeviceRole::InferenceHost.missing(&["gateway".into()]),
        vec!["runtime", "models"]
    );
}

fn host(id: &str, vram: u64, reach: Reachability) -> DeviceView {
    DeviceView {
        id: id.into(),
        name: id.into(),
        platform: String::new(),
        role: DeviceRole::InferenceHost,
        preset: AccessPreset::GatewayModels,
        created_at: 0,
        last_seen: Some(0),
        reachability: reach,
        scopes: vec![],
        endpoints: vec![],
        models: vec![],
        report: Some(DeviceReport {
            vram_free_bytes: Some(vram),
            ..Default::default()
        }),
        components: vec![],
        missing_components: vec!["runtime".into()],
    }
}

#[test]
fn suggests_only_capable_reachable_hosts() {
    let devices = vec![
        host("small", 4 << 30, Reachability::Online),
        host("big-offline", 48 << 30, Reachability::Offline),
        host("big-online", 24 << 30, Reachability::Online),
        host("never", 96 << 30, Reachability::Never),
    ];
    assert!(suggest_hosts(10 << 30, 16 << 30, &devices).is_none());
    let s = suggest_hosts(10 << 30, 6 << 30, &devices).unwrap();
    let ids: Vec<&str> = s.iter().map(|h| h.device_id.as_str()).collect();
    assert_eq!(ids, ["big-online", "big-offline"]);
    assert_eq!(s[0].missing_components, vec!["runtime"]);
    assert!(suggest_hosts(200 << 30, 6 << 30, &devices)
        .unwrap()
        .is_empty());
    let mut client = host("client", 99 << 30, Reachability::Online);
    client.role = DeviceRole::Client;
    assert!(suggest_hosts(10 << 30, 1, &[client]).unwrap().is_empty());
}

#[test]
fn opencode_config_holds_no_secret() {
    let c = opencode_config("http://10.0.0.5:11434/", &["qwen3".into()]);
    let p = &c["provider"]["harness-remote"];
    assert_eq!(p["options"]["baseURL"], "http://10.0.0.5:11434/v1");
    assert_eq!(p["options"]["apiKey"], "{env:HARNESS_REMOTE_TOKEN}");
    assert!(p["models"]["qwen3"].is_object());
}

#[test]
fn signaling_stub_is_local_and_has_no_peers() {
    let s = LocalOnlySignaling;
    assert_eq!(s.name(), "local");
    assert!(s.discover().unwrap().is_empty());
    assert!(s
        .announce(&SignalAnnouncement {
            device_name: "x".into(),
            role: DeviceRole::Client,
            address: None
        })
        .is_ok());
}

#[test]
fn verification_code_binds_all_inputs() {
    let a = verification_code("12345678", "n1", "s1", "c1");
    assert_eq!(a.len(), 6);
    assert_eq!(a, verification_code("12345678", "n1", "s1", "c1"));
    assert_ne!(a, verification_code("12345679", "n1", "s1", "c1"));
    assert_ne!(a, verification_code("12345678", "n2", "s1", "c1"));
}

#[test]
fn audit_log_is_newest_first_and_capped() {
    let (_d, _tokens, hub) = setup();
    for i in 0..5 {
        hub.audit("e", None, None, Some(&i.to_string()));
    }
    let log = hub.audit_log(3);
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].detail.as_deref(), Some("4"));
}
