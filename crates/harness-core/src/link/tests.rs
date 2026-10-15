use super::*;

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
