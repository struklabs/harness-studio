//! Persistent autonomous creation. UI, CLI and API share this worker; no endpoint secrets on disk.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{generator, installed, package, ToolService, ToolSource};
use crate::compaction::Endpoint;
use crate::error_key::err;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryTool {
    pub name: String,
    pub description: String,
    pub category: String,
    pub source: ToolSource,
    pub server: Option<String>,
}

/// Built-ins have explicit categories; MCP metadata gets a conservative keyword fallback.
pub fn category(name: &str, description: &str) -> String {
    match name {
        "read_file" | "write_file" | "edit_file" | "file_glob_search" | "grep_search"
        | "get_info" => "file",
        "calculator" => "calculation",
        "current_datetime" => "datetime",
        "web_search" | "fetch_url" | "weather" | "geocode" => "web",
        _ => {
            let text = format!("{name} {description}").to_lowercase();
            if ["http", "web", "url", "network"]
                .iter()
                .any(|s| text.contains(s))
            {
                "web"
            } else if ["file", "directory", "folder"]
                .iter()
                .any(|s| text.contains(s))
            {
                "file"
            } else if ["calculate", "arithmetic"].iter().any(|s| text.contains(s)) {
                "calculation"
            } else if ["date", "timezone"].iter().any(|s| text.contains(s)) {
                "datetime"
            } else if ["text", "json", "csv", "string"]
                .iter()
                .any(|s| text.contains(s))
            {
                "text"
            } else {
                "other"
            }
        }
    }
    .into()
}

pub fn inventory(service: &ToolService, root: &Path) -> Vec<InventoryTool> {
    inventory_with(service, root, &crate::config::AppSettings::load().tools)
}

/// Brings late-connecting MCP servers in (bounded wait), then lists every tool. Called at every
/// ideation step so servers that connect after the run started are never missed.
pub async fn refresh_inventory(
    service: &ToolService,
    root: &Path,
    settings: &super::ToolSettings,
) -> Vec<InventoryTool> {
    service
        .sync_mcp_bounded(false, std::time::Duration::from_secs(8))
        .await;
    inventory_with(service, root, settings)
}

pub fn inventory_with(
    service: &ToolService,
    root: &Path,
    settings: &super::ToolSettings,
) -> Vec<InventoryTool> {
    let packages = installed::list(root);
    let mut tools: Vec<_> = service
        .listing(settings)
        .tools
        .into_iter()
        .map(|t| {
            let cat = packages
                .iter()
                .find(|p| p.name == t.def.name)
                .map(|p| p.category.clone())
                .unwrap_or_else(|| category(&t.def.name, &t.def.description));
            InventoryTool {
                name: t.def.name,
                description: t.def.description,
                category: cat,
                source: t.def.source,
                server: t.server,
            }
        })
        .collect();
    // Disabled and broken packages also occupy names and belong in My tools.
    for p in packages {
        if !tools.iter().any(|t| t.name == p.name) {
            tools.push(InventoryTool {
                name: p.name,
                description: p.description,
                category: p.category,
                source: ToolSource::Installed,
                server: None,
            });
        }
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub name: String,
    pub description: String,
    #[serde(default = "package::default_category")]
    pub category: String,
    #[serde(default)]
    pub permissions: package::Permissions,
}

/// Words that carry no purpose of their own, in names and descriptions.
const STOP_WORDS: &[&str] = &[
    "the",
    "and",
    "tool",
    "with",
    "for",
    "returns",
    "return",
    "from",
    "into",
    "given",
    "input",
    "this",
    "that",
    "its",
    "any",
    "all",
    "using",
    "based",
    "specified",
    "provided",
    "text",
];

/// Groups of stems that name the same action. The first entry is the canonical one.
const SYNONYMS: &[&[&str]] = &[
    &["count", "tally", "total"],
    &["convert", "transform", "translat", "chang", "turn"],
    &["calc", "calculat", "comput", "evaluat"],
    &["extract", "pull", "grab", "retriev", "find"],
    &["remov", "delet", "strip", "drop", "eras", "clean"],
    &["combin", "merg", "join", "concat", "append"],
    &["split", "separat", "divid", "chunk", "tokeniz"],
    &["pars", "decod", "deserializ", "read"],
    &["format", "prettif", "beautif", "pretty"],
    &["valid", "check", "verif", "lint"],
    &["generat", "creat", "make", "build", "produc"],
    &[
        "summar", "summary", "summaris", "summariz", "digest", "condens", "abstract",
    ],
    &["sort", "order", "rank"],
    &["slug", "slugif"],
    &["lower", "lowercas", "downcas"],
    &["upper", "uppercas", "capitaliz"],
    &["replac", "substitut", "swap"],
    &["dup", "duplicat", "dedup", "dedupl", "uniq", "unique"],
];

/// A crude, language-light stem: plural and agent/gerund endings go, doubled consonants and a
/// trailing `e` collapse, so `counter`, `counting` and `counts` all become `count`.
fn stem(word: &str) -> String {
    let mut w = word.to_string();
    for suffix in ["ers", "er", "ing", "ed", "es", "s"] {
        if let Some(rest) = w.strip_suffix(suffix) {
            if rest.len() >= 3 {
                w = rest.to_string();
                break;
            }
        }
    }
    let b = w.as_bytes();
    if b.len() >= 4 && b[b.len() - 1] == b[b.len() - 2] && !matches!(b[b.len() - 1], b'l' | b's') {
        w.pop();
    }
    if w.len() > 3 && w.ends_with('e') {
        w.pop();
    }
    w
}

fn canonical(word: &str) -> String {
    let s = stem(word);
    SYNONYMS
        .iter()
        .find(|g| g.contains(&s.as_str()))
        .map_or(s, |g| g[0].to_string())
}

/// Order-free, inflection-free and synonym-folded purpose words of a name or description.
fn tokens(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2 && !STOP_WORDS.contains(s))
        .map(canonical)
        .collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> (usize, f64) {
    let common = a.intersection(b).count();
    (common, common as f64 / a.union(b).count().max(1) as f64)
}

fn similar(a: &str, b: &str) -> bool {
    let (common, ratio) = jaccard(&tokens(a), &tokens(b));
    common >= 2 && ratio >= 0.65
}

/// Do two tools do the same job? Checked on meaning rather than spelling: the same set of
/// purpose words in the names (`word_counter` / `count_words`), near-identical descriptions, or
/// related names together with mostly overlapping name-plus-description vocabulary.
pub fn equivalent(name_a: &str, desc_a: &str, name_b: &str, desc_b: &str) -> bool {
    if name_a == name_b {
        return true;
    }
    // "Decode a Base64 encoded string" shares most words with "Encode a string
    // to Base64", but the names explicitly describe inverse operations. Check
    // these stems before synonym folding (which groups decode with parse).
    let encoding_actions = |name: &str| -> BTreeSet<String> {
        name.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .map(stem)
            .filter(|s| matches!(s.as_str(), "encod" | "decod"))
            .collect()
    };
    let (actions_a, actions_b) = (encoding_actions(name_a), encoding_actions(name_b));
    if actions_a.len() == 1 && actions_b.len() == 1 && actions_a != actions_b {
        return false;
    }
    let (na, nb) = (tokens(name_a), tokens(name_b));
    if !na.is_empty() && na == nb {
        return true;
    }
    if similar(name_a, name_b) || similar(desc_a, desc_b) {
        return true;
    }
    let (shared_name, _) = jaccard(&na, &nb);
    let all = |n: &str, d: &str| tokens(&format!("{n} {d}"));
    let (common, ratio) = jaccard(&all(name_a, desc_a), &all(name_b, desc_b));
    shared_name >= 1 && common >= 3 && ratio >= 0.5
}

/// A proposal dropped because an existing or earlier-accepted tool already does the same job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedProposal {
    pub name: String,
    pub matched: String,
}

/// Outcome of one ideation reply: the proposals to queue and the duplicates that were dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedProposals {
    pub accepted: Vec<Proposal>,
    pub skipped: Vec<SkippedProposal>,
}

/// One JSON object (prose/fences tolerated), with a strict schema validated before queueing.
pub fn parse_proposals(
    reply: &str,
    existing: &[InventoryTool],
    count: usize,
) -> Result<ParsedProposals, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        tools: Vec<Proposal>,
    }
    let value = generator::json_object(reply, &["tools"])
        .ok_or("Invalid ideation JSON: expected one complete object")?;
    let r: Reply =
        serde_json::from_value(value).map_err(|e| format!("Invalid ideation JSON: {e}"))?;
    if r.tools.is_empty() || r.tools.len() > 20 {
        return Err("Expected 1–20 proposals".into());
    }
    let mut accepted: Vec<Proposal> = vec![];
    let mut skipped: Vec<SkippedProposal> = vec![];
    for mut p in r.tools {
        if !package::valid_name(&p.name)
            || [
                "tool_name",
                "toolname",
                "name",
                "placeholder",
                "example_tool",
                "your_tool_name",
            ]
            .contains(&p.name.as_str())
            || p.description.trim().is_empty()
            || p.description.len() > 1024
            || !package::CATEGORIES.contains(&p.category.as_str())
            || p.permissions.shell
        {
            return Err("Invalid proposal name, description, category or shell permission".into());
        }
        package::normalize_model_network(&mut p.permissions.network)?;
        package::check_permissions(&p.permissions)?;
        let matched = existing
            .iter()
            .find(|t| equivalent(&t.name, &t.description, &p.name, &p.description))
            .map(|t| t.name.clone())
            .or_else(|| {
                accepted
                    .iter()
                    .find(|t| equivalent(&t.name, &t.description, &p.name, &p.description))
                    .map(|t| t.name.clone())
            });
        if let Some(matched) = matched {
            skipped.push(SkippedProposal {
                name: p.name,
                matched,
            });
        } else if accepted.len() < count {
            accepted.push(p);
        }
    }
    if accepted.is_empty() {
        return Err("All proposals duplicate existing tools".into());
    }
    Ok(ParsedProposals { accepted, skipped })
}

/// Merge one more ideation reply into `accepted`, judging it against the inventory and the
/// proposals already accepted. Returns the duplicates skipped by this reply.
fn merge_proposals(
    accepted: &mut Vec<Proposal>,
    reply: &str,
    inventory: &[InventoryTool],
    count: usize,
) -> Result<Vec<SkippedProposal>, String> {
    let mut known = inventory.to_vec();
    known.extend(accepted.iter().map(|p| InventoryTool {
        name: p.name.clone(),
        description: p.description.clone(),
        category: p.category.clone(),
        source: ToolSource::Installed,
        server: None,
    }));
    let parsed = parse_proposals(reply, &known, count.saturating_sub(accepted.len()))?;
    accepted.extend(parsed.accepted);
    Ok(parsed.skipped)
}

fn top_up_message(missing: usize, accepted: &[Proposal], skipped: &[SkippedProposal]) -> String {
    let names = |v: Vec<&str>| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };
    format!(
        "Only {} of the requested tools were new. Already accepted: {}. Rejected as duplicates: {}. Propose exactly {} MORE tools that are different in purpose from all of these and from the existing inventory. Reply ONLY strict JSON in the same format.",
        accepted.len(),
        names(accepted.iter().map(|p| p.name.as_str()).collect()),
        names(skipped.iter().map(|p| p.name.as_str()).collect()),
        missing
    )
}

fn ideation_prompt(count: usize, inventory: &[InventoryTool]) -> Result<String, String> {
    Ok(format!("Propose {} useful, diverse NEW tools. Existing inventory: {}. Reply ONLY strict JSON: {{\"tools\":[{{\"name\":\"<tool_name>\",\"description\":\"<one precise sentence>\",\"category\":\"text\",\"permissions\":{{\"network\":[],\"fs\":{{\"read\":[],\"write\":[]}}}}}}]}}. This JSON is a schema example only, not a tool suggestion. Replace every placeholder with an original tool name and purpose; do not copy the example or use placeholder names such as tool_name. Categories: file, web, calculation, datetime, text, other. QuickJS: run(args); no imports, require, fetch, process, timers or shell. Only harness.http.get/post with explicit domains, harness.fs.read/write/list inside workspace-relative folders. Prefer pure useful transformations. Text/data tools (CSV/JSON, HTML, Markdown, base64) must take content strings and return content by default, with empty filesystem permissions. Propose paths and filesystem permissions only for explicitly file-oriented tools; describe that contract clearly. Names 2–48 lowercase ASCII characters; no duplicate purpose. Keep descriptions under 300 characters. Descriptions must be internally consistent and verifiable; do not impose impossible constraints.", count, serde_json::to_string(inventory).map_err(|e| e.to_string())?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Queued,
    Building,
    Testing,
    Installing,
    AwaitingApproval,
    Installed,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub proposal: Proposal,
    pub status: ItemStatus,
    pub draft: Option<generator::Draft>,
    #[serde(default)]
    pub usage: generator::TokenUsage,
    #[serde(default)]
    pub history: Vec<generator::Attempt>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    Paused,
    Stopped,
    Completed,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub at: i64,
    pub stage: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    pub version: u32,
    pub id: String,
    pub status: RunStatus,
    pub stage: String,
    pub count: usize,
    #[serde(default)]
    pub planned: bool,
    #[serde(default)]
    pub usage: generator::TokenUsage,
    #[serde(default)]
    pub ideation_usage: generator::TokenUsage,
    #[serde(default)]
    pub estimate: generator::TokenEstimate,
    #[serde(default)]
    pub historical_attempts: Vec<generator::Attempt>,
    pub inventory: Vec<InventoryTool>,
    pub queue: Vec<QueueItem>,
    pub logs: Vec<Activity>,
    pub error: Option<String>,
}
impl Default for RunState {
    fn default() -> Self {
        Self {
            version: 1,
            id: String::new(),
            status: RunStatus::Idle,
            stage: "scan".into(),
            count: 8,
            planned: false,
            usage: Default::default(),
            ideation_usage: Default::default(),
            estimate: Default::default(),
            historical_attempts: vec![],
            inventory: vec![],
            queue: vec![],
            logs: vec![],
            error: None,
        }
    }
}
impl RunState {
    pub fn reorder(&mut self, ids: &[String]) -> Result<(), String> {
        let pending: Vec<_> = self
            .queue
            .iter()
            .filter(|q| q.status == ItemStatus::Queued)
            .map(|q| q.id.clone())
            .collect();
        if ids.len() != pending.len()
            || ids.iter().collect::<BTreeSet<_>>() != pending.iter().collect::<BTreeSet<_>>()
        {
            return Err("Reorder must contain every queued item exactly once".into());
        }
        let ordered: Vec<_> = ids
            .iter()
            .map(|id| self.queue.iter().find(|q| &q.id == id).unwrap().clone())
            .collect();
        let mut iter = ordered.into_iter();
        for q in &mut self.queue {
            if q.status == ItemStatus::Queued {
                *q = iter.next().unwrap();
            }
        }
        Ok(())
    }
    fn recover(&mut self) {
        if self.status == RunStatus::Running {
            self.status = RunStatus::Paused;
        }
        for q in &mut self.queue {
            if matches!(
                q.status,
                ItemStatus::Building | ItemStatus::Testing | ItemStatus::Installing
            ) {
                q.status = ItemStatus::Queued;
            }
        }
    }
}

pub fn needs_approval(p: &package::Permissions) -> bool {
    !p.network.is_empty() || !p.fs.read.is_empty() || !p.fs.write.is_empty() || p.shell
}

/// One durable worker per application data directory. Mutations and installs serialize.
pub struct AutoGenerator {
    path: PathBuf,
    root: PathBuf,
    state: Mutex<RunState>,
    active: AtomicBool,
    cancel: AtomicBool,
}
impl AutoGenerator {
    pub fn open(path: PathBuf, root: PathBuf) -> Result<Arc<Self>, String> {
        let mut state: RunState = match std::fs::read_to_string(&path) {
            Ok(s) => {
                serde_json::from_str(&s).map_err(|e| format!("Run state could not be read: {e}"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => RunState::default(),
            Err(e) => return Err(e.to_string()),
        };
        if state.version != 1 {
            return Err("Unsupported run version".into());
        }
        state.recover();
        Ok(Arc::new(Self {
            path,
            root,
            state: Mutex::new(state),
            active: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }))
    }
    pub fn shared() -> Result<Arc<Self>, String> {
        static SHARED: OnceLock<Result<Arc<AutoGenerator>, String>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                Self::open(
                    crate::paths::data_root().join("auto-tools/run.json"),
                    installed::tools_root(),
                )
            })
            .clone()
    }
    pub fn snapshot(&self) -> RunState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn change(
        &self,
        f: impl FnOnce(&mut RunState) -> Result<(), String>,
    ) -> Result<RunState, String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = state.clone();
        f(&mut next)?;
        let text = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(self.path.parent().ok_or("Missing state directory")?)
            .map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        *state = next.clone();
        Ok(next)
    }
    fn log(s: &mut RunState, stage: &str, message: impl Into<String>) {
        s.stage = stage.into();
        s.logs.push(Activity {
            at: chrono::Utc::now().timestamp_millis(),
            stage: stage.into(),
            message: message.into(),
        });
        if s.logs.len() > 500 {
            s.logs.remove(0);
        }
    }
    pub fn reorder(&self, ids: &[String]) -> Result<RunState, String> {
        self.change(|s| s.reorder(ids))
    }
    pub fn remove(&self, id: &str) -> Result<RunState, String> {
        self.change(|s| {
            let at = s
                .queue
                .iter()
                .position(|q| {
                    q.id == id
                        && matches!(
                            q.status,
                            ItemStatus::Queued | ItemStatus::Failed | ItemStatus::AwaitingApproval
                        )
                })
                .ok_or("Item is not removable")?;
            s.queue.remove(at);
            Ok(())
        })
    }
    pub fn retry(&self, id: &str) -> Result<RunState, String> {
        self.retry_items(Some(id))
    }
    /// Queue every failed item in one persisted mutation, before resuming the worker.
    pub fn retry_failed(&self) -> Result<RunState, String> {
        self.retry_items(None)
    }
    fn retry_items(&self, id: Option<&str>) -> Result<RunState, String> {
        self.change(|s| {
            let mut retried = 0;
            for q in &mut s.queue {
                if q.status == ItemStatus::Failed && id.is_none_or(|id| q.id == id) {
                    q.status = ItemStatus::Queued;
                    q.draft = None;
                    q.error = None;
                    retried += 1;
                }
            }
            if retried == 0 {
                return Err("Only failed items can be retried".into());
            }
            if s.status != RunStatus::Running {
                s.status = RunStatus::Paused;
            }
            s.error = None;
            Self::log(
                s,
                "queue",
                format!("{retried} failed tools queued for retry"),
            );
            Ok(())
        })
    }
    pub fn interrupt(&self, stop: bool) -> Result<RunState, String> {
        self.cancel.store(true, Ordering::SeqCst);
        self.change(|s| {
            if s.status == RunStatus::Running {
                s.status = if stop {
                    RunStatus::Stopped
                } else {
                    RunStatus::Paused
                };
            }
            Ok(())
        })
    }
    /// Human approval identifies a stored draft; no caller-supplied permissions/code accepted.
    pub fn approve(&self, id: &str, service: &ToolService) -> Result<RunState, String> {
        self.change(|s| {
            let q = s
                .queue
                .iter_mut()
                .find(|q| q.id == id && q.status == ItemStatus::AwaitingApproval)
                .ok_or("No draft awaiting approval")?;
            if let Err(e) = self.install(q, service) {
                q.status = ItemStatus::Failed;
                q.error = Some(e);
            }
            Ok(())
        })
    }
    /// Model/draft validation failures belong to this item, never the entire run.
    fn record_draft(q: &mut QueueItem, draft: Result<generator::Draft, String>) {
        let result = draft.and_then(|d| {
            generator::discard(&d.id);
            let manifest = package::parse_manifest(&d.manifest_json);
            let feedback = d.report.error.as_ref().map_or_else(
                || d.report.feedback(),
                |error| String::from(err("create.test_could_not_run").with("detail", error)),
            );
            let passed = d.report.passed;
            q.draft = Some(d);
            let m = manifest?;
            // A failing test is not a name/category problem (S39): its own message.
            if !passed {
                return Err(err("create.tests_failed").with("detail", feedback).into());
            }
            if m.name != q.proposal.name || m.category != q.proposal.category {
                let detail = format!(
                    "{} / {} ≠ {} / {}",
                    m.name, m.category, q.proposal.name, q.proposal.category
                );
                return Err(err("create.validation_failed")
                    .with("detail", detail)
                    .into());
            }
            q.status = if needs_approval(&m.permissions) {
                ItemStatus::AwaitingApproval
            } else {
                ItemStatus::Installing
            };
            q.error = None;
            Ok(())
        });
        if let Err(e) = result {
            q.status = ItemStatus::Failed;
            q.error = Some(e);
        }
    }
    fn install(&self, q: &mut QueueItem, service: &ToolService) -> Result<(), String> {
        let d = q.draft.as_ref().ok_or("Missing draft")?;
        let m = package::parse_manifest(&d.manifest_json)?;
        if !d.report.passed || m.name != q.proposal.name {
            return Err("Draft test/name mismatch".into());
        }
        // Idempotent crash recovery: only accept byte-identical own packages, never replace another tool.
        let target = self.root.join(&m.name);
        if target.exists() {
            if std::fs::read_to_string(target.join("manifest.json"))
                .ok()
                .as_deref()
                != Some(&d.manifest_json)
                || std::fs::read_to_string(target.join("index.js"))
                    .ok()
                    .as_deref()
                    != Some(&d.code)
                || std::fs::read_to_string(target.join("test.js"))
                    .ok()
                    .as_deref()
                    != Some(&d.test)
            {
                return Err("Tool name already installed".into());
            }
        } else {
            if service.registry().get(&m.name).is_some() {
                return Err("Tool name already registered".into());
            }
            let staging = self
                .path
                .parent()
                .unwrap()
                .join(format!("package-{}", q.id));
            std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
            let result = (|| {
                std::fs::write(staging.join("manifest.json"), &d.manifest_json)
                    .map_err(|e| e.to_string())?;
                std::fs::write(staging.join("index.js"), &d.code).map_err(|e| e.to_string())?;
                std::fs::write(staging.join("test.js"), &d.test).map_err(|e| e.to_string())?;
                installed::install(&self.root, &staging, service.registry())?;
                Ok::<_, String>(())
            })();
            let _ = std::fs::remove_dir_all(&staging);
            result?;
        }
        installed::sync(&self.root, service.registry());
        q.status = ItemStatus::Installed;
        q.error = None;
        Ok(())
    }
    /// Start/resume detaches the task from its page. `notify` sends persisted state events.
    pub fn launch(
        self: &Arc<Self>,
        endpoint: Endpoint,
        count: usize,
        resume: bool,
        notify: Arc<dyn Fn(RunState) + Send + Sync>,
    ) -> Result<RunState, String> {
        if !(1..=20).contains(&count) || endpoint.base_url.trim().is_empty() {
            return Err("Choose a model and 1–20 tools".into());
        }
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "Start the automatic generator from an async runtime".to_string())?;
        if self.active.swap(true, Ordering::SeqCst) {
            return Err("Automatic generator is busy".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        let result = self.change(|s| {
            if resume {
                if !matches!(
                    s.status,
                    RunStatus::Paused
                        | RunStatus::Stopped
                        | RunStatus::Failed
                        | RunStatus::Completed
                ) {
                    return Err("Run cannot resume".into());
                }
            } else {
                if s.queue
                    .iter()
                    .any(|q| matches!(q.status, ItemStatus::Queued | ItemStatus::AwaitingApproval))
                {
                    return Err("Finish or remove pending items first".into());
                }
                let mut historical_attempts = s.historical_attempts.clone();
                historical_attempts.extend(s.queue.iter().flat_map(|q| q.history.clone()));
                if historical_attempts.len() > 100 {
                    historical_attempts.drain(..historical_attempts.len() - 100);
                }
                *s = RunState {
                    historical_attempts,
                    id: uuid::Uuid::new_v4().to_string(),
                    count,
                    ..Default::default()
                };
            }
            s.status = RunStatus::Running;
            s.error = None;
            Ok(())
        });
        let state = match result {
            Ok(s) => s,
            Err(e) => {
                self.active.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };
        let worker = self.clone();
        runtime.spawn(async move {
            if let Err(error) = worker.run(endpoint, &notify, ToolService::shared()).await {
                let _ = worker.change(|s| {
                    s.recover();
                    if !worker.cancel.load(Ordering::SeqCst) {
                        s.status = RunStatus::Failed;
                        s.error = Some(error.clone());
                        Self::log(s, "error", error);
                    }
                    Ok(())
                });
            }
            worker.active.store(false, Ordering::SeqCst);
            notify(worker.snapshot());
        });
        Ok(state)
    }
    pub async fn estimate(
        &self,
        endpoint: Endpoint,
        count: usize,
    ) -> Result<generator::TokenEstimate, String> {
        self.estimate_cancel(endpoint, count, &AtomicBool::new(false))
            .await
    }
    async fn estimate_cancel(
        &self,
        endpoint: Endpoint,
        count: usize,
        cancel: &AtomicBool,
    ) -> Result<generator::TokenEstimate, String> {
        let s = self.snapshot();
        let mut history = s.historical_attempts;
        history.extend(s.queue.iter().flat_map(|q| q.history.clone()));
        generator::estimate(
            &generator::GenerateRequest {
                description: "Write a useful sandboxed transformation tool with two to five cases."
                    .into(),
                network: vec![],
                fs_read: vec![],
                fs_write: vec![],
                endpoint,
            },
            count,
            &ideation_prompt(count, &s.inventory)?,
            &history,
            cancel,
        )
        .await
    }

    async fn run(
        &self,
        endpoint: Endpoint,
        notify: &Arc<dyn Fn(RunState) + Send + Sync>,
        service: &ToolService,
    ) -> Result<(), String> {
        if self.snapshot().estimate.high == 0 {
            let estimate = self
                .estimate_cancel(endpoint.clone(), self.snapshot().count, &self.cancel)
                .await?;
            notify(self.change(|s| {
                s.estimate = estimate;
                Ok(())
            })?);
        }
        if !self.snapshot().planned {
            notify(self.change(|s| {
                Self::log(s, "scan", "Reading every existing tool");
                Ok(())
            })?);
            let settings = crate::config::AppSettings::load().tools;
            let tools = refresh_inventory(service, &self.root, &settings).await;
            notify(self.change(|s| {
                s.inventory = tools;
                for t in s.inventory.clone() {
                    Self::log(
                        s,
                        "scan",
                        format!("{} · {} · {:?}", t.name, t.category, t.source),
                    );
                }
                Self::log(s, "ideas", "Planning useful tools without duplicates");
                Ok(())
            })?);
            let s = self.snapshot();
            let prompt = ideation_prompt(s.count, &s.inventory)?;
            let mut messages = vec![
                json!({"role":"system","content":"You plan sandboxed tools. Output JSON only. No thinking."}),
                json!({"role":"user","content":prompt}),
            ];
            let mut accepted: Vec<Proposal> = vec![];
            let mut skipped_all: Vec<SkippedProposal> = vec![];
            let mut last_err = String::from("No proposals");
            // Attempts cover both JSON/duplicate fixes and topping up a short list.
            for attempt in 0..3 {
                // MCP servers may connect after the run started: look again before every
                // model call and judge the answer against what exists right now.
                if attempt > 0 {
                    let fresh = refresh_inventory(service, &self.root, &settings).await;
                    notify(self.change(|s| {
                        let known: BTreeSet<_> =
                            s.inventory.iter().map(|t| t.name.clone()).collect();
                        let late: Vec<_> = fresh
                            .iter()
                            .filter(|t| !known.contains(&t.name))
                            .map(|t| t.name.clone())
                            .collect();
                        if !late.is_empty() {
                            Self::log(s, "scan", format!("Late tools: {}", late.join(", ")));
                        }
                        s.inventory = fresh;
                        Ok(())
                    })?);
                }
                let s = self.snapshot();
                let reply =
                    generator::ask(&endpoint, &messages, &self.cancel, "harness-auto-tools")
                        .await?;
                notify(self.change(|s| {
                    s.usage.add(&reply.usage);
                    s.ideation_usage.add(&reply.usage);
                    Self::log(
                        s,
                        "ideas",
                        format!(
                            "Model call · {}↑{} ↓{}",
                            if reply.usage.estimated { "≈" } else { "" },
                            reply.usage.prompt_tokens,
                            reply.usage.completion_tokens
                        ),
                    );
                    Ok(())
                })?);
                if reply.truncated {
                    return Err(err("create.model_reply_truncated").into());
                }
                let reply = reply.text;
                match merge_proposals(&mut accepted, &reply, &s.inventory, s.count) {
                    Ok(skipped) => {
                        notify(self.change(|s| {
                            for k in &skipped {
                                Self::log(
                                    s,
                                    "ideas",
                                    format!(
                                        "Skipped duplicate: {} (same purpose as {})",
                                        k.name, k.matched
                                    ),
                                );
                            }
                            Ok(())
                        })?);
                        skipped_all.extend(skipped);
                        if accepted.len() >= s.count {
                            break;
                        }
                        let missing = s.count - accepted.len();
                        notify(self.change(|s| {
                            Self::log(
                                s,
                                "ideas",
                                format!("Top-up: asking for {missing} more tool(s)"),
                            );
                            Ok(())
                        })?);
                        messages.push(json!({"role":"assistant","content":reply}));
                        messages.push(json!({"role":"user","content":top_up_message(missing, &accepted, &skipped_all)}));
                    }
                    Err(e) => {
                        notify(self.change(|s| {
                            Self::log(s, "ideas", format!("Retry: {e}"));
                            Ok(())
                        })?);
                        messages.push(json!({"role":"assistant","content":reply}));
                        messages.push(json!({"role":"user","content":format!("Fix JSON/duplicates: {e}. Return complete strict JSON.")}));
                        last_err = e;
                    }
                }
            }
            if accepted.is_empty() {
                return Err(last_err);
            }
            let proposals = accepted;
            let wanted = self.snapshot().count;
            let duplicates = skipped_all.len();
            notify(self.change(|s| {
                s.planned = true;
                s.queue = proposals
                    .into_iter()
                    .map(|proposal| QueueItem {
                        id: uuid::Uuid::new_v4().to_string(),
                        proposal,
                        status: ItemStatus::Queued,
                        usage: Default::default(),
                        history: vec![],
                        draft: None,
                        error: None,
                    })
                    .collect();
                if s.queue.len() < wanted {
                    Self::log(
                        s,
                        "queue",
                        format!(
                            "{} of {} tools queued ({} duplicates skipped)",
                            s.queue.len(),
                            wanted,
                            duplicates
                        ),
                    );
                } else {
                    Self::log(s, "queue", format!("{} tools queued", s.queue.len()));
                }
                Ok(())
            })?);
        }
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err("Cancelled".into());
            }
            // Selection and completion share the same lock as retry. A retry cannot
            // land between an empty snapshot and a later unconditional completion.
            let mut selected = None;
            let state = self.change(|s| {
                selected = s
                    .queue
                    .iter()
                    .find(|q| q.status == ItemStatus::Queued)
                    .cloned();
                if selected.is_none() && s.status == RunStatus::Running {
                    s.status = RunStatus::Completed;
                    Self::log(
                        s,
                        "install",
                        "Run completed; permission approvals remain manual",
                    );
                }
                Ok(())
            })?;
            let Some(item) = selected else {
                notify(state);
                return Ok(());
            };
            if item.draft.as_ref().is_some_and(|d| d.report.passed) {
                notify(self.change(|s| {
                    let q = s
                        .queue
                        .iter_mut()
                        .find(|q| q.id == item.id)
                        .ok_or("Queue item gone")?;
                    let result = (|| {
                        let m = package::parse_manifest(&q.draft.as_ref().unwrap().manifest_json)?;
                        if needs_approval(&m.permissions) {
                            q.status = ItemStatus::AwaitingApproval;
                            Ok(())
                        } else {
                            self.install(q, service)
                        }
                    })();
                    if let Err(e) = result {
                        q.status = ItemStatus::Failed;
                        q.error = Some(e);
                    }
                    Ok(())
                })?);
                continue;
            }
            let p = item.proposal;
            let req = generator::GenerateRequest {
                description: format!(
                    "Name exactly {}. Category exactly {}. {}",
                    p.name, p.category, p.description
                ),
                network: p.permissions.network,
                fs_read: p.permissions.fs.read,
                fs_write: p.permissions.fs.write,
                endpoint: endpoint.clone(),
            };
            let progress = |stage: &str| {
                if let Ok(s) = self.change(|s| {
                    if let Some(raw) = stage.strip_prefix("usage:") {
                        let usage: generator::TokenUsage =
                            serde_json::from_str(raw).map_err(|e| e.to_string())?;
                        s.usage.add(&usage);
                        if let Some(q) = s.queue.iter_mut().find(|q| q.id == item.id) {
                            q.usage.add(&usage);
                        }
                        Self::log(
                            s,
                            "build",
                            format!(
                                "{} · Model call · {}↑{} ↓{}",
                                p.name,
                                if usage.estimated { "≈" } else { "" },
                                usage.prompt_tokens,
                                usage.completion_tokens
                            ),
                        );
                        return Ok(());
                    }
                    if let Some(raw) = stage.strip_prefix("attempt:") {
                        let attempt: generator::Attempt =
                            serde_json::from_str(raw).map_err(|e| e.to_string())?;
                        if let Some(q) = s.queue.iter_mut().find(|q| q.id == item.id) {
                            q.history.push(attempt.clone());
                        }
                        Self::log(
                            s,
                            "test",
                            format!(
                                "{} · Attempt {}: {} · failed: {}",
                                p.name, attempt.number, attempt.change, attempt.failed_cases
                            ),
                        );
                        return Ok(());
                    }
                    if let Some(q) = s.queue.iter_mut().find(|q| q.id == item.id) {
                        q.status = if stage == "test" {
                            ItemStatus::Testing
                        } else {
                            ItemStatus::Building
                        };
                    }
                    Self::log(
                        s,
                        if stage == "test" { "test" } else { "build" },
                        format!("{} · {stage}", p.name),
                    );
                    Ok(())
                }) {
                    notify(s);
                }
            };
            let draft = generator::generate_observed(
                &req,
                service.registry(),
                &self.cancel,
                true,
                Some((&p.name, &p.category)),
                &progress,
            )
            .await;
            if self.cancel.load(Ordering::SeqCst) {
                if let Ok(d) = &draft {
                    generator::discard(&d.id);
                }
                return Err("Cancelled".into());
            }
            notify(self.change(|s| {
                let q = s
                    .queue
                    .iter_mut()
                    .find(|q| q.id == item.id)
                    .ok_or("Queue item gone")?;
                Self::record_draft(q, draft);
                Self::log(
                    s,
                    "test",
                    format!(
                        "{} · {:?}",
                        p.name,
                        s.queue.iter().find(|q| q.id == item.id).unwrap().status
                    ),
                );
                Ok(())
            })?);
            notify(self.change(|s| {
                let q = s.queue.iter_mut().find(|q| q.id == item.id).unwrap();
                if q.status == ItemStatus::Installing {
                    if let Err(e) = self.install(q, service) {
                        q.status = ItemStatus::Failed;
                        q.error = Some(e);
                    }
                    Self::log(
                        s,
                        "install",
                        format!(
                            "{} · {:?}",
                            p.name,
                            s.queue.iter().find(|q| q.id == item.id).unwrap().status
                        ),
                    );
                }
                Ok(())
            })?);
        }
    }
}

#[cfg(test)]
mod tests;
