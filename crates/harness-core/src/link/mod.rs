//! Remote gateway connections and their last successful model lists.

use crate::{
    config::AppSettings,
    domain::{ModelDefinition, ModelRef},
    error_key::err,
    secrets,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::IpAddr,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteLink {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub allow_insecure_http: bool,
    pub enabled: bool,
    #[serde(default)]
    pub token_set: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Online,
    #[default]
    Offline,
    Unauthorized,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LinkStatus {
    pub state: State,
    pub model_count: usize,
    pub last_error: Option<String>,
    pub last_ok: Option<u64>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    #[serde(flatten)]
    pub link: RemoteLink,
    #[serde(flatten)]
    pub status: LinkStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelView {
    pub id: String,
    pub model: String,
    pub link: String,
    pub state: State,
}

pub fn models() -> Vec<ModelView> {
    let links = AppSettings::load().links;
    let cached = cache().lock().unwrap();
    links
        .into_iter()
        .filter(|l| l.enabled)
        .flat_map(|l| {
            cached.get(&l.id).map_or_else(Vec::new, |c| {
                c.models
                    .iter()
                    .map(|m| ModelView {
                        id: model_id(&l.name, &m.id),
                        model: m.id.clone(),
                        link: l.name.clone(),
                        state: c.status.state,
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}

#[derive(Clone, Default)]
struct Cached {
    status: LinkStatus,
    models: Vec<RemoteModel>,
    revision: u64,
}

#[derive(Clone)]
struct RemoteModel {
    id: String,
    definition_id: Option<String>,
    state: Option<String>,
}

pub fn model_metadata(id: &str) -> Option<(Option<String>, Option<String>)> {
    let (name, model) = parse_model(id)?;
    let link = AppSettings::load()
        .links
        .into_iter()
        .find(|l| l.enabled && l.name == name)?;
    let cached = cache().lock().unwrap();
    let model = cached
        .get(&link.id)?
        .models
        .iter()
        .find(|m| m.id == model)?;
    Some((model.definition_id.clone(), model.state.clone()))
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
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

pub fn model_id(link: &str, model: &str) -> String {
    format!("remote:{link}/{model}")
}

pub fn parse_model(id: &str) -> Option<(&str, &str)> {
    let (link, model) = id.strip_prefix("remote:")?.split_once('/')?;
    (valid_name(link) && !model.is_empty() && !model.chars().any(char::is_control))
        .then_some((link, model))
}

fn private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|ip| private_ip(IpAddr::V4(ip)))
            .unwrap_or(ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()),
    }
}

pub async fn validate_url(raw: &str, allow_http: bool) -> Result<String, String> {
    let mut url = crate::network::parse_public_http_url(raw.trim())
        .map_err(|_| String::from(err("link.invalid_url")))?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(err("link.invalid_url").into());
    }
    if url.scheme() == "http" {
        if !allow_http {
            return Err(err("link.insecure_not_allowed").into());
        }
        let addresses = tokio::time::timeout(
            Duration::from_secs(5),
            crate::network::lookup_url_host(&url, url.port_or_known_default().unwrap_or(80)),
        )
        .await
        .map_err(|_| String::from(err("link.timeout")))?
        .map_err(|_| String::from(err("link.dns_failed")))?;
        if addresses.is_empty() || addresses.iter().any(|a| !private_ip(a.ip())) {
            return Err(err("link.insecure_not_allowed").into());
        }
    }
    let path = url
        .path()
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or(url.path().trim_end_matches('/'))
        .to_string();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn client(idle: u32) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none());
    if idle > 0 {
        builder = builder.read_timeout(Duration::from_secs(u64::from(idle)));
    }
    builder.build().map_err(|_| err("link.unreachable").into())
}

pub fn transport_error(error: reqwest::Error, token: &str) -> String {
    let error = error.without_url();
    let classified = crate::http_error::transport(&error, "");
    let key = match crate::error_key::parse(&classified).map(|(key, _)| key) {
        _ if certificate_invalid(&error) => "link.certificate_invalid",
        Some("http.tls_failed") => "link.tls_failed",
        Some("http.dns_failed") => "link.dns_failed",
        _ if error.is_timeout() => "link.timeout",
        _ => "link.unreachable",
    };
    // Only the classified key escapes this boundary; remote text can contain credentials.
    crate::integrations::launch::redact_text(&String::from(err(key)), vec![token])
}

fn certificate_invalid(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(error) = source {
        let text = error.to_string().to_ascii_lowercase();
        if [
            "certificate",
            "cert validation",
            "unknownissuer",
            "unknown issuer",
            "cert_e_",
            "sec_e_untrusted_root",
            "sec_e_wrong_principal",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            return true;
        }
        source = error.source();
    }
    false
}

fn scope_warning(token: &str, identity: &serde_json::Value) -> Option<String> {
    (token.starts_with("hss_")
        || identity["scopes"]
            .as_array()
            .is_some_and(|scopes| scopes.iter().any(|s| s == "admin")))
    .then(|| err("link.inference_scope_warning").into())
}

pub fn token(link: &RemoteLink) -> Result<String, String> {
    secrets::link_get(&link.id)?.ok_or_else(|| err("link.unauthorized").into())
}

pub fn list() -> Vec<LinkView> {
    let links = AppSettings::load().links;
    let cached = cache().lock().unwrap();
    links
        .into_iter()
        .map(|link| {
            let status = cached
                .get(&link.id)
                .map(|c| c.status.clone())
                .unwrap_or_default();
            LinkView { link, status }
        })
        .collect()
}

pub fn status(name: &str) -> Option<LinkStatus> {
    list()
        .into_iter()
        .find(|v| v.link.enabled && v.link.name == name)
        .map(|v| v.status)
}

pub fn mark_failed(name: &str, key: &'static str) {
    let Some(link) = AppSettings::load()
        .links
        .into_iter()
        .find(|l| l.name == name)
    else {
        return;
    };
    let mut cached = cache().lock().unwrap();
    let c = cached.entry(link.id).or_default();
    c.status.state = if key == "link.unauthorized" {
        State::Unauthorized
    } else {
        State::Offline
    };
    c.status.last_error = Some(err(key).into());
    c.status.warning = None;
}

async fn discover(link: &RemoteLink) -> Result<(Vec<RemoteModel>, Option<String>), String> {
    let base = validate_url(&link.base_url, link.allow_insecure_http).await?;
    let token = token(link)?;
    let http = client(10)?;
    // Health is advisory: compatible gateways may not provide it.
    let _ = http
        .get(format!("{base}/health"))
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    let response = http
        .get(format!("{base}/v1/models"))
        .timeout(Duration::from_secs(10))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| transport_error(e, &token))?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(err("link.unauthorized").into());
    }
    if !response.status().is_success() {
        return Err(err("link.unreachable").into());
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|e| transport_error(e, &token))?;
    let data = value["data"]
        .as_array()
        .ok_or_else(|| String::from(err("link.unreachable")))?;
    let mut models: Vec<RemoteModel> = data
        .iter()
        .filter_map(|v| {
            if v["owned_by"] == "harness-link" {
                return None;
            }
            let id = v["id"].as_str()?;
            if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
                return None;
            }
            let metadata = |key: &str| {
                v["harness"][key]
                    .as_str()
                    .filter(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
                    .map(str::to_string)
            };
            Some(RemoteModel {
                id: id.to_string(),
                definition_id: metadata("definition_id"),
                state: metadata("state"),
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    let identity = match http
        .get(format!("{base}/v1/harness/whoami"))
        .timeout(Duration::from_secs(10))
        .bearer_auth(&token)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response.json().await.unwrap_or_default(),
        _ => serde_json::Value::Null,
    };
    Ok((models, scope_warning(&token, &identity)))
}

pub async fn refresh(link: &RemoteLink) -> LinkStatus {
    let revision = cache()
        .lock()
        .unwrap()
        .entry(link.id.clone())
        .or_default()
        .revision;
    let result = discover(link).await;
    // An edit/removal while discovery was in flight must not restore stale state.
    if !AppSettings::load().links.contains(link) {
        return LinkStatus::default();
    }
    let mut cached = cache().lock().unwrap();
    let c = cached.entry(link.id.clone()).or_default();
    if c.revision != revision {
        return c.status.clone();
    }
    match result {
        Ok((models, warning)) => {
            c.status.state = State::Online;
            c.status.model_count = models.len();
            c.status.last_error = None;
            c.status.warning = warning;
            c.status.last_ok = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
            c.models = models;
        }
        Err(error) => {
            c.status.warning = None;
            c.status.state =
                if crate::error_key::parse(&error).is_some_and(|(k, _)| k == "link.unauthorized") {
                    State::Unauthorized
                } else {
                    State::Offline
                };
            c.status.last_error = Some(error);
        }
    }
    c.status.clone()
}

pub async fn refresh_all() {
    futures_util::future::join_all(
        AppSettings::load()
            .links
            .into_iter()
            .filter(|l| l.enabled)
            .map(|l| async move { refresh(&l).await }),
    )
    .await;
}

pub async fn refresh_loop() {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        refresh_all().await;
    }
}

pub fn definitions() -> Vec<ModelDefinition> {
    let links = AppSettings::load().links;
    let cached = cache().lock().unwrap();
    links
        .into_iter()
        .filter(|l| l.enabled)
        .flat_map(|l| {
            cached
                .get(&l.id)
                .map(|c| {
                    c.models
                        .iter()
                        .map(|m| {
                            let id = model_id(&l.name, &m.id);
                            let mut def = ModelDefinition::new(
                                id.clone(),
                                ModelRef::Local {
                                    path: PathBuf::from(&id),
                                },
                            );
                            def.id = id;
                            def.description = l.name.clone();
                            def
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

pub fn endpoint(id: &str) -> Result<(String, String, String), String> {
    let (name, _) = parse_model(id).ok_or_else(|| String::from(err("link.unreachable")))?;
    let link = AppSettings::load()
        .links
        .into_iter()
        .find(|l| l.enabled && l.name == name)
        .ok_or_else(|| String::from(err("link.unreachable")))?;
    let status = status(name).unwrap_or_default();
    if status.state != State::Online {
        return Err(status
            .last_error
            .unwrap_or_else(|| err("link.unreachable").into()));
    }
    Ok((link.base_url.clone(), token(&link)?, name.to_string()))
}

pub fn rewrite_model(id: &str, body: &[u8]) -> Option<Vec<u8>> {
    let (_, model) = parse_model(id)?;
    let mut value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value.as_object_mut()?.insert("model".into(), model.into());
    serde_json::to_vec(&value).ok()
}

pub async fn add(
    name: String,
    base_url: String,
    token: String,
    allow_insecure_http: bool,
) -> Result<LinkView, String> {
    update_inner(None, name, base_url, Some(token), allow_insecure_http, true).await
}

pub async fn update(
    id: String,
    name: String,
    base_url: String,
    token: Option<String>,
    allow_insecure_http: bool,
    enabled: bool,
) -> Result<LinkView, String> {
    update_inner(
        Some(id),
        name,
        base_url,
        token,
        allow_insecure_http,
        enabled,
    )
    .await
}

async fn update_inner(
    id: Option<String>,
    name: String,
    base_url: String,
    token: Option<String>,
    allow_insecure_http: bool,
    enabled: bool,
) -> Result<LinkView, String> {
    let lock = mutations().lock().await;
    let name = name.trim().to_string();
    if !valid_name(&name) {
        return Err(err("link.invalid_name").into());
    }
    let base_url = validate_url(&base_url, allow_insecure_http).await?;
    let mut settings = AppSettings::load();
    if id
        .as_ref()
        .is_some_and(|id| !settings.links.iter().any(|l| &l.id == id))
    {
        return Err(err("link.not_found").into());
    }
    if settings
        .links
        .iter()
        .any(|l| l.name == name && Some(&l.id) != id.as_ref())
    {
        return Err(err("link.name_taken").into());
    }
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let old_token = secrets::link_get(&id)?;
    let token = token
        .filter(|t| !t.is_empty())
        .or_else(|| old_token.clone())
        .ok_or_else(|| String::from(err("link.unauthorized")))?;
    if token.trim().is_empty()
        || reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).is_err()
    {
        return Err(err("link.unauthorized").into());
    }
    secrets::link_set(&id, &token)?;
    let link = RemoteLink {
        id: id.clone(),
        name,
        base_url,
        allow_insecure_http,
        enabled,
        token_set: true,
    };
    let prior = settings.links.iter().find(|l| l.id == id).cloned();
    settings.links.retain(|l| l.id != id);
    settings.links.push(link.clone());
    if let Err(e) = settings.save() {
        if let Some(old) = old_token {
            let _ = secrets::link_set(&id, &old);
        } else {
            let _ = secrets::link_delete(&id);
        }
        return Err(e);
    }
    {
        let mut cached = cache().lock().unwrap();
        let c = cached.entry(id).or_default();
        if prior.as_ref().is_some_and(|p| p.base_url != link.base_url) {
            c.models.clear();
            c.status = LinkStatus::default();
        }
        if old_token.as_deref() != Some(token.as_str()) {
            c.status.warning = None;
        }
        c.revision += 1;
    }
    drop(lock);
    let status = refresh(&link).await;
    Ok(LinkView { link, status })
}

pub async fn remove(id: &str) -> Result<(), String> {
    let _lock = mutations().lock().await;
    let mut settings = AppSettings::load();
    let old = secrets::link_get(id)?;
    secrets::link_delete(id)?;
    settings.links.retain(|l| l.id != id);
    if let Err(e) = settings.save() {
        if let Some(old) = old {
            let _ = secrets::link_set(id, &old);
        }
        return Err(e);
    }
    cache().lock().unwrap().remove(id);
    Ok(())
}

pub async fn test(id: &str) -> Result<LinkStatus, String> {
    let link = AppSettings::load()
        .links
        .into_iter()
        .find(|l| l.id == id)
        .ok_or_else(|| String::from(err("link.not_found")))?;
    Ok(refresh(&link).await)
}

#[cfg(test)]
mod tests;
