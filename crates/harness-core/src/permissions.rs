//! Trust levels and capability grants for the subjects that act on the user's behalf: MCP
//! servers, agent runtimes (Hermes, OpenClaw) and the training backend.
//!
//! This extends the tool policy rather than replacing it:
//!
//! * The **trust level** is the tool engine's [`PermissionClass`] (`safe` / `restricted` /
//!   `dangerous`), judged by the same [`ToolPolicy`](crate::tools::ToolPolicy) decision table.
//!   For an MCP server an explicit level replaces the class its tools get from their
//!   annotations (a tool the user classed by hand in `mcp_classes` keeps that class). For an
//!   agent runtime the level is a ceiling on what the adapter may be handed (`safe` = inference
//!   only: no folder is readable). Nothing is stored until the user sets a level, so existing
//!   behaviour is unchanged by default.
//! * **Grants** are per-capability (`file` with folders, `network`, `shell`, `service`,
//!   `account`, `device`), stored in `permissions.json` in the data directory. The default is
//!   no grant. For MCP servers a grant marks the server as trusted for the "allow trusted"
//!   policy; for agent runtimes the `file` grant folders are the only folders a run may read.
//!   The training backend is modelled but has no runtime: nothing can be granted to it.
//! * Every grant, revoke and level change is a row in the existing tool log
//!   (`analytics.db` → `tool_calls`, tool `permissions.*`, source `permissions`). A grant is
//!   only saved once its audit row is written; a revoke is never blocked by a failing log.
//!
//! The settings are the user's. A model cannot change them: nothing here is reachable from a
//! tool call.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::analytics::{AnalyticsDb, ToolCallRow};
use crate::error_key::err;
use crate::store::JsonStore;
use crate::tools::PermissionClass;

pub const AGENT_HERMES: &str = "hermes";
pub const AGENT_OPENCLAW: &str = "openclaw";
pub const TRAINER_DEFAULT: &str = "training";
/// Most folders one `file` grant may name (the agent adapter's own limit).
pub const MAX_GRANT_PATHS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SubjectKind {
    McpServer,
    Agent,
    Trainer,
}

impl SubjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::McpServer => "mcp_server",
            Self::Agent => "agent",
            Self::Trainer => "trainer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Capability {
    File,
    Network,
    Shell,
    Service,
    Account,
    Device,
}

impl Capability {
    pub const ALL: [Capability; 6] = [
        Self::File,
        Self::Network,
        Self::Shell,
        Self::Service,
        Self::Account,
        Self::Device,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Network => "network",
            Self::Shell => "shell",
            Self::Service => "service",
            Self::Account => "account",
            Self::Device => "device",
        }
    }

    /// The class of what the capability lets a subject do (for the audit row).
    pub fn class(self) -> PermissionClass {
        match self {
            Self::File | Self::Network => PermissionClass::Restricted,
            _ => PermissionClass::Dangerous,
        }
    }
}

/// One granted capability. `paths` are canonical folders and only used by `file`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Grant {
    pub id: String,
    pub capability: Capability,
    #[serde(default)]
    pub paths: Vec<String>,
    pub granted_at_ms: u64,
}

/// What is stored for one subject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectRecord {
    pub kind: SubjectKind,
    pub id: String,
    /// The user's trust level; `None` = the default (an MCP tool's own class; for an agent,
    /// no ceiling beyond the grants).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<PermissionClass>,
    #[serde(default)]
    pub grants: Vec<Grant>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PermissionsFile {
    subjects: Vec<SubjectRecord>,
}

/// A subject as the Permissions page shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubjectView {
    pub kind: SubjectKind,
    pub id: String,
    /// Display name.
    pub label: String,
    /// `false`: modelled, but nothing runs yet (the training backend, OpenClaw) or the
    /// configured server was removed (only its stored grants remain, to be revoked).
    pub available: bool,
    /// The user's level; `None` = default.
    pub trust: Option<PermissionClass>,
    /// The class the subject has by itself, when the user sets nothing: stdio MCP servers and
    /// agent runtimes run with the user's authority (`dangerous`), HTTP MCP servers send data
    /// out (`restricted`).
    pub inherent: PermissionClass,
    /// Capabilities that can be granted to this subject (empty: none).
    pub grantable: Vec<Capability>,
    pub grants: Vec<Grant>,
}

/// Read-only copy of the stored permissions, for the enforcement points.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    subjects: Vec<SubjectRecord>,
}

impl Snapshot {
    fn find(&self, kind: SubjectKind, id: &str) -> Option<&SubjectRecord> {
        self.subjects.iter().find(|s| s.kind == kind && s.id == id)
    }

    /// The class and trusted flag of one MCP tool of `server`. `class` is the tool's own (or
    /// the user's per-tool) class; `by_hand` = the user classed this very tool in
    /// `mcp_classes`, which wins over the server's level.
    pub fn judge_mcp(
        &self,
        server: &str,
        class: PermissionClass,
        by_hand: bool,
    ) -> (PermissionClass, bool) {
        match self.find(SubjectKind::McpServer, server) {
            None => (class, false),
            Some(s) => (
                if by_hand {
                    class
                } else {
                    s.trust.unwrap_or(class)
                },
                !s.grants.is_empty(),
            ),
        }
    }

    /// `Err` when an agent run may not read `folders` (canonical paths).
    pub fn check_agent_folders(&self, agent: &str, folders: &[PathBuf]) -> Result<(), String> {
        if folders.is_empty() {
            return Ok(());
        }
        let Some(rec) = self.find(SubjectKind::Agent, agent) else {
            return Err(err("permissions.folder_not_granted")
                .with("path", folders[0].display())
                .into());
        };
        if rec.trust == Some(PermissionClass::Safe) {
            return Err(err("permissions.trust_too_low").with("agent", agent).into());
        }
        for folder in folders {
            let covered = rec
                .grants
                .iter()
                .filter(|g| g.capability == Capability::File)
                .flat_map(|g| g.paths.iter())
                .any(|root| path_within(folder, Path::new(root)));
            if !covered {
                return Err(err("permissions.folder_not_granted")
                    .with("path", folder.display())
                    .into());
            }
        }
        Ok(())
    }
}

fn path_key(p: &Path) -> Vec<String> {
    p.components()
        .map(|c| {
            let s = c.as_os_str().to_string_lossy().into_owned();
            if cfg!(windows) {
                s.to_lowercase()
            } else {
                s
            }
        })
        .collect()
}

/// `child` is `root` or below it (canonical paths; case-insensitive on Windows).
pub fn path_within(child: &Path, root: &Path) -> bool {
    let (c, r) = (path_key(child), path_key(root));
    c.len() >= r.len() && c[..r.len()] == r[..]
}

/// Where the audit rows go.
#[derive(Clone)]
pub enum AuditSink {
    /// The process-wide `analytics.db`.
    Shared,
    /// A given database (tests).
    Db(Arc<AnalyticsDb>),
}

pub struct Permissions {
    store: JsonStore,
    sink: AuditSink,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

impl Permissions {
    /// The user's permissions: `permissions.json` in the data directory.
    pub fn shared() -> Self {
        Self::at(
            crate::paths::data_root().join("permissions.json"),
            AuditSink::Shared,
        )
    }

    pub fn at(path: impl Into<PathBuf>, sink: AuditSink) -> Self {
        Self {
            store: JsonStore::sensitive(path.into()),
            sink,
        }
    }

    /// What is stored. A missing or unreadable file means no grants: the enforcement points
    /// then fail closed.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            subjects: self.store.load_or_default::<PermissionsFile>().subjects,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn audit(
        &self,
        client: &str,
        action: &str,
        kind: SubjectKind,
        id: &str,
        class: PermissionClass,
        detail: serde_json::Value,
        error: Option<&str>,
    ) -> Result<(), String> {
        let mut args = json!({"subject": id, "kind": kind.as_str()});
        if let (Some(a), Some(d)) = (args.as_object_mut(), detail.as_object()) {
            a.extend(d.clone());
        }
        let at = now_ms();
        let row = ToolCallRow {
            id: uuid::Uuid::new_v4().to_string(),
            started_at_ms: at,
            client: client.to_string(),
            conversation: None,
            model: None,
            tool: format!("permissions.{action}"),
            class: class.as_str().to_string(),
            source: "permissions".into(),
            args: crate::tools::redact_args(&args),
            approved: error.is_none(),
            error_code: error.map(str::to_string),
            duration_ms: 0,
            result_chars: 0,
            truncated: false,
            network_target: None,
        };
        let db = match &self.sink {
            AuditSink::Db(db) => db.clone(),
            AuditSink::Shared => crate::analytics::shared()?,
        };
        db.insert_tool_call(&row)
    }

    /// The subjects with their level and grants. `mcp`: the configured MCP servers as
    /// `(name, is_stdio)`; servers that are gone but still hold grants stay listed.
    pub fn list_with(&self, mcp: &[(String, bool)]) -> Vec<SubjectView> {
        let snap = self.snapshot();
        let view =
            |kind, id: &str, label: &str, available, inherent, grantable: Vec<Capability>| {
                let rec = snap.find(kind, id);
                SubjectView {
                    kind,
                    id: id.to_string(),
                    label: label.to_string(),
                    available,
                    trust: rec.and_then(|r| r.trust),
                    inherent,
                    grantable,
                    grants: rec.map(|r| r.grants.clone()).unwrap_or_default(),
                }
            };
        let mut out = Vec::new();
        for (name, stdio) in mcp {
            out.push(view(
                SubjectKind::McpServer,
                name,
                name,
                true,
                if *stdio {
                    PermissionClass::Dangerous
                } else {
                    PermissionClass::Restricted
                },
                Capability::ALL.to_vec(),
            ));
        }
        for rec in snap
            .subjects
            .iter()
            .filter(|s| s.kind == SubjectKind::McpServer)
            .filter(|s| !mcp.iter().any(|(n, _)| *n == s.id))
        {
            out.push(view(
                SubjectKind::McpServer,
                &rec.id,
                &rec.id,
                false,
                PermissionClass::Dangerous,
                Capability::ALL.to_vec(),
            ));
        }
        out.push(view(
            SubjectKind::Agent,
            AGENT_HERMES,
            "Hermes Agent",
            true,
            PermissionClass::Dangerous,
            vec![Capability::File],
        ));
        out.push(view(
            SubjectKind::Agent,
            AGENT_OPENCLAW,
            "OpenClaw",
            true,
            PermissionClass::Dangerous,
            vec![Capability::File],
        ));
        out.push(view(
            SubjectKind::Trainer,
            TRAINER_DEFAULT,
            "Training Backend",
            false,
            PermissionClass::Dangerous,
            vec![],
        ));
        out
    }

    /// [`Self::list_with`] over the MCP servers of `mcp_servers.json`.
    pub fn list(&self) -> Vec<SubjectView> {
        let servers: Vec<(String, bool)> = crate::mcp::load()
            .map(|c| {
                c.mcp_servers
                    .into_iter()
                    .map(|(n, d)| (n, d.command.is_some()))
                    .collect()
            })
            .unwrap_or_default();
        self.list_with(&servers)
    }

    fn grantable(kind: SubjectKind, id: &str) -> Result<Vec<Capability>, String> {
        match kind {
            SubjectKind::McpServer if valid_id(id) => Ok(Capability::ALL.to_vec()),
            SubjectKind::Agent if id == AGENT_HERMES || id == AGENT_OPENCLAW => {
                Ok(vec![Capability::File])
            }
            SubjectKind::Trainer if id == TRAINER_DEFAULT => Ok(vec![]),
            _ => Err(err("permissions.subject_not_found").with("id", id).into()),
        }
    }

    /// Folders of a `file` grant: absolute, existing, not a drive/filesystem root, stored
    /// canonical so a later run compares like with like.
    fn canonical_folders(paths: &[String]) -> Result<Vec<String>, String> {
        if paths.is_empty() || paths.len() > MAX_GRANT_PATHS {
            return Err(err("permissions.paths_invalid").into());
        }
        let mut out: Vec<String> = Vec::new();
        for raw in paths {
            let p = PathBuf::from(raw.trim());
            if !p.is_absolute() || !p.is_dir() {
                return Err(err("permissions.paths_invalid").with("path", raw).into());
            }
            let canon = p
                .canonicalize()
                .map_err(|_| err("permissions.paths_invalid").with("path", raw))?;
            if canon.parent().is_none() {
                return Err(err("permissions.paths_invalid").with("path", raw).into());
            }
            let s = canon.to_string_lossy().into_owned();
            if !out.contains(&s) {
                out.push(s);
            }
        }
        Ok(out)
    }

    /// Grants `capability` to a subject. `client`: who asked (`ui`, `cli`, `api`).
    pub fn grant(
        &self,
        client: &str,
        kind: SubjectKind,
        id: &str,
        capability: Capability,
        paths: Vec<String>,
    ) -> Result<Grant, String> {
        let refuse = |code: &'static str| -> String {
            let _ = self.audit(
                client,
                "grant",
                kind,
                id,
                capability.class(),
                json!({"capability": capability.as_str()}),
                Some(code),
            );
            err(code)
                .with("id", id)
                .with("capability", capability.as_str())
                .into()
        };
        let allowed = Self::grantable(kind, id)?;
        if kind == SubjectKind::Trainer {
            return Err(refuse("permissions.not_available"));
        }
        if !allowed.contains(&capability) {
            return Err(refuse("permissions.capability_unsupported"));
        }
        let paths = if capability == Capability::File {
            Self::canonical_folders(&paths)?
        } else if paths.is_empty() {
            Vec::new()
        } else {
            return Err(err("permissions.paths_invalid").into());
        };
        let grant = Grant {
            id: uuid::Uuid::new_v4().to_string(),
            capability,
            paths,
            granted_at_ms: now_ms(),
        };
        self.store.update::<PermissionsFile, _>(|file| {
            let idx = match file
                .subjects
                .iter()
                .position(|s| s.kind == kind && s.id == id)
            {
                Some(i) => i,
                None => {
                    file.subjects.push(SubjectRecord {
                        kind,
                        id: id.to_string(),
                        trust: None,
                        grants: vec![],
                    });
                    file.subjects.len() - 1
                }
            };
            let rec = &mut file.subjects[idx];
            // The same grant twice is one grant.
            if let Some(same) = rec
                .grants
                .iter()
                .find(|g| g.capability == capability && g.paths == grant.paths)
            {
                return Ok(same.clone());
            }
            // The audit row comes first: nothing is granted that was not recorded.
            self.audit(
                client,
                "grant",
                kind,
                id,
                capability.class(),
                json!({"grant": grant.id, "capability": capability.as_str(), "paths": grant.paths}),
                None,
            )?;
            rec.grants.push(grant.clone());
            Ok(grant)
        })
    }

    /// Removes one grant. Taking effect on the next run or call; a run in progress is not
    /// stopped.
    pub fn revoke_grant(
        &self,
        client: &str,
        kind: SubjectKind,
        id: &str,
        grant_id: &str,
    ) -> Result<(), String> {
        let removed = self.store.update::<PermissionsFile, _>(|file| {
            let Some(rec) = file
                .subjects
                .iter_mut()
                .find(|s| s.kind == kind && s.id == id)
            else {
                return Ok(None);
            };
            let Some(pos) = rec.grants.iter().position(|g| g.id == grant_id) else {
                return Ok(None);
            };
            let g = rec.grants.remove(pos);
            file.subjects.retain(|s| {
                !(s.kind == kind && s.id == id && s.trust.is_none() && s.grants.is_empty())
            });
            Ok(Some(g))
        })?;
        let Some(g) = removed else {
            return Err(err("permissions.grant_not_found")
                .with("id", grant_id)
                .into());
        };
        let _ = self.audit(
            client,
            "revoke_grant",
            kind,
            id,
            g.capability.class(),
            json!({"grant": g.id, "capability": g.capability.as_str(), "paths": g.paths}),
            None,
        );
        Ok(())
    }

    /// Removes every grant and the level of a subject (back to the defaults).
    pub fn revoke_subject(&self, client: &str, kind: SubjectKind, id: &str) -> Result<u32, String> {
        let count = self.store.update::<PermissionsFile, _>(|file| {
            let n = file
                .subjects
                .iter()
                .filter(|s| s.kind == kind && s.id == id)
                .map(|s| s.grants.len() as u32 + u32::from(s.trust.is_some()))
                .sum();
            file.subjects.retain(|s| !(s.kind == kind && s.id == id));
            Ok(n)
        })?;
        let _ = self.audit(
            client,
            "revoke_subject",
            kind,
            id,
            PermissionClass::Dangerous,
            json!({"removed": count}),
            None,
        );
        Ok(count)
    }

    /// Sets (or with `None` clears) the trust level of a subject.
    pub fn set_trust(
        &self,
        client: &str,
        kind: SubjectKind,
        id: &str,
        level: Option<PermissionClass>,
    ) -> Result<(), String> {
        Self::grantable(kind, id)?;
        if kind == SubjectKind::Trainer {
            let _ = self.audit(
                client,
                "set_trust",
                kind,
                id,
                PermissionClass::Dangerous,
                json!({"trust": level.map(PermissionClass::as_str)}),
                Some("permissions.not_available"),
            );
            return Err(err("permissions.not_available").with("id", id).into());
        }
        self.store.update::<PermissionsFile, _>(|file| {
            let idx = file
                .subjects
                .iter()
                .position(|s| s.kind == kind && s.id == id);
            let was = idx.and_then(|i| file.subjects[i].trust);
            if level.is_some() {
                if idx.is_none() {
                    file.subjects.push(SubjectRecord {
                        kind,
                        id: id.to_string(),
                        trust: None,
                        grants: vec![],
                    });
                }
                let rec = file
                    .subjects
                    .iter_mut()
                    .find(|s| s.kind == kind && s.id == id)
                    .expect("just inserted");
                // Setting a level is audited first: nothing is set that was not recorded.
                self.audit(
                    client,
                    "set_trust",
                    kind,
                    id,
                    level.unwrap_or(PermissionClass::Dangerous),
                    json!({"trust": level.map(PermissionClass::as_str), "was": was.map(PermissionClass::as_str)}),
                    None,
                )?;
                rec.trust = level;
            } else if let Some(i) = idx {
                file.subjects[i].trust = None;
                if file.subjects[i].grants.is_empty() {
                    file.subjects.remove(i);
                }
            }
            Ok(())
        })?;
        if level.is_none() {
            let _ = self.audit(
                client,
                "set_trust",
                kind,
                id,
                PermissionClass::Dangerous,
                json!({"trust": null}),
                None,
            );
        }
        Ok(())
    }

    /// Whether `agent` may read `folders` in a run that starts now; the denial is logged.
    pub fn check_agent_run(&self, agent: &str, folders: &[PathBuf]) -> Result<(), String> {
        let result = self.snapshot().check_agent_folders(agent, folders);
        if let Err(e) = &result {
            let code = crate::error_key::parse(e).map_or("permissions.denied", |(k, _)| k);
            let _ = self.audit(
                "agent",
                "agent_run_denied",
                SubjectKind::Agent,
                agent,
                PermissionClass::Restricted,
                json!({"folders": folders.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()}),
                Some(code),
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::ToolCallQuery;

    struct Fx {
        dir: tempfile::TempDir,
        db: Arc<AnalyticsDb>,
    }

    impl Fx {
        fn new() -> Self {
            let dir = tempfile::Builder::new()
                .prefix(&crate::paths::test_dir_prefix("perms"))
                .tempdir()
                .unwrap();
            let db = Arc::new(AnalyticsDb::open(&dir.path().join("a.db")).unwrap());
            Fx { dir, db }
        }
        fn perms(&self) -> Permissions {
            Permissions::at(
                self.dir.path().join("permissions.json"),
                AuditSink::Db(self.db.clone()),
            )
        }
        fn folder(&self, name: &str) -> String {
            let p = self.dir.path().join(name);
            std::fs::create_dir_all(&p).unwrap();
            p.to_string_lossy().into_owned()
        }
        fn log(&self) -> Vec<ToolCallRow> {
            self.db
                .tool_calls(&ToolCallQuery {
                    limit: Some(100),
                    ..Default::default()
                })
                .unwrap()
        }
    }

    #[test]
    fn defaults_hold_nothing_and_fail_closed() {
        let fx = Fx::new();
        let p = fx.perms();
        let subjects = p.list_with(&[("files".into(), true), ("web".into(), false)]);
        assert_eq!(subjects.len(), 5);
        assert!(subjects
            .iter()
            .all(|s| s.grants.is_empty() && s.trust.is_none()));
        let web = subjects.iter().find(|s| s.id == "web").unwrap();
        assert_eq!(web.inherent, PermissionClass::Restricted);
        let snap = p.snapshot();
        // No record: an MCP tool keeps its class and is not trusted.
        assert_eq!(
            snap.judge_mcp("files", PermissionClass::Dangerous, false),
            (PermissionClass::Dangerous, false)
        );
        // No grant: an agent run with a folder is refused, one without runs.
        let dir = PathBuf::from(fx.folder("work"));
        assert!(snap.check_agent_folders(AGENT_HERMES, &[]).is_ok());
        assert!(snap
            .check_agent_folders(AGENT_HERMES, &[dir])
            .unwrap_err()
            .starts_with("permissions.folder_not_granted"));
    }

    #[test]
    fn grants_persist_revoke_and_are_audited() {
        let fx = Fx::new();
        let work = fx.folder("work");
        let g = fx
            .perms()
            .grant(
                "ui",
                SubjectKind::Agent,
                AGENT_HERMES,
                Capability::File,
                vec![work.clone()],
            )
            .unwrap();
        // A new handle (a restart) sees it; granting twice is one grant.
        let again = fx
            .perms()
            .grant(
                "ui",
                SubjectKind::Agent,
                AGENT_HERMES,
                Capability::File,
                vec![work.clone()],
            )
            .unwrap();
        assert_eq!(g.id, again.id);
        let view = fx.perms().list_with(&[]);
        let hermes = view.iter().find(|s| s.id == AGENT_HERMES).unwrap();
        assert_eq!(hermes.grants.len(), 1);

        let inside = PathBuf::from(&work).join("sub");
        std::fs::create_dir_all(&inside).unwrap();
        let inside = inside.canonicalize().unwrap();
        assert!(fx
            .perms()
            .snapshot()
            .check_agent_folders(AGENT_HERMES, std::slice::from_ref(&inside))
            .is_ok());
        let sibling = PathBuf::from(fx.folder("other")).canonicalize().unwrap();
        assert!(fx
            .perms()
            .snapshot()
            .check_agent_folders(AGENT_HERMES, &[sibling])
            .is_err());

        // Revoke: the very next check fails.
        fx.perms()
            .revoke_grant("cli", SubjectKind::Agent, AGENT_HERMES, &g.id)
            .unwrap();
        assert!(fx
            .perms()
            .snapshot()
            .check_agent_folders(AGENT_HERMES, &[inside])
            .is_err());
        assert!(fx
            .perms()
            .revoke_grant("cli", SubjectKind::Agent, AGENT_HERMES, &g.id)
            .unwrap_err()
            .starts_with("permissions.grant_not_found"));

        let log = fx.log();
        let tools: Vec<_> = log.iter().map(|r| r.tool.as_str()).collect();
        assert!(tools.contains(&"permissions.grant"));
        assert!(tools.contains(&"permissions.revoke_grant"));
        assert_eq!(
            tools.iter().filter(|t| **t == "permissions.grant").count(),
            1
        );
        assert!(log.iter().all(|r| r.source == "permissions"));
        let revoke = log
            .iter()
            .find(|r| r.tool == "permissions.revoke_grant")
            .unwrap();
        assert_eq!(revoke.client, "cli");
        assert!(revoke.args.contains(&g.id));
    }

    #[test]
    fn mcp_level_and_grants_change_the_judgement_until_revoked() {
        let fx = Fx::new();
        let p = fx.perms();
        p.set_trust(
            "ui",
            SubjectKind::McpServer,
            "files",
            Some(PermissionClass::Safe),
        )
        .unwrap();
        let g = p
            .grant(
                "ui",
                SubjectKind::McpServer,
                "files",
                Capability::Network,
                vec![],
            )
            .unwrap();
        let snap = p.snapshot();
        assert_eq!(
            snap.judge_mcp("files", PermissionClass::Dangerous, false),
            (PermissionClass::Safe, true)
        );
        // A class the user gave this very tool wins; other servers are untouched.
        assert_eq!(
            snap.judge_mcp("files", PermissionClass::Dangerous, true),
            (PermissionClass::Dangerous, true)
        );
        assert_eq!(
            snap.judge_mcp("other", PermissionClass::Dangerous, false),
            (PermissionClass::Dangerous, false)
        );
        p.revoke_grant("ui", SubjectKind::McpServer, "files", &g.id)
            .unwrap();
        assert_eq!(
            p.snapshot()
                .judge_mcp("files", PermissionClass::Dangerous, false),
            (PermissionClass::Safe, false)
        );
        p.revoke_subject("ui", SubjectKind::McpServer, "files")
            .unwrap();
        assert_eq!(
            p.snapshot()
                .judge_mcp("files", PermissionClass::Dangerous, false),
            (PermissionClass::Dangerous, false)
        );
        let tools: Vec<_> = fx.log().into_iter().map(|r| r.tool).collect();
        for t in ["set_trust", "grant", "revoke_grant", "revoke_subject"] {
            assert!(tools.contains(&format!("permissions.{t}")), "{t}");
        }
    }

    #[test]
    fn trainer_placeholder_takes_no_grants_and_no_level() {
        let fx = Fx::new();
        let p = fx.perms();
        for cap in Capability::ALL {
            let paths = if cap == Capability::File {
                vec![fx.folder("data")]
            } else {
                vec![]
            };
            let e = p
                .grant("api", SubjectKind::Trainer, TRAINER_DEFAULT, cap, paths)
                .unwrap_err();
            assert!(e.starts_with("permissions.not_available"), "{e}");
        }
        assert!(p
            .set_trust(
                "api",
                SubjectKind::Trainer,
                TRAINER_DEFAULT,
                Some(PermissionClass::Safe)
            )
            .unwrap_err()
            .starts_with("permissions.not_available"));
        assert!(p.snapshot().subjects.is_empty());
        let view = p.list_with(&[]);
        let t = view
            .iter()
            .find(|s| s.kind == SubjectKind::Trainer)
            .unwrap();
        assert!(!t.available && t.grantable.is_empty() && t.grants.is_empty());
        // The refusals are audited too.
        assert!(fx
            .log()
            .iter()
            .all(|r| r.error_code.as_deref() == Some("permissions.not_available")));
        assert_eq!(fx.log().len(), 7);
    }

    #[test]
    fn agents_only_take_file_grants_and_a_safe_level_blocks_folders() {
        let fx = Fx::new();
        let p = fx.perms();
        assert!(p
            .grant(
                "ui",
                SubjectKind::Agent,
                AGENT_HERMES,
                Capability::Shell,
                vec![]
            )
            .unwrap_err()
            .starts_with("permissions.capability_unsupported"));
        assert!(p
            .grant(
                "ui",
                SubjectKind::Agent,
                "unknown",
                Capability::File,
                vec![fx.folder("w")]
            )
            .unwrap_err()
            .starts_with("permissions.subject_not_found"));
        for bad in [
            vec![],
            vec!["relative/dir".to_string()],
            vec![fx.folder("w") + "/missing"],
        ] {
            assert!(p
                .grant(
                    "ui",
                    SubjectKind::Agent,
                    AGENT_HERMES,
                    Capability::File,
                    bad
                )
                .unwrap_err()
                .starts_with("permissions.paths_invalid"));
        }
        let w = fx.folder("w");
        p.grant(
            "ui",
            SubjectKind::Agent,
            AGENT_HERMES,
            Capability::File,
            vec![w.clone()],
        )
        .unwrap();
        let folder = PathBuf::from(&w).canonicalize().unwrap();
        assert!(p
            .check_agent_run(AGENT_HERMES, std::slice::from_ref(&folder))
            .is_ok());
        p.set_trust(
            "ui",
            SubjectKind::Agent,
            AGENT_HERMES,
            Some(PermissionClass::Safe),
        )
        .unwrap();
        assert!(p
            .check_agent_run(AGENT_HERMES, &[folder])
            .unwrap_err()
            .starts_with("permissions.trust_too_low"));
        assert!(fx
            .log()
            .iter()
            .any(|r| r.tool == "permissions.agent_run_denied"
                && r.error_code.as_deref() == Some("permissions.trust_too_low")));
    }
    #[test]
    fn windows_style_case_and_separators_do_not_escape_a_root() {
        let root = Path::new("/srv/Work");
        assert!(path_within(Path::new("/srv/Work/a/b"), root));
        assert!(path_within(Path::new("/srv/Work"), root));
        assert!(!path_within(Path::new("/srv/Work2"), root));
        assert!(!path_within(Path::new("/srv"), root));
    }
}
