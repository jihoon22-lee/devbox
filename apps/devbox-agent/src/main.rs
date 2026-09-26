#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if devbox_agent::run().is_err() {
        std::process::exit(2);
    }
}
