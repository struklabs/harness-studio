use crate::app_update::{AppUpdateInfo, AppUpdateProgress, AppUpdateState};
use crate::benchmark::{BenchmarkManager, BenchmarkRequest, BenchmarkRun, BenchmarkStatus};
use crate::capabilities::features::FeatureStatus;
use crate::capabilities::verify::EffectiveLaunchValues;
use crate::capabilities::{RuntimeCapabilities, RuntimeFlag};
use crate::config::{AppSettings, RuntimeMode};
use crate::diagnostics::{DiagnosticEvent, DiagnosticPayload, DiagnosticsManager};
use crate::downloader::{
    DownloadManager, DownloadProgress, HfAccess, HfModelCard, HfModelSearchResult, HfModelVariant,
    HfSearchOptions, HfSearchPage, ModelArtifactUpdate,
};
use crate::gguf::{read_gguf_metadata, GgufMetadata};
use crate::hardware::{detect_hardware, SystemHardwareInfo};
use crate::integrations::{
    apply_integration as run_registry_apply, detect_all_clients,
    list_integrations as run_list_integrations, preview_integration as run_preview_integration,
    revert_integration as run_revert_integration, ApplyRequest, ClientStatus, FileChange,
    IntegrationInfo,
};
use crate::models::{LocalModel, ModelManager, ModelPreset};
use crate::operation_gate::OperationGate;
use crate::orchestrator::{
    ensure_model_not_in_use, ensure_server_stopped, launch, resolve_server_binary, server,
    server_binary, server_binary_for,
};
use crate::params::ParamCatalog;
use crate::process::{ActiveServerInfo, LaunchConfig, ServerStatus, TimedLogLine};
use crate::runtime::{
    CustomBinaryValidation, ReleaseAssetInfo, RuntimeDownloadProgress, RuntimeManager,
};
use crate::runtime_update::{AutoUpdateStatus, RuntimeAutoUpdater};
use crate::servers::{DefaultServer, ServerProfile, ServerRegistry, ServerSummary};
use crate::state::{
    AppendMessage, Conversation, ConversationPatch, ConversationSummary, ConversationView, Folder,
    LinearImport, ListQuery, MarkdownLabels, NewConversation, PathMessage, PlaygroundStateImport,
    PlaygroundStateImportResult, StateDb, StateError, StateResult, VariantInput,
};
use crate::storage::{StorageDirInfo, StorageManager};
use crate::telemetry::{TelemetryHistoryPoint, TelemetryPoller, TelemetrySnapshot};
use crate::{
    benchmark, capabilities, desktop, diagnostics, importer, mcp, network, params, paths, secrets,
    server_logs, servers,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Emitter, State};

/// Runs blocking work (disk scans, file copies, process probes) off the main thread
/// so the UI never freezes.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| {
        harness_core::error_key::err("task.join_failed")
            .with("error", e)
            .into()
    })
}

include!("office.rs");
include!("analytics.rs");
include!("benchmark.rs");
include!("control_api.rs");
include!("gateway.rs");
include!("link.rs");
include!("devices.rs");
include!("cloud.rs");
include!("modelfile.rs");
include!("conversations.rs");
include!("projects.rs");
include!("taskruns.rs");
include!("compaction.rs");
include!("rag.rs");
include!("knowledge.rs");
include!("tools.rs");
include!("capabilities.rs");
include!("desktop.rs");
include!("diagnostics.rs");
include!("domain.rs");
include!("hardware.rs");
include!("downloads.rs");
include!("integrations.rs");
include!("harness_mcp.rs");
include!("library.rs");
include!("hub.rs");
include!("github_discovery.rs");
include!("logs.rs");
include!("mcp.rs");
include!("network.rs");
include!("models.rs");
include!("runtime.rs");
include!("runtimes.rs");
include!("servers.rs");
include!("definition_prepare.rs");
include!("settings.rs");
include!("telemetry.rs");
include!("tts.rs");
include!("asr.rs");
include!("lemonade.rs");
include!("convert.rs");
include!("training.rs");
include!("pyenv.rs");
include!("otel.rs");
include!("automations.rs");
include!("comfyui.rs");
include!("permissions.rs");
include!("workload.rs");

include!("modules.rs");
include!("setup.rs");
