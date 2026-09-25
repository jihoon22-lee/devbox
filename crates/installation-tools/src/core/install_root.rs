//! Current install-root locator parsing; no application lookup or launch.
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};
pub const INSTALL_ROOT_SCHEMA_VERSION: u32 = 1;
pub const MAX_INSTALL_ROOT_LOCATOR_BYTES: u64 = 16 * 1024;
const MAX_INSTALL_ROOT_PATH_BYTES: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallLookupError {
    InvalidLocator,
}
impl fmt::Display for InstallLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("install-root locator is invalid")
    }
}
impl std::error::Error for InstallLookupError {}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallRootLocator {
    pub schema_version: u32,
    pub registry_revision: u64,
    pub catalog_revision: u64,
    pub root_id: String,
    pub path: String,
    pub manifest_path: String,
    pub updated_at_ms: u64,
}

pub fn parse_install_root_locator(input: &str) -> Result<InstallRootLocator, InstallLookupError> {
    if input.len() > MAX_INSTALL_ROOT_LOCATOR_BYTES as usize {
        return Err(InstallLookupError::InvalidLocator);
    }
    let locator: InstallRootLocator =
        serde_json::from_str(input).map_err(|_| InstallLookupError::InvalidLocator)?;
    if locator.schema_version != INSTALL_ROOT_SCHEMA_VERSION
        || locator.registry_revision == 0
        || locator.catalog_revision == 0
        || !valid_root_id(&locator.root_id)
        || locator.updated_at_ms == 0
        || locator.path.len() > MAX_INSTALL_ROOT_PATH_BYTES
        || locator.manifest_path.len() > MAX_INSTALL_ROOT_PATH_BYTES
        || !valid_absolute_literal(&locator.path)
        || !valid_absolute_literal(&locator.manifest_path)
    {
        return Err(InstallLookupError::InvalidLocator);
    }
    Ok(locator)
}
fn valid_absolute_literal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INSTALL_ROOT_PATH_BYTES
        && !value.contains(['%', '$', '\0'])
        && !value.starts_with('~')
        && !value.starts_with(r"\\?\")
        && !value.starts_with(r"\\.\")
        && !value.starts_with("//?/")
        && !value.starts_with("//./")
        && Path::new(value).is_absolute()
        && !value
            .split(['/', '\\'])
            .any(|segment| matches!(segment, "." | ".."))
        && !Path::new(value).components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
}
fn valid_root_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 96
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn locator_contract_rejects_unknown_fields_zero_revisions_and_untrusted_values_safely() {
        let secret = "locator-secret-must-not-appear";
        let invalid = json!({
            "schemaVersion": 1,
            "registryRevision": 0,
            "catalogRevision": 1,
            "rootId": secret,
            "path": "/tmp/root",
            "manifestPath": "/tmp/root/registry.json",
            "updatedAtMs": 1,
            "extra": true
        })
        .to_string();
        let error = parse_install_root_locator(&invalid)
            .unwrap_err()
            .to_string();

        assert_eq!(error, "install-root locator is invalid");
        assert!(!error.contains(secret));

        let oversized = "x".repeat(MAX_INSTALL_ROOT_LOCATOR_BYTES as usize + 1);
        assert_eq!(
            parse_install_root_locator(&oversized),
            Err(InstallLookupError::InvalidLocator)
        );
    }
}
