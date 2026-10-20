//! Local, opt-in diagnostics. Only sanitized, bounded payloads reach disk.
use crate::config::AppSettings;
use crate::hardware::SystemHardwareInfo;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    pub event_type: String,
    pub message: String,
    pub stack_trace: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedHardwareSummary {
    pub os: String,
    pub arch: String,
    pub primary_gpu: String,
    pub driver_version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticPayload {
    pub id: String,
    pub version: String,
    pub os: String,
    pub engine: String,
    pub event_type: String,
    pub sanitized_message: String,
    pub sanitized_stack_trace: Option<String>,
    pub log_lines: Vec<String>,
    pub timestamp: u64,
    pub hardware: Option<SanitizedHardwareSummary>,
    pub metadata: Option<serde_json::Value>,
}

/// Future upload implementations consume this same reviewed payload/archive boundary.
/// No transport is installed or invoked by this module.
pub trait DiagnosticTransport {
    fn upload(&self, report: &DiagnosticPayload) -> Result<(), String>;
}

#[derive(Clone)]
pub struct DiagnosticsManager {
    dir: PathBuf,
}
static ENABLED: AtomicBool = AtomicBool::new(false);
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static HARDWARE: AtomicBool = AtomicBool::new(false);
static RETENTION: AtomicU32 = AtomicU32::new(30);
static SUMMARY: OnceLock<SanitizedHardwareSummary> = OnceLock::new();
static RECENT_LOGS: Mutex<std::collections::VecDeque<String>> =
    Mutex::new(std::collections::VecDeque::new());
const MAX_FILES: usize = 50;
const LOG_LINES: usize = 40;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn unique_name() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}.json",
        now(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}
fn json_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json") && p.is_file())
        .collect()
}

/// Free text can contain arbitrary chat, file or model content, even without markers.
/// Keep only known failure categories. Never attempt to guess which prose is private.
pub fn scrub_report_text(input: &str) -> String {
    let text = input.to_ascii_lowercase();
    for (needle, label) in [
        ("out of memory", "Out of memory"),
        ("allocation failed", "Memory allocation failed"),
        ("address already in use", "Address already in use"),
        ("assertion", "Assertion failed"),
        ("index out of bounds", "Index out of bounds"),
        ("unwrap()", "Unwrap failed"),
        ("unexpected", "Unexpected process exit"),
    ] {
        if text.contains(needle) {
            return label.into();
        }
    }
    "[content omitted]".into()
}

/// Rust backtrace frames contain compiled symbols, not runtime values. Drop all prose.
fn scrub_stack(input: &str) -> String {
    let frame =
        Regex::new(r"^\s*\d+: (?:0x[0-9a-f]+ - )?[A-Za-z_][A-Za-z0-9_:<> ,&\[\]().{}-]*$").unwrap();
    input
        .lines()
        .take(200)
        .filter(|line| line.len() <= 1024 && line.contains("::") && frame.is_match(line))
        .map(sanitize_text)
        .collect::<Vec<_>>()
        .join("\n")
}

impl DiagnosticsManager {
    /// Only normalized operational categories enter the diagnostic ring.
    pub fn note_log(line: &str) {
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut logs) = RECENT_LOGS.try_lock() {
            if logs.len() >= LOG_LINES {
                logs.pop_front();
            }
            logs.push_back(scrub_report_text(line));
        }
    }
    fn recent_logs() -> Vec<String> {
        RECENT_LOGS
            .try_lock()
            .map(|logs| logs.iter().cloned().collect())
            .unwrap_or_default()
    }
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
    pub fn default_manager() -> Self {
        Self::new(crate::paths::logs_dir().join("diagnostics"))
    }
    fn reports_dir(&self) -> PathBuf {
        self.dir.join("reports")
    }

    pub fn configure(settings: &AppSettings) {
        ENABLED.store(settings.privacy.crash_reports, Ordering::Relaxed);
        HARDWARE.store(settings.privacy.hardware_info, Ordering::Relaxed);
        RETENTION.store(
            settings.privacy.retention_days.clamp(1, 365),
            Ordering::Relaxed,
        );
        let mgr = Self::default_manager();
        mgr.cleanup(settings.privacy.retention_days);
        // Toggle changes also update the unclean-exit marker during this session.
        if settings.privacy.crash_reports && SESSION_ACTIVE.load(Ordering::Relaxed) {
            let _ = fs::create_dir_all(&mgr.dir);
            mgr.checkpoint(settings);
        } else if !settings.privacy.crash_reports {
            if SESSION_ACTIVE.load(Ordering::Relaxed) {
                let _ = fs::remove_file(mgr.dir.join("session-open"));
            }
            if let Ok(mut logs) = RECENT_LOGS.try_lock() {
                logs.clear();
            }
        }
    }
    /// A small sanitized snapshot lets a later launch detect native aborts or forced exits.
    pub fn checkpoint(&self, settings: &AppSettings) {
        if !SESSION_ACTIVE.load(Ordering::Relaxed) {
            return;
        }
        let event = DiagnosticEvent {
            event_type: "crash".into(),
            message: "Unexpected desktop process exit".into(),
            stack_trace: None,
            metadata: None,
        };
        if let Some(mut payload) = self.build_payload(&event, settings, None) {
            payload.log_lines = Self::recent_logs();
            if let Ok(bytes) = serde_json::to_vec(&payload) {
                let _ = fs::write(self.dir.join("session-open"), bytes);
            }
        }
    }
    pub fn start_session(settings: &AppSettings, hardware: &SystemHardwareInfo) {
        SESSION_ACTIVE.store(true, Ordering::Relaxed);
        let mgr = Self::default_manager();
        let marker = fs::read(mgr.dir.join("session-open")).ok();
        let previous = marker
            .as_deref()
            .and_then(|bytes| serde_json::from_slice::<DiagnosticPayload>(bytes).ok());
        let _ = SUMMARY.set(SanitizedHardwareSummary {
            os: sanitize_text(&hardware.os),
            arch: hardware.arch.clone(),
            primary_gpu: hardware
                .gpus
                .first()
                .map(|g| sanitize_text(&g.name))
                .unwrap_or_else(|| "Unknown".into()),
            driver_version: hardware
                .gpus
                .first()
                .and_then(|g| g.driver_version.as_deref().map(sanitize_text)),
        });
        Self::configure(settings);
        if previous.as_ref().is_some_and(|p| {
            p.timestamp
                < now().saturating_sub(
                    u64::from(settings.privacy.retention_days.clamp(1, 365)) * 86400,
                )
        }) {
            return;
        }
        if marker.is_some() && settings.privacy.crash_reports {
            let logs = previous
                .filter(|p| {
                    p.timestamp
                        >= now().saturating_sub(
                            u64::from(settings.privacy.retention_days.clamp(1, 365)) * 86400,
                        )
                })
                .map(|p| p.log_lines)
                .unwrap_or_default();
            let _ = mgr.save_event(
                &DiagnosticEvent {
                    event_type: "crash".into(),
                    message: "Unexpected desktop process exit".into(),
                    stack_trace: None,
                    metadata: None,
                },
                settings,
                "desktop",
                &logs,
            );
        }
    }
    pub fn end_session(&self) {
        SESSION_ACTIVE.store(false, Ordering::Relaxed);
        let _ = fs::remove_file(self.dir.join("session-open"));
    }

    pub fn save_offline_crash(&self, event: &DiagnosticEvent) -> Result<(), String> {
        // Frontend errors are untrusted text; only native Rust hooks supply compiled frames.
        let event = DiagnosticEvent {
            stack_trace: None,
            ..event.clone()
        };
        self.save_event(&event, &AppSettings::load(), "desktop", &[])
    }
    pub fn save_event(
        &self,
        event: &DiagnosticEvent,
        settings: &AppSettings,
        engine: &str,
        logs: &[String],
    ) -> Result<(), String> {
        self.cleanup(settings.privacy.retention_days);
        let Some(mut payload) = self.build_payload(event, settings, None) else {
            return Ok(());
        };
        // Engine is selected by native callers, never taken from event metadata.
        payload.engine = match engine {
            "desktop"
            | "llama.cpp"
            | "whisper.cpp"
            | "stable-diffusion.cpp"
            | "sherpa-onnx"
            | "mistral.rs"
            | "python-hf"
            | "mlx"
            | "mlx-vlm"
            | "mlx-audio"
            | "mflux"
            | "onnxruntime"
            | "crispasr"
            | "transcribe.cpp"
            | "lemonade"
            | "audio.cpp" => engine.into(),
            _ => "other".into(),
        };
        payload.log_lines = logs
            .iter()
            .rev()
            .take(LOG_LINES)
            .rev()
            .map(|s| scrub_report_text(s))
            .collect();
        let dir = self.reports_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let data = serde_json::to_vec_pretty(&payload).map_err(|e| e.to_string())?;
        let temporary = dir.join(format!("{}.tmp", payload.id));
        let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&data).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, dir.join(&payload.id)).map_err(|e| e.to_string())?;
        self.cleanup(settings.privacy.retention_days);
        Ok(())
    }
    pub fn record(event_type: &str, message: String, stack_trace: Option<String>) {
        Self::record_engine(
            event_type,
            message,
            stack_trace,
            "desktop",
            &Self::recent_logs(),
        );
    }
    pub fn record_engine(
        event_type: &str,
        message: String,
        stack_trace: Option<String>,
        engine: &str,
        logs: &[String],
    ) {
        // Panic-safe: no settings/cache mutex, keychain access or hardware subprocesses.
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let mut settings = AppSettings::default();
        settings.privacy.crash_reports = true;
        settings.privacy.hardware_info = HARDWARE.load(Ordering::Relaxed);
        settings.privacy.retention_days = RETENTION.load(Ordering::Relaxed);
        let _ = Self::default_manager().save_event(
            &DiagnosticEvent {
                event_type: event_type.into(),
                message,
                stack_trace,
                metadata: None,
            },
            &settings,
            engine,
            logs,
        );
    }
    pub fn process_offline_crash(
        &self,
        settings: &AppSettings,
        _hardware: Option<&SystemHardwareInfo>,
    ) -> Result<Vec<DiagnosticPayload>, String> {
        self.cleanup(settings.privacy.retention_days);
        Ok(Vec::new())
    }
    pub fn cleanup(&self, days: u32) {
        // Legacy versions stored unsanitized pending events: discard, never migrate them.
        let _ = fs::remove_dir_all(self.dir.join("pending"));
        let cutoff = now().saturating_sub(u64::from(days.clamp(1, 365)) * 86_400);
        let mut kept = Vec::new();
        for path in json_files(&self.reports_dir()) {
            let report = fs::read(&path)
                .ok()
                .and_then(|s| serde_json::from_slice::<DiagnosticPayload>(&s).ok());
            if let Some(report) = report.filter(|r| r.timestamp >= cutoff) {
                kept.push((report.timestamp, path));
            } else {
                let _ = fs::remove_file(path);
            }
        }
        kept.sort_by_key(|entry| entry.0);
        let excess = kept.len().saturating_sub(MAX_FILES);
        for (_, path) in kept.into_iter().take(excess) {
            let _ = fs::remove_file(path);
        }
    }
    pub fn list_reports(&self) -> Vec<DiagnosticPayload> {
        let mut reports: Vec<DiagnosticPayload> = json_files(&self.reports_dir())
            .iter()
            .filter_map(|p| fs::read(p).ok())
            .filter_map(|s| serde_json::from_slice(&s).ok())
            .collect();
        reports.sort_by_key(|report| std::cmp::Reverse(report.timestamp));
        reports
    }
    fn report_path(&self, id: &str) -> Result<PathBuf, String> {
        let valid = id.strip_suffix(".json").is_some_and(|stem| {
            let parts = stem.split('-').collect::<Vec<_>>();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        });
        if !valid {
            return Err(crate::error_key::err("diagnostics.invalid_report_id").into());
        }
        Ok(self.reports_dir().join(id))
    }
    pub fn delete_report(&self, id: &str) -> Result<(), String> {
        fs::remove_file(self.report_path(id)?).map_err(|e| e.to_string())
    }
    pub fn export_zip(&self, id: &str, destination: &Path) -> Result<(), String> {
        let bytes = fs::read(self.report_path(id)?).map_err(|e| e.to_string())?;
        // Parse before exporting; archives contain one sanitized report, no surrounding files.
        let report: DiagnosticPayload =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let file = fs::File::create(destination).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("report.json", zip::write::SimpleFileOptions::default())
            .map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn clear_reports(&self) -> Result<(), String> {
        for path in json_files(&self.reports_dir()) {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        let _ = fs::remove_dir_all(self.dir.join("pending"));
        Ok(())
    }
    pub fn build_payload(
        &self,
        event: &DiagnosticEvent,
        settings: &AppSettings,
        _hardware: Option<&SystemHardwareInfo>,
    ) -> Option<DiagnosticPayload> {
        if !settings.privacy.crash_reports
            || (event.event_type == "app_error" && !settings.privacy.app_errors)
        {
            return None;
        }
        let event_type = match event.event_type.as_str() {
            "crash" | "llama_error" | "app_error" => event.event_type.clone(),
            _ => return None,
        };
        Some(DiagnosticPayload {
            id: unique_name(),
            version: env!("CARGO_PKG_VERSION").into(),
            os: std::env::consts::OS.into(),
            engine: "desktop".into(),
            event_type,
            sanitized_message: scrub_report_text(&event.message),
            sanitized_stack_trace: event.stack_trace.as_deref().map(scrub_stack),
            log_lines: vec![],
            timestamp: now(),
            hardware: if settings.privacy.hardware_info {
                SUMMARY.get().cloned()
            } else {
                None
            },
            // Arbitrary metadata may hold chat, model/file contents or secrets.
            metadata: None,
        })
    }
}

pub fn sanitize_text(input: &str) -> String {
    let rules: &[(&str, &str)] = &[
        // Home paths: omit the entire suffix, which can itself identify a chat or file.
        (r#"(?i)\b[a-z]:[/\\]Users[/\\][^\s"'<>]+"#, "~"),
        (r#"/(?:home|Users)/[^\s"'<>]+"#, "~"),
        // Bearer tokens and key=value secrets.
        (
            r#"(?i)(Bearer\s+|["']?(?:api[_-]?key|access[_-]?token|token|secret|password)["']?\s*[:=]\s*["']?|--api-key\s+)[^\s"',;]+"#,
            "$1[REDACTED_KEY]",
        ),
        // Well-known token formats even without a prefix (HF, OpenAI-style).
        (r"\bhf_[A-Za-z0-9]{20,}", "[REDACTED_KEY]"),
        (r"\bsk-[A-Za-z0-9_\-]{16,}", "[REDACTED_KEY]"),
        // E-mail addresses.
        (
            r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}",
            "<EMAIL>",
        ),
        // Chat template markers (possible prompt leakage).
        (
            r"(?i)(<\|im_start\|>|<\|im_end\|>|<\|end_of_text\|>|<\|eot_id\|>|\[INST\]|\[/INST\])",
            "[PROMPT_TOKEN_REDACTED]",
        ),
    ];
    let mut result = input.to_string();
    if let Some(home) = dirs::home_dir() {
        let home = regex::escape(&home.to_string_lossy());
        if let Ok(re) = Regex::new(&format!(r#"(?i){}(?:[/\\][^\s"'<>]*)?"#, home)) {
            result = re.replace_all(&result, "~").to_string();
        }
    }
    for (pattern, replacement) in rules {
        if let Ok(re) = Regex::new(pattern) {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event() -> DiagnosticEvent {
        DiagnosticEvent {
            event_type: "crash".into(),
            message: "panic: private chat".into(),
            stack_trace: Some("0: harness_core::process::run\nuser: secret chat".into()),
            metadata: Some(serde_json::json!({"chat": "private chat"})),
        }
    }
    #[test]
    fn scrubs_home_paths_and_secrets() {
        for path in [
            r"C:\Users\alice\private\model.gguf",
            "C:/Users/alice/private/model.gguf",
            "/home/alice/private/model.gguf",
            "/Users/alice/private/model.gguf",
        ] {
            let text = sanitize_text(path);
            assert_eq!(text, "~");
        }
        let text = sanitize_text("Bearer abcdefghijklmnop token=abcdefghijk hf_abcdefghijklmnopqrstuvwxyz sk-abcdefghijklmnopqrstuvwxyz");
        assert!(!text.contains("abcdef"));
        let text = sanitize_text(r#"{"api_key":"short", "token":"tiny"} Bearer brief"#);
        for value in ["short", "tiny", "brief"] {
            assert!(!text.contains(value));
        }
    }
    #[test]
    fn unmarked_chat_and_file_content_never_reach_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = DiagnosticsManager::new(dir.path());
        let mut settings = AppSettings::default();
        settings.privacy.crash_reports = true;
        let logs = vec![
            "<|im_start|>user\nmy private chat".into(),
            "arbitrary file contents".into(),
            "model response without markers".into(),
            "out of memory: private chat".into(),
        ];
        mgr.save_event(&event(), &settings, "desktop", &logs)
            .unwrap();
        let report = mgr.list_reports().pop().unwrap();
        let json = serde_json::to_string(&report).unwrap();
        for secret in [
            "private chat",
            "file contents",
            "model response",
            "im_start",
        ] {
            assert!(!json.contains(secret), "{json}");
        }
        assert!(json.contains("harness_core::process::run"));
        assert!(json.contains("Out of memory"));
    }
    #[test]
    fn disabled_writes_no_reports() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = DiagnosticsManager::new(dir.path());
        mgr.save_event(&event(), &AppSettings::default(), "desktop", &[])
            .unwrap();
        assert!(mgr.list_reports().is_empty());
        assert!(!mgr.reports_dir().exists());
        assert_eq!(AppSettings::default().privacy.retention_days, 30);
    }
    #[test]
    fn retention_export_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = DiagnosticsManager::new(dir.path());
        let mut settings = AppSettings::default();
        settings.privacy.crash_reports = true;
        mgr.save_event(&event(), &settings, "desktop", &[]).unwrap();
        let mut old = mgr.list_reports().pop().unwrap();
        old.timestamp = now() - 31 * 86400;
        old.id = "1-1-1.json".into();
        fs::write(
            mgr.reports_dir().join(&old.id),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        mgr.cleanup(30);
        assert_eq!(mgr.list_reports().len(), 1);
        let report = mgr.list_reports().pop().unwrap();
        let dest = dir.path().join("export.zip");
        mgr.export_zip(&report.id, &dest).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(dest).unwrap()).unwrap();
        assert_eq!(zip.len(), 1);
        assert_eq!(zip.by_index(0).unwrap().name().unwrap(), "report.json");
        assert!(mgr.delete_report("../settings.json").is_err());
        mgr.delete_report(&report.id).unwrap();
        assert!(mgr.list_reports().is_empty());
    }
    #[test]
    fn custom_retention_and_legacy_data() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = DiagnosticsManager::new(dir.path());
        let mut settings = AppSettings::default();
        settings.privacy.crash_reports = true;
        mgr.save_event(&event(), &settings, "desktop", &[]).unwrap();
        let mut report = mgr.list_reports().pop().unwrap();
        report.timestamp = now() - 8 * 86400;
        fs::write(
            mgr.reports_dir().join(&report.id),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("pending")).unwrap();
        fs::write(dir.path().join("pending/raw.json"), "private chat").unwrap();
        fs::write(
            mgr.reports_dir().join("legacy.json"),
            "{\"sanitized_message\":\"private chat\"}",
        )
        .unwrap();
        mgr.cleanup(30);
        assert_eq!(mgr.list_reports().len(), 1);
        assert!(!dir.path().join("pending").exists());
        mgr.cleanup(7);
        assert!(mgr.list_reports().is_empty());
    }
    #[test]
    fn log_tail_and_panic_scrubbing() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = DiagnosticsManager::new(dir.path());
        let mut settings = AppSettings::default();
        settings.privacy.crash_reports = true;
        let logs = vec!["allocation failed: secret file content".into(); 100];
        mgr.save_event(&event(), &settings, "onnxruntime", &logs)
            .unwrap();
        let report = mgr.list_reports().pop().unwrap();
        assert_eq!(report.engine, "onnxruntime");
        assert_eq!(report.log_lines.len(), LOG_LINES);
        assert!(report
            .log_lines
            .iter()
            .all(|line| line == "Memory allocation failed"));
        assert_eq!(
            scrub_report_text("assertion failed: secret chat"),
            "Assertion failed"
        );
        assert_eq!(
            scrub_stack("0: private chat\nuser: secret\n0: harness_core::run"),
            "0: harness_core::run"
        );
        assert!(report.metadata.is_none());
        mgr.clear_reports().unwrap();
        assert!(mgr.list_reports().is_empty());
    }
}
