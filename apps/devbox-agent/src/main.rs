#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if std::env::args_os().skip(1).any(|arg| arg == "--mcp-stdio") {
        std::process::exit(devbox_agent::mcp::run_stdio());
    }
    if devbox_agent::run().is_err() {
        std::process::exit(2);
    }
}
