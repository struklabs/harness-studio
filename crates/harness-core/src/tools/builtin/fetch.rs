use futures_util::future::BoxFuture;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::sync::OnceLock;

use super::{http, simple_def};
use crate::tools::{PermissionClass, ToolCtx, ToolDef, ToolError, ToolErrorCode, ToolOutput};

const MAX_DOWNLOAD: usize = 2 * 1024 * 1024;

pub fn def() -> ToolDef {
    simple_def(
        "fetch_url",
        "Fetch a public HTTP or HTTPS URL. HTML is converted to readable text; JSON and text are returned as-is. Private/local network targets, unsupported content types, oversized responses, and unsafe redirects are refused.",
        PermissionClass::Restricted,
        json!({"type":"object","properties":{"url":{"type":"string","description":"Public http or https URL."}},"required":["url"],"additionalProperties":false}),
    )
}

pub fn run(args: Value, ctx: &ToolCtx) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
    Box::pin(async move {
        let raw = args["url"].as_str().unwrap_or_default();
        let (response, final_url) = http::get(raw, ctx).await?;
        finish(response, final_url).await
    })
}

async fn finish(
    response: reqwest::Response,
    final_url: reqwest::Url,
) -> Result<ToolOutput, ToolError> {
    let target = http::target(&final_url);
    if !response.status().is_success() {
        return Err(ToolError::new(
            ToolErrorCode::Network,
            format!("HTTP {} from {final_url}", response.status()),
        )
        .network_target(target));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let kind = if content_type == "text/html" || content_type == "application/xhtml+xml" {
        "html"
    } else if content_type.starts_with("text/")
        || content_type == "application/json"
        || content_type.ends_with("+json")
    {
        "text"
    } else {
        return Err(ToolError::new(ToolErrorCode::UnsupportedContentType, format!("refused unsupported content type {content_type:?}; only HTML, JSON, and text are accepted")).network_target(target));
    };
    let bytes = http::bytes_limited(response, MAX_DOWNLOAD, &target).await?;
    let text = String::from_utf8(bytes).map_err(|_| {
        ToolError::new(
            ToolErrorCode::UnsupportedContentType,
            "response is not valid UTF-8 text",
        )
        .network_target(&target)
    })?;
    let content = if kind == "html" {
        html_to_text(&text)
    } else {
        text
    };
    Ok(ToolOutput {
        content,
        network_target: Some(target),
    })
}

fn strip_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<!--.*?-->|<script\b[^>]*>.*?</script\s*>|<style\b[^>]*>.*?</style\s*>|<nav\b[^>]*>.*?</nav\s*>|<noscript\b[^>]*>.*?</noscript\s*>|<svg\b[^>]*>.*?</svg\s*>").unwrap())
}

pub fn html_to_text(html: &str) -> String {
    let cleaned = strip_re().replace_all(html, " ");
    let doc = Html::parse_document(&cleaned);
    let root = Selector::parse("main, article, body").unwrap();
    let element = doc.select(&root).next();
    let pieces: Vec<_> = element
        .into_iter()
        .flat_map(|e| e.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    pieces.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_world_html_fixture_becomes_readable_text() {
        let text = html_to_text(include_str!("fixtures/article.html"));
        assert!(text.contains("A practical heading"));
        assert!(text.contains("linked explanation"));
        assert!(
            !text.contains("window.tracker")
                && !text.contains("Cookie settings")
                && !text.contains("display:none")
        );
    }

    #[test]
    fn refuses_private_address_literals() {
        for url in [
            "http://127.0.0.1/a",
            "http://10.2.3.4",
            "http://169.254.169.254/latest/meta-data",
        ] {
            let parsed = crate::network::parse_public_http_url(url).unwrap();
            let host: std::net::IpAddr = parsed.host_str().unwrap().parse().unwrap();
            assert!(crate::network::is_private_network_ip(host), "{url}");
        }
    }

    #[test]
    fn additional_non_public_ranges_are_refused() {
        for ip in [
            "0.1.2.3",
            "100.64.0.1",
            "198.18.0.1",
            "::ffff:127.0.0.1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "fd00::1",
            "fe80::1",
            "::1",
        ] {
            assert!(
                crate::network::is_private_network_ip(ip.parse().unwrap()),
                "{ip}"
            );
        }
        for ip in [
            "8.8.8.8",
            "93.184.216.34",
            "2606:4700::1111",
            "64:ff9b::808:808",
        ] {
            assert!(
                !crate::network::is_private_network_ip(ip.parse().unwrap()),
                "{ip}"
            );
        }
    }

    #[tokio::test]
    async fn public_path_refuses_loopback_literals_and_localhost_name() {
        let ctx = ToolCtx::new(None);
        for url in [
            "http://127.0.0.1:1/",
            "http://10.1.2.3/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[::ffff:7f00:1]/",
            "http://2130706433/",
            "http://localhost/",
        ] {
            let e = run(json!({"url": url}), &ctx).await.unwrap_err();
            assert_eq!(e.code, ToolErrorCode::Network, "{url}");
            assert!(
                e.message.contains("private or local"),
                "{url}: {}",
                e.message
            );
        }
        for url in [
            "file:///etc/passwd",
            "ftp://example.com/",
            "http://u:p@example.com/",
        ] {
            let e = run(json!({"url": url}), &ctx).await.unwrap_err();
            assert_eq!(e.code, ToolErrorCode::InvalidArguments, "{url}");
        }
    }

    fn server(response: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(&response).unwrap();
        });
        format!("http://{address}/")
    }
    async fn local(response: Vec<u8>) -> Result<ToolOutput, ToolError> {
        let url = server(response);
        let (r, u) = http::get_allow_first_private(&url, std::time::Duration::from_secs(3)).await?;
        finish(r, u).await
    }

    #[tokio::test]
    async fn refuses_redirect_to_localhost_after_receiving_redirect() {
        let destination = server(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok".to_vec(),
        );
        let response =
            format!("HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\n\r\n")
                .into_bytes();
        let e = local(response).await.unwrap_err();
        assert_eq!(e.code, ToolErrorCode::Network);
        assert!(e.message.contains("private or local"), "{}", e.message);
    }
    #[tokio::test]
    async fn enforces_size_limit_while_streaming() {
        let body = vec![b'x'; MAX_DOWNLOAD + 1];
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend(body);
        assert_eq!(
            local(response).await.unwrap_err().code,
            ToolErrorCode::ResponseTooLarge
        );
    }
    #[tokio::test]
    async fn refuses_binary_content_type() {
        let e = local(
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\nPNG!".to_vec(),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, ToolErrorCode::UnsupportedContentType);
    }

    #[tokio::test]
    #[ignore = "requires public internet"]
    async fn fetches_a_real_web_page() {
        let ctx = ToolCtx::new(None);
        let out = run(json!({"url":"https://example.com/"}), &ctx)
            .await
            .unwrap();
        println!(
            "target={} chars={} preview={}",
            out.network_target.as_deref().unwrap(),
            out.content.chars().count(),
            out.content.lines().take(2).collect::<Vec<_>>().join(" | ")
        );
        assert!(out.content.contains("documentation examples"));
    }
}
