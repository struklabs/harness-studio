//! The n8n tools of the Playground: what a chat model can do with the user's n8n.
//!
//! They exist only while an n8n connection is saved, and which ones depends on the
//! connection's [`N8nPermission`]:
//!
//! | permission | tools |
//! |---|---|
//! | read only | `n8n_list_workflows`, `n8n_get_workflow`, `n8n_list_executions`, `n8n_get_execution` |
//! | draft | + `n8n_propose_workflow`, `n8n_proposal_status` |
//! | trigger | + `n8n_propose_run` (draft and write change what `propose_workflow` may do) |
//!
//! The propose tools only *stage* a proposal ([`super::proposals`]); they wait for the user's
//! decision in the UI (up to the call's time limit) and hand the outcome back to the model, so
//! the model sees the execution result. Each call also checks the *current* permission, so a
//! tool that is still registered after the user lowered it refuses.

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::proposals::{Proposal, ProposalStatus, ProposalStore, StageWorkflow};
use super::{N8nClient, N8nPermission};
use crate::tools::{
    PermissionClass, Tool, ToolCtx, ToolDef, ToolError, ToolErrorCode, ToolOutput, ToolRegistry,
    ToolSource,
};

pub const LIST_WORKFLOWS: &str = "n8n_list_workflows";
pub const GET_WORKFLOW: &str = "n8n_get_workflow";
pub const LIST_EXECUTIONS: &str = "n8n_list_executions";
pub const GET_EXECUTION: &str = "n8n_get_execution";
pub const PROPOSE_WORKFLOW: &str = "n8n_propose_workflow";
pub const PROPOSE_RUN: &str = "n8n_propose_run";
pub const PROPOSAL_STATUS: &str = "n8n_proposal_status";

const ALL: &[&str] = &[
    LIST_WORKFLOWS,
    GET_WORKFLOW,
    LIST_EXECUTIONS,
    GET_EXECUTION,
    PROPOSE_WORKFLOW,
    PROPOSE_RUN,
    PROPOSAL_STATUS,
];

/// The tool names offered at a permission level.
pub fn tool_names(permission: N8nPermission) -> Vec<&'static str> {
    let mut names = vec![LIST_WORKFLOWS, GET_WORKFLOW, LIST_EXECUTIONS, GET_EXECUTION];
    if permission.can_draft() {
        names.extend([PROPOSE_WORKFLOW, PROPOSAL_STATUS]);
    }
    if permission.can_trigger() {
        names.push(PROPOSE_RUN);
    }
    names
}

/// Registers the n8n tools the saved connection allows and removes the others.
pub fn sync(registry: &ToolRegistry) {
    let settings = crate::config::AppSettings::load().n8n;
    let connected =
        !settings.url.is_empty() && crate::secrets::n8n_key_get().ok().flatten().is_some();
    let wanted = if connected {
        tool_names(settings.permission)
    } else {
        Vec::new()
    };
    for name in ALL {
        if !wanted.contains(name) {
            registry.unregister(name);
        }
    }
    for name in wanted {
        if let Some(tool) = tool(name) {
            registry.register(tool);
        }
    }
}

/// Brings the shared tool registry up to date after the connection or its permission changed.
pub fn refresh() {
    if let Some(service) = crate::tools::ToolService::try_shared() {
        sync(service.registry());
    }
}

fn def(name: &str, description: &str, class: PermissionClass, schema: Value) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: schema,
        class,
        source: ToolSource::Builtin,
        uses_workspace: false,
    }
}

const WORKFLOW_SHAPE: &str = "A workflow is a JSON object {\"name\": string, \"nodes\": [...], \"connections\": {...}, \"settings\": {...}}. \
Every node needs \"name\" (unique), \"type\" (full type, e.g. \"n8n-nodes-base.httpRequest\"), \"typeVersion\" (number), \"position\" ([x, y]) and \"parameters\" (object). \
\"connections\" maps a source node name to {\"main\": [[{\"node\": target name, \"type\": \"main\", \"index\": 0}]]}. \
Use n8n_get_workflow on an existing workflow to see exact parameter names before writing similar nodes.";

fn defs() -> Vec<ToolDef> {
    use PermissionClass::{Restricted, Safe};
    vec![
        def(
            LIST_WORKFLOWS,
            "List the workflows of the user's n8n instance: id, name, whether it is active, its trigger nodes and its webhook (method and path) when it has one. Read only.",
            Restricted,
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
        ),
        def(
            GET_WORKFLOW,
            "Read one n8n workflow in full (nodes, connections, settings, active state). Read only.",
            Restricted,
            json!({"type": "object", "properties": {
                "workflow_id": {"type": "string", "description": "The workflow id from n8n_list_workflows."}
            }, "required": ["workflow_id"], "additionalProperties": false}),
        ),
        def(
            LIST_EXECUTIONS,
            "List recent n8n executions (id, workflow, status, mode, times), newest first. Read only.",
            Restricted,
            json!({"type": "object", "properties": {
                "workflow_id": {"type": "string", "description": "Only this workflow's executions."},
                "limit": {"type": "integer", "description": "How many (1-50, default 10)."}
            }, "additionalProperties": false}),
        ),
        def(
            GET_EXECUTION,
            "Read the result of one n8n execution: status, the failing node and error, what each node did and a preview of the last node's output. Read only.",
            Restricted,
            json!({"type": "object", "properties": {
                "execution_id": {"type": "string", "description": "The execution id from n8n_list_executions."}
            }, "required": ["execution_id"], "additionalProperties": false}),
        ),
        // The propose tools change nothing by themselves: the user reviews a diff and approves
        // in Harness first, whatever the tool policy says. That is why they are `Safe`.
        def(
            PROPOSE_WORKFLOW,
            &format!(
                "Propose creating a new n8n workflow, or changing an existing one. Nothing is written to n8n until the user reviews the diff in Harness and approves; this call waits for that decision and returns the outcome. \
{WORKFLOW_SHAPE} \
Omit workflow_id to create a workflow (it is created inactive). Give workflow_id to edit: send the complete new workflow. \
Set activate true to make the workflow live or false to switch it off (needs the connection's write permission; without it a workflow stays a draft). \
A workflow can only be started from outside through a Webhook trigger node (\"n8n-nodes-base.webhook\" with parameters.path and httpMethod). If the user declines, do not retry the same change; ask what to change."
            ),
            Safe,
            json!({"type": "object", "properties": {
                "workflow_id": {"type": "string", "description": "Edit this workflow; omit to create a new one."},
                "workflow": {"type": "object", "description": "The complete workflow: name, nodes, connections, settings. May be omitted when only activate is given."},
                "activate": {"type": "boolean", "description": "true = make live, false = switch off."},
                "reason": {"type": "string", "description": "One sentence for the user: what this change does."}
            }, "additionalProperties": false}),
        ),
        def(
            PROPOSE_RUN,
            "Propose running an active n8n workflow through its webhook trigger. The user reviews and approves in Harness; this call waits for that, then returns the webhook answer and the execution result (status, node results, output preview). Only workflows with an active plain Webhook trigger can be run.",
            Safe,
            json!({"type": "object", "properties": {
                "workflow_id": {"type": "string", "description": "The workflow id from n8n_list_workflows."},
                "payload": {"description": "JSON body sent to the webhook (ignored for GET webhooks)."},
                "reason": {"type": "string", "description": "One sentence for the user: why run it."}
            }, "required": ["workflow_id"], "additionalProperties": false}),
        ),
        def(
            PROPOSAL_STATUS,
            "Check a proposal made earlier (pending, applied, rejected, failed) and read its result. Use it when n8n_propose_workflow or n8n_propose_run said the user has not decided yet.",
            Safe,
            json!({"type": "object", "properties": {
                "proposal_id": {"type": "string"}
            }, "required": ["proposal_id"], "additionalProperties": false}),
        ),
    ]
}

fn tool(name: &str) -> Option<Arc<dyn Tool>> {
    let def = defs().into_iter().find(|d| d.name == name)?;
    Some(Arc::new(N8nTool { def }))
}

struct N8nTool {
    def: ToolDef,
}

impl Tool for N8nTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }

    fn call<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            let client = tokio::task::spawn_blocking(N8nClient::saved)
                .await
                .map_err(|e| ToolError::failed(format!("n8n: {e}")))?
                .map_err(failure)?;
            let permission = N8nPermission::saved();
            // A tool left registered after the permission was lowered.
            if !tool_names(permission).contains(&self.def.name.as_str()) {
                return Err(ToolError::failed(format!(
                    "this connection's n8n permission is \"{}\", which does not allow {}",
                    permission.as_str(),
                    self.def.name
                )));
            }
            ctx.set_network_target(host_of(&client));
            let out = run(&self.def.name, &client, permission, &args, ctx)
                .await
                .map_err(failure)?;
            Ok(ToolOutput::text(out))
        })
    }
}

fn host_of(client: &N8nClient) -> String {
    reqwest::Url::parse(&client.base)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| u.port().map_or(h.to_string(), |p| format!("{h}:{p}")))
        })
        .unwrap_or_default()
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

async fn run(
    name: &str,
    client: &N8nClient,
    permission: N8nPermission,
    args: &Value,
    ctx: &ToolCtx,
) -> Result<String, String> {
    let need = |key: &str| {
        str_arg(args, key).ok_or_else(|| {
            String::from(
                crate::error_key::err("automations.n8n_workflow_invalid")
                    .with("reason", format!("{key} is required")),
            )
        })
    };
    match name {
        LIST_WORKFLOWS => {
            let list = client.workflows().await?;
            Ok(json!(list
                .iter()
                .map(|w| json!({
                    "id": w.id,
                    "name": w.name,
                    "active": w.active,
                    "triggers": w.triggers,
                    "webhook": w.webhook.as_ref().map(|h| format!("{} {}", h.method, h.path)),
                    "updated_at": w.updated_at,
                }))
                .collect::<Vec<_>>())
            .to_string())
        }
        GET_WORKFLOW => {
            let v = client.workflow_json(need("workflow_id")?).await?;
            let wf = client.parse_workflow(&v);
            let mut out = super::diff::normalized(&v);
            out["id"] = v.get("id").cloned().unwrap_or(Value::Null);
            out["active"] = json!(wf.is_some_and(|w| w.active));
            Ok(out.to_string())
        }
        LIST_EXECUTIONS => {
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .clamp(1, 50);
            let list = client
                .executions(str_arg(args, "workflow_id"), limit as u32)
                .await?;
            Ok(json!(list
                .iter()
                .map(|e| json!({
                    "id": e.id,
                    "workflow_id": e.workflow_id,
                    "status": e.status,
                    "mode": e.mode,
                    "started_at": e.started_at,
                    "stopped_at": e.stopped_at,
                }))
                .collect::<Vec<_>>())
            .to_string())
        }
        GET_EXECUTION => Ok(serde_json::to_string(
            &client.execution_report(need("execution_id")?).await?,
        )
        .unwrap_or_default()),
        PROPOSE_WORKFLOW => {
            let store = ProposalStore::global();
            let p = store
                .stage_workflow(
                    client,
                    permission,
                    StageWorkflow {
                        call_id: ctx.call_id.clone(),
                        workflow_id: str_arg(args, "workflow_id").map(str::to_string),
                        workflow: args.get("workflow").filter(|w| !w.is_null()).cloned(),
                        activate: args.get("activate").and_then(Value::as_bool),
                        note: str_arg(args, "reason").map(str::to_string),
                    },
                )
                .await?;
            Ok(await_decision(store, &p, ctx).await)
        }
        PROPOSE_RUN => {
            let store = ProposalStore::global();
            let p = store
                .stage_run(
                    client,
                    permission,
                    ctx.call_id.clone(),
                    need("workflow_id")?,
                    args.get("payload").filter(|p| !p.is_null()).cloned(),
                    str_arg(args, "reason").map(str::to_string),
                )
                .await?;
            Ok(await_decision(store, &p, ctx).await)
        }
        PROPOSAL_STATUS => {
            let p = ProposalStore::global()
                .get(need("proposal_id")?)
                .ok_or_else(|| {
                    String::from(crate::error_key::err("automations.n8n_proposal_not_found"))
                })?;
            Ok(outcome(&p))
        }
        other => Err(format!("unknown n8n tool {other}")),
    }
}

/// Waits for the user's decision, leaving a few seconds of the call's time to answer.
async fn await_decision(store: &ProposalStore, p: &Proposal, ctx: &ToolCtx) -> String {
    let wait = ctx.timeout.saturating_sub(Duration::from_secs(3));
    let latest = store.wait(&p.id, wait).await.unwrap_or_else(|| p.clone());
    outcome(&latest)
}

/// What the model reads about a proposal.
pub fn outcome(p: &Proposal) -> String {
    let mut v = json!({
        "proposal_id": p.id,
        "status": p.status.as_str(),
        "kind": p.kind,
        "workflow_id": p.workflow_id,
        "workflow_name": p.workflow_name,
    });
    match p.status {
        ProposalStatus::Pending | ProposalStatus::Applying => {
            v["message"] = json!(
                "The user has not decided yet. Tell them to review it in Harness; check later with n8n_proposal_status. Do not assume it was applied."
            );
        }
        ProposalStatus::Rejected => {
            v["message"] = json!("The user declined. Nothing was changed in n8n.");
        }
        ProposalStatus::Expired => {
            v["message"] = json!("The proposal expired without a decision. Nothing was changed.");
        }
        ProposalStatus::Applied | ProposalStatus::Failed => {
            if let Some(e) = &p.error {
                v["error"] = json!(say(e));
            }
            if let Some(r) = &p.result {
                v["steps"] = json!(r.steps);
                if let Some(w) = &r.workflow {
                    v["workflow_id"] = json!(w.id);
                    v["active"] = json!(w.active);
                    v["editor_url"] = json!(w.editor_url);
                    if let Some(h) = &w.webhook {
                        v["webhook"] = json!(format!("{} {}", h.method, h.url));
                    }
                }
                if let Some(t) = &r.trigger {
                    v["webhook_status"] = json!(t.status);
                    v["webhook_response"] = json!(t.body);
                }
                if let Some(e) = &r.execution {
                    v["execution"] = serde_json::to_value(e).unwrap_or(Value::Null);
                } else if r.trigger.as_ref().is_some_and(|t| t.ok) {
                    v["execution_note"] =
                        json!("n8n did not report an execution for this run (it may not save executions).");
                }
            }
        }
    }
    v.to_string()
}

fn failure(error: String) -> ToolError {
    ToolError::new(ToolErrorCode::Failed, say(&error))
}

/// A keyed error as a sentence the model can act on.
pub fn say(error: &str) -> String {
    let Some((key, params)) = crate::error_key::parse(error) else {
        return error.to_string();
    };
    let p = |name: &str| {
        params
            .get(name)
            .map(|v| v.as_str().map_or(v.to_string(), str::to_string))
            .unwrap_or_default()
    };
    match key {
        "automations.n8n_permission_denied" => format!(
            "this n8n connection does not allow that: it needs the \"{}\" permission, which the user can grant in Settings > Automations",
            p("need")
        ),
        "automations.n8n_workflow_invalid" => format!("the workflow is not valid: {}", p("reason")),
        "automations.n8n_rejected" => format!("n8n refused it (HTTP {}): {}", p("status"), p("message")),
        "automations.n8n_workflow_not_found" => "no workflow with that id".into(),
        "automations.n8n_proposal_not_found" => "no proposal with that id".into(),
        "automations.n8n_proposal_closed" => format!("the proposal is already {}", p("status")),
        "automations.n8n_proposal_stale" => {
            "the workflow changed in n8n after the proposal was made; propose again from the current version".into()
        }
        "automations.n8n_connection_changed" => "the n8n connection changed since this proposal".into(),
        "automations.n8n_no_webhook" => "this workflow has no plain Webhook trigger, so it cannot be run from here".into(),
        "automations.n8n_workflow_inactive" => "the workflow is not active, so its webhook is not live".into(),
        "automations.n8n_not_configured" | "automations.n8n_key_missing" => {
            "no n8n connection (address and API key) is saved".into()
        }
        "automations.n8n_unauthorized" => "n8n rejected the API key".into(),
        "automations.n8n_forbidden" => "n8n refused the request: the API key's scopes do not cover it".into(),
        _ => {
            let detail: Vec<String> = params
                .iter()
                .map(|(k, v)| format!("{k}={}", v.as_str().map_or(v.to_string(), str::to_string)))
                .collect();
            format!("{key} ({})", detail.join(", "))
        }
    }
}
