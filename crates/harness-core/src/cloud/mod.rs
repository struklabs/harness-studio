//! Cloud providers with the user's own API key (BYOK): OpenAI, Gemini, Anthropic, OpenRouter
//! and custom OpenAI-compatible endpoints. Their chat models are served through the gateway as
//! `cloud:<provider>/<model>`.
//!
//! The provider list is `config/cloud_providers.json`; keys live only in the OS keychain
//! ([`secrets::cloud_key_get`]). Separate from Harness Link (another Harness gateway) and from
//! client integrations (apps that use Harness); only the connection test, error and secret
//! helpers are shared.

use crate::{
    domain::{ModelDefinition, ModelRef},
    error_key::err,
    integrations::{conntest, launch::WireApi},
    paths, secrets,
    store::{Envelope, JsonStore, Schema},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const MODELS_LIMIT: usize = 16 * 1024 * 1024;
const QUOTA_LIMIT: usize = 64 * 1024;
const REFRESH_EVERY: Duration = Duration::from_secs(15 * 60);
/// The `anthropic-version` the gateway sends when the client names none.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Kind {
    Openai,
    Gemini,
    Anthropic,
    Openrouter,
    /// A custom OpenAI-compatible endpoint (Chat Completions, Responses, embeddings).
    Custom,
    /// A custom Anthropic-compatible endpoint (Messages).
    #[serde(rename = "custom_anthropic")]
    CustomAnthropic,
}

impl Kind {
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Kind::Openai => Some("https://api.openai.com/v1"),
            Kind::Gemini => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            Kind::Anthropic => Some("https://api.anthropic.com/v1"),
            Kind::Openrouter => Some("https://openrouter.ai/api/v1"),
            Kind::Custom | Kind::CustomAnthropic => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Openai => "OpenAI",
            Kind::Gemini => "Gemini",
            Kind::Anthropic => "Anthropic",
            Kind::Openrouter => "OpenRouter",
            Kind::Custom => "Custom",
            Kind::CustomAnthropic => "Custom (Anthropic)",
        }
    }

    /// Custom endpoints (a local or self-hosted server) may run without a key.
    pub fn key_required(self) -> bool {
        !matches!(self, Kind::Custom | Kind::CustomAnthropic)
    }

    /// Speaks the Anthropic Messages protocol (`x-api-key`, `anthropic-version`).
    pub fn anthropic_wire(self) -> bool {
        matches!(self, Kind::Anthropic | Kind::CustomAnthropic)
    }

    /// Whether the provider serves a gateway path (`/v1/...`) through the gateway. Chat
    /// completions go to every provider; the others only to the providers that speak them.
    pub fn accepts(self, path: &str) -> bool {
        match path {
            "/v1/chat/completions" => true,
            "/v1/responses" => matches!(self, Kind::Openai | Kind::Openrouter | Kind::Custom),
            "/v1/messages" | "/v1/messages/count_tokens" => self.anthropic_wire(),
            "/v1/embeddings" => !self.anthropic_wire(),
            _ => false,
        }
    }

    /// OpenRouter lists its models without a key.
    fn public_models(self) -> bool {
        matches!(
            self,
            Kind::Openrouter | Kind::Custom | Kind::CustomAnthropic
        )
    }
}

/// A price the user set for a model, USD per million tokens; `None` keeps the provider's own.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPrice {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
}

impl ModelPrice {
    fn valid(&self) -> bool {
        [self.input, self.output]
            .into_iter()
            .flatten()
            .all(|p| p.is_finite() && (0.0..=1_000_000.0).contains(&p))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CloudProvider {
    pub id: String,
    /// Short name in model ids (`cloud:<name>/<model>`).
    pub name: String,
    pub kind: Kind,
    /// Base URL including the API version (`https://api.openai.com/v1`).
    pub base_url: String,
    pub enabled: bool,
    #[serde(default)]
    pub key_set: bool,
    /// Names of the extra request headers a custom endpoint needs; the values are kept with the
    /// API key in the system keychain and never returned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    /// Prices the user set per provider model id (cost tracking).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prices: BTreeMap<String, ModelPrice>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum State {
    Online,
    #[default]
    Offline,
    Unauthorized,
    /// The provider needs a key and none is saved.
    KeyMissing,
}

/// Spending information where the API exposes it (OpenRouter credits, in USD).
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Quota {
    pub limit: Option<f64>,
    pub usage: Option<f64>,
    pub remaining: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProviderStatus {
    pub state: State,
    pub model_count: usize,
    pub last_error: Option<String>,
    pub last_ok: Option<u64>,
    pub latency_ms: Option<u64>,
    pub quota: Option<Quota>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: CloudProvider,
    #[serde(flatten)]
    pub status: ProviderStatus,
    pub label: &'static str,
}

/// A model and what the provider says about it; `None` = not exposed by its API.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CloudModel {
    /// The provider's model id; [`ModelView::model`] carries it (`id` there is the gateway id).
    #[serde(skip_serializing)]
    pub id: String,
    pub display_name: Option<String>,
    pub context_length: Option<u64>,
    /// USD per million input tokens.
    pub input_price: Option<f64>,
    /// USD per million output tokens.
    pub output_price: Option<f64>,
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    /// Reasoning (thinking) model.
    pub reasoning: Option<bool>,
    /// The prices above are the user's own, not the provider's.
    #[serde(default)]
    pub price_custom: bool,
    /// An embedding model (`/v1/embeddings`); listed apart from the chat models.
    #[serde(default)]
    pub embedding: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelView {
    pub id: String,
    pub model: String,
    pub provider: String,
    pub kind: Kind,
    pub label: &'static str,
    pub state: State,
    #[serde(flatten)]
    pub info: CloudModel,
}

pub const SCHEMA: Schema = Schema {
    file: "cloud_providers.json",
    current: 1,
    envelope: Envelope::Wrapped("providers"),
    migrations: &[],
};

fn file() -> PathBuf {
    paths::config_dir().join("cloud_providers.json")
}

fn load() -> Vec<CloudProvider> {
    JsonStore::sensitive(file())
        .load_versioned::<Vec<CloudProvider>>(&SCHEMA)
        .ok()
        .flatten()
        .map(|v| v.data)
        .unwrap_or_default()
}

fn save(list: &[CloudProvider]) -> Result<(), String> {
    JsonStore::sensitive(file()).save_versioned(&SCHEMA, &list.to_vec())
}

fn provider_store() -> &'static Mutex<Option<Vec<CloudProvider>>> {
    static STORE: OnceLock<Mutex<Option<Vec<CloudProvider>>>> = OnceLock::new();
    STORE.get_or_init(Default::default)
}

/// Read once from disk, then only changed by add/update/remove: request paths never touch it.
fn providers() -> Vec<CloudProvider> {
    provider_store()
        .lock()
        .unwrap()
        .get_or_insert_with(load)
        .clone()
}

fn set_providers(list: Vec<CloudProvider>) {
    *provider_store().lock().unwrap() = Some(list);
}

#[derive(Clone, Default)]
struct Cached {
    status: ProviderStatus,
    models: Vec<CloudModel>,
    revision: u64,
}

fn cache() -> &'static Mutex<HashMap<String, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn mutations() -> &'static tokio::sync::Mutex<()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &LOCK
}

pub fn valid_name(name: &str) -> bool {
    crate::link::valid_name(name)
}

pub fn model_id(provider: &str, model: &str) -> String {
    format!("cloud:{provider}/{model}")
}

pub fn parse_model(id: &str) -> Option<(&str, &str)> {
    let (provider, model) = id.strip_prefix("cloud:")?.split_once('/')?;
    (valid_name(provider) && !model.is_empty() && !model.chars().any(char::is_control))
        .then_some((provider, model))
}

/// HTTPS, or HTTP to this machine or a private network (a local OpenAI-compatible server).
/// A custom URL without a path gets `/v1`.
pub fn validate_url(raw: &str, kind: Kind) -> Result<String, String> {
    let raw = raw.trim();
    let raw = if raw.is_empty() {
        kind.default_base_url()
            .ok_or_else(|| String::from(err("cloud.invalid_url")))?
    } else {
        raw
    };
    let mut url = reqwest::Url::parse(raw).map_err(|_| String::from(err("cloud.invalid_url")))?;
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(err("cloud.invalid_url").into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| String::from(err("cloud.invalid_url")))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    match url.scheme() {
        "https" => {}
        "http" => {
            let local =
                host == "localhost" || host.parse::<IpAddr>().is_ok_and(crate::link::private_ip);
            if !local {
                return Err(err("cloud.insecure_not_allowed").into());
            }
        }
        _ => return Err(err("cloud.invalid_url").into()),
    }
    let path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() && matches!(kind, Kind::Custom | Kind::CustomAnthropic) {
        url.set_path("/v1");
    } else {
        url.set_path(&path);
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_client(0).expect("cloud client"))
}

/// Never follows redirects: the key must only reach the configured host.
fn build_client(idle: u32) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if idle > 0 {
        builder = builder.read_timeout(Duration::from_secs(u64::from(idle)));
    }
    builder.build().map_err(|_| err("cloud.unreachable").into())
}

/// The client for proxied requests (`idle`: the gateway's request timeout, 0 = none).
pub fn client_for(idle: u32) -> Result<reqwest::Client, String> {
    static CLIENTS: OnceLock<Mutex<HashMap<u32, reqwest::Client>>> = OnceLock::new();
    let mut map = CLIENTS.get_or_init(Default::default).lock().unwrap();
    if let Some(client) = map.get(&idle) {
        return Ok(client.clone());
    }
    let client = build_client(idle)?;
    map.insert(idle, client.clone());
    Ok(client)
}

/// Only a classified key leaves this boundary; error text could echo credentials.
pub fn transport_error(error: reqwest::Error, key: Option<&str>) -> String {
    let error = error.without_url();
    let classified = crate::http_error::transport(&error, "");
    let key_name = match crate::error_key::parse(&classified).map(|(k, _)| k) {
        Some("http.tls_failed") => "cloud.tls_failed",
        Some("http.dns_failed") => "cloud.dns_failed",
        _ if error.is_timeout() => "cloud.timeout",
        _ => "cloud.unreachable",
    };
    crate::integrations::launch::redact_text(
        &String::from(err(key_name)),
        key.into_iter().collect(),
    )
}

/// The error key for a non-success answer of the provider.
pub fn status_error(status: u16) -> &'static str {
    match status {
        401 | 403 => "cloud.unauthorized",
        402 => "cloud.payment_required",
        404 => "cloud.not_found_upstream",
        429 => "cloud.rate_limited",
        500..=599 => "cloud.server_error",
        _ => "cloud.unreachable",
    }
}

/// `Retry-After` as seconds (the HTTP-date form is not used by the providers).
pub fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|s| (1..=86_400).contains(s))
}

/// A provider says the account is out of credit or quota (not just a momentary rate limit).
fn quota_exhausted(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&body[..body.len().min(8 * 1024)]).to_ascii_lowercase();
    [
        "insufficient_quota",
        "exceeded your current quota",
        "billing",
        "credit balance",
        "resource_exhausted",
        "out of credits",
    ]
    .iter()
    .any(|m| text.contains(m))
}

/// The keyed error for a non-success answer: 401/403 key rejected, 402 and quota-exhausted 429
/// payment or credits, other 429 and 5xx retryable (with the provider's `Retry-After` when it
/// sent one), 404 wrong address.
pub fn http_error(status: u16, retry_after: Option<u64>, body: &[u8]) -> String {
    let key = match (status, retry_after) {
        (429, _) if quota_exhausted(body) => "cloud.quota_exceeded",
        (429, Some(_)) => "cloud.rate_limited_retry",
        (500..=599, Some(_)) => "cloud.server_error_retry",
        _ => status_error(status),
    };
    let mut e = err(key).num("status", status);
    if let Some(secs) = retry_after {
        e = e.num("retry_after", secs);
    }
    e.into()
}

fn auth(req: reqwest::RequestBuilder, kind: Kind, key: Option<&str>) -> reqwest::RequestBuilder {
    match (kind.anthropic_wire(), key) {
        (true, Some(key)) => req
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION),
        (true, None) => req.header("anthropic-version", ANTHROPIC_VERSION),
        (false, Some(key)) => req.bearer_auth(key),
        (false, None) => req,
    }
}

/// Request headers a custom endpoint needs besides the key. Authentication, framing and routing
/// headers stay under Harness' control.
const RESERVED_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "host",
    "content-length",
    "content-type",
    "connection",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "proxy-authorization",
    "cookie",
];
const MAX_HEADERS: usize = 16;

/// One extra header as typed in Advanced settings.
#[derive(Clone, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HeaderInput {
    pub name: String,
    pub value: String,
}

fn check_headers(input: &[HeaderInput]) -> Result<Vec<(String, String)>, String> {
    if input.len() > MAX_HEADERS {
        return Err(err("cloud.invalid_headers").into());
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for h in input {
        let name = h.name.trim().to_ascii_lowercase();
        let value = h.value.trim().to_string();
        let ok = reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok()
            && reqwest::header::HeaderValue::from_str(&value).is_ok()
            && !RESERVED_HEADERS.contains(&name.as_str())
            && !name.starts_with("proxy-")
            && !name.starts_with("x-harness-")
            && !value.is_empty()
            && value.len() <= 2048;
        if !ok || out.iter().any(|(n, _)| *n == name) {
            return Err(err("cloud.invalid_headers").into());
        }
        out.push((name, value));
    }
    Ok(out)
}

fn headers_account(id: &str) -> String {
    format!("{id}.headers")
}

type HeaderList = Vec<(String, String)>;

fn headers_cache() -> &'static Mutex<HashMap<String, HeaderList>> {
    static CACHE: OnceLock<Mutex<HashMap<String, HeaderList>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn read_headers(id: &str) -> Result<Vec<(String, String)>, String> {
    Ok(secrets::cloud_key_get(&headers_account(id))?
        .and_then(|json| serde_json::from_str::<Vec<(String, String)>>(&json).ok())
        .unwrap_or_default())
}

/// The extra headers of a provider (read from the keychain once, then cached).
pub fn extra_headers_of(provider: &CloudProvider) -> Vec<(String, String)> {
    if provider.headers.is_empty() {
        return Vec::new();
    }
    if let Some(h) = headers_cache().lock().unwrap().get(&provider.id) {
        return h.clone();
    }
    let loaded = read_headers(&provider.id).unwrap_or_default();
    headers_cache()
        .lock()
        .unwrap()
        .insert(provider.id.clone(), loaded.clone());
    loaded
}

/// The extra headers for a proxied request to the `cloud:` model `id`.
pub fn extra_headers(id: &str) -> Vec<(String, String)> {
    let Some((name, _)) = parse_model(id) else {
        return Vec::new();
    };
    providers()
        .into_iter()
        .find(|p| p.enabled && p.name == name)
        .map(|p| extra_headers_of(&p))
        .unwrap_or_default()
}

fn with_headers(
    mut req: reqwest::RequestBuilder,
    headers: &[(String, String)],
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        if let Ok(mut value) = reqwest::header::HeaderValue::from_str(value) {
            value.set_sensitive(true);
            req = req.header(name.as_str(), value);
        }
    }
    req
}

async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err(err("cloud.invalid_response").into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| String::from(err("cloud.unreachable")))?
    {
        if body.len() + chunk.len() > limit {
            return Err(err("cloud.invalid_response").into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn clean(s: &str) -> Option<String> {
    (!s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)).then(|| s.to_string())
}

fn per_million(v: &Value) -> Option<f64> {
    let per_token = match v {
        Value::String(s) => s.parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    (per_token >= 0.0).then_some((per_token * 1_000_000.0 * 1e6).round() / 1e6)
}

/// OpenAI also lists embedding, audio, image and moderation models; only chat models are
/// offered.
fn openai_chat_model(id: &str) -> bool {
    ![
        "embedding",
        "tts",
        "whisper",
        "dall-e",
        "moderation",
        "transcribe",
        "image",
        "davinci",
        "babbage",
        "realtime",
        "audio",
        "search",
    ]
    .iter()
    .any(|s| id.contains(s))
}

/// Embedding models: listed for OpenAI, Gemini and custom endpoints (Anthropic has none).
fn embedding_model(kind: Kind, id: &str) -> bool {
    match kind {
        Kind::Openai | Kind::Gemini => id.contains("embedding"),
        Kind::Custom => id.contains("embed"),
        Kind::Openrouter | Kind::Anthropic | Kind::CustomAnthropic => false,
    }
}

/// The models of a `/models` answer, with what each provider exposes about them.
pub fn parse_models(kind: Kind, body: &Value) -> Result<Vec<CloudModel>, String> {
    let data = body["data"]
        .as_array()
        .or_else(|| body["models"].as_array())
        .ok_or_else(|| String::from(err("cloud.invalid_response")))?;
    let mut models: Vec<CloudModel> = data
        .iter()
        .filter_map(|v| {
            let raw = v["id"].as_str().or_else(|| v["name"].as_str())?;
            let id = match kind {
                Kind::Gemini => raw.strip_prefix("models/").unwrap_or(raw),
                _ => raw,
            };
            let id = clean(id)?;
            let embedding = embedding_model(kind, &id);
            if !embedding && kind == Kind::Openai && !openai_chat_model(&id) {
                return None;
            }
            if kind == Kind::Gemini && id.contains("aqa") {
                return None;
            }
            let mut model = CloudModel {
                id,
                display_name: v["display_name"]
                    .as_str()
                    .or_else(|| v["name"].as_str().filter(|_| v["id"].is_string()))
                    .and_then(clean),
                context_length: v["context_length"]
                    .as_u64()
                    .or_else(|| v["context_window"].as_u64())
                    .or_else(|| v["max_input_tokens"].as_u64())
                    .or_else(|| v["inputTokenLimit"].as_u64()),
                // Gemini's native list says `thinking`; OpenAI-style lists say nothing.
                reasoning: v["thinking"].as_bool(),
                embedding,
                ..Default::default()
            };
            if kind.anthropic_wire() {
                // Anthropic lists what a model can do (`capabilities`).
                if let Some(vision) = v["capabilities"]["image_input"]["supported"].as_bool() {
                    model.vision = Some(vision);
                }
                if let Some(thinking) = v["capabilities"]["thinking"]["supported"].as_bool() {
                    model.reasoning = Some(thinking);
                }
            }
            if kind == Kind::Openrouter {
                model.reasoning = v["supported_parameters"].as_array().map(|params| {
                    params
                        .iter()
                        .any(|p| p == "reasoning" || p == "include_reasoning")
                });
                model.input_price = per_million(&v["pricing"]["prompt"]);
                model.output_price = per_million(&v["pricing"]["completion"]);
                if let Some(params) = v["supported_parameters"].as_array() {
                    model.tools = Some(params.iter().any(|p| p == "tools"));
                }
                if let Some(input) = v["architecture"]["input_modalities"].as_array() {
                    model.vision = Some(input.iter().any(|m| m == "image"));
                }
            }
            Some(model)
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

pub fn parse_quota(body: &Value) -> Option<Quota> {
    let data = body.get("data")?;
    let quota = Quota {
        limit: data["limit"].as_f64(),
        usage: data["usage"].as_f64(),
        remaining: data["limit_remaining"].as_f64(),
    };
    (quota != Quota::default()).then_some(quota)
}

fn key_of(provider: &CloudProvider) -> Result<Option<String>, String> {
    if !provider.key_set {
        return Ok(None);
    }
    secrets::cloud_key_get(&provider.id)
}

async fn discover(
    provider: &CloudProvider,
    key: Option<&str>,
    headers: &[(String, String)],
) -> Result<(Vec<CloudModel>, Option<Quota>), String> {
    let base = provider.base_url.as_str();
    let url = if provider.kind.anthropic_wire() {
        format!("{base}/models?limit=1000")
    } else {
        format!("{base}/models")
    };
    let response = with_headers(auth(client().get(url), provider.kind, key), headers)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| transport_error(e, key))?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let retry = retry_after_secs(response.headers());
        let body = read_limited(response, QUOTA_LIMIT)
            .await
            .unwrap_or_default();
        return Err(http_error(status, retry, &body));
    }
    let body = read_limited(response, MODELS_LIMIT).await?;
    let value: Value =
        serde_json::from_slice(&body).map_err(|_| String::from(err("cloud.invalid_response")))?;
    let models = parse_models(provider.kind, &value)?;
    let quota = match (provider.kind, key) {
        (Kind::Openrouter, Some(key)) => {
            match auth(
                client().get(format!("{base}/key")),
                provider.kind,
                Some(key),
            )
            .timeout(Duration::from_secs(10))
            .send()
            .await
            {
                Ok(r) if r.status().is_success() => read_limited(r, QUOTA_LIMIT)
                    .await
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .and_then(|v| parse_quota(&v)),
                _ => None,
            }
        }
        _ => None,
    };
    Ok((models, quota))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Checks the provider (its model list) and caches the result.
pub async fn refresh(provider: &CloudProvider) -> ProviderStatus {
    let revision = cache()
        .lock()
        .unwrap()
        .entry(provider.id.clone())
        .or_default()
        .revision;
    let started = Instant::now();
    let result = match key_of(provider) {
        Ok(None) if provider.kind.key_required() && !provider.kind.public_models() => {
            Err(String::from(err("cloud.key_missing")))
        }
        Ok(key) => discover(provider, key.as_deref(), &extra_headers_of(provider))
            .await
            .map(|found| (found, key.is_some())),
        Err(e) => Err(e),
    };
    let latency = started.elapsed().as_millis() as u64;
    // An edit or removal while the check was in flight must not restore stale state.
    if !providers().contains(provider) {
        return ProviderStatus::default();
    }
    let mut cached = cache().lock().unwrap();
    let c = cached.entry(provider.id.clone()).or_default();
    if c.revision != revision {
        return c.status.clone();
    }
    match result {
        Ok(((models, quota), has_key)) => {
            c.status.state = if has_key || !provider.kind.key_required() {
                State::Online
            } else {
                State::KeyMissing
            };
            c.status.model_count = models.len();
            c.status.last_error = None;
            c.status.last_ok = Some(now_secs());
            c.status.latency_ms = Some(latency);
            c.status.quota = quota;
            c.models = models;
        }
        Err(error) => {
            c.status.state = match crate::error_key::parse(&error).map(|(k, _)| k) {
                Some("cloud.unauthorized") => State::Unauthorized,
                Some("cloud.key_missing") => State::KeyMissing,
                _ => State::Offline,
            };
            c.status.latency_ms = None;
            c.status.last_error = Some(error);
        }
    }
    c.status.clone()
}

pub async fn refresh_all() {
    futures_util::future::join_all(
        providers()
            .into_iter()
            .filter(|p| p.enabled)
            .map(|p| async move { refresh(&p).await }),
    )
    .await;
}

pub async fn refresh_loop() {
    let mut tick = tokio::time::interval(REFRESH_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        refresh_all().await;
    }
}

pub fn list() -> Vec<ProviderView> {
    let list = providers();
    let cached = cache().lock().unwrap();
    list.into_iter()
        .map(|provider| {
            let status = cached
                .get(&provider.id)
                .map(|c| c.status.clone())
                .unwrap_or_default();
            ProviderView {
                label: provider.kind.label(),
                provider,
                status,
            }
        })
        .collect()
}

/// The chat models the gateway serves: those of enabled providers that answered with a usable
/// key.
pub fn models() -> Vec<ModelView> {
    all_models()
        .into_iter()
        .filter(|m| !m.info.embedding)
        .collect()
}

/// The embedding models of those providers (`/v1/embeddings`, knowledge bases).
pub fn embedding_models() -> Vec<ModelView> {
    all_models()
        .into_iter()
        .filter(|m| m.info.embedding)
        .collect()
}

/// Chat and embedding models.
pub fn all_models() -> Vec<ModelView> {
    let list = providers();
    let cached = cache().lock().unwrap();
    list.into_iter()
        .filter(|p| p.enabled)
        .flat_map(|p| {
            let Some(c) = cached.get(&p.id) else {
                return Vec::new();
            };
            if c.status.state == State::KeyMissing {
                return Vec::new();
            }
            c.models
                .iter()
                .map(|m| ModelView {
                    id: model_id(&p.name, &m.id),
                    model: m.id.clone(),
                    provider: p.name.clone(),
                    kind: p.kind,
                    label: p.kind.label(),
                    state: c.status.state,
                    info: priced(m, p.prices.get(&m.id)),
                })
                .collect()
        })
        .collect()
}

/// The model with the user's prices over the provider's.
fn priced(model: &CloudModel, custom: Option<&ModelPrice>) -> CloudModel {
    let mut m = model.clone();
    if let Some(price) = custom {
        m.price_custom = price.input.is_some() || price.output.is_some();
        m.input_price = price.input.or(m.input_price);
        m.output_price = price.output.or(m.output_price);
    }
    m
}

/// USD for one request to the `cloud:` model `id`; `None` when no price is known (neither the
/// provider's nor the user's) or the request used no tokens.
pub fn request_cost(
    id: &str,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) -> Option<f64> {
    let info = model_info(id)?;
    let (input, output) = (info.info.input_price, info.info.output_price);
    if input.is_none() && output.is_none() {
        return None;
    }
    cost_of(input, output, prompt_tokens, completion_tokens)
}

/// USD for token counts at per-million prices.
pub fn cost_of(
    input: Option<f64>,
    output: Option<f64>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) -> Option<f64> {
    let (p, c) = (prompt_tokens.unwrap_or(0), completion_tokens.unwrap_or(0));
    if (input.is_none() && output.is_none()) || p + c == 0 {
        return None;
    }
    let cost = (p as f64 * input.unwrap_or(0.0) + c as f64 * output.unwrap_or(0.0)) / 1_000_000.0;
    Some((cost * 1e8).round() / 1e8)
}

pub fn model_info(id: &str) -> Option<ModelView> {
    all_models().into_iter().find(|m| m.id == id)
}

pub fn definitions() -> Vec<ModelDefinition> {
    all_models()
        .into_iter()
        .map(|m| {
            let mut def = ModelDefinition::new(
                m.id.clone(),
                ModelRef::Local {
                    path: PathBuf::from(&m.id),
                },
            );
            def.id = m.id;
            def.description = format!("{}: {}", m.label, m.provider);
            def
        })
        .collect()
}

/// `(base_url, key, provider name)` for a proxied request to `id`.
pub fn endpoint(id: &str) -> Result<(String, Option<String>, String), String> {
    let (name, _) = parse_model(id).ok_or_else(|| String::from(err("cloud.unreachable")))?;
    let provider = providers()
        .into_iter()
        .find(|p| p.enabled && p.name == name)
        .ok_or_else(|| String::from(err("cloud.provider_not_found")))?;
    let state = cache()
        .lock()
        .unwrap()
        .get(&provider.id)
        .map(|c| c.status.clone())
        .unwrap_or_default();
    if state.state != State::Online {
        return Err(state
            .last_error
            .unwrap_or_else(|| err("cloud.unreachable").into()));
    }
    let key = key_of(&provider)?;
    if key.is_none() && provider.kind.key_required() {
        return Err(err("cloud.key_missing").into());
    }
    Ok((provider.base_url, key, name.to_string()))
}

/// The kind of the provider a `cloud:` model id belongs to.
pub fn kind_of(id: &str) -> Option<Kind> {
    let (name, _) = parse_model(id)?;
    providers()
        .into_iter()
        .find(|p| p.enabled && p.name == name)
        .map(|p| p.kind)
}

pub fn rewrite_model(id: &str, body: &[u8]) -> Option<Vec<u8>> {
    let (_, model) = parse_model(id)?;
    let mut value: Value = serde_json::from_slice(body).ok()?;
    value.as_object_mut()?.insert("model".into(), model.into());
    // The body leaves the machine for a third party: stored keys and well-known token formats
    // that ended up in a prompt or a tool result are removed first.
    crate::leakguard::redact_value(&mut value, crate::leakguard::Mode::Tokens);
    serde_json::to_vec(&value).ok()
}

/// A request through the gateway failed in a way that may change the provider's state.
pub fn request_refresh(name: &str) {
    let Some(provider) = providers()
        .into_iter()
        .find(|p| p.enabled && p.name == name)
    else {
        return;
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn(async move {
        refresh(&provider).await;
    });
}

fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

fn valid_key(key: &str) -> bool {
    !key.trim().is_empty()
        && key.len() <= 4096
        && reqwest::header::HeaderValue::from_str(&format!("Bearer {key}")).is_ok()
}

/// New provider settings; `key`: `None` keeps the saved key, `""` removes it.
#[derive(Clone, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProviderInput {
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub base_url: String,
    /// Accepted when adding or updating; never returned.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    /// Extra request headers (custom endpoints). `None` keeps the saved ones, `[]` removes them;
    /// the values are kept in the keychain and never returned.
    #[serde(default)]
    pub headers: Option<Vec<HeaderInput>>,
}

impl std::fmt::Debug for ProviderInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderInput")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .field("enabled", &self.enabled)
            .field("headers", &self.headers.as_ref().map(Vec::len))
            .finish()
    }
}

fn enabled_default() -> bool {
    true
}

pub async fn add(input: ProviderInput) -> Result<ProviderView, String> {
    save_provider(None, input).await
}

pub async fn update(id: String, input: ProviderInput) -> Result<ProviderView, String> {
    save_provider(Some(id), input).await
}

async fn save_provider(id: Option<String>, input: ProviderInput) -> Result<ProviderView, String> {
    let lock = mutations().lock().await;
    let name = input.name.trim().to_string();
    if !valid_name(&name) {
        return Err(err("cloud.invalid_name").into());
    }
    let base_url = validate_url(&input.base_url, input.kind)?;
    let mut list = load();
    let prior = match &id {
        Some(id) => Some(
            list.iter()
                .find(|p| &p.id == id)
                .cloned()
                .ok_or_else(|| String::from(err("cloud.not_found")))?,
        ),
        None => None,
    };
    if list
        .iter()
        .any(|p| p.name == name && Some(&p.id) != id.as_ref())
    {
        return Err(err("cloud.name_taken").into());
    }
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let new_key = input.key.as_deref().map(str::trim);
    if new_key.is_some_and(|k| !k.is_empty() && !valid_key(k)) {
        return Err(err("cloud.invalid_key").into());
    }
    let old_key = match &prior {
        Some(p) if p.key_set => secrets::cloud_key_get(&id)?,
        _ => None,
    };
    // A saved key must never follow an edit to a different host: it would be sent there.
    if new_key.is_none()
        && old_key.is_some()
        && prior
            .as_ref()
            .is_some_and(|p| !same_origin(&p.base_url, &base_url))
    {
        return Err(err("cloud.key_required_for_new_address").into());
    }
    let new_headers = match &input.headers {
        Some(h) => Some(check_headers(h)?),
        None => None,
    };
    let old_headers = match &prior {
        Some(p) if !p.headers.is_empty() => read_headers(&id)?,
        _ => Vec::new(),
    };
    // Saved headers may carry credentials: like the key, they never follow an edit to another
    // host.
    let moved = prior
        .as_ref()
        .is_some_and(|p| !same_origin(&p.base_url, &base_url));
    if new_headers.is_none() && moved && !old_headers.is_empty() {
        return Err(err("cloud.key_required_for_new_address").into());
    }
    let headers = new_headers.unwrap_or_else(|| old_headers.clone());
    let key = match new_key {
        Some("") => None,
        Some(k) => Some(k.to_string()),
        None => old_key.clone(),
    };
    match &key {
        Some(k) if old_key.as_deref() != Some(k) => secrets::cloud_key_set(&id, k)?,
        None if old_key.is_some() => secrets::cloud_key_delete(&id)?,
        _ => {}
    }
    if headers != old_headers {
        let result = if headers.is_empty() {
            secrets::cloud_key_delete(&headers_account(&id))
        } else {
            secrets::cloud_key_set(
                &headers_account(&id),
                &serde_json::to_string(&headers).unwrap_or_default(),
            )
        };
        if let Err(e) = result {
            // The key written above must not outlive a failed save.
            match &old_key {
                Some(old) => {
                    let _ = secrets::cloud_key_set(&id, old);
                }
                None if key.is_some() => {
                    let _ = secrets::cloud_key_delete(&id);
                }
                None => {}
            }
            return Err(e);
        }
    }
    let provider = CloudProvider {
        id: id.clone(),
        name,
        kind: input.kind,
        base_url,
        enabled: input.enabled,
        key_set: key.is_some(),
        headers: headers.iter().map(|(n, _)| n.clone()).collect(),
        prices: prior.as_ref().map(|p| p.prices.clone()).unwrap_or_default(),
    };
    list.retain(|p| p.id != id);
    list.push(provider.clone());
    if let Err(e) = save(&list) {
        match &old_key {
            Some(old) => {
                let _ = secrets::cloud_key_set(&id, old);
            }
            None => {
                let _ = secrets::cloud_key_delete(&id);
            }
        }
        if headers != old_headers {
            if old_headers.is_empty() {
                let _ = secrets::cloud_key_delete(&headers_account(&id));
            } else {
                let _ = secrets::cloud_key_set(
                    &headers_account(&id),
                    &serde_json::to_string(&old_headers).unwrap_or_default(),
                );
            }
        }
        return Err(e);
    }
    headers_cache().lock().unwrap().insert(id.clone(), headers);
    set_providers(list);
    {
        let mut cached = cache().lock().unwrap();
        let c = cached.entry(id).or_default();
        if prior
            .as_ref()
            .is_some_and(|p| p.base_url != provider.base_url)
        {
            *c = Cached {
                revision: c.revision,
                ..Default::default()
            };
        }
        c.revision += 1;
    }
    drop(lock);
    let status = if provider.enabled {
        refresh(&provider).await
    } else {
        ProviderStatus::default()
    };
    Ok(ProviderView {
        label: provider.kind.label(),
        provider,
        status,
    })
}

/// Removes the provider and its key. When the keychain cannot be reached the provider is still
/// removed and a warning key says the key may remain.
pub async fn remove(id: &str) -> Result<Option<String>, String> {
    let _lock = mutations().lock().await;
    let mut list = load();
    if !list.iter().any(|p| p.id == id) {
        return Err(err("cloud.not_found").into());
    }
    let old = secrets::cloud_key_get(id).ok().flatten();
    let deleted = secrets::cloud_key_delete(id).is_ok();
    let _ = secrets::cloud_key_delete(&headers_account(id));
    headers_cache().lock().unwrap().remove(id);
    list.retain(|p| p.id != id);
    if let Err(e) = save(&list) {
        if let (true, Some(old)) = (deleted, old) {
            let _ = secrets::cloud_key_set(id, &old);
        }
        return Err(e);
    }
    set_providers(list);
    cache().lock().unwrap().remove(id);
    Ok((!deleted).then(|| err("cloud.removed_key_left").into()))
}

fn find(id: &str) -> Result<CloudProvider, String> {
    providers()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| String::from(err("cloud.not_found")))
}

/// Connection test: lists the provider's models with the saved key.
pub async fn test(id: &str) -> Result<ProviderStatus, String> {
    Ok(refresh(&find(id)?).await)
}

/// One small chat request with a tool to `model`, straight to the provider (it may cost a
/// few tokens). The answer text never contains the key.
pub async fn test_model(id: &str, model: &str) -> Result<conntest::ConnectionTest, String> {
    let provider = find(id)?;
    let key = key_of(&provider)?;
    if key.is_none() && provider.kind.key_required() {
        return Err(err("cloud.key_missing").into());
    }
    let headers = extra_headers_of(&provider);
    let wire = if provider.kind.anthropic_wire() {
        WireApi::Anthropic
    } else {
        WireApi::Chat
    };
    let mut result = conntest::test_connection_with(
        wire,
        &provider.base_url,
        key.as_deref(),
        model,
        "harness-studio",
        &conntest::Options {
            x_api_key: provider.kind.anthropic_wire(),
            headers: &headers,
        },
    )
    .await;
    if let Some(key) = &key {
        result.detail = crate::integrations::launch::redact_text(&result.detail, vec![key]);
    }
    Ok(result)
}

/// What a connection check found, before anything is saved.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProbeResult {
    pub state: State,
    pub latency_ms: Option<u64>,
    pub model_count: usize,
    /// Chat models first, a few at most: enough to see the key works.
    pub models: Vec<CloudModel>,
    pub quota: Option<Quota>,
    /// The keyed error when the check failed.
    pub error: Option<String>,
}

/// "Test Connection" in the add flow: lists the models with the typed (unsaved) key. `id`
/// names the provider being edited, whose saved key is used when none is typed and the address
/// stays on the same host. Nothing is stored.
pub async fn probe(input: ProviderInput, id: Option<String>) -> Result<ProbeResult, String> {
    let base_url = validate_url(&input.base_url, input.kind)?;
    let prior = match &id {
        Some(id) => Some(find(id)?),
        None => None,
    };
    let typed = input
        .key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    if typed.is_some_and(|k| !valid_key(k)) {
        return Err(err("cloud.invalid_key").into());
    }
    let same_host = prior
        .as_ref()
        .is_some_and(|p| same_origin(&p.base_url, &base_url));
    let key = match (typed, &prior) {
        (Some(k), _) => Some(k.to_string()),
        (None, Some(p)) if same_host && p.key_set => key_of(p)?,
        _ => None,
    };
    let headers = match (&input.headers, &prior) {
        (Some(h), _) => check_headers(h)?,
        (None, Some(p)) if same_host => extra_headers_of(p),
        _ => Vec::new(),
    };
    let provider = CloudProvider {
        id: "probe".into(),
        name: "probe".into(),
        kind: input.kind,
        base_url,
        enabled: true,
        key_set: key.is_some(),
        headers: Vec::new(),
        prices: BTreeMap::new(),
    };
    let missing = key.is_none() && provider.kind.key_required();
    let started = Instant::now();
    let found = if missing {
        Err(String::from(err("cloud.key_missing")))
    } else {
        discover(&provider, key.as_deref(), &headers).await
    };
    let latency = started.elapsed().as_millis() as u64;
    Ok(match found {
        Ok((mut models, quota)) => {
            let model_count = models.len();
            models.sort_by_key(|m| m.embedding);
            models.truncate(8);
            ProbeResult {
                state: State::Online,
                latency_ms: Some(latency),
                model_count,
                models,
                quota,
                error: None,
            }
        }
        Err(error) => ProbeResult {
            state: match crate::error_key::parse(&error).map(|(k, _)| k) {
                Some("cloud.unauthorized") => State::Unauthorized,
                Some("cloud.key_missing") => State::KeyMissing,
                _ => State::Offline,
            },
            latency_ms: None,
            model_count: 0,
            models: Vec::new(),
            quota: None,
            error: Some(error),
        },
    })
}

/// Sets or clears (an entry with no prices) the prices of provider models, used for cost
/// tracking. Prices are USD per million tokens.
pub async fn set_prices(
    id: &str,
    prices: BTreeMap<String, ModelPrice>,
) -> Result<ProviderView, String> {
    let _lock = mutations().lock().await;
    if prices.len() > 4096 || prices.iter().any(|(m, p)| clean(m).is_none() || !p.valid()) {
        return Err(err("cloud.invalid_price").into());
    }
    let mut list = load();
    let provider = list
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| String::from(err("cloud.not_found")))?;
    provider.prices = prices
        .into_iter()
        .filter(|(_, p)| p.input.is_some() || p.output.is_some())
        .collect();
    let provider = provider.clone();
    save(&list)?;
    set_providers(list);
    let status = cache()
        .lock()
        .unwrap()
        .get(id)
        .map(|c| c.status.clone())
        .unwrap_or_default();
    Ok(ProviderView {
        label: provider.kind.label(),
        provider,
        status,
    })
}

#[cfg(test)]
mod tests;
