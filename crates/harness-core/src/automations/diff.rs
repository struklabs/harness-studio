//! What the user reviews before a proposed workflow reaches n8n: a line diff of the
//! normalized workflow JSON, the nodes that were added, removed or changed, and warnings about
//! what the change does (code execution, a new public webhook, going live).
//!
//! Both sides are normalized the same way (object keys sorted, nodes ordered by name), so only
//! real differences show.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Lines kept around every change.
const CONTEXT: usize = 3;
/// Longest diff handed to the UI.
const MAX_LINES: usize = 600;
/// Above this many lines per side the exact diff is skipped (the middle is shown as replaced).
const MAX_DIFF_SIDE: usize = 2500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nLineOp))]
pub enum LineOp {
    Keep,
    Add,
    Remove,
    /// `…`: lines left out between two changes.
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nDiffLine))]
pub struct DiffLine {
    pub op: LineOp,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nWorkflowDiff))]
pub struct WorkflowDiff {
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
    /// The diff was cut at the line limit.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nNodeChangeKind))]
pub enum NodeChangeKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nNodeChange))]
pub struct NodeChange {
    pub name: String,
    pub kind: NodeChangeKind,
    /// Full node type (`n8n-nodes-base.httpRequest`).
    pub node_type: String,
}

/// Something the reviewer should look at twice. `code` is a stable identifier the UI
/// translates; `node` names the node it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = N8nWarning))]
pub struct Warning {
    pub code: String,
    pub node: Option<String>,
}

fn warn(code: &str, node: Option<&str>) -> Warning {
    Warning {
        code: code.to_string(),
        node: node.map(str::to_string),
    }
}

/// Keys sorted at every level.
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), canonical(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// The comparable form of a workflow body: name, settings, connections and the nodes ordered
/// by name.
pub fn normalized(workflow: &Value) -> Value {
    let mut nodes: Vec<Value> = workflow
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    nodes.sort_by_key(|n| {
        n.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    canonical(&serde_json::json!({
        "name": workflow.get("name").cloned().unwrap_or(Value::Null),
        "settings": workflow.get("settings").cloned().unwrap_or_else(|| serde_json::json!({})),
        "nodes": nodes,
        "connections": workflow.get("connections").cloned().unwrap_or_else(|| serde_json::json!({})),
    }))
}

fn pretty_lines(v: &Value) -> Vec<String> {
    serde_json::to_string_pretty(v)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Line diff of two workflow bodies (`before` = `None`: everything is new).
pub fn diff_workflows(before: Option<&Value>, after: &Value) -> WorkflowDiff {
    let a = before
        .map(|b| pretty_lines(&normalized(b)))
        .unwrap_or_default();
    let b = pretty_lines(&normalized(after));
    diff_lines(&a, &b)
}

#[derive(Clone, Copy)]
enum Raw {
    Keep,
    Add,
    Remove,
}

fn diff_lines(a: &[String], b: &[String]) -> WorkflowDiff {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut ops: Vec<(Raw, &str)> = a[..prefix]
        .iter()
        .map(|l| (Raw::Keep, l.as_str()))
        .collect();
    ops.extend(middle(ma, mb));
    ops.extend(
        a[a.len() - suffix..]
            .iter()
            .map(|l| (Raw::Keep, l.as_str())),
    );

    let added = ops.iter().filter(|(o, _)| matches!(o, Raw::Add)).count();
    let removed = ops.iter().filter(|(o, _)| matches!(o, Raw::Remove)).count();

    // Keep only the lines near a change.
    let mut keep = vec![false; ops.len()];
    for (i, (op, _)) in ops.iter().enumerate() {
        if !matches!(op, Raw::Keep) {
            for k in keep
                .iter_mut()
                .take((i + CONTEXT + 1).min(ops.len()))
                .skip(i.saturating_sub(CONTEXT))
            {
                *k = true;
            }
        }
    }
    let mut lines = Vec::new();
    let mut truncated = false;
    let mut gap = false;
    for (i, (op, text)) in ops.iter().enumerate() {
        if !keep[i] {
            gap = !lines.is_empty();
            continue;
        }
        if lines.len() >= MAX_LINES {
            truncated = true;
            break;
        }
        if gap {
            lines.push(DiffLine {
                op: LineOp::Gap,
                text: String::new(),
            });
            gap = false;
        }
        lines.push(DiffLine {
            op: match op {
                Raw::Keep => LineOp::Keep,
                Raw::Add => LineOp::Add,
                Raw::Remove => LineOp::Remove,
            },
            text: (*text).to_string(),
        });
    }
    WorkflowDiff {
        lines,
        added,
        removed,
        truncated,
    }
}

/// Longest-common-subsequence diff of the lines between the shared prefix and suffix.
fn middle<'a>(a: &'a [String], b: &'a [String]) -> Vec<(Raw, &'a str)> {
    if a.is_empty() || b.is_empty() || a.len() > MAX_DIFF_SIDE || b.len() > MAX_DIFF_SIDE {
        return a
            .iter()
            .map(|l| (Raw::Remove, l.as_str()))
            .chain(b.iter().map(|l| (Raw::Add, l.as_str())))
            .collect();
    }
    let (n, m) = (a.len(), b.len());
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if a[i] == b[j] {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(n + m);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((Raw::Keep, a[i].as_str()));
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            out.push((Raw::Remove, a[i].as_str()));
            i += 1;
        } else {
            out.push((Raw::Add, b[j].as_str()));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| (Raw::Remove, l.as_str())));
    out.extend(b[j..].iter().map(|l| (Raw::Add, l.as_str())));
    out
}

fn nodes_by_name(w: &Value) -> BTreeMap<String, Value> {
    w.get("nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n.get("name")?.as_str()?.to_string(), canonical(n))))
        .collect()
}

fn type_of(node: &Value) -> String {
    node.get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Nodes added, removed or changed between two workflow bodies.
pub fn node_changes(before: Option<&Value>, after: &Value) -> Vec<NodeChange> {
    let old = before.map(nodes_by_name).unwrap_or_default();
    let new = nodes_by_name(after);
    let mut out = Vec::new();
    for (name, node) in &new {
        match old.get(name) {
            None => out.push(NodeChange {
                name: name.clone(),
                kind: NodeChangeKind::Added,
                node_type: type_of(node),
            }),
            Some(prev) if prev != node => out.push(NodeChange {
                name: name.clone(),
                kind: NodeChangeKind::Changed,
                node_type: type_of(node),
            }),
            Some(_) => {}
        }
    }
    for (name, node) in &old {
        if !new.contains_key(name) {
            out.push(NodeChange {
                name: name.clone(),
                kind: NodeChangeKind::Removed,
                node_type: type_of(node),
            });
        }
    }
    out
}

/// Node types that run code or commands on the n8n host, or reach out to other systems.
fn node_warning(node_type: &str) -> Option<&'static str> {
    let short = node_type.rsplit('.').next().unwrap_or(node_type);
    match short {
        "executeCommand" | "ssh" | "code" | "function" | "functionItem" | "readWriteFile"
        | "readBinaryFile" | "writeBinaryFile" => Some("runs_code"),
        "httpRequest" => Some("http_request"),
        _ => None,
    }
}

/// Warnings for the nodes that are new or changed, plus what activation does.
pub fn warnings(
    before: Option<&Value>,
    after: &Value,
    goes_live: bool,
    edits_live: bool,
) -> Vec<Warning> {
    let mut out = Vec::new();
    for change in node_changes(before, after) {
        if change.kind == NodeChangeKind::Removed {
            continue;
        }
        if let Some(code) = node_warning(&change.node_type) {
            out.push(warn(code, Some(&change.name)));
        }
        if change.node_type.ends_with(".webhook") && change.kind == NodeChangeKind::Added {
            out.push(warn("new_webhook", Some(&change.name)));
        }
    }
    if goes_live {
        out.push(warn("goes_live", None));
    }
    if edits_live {
        out.push(warn("edits_live", None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wf(url: &str) -> Value {
        json!({
            "name": "Demo",
            "settings": {"executionOrder": "v1"},
            "nodes": [
                {"name": "Webhook", "type": "n8n-nodes-base.webhook", "parameters": {"path": "demo"}},
                {"name": "HTTP", "type": "n8n-nodes-base.httpRequest", "parameters": {"url": url, "method": "GET"}}
            ],
            "connections": {"Webhook": {"main": [[{"node": "HTTP", "type": "main", "index": 0}]]}}
        })
    }

    #[test]
    fn identical_workflows_have_an_empty_diff_whatever_the_key_order() {
        let a = wf("https://a.example");
        let mut b = wf("https://a.example");
        // Same content, nodes in the other order and keys shuffled.
        b["nodes"].as_array_mut().unwrap().reverse();
        let d = diff_workflows(Some(&a), &b);
        assert_eq!((d.added, d.removed), (0, 0));
        assert!(d.lines.is_empty());
        assert!(node_changes(Some(&a), &b).is_empty());
    }

    #[test]
    fn a_changed_parameter_shows_as_one_removed_and_one_added_line() {
        let d = diff_workflows(Some(&wf("https://a.example")), &wf("https://b.example"));
        assert_eq!((d.added, d.removed), (1, 1));
        assert!(d
            .lines
            .iter()
            .any(|l| l.op == LineOp::Remove && l.text.contains("https://a.example")));
        assert!(d
            .lines
            .iter()
            .any(|l| l.op == LineOp::Add && l.text.contains("https://b.example")));
        // Context lines surround the change.
        assert!(d.lines.iter().any(|l| l.op == LineOp::Keep));
        let changes = node_changes(Some(&wf("https://a.example")), &wf("https://b.example"));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "HTTP");
        assert_eq!(changes[0].kind, NodeChangeKind::Changed);
    }

    #[test]
    fn a_new_workflow_is_all_additions_and_added_nodes() {
        let w = wf("https://a.example");
        let d = diff_workflows(None, &w);
        assert_eq!(d.removed, 0);
        assert!(d.added > 10);
        let changes = node_changes(None, &w);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| c.kind == NodeChangeKind::Added));
    }

    #[test]
    fn removed_nodes_are_listed() {
        let mut after = wf("https://a.example");
        after["nodes"].as_array_mut().unwrap().pop();
        let changes = node_changes(Some(&wf("https://a.example")), &after);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, NodeChangeKind::Removed);
        assert_eq!(changes[0].name, "HTTP");
    }

    #[test]
    fn distant_changes_are_separated_by_a_gap() {
        let a: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        let mut b = a.clone();
        b[2] = "changed 2".into();
        b[35] = "changed 35".into();
        let d = diff_lines(&a, &b);
        assert_eq!((d.added, d.removed), (2, 2));
        assert!(d.lines.iter().any(|l| l.op == LineOp::Gap));
        assert!(d.lines.len() < 25);
    }

    #[test]
    fn warnings_flag_code_http_webhooks_and_going_live() {
        let w = wf("https://a.example");
        let list = warnings(None, &w, true, false);
        let codes: Vec<(&str, Option<&str>)> = list
            .iter()
            .map(|w| (w.code.as_str(), w.node.as_deref()))
            .collect();
        assert!(codes.contains(&("http_request", Some("HTTP"))));
        assert!(codes.contains(&("new_webhook", Some("Webhook"))));
        assert!(codes.contains(&("goes_live", None)));
        let mut code = wf("x");
        code["nodes"][1]["type"] = json!("n8n-nodes-base.executeCommand");
        assert!(warnings(None, &code, false, true)
            .iter()
            .any(|w| w.code == "runs_code" && w.node.as_deref() == Some("HTTP")));
        // An unchanged risky node is not warned about again.
        assert!(warnings(Some(&code), &code, false, false).is_empty());
    }
}
