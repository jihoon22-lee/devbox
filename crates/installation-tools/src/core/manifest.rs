use serde::{Deserialize, Serialize};

/// §5.4 release manifest의 앱별 asset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetRef {
    pub name: String,
    pub sha256: String,
    pub size: i64,
}

/// §5.4 release manifest의 앱 항목. version은 release tag가 아니라 앱 실제 버전이다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppManifest {
    pub id: String,
    pub version: String,
    pub portable: AssetRef,
    pub installer: AssetRef,
}

/// §5.4 release manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub release_tag: String,
    pub generated_at: String,
    pub apps: Vec<AppManifest>,
}

pub fn parse_manifest(input: &str) -> Result<ReleaseManifest, String> {
    let manifest: ReleaseManifest =
        serde_json::from_str(input).map_err(|e| format!("manifest 파싱 실패: {e}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "지원하지 않는 manifest schemaVersion: {}",
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "schemaVersion": 1,
        "releaseTag": "v0.4.0",
        "generatedAt": "2026-08-14T12:00:00Z",
        "apps": [
            {
                "id": "life-log",
                "version": "0.2.2",
                "portable": { "name": "life-log.exe", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 123456 },
                "installer": { "name": "LifeLog_0.2.2_x64-setup.exe", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "size": 234567 }
            }
        ]
    }"#;

    #[test]
    fn parses_valid_manifest() {
        let m = parse_manifest(SAMPLE).unwrap();
        assert_eq!(m.release_tag, "v0.4.0");
        assert_eq!(m.apps[0].id, "life-log");
        assert_eq!(m.apps[0].version, "0.2.2");
        assert_eq!(m.apps[0].portable.name, "life-log.exe");
        assert_eq!(m.apps[0].portable.size, 123456);
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let bad = SAMPLE.replace("\"schemaVersion\": 1", "\"schemaVersion\": 3");
        assert!(parse_manifest(&bad).unwrap_err().contains("schemaVersion"));
    }

    #[test]
    fn rejects_missing_asset_field() {
        let bad = SAMPLE.replace(
            "            \"portable\":",
            "            \"portable_typo\":",
        );
        assert!(parse_manifest(&bad).is_err());
    }
}
