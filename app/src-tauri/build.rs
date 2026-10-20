#[allow(dead_code)]
#[path = "../../crates/protocol/src/channel.rs"]
mod channel;

fn main() {
    println!("cargo:rerun-if-env-changed=DEVBOX_APP_CHANNEL");
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo:rerun-if-changed=../../crates/protocol/src/channel.rs");
    let selected = std::env::var("DEVBOX_APP_CHANNEL").unwrap_or_else(|_| {
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            "prod"
        } else {
            "dev"
        }
        .into()
    });
    let channel = channel::AppChannel::parse(&selected).expect("invalid DEVBOX_APP_CHANNEL");
    println!("cargo:rustc-env=DEVBOX_COMPILED_CHANNEL={selected}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut config: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string("tauri.conf.json").expect("Tauri config"),
        )
        .expect("valid Tauri config");
        for path in [
            "tauri.conf.json",
            "tauri.windows.conf.json",
            "tauri.accept.conf.json",
            "tauri.dev.conf.json",
        ] {
            println!("cargo:rerun-if-changed={path}");
        }
        if let Ok(windows) = std::fs::read_to_string("tauri.windows.conf.json") {
            merge(
                &mut config,
                serde_json::from_str(&windows).expect("Windows Tauri config"),
            );
        }
        if let Ok(overrides) = std::env::var("TAURI_CONFIG") {
            merge(
                &mut config,
                serde_json::from_str(&overrides).expect("TAURI_CONFIG JSON"),
            );
        }
        let schemes = config
            .pointer("/plugins/deep-link/desktop/schemes")
            .and_then(serde_json::Value::as_array)
            .expect("desktop protocols");
        let schemes: Vec<_> = schemes
            .iter()
            .map(|v| v.as_str().expect("protocol name"))
            .collect();
        assert!(
            channel.config().matches_tauri(
                config["identifier"].as_str().expect("identifier"),
                config["productName"].as_str().expect("productName"),
                &schemes,
            ),
            "compiled channel disagrees with Tauri config; select the matching --config override"
        );
    }
    #[cfg(windows)]
    tauri_build::build();
}

fn merge(target: &mut serde_json::Value, patch: serde_json::Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            merge(
                target.entry(key).or_insert(serde_json::Value::Null),
                value.clone(),
            );
        }
    } else {
        *target = patch;
    }
}
