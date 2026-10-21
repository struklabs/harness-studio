//! Consent for heavy workloads and for the microphone and camera.
//!
//! The capability grants in [`crate::permissions`] say what an MCP server or an agent may
//! reach. This module answers a different question: *may this client start something that
//! occupies the machine, or listen and watch?*
//!
//! * [`WorkloadKind::HeavyGpu`] / [`WorkloadKind::HeavyCpu`]: a long training, conversion or
//!   video job. An external client (an API token) that starts one is asked **once**; the
//!   answer is remembered per client and can be revoked on the Permissions page. The desktop
//!   app itself is the owner and is never asked about its own jobs.
//! * [`WorkloadKind::Microphone`] / [`WorkloadKind::Camera`]: two checks. The operating
//!   system's own switch ([`os_access`]) comes first, then Harness's consent prompt. A client
//!   that was refused by either never gets the device.
//!
//! Nothing is stored until a client has been asked and the user chose to remember the answer.
//! Every remembered answer and every revoke is a row in the tool log (`workload.*`, source
//! `permissions`). The settings are the user's: nothing here is reachable from a tool call.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::analytics::{AnalyticsDb, ToolCallRow};
use crate::error_key::err;
use crate::permissions::AuditSink;
use crate::store::JsonStore;

/// A job is "heavy" when it is expected to keep the machine busy at least this long.
pub const HEAVY_JOB_SECS: u64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum WorkloadKind {
    HeavyGpu,
    HeavyCpu,
    Microphone,
    Camera,
}

impl WorkloadKind {
    pub const ALL: [WorkloadKind; 4] = [
        Self::HeavyGpu,
        Self::HeavyCpu,
        Self::Microphone,
        Self::Camera,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::HeavyGpu => "heavy_gpu",
            Self::HeavyCpu => "heavy_cpu",
            Self::Microphone => "microphone",
            Self::Camera => "camera",
        }
    }

    /// The device kinds also answer to the operating system.
    pub fn is_device(self) -> bool {
        matches!(self, Self::Microphone | Self::Camera)
    }
}

/// Which heavy kind a job is, or `None` when it is short enough to need no consent.
pub fn heavy_kind(uses_gpu: bool, expected_secs: u64) -> Option<WorkloadKind> {
    (expected_secs >= HEAVY_JOB_SECS).then_some(if uses_gpu {
        WorkloadKind::HeavyGpu
    } else {
        WorkloadKind::HeavyCpu
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Choice {
    Allow,
    Deny,
}

/// One remembered answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Consent {
    pub id: String,
    /// The client: `ui`, `cli`, or the name of an API token.
    pub client: String,
    pub kind: WorkloadKind,
    pub choice: Choice,
    pub decided_at_ms: u64,
}

/// What a check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Verdict {
    /// Remembered "allow" (and, for a device, the operating system allows it).
    Allowed,
    /// Remembered "deny".
    Denied,
    /// Never asked: the caller shows the prompt.
    Ask,
    /// The operating system blocks the device. Harness cannot override it; the user changes
    /// it in the system settings.
    OsBlocked,
}

/// The operating system's own switch for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OsAccess {
    Allowed,
    Denied,
    /// The system offers no switch Harness can read (or the answer is "ask on first use").
    Unknown,
}

/// What the Permissions page shows for one workload kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkloadView {
    pub kind: WorkloadKind,
    /// `None` for the kinds without an operating-system switch.
    pub os: Option<OsAccess>,
    pub consents: Vec<Consent>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    consents: Vec<Consent>,
}

/// Maps the value of a Windows `CapabilityAccessManager\ConsentStore` entry (`Allow`, `Deny`)
/// to [`OsAccess`]. Anything else (a missing key, `Prompt`) is [`OsAccess::Unknown`].
pub fn parse_consent_value(value: &str) -> OsAccess {
    match value.trim().to_ascii_lowercase().as_str() {
        "allow" => OsAccess::Allowed,
        "deny" => OsAccess::Denied,
        _ => OsAccess::Unknown,
    }
}

/// Extracts the `Value` of `reg query ... /v Value` output.
pub fn parse_reg_query(output: &str) -> OsAccess {
    output
        .lines()
        .find(|l| l.trim_start().starts_with("Value"))
        .and_then(|l| l.split_whitespace().last())
        .map_or(OsAccess::Unknown, parse_consent_value)
}

/// The operating system's switch for the microphone or camera.
///
/// Windows reads the privacy setting (`Settings > Privacy > Microphone / Camera`). macOS and
/// Linux have no switch to read without native frameworks: macOS asks on first use and Linux
/// has none, so they answer [`OsAccess::Unknown`] and Harness's own consent is the only gate
/// there. Other kinds have no switch.
pub fn os_access(kind: WorkloadKind) -> OsAccess {
    #[cfg(windows)]
    {
        let name = match kind {
            WorkloadKind::Microphone => "microphone",
            WorkloadKind::Camera => "webcam",
            _ => return OsAccess::Unknown,
        };
        let key = format!(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{name}"
        );
        let mut cmd = crate::process::quiet_command("reg");
        cmd.args(["query", &key, "/v", "Value"]);
        match cmd.output() {
            Ok(o) if o.status.success() => parse_reg_query(&String::from_utf8_lossy(&o.stdout)),
            _ => OsAccess::Unknown,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = kind;
        OsAccess::Unknown
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn valid_client(client: &str) -> bool {
    !client.trim().is_empty() && client.len() <= 128 && !client.chars().any(char::is_control)
}

pub struct Workloads {
    store: JsonStore,
    sink: AuditSink,
}

impl Workloads {
    /// The user's consents: `workload_consents.json` in the data directory.
    pub fn shared() -> Self {
        Self::at(
            crate::paths::data_root().join("workload_consents.json"),
            AuditSink::Shared,
        )
    }

    pub fn at(path: impl Into<PathBuf>, sink: AuditSink) -> Self {
        Self {
            store: JsonStore::sensitive(path.into()),
            sink,
        }
    }

    fn consents(&self) -> Vec<Consent> {
        self.store.load_or_default::<File>().consents
    }

    /// The pure decision: remembered answer plus the operating-system state.
    pub fn verdict_with(&self, client: &str, kind: WorkloadKind, os: OsAccess) -> Verdict {
        if kind.is_device() && os == OsAccess::Denied {
            return Verdict::OsBlocked;
        }
        match self
            .consents()
            .iter()
            .find(|c| c.client == client && c.kind == kind)
            .map(|c| c.choice)
        {
            Some(Choice::Allow) => Verdict::Allowed,
            Some(Choice::Deny) => Verdict::Denied,
            None => Verdict::Ask,
        }
    }

    /// [`Self::verdict_with`] with the live operating-system state.
    pub fn verdict(&self, client: &str, kind: WorkloadKind) -> Verdict {
        let os = if kind.is_device() {
            os_access(kind)
        } else {
            OsAccess::Unknown
        };
        self.verdict_with(client, kind, os)
    }

    /// `Ok` when `client` may start the workload. `Err` carries the error key the caller turns
    /// into a prompt (`workload.consent_required`) or a refusal.
    pub fn require(&self, client: &str, kind: WorkloadKind) -> Result<(), String> {
        let code = match self.verdict(client, kind) {
            Verdict::Allowed => return Ok(()),
            Verdict::Ask => "workload.consent_required",
            Verdict::Denied => "workload.denied",
            Verdict::OsBlocked => "workload.os_blocked",
        };
        Err(err(code)
            .with("client", client)
            .with("kind", kind.as_str())
            .into())
    }

    /// Remembers the user's answer for `client` and `kind` (replacing an earlier one). The
    /// audit row is written first: nothing is remembered that was not recorded.
    pub fn remember(
        &self,
        by: &str,
        client: &str,
        kind: WorkloadKind,
        choice: Choice,
    ) -> Result<Consent, String> {
        if !valid_client(client) {
            return Err(err("workload.client_invalid").into());
        }
        let consent = Consent {
            id: uuid::Uuid::new_v4().to_string(),
            client: client.to_string(),
            kind,
            choice,
            decided_at_ms: now_ms(),
        };
        self.store.update::<File, _>(|f| {
            self.audit(
                by,
                "remember",
                client,
                kind,
                json!({"choice": choice, "consent": consent.id}),
            )?;
            f.consents
                .retain(|c| !(c.client == client && c.kind == kind));
            f.consents.push(consent.clone());
            Ok(consent.clone())
        })
    }

    /// Removes one remembered answer: the client is asked again next time.
    pub fn revoke(&self, by: &str, consent_id: &str) -> Result<(), String> {
        let removed = self.store.update::<File, _>(|f| {
            Ok(f.consents
                .iter()
                .position(|c| c.id == consent_id)
                .map(|i| f.consents.remove(i)))
        })?;
        let Some(c) = removed else {
            return Err(err("workload.consent_not_found")
                .with("id", consent_id)
                .into());
        };
        let _ = self.audit(
            by,
            "revoke",
            &c.client,
            c.kind,
            json!({"choice": c.choice, "consent": c.id}),
        );
        Ok(())
    }

    /// Removes every remembered answer of a client.
    pub fn revoke_client(&self, by: &str, client: &str) -> Result<u32, String> {
        let n = self.store.update::<File, _>(|f| {
            let before = f.consents.len();
            f.consents.retain(|c| c.client != client);
            Ok((before - f.consents.len()) as u32)
        })?;
        let _ = self.audit(
            by,
            "revoke_client",
            client,
            WorkloadKind::HeavyCpu,
            json!({"removed": n}),
        );
        Ok(n)
    }

    /// Every kind with its operating-system state and remembered answers.
    pub fn list(&self) -> Vec<WorkloadView> {
        let all = self.consents();
        WorkloadKind::ALL
            .iter()
            .map(|&kind| WorkloadView {
                kind,
                os: kind.is_device().then(|| os_access(kind)),
                consents: all.iter().filter(|c| c.kind == kind).cloned().collect(),
            })
            .collect()
    }

    fn audit(
        &self,
        by: &str,
        action: &str,
        client: &str,
        kind: WorkloadKind,
        detail: serde_json::Value,
    ) -> Result<(), String> {
        let mut args = json!({"client": client, "kind": kind.as_str()});
        if let (Some(a), Some(d)) = (args.as_object_mut(), detail.as_object()) {
            a.extend(d.clone());
        }
        let row = ToolCallRow {
            id: uuid::Uuid::new_v4().to_string(),
            started_at_ms: now_ms(),
            client: by.to_string(),
            conversation: None,
            model: None,
            tool: format!("workload.{action}"),
            class: "dangerous".into(),
            source: "permissions".into(),
            args: crate::tools::redact_args(&args),
            approved: true,
            error_code: None,
            duration_ms: 0,
            result_chars: 0,
            truncated: false,
            network_target: None,
        };
        let db: Arc<AnalyticsDb> = match &self.sink {
            AuditSink::Db(db) => db.clone(),
            AuditSink::Shared => crate::analytics::shared()?,
        };
        db.insert_tool_call(&row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Workloads, Arc<AnalyticsDb>) {
        let d = tempfile::tempdir().unwrap();
        let db = Arc::new(AnalyticsDb::open(&d.path().join("a.db")).unwrap());
        let w = Workloads::at(d.path().join("c.json"), AuditSink::Db(db.clone()));
        (d, w, db)
    }

    #[test]
    fn nothing_is_allowed_until_asked() {
        let (_d, w, _) = store();
        for kind in WorkloadKind::ALL {
            assert_eq!(w.verdict_with("tok", kind, OsAccess::Unknown), Verdict::Ask);
        }
        let e = w.require("tok", WorkloadKind::HeavyGpu).unwrap_err();
        assert!(e.starts_with("workload.consent_required"), "{e}");
    }

    #[test]
    fn an_answer_is_remembered_per_client_and_kind_and_revocable() {
        let (_d, w, db) = store();
        let c = w
            .remember("ui", "tok", WorkloadKind::HeavyGpu, Choice::Allow)
            .unwrap();
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::HeavyGpu, OsAccess::Unknown),
            Verdict::Allowed
        );
        // Another client and another kind are still asked.
        assert_eq!(
            w.verdict_with("other", WorkloadKind::HeavyGpu, OsAccess::Unknown),
            Verdict::Ask
        );
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::HeavyCpu, OsAccess::Unknown),
            Verdict::Ask
        );
        // Changing the answer replaces it.
        w.remember("ui", "tok", WorkloadKind::HeavyGpu, Choice::Deny)
            .unwrap();
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::HeavyGpu, OsAccess::Unknown),
            Verdict::Denied
        );
        assert!(w.revoke("ui", &c.id).is_err(), "the old id is gone");
        let id = w.list()[0].consents[0].id.clone();
        w.revoke("ui", &id).unwrap();
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::HeavyGpu, OsAccess::Unknown),
            Verdict::Ask
        );
        // remember x2 + revoke are in the tool log.
        let rows = db
            .tool_calls(&crate::analytics::ToolCallQuery {
                limit: Some(100),
                ..Default::default()
            })
            .unwrap();
        let names: Vec<_> = rows.iter().map(|r| r.tool.clone()).collect();
        assert!(
            names.iter().filter(|n| *n == "workload.remember").count() == 2,
            "{names:?}"
        );
        assert!(names.contains(&"workload.revoke".to_string()), "{names:?}");
    }

    #[test]
    fn the_operating_system_switch_beats_a_remembered_allow() {
        let (_d, w, _) = store();
        w.remember("ui", "tok", WorkloadKind::Microphone, Choice::Allow)
            .unwrap();
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::Microphone, OsAccess::Allowed),
            Verdict::Allowed
        );
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::Microphone, OsAccess::Denied),
            Verdict::OsBlocked
        );
        // The switch only applies to devices.
        w.remember("ui", "tok", WorkloadKind::HeavyCpu, Choice::Allow)
            .unwrap();
        assert_eq!(
            w.verdict_with("tok", WorkloadKind::HeavyCpu, OsAccess::Denied),
            Verdict::Allowed
        );
    }

    #[test]
    fn revoking_a_client_removes_all_its_answers() {
        let (_d, w, _) = store();
        for k in [WorkloadKind::HeavyGpu, WorkloadKind::Camera] {
            w.remember("ui", "tok", k, Choice::Allow).unwrap();
        }
        w.remember("ui", "keep", WorkloadKind::Camera, Choice::Allow)
            .unwrap();
        assert_eq!(w.revoke_client("ui", "tok").unwrap(), 2);
        assert_eq!(
            w.verdict_with("keep", WorkloadKind::Camera, OsAccess::Unknown),
            Verdict::Allowed
        );
    }

    #[test]
    fn bad_client_names_are_refused() {
        let (_d, w, _) = store();
        assert!(w
            .remember("ui", "  ", WorkloadKind::Camera, Choice::Allow)
            .is_err());
        assert!(w
            .remember("ui", "a\nb", WorkloadKind::Camera, Choice::Allow)
            .is_err());
    }

    #[test]
    fn only_long_jobs_are_heavy() {
        assert_eq!(heavy_kind(true, 30), None);
        assert_eq!(
            heavy_kind(true, HEAVY_JOB_SECS),
            Some(WorkloadKind::HeavyGpu)
        );
        assert_eq!(heavy_kind(false, 3600), Some(WorkloadKind::HeavyCpu));
    }

    #[test]
    fn windows_privacy_values_are_parsed() {
        assert_eq!(parse_consent_value("Allow"), OsAccess::Allowed);
        assert_eq!(parse_consent_value(" Deny "), OsAccess::Denied);
        assert_eq!(parse_consent_value("Prompt"), OsAccess::Unknown);
        let out = "\r\nHKEY_CURRENT_USER\\Software\\...\\microphone\r\n    Value    REG_SZ    Deny\r\n\r\n";
        assert_eq!(parse_reg_query(out), OsAccess::Denied);
        assert_eq!(parse_reg_query("ERROR: not found"), OsAccess::Unknown);
    }
}
