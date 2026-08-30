//! Privacy-safe Workbench profile snapshot producer.
//!
//! The profile store remains Workbench's source of truth.  This module only
//! publishes the minimum metadata the Launcher needs to search for and route
//! back to a profile: an existing opaque profile id and a safe display label.
//! Project paths, environment metadata, service ids, and other profile
//! details never cross the integration boundary.

use crate::commands::workspace::{load_store_document, ProfileStoreState};
use crate::core::profile::{validate_profile_id, ProfileStore, ProjectProfile, MAX_PROFILES};
use devbox_applink::contains_sensitive_value;
use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use tauri::AppHandle;

const PRODUCER_ID: &str = "workbench";
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const PROFILES_VIEW_KIND: &str = "profiles";
pub const PROFILES_VIEW_SCHEMA_VERSION: u32 = 1;
const MAX_PROFILE_LABEL_BYTES: usize = 256;
const PROFILE_LABEL_FALLBACK: &str = "Workbench 프로필";
const PROFILE_DETAIL: &str = "Workbench · profile";
const SNAPSHOT_ERROR: &str = "Workbench profile snapshot을 만들 수 없습니다";
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileEntry {
    id: String,
    label: String,
    detail: &'static str,
    target_app: &'static str,
    target_kind: &'static str,
    payload_version: u32,
    payload: ProfilePayload,
}

#[derive(Debug, Clone, Serialize)]
struct ProfilePayload {
    id: String,
}

/// Publish the current validated profile store to the default integration
/// root.  CRUD commands call this only after their primary operation has
/// succeeded; a publication error is intentionally isolated from that result.
pub(crate) fn publish_profiles_best_effort(store: &ProfileStore) {
    if let Err(error) = write_profiles(store) {
        eprintln!("workbench integration snapshot 실패: {error}");
    }
}

pub(crate) fn write_profiles(store: &ProfileStore) -> Result<(), String> {
    write_profiles_in(&devbox_integration::integration_root(), store)
}

/// Refresh the durable profile projections while Workbench is running. The
/// producer stops naturally with the process; Port Manager then marks the
/// last sidecar stale after its own bounded freshness window.
pub(crate) fn spawn_profile_snapshot_writer(app: AppHandle, store_state: Arc<ProfileStoreState>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let result = match store_state.lock.lock() {
                Ok(_store_lock) => {
                    load_store_document(&app).and_then(|document| write_profiles(&document.store))
                }
                Err(_) => Err(SNAPSHOT_ERROR.to_owned()),
            };
            if let Err(error) = result {
                eprintln!("workbench integration snapshot 실패: {error}");
            }
            tokio::time::sleep(SNAPSHOT_INTERVAL).await;
        }
    });
}

fn write_profiles_in(root: &Path, store: &ProfileStore) -> Result<(), String> {
    let envelope = build_profiles_envelope(store)?;
    devbox_integration::write_named_view_snapshot_atomic(&envelope, root, PROFILES_VIEW_KIND)?;
    write_port_bindings_in(root, store)
}

fn write_port_bindings_in(root: &Path, store: &ProfileStore) -> Result<(), String> {
    let entries = build_port_binding_entries(store)?;
    let envelope =
        devbox_integration::port_bindings_envelope(PRODUCER_ID, env!("CARGO_PKG_VERSION"), entries)
            .map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    devbox_integration::write_named_view_snapshot_atomic(
        &envelope,
        root,
        devbox_integration::PORT_BINDINGS_VIEW_KIND,
    )
}

fn build_port_binding_entries(
    store: &ProfileStore,
) -> Result<Vec<devbox_integration::PortBindingEntry>, String> {
    store.validate().map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    if store.profiles.len() > MAX_PROFILES {
        return Err(SNAPSHOT_ERROR.to_owned());
    }
    let mut entries = Vec::new();
    for profile in &store.profiles {
        if !valid_opaque_id(&profile.id) {
            return Err(SNAPSHOT_ERROR.to_owned());
        }
        let label = public_label(profile);
        for port in &profile.expected_ports {
            entries.push(devbox_integration::PortBindingEntry::WorkbenchProfile {
                id: profile.id.clone(),
                label: label.clone(),
                port: *port,
            });
        }
    }
    entries.sort_by(|left, right| match (left, right) {
        (
            devbox_integration::PortBindingEntry::WorkbenchProfile {
                id: left_id,
                port: left_port,
                ..
            },
            devbox_integration::PortBindingEntry::WorkbenchProfile {
                id: right_id,
                port: right_port,
                ..
            },
        ) => (left_id, left_port).cmp(&(right_id, right_port)),
        _ => std::cmp::Ordering::Equal,
    });
    devbox_integration::validate_port_binding_entries(&entries)
        .map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    Ok(entries)
}

fn build_profiles_envelope(store: &ProfileStore) -> Result<Envelope, String> {
    let entries = build_profile_entries(store)?;
    let mut views = SnapshotViews::new();
    views.insert(
        PROFILES_VIEW_KIND.to_owned(),
        SnapshotView {
            schema_version: PROFILES_VIEW_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    );
    let envelope = Envelope::with_views(PRODUCER_ID, env!("CARGO_PKG_VERSION"), views);
    if envelope.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(SNAPSHOT_ERROR.to_owned());
    }
    Ok(envelope)
}

fn build_profile_entries(store: &ProfileStore) -> Result<Vec<serde_json::Value>, String> {
    // Validate the complete store before projecting it.  This keeps a
    // malformed store from replacing a previously good Launcher snapshot.
    store.validate().map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    if store.profiles.len() > MAX_PROFILES {
        return Err(SNAPSHOT_ERROR.to_owned());
    }

    let mut ids = BTreeSet::new();
    let mut entries = Vec::with_capacity(store.profiles.len());
    for profile in &store.profiles {
        if !valid_opaque_id(&profile.id) || !ids.insert(profile.id.clone()) {
            return Err(SNAPSHOT_ERROR.to_owned());
        }

        let entry = ProfileEntry {
            id: profile.id.clone(),
            label: public_label(profile),
            detail: PROFILE_DETAIL,
            target_app: PRODUCER_ID,
            target_kind: "profile",
            payload_version: 1,
            payload: ProfilePayload {
                id: profile.id.clone(),
            },
        };
        entries.push(serde_json::to_value(entry).map_err(|_| SNAPSHOT_ERROR.to_owned())?);
    }

    entries.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });
    Ok(entries)
}

fn public_label(profile: &ProjectProfile) -> String {
    let candidate = profile.name.trim();
    if valid_public_text(candidate)
        && !contains_sensitive_value(candidate)
        && !looks_like_path(candidate)
        && !looks_like_environment(candidate)
    {
        return candidate.to_owned();
    }
    PROFILE_LABEL_FALLBACK.to_owned()
}

fn valid_opaque_id(value: &str) -> bool {
    validate_profile_id(value).is_ok()
        && !contains_sensitive_value(value)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_public_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROFILE_LABEL_BYTES
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// Profile names are labels, not paths.  Reject absolute, UNC, relative, and
/// file-URI forms so a path cannot be copied into the Launcher index.
fn looks_like_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("//")
        || value.starts_with("\\\\")
        || value.starts_with("~/")
        || value.starts_with("~\\")
        || value.starts_with("./")
        || value.starts_with(".\\")
        || value.starts_with("../")
        || value.starts_with("..\\")
        || value
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// Avoid exporting environment-shaped labels such as `PATH=/project` or
/// `${PRIVATE_ROOT}` even when the value is not recognizable as a credential.
fn looks_like_environment(value: &str) -> bool {
    if value.starts_with('$') || value.contains("${") {
        return true;
    }
    let Some((name, assigned)) = value.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !assigned.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::{ProfileStore, WslProfile};
    use devbox_integration::{
        named_view_snapshot_path_in, read_named_view_snapshot_in, snapshot_path_in,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

    fn test_root(label: &str) -> PathBuf {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "workbench-profile-snapshot-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn profile(id: &str, name: &str, path: &str) -> ProjectProfile {
        let mut profile = ProjectProfile::new(name);
        profile.id = id.to_owned();
        profile.windows_path = Some(path.to_owned());
        profile
    }

    fn store(profiles: Vec<ProjectProfile>) -> ProfileStore {
        ProfileStore {
            version: crate::core::profile::PROFILE_VERSION,
            profiles,
        }
    }

    #[test]
    fn projection_contains_only_safe_profile_action_metadata() {
        let mut first = profile("profile-z", "Safe profile", "C:/private/project");
        first.git_root = Some("C:/private/project/.git".to_owned());
        first.wsl = Some(WslProfile {
            distro: "Ubuntu".to_owned(),
            path: "/home/private/project".to_owned(),
        });
        first.run_manager_service_ids = vec!["PRIVATE_TOKEN".to_owned()];

        let envelope = build_profiles_envelope(&store(vec![first])).unwrap();
        let entry = &envelope.views().unwrap()[PROFILES_VIEW_KIND].entries[0];
        assert_eq!(entry["id"], "profile-z");
        assert_eq!(entry["label"], "Safe profile");
        assert_eq!(entry["detail"], PROFILE_DETAIL);
        assert_eq!(entry["targetApp"], PRODUCER_ID);
        assert_eq!(entry["targetKind"], "profile");
        assert_eq!(entry["payloadVersion"], 1);
        assert_eq!(entry["payload"], serde_json::json!({"id": "profile-z"}));
        assert_eq!(
            entry.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec![
                "detail",
                "id",
                "label",
                "payload",
                "payloadVersion",
                "targetApp",
                "targetKind"
            ]
        );

        let serialized = serde_json::to_string(&envelope).unwrap();
        for private in [
            "C:/private/project",
            "C:/private/project/.git",
            "/home/private/project",
            "PRIVATE_TOKEN",
            "run_manager_service_ids",
        ] {
            assert!(!serialized.contains(private), "leaked {private}");
        }
    }

    #[test]
    fn sensitive_path_and_environment_labels_use_static_fallback() {
        for (name, expected) in [
            ("SECRET=TOP_SECRET", PROFILE_LABEL_FALLBACK),
            ("PATH=/home/private/project", PROFILE_LABEL_FALLBACK),
            ("C:\\private\\project", PROFILE_LABEL_FALLBACK),
            ("${PRIVATE_ROOT}", PROFILE_LABEL_FALLBACK),
        ] {
            let envelope =
                build_profiles_envelope(&store(vec![profile("profile-1", name, "C:/project")]))
                    .unwrap();
            assert_eq!(
                envelope.views().unwrap()[PROFILES_VIEW_KIND].entries[0]["label"],
                expected
            );
        }
    }

    #[test]
    fn expected_ports_publish_only_profile_navigation_metadata() {
        let mut first = profile("profile-z", "Frontend", "C:/private/project");
        first.git_root = Some("C:/private/project/.git".into());
        first.expected_ports = vec![5173, 3000];
        let entries = build_port_binding_entries(&store(vec![first])).unwrap();
        assert_eq!(
            entries,
            vec![
                devbox_integration::PortBindingEntry::WorkbenchProfile {
                    id: "profile-z".into(),
                    label: "Frontend".into(),
                    port: 3000,
                },
                devbox_integration::PortBindingEntry::WorkbenchProfile {
                    id: "profile-z".into(),
                    label: "Frontend".into(),
                    port: 5173,
                },
            ]
        );
        let serialized = serde_json::to_string(&entries).unwrap();
        for private in ["C:/private/project", ".git", "windowsPath", "gitRoot"] {
            assert!(!serialized.contains(private));
        }
    }

    #[test]
    fn projection_is_bounded_by_profile_store_limit() {
        let bounded = store(
            (0..MAX_PROFILES)
                .map(|index| {
                    profile(
                        &format!("profile-{index}"),
                        &format!("Profile {index}"),
                        &format!("C:/projects/{index}"),
                    )
                })
                .collect(),
        );
        let envelope = build_profiles_envelope(&bounded).unwrap();
        assert_eq!(
            envelope.views().unwrap()[PROFILES_VIEW_KIND].entries.len(),
            MAX_PROFILES
        );

        let oversized = store(
            (0..=MAX_PROFILES)
                .map(|index| {
                    profile(
                        &format!("profile-{index}"),
                        &format!("Profile {index}"),
                        &format!("C:/projects/{index}"),
                    )
                })
                .collect(),
        );
        assert_eq!(
            build_profiles_envelope(&oversized).unwrap_err(),
            SNAPSHOT_ERROR
        );

        let invalid_id = store(vec![profile("/private/path", "Profile", "C:/project")]);
        assert_eq!(
            build_profiles_envelope(&invalid_id).unwrap_err(),
            SNAPSHOT_ERROR
        );
    }

    #[test]
    fn writes_profiles_to_atomic_named_snapshot_filename() {
        let root = test_root("atomic");
        let store = store(vec![profile("profile-1", "Profile", "C:/project")]);
        write_profiles_in(&root, &store).unwrap();

        let expected =
            named_view_snapshot_path_in(&root, PRODUCER_ID, 1, PROFILES_VIEW_KIND).unwrap();
        assert_eq!(
            expected,
            root.join("workbench").join("v1").join("profiles.json")
        );
        assert!(expected.is_file());
        assert!(!snapshot_path_in(&root, PRODUCER_ID, 1).exists());

        let read = read_named_view_snapshot_in(&root, PRODUCER_ID, 1, PROFILES_VIEW_KIND)
            .unwrap()
            .unwrap();
        assert_eq!(read, build_profiles_envelope(&store).unwrap());
        let port_bindings = read_named_view_snapshot_in(
            &root,
            PRODUCER_ID,
            1,
            devbox_integration::PORT_BINDINGS_VIEW_KIND,
        )
        .unwrap()
        .unwrap();
        assert!(
            devbox_integration::port_bindings_from_envelope(&port_bindings)
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
