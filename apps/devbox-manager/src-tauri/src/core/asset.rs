use super::manifest::{AssetRef, ReleaseManifest};

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
}
