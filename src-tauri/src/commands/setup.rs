
// First-run Setup window (src/lib/components/SetupWindow.svelte): facts only the backend knows.

/// File the Windows installer leaves next to the app when it ran silently (`/S`): a silent install
/// is a scripted one, so the first launch must not open Setup.
const SILENT_INSTALL_MARKER: &str = "setup-skipped";

#[derive(serde::Serialize)]
pub(crate) struct SetupContext {
    /// The installer ran silently: do not open Setup on its own.
    skip_first_run: bool,
    /// The data root of this run.
    data_dir: String,
    /// The data root comes from `HARNESS_DATA_DIR`, so it cannot be changed from the window.
    data_dir_fixed: bool,
}

fn silent_install_marker(install_dir: &Path) -> bool {
    install_dir.join(SILENT_INSTALL_MARKER).is_file()
}

#[tauri::command]
pub(crate) fn setup_context() -> SetupContext {
    let skip = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(silent_install_marker))
        .unwrap_or(false);
    SetupContext {
        skip_first_run: skip,
        data_dir: paths::data_root().to_string_lossy().into_owned(),
        data_dir_fixed: std::env::var_os("HARNESS_DATA_DIR").is_some_and(|v| !v.is_empty()),
    }
}

#[cfg(test)]
mod setup_tests {
    use super::*;

    #[test]
    fn silent_install_marker_is_read_from_the_install_folder() {
        let dir = std::env::temp_dir().join(format!("harness-setup-marker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!silent_install_marker(&dir));
        std::fs::write(dir.join(SILENT_INSTALL_MARKER), b"").unwrap();
        assert!(silent_install_marker(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
