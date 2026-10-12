//! Harness's own tool registry (`harness_core::tools`: file, shell, network, calculator...)
//! offered to MCP clients next to the management tools. Opt-in: `settings.tools.mcp_expose`
//! (default off) and the `tools` scope.
//!
//! * **Which tools.** The built-in ones (any source but `mcp`). The MCP tools Harness itself
//!   consumes are not re-exported: a client that wants them connects to their server; and
//!   Harness consuming its own `harness mcp` would otherwise loop. A name that a management
//!   tool already has stays with the management tool.
//! * **Same policy, no approval channel.** Every call goes through `POST /tools/{name}/run`, so
//!   the saved policy decides. MCP cannot ask the user: a call the policy would ask about
//!   returns a tool error that says how to trust the tool (Settings > Tools); "off" returns
//!   one that says tools are off. `approved` and a working directory are never sent, so the
//!   calls run in the saved default folder.
//! * **Annotations** come from the permission class: safe and restricted tools are read-only
//!   (restricted ones without a working directory reach the network: open world), dangerous
//!   ones are destructive. Tools that are listed but need approval say so in their text.
//! * **Log.** The API logs the call to `tool_calls` with the client label (`mcp:<client name>`).

use serde_json::{json, Value};

use crate::resolve::{array, get, seg};
use crate::tools::{error_result, find};
use crate::{McpServer, Method};

/// A built-in tool as `GET /tools` reports it, ready to list.
struct Listed {
    name: String,
    json: Value,
}

async fn fetch(server: &McpServer) -> Option<Vec<Listed>> {
    if !server.allows(harness_core::control_api::Scope::Tools) {
        return None;
    }
    let listing = get(server.backend.as_ref(), "/tools?builtin_only=true")
        .await
        .ok()?;
    if listing["mcp_expose"] != json!(true) || listing["policy"] == json!("off") {
        return None;
    }
    let mut out = Vec::new();
    for t in array(&listing["tools"]) {
        let Some(name) = t["name"].as_str() else {
            continue;
        };
        if t["source"] == json!("mcp") || t["decision"] == json!("refuse") || find(name).is_some() {
            continue;
        }
        out.push(Listed {
            name: name.to_string(),
            json: to_mcp(&t),
        });
    }
    Some(out)
}

fn to_mcp(t: &Value) -> Value {
    let class = t["class"].as_str().unwrap_or("dangerous");
    let uses_workspace = t["uses_workspace"] == json!(true);
    let dangerous = class == "dangerous";
    let mut description = t["description"].as_str().unwrap_or("").to_string();
    if t["decision"] == json!("ask") {
        description.push_str(
            " (Needs the user's approval, which MCP cannot ask for: it fails until the user \
trusts this tool in Harness Settings > Tools.)",
        );
    }
    if uses_workspace {
        description.push_str(" Runs in the default working folder set in Settings > Tools.");
    }
    let name = t["name"].as_str().unwrap_or_default();
    let mut annotations = json!({
        "title": name,
        "readOnlyHint": !dangerous,
        "openWorldHint": class == "restricted" && !uses_workspace,
    });
    if dangerous {
        annotations["destructiveHint"] = json!(true);
    }
    json!({
        "name": name,
        "title": name,
        "description": description,
        "inputSchema": t["input_schema"],
        "annotations": annotations,
    })
}

/// The tools to append to `tools/list`.
pub async fn list(server: &McpServer) -> Vec<Value> {
    fetch(server)
        .await
        .map(|l| l.into_iter().map(|t| t.json).collect())
        .unwrap_or_default()
}

/// Runs `name` when it is one of the offered Harness tools (`None`: not one, the caller reports
/// an unknown tool). `meta_client`: the client name from the request's `_meta`, when it has one.
pub async fn call(
    server: &McpServer,
    name: &str,
    args: &Value,
    meta_client: Option<&str>,
) -> Option<Value> {
    let listed = fetch(server).await?;
    if !listed.iter().any(|t| t.name == name) {
        return None;
    }
    let body = json!({"args": args, "client": server.client_label(meta_client)});
    let path = format!("/tools/{}/run", seg(name));
    let reply = server
        .backend
        .request(Method::Post, &path, Some(body))
        .await;
    Some(match reply {
        Ok(v) => {
            let text = v["content"].as_str().unwrap_or_default();
            let mut result = json!({
                "content": [{"type": "text", "text": text}],
                "isError": v["error"].is_string(),
            });
            if v["truncated"] == json!(true) {
                result["_meta"] = json!({"harness/truncated": true});
            }
            result
        }
        Err(e) if e.code == "tools.approval_required" => error_result(
            "tools.approval_required",
            &format!(
                "\"{name}\" needs the user's approval and MCP has no way to ask. The user can \
let it run by trusting it in Harness Settings > Tools (policy \"Allow trusted tools\" and \
switch this tool on, or \"Allow all\")."
            ),
            None,
        ),
        Err(e) if e.code == "tools.disabled" => error_result(
            "tools.disabled",
            "Tools are turned off in Harness Settings > Tools (policy \"Off\").",
            None,
        ),
        Err(e) if e.code == "api.insufficient_scope" => error_result(
            "api.insufficient_scope",
            "This connection needs the \"tools\" scope to run Harness tools.",
            Some(json!({"scope": "tools"})),
        ),
        Err(e) => error_result(
            &e.code,
            &e.message,
            Some(Value::Object((*e.params).clone())),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_follow_the_permission_class() {
        let t = |class: &str, ws: bool, decision: &str| {
            to_mcp(
                &json!({"name": "x", "description": "d", "input_schema": {"type": "object"},
                "class": class, "uses_workspace": ws, "decision": decision}),
            )
        };
        let safe = t("safe", false, "run");
        assert_eq!(safe["annotations"]["readOnlyHint"], true);
        assert_eq!(safe["annotations"]["openWorldHint"], false);
        assert!(safe["annotations"].get("destructiveHint").is_none());
        let net = t("restricted", false, "run");
        assert_eq!(net["annotations"]["openWorldHint"], true);
        let read = t("restricted", true, "ask");
        assert_eq!(read["annotations"]["readOnlyHint"], true);
        assert_eq!(read["annotations"]["openWorldHint"], false);
        assert!(read["description"]
            .as_str()
            .unwrap()
            .contains("Settings > Tools"));
        let write = t("dangerous", true, "ask");
        assert_eq!(write["annotations"]["readOnlyHint"], false);
        assert_eq!(write["annotations"]["destructiveHint"], true);
    }
}
