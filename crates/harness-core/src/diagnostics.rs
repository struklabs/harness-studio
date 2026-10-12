use crate::config::AppSettings;
use crate::hardware::SystemHardwareInfo;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    pub event_type: String, // "crash", "app_error", "llama_error", "performance"
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
    pub total_ram_gb: f64,
    pub total_vram_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticPayload {
    pub event_type: String,
    pub sanitized_message: String,
    pub sanitized_stack_trace: Option<String>,
    pub timestamp: u64,
    pub hardware: Option<SanitizedHardwareSummary>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct DiagnosticsManager {
    /// Root folder: `pending/` holds raw events, `reports/` sanitized payloads.
    dir: PathBuf,
}

/// Upper bound on stored files per folder, so a crash loop can't fill the disk.
const MAX_FILES: usize = 50;

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Unique, time-sortable file name (millisecond timestamp + per-process sequence).
fn create_dir_failed(dir: &Path, e: std::io::Error) -> String {
    crate::error_key::err("fs.create_dir_failed")
        .with("path", dir.display())
        .with("error", e)
        .into()
}

fn unique_name() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!(
        "{:016}-{}-{:06}.json",
        now_millis(),
        std::process::id(),
        seq
    )
}

fn sorted_json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn prune(dir: &Path) {
    let files = sorted_json_files(dir);
    if files.len() > MAX_FILES {
        for old in &files[..files.len() - MAX_FILES] {
            let _ = fs::remove_file(old);
        }
    }
}

impl DiagnosticsManager {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    pub fn default_manager() -> Self {
        Self::new(crate::paths::logs_dir().join("diagnostics"))
    }

    fn pending_dir(&self) -> PathBuf {
        self.dir.join("pending")
    }

    fn reports_dir(&self) -> PathBuf {
        self.dir.join("reports")
    }

    /// Stores a raw event locally. Nothing leaves the machine; consent is applied when processed.
    pub fn save_offline_crash(&self, event: &DiagnosticEvent) -> Result<(), String> {
        let dir = self.pending_dir();
        fs::create_dir_all(&dir).map_err(|e| create_dir_failed(&dir, e))?;
        let path = dir.join(unique_name());
        let serialized = serde_json::to_string_pretty(event).map_err(|e| {
            String::from(crate::error_key::err("json.serialize_failed").with("error", e))
        })?;
        fs::write(&path, serialized).map_err(|e| {
            String::from(
                crate::error_key::err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
        prune(&dir);
        Ok(())
    }

    /// Records an event using the default location (usable from anywhere, e.g. the panic hook).
    pub fn record(event_type: &str, message: String, stack_trace: Option<String>) {
        let _ = Self::default_manager().save_offline_crash(&DiagnosticEvent {
            event_type: event_type.to_string(),
            message,
            stack_trace,
            metadata: None,
        });
    }

    /// Converts all pending events into sanitized reports, honouring the per-category consent
    /// toggles. Events without consent are discarded. Pending files are always removed.
    pub fn process_offline_crash(
        &self,
        settings: &AppSettings,
        hardware: Option<&SystemHardwareInfo>,
    ) -> Result<Vec<DiagnosticPayload>, String> {
        let mut payloads = Vec::new();
        for file in sorted_json_files(&self.pending_dir()) {
            let event = fs::read_to_string(&file)
                .ok()
                .and_then(|c| serde_json::from_str::<DiagnosticEvent>(&c).ok());
            // Always remove the raw dump to avoid duplicates or keeping revoked data.
            let _ = fs::remove_file(&file);
            if let Some(payload) = event.and_then(|e| self.build_payload(&e, settings, hardware)) {
                payloads.push(payload);
            }
        }
        if !payloads.is_empty() {
            let dir = self.reports_dir();
            fs::create_dir_all(&dir).map_err(|e| create_dir_failed(&dir, e))?;
            for p in &payloads {
                let path = dir.join(unique_name());
                if let Ok(data) = serde_json::to_string_pretty(p) {
                    let _ = fs::write(path, data);
                }
            }
            prune(&dir);
        }
        Ok(payloads)
    }

    /// Sanitized reports kept locally (newest first) so the user can review or share them.
    pub fn list_reports(&self) -> Vec<DiagnosticPayload> {
        let mut reports: Vec<DiagnosticPayload> = sorted_json_files(&self.reports_dir())
            .iter()
            .filter_map(|p| fs::read_to_string(p).ok())
            .filter_map(|c| serde_json::from_str(&c).ok())
            .collect();
        reports.reverse();
        reports
    }

    pub fn clear_reports(&self) {
        let _ = fs::remove_dir_all(self.reports_dir());
        let _ = fs::remove_dir_all(self.pending_dir());
    }

    pub fn build_payload(
        &self,
        event: &DiagnosticEvent,
        settings: &AppSettings,
        hardware: Option<&SystemHardwareInfo>,
    ) -> Option<DiagnosticPayload> {
        // Strict Privacy Rule 1: No transmission without onboarding consent
        if !settings.privacy.onboarding_completed {
            return None;
        }

        // Strict Privacy Rule 2: Verify category-specific toggle
        let category_allowed = match event.event_type.as_str() {
            "crash" => settings.privacy.crash_reports,
            "app_error" => settings.privacy.app_errors,
            "llama_error" => settings.privacy.llama_errors,
            "performance" => settings.privacy.performance_metrics,
            _ => false,
        };

        if !category_allowed {
            return None;
        }

        // Strict Privacy Rule 3: Sanitize message and stack trace (strip user paths, API keys, prompt tokens)
        let sanitized_message = sanitize_text(&event.message);
        let sanitized_stack_trace = event.stack_trace.as_deref().map(sanitize_text);

        // Strict Privacy Rule 4: Include hardware ONLY if hardware_info toggle is true
        let hw_summary = if settings.privacy.hardware_info {
            hardware.map(|hw| {
                let primary_gpu = hw
                    .gpus
                    .first()
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                let driver_version = hw.gpus.first().and_then(|g| g.driver_version.clone());
                let total_vram_gb = hw.gpus.first().and_then(|g| g.vram_bytes).unwrap_or(0) as f64
                    / (1024.0 * 1024.0 * 1024.0);
                let total_ram_gb = hw.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

                SanitizedHardwareSummary {
                    os: hw.os.clone(),
                    arch: hw.arch.clone(),
                    primary_gpu,
                    driver_version,
                    total_ram_gb: (total_ram_gb * 10.0).round() / 10.0,
                    total_vram_gb: (total_vram_gb * 10.0).round() / 10.0,
                }
            })
        } else {
            None
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Some(DiagnosticPayload {
            event_type: event.event_type.clone(),
            sanitized_message,
            sanitized_stack_trace,
            timestamp,
            hardware: hw_summary,
            metadata: event.metadata.clone(),
        })
    }
}

pub fn sanitize_text(input: &str) -> String {
    let rules: &[(&str, &str)] = &[
        // User directories (keep the drive letter).
        (r"(?i)\b([a-z]):\\Users\\[^\\/\s]+", r"$1:\Users\<USER>"),
        (r"/home/[^/\s]+", "/home/<USER>"),
        (r"/Users/[^/\s]+", "/Users/<USER>"),
        // Bearer tokens and key=value secrets.
        (
            r"(?i)(Bearer\s+|api[_-]?key[\s:=]+|--api-key\s+|token[\s:=]+)[A-Za-z0-9_\-\.]{8,}",
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

    #[test]
    fn test_sanitize_windows_user_path() {
        let raw = r"Error reading file at C:\Users\alice\Documents\Harness Studio\models\model.gguf: access denied";
        let sanitized = sanitize_text(raw);
        assert!(!sanitized.contains("alice"));
        assert!(sanitized.contains(r"C:\Users\<USER>\"));
    }

    #[test]
    fn test_sanitize_unix_user_path() {
        let raw = "Failed to open /home/developer/models/model.gguf";
        let sanitized = sanitize_text(raw);
        assert!(!sanitized.contains("developer"));
        assert!(sanitized.contains("/home/<USER>/"));
    }

    #[test]
    fn test_sanitize_api_keys() {
        let raw = "Sending request with Bearer hf_1234567890abcdef1234567890 to API";
        let sanitized = sanitize_text(raw);
        assert!(!sanitized.contains("hf_1234567890"));
        assert!(sanitized.contains("[REDACTED_KEY]"));
    }

    #[test]
    fn test_privacy_gate_consent_enforcement() {
        let mgr = DiagnosticsManager::new(std::env::temp_dir().join("harness_diag_gate_test"));
        let event = DiagnosticEvent {
            event_type: "crash".to_string(),
            message: r"Crash in C:\Users\johndoe\app.exe".to_string(),
            stack_trace: None,
            metadata: None,
        };

        // 1. Without onboarding completed -> None
        let mut settings = AppSettings::default();
        settings.privacy.onboarding_completed = false;
        assert!(mgr.build_payload(&event, &settings, None).is_none());

        // 2. Onboarding completed, but crash_reports == false -> None
        settings.privacy.onboarding_completed = true;
        settings.privacy.crash_reports = false;
        assert!(mgr.build_payload(&event, &settings, None).is_none());

        // 3. crash_reports == true, but hardware_info == false
        settings.privacy.crash_reports = true;
        settings.privacy.hardware_info = false;
        let payload = mgr
            .build_payload(&event, &settings, None)
            .expect("Payload expected");
        assert!(payload.hardware.is_none());
        assert!(!payload.sanitized_message.contains("johndoe"));
        assert!(payload.sanitized_message.contains("<USER>"));
    }

    #[test]
    fn test_sanitize_more_secrets_keeps_drive() {
        let raw = r"D:\Users\alice\x.gguf token=hf_abcdefghijklmnopqrstuvwxyz12 mail bob@example.com sk-ABCDEFGHIJKLMNOPQRST";
        let s = sanitize_text(raw);
        assert!(s.contains(r"D:\Users\<USER>"), "{}", s);
        assert!(
            !s.contains("alice")
                && !s.contains("hf_abc")
                && !s.contains("bob@")
                && !s.contains("sk-ABC"),
            "{}",
            s
        );
    }

    #[test]
    fn test_offline_crash_lifecycle_and_destruction() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = DiagnosticsManager::new(dir.path());
        let event = DiagnosticEvent {
            event_type: "crash".to_string(),
            message: "Fatal exit at 0xdeadbeef".to_string(),
            stack_trace: Some(r"at C:\Users\alice\main.rs:42".to_string()),
            metadata: None,
        };

        // Case A: consent revoked -> pending dump is deleted, nothing reported.
        mgr.save_offline_crash(&event).unwrap();
        let mut settings = AppSettings::default();
        settings.privacy.onboarding_completed = true;
        settings.privacy.crash_reports = false;
        assert!(mgr
            .process_offline_crash(&settings, None)
            .unwrap()
            .is_empty());
        assert!(
            sorted_json_files(&mgr.pending_dir()).is_empty(),
            "pending dumps must be purged"
        );
        assert!(mgr.list_reports().is_empty());

        // Case B: consent granted -> sanitized report is produced and kept locally.
        mgr.save_offline_crash(&event).unwrap();
        mgr.save_offline_crash(&DiagnosticEvent {
            event_type: "app_error".into(),
            ..event.clone()
        })
        .unwrap();
        settings.privacy.crash_reports = true;
        let processed = mgr.process_offline_crash(&settings, None).unwrap();
        assert_eq!(
            processed.len(),
            1,
            "app_error has no consent and must be dropped"
        );
        assert!(processed[0]
            .sanitized_stack_trace
            .as_ref()
            .unwrap()
            .contains("<USER>"));
        assert_eq!(mgr.list_reports().len(), 1);

        mgr.clear_reports();
        assert!(mgr.list_reports().is_empty());
    }
}
