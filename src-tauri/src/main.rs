#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    openpup_tauri::run();
}
