use std::time::Duration;

use reqwest::{redirect::Policy, Response, Url};

use crate::network::{
    parse_public_http_url, public_redirect_target, resolve_public_http_url,
    PUBLIC_HTTP_REDIRECT_LIMIT,
};
use crate::tools::{ToolCtx, ToolError, ToolErrorCode};

pub async fn get(raw: &str, ctx: &ToolCtx) -> Result<(Response, Url), ToolError> {
    request(
        reqwest::Method::GET,
        raw,
        ctx,
        reqwest::header::HeaderMap::new(),
        None,
    )
    .await
}

pub async fn request(
    method: reqwest::Method,
    raw: &str,
    ctx: &ToolCtx,
    headers: reqwest::header::HeaderMap,
    json: Option<serde_json::Value>,
) -> Result<(Response, Url), ToolError> {
    request_inner(
        method,
        raw,
        ctx.timeout,
        headers,
        json,
        None,
        None,
        None,
        Some(ctx),
        None,
    )
    .await
}

/// POST an `application/x-www-form-urlencoded` body (a search form) to a public URL.
pub async fn post_form(
    raw: &str,
    ctx: &ToolCtx,
    headers: reqwest::header::HeaderMap,
    form: &[(&str, &str)],
    limits: Option<(Duration, Duration)>,
) -> Result<(Response, Url), ToolError> {
    let (connect, total) =
        limits.map_or((None, ctx.timeout), |(c, t)| (Some(c), t.min(ctx.timeout)));
    request_inner(
        reqwest::Method::POST,
        raw,
        total,
        headers,
        None,
        Some(form),
        connect,
        None,
        Some(ctx),
        None,
    )
    .await
}

/// A check run on every URL a request visits (the first and each redirect target) before it is
/// resolved or contacted.
pub type UrlCheck<'a> = &'a (dyn Fn(&Url) -> Result<(), ToolError> + Sync);

/// [`request`] for installed tools: `check` vets every hop (the manifest's domain allowlist), and
/// `trusted_host` (`host[:port]`) may resolve to a private address (tests against a local
/// server).
pub async fn request_checked(
    method: reqwest::Method,
    raw: &str,
    ctx: &ToolCtx,
    json: Option<serde_json::Value>,
    trusted_host: Option<String>,
    check: UrlCheck<'_>,
) -> Result<(Response, Url), ToolError> {
    request_inner(
        method,
        raw,
        ctx.timeout,
        reqwest::header::HeaderMap::new(),
        json,
        None,
        None,
        trusted_host,
        Some(ctx),
        Some(check),
    )
    .await
}

/// GET against an endpoint the user configured themselves (a self-hosted SearXNG is usually on
/// localhost or the LAN). Only that exact host may be private; every redirect to any other host
/// goes through the full public-address check.
pub async fn get_configured(raw: &str, ctx: &ToolCtx) -> Result<(Response, Url), ToolError> {
    let trusted = Some(target(
        &parse_public_http_url(raw).map_err(ToolError::invalid)?,
    ));
    request_inner(
        reqwest::Method::GET,
        raw,
        ctx.timeout,
        reqwest::header::HeaderMap::new(),
        None,
        None,
        None,
        trusted,
        Some(ctx),
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn request_inner(
    method: reqwest::Method,
    raw: &str,
    timeout: Duration,
    headers: reqwest::header::HeaderMap,
    json: Option<serde_json::Value>,
    form: Option<&[(&str, &str)]>,
    connect_timeout: Option<Duration>,
    trusted_host: Option<String>,
    ctx: Option<&ToolCtx>,
    check: Option<UrlCheck<'_>>,
) -> Result<(Response, Url), ToolError> {
    let mut url = parse_public_http_url(raw).map_err(ToolError::invalid)?;
    let sensitive = !headers.is_empty() || json.is_some();
    for _ in 0..=PUBLIC_HTTP_REDIRECT_LIMIT {
        let target = target(&url);
        if let Some(check) = check {
            check(&url)?;
        }
        if let Some(ctx) = ctx {
            ctx.set_network_target(&target);
        }
        let host = url.host_str().unwrap_or_default();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| ToolError::invalid("URL has no known port"))?;
        let addresses = if trusted_host.as_deref() == Some(target.as_str()) {
            crate::network::lookup_url_host(&url, port)
                .await
                .map_err(|e| {
                    ToolError::new(
                        ToolErrorCode::Network,
                        crate::http_error::dns(&e, url.as_str()),
                    )
                })?
        } else {
            resolve_public_http_url(&url)
                .await
                .map_err(|e| ToolError::new(ToolErrorCode::Network, e).network_target(&target))?
        };
        let host = host.to_string();
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .connect_timeout(
                connect_timeout
                    .unwrap_or(Duration::from_secs(15))
                    .min(timeout),
            )
            .timeout(timeout)
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|e| {
                ToolError::new(
                    ToolErrorCode::Network,
                    crate::http_error::transport(&e, url.as_str()),
                )
            })?;
        let mut request = client
            .request(method.clone(), url.clone())
            .header(reqwest::header::USER_AGENT, "Harness-Studio/0.1")
            .headers(headers.clone());
        if let Some(body) = &json {
            request = request.json(body);
        }
        if let Some(form) = form {
            request = request.form(form);
        }
        let response = request.send().await.map_err(|e| {
            ToolError::new(
                ToolErrorCode::Network,
                crate::http_error::transport(&e, url.as_str()),
            )
            .network_target(&target)
        })?;
        if !response.status().is_redirection() {
            return Ok((response, url));
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ToolError::new(
                    ToolErrorCode::Network,
                    "redirect response has no valid Location header",
                )
                .network_target(&target)
            })?;
        let next = public_redirect_target(&url, location)
            .map_err(|e| ToolError::new(ToolErrorCode::Network, e).network_target(&target))?;
        if sensitive && next.host_str() != url.host_str() {
            return Err(ToolError::new(
                ToolErrorCode::Network,
                "refused a cross-host redirect for a request containing credentials",
            )
            .network_target(target));
        }
        url = next;
    }
    Err(ToolError::new(ToolErrorCode::Network, "too many redirects").network_target(target(&url)))
}

#[cfg(test)]
pub async fn get_allow_first_private(
    raw: &str,
    timeout: Duration,
) -> Result<(Response, Url), ToolError> {
    request_inner(
        reqwest::Method::GET,
        raw,
        timeout,
        reqwest::header::HeaderMap::new(),
        None,
        None,
        None,
        reqwest::Url::parse(raw).ok().map(|u| target(&u)),
        None,
        None,
    )
    .await
}

pub fn target(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_string(),
    }
}

pub async fn bytes_limited(
    response: Response,
    max: usize,
    target: &str,
) -> Result<Vec<u8>, ToolError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(ToolError::new(
            ToolErrorCode::ResponseTooLarge,
            format!("response exceeds the {max}-byte download limit"),
        )
        .network_target(target));
    }
    use futures_util::StreamExt;
    let url = response.url().to_string();
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            ToolError::new(
                ToolErrorCode::Network,
                crate::http_error::transport(&e, &url),
            )
            .network_target(target)
        })?;
        if bytes.len().saturating_add(chunk.len()) > max {
            return Err(ToolError::new(
                ToolErrorCode::ResponseTooLarge,
                format!("response exceeds the {max}-byte download limit"),
            )
            .network_target(target));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
