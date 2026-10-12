//! The tool policy: how much the user lets tools do without asking.
//!
//! The policy and the trusted set are the user's settings (`settings.json` → `tools`). A model
//! cannot change them, and a call's arguments cannot change a tool's class, so a model can
//! never raise its own permission.

use serde::{Deserialize, Serialize};

use super::types::{PermissionClass, ToolDef, ToolSource};

/// `settings.tools.policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ToolPolicy {
    /// No tool runs.
    Off,
    /// Safe tools run; everything else asks first (the default).
    #[default]
    AskEveryTime,
    /// Safe tools and the tools in the trusted set run; everything else asks.
    AllowTrusted,
    /// Every tool runs without asking.
    AllowAll,
}

/// What to do with one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Decision {
    Run,
    /// Ask the user; run only when they approve.
    Ask,
    Refuse,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Run => "run",
            Decision::Ask => "ask",
            Decision::Refuse => "refuse",
        }
    }
}

/// The decision table. `trusted`: the tool is in the user's trusted set (it only counts under
/// [`ToolPolicy::AllowTrusted`]).
///
/// | policy         | safe   | restricted              | dangerous               |
/// |----------------|--------|-------------------------|-------------------------|
/// | off            | refuse | refuse                  | refuse                  |
/// | ask every time | run    | ask                     | ask                     |
/// | allow trusted  | run    | run if trusted, else ask | run if trusted, else ask |
/// | allow all      | run    | run                     | run                     |
pub fn decide(policy: ToolPolicy, class: PermissionClass, trusted: bool) -> Decision {
    match (policy, class) {
        (ToolPolicy::Off, _) => Decision::Refuse,
        (_, PermissionClass::Safe) => Decision::Run,
        (ToolPolicy::AllowAll, _) => Decision::Run,
        (ToolPolicy::AllowTrusted, _) if trusted => Decision::Run,
        (ToolPolicy::AllowTrusted | ToolPolicy::AskEveryTime, _) => Decision::Ask,
    }
}

/// `settings.tools`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolSettings {
    pub policy: ToolPolicy,
    /// Tools the user trusts (names), honoured under "allow trusted".
    pub trusted: Vec<String>,
    /// Working directory of file tools and the shell when the caller names none. Empty = none:
    /// such calls fail with `no_workspace` instead of guessing.
    pub workdir: String,
    /// Saved folders in most-recently-used order. None migrates legacy settings and
    /// preserves the list when an older client saves only the original fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,
    /// The user's permission class for an MCP tool (by its registry name, `<server>_<tool>`),
    /// replacing the class its annotations give it. Only MCP tools: a built-in tool's class is
    /// fixed in the code.
    ///
    /// `None` = not given: saving settings from a screen that does not know the table keeps
    /// the stored one (`save_settings`); `Some(empty)` clears it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<std::collections::BTreeMap<String, PermissionClass>>))]
    pub mcp_classes: Option<std::collections::BTreeMap<String, PermissionClass>>,
    /// Provider choice and non-secret endpoint for the provider-independent web_search tool.
    pub web_search: WebSearchSettings,
    /// Offer the built-in tools to MCP clients (`harness mcp`, `/mcp`) as well. Off by default
    /// so existing clients do not suddenly see file and shell tools. The policy above still
    /// decides each call; MCP cannot ask, so only calls the policy allows run.
    pub mcp_expose: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SearchProviderKind {
    None,
    #[default]
    Duckduckgo,
    Searxng,
    Brave,
    Tavily,
}

impl SearchProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Duckduckgo => "duckduckgo",
            Self::Searxng => "searxng",
            Self::Brave => "brave",
            Self::Tavily => "tavily",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WebSearchSettings {
    pub provider: SearchProviderKind,
    /// SearXNG base URL only. API keys are always held by the OS keychain.
    pub searxng_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WebSearchSettingsView {
    pub provider: SearchProviderKind,
    pub searxng_url: String,
    pub api_key_set: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetWebSearchSettings {
    pub provider: SearchProviderKind,
    #[serde(default)]
    pub searxng_url: String,
    /// Missing keeps the current key; empty removes it. Never serialized to settings.json.
    #[serde(default)]
    pub api_key: Option<String>,
}

pub fn web_search_settings() -> Result<WebSearchSettingsView, String> {
    let cfg = crate::config::AppSettings::load().tools.web_search;
    let api_key_set = matches!(
        cfg.provider,
        SearchProviderKind::Brave | SearchProviderKind::Tavily
    ) && crate::secrets::web_search_key_get(cfg.provider.as_str())?.is_some();
    Ok(WebSearchSettingsView {
        provider: cfg.provider,
        searxng_url: cfg.searxng_url,
        api_key_set,
    })
}

pub fn save_web_search_settings(
    req: SetWebSearchSettings,
) -> Result<WebSearchSettingsView, String> {
    let url = req.searxng_url.trim().trim_end_matches('/').to_string();
    if req.provider == SearchProviderKind::Searxng {
        if url.is_empty() {
            return Err("SearXNG base URL is required".into());
        }
        crate::network::parse_public_http_url(&url)?;
    }
    if matches!(
        req.provider,
        SearchProviderKind::Brave | SearchProviderKind::Tavily
    ) {
        if let Some(value) = req.api_key.as_deref() {
            crate::secrets::web_search_key_set(req.provider.as_str(), Some(value))?;
        }
    }
    let mut app = crate::config::AppSettings::load();
    app.tools.web_search = WebSearchSettings {
        provider: req.provider,
        searxng_url: url,
    };
    app.save()?;
    web_search_settings()
}

impl ToolSettings {
    /// The class `def` is judged by: the user's override for an MCP tool, otherwise its own.
    pub fn class_for(&self, def: &ToolDef) -> PermissionClass {
        if def.source == ToolSource::Mcp {
            self.mcp_classes
                .as_ref()
                .and_then(|m| m.get(&def.name))
                .copied()
                .unwrap_or(def.class)
        } else {
            def.class
        }
    }

    /// `def` with the class the user's settings give it.
    pub fn effective(&self, def: &ToolDef) -> ToolDef {
        ToolDef {
            class: self.class_for(def),
            ..def.clone()
        }
    }

    pub fn is_trusted(&self, tool: &str) -> bool {
        self.trusted.iter().any(|t| t == tool)
    }

    pub fn decide(&self, tool: &str, class: PermissionClass) -> Decision {
        decide(self.policy, class, self.is_trusted(tool))
    }

    /// Trims, drops empty names and duplicates (a settings editor's input).
    pub fn normalized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.trusted = self
            .trusted
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && seen.insert(t.clone()))
            .collect();
        self.workdir = self.workdir.trim().to_string();
        let mut folders = self.workspaces.take().unwrap_or_default();
        if !self.workdir.is_empty() && !folders.contains(&self.workdir) {
            folders.push(self.workdir.clone());
        }
        let mut seen = std::collections::HashSet::new();
        self.workspaces = Some(
            folders
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty() && seen.insert(workspace_key(p)))
                .collect(),
        );
        if let Some(m) = &mut self.mcp_classes {
            m.retain(|k, _| !k.trim().is_empty());
        }
        self.web_search.searxng_url = self
            .web_search
            .searxng_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        self
    }
}

/// Match Windows drive/UNC paths by case and separator, keeping stored display spelling.
/// POSIX paths remain case-sensitive even when tested on Windows.
fn workspace_key(path: &str) -> String {
    let bytes = path.as_bytes();
    let windows = (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || path.starts_with("\\\\")
        || path.starts_with("//");
    if windows {
        path.replace('\\', "/").trim_end_matches('/').to_lowercase()
    } else {
        let key = path.trim_end_matches('/');
        if key.is_empty() {
            "/".into()
        } else {
            key.into()
        }
    }
}

/// Saves `settings.tools` (the one way the policy changes: the user's Settings, the Control
/// API's `PUT /tools/policy` for an admin token). A non-empty working directory must exist.
pub fn save_settings(settings: ToolSettings) -> Result<ToolSettings, String> {
    let mut app = crate::config::AppSettings::load();
    let mut settings = settings;
    if settings.workspaces.is_none() {
        settings.workspaces = app.tools.workspaces.clone();
    }
    let mut settings = settings.normalized();
    if settings.workdir != app.tools.workdir
        && !settings.workdir.is_empty()
        && !std::path::Path::new(&settings.workdir).is_dir()
    {
        return Err(crate::error_key::err("tools.workdir_invalid")
            .with("path", &settings.workdir)
            .into());
    }
    if settings.mcp_classes.is_none() {
        settings.mcp_classes = app.tools.mcp_classes.clone();
    }
    // Provider/key changes have their own explicit route. Preserve them when an older client
    // PUTs only the T1 policy fields (missing serde fields default instead of meaning "clear").
    settings.web_search = app.tools.web_search.clone();
    app.tools = settings.clone();
    app.save()?;
    Ok(settings)
}

/// Sets (or, with `None`, removes) the user's permission class of one MCP tool, leaving every
/// other tool setting as it is.
pub fn set_mcp_class(tool: &str, class: Option<PermissionClass>) -> Result<ToolSettings, String> {
    let mut app = crate::config::AppSettings::load();
    let map = app.tools.mcp_classes.get_or_insert_with(Default::default);
    match class {
        Some(c) => map.insert(tool.to_string(), c),
        None => map.remove(tool),
    };
    let saved = app.tools.clone();
    app.save()?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Decision::*;
    use PermissionClass::*;
    use ToolPolicy::*;

    #[test]
    fn legacy_workdir_migrates_without_losing_missing_folders() {
        let old: ToolSettings = serde_json::from_str(r#"{"workdir":" C:/missing "}"#).unwrap();
        let migrated = old.normalized();
        assert_eq!(migrated.workdir, "C:/missing");
        assert_eq!(migrated.workspaces, Some(vec!["C:/missing".into()]));
        let saved = serde_json::to_string(&migrated).unwrap();
        let reloaded: ToolSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(reloaded.normalized(), migrated);
        assert_eq!(
            ToolSettings::default().normalized().workspaces,
            Some(vec![])
        );
    }

    #[test]
    fn windows_workspace_duplicates_keep_first_spelling() {
        let settings = ToolSettings {
            workdir: "c:/PROJECT/".into(),
            workspaces: Some(vec![
                "C:\\Project".into(),
                "c:/project/".into(),
                "\\\\HOST\\Share\\".into(),
                "//host/share/".into(),
                "/home/A".into(),
                "/home/a/".into(),
            ]),
            ..Default::default()
        }
        .normalized();
        assert_eq!(
            settings.workspaces.unwrap(),
            vec!["C:\\Project", "\\\\HOST\\Share\\", "/home/A", "/home/a/"]
        );
    }

    #[test]
    fn workspace_order_and_explicit_empty_list_survive_normalization() {
        let settings = ToolSettings {
            workdir: "C:/default".into(),
            workspaces: Some(vec![" C:/recent ".into(), "".into(), "C:/recent".into()]),
            ..Default::default()
        }
        .normalized();
        assert_eq!(
            settings.workspaces,
            Some(vec!["C:/recent".into(), "C:/default".into()])
        );
        let cleared = ToolSettings {
            workspaces: Some(vec![]),
            ..Default::default()
        }
        .normalized();
        assert_eq!(cleared.workspaces, Some(vec![]));
    }

    #[test]
    fn search_provider_defaults_and_explicit_choices_round_trip() {
        for input in ["{}", r#"{"web_search":{}}"#] {
            let settings: ToolSettings = serde_json::from_str(input).unwrap();
            assert_eq!(settings.web_search.provider, SearchProviderKind::Duckduckgo);
        }
        for provider in [
            SearchProviderKind::None,
            SearchProviderKind::Duckduckgo,
            SearchProviderKind::Searxng,
            SearchProviderKind::Brave,
            SearchProviderKind::Tavily,
        ] {
            let input = serde_json::json!({"web_search": {"provider": provider.as_str()}});
            let settings: ToolSettings = serde_json::from_value(input).unwrap();
            assert_eq!(settings.web_search.provider, provider);
            let saved = serde_json::to_value(&settings).unwrap();
            assert_eq!(saved["web_search"]["provider"], provider.as_str());
            let loaded: ToolSettings = serde_json::from_value(saved).unwrap();
            assert_eq!(loaded, settings);
        }
    }

    #[test]
    fn decision_table() {
        // (policy, class, trusted) -> decision
        let table = [
            (Off, Safe, false, Refuse),
            (Off, Restricted, true, Refuse),
            (Off, Dangerous, true, Refuse),
            (AskEveryTime, Safe, false, Run),
            (AskEveryTime, Restricted, false, Ask),
            (AskEveryTime, Dangerous, false, Ask),
            // The trusted set only counts under "allow trusted".
            (AskEveryTime, Dangerous, true, Ask),
            (AllowTrusted, Safe, false, Run),
            (AllowTrusted, Restricted, false, Ask),
            (AllowTrusted, Restricted, true, Run),
            (AllowTrusted, Dangerous, false, Ask),
            (AllowTrusted, Dangerous, true, Run),
            (AllowAll, Safe, false, Run),
            (AllowAll, Restricted, false, Run),
            (AllowAll, Dangerous, false, Run),
        ];
        for (policy, class, trusted, want) in table {
            assert_eq!(
                decide(policy, class, trusted),
                want,
                "{policy:?} {class:?} trusted={trusted}"
            );
        }
    }

    #[test]
    fn settings_load_from_old_files_and_default_to_asking() {
        let s: ToolSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.policy, AskEveryTime);
        assert!(s.trusted.is_empty() && s.workdir.is_empty());
        let s: ToolSettings =
            serde_json::from_str(r#"{"policy":"allow_trusted","trusted":["read_file"]}"#).unwrap();
        assert_eq!(s.decide("read_file", Dangerous), Run);
        assert_eq!(s.decide("write_file", Dangerous), Ask);
        let n = ToolSettings {
            trusted: vec![" a ".into(), "a".into(), "".into(), "b".into()],
            workdir: "  C:/w ".into(),
            web_search: WebSearchSettings::default(),
            ..Default::default()
        }
        .normalized();
        assert_eq!(n.trusted, ["a", "b"]);
        assert_eq!(n.workdir, "C:/w");
    }

    #[test]
    fn a_users_class_for_an_mcp_tool_replaces_the_annotation_class() {
        let mcp = ToolDef {
            name: "srv_t".into(),
            description: String::new(),
            input_schema: serde_json::json!({}),
            class: Dangerous,
            source: ToolSource::Mcp,
            uses_workspace: false,
        };
        let builtin = ToolDef {
            name: "read_file".into(),
            source: ToolSource::Builtin,
            class: Restricted,
            ..mcp.clone()
        };
        let mut s = ToolSettings::default();
        assert_eq!(s.class_for(&mcp), Dangerous);
        let table = s.mcp_classes.get_or_insert_with(Default::default);
        table.insert("srv_t".into(), Restricted);
        table.insert("read_file".into(), Safe);
        assert_eq!(s.class_for(&mcp), Restricted);
        assert_eq!(s.effective(&mcp).class, Restricted);
        // The override table never touches a built-in tool.
        assert_eq!(s.class_for(&builtin), Restricted);
        // Old settings files have no table.
        let old: ToolSettings = serde_json::from_str(r#"{"policy":"allow_all"}"#).unwrap();
        assert!(old.mcp_classes.is_none());
    }
}
