include!("../../crates/product-shell-tauri/build_support.rs");
fn main() {
    let _staging = lock_bundle_staging();
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    // Keep native library tests under the same Common Controls v6 activation
    // context as the application. Tauri's default resource covers binaries only.
    let windows = if windows_msvc {
        tauri_build::WindowsAttributes::new_without_app_manifest()
    } else {
        tauri_build::WindowsAttributes::new()
    };
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to prepare Devbox Agent");
    if windows_msvc {
        let manifest = std::env::current_dir()
            .expect("Agent crate directory")
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
