//! Real HTTP security regression tests for the serve-only browser boundary.
mod common;
use harness_api::browser::{BrowserAuth, BrowserConfig};
use reqwest::{header, StatusCode};
use serde_json::{json, Value};

#[tokio::test]
async fn browser_setup_cookie_csrf_scopes_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let ui = tempfile::tempdir().unwrap();
    std::fs::write(ui.path().join("index.html"), "<html>Harness UI</html>").unwrap();
    let (auth, code) = BrowserAuth::open(
        dir.path(),
        BrowserConfig {
            ui_dir: ui.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let server = harness_api::ApiServer::new();
    server.set_browser(auth);
    let (listener, port) = common::listener().await;
    let mut settings = common::settings(port);
    settings.cors_origins = vec!["https://evil.example".into()];
    let status = server
        .start_on(common::context(dir.path()), &settings, listener)
        .await
        .unwrap();
    let base = status.base_url.unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(&origin)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "<html>Harness UI</html>"
    );
    let missing = http
        .get(format!("{base}/not-a-route"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        missing.json::<Value>().await.unwrap()["error"]["code"],
        "api.no_route"
    );
    let session = http
        .get(format!("{base}/browser/session"))
        .send()
        .await
        .unwrap();
    assert_eq!(session.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        session.json::<Value>().await.unwrap()["setup_required"],
        true
    );
    let credentials = json!({"setup_code": code, "password": "browser-password-1234"});
    for origin in [None, Some("https://evil.example"), Some("null")] {
        let mut request = http
            .post(format!("{base}/browser/login"))
            .json(&credentials);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    let login = http
        .post(format!("{base}/browser/login"))
        .header(header::ORIGIN, &origin)
        .json(&credentials)
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    for flag in [
        "__Host-harness=",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(cookie.contains(flag));
    }
    let cookie = cookie.split(';').next().unwrap();
    let csrf = login.json::<Value>().await.unwrap()["csrf"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        http.get(format!("{base}/servers/default/api-key"))
            .header(header::COOKIE, cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let auth_get = || {
        http.get(format!("{base}/tokens"))
            .header(header::COOKIE, cookie)
    };
    assert_eq!(auth_get().send().await.unwrap().status(), StatusCode::OK);
    for csrf in [None, Some("wrong")] {
        let mut request = http
            .post(format!("{base}/tokens"))
            .header(header::ORIGIN, &origin)
            .header(header::COOKIE, cookie)
            .json(&json!({"name":"browser test", "scopes":["read"]}));
        if let Some(csrf) = csrf {
            request = request.header("x-harness-csrf", csrf);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    let created = http
        .post(format!("{base}/tokens"))
        .header(header::ORIGIN, &origin)
        .header(header::COOKIE, cookie)
        .header("x-harness-csrf", &csrf)
        .json(&json!({"name":"browser test", "scopes":["read"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = created.json::<Value>().await.unwrap();
    let token = created["token"].as_str().unwrap();
    // Existing bearer clients still use their scopes; a cookie cannot rescue an invalid bearer.
    assert_eq!(
        http.get(format!("{base}/servers"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        http.get(format!("{base}/tokens"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        auth_get()
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        auth_get()
            .header(header::ORIGIN, "https://evil.example")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        auth_get()
            .header("sec-fetch-site", "cross-site")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    // Existing SSE streams close on revocation, including when the server is otherwise idle.
    let stream = http
        .get(format!("{base}/events"))
        .header(header::COOKIE, cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    assert_eq!(
        http.delete(format!("{base}/browser/sessions"))
            .header(header::ORIGIN, &origin)
            .header(header::COOKIE, cookie)
            .header("x-harness-csrf", &csrf)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(3), stream.bytes())
            .await
            .is_ok()
    );
    assert_eq!(
        auth_get().send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    // Re-login, then logout revokes just this cookie.
    let login = http
        .post(format!("{base}/browser/login"))
        .header(header::ORIGIN, &origin)
        .json(&json!({"password":"browser-password-1234"}))
        .send()
        .await
        .unwrap();
    let cookie2 = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf2 = login.json::<Value>().await.unwrap()["csrf"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(cookie, cookie2);
    assert_eq!(
        http.post(format!("{base}/browser/logout"))
            .header(header::ORIGIN, &origin)
            .header(header::COOKIE, &cookie2)
            .header("x-harness-csrf", csrf2)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        http.get(format!("{base}/servers"))
            .header(header::COOKIE, cookie2)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    server.stop().await;
}

#[tokio::test]
async fn login_backoff_is_global_and_has_retry_after() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "ui").unwrap();
    let (auth, code) = BrowserAuth::open(
        dir.path(),
        BrowserConfig {
            ui_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let server = harness_api::ApiServer::new();
    server.set_browser(auth);
    let (listener, port) = common::listener().await;
    server
        .start_on(
            common::context(dir.path()),
            &common::settings(port),
            listener,
        )
        .await
        .unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let url = format!("{origin}/api/harness/v1/browser/login");
    assert_eq!(
        http.post(&url)
            .header(header::ORIGIN, &origin)
            .json(&json!({"password":"browser-password", "setup_code":"wrong"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = http
        .post(&url)
        .header(header::ORIGIN, &origin)
        .header("x-forwarded-for", "different-client")
        .json(&json!({"password":"browser-password", "setup_code":code}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        response.headers()[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    server.stop().await;
}
#[tokio::test]
async fn browser_network_login_requires_tls_and_invalid_tls_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "ui").unwrap();
    let ctx = common::context(dir.path());
    harness_core::control_api::TokenStore::at(&dir.path().join("config"))
        .create(
            "network bootstrap",
            &[harness_core::control_api::Scope::Read],
        )
        .unwrap();
    let server = harness_api::ApiServer::new();
    let (listener, port) = common::listener().await;
    let origin = format!("http://127.0.0.1:{port}");
    let config = BrowserConfig {
        ui_dir: dir.path().into(),
        public_origin: Some(origin.clone()),
        ..Default::default()
    };
    let (auth, code) = BrowserAuth::open(dir.path(), config).unwrap();
    server.set_browser(auth);
    let settings = harness_core::control_api::ControlApiSettings {
        host: "0.0.0.0".into(),
        allow_network: true,
        ..common::settings(port)
    };
    // Model the network-bind policy while the actual test listener stays safely on loopback.
    server
        .start_on(ctx.clone(), &settings, listener)
        .await
        .unwrap();
    let response = reqwest::Client::new()
        .post(format!("{origin}/api/harness/v1/browser/login"))
        .header(header::ORIGIN, &origin)
        .json(&json!({"setup_code": code, "password":"browser-password-1234"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "browser.https_required"
    );
    assert!(!dir.path().join("config/browser-admin.json").exists());
    server.stop().await;
    let config = BrowserConfig {
        ui_dir: dir.path().into(),
        tls_cert: Some(dir.path().join("missing.pem")),
        tls_key: Some(dir.path().join("missing.key")),
        ..Default::default()
    };
    let (auth, _) = BrowserAuth::open(dir.path(), config).unwrap();
    server.set_browser(auth);
    let (listener, port) = common::listener().await;
    assert!(server
        .start_on(ctx, &common::settings(port), listener)
        .await
        .unwrap_err()
        .starts_with("browser.tls:"));
    assert!(!server.status().await.running);
    assert!(!dir.path().join("api-token").exists());
}
