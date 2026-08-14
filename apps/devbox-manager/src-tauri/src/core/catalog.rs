use serde::{Deserialize, Serialize};

/// §5.3 앱 카탈로그 스키마의 앱 항목.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogApp {
    pub id: String,
    pub display_name: String,
    pub product_name: String,
    pub identifier: String,
    pub cargo_package: String,
    pub app_dir: String,
    pub release: bool,
    pub manager_visible: bool,
    pub self_managed: bool,
}

/// §5.3 앱 카탈로그. 버전은 카탈로그가 소유하지 않는다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    pub apps: Vec<CatalogApp>,
}

pub fn parse_catalog(input: &str) -> Result<Catalog, String> {
    let catalog: Catalog =
        serde_json::from_str(input).map_err(|e| format!("카탈로그 파싱 실패: {e}"))?;
    if catalog.schema_version != 1 {
        return Err(format!(
            "지원하지 않는 카탈로그 schemaVersion: {}",
            catalog.schema_version
        ));
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "schemaVersion": 1,
        "apps": [
            {
                "id": "code-pad",
                "displayName": "Code Pad",
                "productName": "Code Pad",
                "identifier": "com.devbox.codepad",
                "cargoPackage": "code-pad",
                "appDir": "apps/code-pad",
                "release": true,
                "managerVisible": true,
                "selfManaged": false
            }
        ]
    }"#;

    #[test]
    fn parses_valid_catalog() {
        let cat = parse_catalog(SAMPLE).unwrap();
        assert_eq!(cat.schema_version, 1);
        assert_eq!(cat.apps.len(), 1);
        assert_eq!(cat.apps[0].id, "code-pad");
        assert_eq!(cat.apps[0].identifier, "com.devbox.codepad");
        assert!(cat.apps[0].release);
        assert!(!cat.apps[0].self_managed);
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let bad = SAMPLE.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        let err = parse_catalog(&bad).unwrap_err();
        assert!(err.contains("schemaVersion"), "{err}");
    }

    #[test]
    fn rejects_missing_field() {
        let bad = SAMPLE.replace(
            "\"managerVisible\": true,\n                \"selfManaged\": false",
            "\"managerVisible\": true",
        );
        assert!(parse_catalog(&bad).is_err());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_catalog("{ not json").is_err());
    }
}
