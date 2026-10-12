//! Run state of model definitions across server instances (Playground model picker, future
//! CLI `harness ps`).

use super::ModelDefinition;
use crate::process::{LaunchConfig, ServerStatus};
use serde::{Deserialize, Serialize};

/// Where a definition stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RunState {
    /// Base file not on disk (or a `hf:` reference not downloaded yet).
    Missing,
    /// Downloaded, not loaded by any server.
    Idle,
    /// A server is starting with it.
    Loading,
    /// A server runs it.
    Running,
}

/// One server instance running (or starting) a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DefinitionServer {
    pub server_id: String,
    pub status: ServerStatus,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DefinitionState {
    pub definition_id: String,
    pub alias: String,
    pub state: RunState,
    pub servers: Vec<DefinitionServer>,
}

/// A server instance as seen by [`definition_states`]: id, status and its current (or last)
/// launch config.
pub type ServerView = (String, ServerStatus, Option<LaunchConfig>);

/// Whether `config` serves `def`: by the definition id the launch was resolved from, else
/// (launches from before definitions) by file and served name.
fn serves(def: &ModelDefinition, config: &LaunchConfig, server_id: &str) -> bool {
    // A router serves the transient definitions the gateway builds for its models
    // (`gateway::scheduler::router_models`), never a stored one.
    if config.router_mode {
        return crate::gateway::scheduler::router_def_server(&def.id) == Some(server_id);
    }
    if let Some(id) = config.definition_id.as_deref() {
        return id == def.id;
    }
    let same_file = def.base.local_path() == Some(config.model_path.as_path());
    let served = config
        .alias
        .clone()
        .or_else(|| crate::process::model_alias(&config.model_path));
    same_file && served.is_some_and(|a| a.eq_ignore_ascii_case(&def.alias))
}

pub fn definition_states(defs: &[ModelDefinition], servers: &[ServerView]) -> Vec<DefinitionState> {
    defs.iter()
        .map(|def| {
            let running: Vec<DefinitionServer> = servers
                .iter()
                .filter(|(_, status, _)| {
                    matches!(status, ServerStatus::Starting | ServerStatus::Running)
                })
                .filter_map(|(id, status, config)| {
                    let c = config.as_ref()?;
                    serves(def, c, id).then(|| DefinitionServer {
                        server_id: id.clone(),
                        status: status.clone(),
                        port: c.port,
                    })
                })
                .collect();
            let state = if running.iter().any(|s| s.status == ServerStatus::Running) {
                RunState::Running
            } else if !running.is_empty() {
                RunState::Loading
            } else if def
                .base
                .local_path()
                .is_some_and(crate::engine::model_on_disk)
            {
                RunState::Idle
            } else {
                RunState::Missing
            };
            DefinitionState {
                definition_id: def.id.clone(),
                alias: def.alias.clone(),
                state,
                servers: running,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ModelRef;

    #[test]
    fn states_follow_servers_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Qwen3-4B-Q4_K_M.gguf");
        std::fs::write(&file, b"GGUF").unwrap();
        let mut a = ModelDefinition::new("qwen-coder", ModelRef::Local { path: file.clone() });
        a.id = "md-a".into();
        let mut b = ModelDefinition::new("Qwen3-4B-Q4_K_M", ModelRef::Local { path: file.clone() });
        b.id = "md-b".into();
        let mut gone = ModelDefinition::new("gone", ModelRef::parse("hf:x/y:z.gguf"));
        gone.id = "md-c".into();
        let cfg = |def: Option<&str>| LaunchConfig {
            model_path: file.clone(),
            port: 18100,
            definition_id: def.map(str::to_string),
            ..Default::default()
        };
        let servers: Vec<ServerView> = vec![
            (
                "default".into(),
                ServerStatus::Running,
                Some(cfg(Some("md-a"))),
            ),
            // An old launch without a definition id: matched by file + served name (stem).
            ("old".into(), ServerStatus::Starting, Some(cfg(None))),
            (
                "stopped".into(),
                ServerStatus::Stopped,
                Some(cfg(Some("md-b"))),
            ),
        ];
        let states = definition_states(&[a, b, gone], &servers);
        assert_eq!(states[0].state, RunState::Running);
        assert_eq!(states[0].servers[0].server_id, "default");
        assert_eq!(states[1].state, RunState::Loading);
        assert_eq!(states[1].servers.len(), 1);
        assert_eq!(states[2].state, RunState::Missing);
        std::fs::remove_file(&file).unwrap();
        let idle = definition_states(&[states_def("md-a", &file)], &[]);
        assert_eq!(idle[0].state, RunState::Missing, "file removed");
    }

    fn states_def(id: &str, path: &std::path::Path) -> ModelDefinition {
        let mut d = ModelDefinition::new("x", ModelRef::Local { path: path.into() });
        d.id = id.into();
        d
    }
}
