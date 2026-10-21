//! The result of an n8n execution, read from the public API for review: status, the failing
//! node, what every node did, and a short preview of the last node's output.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{clip, segment, N8nClient};
use crate::error_key::err;

/// Longest output preview handed to the UI and the model.
const OUTPUT_PREVIEW: usize = 1500;
/// How often a started run is looked at while it has not finished.
const POLL_EVERY: Duration = Duration::from_millis(500);

/// What one node did in an execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nNodeRun))]
pub struct NodeRun {
    pub name: String,
    /// `success` or `error` (n8n's own word for the node's run).
    pub status: String,
    /// Items the node produced on its first output.
    pub items: Option<u64>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nExecutionReport))]
pub struct ExecutionReport {
    pub id: String,
    pub workflow_id: String,
    /// `success`, `error`, `crashed`, `running`, `waiting`, `canceled`, `new` or `unknown`.
    pub status: String,
    pub mode: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub error: Option<super::ExecutionError>,
    /// Nodes in the order they started.
    pub nodes: Vec<NodeRun>,
    /// Start of the last node's first output item (JSON text).
    pub output: Option<String>,
    pub editor_url: String,
}

fn is_running(status: &str) -> bool {
    matches!(status, "running" | "new" | "waiting")
}

impl N8nClient {
    /// The id of the newest execution of a workflow (`None` = it never ran). Taken before a
    /// webhook call so the run it starts can be told apart from older ones.
    pub async fn newest_execution_id(&self, workflow_id: &str) -> Result<Option<u64>, String> {
        Ok(self
            .executions(Some(workflow_id), 1)
            .await?
            .first()
            .and_then(|e| e.id.parse().ok()))
    }

    /// The execution with its run data, summarized.
    pub async fn execution_report(&self, id: &str) -> Result<ExecutionReport, String> {
        let v = self
            .get(
                &format!("executions/{}", segment(id)?),
                &[("includeData", "true".into())],
            )
            .await?;
        self.parse_report(&v)
            .ok_or_else(|| err("automations.n8n_invalid_response").into())
    }

    /// Waits for the run a webhook call started: the first execution of `workflow_id` newer
    /// than `after`, until it has finished or `wait` is over (then it is reported as it is,
    /// `running`). `None` = no new execution appeared (the webhook answered but n8n does not
    /// store executions of this workflow).
    pub async fn await_execution(
        &self,
        workflow_id: &str,
        after: Option<u64>,
        wait: Duration,
    ) -> Result<Option<ExecutionReport>, String> {
        let deadline = tokio::time::Instant::now() + wait;
        let mut found: Option<String> = None;
        loop {
            if found.is_none() {
                found = self
                    .executions(Some(workflow_id), 5)
                    .await?
                    .into_iter()
                    .filter_map(|e| e.id.parse::<u64>().ok().map(|n| (n, e.id)))
                    .filter(|(n, _)| after.is_none_or(|a| *n > a))
                    .max_by_key(|(n, _)| *n)
                    .map(|(_, id)| id);
            }
            if let Some(id) = &found {
                let report = self.execution_report(id).await?;
                if !is_running(&report.status) || tokio::time::Instant::now() >= deadline {
                    return Ok(Some(report));
                }
            } else if tokio::time::Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(POLL_EVERY).await;
        }
    }

    fn parse_report(&self, v: &Value) -> Option<ExecutionReport> {
        let base = self.parse_execution(v)?;
        // The run data is an object in the public API; a string (older or compressed form) is
        // read when it is plain JSON and skipped otherwise.
        let parsed;
        let data = match v.get("data") {
            Some(Value::String(s)) => {
                parsed = serde_json::from_str::<Value>(s).ok();
                parsed.as_ref()
            }
            other => other,
        };
        let result = data.and_then(|d| d.get("resultData"));
        let error = result.and_then(|r| r.get("error"));
        let message = error
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map(|m| clip(m, 600));
        let error_node = error
            .and_then(|e| e.pointer("/node/name"))
            .or_else(|| result.and_then(|r| r.get("lastNodeExecuted")))
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut nodes: Vec<(i64, NodeRun)> = Vec::new();
        if let Some(run_data) = result
            .and_then(|r| r.get("runData"))
            .and_then(Value::as_object)
        {
            for (name, runs) in run_data {
                let Some(run) = runs.as_array().and_then(|r| r.first()) else {
                    continue;
                };
                let items = run
                    .pointer("/data/main/0")
                    .and_then(Value::as_array)
                    .map(|i| i.len() as u64);
                let node_error = run
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(|m| clip(m, 300));
                let status = run
                    .get("executionStatus")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        if node_error.is_some() {
                            "error".into()
                        } else {
                            "success".into()
                        }
                    });
                nodes.push((
                    run.get("startTime").and_then(Value::as_i64).unwrap_or(0),
                    NodeRun {
                        name: name.clone(),
                        status,
                        items,
                        error: node_error,
                        duration_ms: run.get("executionTime").and_then(Value::as_u64),
                    },
                ));
            }
        }
        nodes.sort_by_key(|(start, _)| *start);
        let last = result
            .and_then(|r| r.get("lastNodeExecuted"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| nodes.last().map(|(_, n)| n.name.clone()));
        let output = last.and_then(|name| {
            let item = result?.pointer(&format!(
                "/runData/{}/0/data/main/0/0/json",
                pointer_escape(&name)
            ))?;
            Some(clip(&item.to_string(), OUTPUT_PREVIEW))
        });
        Some(ExecutionReport {
            workflow_id: base.workflow_id,
            id: base.id,
            status: base.status,
            mode: base.mode,
            started_at: base.started_at,
            stopped_at: base.stopped_at,
            error: error.is_some().then_some(super::ExecutionError {
                message,
                node: error_node,
            }),
            nodes: nodes.into_iter().map(|(_, n)| n).collect(),
            output,
            editor_url: base.editor_url,
        })
    }
}

/// A key as a JSON pointer segment (`~` and `/` are escaped).
fn pointer_escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
