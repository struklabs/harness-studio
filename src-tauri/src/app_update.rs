//! Self-update of Harness Studio via the Tauri updater (signed `latest.json`
//! published by the release workflow).

use harness_core::error_key::err;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize)]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AppUpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub finished: bool,
}

#[derive(Clone, Default)]
pub struct AppUpdateState {
    progress: Arc<Mutex<Option<AppUpdateProgress>>>,
}

fn endpoint_configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("endpoints"))
        .and_then(|e| e.as_array())
        .map(|list| {
            list.iter()
                .any(|v| v.as_str().is_some_and(|url| !url.trim().is_empty()))
        })
        .unwrap_or(false)
}

fn not_configured() -> String {
    err("update.not_configured").into()
}

fn check_failed(e: impl std::fmt::Display) -> String {
    err("update.check_failed").with("error", e).into()
}

impl AppUpdateState {
    pub fn progress(&self) -> Option<AppUpdateProgress> {
        self.progress.lock().unwrap().clone()
    }

    pub async fn check(&self, app: &AppHandle) -> Result<Option<AppUpdateInfo>, String> {
        if !endpoint_configured(app) {
            return Err(not_configured());
        }
        let update = app
            .updater()
            .map_err(check_failed)?
            .check()
            .await
            .map_err(check_failed)?;
        Ok(update.map(|u| AppUpdateInfo {
            current_version: u.current_version.clone(),
            version: u.version.clone(),
            notes: u.body.clone(),
            date: u.date.map(|d| d.to_string()),
        }))
    }

    /// Downloads, verifies (signature) and installs the update, then restarts the app.
    /// Running servers are stopped first so the new version starts cleanly.
    pub async fn install(
        &self,
        app: &AppHandle,
        registry: &crate::servers::ServerRegistry,
    ) -> Result<(), String> {
        if !endpoint_configured(app) {
            return Err(not_configured());
        }
        let update = app
            .updater()
            .map_err(check_failed)?
            .check()
            .await
            .map_err(check_failed)?
            .ok_or_else(|| String::from(err("update.none_available")))?;

        *self.progress.lock().unwrap() = Some(AppUpdateProgress::default());
        let progress = self.progress.clone();
        let result = update
            .download_and_install(
                move |chunk, total| {
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.downloaded += chunk as u64;
                        p.total = total;
                    }
                },
                || {},
            )
            .await;
        if let Err(e) = result {
            *self.progress.lock().unwrap() = None;
            return Err(err("update.install_failed").with("error", e).into());
        }
        if let Some(p) = self.progress.lock().unwrap().as_mut() {
            p.finished = true;
        }
        registry.stop_all().await;
        app.restart();
    }
}
