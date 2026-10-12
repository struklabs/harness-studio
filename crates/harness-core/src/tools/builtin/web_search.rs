use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::future::BoxFuture;
use reqwest::header::{HeaderMap, HeaderValue};
use scraper::{Html, Selector};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use super::{http, simple_def};
use crate::tools::{
    PermissionClass, SearchProviderKind, ToolCtx, ToolDef, ToolError, ToolErrorCode, ToolOutput,
};

const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct SearchItem {
    title: String,
    url: String,
    snippet: String,
}

/// Provider boundary: the model-facing tool is stable while configuration selects an adapter.
pub trait SearchProvider: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>>;

    /// Name under which the keyless chain remembers that this backend is blocked or down.
    fn cooldown_key(&self) -> Option<&'static str> {
        None
    }
}

pub fn def() -> ToolDef {
    simple_def(
        "web_search",
        "Search the web for people, organizations, news, recent events, and current or unknown facts. Use this before answering instead of guessing or asking the user to rephrase. Returns title, URL, and snippet items using the configured provider; never reveals provider credentials.",
        PermissionClass::Restricted,
        json!({"type":"object","properties":{"query":{"type":"string","description":"Search query."},"count":{"type":"integer","minimum":1,"maximum":10,"description":"Maximum results (default 5)."}},"required":["query"],"additionalProperties":false}),
    )
}

pub fn run(args: Value, ctx: &ToolCtx) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
    Box::pin(async move {
        let query = args["query"].as_str().unwrap_or_default().trim();
        if query.is_empty() {
            return Err(ToolError::invalid("query must not be empty"));
        }
        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 10) as usize;
        let cfg = crate::config::AppSettings::load().tools.web_search;
        let provider: Box<dyn SearchProvider> = match cfg.provider {
            SearchProviderKind::None => {
                return Err(not_configured("no web search provider is configured"))
            }
            SearchProviderKind::Duckduckgo => Box::new(Keyless),
            SearchProviderKind::Searxng if cfg.searxng_url.is_empty() => {
                return Err(not_configured(
                    "SearXNG is selected but its base URL is not configured",
                ))
            }
            SearchProviderKind::Searxng => Box::new(Searxng {
                base: cfg.searxng_url,
            }),
            SearchProviderKind::Brave => Box::new(Brave { key: key("brave")? }),
            SearchProviderKind::Tavily => Box::new(Tavily {
                key: key("tavily")?,
            }),
        };
        let (results, target) = provider.search(query, count, ctx).await?;
        Ok(ToolOutput {
            content: json!({"query":query,"source":target,"results":results}).to_string(),
            network_target: Some(target),
        })
    })
}

/// The model reads this text: say plainly that searching is impossible and what the user must
/// do, so it does not invent an answer or blame the query.
fn not_configured(message: &str) -> ToolError {
    ToolError::new(
        ToolErrorCode::NotConfigured,
        format!("{message}. Web search is unavailable: tell the user to pick a search provider in Harness Studio Settings > Tools > Web search, and do not guess an answer."),
    )
}
fn key(provider: &str) -> Result<String, ToolError> {
    crate::secrets::web_search_key_get(provider)
        .map_err(ToolError::failed)?
        .ok_or_else(|| {
            not_configured(&format!(
                "{provider} is selected but its API key is not saved in the OS keychain"
            ))
        })
}

/// Keep the persisted `duckduckgo` setting while trying independent keyless backends. Measured
/// order (06.10): DuckDuckGo needs an unflagged address, Bing answers plain HTTP clients that send
/// browser-like headers, Mwmbl is an open JSON index (slow, small), DuckDuckGo Lite only helps when
/// the HTML front end failed for another reason (measured: flagged addresses are blocked on both),
/// Wikipedia is the encyclopedia last resort (not a general web search).
struct Keyless;
impl SearchProvider for Keyless {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let languages = wikipedia_languages(q);
            let first = Wikipedia {
                language: languages[0],
            };
            let second = Wikipedia {
                language: languages[1],
            };
            search_fallback(
                &[
                    &Duckduckgo { lite: false },
                    &Bing,
                    &Mwmbl,
                    &Duckduckgo { lite: true },
                    &first,
                    &second,
                ],
                q,
                count,
                ctx,
            )
            .await
        })
    }
}

/// Backends that just failed hard (challenge, refused or dropped connection, timeout, HTTP error)
/// are skipped for a while instead of making every search wait for them again. In memory only;
/// the clock is passed in so tests do not sleep.
#[derive(Default)]
struct Cooldowns(std::sync::Mutex<std::collections::HashMap<&'static str, Instant>>);
impl Cooldowns {
    fn until(&self, key: &'static str) -> Option<Instant> {
        self.0.lock().ok()?.get(key).copied()
    }
    fn start(&self, key: &'static str, until: Instant) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(key, until);
        }
    }
    fn clear(&self, key: &'static str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(key);
        }
    }
}
static COOLDOWNS: std::sync::LazyLock<Cooldowns> = std::sync::LazyLock::new(Cooldowns::default);

const COOLDOWN_BLOCKED: Duration = Duration::from_secs(10 * 60);
const COOLDOWN_RATE_LIMITED: Duration = Duration::from_secs(3 * 60);

/// How long a failed backend rests, or `None` when the failure says nothing about the backend
/// itself (a query it simply had no recognizable results for).
fn cooldown_for(error: &ToolError) -> Option<Duration> {
    if error.message.contains("no recognizable") {
        None
    } else if error.message.contains("HTTP 429") {
        Some(COOLDOWN_RATE_LIMITED)
    } else {
        Some(COOLDOWN_BLOCKED)
    }
}

async fn search_fallback(
    backends: &[&dyn SearchProvider],
    q: &str,
    count: usize,
    ctx: &ToolCtx,
) -> Result<(Vec<SearchItem>, String), ToolError> {
    search_fallback_at(backends, q, count, ctx, &COOLDOWNS, Instant::now()).await
}

async fn search_fallback_at(
    backends: &[&dyn SearchProvider],
    q: &str,
    count: usize,
    ctx: &ToolCtx,
    cooldowns: &Cooldowns,
    now: Instant,
) -> Result<(Vec<SearchItem>, String), ToolError> {
    let mut failures = Vec::new();
    let mut ready: Vec<&dyn SearchProvider> = Vec::new();
    let mut cooling: Vec<(Instant, &dyn SearchProvider)> = Vec::new();
    for backend in backends {
        match backend.cooldown_key().and_then(|key| cooldowns.until(key)) {
            Some(until) if until > now => cooling.push((until, *backend)),
            _ => ready.push(*backend),
        }
    }
    if ready.is_empty() {
        // Everything is resting: searching with the one that rested longest beats not searching.
        if let Some((_, oldest)) = cooling.iter().min_by_key(|(until, _)| *until) {
            ready.push(*oldest);
        }
    } else {
        failures.extend(
            cooling
                .iter()
                .filter_map(|(_, b)| b.cooldown_key())
                .map(|key| format!("{key}: skipped, recently blocked")),
        );
    }
    for backend in ready {
        let key = backend.cooldown_key();
        match backend.search(q, count, ctx).await {
            Ok((items, target)) if !items.is_empty() => {
                if let Some(key) = key {
                    cooldowns.clear(key);
                }
                return Ok((items, target));
            }
            Ok((_, target)) => failures.push(format!("{target}: zero results")),
            Err(e) if matches!(e.code, ToolErrorCode::Cancelled | ToolErrorCode::Timeout) => {
                return Err(e)
            }
            Err(e) => {
                if let (Some(key), Some(rest)) = (key, cooldown_for(&e)) {
                    cooldowns.start(key, now + rest);
                }
                failures.push(e.message)
            }
        }
    }
    Err(ToolError::new(ToolErrorCode::Network, format!(
        "All keyless search backends failed, were blocked, or returned no results ({}). Tell the user no search evidence is available and that free keyless search is often blocked; recommend configuring SearXNG (their own instance), Brave Search or Tavily in Harness Studio Settings > Tools > Web search for reliable results. Do not guess an answer.", failures.join("; "))))
}

const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

fn has_turkish(q: &str) -> bool {
    q.chars().any(|c| "çÇğĞıİöÖşŞüÜ".contains(c))
}

/// Headers a browser form submission carries. Search front ends treat a bare client (no
/// `Accept-Language`, no `Accept`) as a bot.
fn browser_headers(q: &str, referer: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_static(CHROME_UA),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        HeaderValue::from_static(if has_turkish(q) {
            "tr-TR,tr;q=0.9,en;q=0.8"
        } else {
            "en-US,en;q=0.9"
        }),
    );
    if let Some(referer) = referer.and_then(|r| HeaderValue::from_str(r).ok()) {
        headers.insert(reqwest::header::REFERER, referer);
    }
    headers
}

/// Wikimedia and open indexes ask API clients for a descriptive agent with a project URL.
fn api_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    let agent = format!(
        "HarnessStudio/{} (https://github.com/struklabs/harness-studio; desktop app, user-initiated search tool)",
        env!("CARGO_PKG_VERSION")
    );
    if let Ok(value) = HeaderValue::from_str(&agent) {
        headers.insert(reqwest::header::USER_AGENT, value);
    }
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("application/json"),
    );
    headers
}

/// Wait before the single retry: `Retry-After` seconds when sent, otherwise a short default,
/// always bounded so a tool call never stalls.
fn retry_delay(headers: &reqwest::header::HeaderMap) -> std::time::Duration {
    let seconds = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .unwrap_or(1.5);
    std::time::Duration::from_secs_f64(seconds.clamp(0.5, 4.0))
}

/// One request, retried once after a short wait when the server rate limits (429) or is briefly
/// unavailable (503).
async fn send(
    url: &str,
    headers: HeaderMap,
    form: Option<&[(&str, &str)]>,
    limits: Option<(Duration, Duration)>,
    ctx: &ToolCtx,
) -> Result<(reqwest::Response, reqwest::Url), ToolError> {
    let mut attempt = 0;
    loop {
        let (response, final_url) = match form {
            Some(form) => http::post_form(url, ctx, headers.clone(), form, limits).await?,
            None => http::request(reqwest::Method::GET, url, ctx, headers.clone(), None).await?,
        };
        if attempt == 0 && matches!(response.status().as_u16(), 429 | 503) {
            attempt += 1;
            tokio::time::sleep(retry_delay(response.headers())).await;
            continue;
        }
        return Ok((response, final_url));
    }
}

fn element_text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

struct Bing;
impl SearchProvider for Bing {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let mut url = reqwest::Url::parse("https://www.bing.com/search").unwrap();
            url.query_pairs_mut().append_pair("q", q);
            let (response, final_url) =
                send(url.as_str(), browser_headers(q, None), None, None, ctx).await?;
            let target = http::target(&final_url);
            if response.status() != reqwest::StatusCode::OK {
                return Err(ToolError::new(
                    ToolErrorCode::Network,
                    format!("Bing returned HTTP {}", response.status()),
                )
                .network_target(&target));
            }
            let bytes = http::bytes_limited(response, MAX_RESPONSE, &target).await?;
            let items = parse_bing(&String::from_utf8_lossy(&bytes), count)
                .map_err(|e| e.network_target(&target))?;
            Ok((items, target))
        })
    }

    fn cooldown_key(&self) -> Option<&'static str> {
        Some("bing")
    }
}

fn bing_result_url(href: &str) -> Option<String> {
    let base = reqwest::Url::parse("https://www.bing.com/").unwrap();
    let url = base.join(href).ok()?;
    let url = if url
        .host_str()
        .is_some_and(|h| h == "bing.com" || h.ends_with(".bing.com"))
        && url.path() == "/ck/a"
    {
        let (_, encoded) = url.query_pairs().find(|(key, _)| key == "u")?;
        let decoded = URL_SAFE_NO_PAD
            .decode(encoded.strip_prefix("a1")?.trim_end_matches('='))
            .ok()?;
        reqwest::Url::parse(std::str::from_utf8(&decoded).ok()?).ok()?
    } else {
        url
    };
    crate::network::parse_public_http_url(url.as_str())
        .ok()
        .map(Into::into)
}

fn parse_bing(html: &str, count: usize) -> Result<Vec<SearchItem>, ToolError> {
    let document = Html::parse_document(html);
    let challenge =
        Selector::parse("#b_captcha, #captcha, .captcha, #challenge-form, iframe[src*='captcha']")
            .unwrap();
    if document.select(&challenge).next().is_some() {
        return Err(ToolError::new(
            ToolErrorCode::Network,
            "Bing returned a challenge",
        ));
    }
    let result = Selector::parse("li.b_algo").unwrap();
    let link = Selector::parse("h2 a").unwrap();
    let caption = Selector::parse(".b_caption p, .b_lineclamp").unwrap();
    let items: Vec<_> = document
        .select(&result)
        .filter_map(|row| {
            let link = row.select(&link).next()?;
            let title = element_text(link);
            if title.is_empty() {
                return None;
            }
            Some(SearchItem {
                title,
                url: bing_result_url(link.value().attr("href")?)?,
                snippet: row
                    .select(&caption)
                    .next()
                    .map(element_text)
                    .unwrap_or_default(),
            })
        })
        .take(count)
        .collect();
    if items.is_empty() {
        return Err(ToolError::new(
            ToolErrorCode::Network,
            "Bing returned no recognizable search results",
        ));
    }
    Ok(items)
}

fn wikipedia_languages(q: &str) -> [&'static str; 2] {
    if has_turkish(q) {
        ["tr", "en"]
    } else {
        ["en", "tr"]
    }
}

struct Wikipedia {
    language: &'static str,
}
impl SearchProvider for Wikipedia {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let mut url = reqwest::Url::parse(&format!(
                "https://{}.wikipedia.org/w/api.php",
                self.language
            ))
            .unwrap();
            url.query_pairs_mut()
                .append_pair("action", "query")
                .append_pair("list", "search")
                .append_pair("srsearch", q)
                .append_pair("srlimit", &count.to_string())
                .append_pair("format", "json");
            let (response, final_url) = send(url.as_str(), api_headers(), None, None, ctx).await?;
            let target = http::target(&final_url);
            let value = read_json(response, &target).await?;
            let items = parse_wikipedia(&value, self.language, count)
                .map_err(|e| e.network_target(&target))?;
            Ok((items, target))
        })
    }

    fn cooldown_key(&self) -> Option<&'static str> {
        Some(if self.language == "tr" {
            "wikipedia-tr"
        } else {
            "wikipedia-en"
        })
    }
}

fn parse_wikipedia(
    value: &Value,
    language: &str,
    count: usize,
) -> Result<Vec<SearchItem>, ToolError> {
    let rows = value["query"]["search"].as_array().ok_or_else(|| {
        ToolError::new(
            ToolErrorCode::Network,
            "Wikipedia returned no recognizable search response",
        )
    })?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let title = row["title"].as_str()?.trim();
            if title.is_empty() {
                return None;
            }
            let mut url =
                reqwest::Url::parse(&format!("https://{language}.wikipedia.org/")).ok()?;
            url.path_segments_mut()
                .ok()?
                .extend(["wiki", &title.replace(' ', "_")]);
            let fragment = Html::parse_fragment(row["snippet"].as_str().unwrap_or_default());
            Some(SearchItem {
                title: title.into(),
                url: url.into(),
                snippet: element_text(fragment.root_element()),
            })
        })
        .take(count)
        .collect())
}

/// DuckDuckGo drops connections from addresses it flagged; do not let that stall a search
/// (connect, total).
const DUCKDUCKGO_LIMITS: (Duration, Duration) = (Duration::from_secs(5), Duration::from_secs(8));

/// DuckDuckGo's two script-free front ends. Both take the form a browser would POST; an address
/// DuckDuckGo has flagged gets an image challenge (HTTP 202) from either.
struct Duckduckgo {
    lite: bool,
}
impl SearchProvider for Duckduckgo {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let (url, referer) = if self.lite {
                (
                    "https://lite.duckduckgo.com/lite/",
                    "https://lite.duckduckgo.com/",
                )
            } else {
                (
                    "https://html.duckduckgo.com/html/",
                    "https://html.duckduckgo.com/",
                )
            };
            let (response, final_url) = send(
                url,
                browser_headers(q, Some(referer)),
                Some(&[("q", q), ("b", "")]),
                Some(DUCKDUCKGO_LIMITS),
                ctx,
            )
            .await?;
            let target = http::target(&final_url);
            // 202 is DuckDuckGo's own "prove you are human" status; it carries the challenge page.
            let challenged = response.status() == reqwest::StatusCode::ACCEPTED;
            if !challenged && !response.status().is_success() {
                return Err(ToolError::new(
                    ToolErrorCode::Network,
                    format!(
                        "DuckDuckGo returned HTTP {}; try again or choose another search provider",
                        response.status()
                    ),
                )
                .network_target(&target));
            }
            let bytes = http::bytes_limited(response, MAX_RESPONSE, &target).await?;
            let html = String::from_utf8_lossy(&bytes);
            let items = if self.lite {
                parse_duckduckgo_lite(&html, count)
            } else {
                parse_duckduckgo(&html, count)
            }
            .map_err(|e| e.network_target(&target))?;
            Ok((items, target))
        })
    }

    fn cooldown_key(&self) -> Option<&'static str> {
        Some("duckduckgo")
    }
}

fn duckduckgo_result_url(href: &str) -> Option<String> {
    let base = reqwest::Url::parse("https://html.duckduckgo.com/").unwrap();
    let url = base.join(href).ok()?;
    let url = if matches!(
        url.host_str(),
        Some("duckduckgo.com" | "html.duckduckgo.com")
    ) && url.path() == "/l/"
    {
        let (_, destination) = url.query_pairs().find(|(key, _)| key == "uddg")?;
        reqwest::Url::parse(&destination).ok()?
    } else {
        url
    };
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.into())
}

fn parse_duckduckgo(html: &str, count: usize) -> Result<Vec<SearchItem>, ToolError> {
    let document = Html::parse_document(html);
    // Challenges must be errors, never a successful empty search that invites guessing.
    if duckduckgo_challenged(&document) {
        return Err(duckduckgo_challenge_error());
    }
    let result = Selector::parse(".result").unwrap();
    let title = Selector::parse("a.result__a").unwrap();
    let snippet = Selector::parse(".result__snippet").unwrap();
    let text = |element: scraper::ElementRef<'_>| {
        element
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let items: Vec<_> = document
        .select(&result)
        .filter_map(|result| {
            let link = result.select(&title).next()?;
            let url = duckduckgo_result_url(link.value().attr("href")?)?;
            let title = text(link);
            if title.is_empty() {
                return None;
            }
            Some(SearchItem {
                title,
                url,
                snippet: result.select(&snippet).next().map(text).unwrap_or_default(),
            })
        })
        .take(count)
        .collect();
    let no_results = Selector::parse(".no-results").unwrap();
    if items.is_empty() && document.select(&no_results).next().is_none() {
        return Err(ToolError::new(ToolErrorCode::Network,
            "DuckDuckGo returned no recognizable search results; tell the user search is unavailable and do not guess"));
    }
    Ok(items)
}

fn duckduckgo_challenged(document: &Html) -> bool {
    let challenge = Selector::parse("#challenge-form, #anomaly-form, .anomaly-modal").unwrap();
    document.select(&challenge).next().is_some()
}

fn duckduckgo_challenge_error() -> ToolError {
    ToolError::new(ToolErrorCode::Network,
        "DuckDuckGo blocked the search with a challenge; tell the user search is unavailable, do not guess, and try again or choose another provider")
}

/// `lite.duckduckgo.com`: a table where a `result-link` row is followed by its snippet row.
fn parse_duckduckgo_lite(html: &str, count: usize) -> Result<Vec<SearchItem>, ToolError> {
    let document = Html::parse_document(html);
    if duckduckgo_challenged(&document) {
        return Err(duckduckgo_challenge_error());
    }
    let cells = Selector::parse("a.result-link, td.result-snippet").unwrap();
    let mut items: Vec<SearchItem> = Vec::new();
    // A snippet belongs to the link right before it; a rejected link must not lend its snippet
    // to the previous result.
    let mut awaiting_snippet = false;
    for cell in document.select(&cells) {
        if cell.value().name() == "a" {
            let title = element_text(cell);
            let url = cell.value().attr("href").and_then(duckduckgo_result_url);
            awaiting_snippet = false;
            if let (false, Some(url)) = (title.is_empty(), url) {
                items.push(SearchItem {
                    title,
                    url,
                    snippet: String::new(),
                });
                awaiting_snippet = true;
            }
        } else if awaiting_snippet {
            if let Some(last) = items.last_mut() {
                last.snippet = element_text(cell);
            }
            awaiting_snippet = false;
        }
    }
    items.truncate(count);
    let no_results = Selector::parse(".no-results, .no-results-text").unwrap();
    if items.is_empty() && document.select(&no_results).next().is_none() {
        return Err(ToolError::new(ToolErrorCode::Network,
            "DuckDuckGo Lite returned no recognizable search results; tell the user search is unavailable and do not guess"));
    }
    Ok(items)
}

/// Mwmbl: an open-source web index with a keyless JSON API. Small index and slow, so it ranks
/// after the engines that carry the whole web.
struct Mwmbl;
impl SearchProvider for Mwmbl {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let mut url = reqwest::Url::parse("https://mwmbl.org/api/v1/search/").unwrap();
            url.query_pairs_mut().append_pair("s", q);
            let (response, final_url) = send(url.as_str(), api_headers(), None, None, ctx).await?;
            let target = http::target(&final_url);
            let value = read_json(response, &target).await?;
            let items = parse_mwmbl(&value, count).map_err(|e| e.network_target(&target))?;
            Ok((items, target))
        })
    }

    fn cooldown_key(&self) -> Option<&'static str> {
        Some("mwmbl")
    }
}

fn parse_mwmbl(value: &Value, count: usize) -> Result<Vec<SearchItem>, ToolError> {
    let rows = value.as_array().ok_or_else(|| {
        ToolError::new(
            ToolErrorCode::Network,
            "Mwmbl returned no recognizable search response",
        )
    })?;
    // Titles and extracts arrive as runs of `{value, is_bold}` pieces.
    let runs = |v: &Value| -> String {
        let text: String = v
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|run| run["value"].as_str())
            .collect();
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            let title = runs(&row["title"]);
            if title.is_empty() {
                return None;
            }
            let url = crate::network::parse_public_http_url(row["url"].as_str()?).ok()?;
            Some(SearchItem {
                title,
                url: url.into(),
                snippet: runs(&row["extract"]),
            })
        })
        .take(count)
        .collect())
}

struct Searxng {
    base: String,
}
impl SearchProvider for Searxng {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let mut url =
                reqwest::Url::parse(&format!("{}/search", self.base.trim_end_matches('/')))
                    .map_err(|e| ToolError::invalid(format!("invalid SearXNG URL: {e}")))?;
            url.query_pairs_mut()
                .append_pair("q", q)
                .append_pair("format", "json");
            let (response, final_url) = http::get_configured(url.as_str(), ctx).await?;
            let target = http::target(&final_url);
            let v = read_json(response, &target).await?;
            let items = v["results"]
                .as_array()
                .into_iter()
                .flatten()
                .take(count)
                .map(|r| SearchItem {
                    title: s(&r["title"]),
                    url: s(&r["url"]),
                    snippet: s(&r["content"]),
                })
                .collect();
            Ok((items, target))
        })
    }
}
struct Brave {
    key: String,
}
impl SearchProvider for Brave {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let mut url =
                reqwest::Url::parse("https://api.search.brave.com/res/v1/web/search").unwrap();
            url.query_pairs_mut()
                .append_pair("q", q)
                .append_pair("count", &count.to_string());
            let mut headers = HeaderMap::new();
            headers.insert(
                "X-Subscription-Token",
                HeaderValue::from_str(&self.key).map_err(|_| {
                    ToolError::failed("the saved Brave API key is not a valid HTTP header value")
                })?,
            );
            let (response, final_url) =
                http::request(reqwest::Method::GET, url.as_str(), ctx, headers, None).await?;
            let target = http::target(&final_url);
            let v = read_json(response, &target).await?;
            let items = v["web"]["results"]
                .as_array()
                .into_iter()
                .flatten()
                .take(count)
                .map(|r| SearchItem {
                    title: s(&r["title"]),
                    url: s(&r["url"]),
                    snippet: s(&r["description"]),
                })
                .collect();
            Ok((items, target))
        })
    }
}
struct Tavily {
    key: String,
}
impl SearchProvider for Tavily {
    fn search<'a>(
        &'a self,
        q: &'a str,
        count: usize,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
        Box::pin(async move {
            let body = json!({"api_key":self.key,"query":q,"max_results":count,"include_answer":false,"include_raw_content":false});
            let (response, final_url) = http::request(
                reqwest::Method::POST,
                "https://api.tavily.com/search",
                ctx,
                HeaderMap::new(),
                Some(body),
            )
            .await?;
            let target = http::target(&final_url);
            let v = read_json(response, &target).await?;
            let items = v["results"]
                .as_array()
                .into_iter()
                .flatten()
                .take(count)
                .map(|r| SearchItem {
                    title: s(&r["title"]),
                    url: s(&r["url"]),
                    snippet: s(&r["content"]),
                })
                .collect();
            Ok((items, target))
        })
    }
}

async fn read_json(response: reqwest::Response, target: &str) -> Result<Value, ToolError> {
    if !response.status().is_success() {
        return Err(ToolError::new(
            ToolErrorCode::Network,
            format!("search provider returned HTTP {}", response.status()),
        )
        .network_target(target));
    }
    let bytes = http::bytes_limited(response, MAX_RESPONSE, target).await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        ToolError::new(
            ToolErrorCode::Network,
            format!("search provider returned invalid JSON: {e}"),
        )
        .network_target(target)
    })
}
fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn definition_offers_search_before_guessing_or_rephrasing() {
        let wire = def().to_openai();
        let description = wire["function"]["description"].as_str().unwrap();
        for guidance in [
            "people",
            "organizations",
            "news",
            "recent events",
            "current or unknown facts",
            "before answering instead of guessing or asking the user to rephrase",
        ] {
            assert!(description.contains(guidance), "missing: {guidance}");
        }
        assert_eq!(wire["function"]["name"], "web_search");
        assert_eq!(wire["function"]["parameters"]["required"], json!(["query"]));
    }

    #[test]
    fn duckduckgo_html_titles_redirects_snippets_and_limit() {
        let html = include_str!("fixtures/duckduckgo.html");
        let items = parse_duckduckgo(html, 10).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "Rust & tools");
        assert_eq!(items[0].url, "https://example.org/rust?q=one&lang=en");
        assert_eq!(items[0].snippet, "Learn Rust & build tools.");
        assert_eq!(items[1].url, "https://example.org/direct");
        assert_eq!(items[1].snippet, "");
        assert_eq!(items[2].url, "https://example.org/relative");
        assert_eq!(parse_duckduckgo(html, 1).unwrap().len(), 1);
    }

    #[test]
    fn duckduckgo_real_challenge_page_is_an_error_in_both_front_ends() {
        let html = include_str!("fixtures/ddg_challenge.html");
        assert!(html.contains("bots use DuckDuckGo too"));
        for error in [
            parse_duckduckgo(html, 5).unwrap_err(),
            parse_duckduckgo_lite(html, 5).unwrap_err(),
        ] {
            assert_eq!(error.code, ToolErrorCode::Network);
            assert!(error.message.contains("challenge"));
        }
    }

    #[test]
    fn duckduckgo_lite_rows_redirects_snippets_and_rejected_links() {
        let html = include_str!("fixtures/ddg_lite.html");
        let items = parse_duckduckgo_lite(html, 10).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Rust & tools");
        assert_eq!(items[0].url, "https://example.org/rust?q=one&lang=en");
        assert_eq!(items[0].snippet, "Learn Rust & build tools.");
        assert_eq!(items[1].url, "https://example.org/direct");
        assert_eq!(items[1].snippet, "");
        assert_eq!(parse_duckduckgo_lite(html, 1).unwrap().len(), 1);
        assert!(parse_duckduckgo_lite("<p class='no-results'>none</p>", 5)
            .unwrap()
            .is_empty());
        assert!(parse_duckduckgo_lite("<html>changed layout</html>", 5).is_err());
    }

    #[test]
    fn live_bing_page_sample_is_parsed() {
        let items = parse_bing(include_str!("fixtures/bing_live.html"), 10).unwrap();
        assert_eq!(items.len(), 3);
        assert!(items
            .iter()
            .all(|i| !i.title.is_empty() && i.url.starts_with("http") && !i.snippet.is_empty()));
        assert!(items.iter().all(|i| !i.url.contains("bing.com")));
    }

    #[test]
    fn mwmbl_runs_are_joined_and_unsafe_urls_dropped() {
        let value: Value = serde_json::from_str(include_str!("fixtures/mwmbl.json")).unwrap();
        let items = parse_mwmbl(&value, 10).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "The Rust Programming Language Forum");
        assert_eq!(items[0].url, "http://users.rust-lang.org/");
        assert!(items[0].snippet.starts_with("This forum is for help"));
        assert_eq!(parse_mwmbl(&value, 1).unwrap().len(), 1);
        let hostile = json!([
            {"url":"javascript:alert(1)","title":[{"value":"Script"}],"extract":[]},
            {"url":"https://example.org/","title":[],"extract":[]}
        ]);
        assert!(parse_mwmbl(&hostile, 5).unwrap().is_empty());
        assert!(parse_mwmbl(&json!({"error":"x"}), 5).is_err());
    }

    #[test]
    fn retry_delay_honors_retry_after_within_bounds() {
        let with = |value: &str| {
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(reqwest::header::RETRY_AFTER, value.parse().unwrap());
            retry_delay(&h)
        };
        assert_eq!(with("2"), std::time::Duration::from_secs(2));
        assert_eq!(with("120"), std::time::Duration::from_secs(4));
        assert_eq!(with("0"), std::time::Duration::from_millis(500));
        assert_eq!(
            with("Wed, 21 Oct 2026 07:28:00 GMT"),
            std::time::Duration::from_millis(1500)
        );
        assert_eq!(
            retry_delay(&reqwest::header::HeaderMap::new()),
            std::time::Duration::from_millis(1500)
        );
    }

    #[test]
    fn exhausted_chain_recommends_a_configured_provider() {
        let ctx = ToolCtx::new(None);
        let calls = std::sync::Mutex::new(Vec::new());
        let failed = FakeBackend {
            name: "failed",
            outcome: 0,
            calls: &calls,
        };
        let message = futures_executor_block(search_fallback_at(
            &[&failed],
            "q",
            5,
            &ctx,
            &Cooldowns::default(),
            Instant::now(),
        ))
        .unwrap_err()
        .message;
        for word in [
            "SearXNG",
            "Brave",
            "Tavily",
            "Settings > Tools > Web search",
        ] {
            assert!(message.contains(word), "missing {word}");
        }
    }

    fn futures_executor_block<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn backend<'a>(
        name: &'static str,
        outcome: u8,
        calls: &'a std::sync::Mutex<Vec<&'static str>>,
    ) -> FakeBackend<'a> {
        FakeBackend {
            name,
            outcome,
            calls,
        }
    }

    #[tokio::test]
    async fn failed_backend_is_skipped_until_its_cooldown_ends() {
        let ctx = ToolCtx::new(None);
        let cooldowns = Cooldowns::default();
        let t0 = Instant::now();
        let calls = std::sync::Mutex::new(Vec::new());
        let blocked = backend("blocked", 0, &calls);
        let good = backend("good", 2, &calls);
        let chain: [&dyn SearchProvider; 2] = [&blocked, &good];
        let run = |now| search_fallback_at(&chain, "q", 5, &ctx, &cooldowns, now);

        assert_eq!(run(t0).await.unwrap().1, "good");
        assert_eq!(*calls.lock().unwrap(), ["blocked", "good"]);
        calls.lock().unwrap().clear();

        // Inside the window the blocked backend is not even contacted.
        assert_eq!(run(t0 + Duration::from_secs(60)).await.unwrap().1, "good");
        assert_eq!(*calls.lock().unwrap(), ["good"]);
        calls.lock().unwrap().clear();

        // After the window it is tried again.
        assert_eq!(
            run(t0 + COOLDOWN_BLOCKED + Duration::from_secs(1))
                .await
                .unwrap()
                .1,
            "good"
        );
        assert_eq!(*calls.lock().unwrap(), ["blocked", "good"]);
    }

    #[tokio::test]
    async fn rate_limit_rests_shorter_and_success_clears_the_cooldown() {
        let rate_limited = ToolError::new(ToolErrorCode::Network, "Bing returned HTTP 429");
        assert_eq!(cooldown_for(&rate_limited), Some(COOLDOWN_RATE_LIMITED));
        assert!(COOLDOWN_RATE_LIMITED < COOLDOWN_BLOCKED);
        let blocked = ToolError::new(ToolErrorCode::Network, "DuckDuckGo blocked ... challenge");
        assert_eq!(cooldown_for(&blocked), Some(COOLDOWN_BLOCKED));
        // A query without hits says nothing about the backend.
        let layout = ToolError::new(
            ToolErrorCode::Network,
            "Bing returned no recognizable search results",
        );
        assert_eq!(cooldown_for(&layout), None);

        let cooldowns = Cooldowns::default();
        let t0 = Instant::now();
        cooldowns.start("x", t0 + COOLDOWN_BLOCKED);
        cooldowns.clear("x");
        assert!(cooldowns.until("x").is_none());
    }

    #[tokio::test]
    async fn when_every_backend_is_cooling_the_oldest_one_is_tried() {
        let ctx = ToolCtx::new(None);
        let cooldowns = Cooldowns::default();
        let t0 = Instant::now();
        let calls = std::sync::Mutex::new(Vec::new());
        let first = backend("first", 2, &calls);
        let second = backend("second", 2, &calls);
        cooldowns.start("second", t0 + Duration::from_secs(500));
        cooldowns.start("first", t0 + Duration::from_secs(300));
        let chain: [&dyn SearchProvider; 2] = [&second, &first];
        let result = search_fallback_at(&chain, "q", 5, &ctx, &cooldowns, t0).await;
        // `first` entered its cooldown earlier (ends earlier), so it goes first, alone.
        assert_eq!(result.unwrap().1, "first");
        assert_eq!(*calls.lock().unwrap(), ["first"]);
        assert!(cooldowns.until("first").is_none());
    }

    #[test]
    fn duckduckgo_limits_are_short_and_other_backends_keep_the_default() {
        assert_eq!(
            DUCKDUCKGO_LIMITS,
            (Duration::from_secs(5), Duration::from_secs(8))
        );
        assert_eq!(
            Duckduckgo { lite: false }.cooldown_key(),
            Some("duckduckgo")
        );
        assert_eq!(Duckduckgo { lite: true }.cooldown_key(), Some("duckduckgo"));
        assert_eq!(
            Wikipedia { language: "tr" }.cooldown_key(),
            Some("wikipedia-tr")
        );
        assert!(Searxng {
            base: String::new()
        }
        .cooldown_key()
        .is_none());
    }

    #[test]
    fn duckduckgo_empty_search_and_challenge_are_distinct() {
        assert!(
            parse_duckduckgo("<div class='no-results'>No results</div>", 5)
                .unwrap()
                .is_empty()
        );
        for html in [
            "<form id='challenge-form'></form>",
            "<form id='anomaly-form'></form>",
            "<html>changed layout</html>",
        ] {
            assert_eq!(
                parse_duckduckgo(html, 5).unwrap_err().code,
                ToolErrorCode::Network
            );
        }
        for href in [
            "javascript:alert(1)",
            "//duckduckgo.com/l/?uddg=javascript%3Aalert(1)",
            "//duckduckgo.com/l/?other=x",
        ] {
            assert!(duckduckgo_result_url(href).is_none());
        }
    }

    #[tokio::test]
    #[ignore = "requires public keyless search backends"]
    async fn real_keyless_search() {
        let ctx = ToolCtx::new(None);
        let (items, target) = Keyless
            .search("Rust programming language", 3, &ctx)
            .await
            .unwrap();
        assert!(!items.is_empty());
        assert!(items.len() <= 3);
        assert!(items
            .iter()
            .all(|item| !item.title.is_empty() && item.url.starts_with("https://")));
        println!(
            "target={target} results={}",
            serde_json::to_string(&items).unwrap()
        );
    }

    /// Live reliability measurement. Every keyless provider runs alone on the first five queries
    /// (to spare the services) and the full chain on all ten.
    /// Three searches in a row through the real chain: the first one pays for any blocked backend,
    /// the next ones skip it.
    #[tokio::test]
    #[ignore = "live measurement against public keyless search backends"]
    async fn live_keyless_cooldown_three_searches() {
        let ctx = ToolCtx::new(None);
        for q in ["Rust programming language", "Ankara", "Python 3.14 release"] {
            let started = Instant::now();
            let result = Keyless.search(q, 3, &ctx).await;
            let outcome = match result {
                Ok((items, target)) => format!("ok({}) via {target}", items.len()),
                Err(e) => format!("error[{}]", e.message.chars().take(80).collect::<String>()),
            };
            println!(
                "SEARCH {q}: {outcome} in {:.1}s, duckduckgo cooling: {}",
                started.elapsed().as_secs_f64(),
                COOLDOWNS
                    .until("duckduckgo")
                    .is_some_and(|u| u > Instant::now())
            );
        }
    }

    #[tokio::test]
    #[ignore = "live measurement against public keyless search backends"]
    async fn live_keyless_provider_reliability() {
        fn classify(result: Result<(Vec<SearchItem>, String), ToolError>) -> String {
            match result {
                Ok((items, _)) if items.is_empty() => "zero".into(),
                Ok((items, _)) => format!("ok({})", items.len()),
                Err(e) if e.message.to_lowercase().contains("challenge") => "challenge".into(),
                Err(e) => {
                    let message: String = e.message.chars().take(50).collect();
                    format!("error[{message}]")
                }
            }
        }
        async fn pause() {
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        }

        let queries = [
            "Mert Güzeldağ",
            "İzmir hava durumu",
            "Rust programming language",
            "llama.cpp vulkan",
            "Atatürk",
            "Python 3.14 release",
            "İstanbul nüfusu",
            "Tauri 2 tray icon",
            "Ankara",
            "OpenAI GPT",
        ];
        let ctx = ToolCtx::new(None);
        let names = [
            "ddg-html",
            "ddg-lite",
            "bing",
            "mwmbl",
            "wikipedia",
            "chain",
        ];
        let mut rows: Vec<(&str, Vec<String>)> = Vec::new();
        for (index, q) in queries.into_iter().enumerate() {
            let mut cells = Vec::new();
            if index < 5 {
                cells.push(classify(
                    Duckduckgo { lite: false }.search(q, 5, &ctx).await,
                ));
                pause().await;
                cells.push(classify(Duckduckgo { lite: true }.search(q, 5, &ctx).await));
                pause().await;
                cells.push(classify(Bing.search(q, 5, &ctx).await));
                pause().await;
                cells.push(classify(Mwmbl.search(q, 5, &ctx).await));
                pause().await;
                // Wikipedia alone: preferred language first, then the other when it yields nothing.
                let mut wiki = String::new();
                for language in wikipedia_languages(q) {
                    let outcome = classify(Wikipedia { language }.search(q, 5, &ctx).await);
                    wiki = format!("{language}:{outcome}");
                    if outcome.starts_with("ok") {
                        break;
                    }
                }
                cells.push(wiki);
                pause().await;
            } else {
                cells.extend(std::iter::repeat_n("-".to_string(), 5));
            }
            cells.push(match Keyless.search(q, 5, &ctx).await {
                Ok((items, target)) if !items.is_empty() => {
                    format!("ok({}) via {target}", items.len())
                }
                // The chain error lists every backend failure; do not classify it by keyword.
                Err(e) => format!("error[{}]", e.message.chars().take(40).collect::<String>()),
                Ok(_) => "zero".into(),
            });
            pause().await;
            rows.push((q, cells));
        }

        println!("\n{:<26} | {}", "query", names.join(" | "));
        for (q, cells) in &rows {
            println!("{q:<26} | {}", cells.join(" | "));
        }
        for (column, name) in names.iter().enumerate() {
            let (mut ok, mut challenge, mut zero, mut error) = (0, 0, 0, 0);
            for (_, cells) in &rows {
                let cell = cells[column].as_str();
                let cell = cell
                    .split_once(':')
                    .filter(|_| column == 4)
                    .map_or(cell, |c| c.1);
                if cell == "-" {
                } else if cell.starts_with("ok") {
                    ok += 1;
                } else if cell.starts_with("challenge") {
                    challenge += 1;
                } else if cell.starts_with("zero") {
                    zero += 1;
                } else {
                    error += 1;
                }
            }
            println!("TOTAL {name}: ok={ok} challenge={challenge} zero={zero} error={error}");
        }
    }

    #[test]
    fn bing_titles_snippets_redirects_and_limit() {
        let items = parse_bing(include_str!("fixtures/bing.html"), 10).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Rust & tools");
        assert_eq!(items[0].url, "https://example.org/rust?q=one&lang=en");
        assert_eq!(items[0].snippet, "Learn Rust & build tools.");
        assert_eq!(items[1].snippet, "Second snippet");
        assert_eq!(
            parse_bing(include_str!("fixtures/bing.html"), 1)
                .unwrap()
                .len(),
            1
        );
        for html in [
            "<div id='b_captcha'></div>",
            "<html>changed layout</html>",
            "<ol></ol>",
        ] {
            assert!(parse_bing(html, 5).is_err());
        }
        for href in [
            "javascript:alert(1)",
            "/ck/a?u=a1%%%",
            "/ck/a?u=bad",
            "https://user:pass@example.org/",
        ] {
            assert!(bing_result_url(href).is_none());
        }
    }

    #[test]
    fn wikipedia_titles_urls_snippets_and_language_order() {
        let value: Value = serde_json::from_str(include_str!("fixtures/wikipedia.json")).unwrap();
        let items = parse_wikipedia(&value, "tr", 5).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].url,
            "https://tr.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert_eq!(items[0].snippet, "Rust & tools language.");
        assert!(items[1].url.contains("%C3%BC"));
        assert_eq!(parse_wikipedia(&value, "en", 1).unwrap().len(), 1);
        assert!(parse_wikipedia(&json!({"query":{"search":[]}}), "en", 5)
            .unwrap()
            .is_empty());
        assert!(parse_wikipedia(&json!({"error":{"code":"blocked"}}), "en", 5).is_err());
        assert_eq!(
            wikipedia_languages("Mert G\u{fc}zelda\u{11f}"),
            ["tr", "en"]
        );
        assert_eq!(wikipedia_languages("Rust"), ["en", "tr"]);
    }

    struct FakeBackend<'a> {
        name: &'static str,
        outcome: u8,
        calls: &'a std::sync::Mutex<Vec<&'static str>>,
    }
    impl SearchProvider for FakeBackend<'_> {
        fn cooldown_key(&self) -> Option<&'static str> {
            Some(self.name)
        }
        fn search<'a>(
            &'a self,
            _: &'a str,
            _: usize,
            _: &'a ToolCtx,
        ) -> BoxFuture<'a, Result<(Vec<SearchItem>, String), ToolError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(self.name);
                match self.outcome {
                    0 => Err(ToolError::new(
                        ToolErrorCode::Network,
                        "challenge / HTTP error",
                    )),
                    1 => Ok((vec![], self.name.into())),
                    2 => Ok((
                        vec![SearchItem {
                            title: "Result".into(),
                            url: "https://example.org/".into(),
                            snippet: "Evidence".into(),
                        }],
                        self.name.into(),
                    )),
                    _ => Err(ToolError::new(ToolErrorCode::Cancelled, "cancelled")),
                }
            })
        }
    }

    #[tokio::test]
    async fn fallback_order_errors_empty_success_and_exhaustion() {
        let ctx = ToolCtx::new(None);
        for winning in 0..4 {
            let calls = std::sync::Mutex::new(Vec::new());
            let names = ["duckduckgo", "bing", "wikipedia-tr", "wikipedia-en"];
            let backends: Vec<_> = names
                .iter()
                .enumerate()
                .map(|(i, name)| FakeBackend {
                    name,
                    outcome: if i == winning { 2 } else { (i % 2) as u8 },
                    calls: &calls,
                })
                .collect();
            let refs: Vec<&dyn SearchProvider> =
                backends.iter().map(|b| b as &dyn SearchProvider).collect();
            assert_eq!(
                search_fallback_at(
                    &refs,
                    "query",
                    5,
                    &ctx,
                    &Cooldowns::default(),
                    Instant::now()
                )
                .await
                .unwrap()
                .1,
                names[winning]
            );
            assert_eq!(*calls.lock().unwrap(), names[..=winning]);
        }
        let calls = std::sync::Mutex::new(Vec::new());
        let empty = FakeBackend {
            name: "empty",
            outcome: 1,
            calls: &calls,
        };
        let failed = FakeBackend {
            name: "failed",
            outcome: 0,
            calls: &calls,
        };
        let cancelled = FakeBackend {
            name: "cancelled",
            outcome: 3,
            calls: &calls,
        };
        assert!(search_fallback_at(
            &[&failed, &empty],
            "query",
            5,
            &ctx,
            &Cooldowns::default(),
            Instant::now()
        )
        .await
        .unwrap_err()
        .message
        .contains("Do not guess"));
        calls.lock().unwrap().clear();
        assert_eq!(
            search_fallback_at(
                &[&cancelled, &empty],
                "query",
                5,
                &ctx,
                &Cooldowns::default(),
                Instant::now()
            )
            .await
            .unwrap_err()
            .code,
            ToolErrorCode::Cancelled
        );
        assert_eq!(*calls.lock().unwrap(), vec!["cancelled"]);
    }

    fn serve(response: String) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn searxng_on_the_users_own_host_is_reachable_and_parsed() {
        let body = r#"{"results":[{"title":"One","url":"https://example.org/1","content":"first"},{"title":"Two","url":"https://example.org/2","content":"second"}]}"#;
        let base = serve(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        let ctx = ToolCtx::new(None);
        let (items, target) = Searxng { base }.search("rust", 1, &ctx).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "One");
        assert_eq!(items[0].snippet, "first");
        assert!(target.starts_with("127.0.0.1:"));
    }

    #[tokio::test]
    async fn searxng_redirect_to_another_local_port_is_refused() {
        let other = serve("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".into());
        let base = serve(format!(
            "HTTP/1.1 302 Found\r\nLocation: {other}/search\r\nContent-Length: 0\r\n\r\n"
        ));
        let ctx = ToolCtx::new(None);
        let e = Searxng { base }
            .search("rust", 1, &ctx)
            .await
            .map(|_| ())
            .unwrap_err();
        assert!(e.message.contains("private or local"), "{}", e.message);
    }
}
