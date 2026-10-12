//! Localized transport failures shared by HTTP clients.
use std::error::Error;
use std::net::IpAddr;

use crate::error_key::{err, ErrorKey};

/// A server that dies mid-request: Windows reports it as a reset/abort ("forcibly closed",
/// os error 10054/10053), not as hyper's incomplete message.
fn closed_mid_request(error: &(dyn Error + 'static)) -> bool {
    use std::io::ErrorKind;
    let mut source = Some(error);
    while let Some(e) = source {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            if matches!(
                io.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::UnexpectedEof
                    | ErrorKind::BrokenPipe
            ) {
                return true;
            }
        }
        source = e.source();
    }
    false
}

fn source_class(error: &(dyn Error + 'static)) -> Option<&'static str> {
    let mut source = Some(error);
    while let Some(e) = source {
        let text = e.to_string().to_ascii_lowercase();
        // macOS getaddrinfo: "failed to lookup address information: nodename nor servname
        // provided, or not known"; glibc: "Name or service not known".
        if [
            "dns",
            "resolve",
            "name resolution",
            "no such host",
            "failed to lookup address",
            "nodename nor servname",
            "name or service not known",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            return Some("http.dns_failed");
        }
        if ["tls", "ssl", "certificate", "cert validation", "schannel"]
            .iter()
            .any(|s| text.contains(s))
        {
            return Some("http.tls_failed");
        }
        source = e.source();
    }
    None
}

/// Keeps the original source chain outside the user-facing message.
pub fn transport(error: &reqwest::Error, url: &str) -> String {
    let url = error.url().map(|u| u.as_str()).unwrap_or(url);
    let key = if error.is_timeout() {
        timeout_key(url)
    } else if let Some(key) = error.source().and_then(source_class) {
        key
    } else if error.is_connect() {
        if local_url(url) {
            "http.connect_failed_local"
        } else {
            "http.connect_failed"
        }
    } else if error.is_body()
        || closed_mid_request(error)
        || [
            "incomplete message",
            "connection reset",
            "connection closed",
            "unexpected eof",
            "broken pipe",
        ]
        .iter()
        .any(|s| chain(error).to_ascii_lowercase().contains(s))
    {
        "http.connection_closed"
    } else if error.is_decode() {
        // The whole body arrived but is not what the client expected (HTML error page,
        // broken JSON): the connection was fine, so do not blame it.
        "http.invalid_response"
    } else {
        "http.transport_failed"
    };
    keyed(key, error, url).into()
}

fn local_url(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    let host = host.to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

pub(crate) fn timeout_key(url: &str) -> &'static str {
    if local_url(url) {
        "http.timeout_local"
    } else {
        "http.timeout"
    }
}

pub fn dns(error: &std::io::Error, url: &str) -> String {
    keyed("http.dns_failed", error, url).into()
}

fn chain(error: &(dyn Error + 'static)) -> String {
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        detail.push_str(": ");
        detail.push_str(&e.to_string());
        source = e.source();
    }
    detail
}

/// The URL as shown to the user: no credentials, query or fragment (API keys travel there).
fn shown_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => url.split(['?', '#']).next().unwrap_or(url).to_string(),
    }
}

fn keyed(key: &'static str, error: &(dyn Error + 'static), url: &str) -> ErrorKey {
    let shown = shown_url(url);
    // reqwest's own text repeats the full URL; show the same stripped form there.
    let detail = chain(error).replace(url, &shown);
    err(key).with("url", shown).with("transport_detail", detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn assert_key(error: reqwest::Error, url: &str, key: &str) {
        let text = transport(&error, url);
        let (actual, params) = crate::error_key::parse(&text).unwrap();
        assert_eq!(actual, key);
        assert_eq!(params["url"], url);
        assert!(params["transport_detail"]
            .as_str()
            .unwrap()
            .contains(&error.to_string()));
        assert!(!params.contains_key("error"));
        assert!(!params.contains_key("detail"));
    }

    #[tokio::test]
    async fn secrets_in_the_url_are_not_shown() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("http://user:pw@{addr}/v1/search?key=SECRET123#frag");
        let e = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap_err();
        let text = transport(&e, "http://example.com/");
        assert!(!text.contains("SECRET123"), "{text}");
        assert!(!text.contains("pw@"), "{text}");
        let (key, params) = crate::error_key::parse(&text).unwrap();
        assert_eq!(key, "http.connect_failed_local");
        assert_eq!(params["url"], format!("http://{addr}/v1/search"));
    }

    #[tokio::test]
    async fn closed_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/dns/resolve/tls", listener.local_addr().unwrap());
        drop(listener);
        let e = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap_err();
        assert_key(e.without_url(), &url, "http.connect_failed_local");
    }

    #[test]
    fn loopback_hosts() {
        for url in [
            "http://127.0.0.1/",
            "http://127.255.255.254/",
            "http://localhost/",
            "http://LOCALHOST/",
            "http://api.LocalHost/",
            "http://nested.api.localhost/",
            "http://[::1]/",
            "http://user:pw@[::1]/?host=example.com#remote",
        ] {
            assert!(local_url(url), "{url}");
            assert_eq!(timeout_key(url), "http.timeout_local", "{url}");
        }
        for url in [
            "http://example.com/",
            "http://localhost.example.com/",
            "http://notlocalhost/",
            "http://192.168.1.20/",
            "http://128.0.0.1/",
            "http://[::2]/",
            "http://[::ffff:127.0.0.1]/",
            "http://localhost@example.com/?host=localhost#localhost",
            "not a URL",
        ] {
            assert!(!local_url(url), "{url}");
            assert_eq!(timeout_key(url), "http.timeout", "{url}");
        }
    }

    #[tokio::test]
    async fn remote_closed_proxy_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let url = "http://example.com/";
        let e = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(proxy).unwrap())
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap_err();
        assert!(e.is_connect());
        assert_key(e, url, "http.connect_failed");
    }

    async fn response(delay: bool) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            assert!(socket.read(&mut buf).await.unwrap() > 0);
            if delay {
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else {
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort",
                    )
                    .await
                    .unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        (url, task)
    }

    /// A complete 200 response whose body is not the expected JSON (an HTML error page from a
    /// proxy or captive portal): the connection did not close early, the content was wrong.
    #[tokio::test]
    async fn complete_but_invalid_json() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/models", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            assert!(socket.read(&mut buf).await.unwrap() > 0);
            let body = "<html>not json</html>";
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.shutdown().await.unwrap();
        });
        let e = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert!(e.is_decode(), "{e:?}");
        assert_key(e, &url, "http.invalid_response");
        task.await.unwrap();
    }

    /// A body cut short and then parsed as JSON is still the connection's fault.
    #[tokio::test]
    async fn truncated_json_body() {
        let (url, task) = response(false).await;
        let e = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert_key(e, &url, "http.connection_closed");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn slow_response() {
        let (url, task) = response(true).await;
        let e = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap_err();
        assert_key(e, &url, "http.timeout_local");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn remote_slow_proxy_response() {
        let (proxy, task) = response(true).await;
        let url = "http://example.com/";
        let e = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(proxy).unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap_err();
        assert!(e.is_timeout());
        assert_key(e, url, "http.timeout");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn truncated_body() {
        let (url, task) = response(false).await;
        let e = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap_err();
        assert_key(e, &url, "http.connection_closed");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn server_killed_mid_request() {
        // A killed llama-server: the request was read, then the socket is reset (RST).
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            assert!(socket.read(&mut buf).await.unwrap() > 0);
            socket.set_zero_linger().unwrap();
            drop(socket);
        });
        let e = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(&url)
            .body("{}")
            .send()
            .await
            .unwrap_err();
        assert_key(e, &url, "http.connection_closed");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn dns_failure_without_external_lookup() {
        #[derive(Debug)]
        struct FailedDns;
        impl reqwest::dns::Resolve for FailedDns {
            fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
                Box::pin(async {
                    Err(Box::<dyn Error + Send + Sync>::from(std::io::Error::other(
                        "dns error: failed to resolve host",
                    )))
                })
            }
        }
        let url = "http://unreachable.invalid/";
        let e = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(std::sync::Arc::new(FailedDns))
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap_err();
        assert!(e.is_connect());
        assert_key(e, url, "http.dns_failed");
    }

    #[tokio::test]
    async fn invalid_request_is_other() {
        let url = "not a URL";
        let e = reqwest::Client::new().get(url).send().await.unwrap_err();
        assert_key(e, url, "http.transport_failed");
    }

    #[test]
    fn dns_and_tls_sources_without_network() {
        #[derive(Debug)]
        struct Wrapped(std::io::Error);
        impl std::fmt::Display for Wrapped {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("connection failed")
            }
        }
        impl Error for Wrapped {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&self.0)
            }
        }
        for (detail, key) in [
            ("dns error: failed to resolve host", "http.dns_failed"),
            (
                "failed to lookup address information: nodename nor servname provided, or not known",
                "http.dns_failed",
            ),
            ("TLS certificate invalid", "http.tls_failed"),
        ] {
            let e = Wrapped(std::io::Error::other(detail));
            assert_eq!(source_class(&e), Some(key));
            assert!(chain(&e).contains(detail));
        }
    }

    #[test]
    fn connection_reset_is_closed_mid_request() {
        // macOS/Linux report a server killed mid-request as ECONNRESET.
        let e = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert!(closed_mid_request(&e));
        let e = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert!(!closed_mid_request(&e));
    }
}
