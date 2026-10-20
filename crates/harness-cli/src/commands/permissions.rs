//! `harness permissions`: trust levels and capability grants of MCP servers, agent runtimes and
//! the training backend, through the Control API (`/permissions`, needs `admin`). Every change
//! is recorded in the tool log (`harness tools` → `/tools/log`, tool `permissions.*`).

use serde_json::{json, Value};

use crate::cli::PermissionsCommand;
use crate::client::{seg, ApiClient};
use crate::error::CliResult;
use crate::output::{table, Out};

fn grants_text(s: &Value) -> String {
    let grants = s["grants"].as_array().cloned().unwrap_or_default();
    if grants.is_empty() {
        return "-".into();
    }
    grants
        .iter()
        .map(|g| {
            let paths = g["paths"]
                .as_array()
                .map(|p| {
                    p.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(";")
                })
                .unwrap_or_default();
            let cap = g["capability"].as_str().unwrap_or("?");
            let id = g["id"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(8)
                .collect::<String>();
            if paths.is_empty() {
                format!("{cap}[{id}]")
            } else {
                format!("{cap}({paths})[{id}]")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub async fn run(client: &ApiClient, command: PermissionsCommand, out: &Out) -> CliResult<()> {
    match command {
        PermissionsCommand::List => {
            let v = client.get("/permissions").await?;
            out.data(&v, || {
                let rows: Vec<Vec<String>> = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|s| {
                        vec![
                            s["kind"].as_str().unwrap_or("").to_string(),
                            s["id"].as_str().unwrap_or("").to_string(),
                            if s["available"].as_bool() == Some(true) {
                                "yes".into()
                            } else {
                                "not available yet".into()
                            },
                            s["trust"].as_str().unwrap_or("default").to_string(),
                            grants_text(s),
                        ]
                    })
                    .collect();
                table(&["KIND", "ID", "AVAILABLE", "TRUST", "GRANTS"], &rows)
            });
        }
        PermissionsCommand::Grant {
            kind,
            id,
            capability,
            paths,
        } => {
            let v = client
                .post(
                    "/permissions/grants",
                    &json!({"kind": kind, "id": id, "capability": capability, "paths": paths}),
                )
                .await?;
            out.data(&v, || {
                format!(
                    "Granted {capability} to {kind} {id} (grant {}).",
                    v["id"].as_str().unwrap_or("?")
                )
            });
        }
        PermissionsCommand::Revoke { kind, id, grant } => {
            let path = match &grant {
                Some(g) => format!("/permissions/{}/{}/grants/{}", seg(&kind), seg(&id), seg(g)),
                None => format!("/permissions/{}/{}", seg(&kind), seg(&id)),
            };
            client.delete(&path).await?;
            out.data(&json!({"revoked": true}), || match grant {
                Some(g) => format!("Revoked grant {g} of {kind} {id}."),
                None => format!("Revoked every grant and the trust level of {kind} {id}."),
            });
        }
        PermissionsCommand::Trust { kind, id, level } => {
            let trust = if level == "default" {
                Value::Null
            } else {
                json!(level)
            };
            client
                .put(
                    &format!("/permissions/{}/{}/trust", seg(&kind), seg(&id)),
                    &json!({"trust": trust}),
                )
                .await?;
            out.data(&json!({"trust": trust}), || {
                format!("Trust level of {kind} {id}: {level}.")
            });
        }
    }
    Ok(())
}
