include!("../../../crates/product-shell-tauri/wsl_build_support.rs");
include!("../../../crates/product-shell-tauri/build_support.rs");
fn main() {
    let _bundle_staging = lock_bundle_staging();
    helper_digest();
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    // Tauri's resource compiler links its manifest to binaries only. The native
    // library tests also retain rfd's TaskDialogIndirect import and need v6.
    let windows = if windows_msvc {
        tauri_build::WindowsAttributes::new_without_app_manifest()
    } else {
        tauri_build::WindowsAttributes::new()
    };
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(windows)
            .plugin(
                "suite",
                tauri_build::InlinedPlugin::new().commands(&["connection"]),
            )
            .plugin(
                "product-shell",
                tauri_build::InlinedPlugin::new().commands(&["describe", "route_status"]),
            )
            .plugin(
                "workspace",
                tauri_build::InlinedPlugin::new().commands(&[
                    "files",
                    "lsp",
                    "source",
                    "registry",
                    "setup",
                    "definitions",
                    "dependencies",
                    "runtime",
                    "processes",
                    "process_actions",
                    "logs",
                    "terminal",
                    "problems",
                    "commands",
                    "terminal_describe",
                    "terminal_execute",
                    "terminal_output_stream",
                ]),
            ),
    )
    .expect("failed to generate Workspace capabilities");
    if windows_msvc {
        // Apply the same manifest to the application and native test targets.
        // https://github.com/tauri-apps/tauri/blob/dev/examples/api/src-tauri/build.rs
        let manifest = std::env::current_dir()
            .expect("Workspace crate directory")
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
