// Prevents an additional console window on Windows in release (desktop runs of the mobile UI).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    harness_mobile_lib::run()
}
