use super::*;

#[test]
fn full_access_tokens_warn_without_blocking() {
    let admin = serde_json::json!({"scopes": ["read", "admin"]});
    let inference = serde_json::json!({"scopes": ["inference"]});
    for (token, identity, expected) in [
        ("hs_named", &admin, true),
        ("hs_named", &inference, false),
        ("hs_named", &serde_json::Value::Null, false),
        ("hss_session", &serde_json::Value::Null, true),
        ("hss_session", &inference, true),
    ] {
        let warning = scope_warning(token, identity);
        assert_eq!(warning.is_some(), expected);
        if let Some(warning) = warning {
            assert_eq!(
                crate::error_key::parse(&warning).unwrap().0,
                "link.inference_scope_warning"
            );
        }
    }
}

#[test]
fn certificate_errors_are_distinct_from_other_tls_failures() {
    #[derive(Debug)]
    struct Wrapped(std::io::Error);
    impl std::fmt::Display for Wrapped {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("TLS connection failed")
        }
    }
    impl std::error::Error for Wrapped {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }
    for detail in [
        "invalid peer certificate: UnknownIssuer",
        "self signed certificate",
        "certificate has expired",
        "CERT_E_CN_NO_MATCH",
        "SEC_E_UNTRUSTED_ROOT",
    ] {
        assert!(
            certificate_invalid(&Wrapped(std::io::Error::other(detail))),
            "{detail}"
        );
    }
    for detail in [
        "TLS handshake failed",
        "connection refused",
        "dns lookup failed",
    ] {
        assert!(
            !certificate_invalid(&Wrapped(std::io::Error::other(detail))),
            "{detail}"
        );
    }
}

#[test]
fn remote_ids_preserve_model_paths() {
    let id = model_id("office", "org/model:latest");
    assert_eq!(parse_model(&id), Some(("office", "org/model:latest")));
    for bad in [
        "remote:Office/a",
        "remote:/a",
        "remote:office/",
        "office/a",
        "remote:-office/a",
        "remote:office/a\n",
    ] {
        assert_eq!(parse_model(bad), None, "{bad}");
    }
    let rewritten = rewrite_model(
        &id,
        br#"{"model":"remote:office/org/model:latest","stream":true}"#,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&rewritten).unwrap();
    assert_eq!(value["model"], "org/model:latest");
    assert_eq!(value["stream"], true);
}

#[tokio::test]
async fn url_rules_and_v1_normalization() {
    assert_eq!(
        validate_url("https://example.com/prefix/v1/", false)
            .await
            .unwrap(),
        "https://example.com/prefix"
    );
    assert!(validate_url("https://example.com", false).await.is_ok());
    for url in [
        "http://127.0.0.1:1234",
        "http://192.168.1.2",
        "http://100.64.1.2",
        "http://[::1]",
    ] {
        assert!(validate_url(url, false).await.is_err());
        assert!(validate_url(url, true).await.is_ok(), "{url}");
    }
    assert!(validate_url("http://localhost:1234", true).await.is_ok());
    for url in [
        "http://8.8.8.8",
        "http://0.0.0.0",
        "http://224.0.0.1",
        "http://198.18.1.1",
        "http://[::]",
    ] {
        assert!(validate_url(url, true).await.is_err(), "{url}");
    }
    for url in [
        "ftp://example.com",
        "https://user:pass@example.com",
        "https://example.com/?token=secret",
        "https://example.com/#secret",
    ] {
        assert!(validate_url(url, true).await.is_err(), "{url}");
    }
}

#[test]
fn general_settings_save_preserves_links() {
    let mut stored = AppSettings::default();
    stored.links.push(RemoteLink {
        id: "link-id".into(),
        name: "office".into(),
        base_url: "https://example.com".into(),
        allow_insecure_http: false,
        enabled: true,
        token_set: true,
    });
    let mut incoming = AppSettings::default();
    crate::network::sanitize_settings(&mut incoming, &stored).unwrap();
    assert_eq!(incoming.links, stored.links);
    let value = serde_json::to_value(&incoming).unwrap();
    assert!(value["links"][0].get("token").is_none());
    assert_eq!(value["links"][0]["token_set"], true);
}

#[test]
fn secrets_are_redacted() {
    let secret = "hs_link_secret";
    let text = crate::integrations::launch::redact_text(&format!("Bearer {secret}"), vec![secret]);
    assert!(!text.contains(secret));
    secrets::link_set("unit-link", secret).unwrap();
    assert_eq!(
        secrets::link_get("unit-link").unwrap().as_deref(),
        Some(secret)
    );
    secrets::link_delete("unit-link").unwrap();
    assert_eq!(secrets::link_get("unit-link").unwrap(), None);
}

#[test]
fn http_links_require_every_resolved_address_to_be_private() {
    let addr = |s: &str| s.parse::<SocketAddr>().unwrap();
    assert!(all_private(&[addr("127.0.0.1:80"), addr("[::1]:80")]));
    assert!(all_private(&[addr("192.168.1.2:80")]));
    // A name that now answers with a public address (DNS rebinding) is refused.
    assert!(!all_private(&[addr("8.8.8.8:80")]));
    assert!(!all_private(&[addr("192.168.1.2:80"), addr("8.8.8.8:80")]));
    assert!(!all_private(&[]));
}

#[test]
fn token_must_be_re_entered_when_the_origin_changes() {
    assert!(same_origin("https://a.example", "https://a.example/prefix"));
    assert!(same_origin("http://a.example:80", "http://a.example"));
    for other in [
        "http://a.example",
        "https://b.example",
        "https://a.example:8443",
    ] {
        assert!(!same_origin("https://a.example", other), "{other}");
    }
}

#[tokio::test]
async fn pinned_client_connects_only_to_the_checked_address() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 1024];
            let _ = socket.read(&mut buffer).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await;
        }
    });
    let base = format!("http://pinned.invalid:{}", addr.port());
    // The name does not exist in DNS; only the pin can make it reachable.
    let pinned = build_client(&base, &[addr], 5).unwrap();
    let body = pinned
        .get(&base)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "ok");
    let unpinned = build_client(&base, &[], 5).unwrap();
    assert!(unpinned.get(&base).send().await.is_err());
}
