//! Paperclip: connect Harness Studio to the user's own Paperclip install (the open-source
//! AI agent company control plane).
//!
//! Harness never embeds, installs or runs Paperclip here. It talks to an instance the user
//! already operates through Paperclip's documented REST API (`<url>/api/...`, header
//! `Authorization: Bearer <board API key>`):
//!
//! * `GET /health`, `GET /companies`: the instance and its companies;
//! * `GET /companies/{id}/agents`: agents with role, title, adapter ("model protocol"),
//!   status and monthly budget; the reporting lines give the teams;
//! * `GET /companies/{id}/issues`, `.../heartbeat-runs`: tasks and agent runs;
//! * `GET /companies/{id}/dashboard`, `.../costs/by-agent`: budgets and spend.
//!
//! A local trusted Paperclip (the default of `npx paperclipai onboard`) needs no key; an
//! authenticated one needs a board API key. The key lives in the OS keychain
//! ([`crate::secrets`]); `settings.json` keeps the address, the chosen company and the links
//! between Paperclip tasks and Harness Projects. The key is only sent to the saved address,
//! redirects are never followed, and it never appears in errors or logs. Plain `http` is only
//! accepted for this machine.
//!
//! Harness models reach a Paperclip agent through the gateway: [`use_harness_model`] mints a
//! token limited to one model and the chat/model endpoints and writes the provider into the
//! agent's `opencode_local` adapter config ([`crate::modules::paperclip`]).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error_key::err;

#[cfg(test)]
mod tests;

pub const ERR_URL_INVALID: &str = "paperclip.url_invalid";
pub const ERR_NOT_CONFIGURED: &str = "paperclip.not_configured";
pub const ERR_NO_COMPANY: &str = "paperclip.no_company";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Most tasks and runs handed to the UI.
const MAX_TASKS: usize = 200;
const MAX_RUNS: u32 = 50;
const MAX_LINKS: usize = 5000;

/// `settings.json` -> `paperclip`. The key is not here (OS keychain).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PaperclipSettings {
    /// Base URL of the user's Paperclip (`http://localhost:3100`), empty = not connected.
    pub url: String,
    /// The company shown in Harness (a Paperclip instance can hold several).
    pub company_id: String,
    /// Paperclip task id -> Harness project id.
    pub task_links: BTreeMap<String, String>,
}

/// What the UI shows: the saved values and whether a key is stored (never the key).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PaperclipView {
    pub url: String,
    pub company_id: String,
    pub key_set: bool,
    pub task_links: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PaperclipHealth {
    /// `ok`, `starting` or `unhealthy`.
    pub status: String,
    pub version: Option<String>,
    /// `local_trusted` or `authenticated`.
    pub deployment_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Company {
    pub id: String,
    pub name: String,
    pub status: String,
    pub budget_monthly_cents: i64,
}

/// Result of the connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Connection {
    pub ok: bool,
    /// The normalized address that answered.
    pub url: String,
    pub health: PaperclipHealth,
    pub companies: Vec<Company>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub role: String,
    pub title: Option<String>,
    pub reports_to: Option<String>,
    /// `idle`, `running`, `paused`, `error`, `terminated`, ...
    pub status: String,
    /// The execution adapter ("model protocol"): `claude_local`, `codex_local`,
    /// `opencode_local`, ...
    pub adapter_type: String,
    /// The model the adapter is set to, when its config names one.
    pub model: Option<String>,
    /// The adapter's model is a Harness gateway entry (`harness/<model>`).
    pub uses_harness: bool,
    pub budget_monthly_cents: i64,
    pub spent_monthly_cents: i64,
}

/// One reporting line: a manager and the agents that report to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Team {
    pub lead_id: String,
    pub lead_name: String,
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Task {
    pub id: String,
    /// Readable id such as `PAP-12`.
    pub identifier: Option<String>,
    pub title: String,
    pub status: String,
    pub priority: Option<String>,
    pub assignee_agent_id: Option<String>,
    pub project_id: Option<String>,
    pub updated_at: Option<String>,
    /// Harness project this task is linked to (from `settings.json`).
    pub harness_project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Run {
    pub id: String,
    pub agent_id: String,
    pub issue_id: Option<String>,
    /// `queued`, `running`, `succeeded`, `failed`, `timed_out`, `cancelled`, ...
    pub status: String,
    pub source: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Budget {
    pub month_spend_cents: f64,
    pub month_budget_cents: f64,
    pub utilization_percent: f64,
    pub pending_approvals: u64,
    pub paused_agents: u64,
    pub active_incidents: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentCost {
    pub agent_id: String,
    pub cost_cents: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
}

/// Everything the Paperclip page shows for one company.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Overview {
    pub company: Company,
    pub health: PaperclipHealth,
    pub agents: Vec<Agent>,
    pub teams: Vec<Team>,
    pub tasks: Vec<Task>,
    pub runs: Vec<Run>,
    /// `None` when the key may not read budgets.
    pub budget: Option<Budget>,
    pub costs: Vec<AgentCost>,
}

/// Outcome of [`use_harness_model`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelApplied {
    pub agent_id: String,
    pub model: String,
    /// The scoped gateway token created for it (revocable in Settings).
    pub token_id: String,
}

// --- URL and settings ---

/// Validates the address: `https://host[:port][/prefix]`, or `http` for this machine only;
/// no credentials, query or fragment. A trailing slash or pasted `/api` tail is dropped.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let bad = || -> String { err(ERR_URL_INVALID).into() };
    let mut url = reqwest::Url::parse(raw.trim()).map_err(|_| bad())?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => matches!(
            url.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
        ),
        _ => false,
    };
    if !secure
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    if let Some(p) = path.strip_suffix("/api") {
        path = p.to_string();
    }
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn view() -> Result<PaperclipView, String> {
    let s = crate::config::AppSettings::load().paperclip;
    let key_set = !s.url.is_empty() && crate::secrets::paperclip_key_get()?.is_some();
    Ok(PaperclipView {
        url: s.url,
        company_id: s.company_id,
        key_set,
        task_links: s.task_links,
    })
}

/// Saves the address and key. `api_key`: `None` keeps the saved key, `Some("")` removes it,
/// anything else replaces it; `None` with a changed address also removes it (a key is never
/// reused for another host). An empty address disconnects (key, company and links go).
pub fn save(url: &str, api_key: Option<&str>) -> Result<PaperclipView, String> {
    let mut all = crate::config::AppSettings::load();
    if url.trim().is_empty() {
        crate::secrets::paperclip_key_set(None)?;
        all.paperclip = PaperclipSettings::default();
        all.save()?;
        return view();
    }
    let url = normalize_url(url)?;
    let changed = url != all.paperclip.url;
    match api_key {
        Some(key) => crate::secrets::paperclip_key_set(Some(key))?,
        None if changed => crate::secrets::paperclip_key_set(None)?,
        None => {}
    }
    if changed {
        // Another instance: its company ids and task ids mean nothing here.
        all.paperclip = PaperclipSettings {
            url,
            ..Default::default()
        };
    }
    all.save()?;
    view()
}

/// Chooses the company shown in Harness.
pub fn set_company(company_id: &str) -> Result<PaperclipView, String> {
    let id = segment(company_id)?;
    let mut all = crate::config::AppSettings::load();
    if all.paperclip.url.is_empty() {
        return Err(err(ERR_NOT_CONFIGURED).into());
    }
    if all.paperclip.company_id != id {
        all.paperclip.company_id = id.to_string();
        all.paperclip.task_links.clear();
        all.save()?;
    }
    view()
}

/// Links a Paperclip task to a Harness project (`None` removes the link). The caller checks
/// that the project exists.
pub fn set_task_link(task_id: &str, project_id: Option<&str>) -> Result<PaperclipView, String> {
    let task = segment(task_id)?;
    let mut all = crate::config::AppSettings::load();
    match project_id.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            if p.len() > 128 {
                return Err(err("paperclip.id_invalid").into());
            }
            if !all.paperclip.task_links.contains_key(task)
                && all.paperclip.task_links.len() >= MAX_LINKS
            {
                return Err(err("paperclip.too_many_links").into());
            }
            all.paperclip
                .task_links
                .insert(task.to_string(), p.to_string());
        }
        None => {
            all.paperclip.task_links.remove(task);
        }
    }
    all.save()?;
    view()
}

/// Drops the links to a Harness project that no longer exists.
pub fn forget_project(project_id: &str) -> Result<(), String> {
    let mut all = crate::config::AppSettings::load();
    let before = all.paperclip.task_links.len();
    all.paperclip.task_links.retain(|_, p| p != project_id);
    if all.paperclip.task_links.len() != before {
        all.save()?;
    }
    Ok(())
}

// --- Client ---

pub struct PaperclipClient {
    base: String,
    /// Board API key; `None` for a local trusted instance.
    key: Option<String>,
    http: reqwest::Client,
}

impl PaperclipClient {
    pub fn new(url: &str, key: Option<&str>) -> Result<Self, String> {
        let base = normalize_url(url)?;
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            // The key header must never follow a redirect to another host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| crate::http_error::transport(&e, &base))?;
        Ok(Self {
            base,
            key: key
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_string),
            http,
        })
    }

    /// A client for the saved address and key.
    pub fn saved() -> Result<Self, String> {
        let url = crate::config::AppSettings::load().paperclip.url;
        if url.is_empty() {
            return Err(err(ERR_NOT_CONFIGURED).into());
        }
        Self::new(&url, crate::secrets::paperclip_key_get()?.as_deref())
    }

    /// A client for a form's values: a missing address falls back to the saved one; a saved
    /// key is only ever used for the saved address.
    pub fn from_form(url: Option<&str>, key: Option<&str>) -> Result<Self, String> {
        let saved = crate::config::AppSettings::load().paperclip.url;
        let url = url
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .unwrap_or(&saved);
        if url.is_empty() {
            return Err(err(ERR_NOT_CONFIGURED).into());
        }
        let key = match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => Some(k.to_string()),
            None if normalize_url(url)? == saved => crate::secrets::paperclip_key_get()?,
            None => None,
        };
        Self::new(url, key.as_deref())
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
        let url = format!("{}/api/{}", self.base, path);
        let mut req = self
            .http
            .get(&url)
            .header("Accept", "application/json")
            .query(query);
        if let Some(key) = &self.key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(status_error(status.as_u16()));
        }
        resp.json::<Value>()
            .await
            .map_err(|_| err("paperclip.invalid_response").into())
    }

    pub async fn health(&self) -> Result<PaperclipHealth, String> {
        // The health route answers 503 with a body when the database is down.
        let url = format!("{}/api/health", self.base);
        let mut req = self.http.get(&url).header("Accept", "application/json");
        if let Some(key) = &self.key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        let code = resp.status().as_u16();
        let v = resp.json::<Value>().await.ok();
        let Some(status) = v
            .as_ref()
            .and_then(|v| v.get("status"))
            .and_then(Value::as_str)
        else {
            return Err(if code >= 400 {
                status_error(code)
            } else {
                err("paperclip.invalid_response").into()
            });
        };
        let v = v.as_ref().unwrap_or(&Value::Null);
        Ok(PaperclipHealth {
            status: status.to_string(),
            version: str_of(v, &["version", "serverVersion"]),
            deployment_mode: str_of(v, &["deploymentMode"]),
        })
    }

    pub async fn companies(&self) -> Result<Vec<Company>, String> {
        let v = self.get("companies", &[]).await?;
        Ok(array(&v)?.iter().filter_map(parse_company).collect())
    }

    /// Connection test: the instance must answer and the key must list companies.
    pub async fn test(&self) -> Result<Connection, String> {
        let health = self.health().await?;
        let companies = self.companies().await?;
        Ok(Connection {
            ok: true,
            url: self.base.clone(),
            health,
            companies,
        })
    }

    pub async fn agents(&self, company: &str) -> Result<Vec<Agent>, String> {
        let v = self
            .get(&format!("companies/{}/agents", segment(company)?), &[])
            .await?;
        Ok(array(&v)?.iter().filter_map(parse_agent).collect())
    }

    pub async fn tasks(&self, company: &str) -> Result<Vec<Task>, String> {
        let v = self
            .get(&format!("companies/{}/issues", segment(company)?), &[])
            .await?;
        Ok(array(&v)?
            .iter()
            .filter_map(parse_task)
            .take(MAX_TASKS)
            .collect())
    }

    pub async fn runs(&self, company: &str, limit: u32) -> Result<Vec<Run>, String> {
        let q = [
            ("limit", limit.clamp(1, MAX_RUNS).to_string()),
            ("summary", "true".to_string()),
        ];
        let v = self
            .get(
                &format!("companies/{}/heartbeat-runs", segment(company)?),
                &q,
            )
            .await?;
        Ok(array(&v)?.iter().filter_map(parse_run).collect())
    }

    async fn budget(&self, company: &str) -> Result<Budget, String> {
        let v = self
            .get(&format!("companies/{}/dashboard", segment(company)?), &[])
            .await?;
        let n = |p: &str| v.pointer(p).and_then(Value::as_f64).unwrap_or(0.0);
        Ok(Budget {
            month_spend_cents: n("/costs/monthSpendCents"),
            month_budget_cents: n("/costs/monthBudgetCents"),
            utilization_percent: n("/costs/monthUtilizationPercent"),
            pending_approvals: n("/pendingApprovals") as u64,
            paused_agents: n("/budgets/pausedAgents") as u64,
            active_incidents: n("/budgets/activeIncidents") as u64,
        })
    }

    async fn costs(&self, company: &str) -> Result<Vec<AgentCost>, String> {
        let v = self
            .get(
                &format!("companies/{}/costs/by-agent", segment(company)?),
                &[],
            )
            .await?;
        Ok(array(&v)?
            .iter()
            .filter_map(|r| {
                let num = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                Some(AgentCost {
                    agent_id: id_of(r.get("agentId")?)?,
                    cost_cents: num("costCents"),
                    input_tokens: num("inputTokens"),
                    output_tokens: num("outputTokens"),
                })
            })
            .collect())
    }

    /// Teams, agents, tasks, runs and budgets of one company in one go.
    pub async fn overview(
        &self,
        company_id: &str,
        links: &BTreeMap<String, String>,
    ) -> Result<Overview, String> {
        let company = self
            .get(&format!("companies/{}", segment(company_id)?), &[])
            .await?;
        let company = parse_company(&company)
            .ok_or_else(|| String::from(err("paperclip.invalid_response")))?;
        let (health, agents, tasks, runs, budget, costs) = tokio::join!(
            self.health(),
            self.agents(company_id),
            self.tasks(company_id),
            self.runs(company_id, 25),
            self.budget(company_id),
            self.costs(company_id),
        );
        let agents = agents?;
        let mut tasks = tasks?;
        for t in &mut tasks {
            t.harness_project_id = links.get(&t.id).cloned();
        }
        Ok(Overview {
            company,
            health: health?,
            teams: teams_of(&agents),
            agents,
            tasks,
            runs: runs?,
            // Budgets and costs need extra read rights; their absence is not an error.
            budget: budget.ok(),
            costs: costs.unwrap_or_default(),
        })
    }

    /// The overview of the saved company (the only one when the instance has just one).
    pub async fn saved_overview() -> Result<Overview, String> {
        let client = Self::saved()?;
        let settings = crate::config::AppSettings::load().paperclip;
        let company = if settings.company_id.is_empty() {
            let mut all = client.companies().await?;
            match all.len() {
                0 => return Err(err(ERR_NO_COMPANY).into()),
                1 => all.remove(0).id,
                _ => return Err(err("paperclip.company_required").into()),
            }
        } else {
            settings.company_id
        };
        client.overview(&company, &settings.task_links).await
    }
}

// --- Parsing ---

fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array()
        .or_else(|| v.get("data").and_then(Value::as_array))
        .ok_or_else(|| err("paperclip.invalid_response").into())
}

fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn str_of(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn cents(v: &Value, key: &str) -> i64 {
    v.get(key)
        .and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)))
        .unwrap_or(0)
}

fn parse_company(v: &Value) -> Option<Company> {
    Some(Company {
        id: id_of(v.get("id")?)?,
        name: str_of(v, &["name"]).unwrap_or_default(),
        status: str_of(v, &["status"]).unwrap_or_else(|| "active".into()),
        budget_monthly_cents: cents(v, "budgetMonthlyCents"),
    })
}

fn parse_agent(v: &Value) -> Option<Agent> {
    let model = v.get("adapterConfig").and_then(|c| str_of(c, &["model"]));
    Some(Agent {
        id: id_of(v.get("id")?)?,
        name: str_of(v, &["name"]).unwrap_or_default(),
        role: str_of(v, &["role"]).unwrap_or_default(),
        title: str_of(v, &["title"]),
        reports_to: v.get("reportsTo").and_then(id_of),
        status: str_of(v, &["status"]).unwrap_or_else(|| "unknown".into()),
        adapter_type: str_of(v, &["adapterType"]).unwrap_or_default(),
        uses_harness: model.as_deref().is_some_and(|m| m.starts_with("harness/")),
        model,
        budget_monthly_cents: cents(v, "budgetMonthlyCents"),
        spent_monthly_cents: cents(v, "spentMonthlyCents"),
    })
}

fn parse_task(v: &Value) -> Option<Task> {
    Some(Task {
        id: id_of(v.get("id")?)?,
        identifier: str_of(v, &["identifier"]),
        title: str_of(v, &["title"]).unwrap_or_default(),
        status: str_of(v, &["status"]).unwrap_or_else(|| "unknown".into()),
        priority: str_of(v, &["priority"]),
        assignee_agent_id: v.get("assigneeAgentId").and_then(id_of),
        project_id: v.get("projectId").and_then(id_of),
        updated_at: str_of(v, &["updatedAt"]),
        harness_project_id: None,
    })
}

fn parse_run(v: &Value) -> Option<Run> {
    Some(Run {
        id: id_of(v.get("id")?)?,
        agent_id: v.get("agentId").and_then(id_of).unwrap_or_default(),
        issue_id: v.get("issueId").and_then(id_of),
        status: str_of(v, &["status"]).unwrap_or_else(|| "unknown".into()),
        source: str_of(v, &["invocationSource"]),
        started_at: str_of(v, &["startedAt"]),
        finished_at: str_of(v, &["finishedAt"]),
        error: str_of(v, &["error"]).map(|e| clip(&e, 400)),
    })
}

/// Reporting lines: every agent that has at least one report is a team lead.
fn teams_of(agents: &[Agent]) -> Vec<Team> {
    agents
        .iter()
        .filter_map(|lead| {
            let members: Vec<String> = agents
                .iter()
                .filter(|a| a.reports_to.as_deref() == Some(lead.id.as_str()))
                .map(|a| a.id.clone())
                .collect();
            (!members.is_empty()).then(|| Team {
                lead_id: lead.id.clone(),
                lead_name: lead.name.clone(),
                member_ids: members,
            })
        })
        .collect()
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
        return Err(err("paperclip.id_invalid").into());
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
        401 => err("paperclip.unauthorized").into(),
        403 => err("paperclip.forbidden").into(),
        404 => err("paperclip.api_missing").into(),
        301 | 302 | 303 | 307 | 308 => err("paperclip.redirect").into(),
        s if s >= 500 => err("paperclip.server_error").num("status", s).into(),
        s => err("paperclip.http_error").num("status", s).into(),
    }
}

// --- Gateway side: Harness models for a Paperclip agent ---

/// Points an existing `opencode_local` Paperclip agent at a Harness model: creates a token
/// for exactly that model (chat and model-list endpoints only) and writes the `harness`
/// provider into the agent's config through the documented GET/PATCH agent API. The token
/// is revoked again when the write fails.
pub async fn use_harness_model(
    agent_id: &str,
    model: &str,
    gateway_base: &str,
) -> Result<ModelApplied, String> {
    let agent = segment(agent_id)?.to_string();
    let url = crate::config::AppSettings::load().paperclip.url;
    if url.is_empty() {
        return Err(err(ERR_NOT_CONFIGURED).into());
    }
    let key = crate::secrets::paperclip_key_get()?.unwrap_or_default();
    let (base, model_name) = (gateway_base.to_string(), model.to_string());
    let handoff = tokio::task::spawn_blocking(move || {
        crate::modules::paperclip::generate(&base, &model_name)
    })
    .await
    .map_err(|e| e.to_string())??;
    let applied =
        crate::modules::paperclip::apply(url, agent.clone(), key, handoff.config.clone()).await;
    if let Err(e) = applied {
        let _ = crate::modules::paperclip::revoke(&handoff.token_id);
        return Err(e);
    }
    Ok(ModelApplied {
        agent_id: agent,
        model: model.to_string(),
        token_id: handoff.token_id,
    })
}
