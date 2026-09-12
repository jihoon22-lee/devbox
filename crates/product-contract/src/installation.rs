//! Package declarations are untrusted until a native owner verifies their files
//! and approves the captured generation. This schema itself grants no launch right.
use crate::commands::{opaque_id, revision};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const PRODUCTS: [&str; 4] = ["workspace", "api-studio", "knowledge", "control-center"];

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Member {
    pub product: String,
    pub executable: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub installation_id: String,
    pub generation: String,
    pub suite_version: String,
    pub protocol_version: u32,
    pub members: Vec<Member>,
}
impl Manifest {
    pub fn parse(bytes: &[u8], expected_version: &str) -> Result<Self, &'static str> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err("installation_manifest_limit");
        }
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|_| "installation_manifest_invalid")?;
        manifest.validate(expected_version)?;
        Ok(manifest)
    }
    pub fn validate(&self, expected_version: &str) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.protocol_version != 1
            || self.suite_version != expected_version
        {
            return Err("installation_version_mismatch");
        }
        if !opaque_id(&self.installation_id)
            || !opaque_id(&self.generation)
            || self.members.is_empty()
            || self.members.len() > 4
        {
            return Err("installation_manifest_invalid");
        }
        let mut products = BTreeSet::new();
        for member in &self.members {
            if !PRODUCTS.contains(&member.product.as_str())
                || !products.insert(&member.product)
                || !revision(&member.sha256)
                || (!member_path(&member.product, &member.executable)
                    && member.executable
                        != format!(
                            "generations/{}/products/{}/devbox-{}.exe",
                            self.generation, member.product, member.product
                        ))
            {
                return Err("installation_member_invalid");
            }
        }
        Ok(())
    }
}
/// Closed suite/portable layouts, never an arbitrary registry executable string.
pub fn member_path(product: &str, path: &str) -> bool {
    let filename = format!("devbox-{product}.exe");
    path == filename
        || path == format!("products/{product}/{filename}")
        || path == format!("components/{product}/{filename}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            installation_id: "fixture-install".into(),
            generation: "fixture-generation".into(),
            suite_version: "0.8.0".into(),
            protocol_version: 1,
            members: vec![Member {
                product: "workspace".into(),
                executable: "devbox-workspace.exe".into(),
                sha256: "a".repeat(64),
            }],
        }
    }
    #[test]
    fn version_duplicates_and_arbitrary_binary_locations_are_rejected() {
        let original = manifest();
        original.validate("0.8.0").unwrap();
        assert_eq!(
            original.validate("0.8.1"),
            Err("installation_version_mismatch")
        );
        let mut duplicate = original.clone();
        duplicate.members.push(duplicate.members[0].clone());
        assert!(duplicate.validate("0.8.0").is_err());
        for path in [
            "../devbox-workspace.exe",
            "C:/devbox-workspace.exe",
            "\\\\host\\devbox-workspace.exe",
            "products/workspace/../../devbox-workspace.exe",
            "other.exe",
        ] {
            let mut changed = original.clone();
            changed.members[0].executable = path.into();
            assert!(changed.validate("0.8.0").is_err());
        }
    }
    #[test]
    fn declarations_have_no_raw_command_environment_or_data_root_fields() {
        let mut value = serde_json::to_value(manifest()).unwrap();
        for field in ["argv", "environment", "dataRoot", "secret"] {
            value[field] = serde_json::json!("untrusted");
            assert!(Manifest::parse(&serde_json::to_vec(&value).unwrap(), "0.8.0").is_err());
            value.as_object_mut().unwrap().remove(field);
        }
    }
}
