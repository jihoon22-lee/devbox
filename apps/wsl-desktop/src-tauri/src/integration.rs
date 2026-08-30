//! WSL Desktop의 Launcher용 `profiles/v1` named snapshot producer.
//!
//! 프로필 저장소는 WSL Desktop만 소유한다. 이 모듈은 Launcher가 프로필을 다시
//! 열 수 있도록 필요한 opaque id와 표시용 metadata만 복제하며, distro·cwd·시작
//! 명령·pane 구성·경로·secret은 integration 경계를 넘기지 않는다.

use crate::core::workspace::{ProfileStore, WorkspaceProfile, MAX_PROFILES};
use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use serde_json::Value;

pub const PRODUCER_ID: &str = "wsl-desktop";
pub const VIEW_KIND: &str = "profiles";
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const PAYLOAD_VERSION: u32 = 1;

const STATIC_PROFILE_LABEL: &str = "WSL Desktop 프로필";
const STATIC_PROFILE_DETAIL: &str = "WSL Desktop · 터미널 프로필";
const MAX_LABEL_BYTES: usize = 256;

/// 검증된 WSL Desktop 프로필 저장소를 Launcher가 읽을 수 있는 envelope로 변환한다.
///
/// `ProfileStore::validate`가 기존 저장소의 전체 구조·id·프로필 상한을 먼저
/// 확인하므로, 하나라도 손상된 프로필이 있으면 부분 snapshot을 만들지 않는다.
pub fn build_profile_snapshot(store: &ProfileStore) -> Result<Envelope, String> {
    store.validate()?;
    if store.profiles.len() > MAX_PROFILES {
        return Err("터미널 프로필 저장소 형식이 올바르지 않습니다".into());
    }

    let view = SnapshotView {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        freshness_ms: 0,
        entries: store
            .profiles
            .iter()
            .map(profile_entry)
            .collect::<Result<Vec<_>, _>>()?,
    };
    let views = SnapshotViews::from([(VIEW_KIND.to_owned(), view)]);
    Ok(Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

/// 현재 프로필 저장소를 공용 integration root에 named sidecar로 발행한다.
///
/// 호출자는 이 결과를 best-effort로 처리한다. snapshot 경로 오류가 발생해도
/// 프로필 저장·삭제 명령의 핵심 결과를 되돌리지 않도록 명령 계층에서 무시한다.
pub fn publish_profile_snapshot(store: &ProfileStore) -> Result<(), String> {
    let envelope = build_profile_snapshot(store)?;
    devbox_integration::write_named_view_snapshot_atomic(
        &envelope,
        &devbox_integration::integration_root(),
        VIEW_KIND,
    )
}

fn profile_entry(profile: &WorkspaceProfile) -> Result<Value, String> {
    if devbox_applink::contains_sensitive_value(&profile.id) {
        return Err("터미널 프로필 snapshot 식별자가 올바르지 않습니다".into());
    }
    let id = profile.id.clone();
    Ok(serde_json::json!({
        "id": id,
        "label": safe_profile_label(&profile.name),
        "detail": STATIC_PROFILE_DETAIL,
        "targetApp": PRODUCER_ID,
        "targetKind": "profile",
        "payloadVersion": PAYLOAD_VERSION,
        "payload": { "id": profile.id },
    }))
}

fn safe_profile_label(name: &str) -> String {
    if name.trim() != name
        || name.is_empty()
        || name.len() > MAX_LABEL_BYTES
        || name.chars().any(char::is_control)
        || devbox_applink::contains_sensitive_value(name)
        || looks_like_path(name)
        || looks_like_environment(name)
    {
        STATIC_PROFILE_LABEL.to_owned()
    } else {
        name.to_owned()
    }
}

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
    use crate::core::workspace::{
        Layout, MultiplexerKind, WorkspacePane, WorkspaceTab, PROFILE_STORE_VERSION,
    };
    use devbox_integration::{
        named_view_snapshot_path_in, read_named_view_snapshot_in, write_named_view_snapshot_atomic,
    };
    use std::collections::BTreeSet;

    fn profile(id: impl Into<String>, name: impl Into<String>) -> WorkspaceProfile {
        WorkspaceProfile {
            id: id.into(),
            name: name.into(),
            tabs: vec![WorkspaceTab {
                id: "tab-1".into(),
                title: "server".into(),
                custom_title: false,
                layout: Layout::Grid,
                pane_keys: vec!["pane-1".into()],
            }],
            panes: vec![WorkspacePane {
                key: "pane-1".into(),
                distro: "Ubuntu".into(),
                cwd: Some("/home/jihoon/projects/private".into()),
                start_command: Some("pnpm dev --token=$DEV_TOKEN".into()),
                multiplexer: MultiplexerKind::Native,
            }],
            active_tab_id: "tab-1".into(),
            active_pane_key: Some("pane-1".into()),
        }
    }

    fn entries(envelope: &Envelope) -> Vec<Value> {
        envelope.data["views"][VIEW_KIND]["entries"]
            .as_array()
            .expect("profiles entries")
            .clone()
    }

    #[test]
    fn snapshot_never_leaks_profile_runtime_details() {
        let mut store = ProfileStore::empty();
        store
            .upsert(profile("profile-safe", "Safe workspace"))
            .unwrap();

        let envelope = build_profile_snapshot(&store).unwrap();
        let encoded = serde_json::to_string(&envelope).unwrap();

        for forbidden in [
            "Ubuntu",
            "/home/jihoon/projects/private",
            "pnpm dev",
            "startCommand",
            "distro",
            "cwd",
            "pane",
            "path",
        ] {
            assert!(!encoded.contains(forbidden), "snapshot leaked {forbidden}");
        }
        assert_eq!(entries(&envelope)[0]["label"], "Safe workspace");
        assert_eq!(entries(&envelope)[0]["detail"], STATIC_PROFILE_DETAIL);
    }

    #[test]
    fn sensitive_profile_name_uses_static_fallback_without_copying_value() {
        for private_name in [
            "password=do-not-copy",
            "/home/jihoon/projects/private",
            "C:\\private\\project",
            "PATH=/home/jihoon/projects/private",
            "${PRIVATE_ROOT}",
        ] {
            let mut store = ProfileStore::empty();
            store
                .upsert(profile("profile-sensitive", private_name))
                .unwrap();

            let envelope = build_profile_snapshot(&store).unwrap();
            let encoded = serde_json::to_string(&envelope).unwrap();
            assert!(!encoded.contains(private_name));
            assert_eq!(entries(&envelope)[0]["label"], STATIC_PROFILE_LABEL);
        }
    }

    #[test]
    fn sensitive_profile_id_rejects_the_complete_snapshot() {
        let mut store = ProfileStore::empty();
        store
            .upsert(profile("sk-live-value", "Safe label"))
            .unwrap();
        assert!(build_profile_snapshot(&store).is_err());
    }

    #[test]
    fn snapshot_is_bounded_and_named_shape_is_strict() {
        let mut store = ProfileStore::empty();
        for index in 0..MAX_PROFILES {
            store
                .upsert(profile(
                    format!("profile-{index}"),
                    format!("Profile {index}"),
                ))
                .unwrap();
        }
        let envelope = build_profile_snapshot(&store).unwrap();
        assert_eq!(entries(&envelope).len(), MAX_PROFILES);

        let root = tempfile::tempdir().unwrap();
        write_named_view_snapshot_atomic(&envelope, root.path(), VIEW_KIND).unwrap();
        let path = named_view_snapshot_path_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            VIEW_KIND,
        )
        .unwrap();
        assert!(path.ends_with("wsl-desktop/v1/profiles.json"));
        assert!(path.is_file());
        assert!(!path.with_file_name("summary.json").exists());

        let read = read_named_view_snapshot_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            VIEW_KIND,
        )
        .unwrap()
        .unwrap();
        assert_eq!(read, envelope);
        let entry = &entries(&read)[0];
        assert_eq!(
            entry
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "detail".into(),
                "id".into(),
                "label".into(),
                "payload".into(),
                "payloadVersion".into(),
                "targetApp".into(),
                "targetKind".into(),
            ])
        );
        assert_eq!(
            entry["payload"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec![&"id".to_owned()]
        );
        assert_eq!(entry["targetApp"], PRODUCER_ID);
        assert_eq!(entry["targetKind"], "profile");
        assert_eq!(entry["payloadVersion"], PAYLOAD_VERSION);
    }

    #[test]
    fn snapshot_rejects_store_over_profile_bound() {
        let profiles = (0..=MAX_PROFILES)
            .map(|index| profile(format!("profile-{index}"), format!("Profile {index}")))
            .collect();
        let store = ProfileStore {
            version: PROFILE_STORE_VERSION,
            profiles,
        };
        assert!(build_profile_snapshot(&store).is_err());
    }
}
