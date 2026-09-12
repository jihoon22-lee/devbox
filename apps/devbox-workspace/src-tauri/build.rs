fn main() {
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
                    "execute",
                    "terminal_describe",
                    "terminal_export_message",
                    "terminal_execute",
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

fn helper_digest() {
    use sha2::{Digest, Sha256};
    use std::{fs, path::Path};
    let directory = Path::new("resources/wsl");
    let binary = directory.join("devbox-workspace-wsl");
    let manifest = directory.join("manifest.json");
    println!("cargo:rerun-if-changed=resources/wsl");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    if !binary.exists() && !manifest.exists() {
        return; // Source-only Rust checks have no launchable helper.
    }
    let metadata = fs::symlink_metadata(&manifest).expect("missing WSL helper manifest");
    assert!(
        metadata.is_file() && metadata.len() <= 4096,
        "invalid helper manifest"
    );
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest).unwrap()).expect("invalid helper manifest");
    assert_eq!(value.as_object().unwrap().len(), 6);
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["protocol"], 1);
    assert_eq!(value["target"], "x86_64-unknown-linux-musl");
    let source = value["sourceSha"].as_str().unwrap();
    assert!(
        source.len() == 40
            && source
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    if let Ok(expected) = std::env::var("GITHUB_SHA") {
        assert_eq!(
            source, expected,
            "helper source differs from product source"
        );
    }
    let metadata = fs::symlink_metadata(&binary).expect("missing WSL helper binary");
    assert!(
        metadata.is_file() && (64..=64 * 1024 * 1024).contains(&metadata.len()),
        "invalid helper binary"
    );
    let bytes = fs::read(binary).unwrap();
    assert_eq!(value["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(&bytes[..6], b"\x7fELF\x02\x01");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        value["sha256"].as_str(),
        Some(digest.as_str()),
        "helper digest differs from manifest"
    );
    println!("cargo:rustc-env=DEVBOX_WSL_HELPER_SHA256={digest}");
    println!("cargo:rustc-env=DEVBOX_WSL_HELPER_BYTES={}", bytes.len());
}
