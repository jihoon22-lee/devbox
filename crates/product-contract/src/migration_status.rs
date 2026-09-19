//! Owner-reported migration metadata. This is an observation for review, not a
//! permit to commit an installation or delete its sources/backups.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Summary {
    pub schema_version: u32,
    pub owner: String,
    pub suite_version: String,
    pub busy: bool,
    pub setup_selected: bool,
    pub review_required: bool,
    pub revision: String,
    #[serde(default)]
    pub mappings: Option<MappingSummary>,
}
/// A digest of the owner's retained mapping ledger. It contains no record IDs,
/// paths or content, and does not certify source backup or activation readiness.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MappingSummary {
    pub record_count: u64,
    pub revision: String,
}
impl MappingSummary {
    pub fn new(record_count: u64, revision: String) -> Result<Self, &'static str> {
        if record_count > 1_000_000
            || revision.len() != 64
            || !revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("migration_mapping_summary_invalid");
        }
        Ok(Self {
            record_count,
            revision,
        })
    }
}
impl Summary {
    pub fn with_mappings(mut self, mappings: MappingSummary) -> Result<Self, &'static str> {
        let mappings = MappingSummary::new(mappings.record_count, mappings.revision)?;
        let bytes = serde_json::to_vec(&(&self.revision, &mappings))
            .map_err(|_| "migration_summary_invalid")?;
        self.revision = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.mappings = Some(mappings);
        Ok(self)
    }
    pub fn new(
        owner: &str,
        version: &str,
        busy: bool,
        setup_selected: bool,
        review_required: bool,
        native_revision: &[u8],
    ) -> Result<Self, &'static str> {
        if !crate::installation::PRODUCTS.contains(&owner)
            || version.len() > 32
            || native_revision.len() > 1024 * 1024
        {
            return Err("migration_summary_invalid");
        }
        let revision = Sha256::digest(native_revision)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Self {
            schema_version: 1,
            owner: owner.into(),
            suite_version: version.into(),
            busy,
            setup_selected,
            review_required,
            revision,
            mappings: None,
        })
    }
}
