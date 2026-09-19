//! Owner-selected, path-free backup metadata. Verification proves retained bytes,
//! not a fresh legacy cutover snapshot, complete migration coverage or activation.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    pub id: String,
    pub acquisition: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Verified {
    pub owner: String,
    pub id: String,
    pub acquisition: String,
    pub bytes: u64,
    pub schema: u32,
    pub sha256: String,
}
pub fn acquisition(value: &str) -> bool {
    matches!(
        value,
        "sqlite-online-backup/v1"
            | "normalized-import-bundle/v1"
            | "closed-leveldb-exclusive-copy/v1"
            | "stable-json-pair/v1"
            | "stable-json-files/v1"
    )
}
impl Descriptor {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !crate::commands::opaque_id(&self.id) || !acquisition(&self.acquisition) {
            return Err("migration_backup_invalid");
        }
        Ok(())
    }
}
impl Verified {
    pub fn validate(&self, owner: &str, id: &str) -> Result<(), &'static str> {
        Descriptor {
            id: self.id.clone(),
            acquisition: self.acquisition.clone(),
        }
        .validate()?;
        if self.owner != owner
            || !crate::installation::PRODUCTS.contains(&owner)
            || self.id != id
            || self.bytes > 512 * 1024 * 1024
            || !crate::commands::revision(&self.sha256)
        {
            return Err("migration_backup_invalid");
        }
        Ok(())
    }
}
