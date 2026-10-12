// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // PDF text extraction child (harness_core::rag::extract): release builds abort on panic,
    // so malformed PDFs are parsed in a separate process that exits here.
    if let Some(code) = harness_core::rag::extract::child_main() {
        std::process::exit(code);
    }
    harness_studio_lib::run()
}
