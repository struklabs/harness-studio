//! Wire types of the tool engine: definitions, invocations and results.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a tool may do to the user's machine. The class is fixed by the tool's author (the
/// registry entry), never by the model or by call arguments: nothing a model sends can make a
/// tool less restricted than it was registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PermissionClass {
    /// Pure computation (time, math, text transforms). Needs no consent.
    Safe,
    /// Network access and reading files: governed by the tool policy.
    Restricted,
    /// Writing files, running a shell or other processes, credentials: explicit user approval
    /// (or the user's standing "trusted" / "allow all" choice).
    Dangerous,
}

impl PermissionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionClass::Safe => "safe",
            PermissionClass::Restricted => "restricted",
            PermissionClass::Dangerous => "dangerous",
        }
    }
}

/// Where a tool comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ToolSource {
    /// Shipped in Harness itself.
    Builtin,
    /// Served by an MCP server.
    Mcp,
    /// Installed by the user.
    Installed,
    /// Written by a model and kept by the user.
    Generated,
}

impl ToolSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolSource::Builtin => "builtin",
            ToolSource::Mcp => "mcp",
            ToolSource::Installed => "installed",
            ToolSource::Generated => "generated",
        }
    }
}

/// A tool as the model and the clients see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema (`type: object`) of the arguments: the `parameters` of an OpenAI function.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub input_schema: Value,
    pub class: PermissionClass,
    pub source: ToolSource,
    /// The tool works inside a working directory (file tools, shell).
    pub uses_workspace: bool,
}

impl ToolDef {
    /// The OpenAI chat-completions form: `{"type":"function","function":{name,description,parameters}}`.
    pub fn to_openai(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.input_schema,
            }
        })
    }
}

/// Who asked for a call: written to the tool log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolOrigin {
    /// The client: `playground`, `cli`, `api`, a token name...
    pub client: String,
    pub conversation: Option<String>,
    /// The model that wrote the call (alias), when a model did.
    pub model: Option<String>,
}

impl ToolOrigin {
    pub fn new(client: impl Into<String>) -> Self {
        Self {
            client: client.into(),
            conversation: None,
            model: None,
        }
    }

    pub fn conversation(mut self, id: impl Into<String>) -> Self {
        self.conversation = Some(id.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// One call of one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolInvocation {
    /// Call id (the model's `tool_call_id` when it has one, otherwise generated).
    pub id: String,
    pub tool: String,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub args: Value,
    pub origin: ToolOrigin,
    pub started_at_ms: u64,
}

impl ToolInvocation {
    pub fn new(tool: impl Into<String>, args: Value, origin: ToolOrigin) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            args,
            origin,
            started_at_ms: crate::analytics::now_ms(),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
}

/// Why a call did not produce a normal result. The text of the result says why in words (it
/// is what the model reads); the code is for programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ToolErrorCode {
    UnknownTool,
    /// The arguments do not fit the tool's schema.
    InvalidArguments,
    /// The tool policy is "off".
    PolicyOff,
    /// The policy asks for the user's approval and none was given.
    ApprovalRequired,
    /// A file tool was called without a working directory.
    NoWorkspace,
    /// The path leaves the working directory (`..`, absolute path, symlink).
    OutsideWorkspace,
    NotFound,
    /// The operating system refused (permissions, disk).
    Io,
    /// The tool ran and reported a failure (no match for an edit, bad regex...).
    Failed,
    Timeout,
    Cancelled,
    NotConfigured,
    Network,
    ResponseTooLarge,
    UnsupportedContentType,
}

impl ToolErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolErrorCode::UnknownTool => "unknown_tool",
            ToolErrorCode::InvalidArguments => "invalid_arguments",
            ToolErrorCode::PolicyOff => "policy_off",
            ToolErrorCode::ApprovalRequired => "approval_required",
            ToolErrorCode::NoWorkspace => "no_workspace",
            ToolErrorCode::OutsideWorkspace => "outside_workspace",
            ToolErrorCode::NotFound => "not_found",
            ToolErrorCode::Io => "io",
            ToolErrorCode::Failed => "failed",
            ToolErrorCode::Timeout => "timeout",
            ToolErrorCode::Cancelled => "cancelled",
            ToolErrorCode::NotConfigured => "not_configured",
            ToolErrorCode::Network => "network",
            ToolErrorCode::ResponseTooLarge => "response_too_large",
            ToolErrorCode::UnsupportedContentType => "unsupported_content_type",
        }
    }
}

/// A tool's failure: a code and the sentence the model reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub code: ToolErrorCode,
    pub message: String,
    pub network_target: Option<String>,
}

impl ToolError {
    pub fn new(code: ToolErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            network_target: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(ToolErrorCode::Failed, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ToolErrorCode::InvalidArguments, message)
    }

    pub fn network_target(mut self, target: impl Into<String>) -> Self {
        self.network_target = Some(target.into());
        self
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a successful tool run returns before the registry applies its limits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutput {
    pub content: String,
    /// Host (and port) a network tool contacted, for the tool log.
    pub network_target: Option<String>,
}

impl ToolOutput {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            network_target: None,
        }
    }
}

/// The result of one call, as the registry hands it back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolResult {
    /// The text the model reads (the error sentence when `error` is set).
    pub content: String,
    pub error: Option<ToolErrorCode>,
    pub duration_ms: u64,
    /// `content` was cut at the result size limit.
    pub truncated: bool,
    pub network_target: Option<String>,
}

impl ToolResult {
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn failure(code: ToolErrorCode, message: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            content: message.into(),
            error: Some(code),
            duration_ms,
            truncated: false,
            network_target: None,
        }
    }
}

/// Result size limit in characters (the Playground's `TOOL_RESULT_MAX_CHARS`).
pub const RESULT_MAX_CHARS: usize = 16_000;

/// Cuts `text` to `max` characters; the cut is marked like the Playground always marked it:
/// `\n…[truncated N chars]`. Returns whether it cut.
pub fn truncate_chars(text: String, max: usize) -> (String, bool) {
    let total = text.chars().count();
    if total <= max {
        return (text, false);
    }
    let end = text
        .char_indices()
        .nth(max)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    (
        format!("{}\n…[truncated {} chars]", &text[..end], total - max),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_characters_and_marks_the_cut() {
        let (t, cut) = truncate_chars("abc".into(), 3);
        assert_eq!((t.as_str(), cut), ("abc", false));
        let (t, cut) = truncate_chars("çğıöşü-abcdef".into(), 6);
        assert!(cut);
        assert_eq!(t, "çğıöşü\n…[truncated 7 chars]");
        let long = "x".repeat(RESULT_MAX_CHARS + 5);
        let (t, cut) = truncate_chars(long, RESULT_MAX_CHARS);
        assert!(cut);
        assert!(t.ends_with("…[truncated 5 chars]"));
    }

    #[test]
    fn definitions_have_the_openai_shape() {
        let def = ToolDef {
            name: "t".into(),
            description: "d".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            class: PermissionClass::Safe,
            source: ToolSource::Builtin,
            uses_workspace: false,
        };
        let v = def.to_openai();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "t");
        assert_eq!(v["function"]["parameters"]["type"], "object");
        assert_eq!(
            serde_json::to_value(PermissionClass::Dangerous).unwrap(),
            "dangerous"
        );
    }
}
