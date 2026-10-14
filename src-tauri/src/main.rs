// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // First, before any thread: what AppRun set for interpreters the app does not embed is not
    // passed on to the processes Harness starts (Linux AppImage only).
    harness_studio_lib::app_update::scrub_appimage_env();
    // PDF text extraction child (harness_core::rag::extract): release builds abort on panic,
    // so malformed PDFs are parsed in a separate process that exits here.
    if let Some(code) = harness_core::rag::extract::child_main() {
        std::process::exit(code);
    }
    harness_studio_lib::run()
}
