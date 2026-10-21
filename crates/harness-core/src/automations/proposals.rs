//! Proposals: what the Playground's model wants to do on the user's n8n, held until the user
//! decides.
//!
//! A model never changes n8n itself. Its tools ([`super::tools`]) *stage* a proposal here: the
//! workflow JSON is checked and normalized, compared with what n8n holds now, and kept with a
//! diff and warnings. Only [`ProposalStore::apply`] writes to n8n, and nothing the model can
//! call reaches it: it is run by the UI's Approve button (or an admin Control API token).
//!
//! The connection's [`N8nPermission`] is checked twice, when a proposal is staged and again
//! when it is applied, and a workflow that changed in n8n since it was staged is refused
//! (the diff the user approved would no longer be the change that happens).

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::watch;

use super::diff::{self, NodeChange, Warning, WorkflowDiff};
use super::report::ExecutionReport;
use super::{clip, value_id, N8nClient, N8nPermission, TriggerResult, WebhookTrigger, Workflow};
use crate::error_key::err;

/// A pending proposal nobody decided on is dropped after this long.
const PENDING_TTL_MS: u64 = 30 * 60 * 1000;
/// Proposals kept (finished ones are dropped first, oldest first).
const MAX_PROPOSALS: usize = 50;
const MAX_NODES: usize = 200;
const MAX_PAYLOAD_BYTES: usize = 20_000;
/// How long a run is followed after its webhook answered.
const RUN_WAIT: Duration = Duration::from_secs(15);
/// Retries of a webhook that answers 404 right after activation.
const WEBHOOK_RETRIES: usize = 6;
const WEBHOOK_RETRY_EVERY: Duration = Duration::from_secs(1);

/// Workflow settings the public API accepts (anything else is a 400). `binaryMode` is ignored
/// by n8n and left out.
const SETTINGS_KEYS: &[&str] = &[
    "saveExecutionProgress",
    "saveManualExecutions",
    "saveDataErrorExecution",
    "saveDataSuccessExecution",
    "executionTimeout",
    "errorWorkflow",
    "timezone",
    "executionOrder",
    "callerPolicy",
    "callerIds",
    "availableInMCP",
];

/// Keys of a node the public API accepts.
const NODE_KEYS: &[&str] = &[
    "id",
    "name",
    "webhookId",
    "disabled",
    "notesInFlow",
    "notes",
    "type",
    "typeVersion",
    "executeOnce",
    "alwaysOutputData",
    "retryOnFail",
    "maxTries",
    "waitBetweenTries",
    "continueOnFail",
    "onError",
    "position",
    "parameters",
    "credentials",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nProposalKind))]
pub enum ProposalKind {
    CreateWorkflow,
    UpdateWorkflow,
    RunWorkflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nProposalStatus))]
pub enum ProposalStatus {
    /// Waiting for the user.
    Pending,
    /// Approved and being written to n8n.
    Applying,
    Applied,
    Rejected,
    /// Approved, but n8n refused or a step failed (see `error` and `result`).
    Failed,
    /// Nobody decided in time.
    Expired,
}

impl ProposalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}

/// What an approved proposal did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nApplyResult))]
pub struct ApplyResult {
    /// The workflow as n8n holds it after the change.
    pub workflow: Option<Workflow>,
    /// `created`, `updated`, `activated`, `deactivated`, `triggered`.
    pub steps: Vec<String>,
    /// The webhook's answer (runs only).
    pub trigger: Option<TriggerResult>,
    /// The execution the webhook started (runs only; `None` when n8n stored none).
    pub execution: Option<ExecutionReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nProposal))]
pub struct Proposal {
    pub id: String,
    /// The tool call that staged it (the Playground matches its tool card by this).
    pub call_id: Option<String>,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub created_at_ms: u64,
    pub decided_at_ms: Option<u64>,
    /// The n8n address it was staged for.
    pub connection_url: String,
    pub workflow_id: Option<String>,
    pub workflow_name: String,
    /// `Some(true)`: go live, `Some(false)`: switch off, `None`: leave as it is.
    pub activate: Option<bool>,
    pub node_changes: Vec<NodeChange>,
    pub warnings: Vec<Warning>,
    pub diff: WorkflowDiff,
    /// Runs: the webhook that will be called.
    pub webhook: Option<WebhookTrigger>,
    /// Runs: the body sent to it.
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
    pub payload: Option<Value>,
    /// The model's one-line reason.
    pub note: Option<String>,
    /// The lowest connection permission that allows it.
    pub required_permission: N8nPermission,
    pub result: Option<ApplyResult>,
    /// Keyed error text when `status` is `failed`.
    pub error: Option<String>,
    /// What is sent to n8n (create/update); the UI shows `diff` instead.
    #[serde(skip)]
    body: Option<Value>,
    /// `updatedAt` / `versionId` of the workflow when it was staged (updates and runs).
    #[serde(skip)]
    base_version: Option<String>,
}

pub struct ProposalStore {
    items: Mutex<Vec<Proposal>>,
    changed: watch::Sender<u64>,
}

impl Default for ProposalStore {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    crate::analytics::now_ms()
}

fn denied(need: N8nPermission) -> String {
    err("automations.n8n_permission_denied")
        .with("need", need.as_str())
        .into()
}

fn invalid(reason: impl std::fmt::Display) -> String {
    err("automations.n8n_workflow_invalid")
        .with("reason", reason)
        .into()
}

/// A workflow body as the public API takes it, checked: `name`, `nodes`, `connections`,
/// `settings` and nothing else. `default_name` is used when the model left the name out (an
/// edit).
pub fn sanitize_workflow(raw: &Value, default_name: Option<&str>) -> Result<Value, String> {
    sanitize_against(raw, default_name, None)
}

/// [`sanitize_workflow`] for an edit: a node the model sends without an `id` or `webhookId`
/// gets the ones the node of the same name has in `existing` (the workflow as n8n holds it), so
/// an edit does not turn every node into a new one.
pub fn sanitize_against(
    raw: &Value,
    default_name: Option<&str>,
    existing: Option<&Value>,
) -> Result<Value, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| invalid("the workflow must be a JSON object"))?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .or(default_name)
        .ok_or_else(|| invalid("the workflow needs a name"))?;
    if name.chars().count() > 128 {
        return Err(invalid("the name is longer than 128 characters"));
    }
    let raw_nodes = obj
        .get("nodes")
        .and_then(Value::as_array)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| invalid("nodes must be a non-empty array"))?;
    if raw_nodes.len() > MAX_NODES {
        return Err(invalid(format!("more than {MAX_NODES} nodes")));
    }
    let mut nodes = Vec::with_capacity(raw_nodes.len());
    let mut names: Vec<String> = Vec::new();
    for (i, node) in raw_nodes.iter().enumerate() {
        let n = node
            .as_object()
            .ok_or_else(|| invalid(format!("node {i} is not an object")))?;
        let node_name = n
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid(format!("node {i} has no name")))?;
        if names.iter().any(|x| x == node_name) {
            return Err(invalid(format!("two nodes are named \"{node_name}\"")));
        }
        let ty = n
            .get("type")
            .and_then(Value::as_str)
            .filter(|t| t.contains('.'))
            .ok_or_else(|| {
                invalid(format!(
                    "node \"{node_name}\" needs a full type such as n8n-nodes-base.httpRequest"
                ))
            })?;
        let mut out = Map::new();
        for key in NODE_KEYS {
            if let Some(v) = n.get(*key) {
                out.insert((*key).to_string(), v.clone());
            }
        }
        out.insert("name".into(), json!(node_name));
        out.insert("type".into(), json!(ty));
        match out.get("typeVersion") {
            Some(v) if v.is_number() => {}
            None => {
                out.insert("typeVersion".into(), json!(1));
            }
            Some(_) => {
                return Err(invalid(format!(
                    "node \"{node_name}\": typeVersion must be a number"
                )))
            }
        }
        match out.get("position") {
            Some(Value::Array(p)) if p.len() == 2 && p.iter().all(Value::is_number) => {}
            None => {
                out.insert("position".into(), json!([250 + 220 * i as i64, 300]));
            }
            Some(_) => {
                return Err(invalid(format!(
                    "node \"{node_name}\": position must be [x, y]"
                )))
            }
        }
        match out.get("parameters") {
            Some(Value::Object(_)) => {}
            None => {
                out.insert("parameters".into(), json!({}));
            }
            Some(_) => {
                return Err(invalid(format!(
                    "node \"{node_name}\": parameters must be an object"
                )))
            }
        }
        let prior = existing
            .and_then(|e| e.get("nodes"))
            .and_then(Value::as_array)
            .and_then(|nodes| {
                nodes
                    .iter()
                    .find(|n| n.get("name").and_then(Value::as_str) == Some(node_name))
            });
        for key in ["id", "webhookId"] {
            if !out.contains_key(key) {
                if let Some(v) = prior.and_then(|p| p.get(key)) {
                    out.insert(key.to_string(), v.clone());
                }
            }
        }
        if !out.get("id").is_some_and(|v| v.as_str().is_some()) {
            out.insert("id".into(), json!(uuid::Uuid::new_v4().to_string()));
        }
        names.push(node_name.to_string());
        nodes.push(Value::Object(out));
    }
    let connections = match obj.get("connections") {
        None | Some(Value::Null) => json!({}),
        Some(c) if c.is_object() => c.clone(),
        Some(_) => return Err(invalid("connections must be an object")),
    };
    for (from, outputs) in connections.as_object().into_iter().flatten() {
        if !names.contains(from) {
            return Err(invalid(format!(
                "connections start at \"{from}\", which is not a node"
            )));
        }
        for target in outputs
            .as_object()
            .into_iter()
            .flat_map(Map::values)
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(Value::as_array)
            .flatten()
        {
            let to = target.get("node").and_then(Value::as_str).unwrap_or("");
            if !names.iter().any(|x| x == to) {
                return Err(invalid(format!(
                    "a connection from \"{from}\" goes to \"{to}\", which is not a node"
                )));
            }
        }
    }
    let mut settings = Map::new();
    match obj.get("settings") {
        None | Some(Value::Null) => {}
        Some(Value::Object(s)) => {
            for key in SETTINGS_KEYS {
                if let Some(v) = s.get(*key) {
                    settings.insert((*key).to_string(), v.clone());
                }
            }
        }
        Some(_) => return Err(invalid("settings must be an object")),
    }
    settings
        .entry("executionOrder")
        .or_insert_with(|| json!("v1"));
    Ok(json!({
        "name": name,
        "nodes": nodes,
        "connections": connections,
        "settings": settings,
    }))
}

/// The comparable body of a workflow n8n holds: the same normalization without refusing
/// anything (an existing workflow may use more than the API accepts back).
fn existing_body(v: &Value) -> Value {
    sanitize_workflow(v, Some("")).unwrap_or_else(|_| {
        json!({
            "name": v.get("name").cloned().unwrap_or(Value::Null),
            "nodes": v.get("nodes").cloned().unwrap_or_else(|| json!([])),
            "connections": v.get("connections").cloned().unwrap_or_else(|| json!({})),
            "settings": v.get("settings").cloned().unwrap_or_else(|| json!({})),
        })
    })
}

fn version_of(v: &Value) -> Option<String> {
    ["versionId", "updatedAt"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_string)
}

/// What the model asks for when it stages a workflow change.
#[derive(Debug, Clone, Default)]
pub struct StageWorkflow {
    pub call_id: Option<String>,
    /// `None` = a new workflow.
    pub workflow_id: Option<String>,
    /// The new content. `None` on an edit = only change the live state.
    pub workflow: Option<Value>,
    pub activate: Option<bool>,
    pub note: Option<String>,
}

impl ProposalStore {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
            changed: watch::channel(0).0,
        }
    }

    /// The proposals of this process.
    pub fn global() -> &'static ProposalStore {
        static STORE: OnceLock<ProposalStore> = OnceLock::new();
        STORE.get_or_init(ProposalStore::new)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Proposal>> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn notify(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    fn expire(items: &mut [Proposal]) {
        let now = now_ms();
        for p in items.iter_mut() {
            if p.status == ProposalStatus::Pending && now > p.created_at_ms + PENDING_TTL_MS {
                p.status = ProposalStatus::Expired;
                p.decided_at_ms = Some(now);
            }
        }
    }

    fn push(&self, p: Proposal) -> Proposal {
        let mut items = self.lock();
        Self::expire(&mut items);
        items.push(p.clone());
        while items.len() > MAX_PROPOSALS {
            let Some(i) = items.iter().position(|x| {
                !matches!(x.status, ProposalStatus::Pending | ProposalStatus::Applying)
            }) else {
                break;
            };
            items.remove(i);
        }
        drop(items);
        self.notify();
        p
    }

    pub fn get(&self, id: &str) -> Option<Proposal> {
        let mut items = self.lock();
        Self::expire(&mut items);
        items.iter().find(|p| p.id == id).cloned()
    }

    /// The proposal a tool call staged.
    pub fn for_call(&self, call_id: &str) -> Option<Proposal> {
        let mut items = self.lock();
        Self::expire(&mut items);
        items
            .iter()
            .rev()
            .find(|p| p.call_id.as_deref() == Some(call_id))
            .cloned()
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Proposal> {
        let mut items = self.lock();
        Self::expire(&mut items);
        items.iter().rev().cloned().collect()
    }

    /// Waits until the proposal is decided and applied (or `max` is over); returns it as it is then.
    pub async fn wait(&self, id: &str, max: Duration) -> Option<Proposal> {
        let mut rx = self.changed.subscribe();
        let deadline = tokio::time::Instant::now() + max;
        loop {
            let p = self.get(id)?;
            if !matches!(p.status, ProposalStatus::Pending | ProposalStatus::Applying)
                || tokio::time::Instant::now() >= deadline
            {
                return Some(p);
            }
            tokio::select! {
                _ = rx.changed() => {}
                _ = tokio::time::sleep_until(deadline) => {}
                // A pending proposal expires without anyone touching the store.
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    }

    /// Stages a new workflow or an edit.
    pub async fn stage_workflow(
        &self,
        client: &N8nClient,
        permission: N8nPermission,
        req: StageWorkflow,
    ) -> Result<Proposal, String> {
        if !permission.can_draft() {
            return Err(denied(N8nPermission::Draft));
        }
        let existing = match req.workflow_id.as_deref() {
            Some(id) => {
                let json = client.workflow_json(id).await?;
                let wf = client
                    .parse_workflow(&json)
                    .ok_or_else(|| String::from(err("automations.n8n_invalid_response")))?;
                Some((json, wf))
            }
            None => None,
        };
        let live = existing.as_ref().is_some_and(|(_, w)| w.active);
        if live && !permission.can_write() {
            return Err(denied(N8nPermission::Write));
        }
        // A switch that changes nothing is dropped; one that does needs `write`.
        let activate = req.activate.filter(|a| match &existing {
            Some((_, w)) => w.active != *a,
            None => *a,
        });
        if activate.is_some() && !permission.can_write() {
            return Err(denied(N8nPermission::Write));
        }
        let (kind, workflow_id, name, before, body) = match &existing {
            None => {
                let raw = req
                    .workflow
                    .as_ref()
                    .ok_or_else(|| invalid("a new workflow needs its content"))?;
                let body = sanitize_workflow(raw, None)?;
                let name = body["name"].as_str().unwrap_or("").to_string();
                (ProposalKind::CreateWorkflow, None, name, None, Some(body))
            }
            Some((json, wf)) => {
                let before = existing_body(json);
                let body = req
                    .workflow
                    .as_ref()
                    .map(|raw| sanitize_against(raw, Some(&wf.name), Some(json)))
                    .transpose()?;
                if body.is_none() && activate.is_none() {
                    return Err(invalid("nothing to change"));
                }
                // An edit that changes nothing is not worth a review.
                let body = body.filter(|b| diff::normalized(b) != diff::normalized(&before));
                if body.is_none() && activate.is_none() {
                    return Err(invalid("the workflow is already like this"));
                }
                let name = body
                    .as_ref()
                    .and_then(|b| b["name"].as_str())
                    .unwrap_or(&wf.name)
                    .to_string();
                (
                    ProposalKind::UpdateWorkflow,
                    Some(wf.id.clone()),
                    name,
                    Some(before),
                    body,
                )
            }
        };
        let (diff_view, node_changes, warnings) = match &body {
            Some(b) => (
                diff::diff_workflows(before.as_ref(), b),
                diff::node_changes(before.as_ref(), b),
                diff::warnings(
                    before.as_ref(),
                    b,
                    activate == Some(true),
                    live && activate != Some(false),
                ),
            ),
            None => (
                WorkflowDiff::default(),
                Vec::new(),
                diff::warnings(
                    before.as_ref(),
                    &before.clone().unwrap_or(Value::Null),
                    activate == Some(true),
                    false,
                ),
            ),
        };
        Ok(self.push(Proposal {
            id: uuid::Uuid::new_v4().to_string(),
            call_id: req.call_id,
            kind,
            status: ProposalStatus::Pending,
            created_at_ms: now_ms(),
            decided_at_ms: None,
            connection_url: client.base.clone(),
            workflow_id,
            workflow_name: name,
            activate,
            node_changes,
            warnings,
            diff: diff_view,
            webhook: None,
            payload: None,
            note: req
                .note
                .map(|n| clip(n.trim(), 300))
                .filter(|n| !n.is_empty()),
            required_permission: if activate.is_some() || live {
                N8nPermission::Write
            } else {
                N8nPermission::Draft
            },
            result: None,
            error: None,
            base_version: existing.as_ref().and_then(|(j, _)| version_of(j)),
            body,
        }))
    }

    /// Stages a run through the workflow's webhook trigger.
    pub async fn stage_run(
        &self,
        client: &N8nClient,
        permission: N8nPermission,
        call_id: Option<String>,
        workflow_id: &str,
        payload: Option<Value>,
        note: Option<String>,
    ) -> Result<Proposal, String> {
        if !permission.can_trigger() {
            return Err(denied(N8nPermission::Trigger));
        }
        let json = client.workflow_json(workflow_id).await?;
        let wf = client
            .parse_workflow(&json)
            .ok_or_else(|| String::from(err("automations.n8n_invalid_response")))?;
        let webhook = wf
            .webhook
            .clone()
            .ok_or_else(|| String::from(err("automations.n8n_no_webhook")))?;
        if !wf.active {
            return Err(err("automations.n8n_workflow_inactive").into());
        }
        if let Some(p) = &payload {
            if p.to_string().len() > MAX_PAYLOAD_BYTES {
                return Err(invalid("the payload is larger than 20 KB"));
            }
        }
        Ok(self.push(Proposal {
            id: uuid::Uuid::new_v4().to_string(),
            call_id,
            kind: ProposalKind::RunWorkflow,
            status: ProposalStatus::Pending,
            created_at_ms: now_ms(),
            decided_at_ms: None,
            connection_url: client.base.clone(),
            workflow_id: Some(wf.id.clone()),
            workflow_name: wf.name.clone(),
            activate: None,
            node_changes: Vec::new(),
            warnings: Vec::new(),
            diff: WorkflowDiff::default(),
            webhook: Some(webhook),
            payload,
            note: note.map(|n| clip(n.trim(), 300)).filter(|n| !n.is_empty()),
            required_permission: N8nPermission::Trigger,
            result: None,
            error: None,
            base_version: version_of(&json),
            body: None,
        }))
    }

    /// The user said no.
    pub fn reject(&self, id: &str) -> Result<Proposal, String> {
        let mut items = self.lock();
        Self::expire(&mut items);
        let p = items
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| String::from(err("automations.n8n_proposal_not_found")))?;
        if p.status != ProposalStatus::Pending {
            return Err(err("automations.n8n_proposal_closed")
                .with("status", p.status.as_str())
                .into());
        }
        p.status = ProposalStatus::Rejected;
        p.decided_at_ms = Some(now_ms());
        let out = p.clone();
        drop(items);
        self.notify();
        Ok(out)
    }

    /// The user said yes: writes to n8n. A proposal that cannot be applied at all (unknown,
    /// already decided, another connection, not allowed any more) is an `Err` and stays as it
    /// was; one that was attempted comes back `applied` or `failed`.
    pub async fn apply(
        &self,
        id: &str,
        client: &N8nClient,
        permission: N8nPermission,
    ) -> Result<Proposal, String> {
        let p = {
            let mut items = self.lock();
            Self::expire(&mut items);
            let p = items
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| String::from(err("automations.n8n_proposal_not_found")))?;
            if p.status != ProposalStatus::Pending {
                return Err(err("automations.n8n_proposal_closed")
                    .with("status", p.status.as_str())
                    .into());
            }
            if p.connection_url != client.base {
                return Err(err("automations.n8n_connection_changed").into());
            }
            let allowed = match p.kind {
                ProposalKind::RunWorkflow => permission.can_trigger(),
                _ if p.required_permission >= N8nPermission::Write => permission.can_write(),
                _ => permission.can_draft(),
            };
            if !allowed {
                return Err(denied(p.required_permission));
            }
            p.status = ProposalStatus::Applying;
            p.clone()
        };
        self.notify();

        let mut result = ApplyResult::default();
        let outcome = run_apply(client, permission, &p, &mut result).await;
        let mut items = self.lock();
        if let Some(slot) = items.iter_mut().find(|x| x.id == id) {
            slot.decided_at_ms = Some(now_ms());
            slot.result = Some(result);
            match outcome {
                Ok(()) => slot.status = ProposalStatus::Applied,
                Err(e) => {
                    slot.status = ProposalStatus::Failed;
                    slot.error = Some(e);
                }
            }
        }
        let out = items.iter().find(|x| x.id == id).cloned();
        drop(items);
        self.notify();
        out.ok_or_else(|| err("automations.n8n_proposal_not_found").into())
    }
}

/// The writes of one proposal. Steps already done stay in `result` when a later one fails.
async fn run_apply(
    client: &N8nClient,
    permission: N8nPermission,
    p: &Proposal,
    result: &mut ApplyResult,
) -> Result<(), String> {
    match p.kind {
        ProposalKind::CreateWorkflow => {
            let body = p.body.as_ref().ok_or_else(|| invalid("no content"))?;
            let created = client.create_workflow(body).await?;
            let id = created
                .get("id")
                .and_then(value_id)
                .ok_or_else(|| String::from(err("automations.n8n_invalid_response")))?;
            result.steps.push("created".into());
            result.workflow = client.workflow(&id).await.ok();
            if p.activate == Some(true) {
                client.activate_workflow(&id).await?;
                result.steps.push("activated".into());
                result.workflow = client.workflow(&id).await.ok();
            }
            Ok(())
        }
        ProposalKind::UpdateWorkflow => {
            let id = p
                .workflow_id
                .as_deref()
                .ok_or_else(|| invalid("no workflow"))?;
            let current = client.workflow_json(id).await?;
            if version_of(&current) != p.base_version {
                return Err(err("automations.n8n_proposal_stale").into());
            }
            let live = client.parse_workflow(&current).is_some_and(|w| w.active);
            if live && !permission.can_write() {
                return Err(denied(N8nPermission::Write));
            }
            if let Some(body) = &p.body {
                client.update_workflow(id, body).await?;
                result.steps.push("updated".into());
            }
            match p.activate {
                Some(true) => {
                    client.activate_workflow(id).await?;
                    result.steps.push("activated".into());
                }
                Some(false) => {
                    client.deactivate_workflow(id).await?;
                    result.steps.push("deactivated".into());
                }
                None => {}
            }
            result.workflow = client.workflow(id).await.ok();
            Ok(())
        }
        ProposalKind::RunWorkflow => {
            let id = p
                .workflow_id
                .as_deref()
                .ok_or_else(|| invalid("no workflow"))?;
            let current = client.workflow_json(id).await?;
            let wf = client
                .parse_workflow(&current)
                .ok_or_else(|| String::from(err("automations.n8n_invalid_response")))?;
            // The webhook the user approved must still be the one that is called.
            if wf.webhook != p.webhook || version_of(&current) != p.base_version {
                return Err(err("automations.n8n_proposal_stale").into());
            }
            let before = client.newest_execution_id(id).await.unwrap_or(None);
            // n8n registers a webhook a moment after the workflow goes live: a 404 means
            // nothing ran, so asking again is safe.
            let mut trigger = client.trigger(id, p.payload.as_ref()).await?;
            for _ in 0..WEBHOOK_RETRIES {
                if trigger.status != 404 {
                    break;
                }
                tokio::time::sleep(WEBHOOK_RETRY_EVERY).await;
                trigger = client.trigger(id, p.payload.as_ref()).await?;
            }
            result.steps.push("triggered".into());
            let ok = trigger.ok;
            result.trigger = Some(trigger);
            result.workflow = Some(wf);
            if ok {
                // The run is read on a best-effort basis: the webhook has already fired.
                result.execution = client
                    .await_execution(id, before, RUN_WAIT)
                    .await
                    .unwrap_or(None);
            }
            Ok(())
        }
    }
}
