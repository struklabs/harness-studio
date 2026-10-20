//! Automations: connect Harness Studio to the user's own n8n instance.
//!
//! Harness never embeds, installs, ships or runs n8n. It only talks to an instance the user
//! already operates, through n8n's documented public REST API (`<url>/api/v1/...`, header
//! `X-N8N-API-KEY`):
//!
//! * `GET /workflows`, `GET /workflows/{id}`: workflows with their active state and triggers;
//! * `GET /executions`, `GET /executions/{id}?includeData=true`: recent runs and their errors.
//!
//! The public API has no "run workflow" call, so a manual run goes through the workflow's own
//! **Webhook** trigger (`<url>/webhook/<path>`, active workflows only). Workflows that only have
//! a manual or schedule trigger are listed but cannot be started from here.
//!
//! The API key lives in the OS keychain ([`crate::secrets`]); `settings.json` only keeps the
//! instance URL. The key is sent only to the saved n8n address, redirects are never followed
//! (a custom header would be forwarded to the new host), and it never appears in errors or logs.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error_key::err;

#[cfg(test)]
mod tests;

pub const ERR_URL_INVALID: &str = "automations.n8n_url_invalid";
pub const ERR_NOT_CONFIGURED: &str = "automations.n8n_not_configured";
pub const ERR_KEY_MISSING: &str = "automations.n8n_key_missing";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PAGE_LIMIT: u32 = 100;
/// Workflows read from n8n at most (pages of [`PAGE_LIMIT`]).
const MAX_WORKFLOW_PAGES: usize = 5;
/// Longest webhook answer text handed back to the UI.
const RESPONSE_PREVIEW: usize = 2000;
/// Name of the gateway token created for n8n's OpenAI node.
pub const N8N_TOKEN_NAME: &str = "n8n";

/// `settings.json` -> `n8n`. The key is not here (OS keychain). Changed only through [`save`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct N8nSettings {
    /// Base URL of the user's n8n (`https://n8n.example.com`), empty = not connected.
    pub url: String,
}

/// What Settings shows: the saved URL and whether a key is stored (never the key).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct N8nView {
    pub url: String,
    pub key_set: bool,
}

/// Result of the connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct N8nConnection {
    pub ok: bool,
    /// The normalized address that answered.
    pub url: String,
    /// Whether the key can read at least one workflow (`false` for an empty instance).
    pub has_workflows: bool,
}

/// How a workflow can be started from outside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WebhookTrigger {
    pub method: String,
    pub path: String,
    /// Production webhook URL (`<n8n>/webhook/<path>`), live while the workflow is active.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub active: bool,
    /// Short names of the trigger nodes (`webhook`, `scheduleTrigger`, `manualTrigger`, ...).
    pub triggers: Vec<String>,
    /// The first webhook trigger, when there is one: the only way to start a run from here.
    pub webhook: Option<WebhookTrigger>,
    pub updated_at: Option<String>,
    /// Link that opens the workflow in the n8n editor.
    pub editor_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Execution {
    pub id: String,
    pub workflow_id: String,
    /// `success`, `error`, `crashed`, `running`, `waiting`, `canceled`, `new` or `unknown`.
    pub status: String,
    /// `manual`, `trigger`, `webhook`, `retry`, ...
    pub mode: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub editor_url: String,
}

/// Why an execution failed (read on demand: the list call does not carry run data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExecutionError {
    pub message: Option<String>,
    /// The node that failed.
    pub node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TriggerResult {
    /// HTTP status of the webhook answer.
    pub status: u16,
    pub ok: bool,
    /// Start of the answer body.
    pub body: String,
}

/// The gateway address n8n's OpenAI node uses (Base URL) and whether it needs a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OpenAiConnection {
    pub running: bool,
    pub base_url: Option<String>,
    pub requires_token: bool,
}

// --- URL and settings ---

/// Validates the address: `http(s)://host[:port][/prefix]`, no credentials, query or
/// fragment. A trailing slash and a pasted `/api/v1` or `/home/...` tail are dropped.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let bad = || -> String { err(ERR_URL_INVALID).into() };
    let mut url = reqwest::Url::parse(raw.trim()).map_err(|_| bad())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    for tail in ["/api/v1", "/home/workflows", "/home"] {
        if let Some(p) = path.strip_suffix(tail) {
            path = p.to_string();
            break;
        }
    }
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn view() -> Result<N8nView, String> {
    let url = crate::config::AppSettings::load().n8n.url;
    let key_set = !url.is_empty() && crate::secrets::n8n_key_get()?.is_some();
    Ok(N8nView { url, key_set })
}

/// Saves the address and key. `api_key`: `None` keeps the saved key, `Some("")` removes it,
/// anything else replaces it. An empty address disconnects (the key is removed too).
pub fn save(url: &str, api_key: Option<&str>) -> Result<N8nView, String> {
    let mut all = crate::config::AppSettings::load();
    if url.trim().is_empty() {
        crate::secrets::n8n_key_set(None)?;
        all.n8n = N8nSettings::default();
        all.save()?;
        return view();
    }
    let url = normalize_url(url)?;
    if let Some(key) = api_key {
        crate::secrets::n8n_key_set(Some(key))?;
    }
    all.n8n = N8nSettings { url };
    all.save()?;
    view()
}

// --- Client ---

pub struct N8nClient {
    base: String,
    key: String,
    http: reqwest::Client,
}

impl N8nClient {
    pub fn new(url: &str, key: &str) -> Result<Self, String> {
        let base = normalize_url(url)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(err(ERR_KEY_MISSING).into());
        }
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            // The key header must never follow a redirect to another host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| crate::http_error::transport(&e, &base))?;
        Ok(Self {
            base,
            key: key.to_string(),
            http,
        })
    }

    /// A client for the saved address and key.
    pub fn saved() -> Result<Self, String> {
        let url = crate::config::AppSettings::load().n8n.url;
        if url.is_empty() {
            return Err(err(ERR_NOT_CONFIGURED).into());
        }
        let key =
            crate::secrets::n8n_key_get()?.ok_or_else(|| String::from(err(ERR_KEY_MISSING)))?;
        Self::new(&url, &key)
    }

    /// A client for a form's values: a missing address or key falls back to the saved one.
    pub fn from_form(url: Option<&str>, key: Option<&str>) -> Result<Self, String> {
        let saved = crate::config::AppSettings::load().n8n.url;
        let url = url
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .unwrap_or(&saved);
        if url.is_empty() {
            return Err(err(ERR_NOT_CONFIGURED).into());
        }
        let key = match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => k.to_string(),
            None => {
                crate::secrets::n8n_key_get()?.ok_or_else(|| String::from(err(ERR_KEY_MISSING)))?
            }
        };
        Self::new(url, &key)
    }

    fn api(&self, path: &str) -> String {
        format!("{}/api/v1/{}", self.base, path)
    }

    fn editor_url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path)
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
        let url = self.api(path);
        let resp = self
            .http
            .get(&url)
            .header("X-N8N-API-KEY", &self.key)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(status_error(status.as_u16()));
        }
        resp.json::<Value>()
            .await
            .map_err(|_| err("automations.n8n_invalid_response").into())
    }

    /// Connection test: the key must be accepted by the public API.
    pub async fn test(&self) -> Result<N8nConnection, String> {
        let v = self.get("workflows", &[("limit", "1".into())]).await?;
        let list = data_array(&v)?;
        Ok(N8nConnection {
            ok: true,
            url: self.base.clone(),
            has_workflows: !list.is_empty(),
        })
    }

    pub async fn workflows(&self) -> Result<Vec<Workflow>, String> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_WORKFLOW_PAGES {
            let mut q = vec![("limit", PAGE_LIMIT.to_string())];
            if let Some(c) = &cursor {
                q.push(("cursor", c.clone()));
            }
            let v = self.get("workflows", &q).await?;
            out.extend(
                data_array(&v)?
                    .iter()
                    .filter_map(|w| self.parse_workflow(w)),
            );
            cursor = v
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn workflow(&self, id: &str) -> Result<Workflow, String> {
        let v = self
            .get(&format!("workflows/{}", segment(id)?), &[])
            .await?;
        self.parse_workflow(&v)
            .ok_or_else(|| err("automations.n8n_invalid_response").into())
    }

    pub async fn executions(
        &self,
        workflow_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Execution>, String> {
        let mut q = vec![("limit", limit.clamp(1, PAGE_LIMIT).to_string())];
        if let Some(id) = workflow_id.filter(|i| !i.is_empty()) {
            q.push(("workflowId", segment(id)?.to_string()));
        }
        let v = self.get("executions", &q).await?;
        Ok(data_array(&v)?
            .iter()
            .filter_map(|e| self.parse_execution(e))
            .collect())
    }

    /// The error of a failed execution (message and failing node), read from its run data.
    pub async fn execution_error(&self, id: &str) -> Result<ExecutionError, String> {
        let v = self
            .get(
                &format!("executions/{}", segment(id)?),
                &[("includeData", "true".into())],
            )
            .await?;
        let result = v.pointer("/data/resultData");
        let error = result.and_then(|r| r.get("error"));
        let message = error
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map(|m| clip(m, 600));
        let node = error
            .and_then(|e| e.pointer("/node/name"))
            .or_else(|| result.and_then(|r| r.get("lastNodeExecuted")))
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(ExecutionError { message, node })
    }

    /// Starts a workflow through its webhook trigger. The workflow is read again first, so
    /// only an active workflow that really has a webhook is called. The API key is not sent
    /// to the webhook.
    pub async fn trigger(
        &self,
        id: &str,
        payload: Option<&Value>,
    ) -> Result<TriggerResult, String> {
        let wf = self.workflow(id).await?;
        let hook = wf
            .webhook
            .ok_or_else(|| String::from(err("automations.n8n_no_webhook")))?;
        if !wf.active {
            return Err(err("automations.n8n_workflow_inactive").into());
        }
        let method = reqwest::Method::from_bytes(hook.method.as_bytes())
            .map_err(|_| String::from(err("automations.n8n_no_webhook")))?;
        let mut req = self.http.request(method.clone(), &hook.url);
        if let Some(body) = payload.filter(|_| method != reqwest::Method::GET) {
            req = req.json(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &hook.url))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Ok(TriggerResult {
            status: status.as_u16(),
            ok: status.is_success(),
            body: clip(&text, RESPONSE_PREVIEW),
        })
    }

    fn parse_workflow(&self, w: &Value) -> Option<Workflow> {
        let id = value_id(w.get("id")?)?;
        let name = w
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let nodes = w.get("nodes").and_then(Value::as_array);
        let mut triggers = Vec::new();
        let mut webhook = None;
        for node in nodes.into_iter().flatten() {
            let ty = node.get("type").and_then(Value::as_str).unwrap_or("");
            if node.get("disabled").and_then(Value::as_bool) == Some(true) || !is_trigger(ty) {
                continue;
            }
            let short = ty.rsplit('.').next().unwrap_or(ty).to_string();
            if webhook.is_none() && ty == "n8n-nodes-base.webhook" {
                webhook = self.webhook_of(node);
            }
            if !triggers.contains(&short) {
                triggers.push(short);
            }
        }
        Some(Workflow {
            editor_url: self.editor_url(&format!("workflow/{id}")),
            id,
            name,
            active: w.get("active").and_then(Value::as_bool).unwrap_or(false),
            triggers,
            webhook,
            updated_at: w
                .get("updatedAt")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    fn webhook_of(&self, node: &Value) -> Option<WebhookTrigger> {
        let params = node.get("parameters");
        let path = params
            .and_then(|p| p.get("path"))
            .and_then(Value::as_str)
            .filter(|p| !p.trim().is_empty())
            .or_else(|| node.get("webhookId").and_then(Value::as_str))?
            .trim()
            .trim_matches('/')
            .to_string();
        // Paths with `:parameters` need a caller-specific URL; not a plain "run".
        if path.is_empty() || path.contains(':') || path.contains('{') {
            return None;
        }
        let method = params
            .and_then(|p| p.get("httpMethod"))
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase();
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
            return None;
        }
        Some(WebhookTrigger {
            url: format!("{}/webhook/{}", self.base, path),
            method,
            path,
        })
    }

    fn parse_execution(&self, e: &Value) -> Option<Execution> {
        let id = value_id(e.get("id")?)?;
        let workflow_id = e.get("workflowId").and_then(value_id).unwrap_or_default();
        let status = e
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if e.get("finished").and_then(Value::as_bool) == Some(true) {
                    "success".into()
                } else {
                    "unknown".into()
                }
            });
        Some(Execution {
            editor_url: self.editor_url(&format!("workflow/{workflow_id}/executions/{id}")),
            id,
            workflow_id,
            status,
            mode: e
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            started_at: e
                .get("startedAt")
                .and_then(Value::as_str)
                .map(str::to_string),
            stopped_at: e
                .get("stoppedAt")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

fn data_array(v: &Value) -> Result<&Vec<Value>, String> {
    v.get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| err("automations.n8n_invalid_response").into())
}

fn is_trigger(node_type: &str) -> bool {
    let t = node_type.to_ascii_lowercase();
    t.ends_with(".webhook") || t.ends_with(".cron") || t.contains("trigger")
}

/// n8n ids are strings, older ones numbers.
fn value_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A path segment from the caller: ids only, never `/`, `?` or `..`.
fn segment(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(err("automations.n8n_id_invalid").into());
    }
    Ok(id)
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_string(),
    }
}

fn status_error(status: u16) -> String {
    match status {
        401 => err("automations.n8n_unauthorized").into(),
        403 => err("automations.n8n_forbidden").into(),
        404 => err("automations.n8n_api_missing").into(),
        301 | 302 | 303 | 307 | 308 => err("automations.n8n_redirect").into(),
        s if s >= 500 => err("automations.n8n_server_error").num("status", s).into(),
        s => err("automations.n8n_http_error").num("status", s).into(),
    }
}

// --- Gateway side: what n8n's OpenAI node needs ---

/// The managed gateway's OpenAI-compatible base URL (while it runs).
pub fn openai_connection() -> OpenAiConnection {
    let status = crate::gateway::status();
    OpenAiConnection {
        running: status.running,
        base_url: status.endpoint.map(|e| e.base_url),
        requires_token: status.require_token,
    }
}

/// Creates a gateway token with only the `inference` scope for n8n (shown once).
pub fn create_gateway_token() -> Result<crate::control_api::CreatedToken, String> {
    crate::control_api::TokenStore::global()
        .create(N8N_TOKEN_NAME, &[crate::control_api::Scope::Inference])
}
