//! The models an agent tool (OpenCode, Claude Code, Codex...) can use through Harness, with the
//! two things that decide whether agent work is usable on them:
//!
//! * **Native tool calls.** Agents drive everything through tool calls. llama-server returns a
//!   structured `tool_calls` answer only when the chat template handles tools *and* the model
//!   writes calls in the format the template's parser expects. The template alone is not
//!   enough: Qwen2.5-Coder-14B's template has the same `<tool_call>` markers as Qwen3's and
//!   llama-server reports `chat_template_caps.supports_tools = true` for both, yet the coder
//!   model writes the call as plain JSON text (`<tools>{"name": ...}</tools>`), which OpenCode
//!   shows as text (end-to-end test, 30.09). So the verdict comes from real answers, strongest
//!   first:
//!   1. a tool check ([`check`]): one real Chat Completions request with a tool
//!      ([`super::conntest`]); stored per alias with the model file it ran against;
//!   2. the gateway log: answers to this alias that ended in a native tool call
//!      (`finish_reason` `tool_calls` / `tool_use` / `function_call`);
//!   3. the GGUF chat template: no tool handling means llama-server cannot return tool calls;
//!      with it, support is only possible (not verified).
//!
//!   No model names are hard-coded.
//! * **Tool-calling level.** Beyond "has it ever worked", the gateway log says how *reliably*:
//!   the success ratio of the tool rounds the gateway diagnosed (`gateway::tool_diag`) gives
//!   native / generic / limited (`tool_level`) and, with the model size, the number of tools
//!   worth enabling at once (`recommended_tools`).
//! * **Prompt processing speed.** OpenCode sends a ~29k-token system prompt; its first answer
//!   waits for all of it (68 s on Qwen2.5-Coder-14B, RX 7900 GRE). The speed comes from the
//!   gateway log of this alias when there is one.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::conntest;
use super::launch::WireApi;
use crate::analytics::AgentSignals;
use crate::domain::{detect_kind, ModelDefinition, ModelKind, ModelRef};
use crate::error_key::err;
use crate::gateway::tool_diag::{recommended_tools, ToolLevel, ToolRounds};
use crate::gguf::GgufMetadata;
use crate::params::ParamValue;

/// How sure Harness is that the model answers tool requests with native tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSupport {
    /// A real request with a tool came back as a structured tool call.
    Verified,
    /// A real request with a tool came back as text (e.g. the call written as JSON text).
    Failed,
    /// The chat template handles tools, but no real tool call has been seen yet.
    TemplateOnly,
    /// The chat template has no tool handling: llama-server cannot return tool calls.
    NoTemplate,
    /// No local file to read (not downloaded yet) or a custom template.
    Unknown,
}

/// Where a verified/failed verdict came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEvidence {
    Check,
    GatewayLog,
    Template,
    None,
}

/// Result of one tool check, stored per alias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCheck {
    pub alias: String,
    /// The model file the check ran against (a later file change makes it stale).
    pub model_path: Option<String>,
    /// The definition's template override at the time (`None` = the GGUF's own).
    #[serde(default)]
    pub chat_template: Option<String>,
    /// The answer was a structured tool call.
    pub tool_called: bool,
    /// The answer was text that looks like a tool call (`{"name": ..., "arguments": ...}`,
    /// `<tool_call>`): the model tried, but not in a form llama-server parses.
    pub text_call: bool,
    /// Excerpt of the answer text.
    pub detail: String,
    pub latency_ms: u64,
    /// Unix seconds.
    pub checked_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolChecks {
    #[serde(default)]
    pub checks: Vec<ToolCheck>,
}

impl ToolChecks {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e)
            })?;
        }
        let text = serde_json::to_vec_pretty(self).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| {
            err("fs.write_failed")
                .with("path", tmp.display())
                .with("error", e)
        })?;
        std::fs::rename(&tmp, path).map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })
    }

    /// Replaces the alias's earlier check.
    pub fn put(&mut self, check: ToolCheck) {
        self.checks
            .retain(|c| !c.alias.eq_ignore_ascii_case(&check.alias));
        self.checks.push(check);
    }

    fn get(&self, alias: &str) -> Option<&ToolCheck> {
        self.checks
            .iter()
            .find(|c| c.alias.eq_ignore_ascii_case(alias))
    }
}

/// `<data>/config/tool_checks.json`.
pub fn checks_file() -> PathBuf {
    crate::paths::config_dir().join("tool_checks.json")
}

/// One model an agent tool can be pointed at.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentModel {
    pub alias: String,
    pub definition_id: Option<String>,
    pub model_path: Option<String>,
    /// A chat model (an LLM with a chat template); embedding and rerank models are false.
    /// `None` when the file cannot be read (not downloaded yet).
    pub chat: Option<bool>,
    /// The definition's load context (`None` = llama-server's default).
    pub context_size: Option<u32>,
    /// Parameter count from the GGUF header.
    pub params: Option<u64>,
    /// `general.size_label` ("14B").
    pub size_label: Option<String>,
    pub file_bytes: Option<u64>,
    /// The GGUF chat template handles tools (`None` = unknown).
    pub template_tools: Option<bool>,
    pub tools: ToolSupport,
    pub evidence: ToolEvidence,
    /// The latest tool check (only when it ran against the current file and template).
    pub check: Option<ToolCheck>,
    /// Native tool-call answers in the gateway log.
    pub native_tool_calls: u64,
    /// Tool rounds with a clean call / rounds the model or its template failed (gateway log).
    pub tool_rounds_ok: u64,
    pub tool_rounds_bad: u64,
    /// How reliably the model calls tools, from those rounds (`None`: too few rounds yet).
    pub tool_level: Option<ToolLevel>,
    /// Most tools worth enabling at once for this model (size and level; see
    /// [`recommended_tools`]).
    pub recommended_tools: u32,
    /// Prompt processing speed from the gateway log, tokens/s.
    pub prompt_tps: Option<f64>,
    /// Uncached prompt tokens the speed was measured over.
    pub prompt_tokens_timed: u64,
    /// Prompt processing speed of the newest llama-bench of this file (the side the definition
    /// loads on), tokens/s: the estimate for a model that has no long gateway requests yet.
    pub bench_prompt_tps: Option<f64>,
    /// A model of a Harness Link (served by another Harness over the network): there is no
    /// local file, so no hardware fit applies, and tool support is not known from a template.
    #[serde(default)]
    pub remote: bool,
}

/// The verdict from the signals, strongest first (see the module docs).
pub fn tool_support(
    template_tools: Option<bool>,
    check: Option<&ToolCheck>,
    native_tool_calls: u64,
) -> (ToolSupport, ToolEvidence) {
    match check {
        Some(c) if c.tool_called => (ToolSupport::Verified, ToolEvidence::Check),
        Some(_) => (ToolSupport::Failed, ToolEvidence::Check),
        None if native_tool_calls > 0 => (ToolSupport::Verified, ToolEvidence::GatewayLog),
        None => match template_tools {
            Some(true) => (ToolSupport::TemplateOnly, ToolEvidence::Template),
            Some(false) => (ToolSupport::NoTemplate, ToolEvidence::Template),
            None => (ToolSupport::Unknown, ToolEvidence::None),
        },
    }
}

fn local_path(d: &ModelDefinition) -> Option<PathBuf> {
    match &d.base {
        ModelRef::Local { path } => Some(path.clone()),
        ModelRef::Hf { .. } => None,
    }
}

fn load_ctx(d: &ModelDefinition) -> Option<u32> {
    match d.load.get("context_size")? {
        ParamValue::Int(n) if *n > 0 => u32::try_from(*n).ok(),
        ParamValue::Float(f) if *f > 0.0 => Some(*f as u32),
        ParamValue::Text(t) => t.trim().parse().ok().filter(|n| *n > 0),
        _ => None,
    }
}

fn template_of(d: &ModelDefinition) -> Option<String> {
    d.chat_template.clone().filter(|t| !t.trim().is_empty())
}

/// The check applies to the definition as it is now (same file, same template override).
fn check_matches(c: &ToolCheck, d: &ModelDefinition) -> bool {
    let path = local_path(d).map(|p| p.to_string_lossy().to_string());
    c.model_path == path && c.chat_template == template_of(d)
}

/// Shared chat eligibility for agent lists and every client config writer. Unreadable GGUFs
/// remain unknown (remote/not downloaded), but speech files and non-chat serving modes do not.
pub fn chat_capability(d: &ModelDefinition, meta: Option<&GgufMetadata>) -> Option<bool> {
    let name = match &d.base {
        ModelRef::Local { path } => {
            if crate::engine::is_ggml_file(path) {
                return Some(false);
            }
            // No GGUF header to read: a Python-engine folder names its task, and a single
            // non-GGUF file (a `.ckpt`/`.safetensors` image model) never chats.
            if path.is_dir() {
                if let Some(task) = crate::engine::python_hf::launch_task(path) {
                    return Some(crate::engine::python_hf::CHAT_TASKS.contains(&task.as_str()));
                }
            } else if path.is_file()
                && !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
            {
                return Some(false);
            }
            path.to_string_lossy().to_string()
        }
        ModelRef::Hf { file, .. } => file.clone(),
    };
    // A not-yet-downloaded whisper model cannot be read for its magic yet.
    let file = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&name)
        .to_ascii_lowercase();
    if file.starts_with("ggml-") && file.ends_with(".bin") {
        return Some(false);
    }
    let kind = detect_kind(meta);
    if crate::serve_mode::effective(&d.load, kind, false) != crate::serve_mode::ServeMode::Chat {
        return Some(false);
    }
    meta.map(|m| kind == ModelKind::Llm && (m.has_chat_template || template_of(d).is_some()))
}

/// The agent models among `defs` (chat models and unreadable ones; embedding/rerank models are
/// left out). `meta` reads a GGUF header (cached by the caller if it wants).
pub fn build(
    defs: &[ModelDefinition],
    meta: impl Fn(&Path) -> Option<GgufMetadata>,
    checks: &ToolChecks,
    signals: &[AgentSignals],
) -> Vec<AgentModel> {
    defs.iter()
        .filter_map(|d| {
            if crate::link::parse_model(&d.id).is_some() {
                return Some(remote_model(d));
            }
            let path = local_path(d);
            let m = path.as_deref().filter(|p| p.is_file()).and_then(&meta);
            let chat = chat_capability(d, m.as_ref());
            if chat == Some(false) {
                return None;
            }
            // A template override replaces the GGUF's: its tool handling is not known here.
            let template_tools = match (&m, template_of(d)) {
                (Some(m), None) => Some(m.template_supports_tools),
                _ => None,
            };
            let check = checks
                .get(&d.alias)
                .filter(|c| check_matches(c, d))
                .cloned();
            let sig = signals
                .iter()
                .find(|s| s.alias.eq_ignore_ascii_case(&d.alias));
            let native = sig.map(|s| s.native_tool_calls).unwrap_or(0);
            let (tools, evidence) = tool_support(template_tools, check.as_ref(), native);
            let rounds = ToolRounds {
                ok: sig.map(|s| s.tool_ok).unwrap_or(0),
                bad: sig.map(|s| s.tool_bad).unwrap_or(0),
            };
            let tool_level = rounds.level(template_tools);
            let params = m.as_ref().and_then(|m| m.param_count);
            Some(AgentModel {
                alias: d.alias.clone(),
                definition_id: Some(d.id.clone()),
                model_path: path.as_ref().map(|p| p.to_string_lossy().to_string()),
                chat,
                context_size: load_ctx(d),
                params: m.as_ref().and_then(|m| m.param_count),
                size_label: m.as_ref().and_then(|m| m.size_label.clone()),
                file_bytes: path
                    .as_deref()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map(|md| md.len()),
                template_tools,
                tools,
                evidence,
                check,
                native_tool_calls: native,
                tool_rounds_ok: rounds.ok,
                tool_rounds_bad: rounds.bad,
                tool_level,
                recommended_tools: recommended_tools(params, tool_level),
                prompt_tps: sig.and_then(|s| s.prompt_tps),
                prompt_tokens_timed: sig.map(|s| s.prompt_tokens_timed).unwrap_or(0),
                bench_prompt_tps: None,
                remote: false,
            })
        })
        .collect()
}

/// A Harness Link model. Its kind is not known from here (the other Harness serves it), so
/// `chat` stays unknown instead of claiming a verdict; the id is matched exactly by tokens.
fn remote_model(d: &ModelDefinition) -> AgentModel {
    AgentModel {
        alias: d.alias.clone(),
        definition_id: Some(d.id.clone()),
        model_path: None,
        chat: None,
        context_size: None,
        params: None,
        size_label: None,
        file_bytes: None,
        template_tools: None,
        tools: ToolSupport::Unknown,
        evidence: ToolEvidence::None,
        check: None,
        native_tool_calls: 0,
        tool_rounds_ok: 0,
        tool_rounds_bad: 0,
        tool_level: None,
        recommended_tools: recommended_tools(None, None),
        prompt_tps: None,
        prompt_tokens_timed: 0,
        bench_prompt_tps: None,
        remote: true,
    }
}

/// The agent models of the user's definitions, with the stored checks and the gateway log.
pub fn list() -> Result<Vec<AgentModel>, String> {
    let defs = crate::domain::store::Domain::global().definitions()?;
    let checks = ToolChecks::load(&checks_file());
    // Analytics may be off or unreadable: the other signals still count.
    let signals = crate::analytics::agent_signals().unwrap_or_default();
    let mut models = build(&defs, crate::models::cached_metadata, &checks, &signals);
    let runs = crate::benchmark::load_history();
    for m in &mut models {
        let def = defs
            .iter()
            .find(|d| Some(&d.id) == m.definition_id.as_ref());
        m.bench_prompt_tps = def.zip(m.model_path.as_deref()).and_then(|(d, path)| {
            crate::benchmark::prompt_speed(&runs, Path::new(path), load_gpu_layers(d))
        });
    }
    Ok(models)
}

/// [`list`] plus the models of the user's enabled Harness Links, for features that only send
/// chat requests through the gateway (agents, Paperclip). Other lists stay local-only: their
/// callers read model files.
pub fn list_with_link() -> Result<Vec<AgentModel>, String> {
    let mut models = list()?;
    models.extend(build(
        &crate::link::definitions(),
        |_| None,
        &ToolChecks::default(),
        &[],
    ));
    Ok(models)
}

fn load_gpu_layers(d: &ModelDefinition) -> Option<u32> {
    match d.load.get("gpu_layers")? {
        ParamValue::Int(n) => u32::try_from(*n).ok(),
        ParamValue::Float(f) if *f >= 0.0 => Some(*f as u32),
        ParamValue::Text(t) => t.trim().parse().ok(),
        _ => None,
    }
}

/// Text that looks like a tool call the parser did not recognise.
pub fn looks_like_text_call(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("<tool_call>")
        || t.contains("<tools>")
        || t.contains("<function")
        || (t.contains("\"name\"") && (t.contains("\"arguments\"") || t.contains("\"parameters\"")))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sends one real Chat Completions request with a tool to `alias` at `base_url`
/// (`http://host:port/v1`; through the gateway this loads the model if needed) and stores the
/// result for the definition's current file in `store`. An endpoint error is returned, not
/// stored.
pub async fn check(
    base_url: &str,
    api_key: Option<&str>,
    alias: &str,
    def: Option<&ModelDefinition>,
    store: &Path,
) -> Result<ToolCheck, String> {
    let t = conntest::test_connection(
        WireApi::Chat,
        base_url,
        api_key,
        alias,
        "harness-studio-tool-check",
    )
    .await;
    if !t.ok {
        return Err(err("agent_models.check_failed")
            .with("model", alias)
            .with("detail", &t.detail)
            .into());
    }
    let result = ToolCheck {
        alias: alias.to_string(),
        model_path: def
            .and_then(local_path)
            .map(|p| p.to_string_lossy().to_string()),
        chat_template: def.and_then(template_of),
        tool_called: t.tool_called,
        text_call: !t.tool_called && looks_like_text_call(&t.detail),
        detail: t.detail,
        latency_ms: t.latency_ms,
        checked_at: now_secs(),
    };
    let mut all = ToolChecks::load(store);
    all.put(result.clone());
    all.save(store)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamValues;

    fn def(alias: &str, path: &Path, ctx: Option<i64>) -> ModelDefinition {
        let mut load = ParamValues::new();
        if let Some(c) = ctx {
            load.insert("context_size".into(), ParamValue::Int(c));
        }
        ModelDefinition {
            id: format!("d-{alias}"),
            alias: alias.into(),
            base: ModelRef::Local {
                path: path.to_path_buf(),
            },
            description: String::new(),
            mmproj: None,
            adapters: Vec::new(),
            draft: None,
            chat_template: None,
            chat_template_kwargs: None,
            extra_args: None,
            load,
            default_preset_id: None,
            ttl_seconds: None,
            pinned: false,
            capture_content: false,
            content_retention_days: None,
            auto_created: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn check_of(alias: &str, path: &Path, called: bool) -> ToolCheck {
        ToolCheck {
            alias: alias.into(),
            model_path: Some(path.to_string_lossy().to_string()),
            chat_template: None,
            tool_called: called,
            text_call: !called,
            detail: String::new(),
            latency_ms: 1,
            checked_at: 1,
        }
    }

    #[test]
    fn link_models_are_listed_as_remote_without_a_local_fit() {
        let local = def("local", Path::new("missing.gguf"), None);
        let mut remote = ModelDefinition::new(
            "remote:lab-box/qwen3:8b",
            ModelRef::Local {
                path: PathBuf::from("remote:lab-box/qwen3:8b"),
            },
        );
        remote.id = "remote:lab-box/qwen3:8b".into();
        let models = build(&[local, remote], |_| None, &ToolChecks::default(), &[]);
        assert_eq!(models.len(), 2);
        assert!(!models[0].remote);
        let r = &models[1];
        assert!(r.remote);
        assert_eq!(r.alias, "remote:lab-box/qwen3:8b");
        // No file: nothing to fit, no verdict from a template, but selectable as a chat model.
        assert!(r.model_path.is_none() && r.file_bytes.is_none() && r.context_size.is_none());
        assert_eq!(r.tools, ToolSupport::Unknown);
        assert_ne!(r.chat, Some(false));
    }

    #[test]
    fn verdict_prefers_real_answers_over_the_template() {
        let c_ok = ToolCheck {
            tool_called: true,
            ..check_of("a", Path::new("x"), true)
        };
        let c_bad = check_of("a", Path::new("x"), false);
        assert_eq!(
            tool_support(Some(true), Some(&c_bad), 5),
            (ToolSupport::Failed, ToolEvidence::Check)
        );
        assert_eq!(
            tool_support(Some(false), Some(&c_ok), 0),
            (ToolSupport::Verified, ToolEvidence::Check)
        );
        assert_eq!(
            tool_support(Some(true), None, 2),
            (ToolSupport::Verified, ToolEvidence::GatewayLog)
        );
        assert_eq!(
            tool_support(Some(true), None, 0),
            (ToolSupport::TemplateOnly, ToolEvidence::Template)
        );
        assert_eq!(
            tool_support(Some(false), None, 0).0,
            ToolSupport::NoTemplate
        );
        assert_eq!(tool_support(None, None, 0).0, ToolSupport::Unknown);
    }

    #[test]
    fn text_calls_are_recognised() {
        // The answers Qwen2.5-Coder-14B gave through llama-server (30.09).
        assert!(looks_like_text_call(
            "<tools>\n{\"name\": \"get_weather\", \"arguments\": {\"city\": \"Paris\"}}\n</tools>"
        ));
        assert!(looks_like_text_call(
            "{\n  \"name\": \"glob\",\n  \"arguments\": {\n    \"pattern\": \"**/*\"\n  }\n}"
        ));
        assert!(!looks_like_text_call("It is sunny in Paris."));
    }

    #[test]
    fn checks_are_stored_per_alias_and_go_stale_with_the_file() {
        let d = tempfile::tempdir().unwrap();
        let store = d.path().join("config").join("tool_checks.json");
        let a = d.path().join("a.gguf");
        let b = d.path().join("b.gguf");
        let mut all = ToolChecks::default();
        all.put(check_of("coder", &a, false));
        all.put(check_of("CODER", &a, true));
        all.save(&store).unwrap();
        let loaded = ToolChecks::load(&store);
        assert_eq!(loaded.checks.len(), 1);
        assert!(loaded.checks[0].tool_called);
        // Missing files: no header, chat unknown, the stored check still applies.
        let models = build(&[def("coder", &a, Some(65_536))], |_| None, &loaded, &[]);
        assert_eq!(models[0].tools, ToolSupport::Verified);
        assert_eq!(models[0].context_size, Some(65_536));
        assert_eq!(models[0].chat, None);
        // The definition now points at another file: the check no longer counts.
        let models = build(&[def("coder", &b, None)], |_| None, &loaded, &[]);
        assert_eq!(models[0].tools, ToolSupport::Unknown);
        assert!(models[0].check.is_none());
        // A template override makes the stored check stale too.
        let mut custom = def("coder", &a, None);
        custom.chat_template = Some("chatml".into());
        assert!(build(&[custom], |_| None, &loaded, &[])[0].check.is_none());
    }

    #[test]
    fn chat_filter_reads_python_engine_folders_and_skips_image_checkpoints() {
        // These have no GGUF header; the Coding model list offered all of them for chat.
        let dir = tempfile::tempdir().unwrap();
        let classifier = dir.path().join("toxic-bert");
        std::fs::create_dir_all(&classifier).unwrap();
        std::fs::write(
            classifier.join("config.json"),
            r#"{"architectures":["BertForSequenceClassification"],"id2label":{"0":"NEG","1":"POS"}}"#,
        )
        .unwrap();
        std::fs::write(classifier.join("model.safetensors"), "x").unwrap();
        let checkpoint = dir.path().join("sd-v1-5-inpainting.ckpt");
        std::fs::write(&checkpoint, "x").unwrap();
        let gguf = dir.path().join("chat.gguf");
        std::fs::write(&gguf, "x").unwrap();
        assert_eq!(
            chat_capability(&def("cls", &classifier, None), None),
            Some(false)
        );
        assert_eq!(
            chat_capability(&def("sd", &checkpoint, None), None),
            Some(false)
        );
        // An unreadable GGUF and a folder without a known task stay unknown and selectable.
        assert_eq!(chat_capability(&def("chat", &gguf, None), None), None);
        assert_eq!(chat_capability(&def("empty", dir.path(), None), None), None);
    }

    #[test]
    fn chat_filter_excludes_speech_and_non_chat_serving_modes() {
        let dir = tempfile::tempdir().unwrap();
        let speech = dir.path().join("renamed.model");
        std::fs::write(&speech, 0x6767_6d6cu32.to_le_bytes()).unwrap();
        let named_speech = dir.path().join("ggml-base.en.bin");
        let chat = dir.path().join("chat.gguf");
        let embed = dir.path().join("embed.gguf");
        let rerank = dir.path().join("rerank.gguf");
        for path in [&chat, &embed, &rerank] {
            std::fs::write(path, b"fixture").unwrap();
        }
        let models = build(
            &[
                def("speech", &speech, None),
                def("missing-speech", &named_speech, None),
                def("chat", &chat, None),
                def("embed", &embed, None),
                def("rerank", &rerank, None),
            ],
            |p| {
                Some(GgufMetadata {
                    architecture: Some("qwen3".into()),
                    has_chat_template: true,
                    pooling_type: if p == embed {
                        Some(1)
                    } else if p == rerank {
                        Some(4)
                    } else {
                        None
                    },
                    ..Default::default()
                })
            },
            &ToolChecks::default(),
            &[],
        );
        assert_eq!(
            models.iter().map(|m| m.alias.as_str()).collect::<Vec<_>>(),
            vec!["chat"]
        );
        for mode in ["embedding", "rerank", "transcription"] {
            let mut d = def("mode", Path::new("missing.gguf"), None);
            d.load
                .insert("serve_mode".into(), ParamValue::Text(mode.into()));
            assert_eq!(chat_capability(&d, None), Some(false));
        }
        let hf = ModelDefinition::new(
            "speech-hf",
            ModelRef::Hf {
                repo: "fixture".into(),
                file: "ggml-base.en.bin".into(),
            },
        );
        assert_eq!(chat_capability(&hf, None), Some(false));
    }

    #[test]
    fn gateway_log_signals_are_matched_by_alias() {
        let a = Path::new("missing.gguf");
        let signals = vec![AgentSignals {
            alias: "Qwen3".into(),
            native_tool_calls: 3,
            prompt_tps: Some(1200.0),
            prompt_tokens_timed: 5000,
            max_prompt_tokens: Some(4000),
            tool_ok: 9,
            tool_bad: 1,
        }];
        let m = build(
            &[def("qwen3", a, None)],
            |_| None,
            &ToolChecks::default(),
            &signals,
        );
        assert_eq!(m[0].tools, ToolSupport::Verified);
        assert_eq!(m[0].evidence, ToolEvidence::GatewayLog);
        assert_eq!(m[0].prompt_tps, Some(1200.0));
        // 9 of 10 tool rounds clean: native; the size is unknown, so the default cap.
        assert_eq!((m[0].tool_rounds_ok, m[0].tool_rounds_bad), (9, 1));
        assert_eq!(m[0].tool_level, Some(ToolLevel::Native));
        assert_eq!(m[0].recommended_tools, 8);
        // Without rounds there is no level.
        let m = build(
            &[def("qwen3", a, None)],
            |_| None,
            &ToolChecks::default(),
            &[],
        );
        assert_eq!(m[0].tool_level, None);
    }

    /// Real GGUF headers when the models are on this machine (skipped elsewhere): the chat
    /// models are listed with template tool handling, embedding/rerank models are not.
    #[test]
    fn real_models_when_present() {
        let root = Path::new(r"D:\Models");
        let chat =
            root.join(r"unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        let embed =
            root.join(r"nomic-ai\nomic-embed-text-v1.5-GGUF\nomic-embed-text-v1.5.Q8_0.gguf");
        if !chat.is_file() || !embed.is_file() {
            return;
        }
        let models = build(
            &[def("qwen3", &chat, None), def("nomic", &embed, None)],
            |p| crate::gguf::read_gguf_metadata(p).ok(),
            &ToolChecks::default(),
            &[],
        );
        assert_eq!(models.len(), 1, "{models:?}");
        assert_eq!(models[0].alias, "qwen3");
        assert_eq!(models[0].chat, Some(true));
        assert_eq!(models[0].template_tools, Some(true));
        assert_eq!(models[0].tools, ToolSupport::TemplateOnly);
        assert!(models[0].params.unwrap_or(0) > 3_000_000_000);
    }
}
