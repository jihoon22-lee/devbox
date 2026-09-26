//! Current stored profiles and their explicit project bindings.
use projects_engine::component::ProjectProfile;
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct ImportedProfile {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub local: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_template_id: Option<String>,
    pub profile: ProjectProfile,
}
pub use projects_engine::api::ProfileTarget;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct ProfileBinding {
    pub imported_id: String,
    pub target: ProfileTarget,
    pub worktree_id: String,
}

impl ImportedProfile {
    pub fn validate(&self) -> Result<()> {
        if uuid::Uuid::parse_str(&self.id).is_err()
            || !valid_origin(self.source_snapshot_id.as_deref(), self.local)
            || (self.local && self.source_template_id.is_none())
            || self.profile.validate().is_err()
        {
            return Err("invalid_imported_profile");
        }
        Ok(())
    }
}
pub(super) fn is_false(value: &bool) -> bool {
    !value
}
pub(super) fn valid_origin(snapshot: Option<&str>, local: bool) -> bool {
    matches!((snapshot, local), (None, true))
        || matches!((snapshot, local), (Some(id), false) if snapshot_id(id))
}
pub(super) fn snapshot_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
