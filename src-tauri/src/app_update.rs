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

fn check_failed(e: tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match e {
        // The release has no package for this system (yet): not a failed check.
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => err("update.no_platform").into(),
        e => err("update.check_failed").with("error", e).into(),
    }
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
        // AppImage: the new version is started once this one has exited, with the environment the
        // old AppRun set up removed (see `relaunch_appimage_after_exit`).
        #[cfg(target_os = "linux")]
        if appimage::running_from_appimage() {
            appimage::RELAUNCH.store(true, std::sync::atomic::Ordering::SeqCst);
            app.exit(0);
            return Ok(());
        }
        app.restart();
    }
}

/// Starts the updated AppImage after the app has exited, if an update asked for it. Called from
/// the `RunEvent::Exit` handler, after the cleanup.
#[cfg(target_os = "linux")]
pub fn relaunch_appimage_after_exit() {
    if appimage::RELAUNCH.load(std::sync::atomic::Ordering::SeqCst) {
        appimage::relaunch();
    }
}

/// Linux AppImage relaunch. Tauri's `restart` starts the new AppImage with this process's
/// environment and working directory, both set up by the old AppRun: `PATH`, `LD_LIBRARY_PATH`,
/// `XDG_DATA_DIRS`, `GST_PLUGIN_SYSTEM_PATH`... point into the old version's mount and the working
/// directory is inside it. The new version would then load modules from the old one, and the old
/// mount (with its runtime process) would stay alive as long as the new version runs. The runtime
/// also hands the app descriptors that keep its mount alive while they are open (the mount
/// directory as fd 1023 and the read end of the FUSE process's keepalive pipe), inherited by
/// children on purpose so that sidecars keep the mount; the new version must not inherit them.
#[cfg(target_os = "linux")]
mod appimage {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    pub static RELAUNCH: AtomicBool = AtomicBool::new(false);

    /// Set by the AppImage runtime for the process it starts; they are set again on the relaunch.
    const RUNTIME_VARS: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

    pub fn running_from_appimage() -> bool {
        std::env::var_os("APPIMAGE").is_some() && std::env::var_os("APPDIR").is_some()
    }

    pub fn relaunch() {
        let (Some(appimage), Some(appdir)) =
            (std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR"))
        else {
            return;
        };
        let appdir = PathBuf::from(appdir);
        // The directory the AppImage was started from (`OWD`), unless it is inside the mount.
        let cwd = std::env::var_os("OWD")
            .map(PathBuf::from)
            .filter(|d| !inside(d.as_os_str(), &appdir) && d.is_dir())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        close_descriptors_on_exec();
        let result = std::process::Command::new(&appimage)
            .args(std::env::args_os().skip(1))
            .env_clear()
            .envs(clean_env(std::env::vars_os(), &appdir))
            .current_dir(cwd)
            .spawn();
        if let Err(e) = result {
            eprintln!("failed to start the updated AppImage: {e}");
        }
    }

    /// Marks every descriptor above stderr close-on-exec, so the relaunched AppImage inherits none
    /// of this process's (among them the old mount's keepalive descriptors). This process exits
    /// right after the relaunch.
    fn close_descriptors_on_exec() {
        let Ok(entries) = std::fs::read_dir("/proc/self/fd") else {
            return;
        };
        let fds: Vec<i32> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok()))
            .filter(|fd| *fd > 2)
            .collect();
        for fd in fds {
            // SAFETY: fcntl(F_GETFD/F_SETFD) only changes the descriptor's flags; an fd that is
            // no longer open (such as the one read_dir used) makes it fail with EBADF, ignored.
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
        }
    }

    /// Whether `path` is `appdir` or lies inside it.
    fn inside(path: &OsStr, appdir: &Path) -> bool {
        Path::new(path).starts_with(appdir)
    }

    /// The environment without what points into `appdir`: list variables (`a:b:c`) lose those
    /// entries (and duplicates), other variables with such a value are removed, as are the
    /// AppImage runtime's own variables.
    pub(super) fn clean_env(
        vars: impl Iterator<Item = (OsString, OsString)>,
        appdir: &Path,
    ) -> Vec<(OsString, OsString)> {
        let points_inside = |part: &[u8]| inside(OsStr::from_bytes(part), appdir);
        vars.filter(|(k, _)| !RUNTIME_VARS.iter().any(|r| k == *r))
            .filter_map(|(k, v)| {
                let bytes = v.as_bytes();
                if !bytes.split(|b| *b == b':').any(points_inside) {
                    return Some((k, v));
                }
                let mut kept: Vec<&[u8]> = Vec::new();
                for part in bytes.split(|b| *b == b':') {
                    if !part.is_empty() && !points_inside(part) && !kept.contains(&part) {
                        kept.push(part);
                    }
                }
                (!kept.is_empty()).then(|| (k, OsString::from_vec(kept.join(&b':'))))
            })
            .collect()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::appimage::clean_env;
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn relaunch_env_drops_the_old_mount() {
        let old = Path::new("/tmp/.mount_hsAbc");
        let vars = [
            (
                "PATH",
                "/tmp/.mount_hsAbc/usr/bin/:/tmp/.mount_hsAbc/bin/:/usr/local/bin:/usr/bin",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_hsAbc/usr/share:/usr/share:/usr/share:/usr/local/share",
            ),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_hsAbc/usr/lib/:/tmp/.mount_hsAbc/lib/:",
            ),
            ("GTK_DATA_PREFIX", "/tmp/.mount_hsAbc"),
            (
                "GDK_PIXBUF_MODULE_FILE",
                "/tmp/.mount_hsAbc//usr/lib/loaders.cache",
            ),
            ("PWD", "/tmp/.mount_hsAbc/usr"),
            ("APPDIR", "/tmp/.mount_hsAbc"),
            ("APPIMAGE", "/home/u/Apps/hs.AppImage"),
            ("OWD", "/home/u"),
            ("HOME", "/home/u"),
            ("OTHER", "/tmp/.mount_hsAbcd/kept"),
            ("GST_REGISTRY_REUSE_PLUGIN_SCANNER", "no"),
        ]
        .into_iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let env: Vec<(String, String)> = clean_env(vars, old)
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect();
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("PATH"), Some("/usr/local/bin:/usr/bin"));
        assert_eq!(get("XDG_DATA_DIRS"), Some("/usr/share:/usr/local/share"));
        for gone in [
            "LD_LIBRARY_PATH",
            "GTK_DATA_PREFIX",
            "GDK_PIXBUF_MODULE_FILE",
            "PWD",
            "APPDIR",
            "APPIMAGE",
            "OWD",
        ] {
            assert_eq!(get(gone), None, "{gone}");
        }
        assert_eq!(get("HOME"), Some("/home/u"));
        // A path that only shares the prefix is not inside the mount.
        assert_eq!(get("OTHER"), Some("/tmp/.mount_hsAbcd/kept"));
        assert_eq!(get("GST_REGISTRY_REUSE_PLUGIN_SCANNER"), Some("no"));
    }
}
