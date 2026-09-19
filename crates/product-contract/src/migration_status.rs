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
}
impl Summary {
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
        })
    }
}
