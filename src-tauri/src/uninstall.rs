//! Uninstall mode: `harness-studio --uninstall [--installer] [--wait-pid N]`.
//!
//! A deliberately small app: no state database, no servers, no gateway and no instance lock, so
//! nothing here holds a file the removal needs. The window shows the `/uninstall` screens; the
//! removal itself is `harness_core::cleanup::execute_selection`, with the same safety checks as
//! `harness cleanup`. With `--installer` the OS uninstaller (NSIS) started this process and waits
//! for its exit code: 0 = continue removing app files, 2 = the user cancelled.
use harness_core::cleanup::{self, Selection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub const EXIT_DONE: i32 = 0;
pub const EXIT_CANCELLED: i32 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub installer: bool,
    pub wait_pid: Option<u32>,
}

/// `Some` when the process was started in uninstall mode.
pub fn parse_args() -> Option<Options> {
    parse(std::env::args().skip(1))
}

fn parse(args: impl IntoIterator<Item = String>) -> Option<Options> {
    let mut options = Options::default();
    let mut enabled = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--uninstall" => enabled = true,
            "--installer" => options.installer = true,
            "--wait-pid" => options.wait_pid = args.next().and_then(|v| v.parse().ok()),
            _ => {}
        }
    }
    enabled.then_some(options)
}

/// How the app was installed decides what the last screen can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum InstallKind {
    WindowsInstalled,
    MacApp,
    LinuxDeb,
    LinuxAppImage,
    Portable,
}

fn install_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

fn mac_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(PathBuf::from)
}

/// Moves the app bundle to the Trash with NSFileManager. Asking Finder through `osascript` needs
/// the Automation permission: macOS prompts for it, and the move fails when it is denied.
#[cfg(target_os = "macos")]
fn move_to_trash(path: &std::path::Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| format!("Moving to Trash failed: {}", e.localizedDescription()))
}

#[cfg(not(target_os = "macos"))]
fn move_to_trash(_path: &std::path::Path) -> Result<(), String> {
    Err("Moving to Trash is only done on macOS".into())
}

fn install_kind() -> InstallKind {
    if cfg!(windows) {
        let uninstaller = install_dir().map(|d| d.join("uninstall.exe"));
        return if uninstaller.is_some_and(|p| p.is_file()) {
            InstallKind::WindowsInstalled
        } else {
            InstallKind::Portable
        };
    }
    if cfg!(target_os = "macos") {
        return if mac_bundle().is_some() {
            InstallKind::MacApp
        } else {
            InstallKind::Portable
        };
    }
    if std::env::var_os("APPIMAGE").is_some() {
        InstallKind::LinuxAppImage
    } else if std::env::current_exe().is_ok_and(|exe| exe.starts_with("/usr")) {
        InstallKind::LinuxDeb
    } else {
        InstallKind::Portable
    }
}

struct UninstallState {
    options: Options,
    identifier: String,
    ran: AtomicBool,
    temp_webview: PathBuf,
}

#[derive(Serialize)]
struct Context {
    installer: bool,
    kind: InstallKind,
    version: String,
    data_dir: PathBuf,
    language: String,
    theme: String,
    color_theme: String,
    cvd_safe: bool,
}

#[tauri::command]
fn uninstall_context(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<UninstallState>>,
) -> Context {
    let settings = harness_core::config::AppSettings::load();
    Context {
        installer: state.options.installer,
        kind: install_kind(),
        version: app.package_info().version.to_string(),
        data_dir: harness_core::paths::data_root(),
        language: settings.language,
        theme: settings.theme,
        color_theme: settings.color_theme,
        cvd_safe: settings.cvd_safe,
    }
}

#[tauri::command]
async fn uninstall_estimate(
    state: tauri::State<'_, Arc<UninstallState>>,
) -> Result<cleanup::Estimate, String> {
    let identifier = state.identifier.clone();
    tauri::async_runtime::spawn_blocking(move || {
        cleanup::estimate(
            &harness_core::paths::data_root(),
            &identifier,
            std::time::Duration::from_millis(700),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stops Harness (5 s at most), then removes the selection, emitting `uninstall-step` events.
#[tauri::command]
async fn uninstall_run(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<UninstallState>>,
    selection: Selection,
) -> Result<(), String> {
    let state = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emit = |step: cleanup::Step| {
            let _ = app.emit("uninstall-step", step);
        };
        let root = harness_core::paths::data_root();
        emit(cleanup::Step::Started {
            step: "stop".into(),
        });
        if let Some(pid) = state.options.wait_pid {
            let _ = cleanup::wait_for_exit(pid);
        }
        let mut owned = vec![root.clone()];
        owned.extend(install_dir());
        let alive = cleanup::stop_processes(&owned, std::time::Duration::from_secs(5));
        if alive == 0 {
            emit(cleanup::Step::Done {
                step: "stop".into(),
            });
        } else {
            emit(cleanup::Step::Failed {
                step: "stop".into(),
                error: format!("{alive} process(es) are still running"),
            });
        }
        // Start at login points into the app files that are about to disappear.
        let _ = crate::commands::set_launch_at_login(app.clone(), false);
        state.ran.store(true, Ordering::SeqCst);
        cleanup::execute_selection(&root, &state.identifier, &selection, &mut |step| emit(step))
    })
    .await
    .map_err(|e| e.to_string())?;
    result
}

/// The last screen's action. Paths come from this process, never from the window.
#[tauri::command]
async fn uninstall_finish() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(finish)
        .await
        .map_err(|e| e.to_string())?
}

fn finish() -> Result<(), String> {
    match install_kind() {
        InstallKind::MacApp => move_to_trash(&mac_bundle().ok_or("App bundle not found")?),
        InstallKind::LinuxDeb => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let found = harness_core::process::hidden_command("dpkg")
                .arg("-S")
                .arg(&exe)
                .output()
                .map_err(|e| e.to_string())?;
            let package = String::from_utf8_lossy(&found.stdout)
                .split(':')
                .next()
                .map(|p| p.trim().to_owned())
                .filter(|p| {
                    !p.is_empty()
                        && p.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'+'))
                })
                .ok_or("Package name not found")?;
            let status = harness_core::process::hidden_command("pkexec")
                .args(["apt-get", "remove", "-y", &package])
                .status()
                .map_err(|e| e.to_string())?;
            status
                .success()
                .then_some(())
                .ok_or("apt remove failed".into())
        }
        InstallKind::LinuxAppImage => {
            let image = PathBuf::from(std::env::var_os("APPIMAGE").ok_or("AppImage path unknown")?);
            std::fs::remove_file(image).map_err(|e| e.to_string())
        }
        InstallKind::WindowsInstalled => {
            let uninstaller = install_dir()
                .ok_or("Install folder unknown")?
                .join("uninstall.exe");
            let mut command = harness_core::process::hidden_command(uninstaller);
            // Silent: this window already asked what to remove; the uninstaller only removes files.
            command.arg("/S");
            // Tells the NSIS hook to skip its plain cleanup: the user already chose what to remove.
            command.env("HARNESS_UNINSTALL_DONE", "1");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            command.spawn().map(|_| ()).map_err(|e| e.to_string())
        }
        InstallKind::Portable => Ok(()),
    }
}

#[tauri::command]
fn uninstall_exit(app: tauri::AppHandle, code: i32) {
    app.exit(code);
}

pub fn run(options: Options, mut context: tauri::Context) {
    // The window is built below, with its own throwaway web view data.
    context.config_mut().app.windows.clear();
    let identifier = context.config().identifier.clone();
    let temp_webview =
        std::env::temp_dir().join(format!("harness-uninstall-{}", std::process::id()));
    let cleanup_dir = temp_webview.clone();
    let state = Arc::new(UninstallState {
        options: options.clone(),
        identifier,
        ran: AtomicBool::new(false),
        temp_webview,
    });
    let result = tauri::Builder::default()
        .plugin(crate::desktop::autostart_plugin())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            uninstall_context,
            uninstall_estimate,
            uninstall_run,
            uninstall_finish,
            uninstall_exit,
        ])
        .setup(move |app| {
            let override_dir = std::env::var_os("HARNESS_DATA_DIR")
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty());
            let data_dir = match &override_dir {
                Some(dir) => dir.clone(),
                None => app.path().app_data_dir()?,
            };
            harness_core::paths::init(data_dir.clone());
            harness_core::secrets::init_service(&app.config().identifier);
            if std::env::var_os("HARNESS_MEMORY_SECRETS").is_some_and(|v| v == "1") {
                harness_core::secrets::use_memory_store();
            }
            if override_dir.is_some() {
                std::fs::write(data_dir.join(".harness-owned"), &app.config().identifier)?;
            }
            let window_state = state.clone();
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("uninstall".into()),
            )
            .title("Uninstall Harness Studio")
            .inner_size(780.0, 700.0)
            .min_inner_size(620.0, 560.0)
            .center()
            .data_directory(state.temp_webview.clone())
            .build()?;
            window.on_window_event(move |event| {
                // Closing the window is a cancel until the removal has started.
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    let code = if window_state.ran.load(Ordering::SeqCst) {
                        EXIT_DONE
                    } else {
                        EXIT_CANCELLED
                    };
                    std::process::exit(code);
                }
            });
            Ok(())
        })
        .build(context);
    match result {
        Ok(app) => {
            let code = app.run_return(|_, _| {});
            let _ = std::fs::remove_dir_all(cleanup_dir);
            std::process::exit(code);
        }
        // The installer falls back to its plain removal when this exits with an error.
        Err(error) => {
            eprintln!("[Harness Studio] uninstall window failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Option<Options> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn uninstall_mode_needs_the_flag() {
        assert_eq!(args(&[]), None);
        assert_eq!(args(&["--installer"]), None);
        assert_eq!(args(&["--uninstall"]), Some(Options::default()));
    }

    #[test]
    fn installer_and_wait_pid_are_read() {
        let options = args(&["--uninstall", "--installer", "--wait-pid", "42"]).unwrap();
        assert!(options.installer);
        assert_eq!(options.wait_pid, Some(42));
        assert_eq!(
            args(&["--uninstall", "--wait-pid", "x"]).unwrap().wait_pid,
            None
        );
    }
}
