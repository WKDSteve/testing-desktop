// The desktop entry of the shell: a preview for a developer (keys in memory, no Keystore). The phone starts at
// `run` through Tauri's mobile entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    testing_mobile_lib::run();
}
