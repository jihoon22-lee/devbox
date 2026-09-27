include!("../product-shell-tauri/wsl_build_support.rs");
fn main() {
    // Artifact::open is compiled here, not in either consuming application.
    // Pin the same packaged Workspace helper used by Workspace and the agent.
    helper_digest("../../apps/devbox-workspace/src-tauri/resources/wsl");
}
