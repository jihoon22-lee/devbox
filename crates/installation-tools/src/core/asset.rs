use super::manifest::{AssetRef, ReleaseManifest};
use crate::core::download::is_valid_sha256;
use std::collections::HashSet;

fn safe_artifact_component(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.trim() == value
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

pub fn validate_version_component(version: &str) -> Result<(), String> {
    if safe_artifact_component(version, 128) {
        Ok(())
    } else {
        Err("manifest version identity is invalid".to_string())
    }
}

pub fn validate_manifest_artifacts(manifest: &ReleaseManifest) -> Result<(), String> {
    if !safe_artifact_component(&manifest.release_tag, 128) {
        return Err("manifest release identity is invalid".to_string());
    }
    let mut ids = HashSet::new();
    for app in &manifest.apps {
        if !safe_artifact_component(&app.id, 64)
            || !ids.insert(app.id.as_str())
            || validate_version_component(&app.version).is_err()
            || validate_artifact_coordinates(
                &manifest.release_tag,
                &app.version,
                &app.portable.name,
            )
            .is_err()
            || validate_artifact_coordinates(
                &manifest.release_tag,
                &app.version,
                &app.installer.name,
            )
            .is_err()
            || app.portable.size < 0
            || app.installer.size < 0
            || !is_valid_sha256(&app.portable.sha256)
            || !is_valid_sha256(&app.installer.sha256)
        {
            return Err("manifest app artifact is invalid".to_string());
        }
    }
    Ok(())
}

/// Values interpolated into a GitHub release URL or Manager-owned path must
/// remain single, bounded ASCII components. Errors deliberately do not echo
/// the untrusted manifest value.
pub fn validate_artifact_coordinates(
    release_tag: &str,
    version: &str,
    asset_name: &str,
) -> Result<(), String> {
    if !safe_artifact_component(release_tag, 128)
        || !safe_artifact_component(version, 128)
        || !safe_artifact_component(asset_name, 255)
        || !asset_name.to_ascii_lowercase().ends_with(".exe")
    {
        return Err("manifest artifact identity is invalid".to_string());
    }
    Ok(())
}

/// manifest에서 앱의 mode에 해당하는 asset을 고른다. 파일명 추측 로직은 없다.
pub fn select_asset<'a>(
    manifest: &'a ReleaseManifest,
    app_id: &str,
    mode: &str,
) -> Result<&'a AssetRef, String> {
    let app = manifest
        .apps
        .iter()
        .find(|a| a.id == app_id)
        .ok_or_else(|| format!("manifest에 앱이 없다: {app_id}"))?;
    match mode {
        "portable" => Ok(&app.portable),
        "installer" => Ok(&app.installer),
        other => Err(format!("지원하지 않는 설치 모드: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::manifest::AppManifest;
    use super::*;

    fn sample_manifest() -> ReleaseManifest {
        ReleaseManifest {
            schema_version: 1,
            release_tag: "v0.4.0".into(),
            generated_at: "2026-08-14T12:00:00Z".into(),
            apps: vec![AppManifest {
                id: "life-log".into(),
                version: "0.2.2".into(),
                portable: AssetRef {
                    name: "life-log.exe".into(),
                    sha256: "a".repeat(64),
                    size: 123,
                },
                installer: AssetRef {
                    name: "LifeLog_0.2.2_x64-setup.exe".into(),
                    sha256: "b".repeat(64),
                    size: 456,
                },
            }],
        }
    }

    fn app_manifest(id: &str) -> AppManifest {
        AppManifest {
            id: id.into(),
            version: "0.2.2".into(),
            portable: AssetRef {
                name: format!("{id}.exe"),
                sha256: "a".repeat(64),
                size: 1,
            },
            installer: AssetRef {
                name: format!("{id}_setup.exe"),
                sha256: "b".repeat(64),
                size: 2,
            },
        }
    }

    #[test]
    fn selects_portable() {
        let m = sample_manifest();
        let asset = select_asset(&m, "life-log", "portable").unwrap();
        assert_eq!(asset.name, "life-log.exe");
    }

    #[test]
    fn selects_installer() {
        let m = sample_manifest();
        let asset = select_asset(&m, "life-log", "installer").unwrap();
        assert_eq!(asset.name, "LifeLog_0.2.2_x64-setup.exe");
    }

    #[test]
    fn errors_on_missing_app() {
        let m = sample_manifest();
        let err = select_asset(&m, "ghost", "portable").unwrap_err();
        assert!(err.contains("ghost"));
    }

    #[test]
    fn errors_on_unknown_mode() {
        let m = sample_manifest();
        let err = select_asset(&m, "life-log", "bundle").unwrap_err();
        assert!(err.contains("bundle"));
    }

    #[test]
    fn portable_only_app_has_no_installer_selection_dependency() {
        let m = ReleaseManifest {
            schema_version: 1,
            release_tag: "v0.4.0".into(),
            generated_at: "x".into(),
            apps: vec![app_manifest("port-only")],
        };
        // portable 선택은 잘 동작
        assert!(select_asset(&m, "port-only", "portable").is_ok());
        // installer도 정상 (스키마가 강제)
        assert!(select_asset(&m, "port-only", "installer").is_ok());
    }

    #[test]
    fn artifact_coordinates_reject_path_components_without_echoing_them() {
        let secret = "../TOP_SECRET.exe";
        let error = validate_artifact_coordinates("v0.5.0", "0.5.0", secret).unwrap_err();
        assert!(!error.contains(secret));
        assert!(validate_artifact_coordinates(
            "v0.5.0-rc.1",
            "0.5.0-rc.1+build.2",
            "CodePad_0.5.0_x64-setup.exe",
        )
        .is_ok());
    }

    #[test]
    fn manifest_artifacts_reject_unknown_paths_duplicates_and_invalid_digest() {
        let mut manifest = sample_manifest();
        assert!(validate_manifest_artifacts(&manifest).is_ok());

        manifest.apps[0].portable.name = "../TOP_SECRET.exe".to_string();
        let error = validate_manifest_artifacts(&manifest).unwrap_err();
        assert!(!error.contains("TOP_SECRET"));

        let mut duplicate = sample_manifest();
        duplicate.apps.push(duplicate.apps[0].clone());
        assert!(validate_manifest_artifacts(&duplicate).is_err());

        let mut invalid_digest = sample_manifest();
        invalid_digest.apps[0].installer.sha256 = "short".to_string();
        assert!(validate_manifest_artifacts(&invalid_digest).is_err());
    }
}
