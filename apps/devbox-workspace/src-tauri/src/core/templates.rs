//! Destination copies of validated Workbench templates. Creating a concrete
//! project requires a separate native registration preview.
use super::profiles::{is_false, valid_origin};
use projects_engine::component::ProfileTemplate;
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct ImportedTemplate {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub local: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    pub template: ProfileTemplate,
}
impl ImportedTemplate {
    pub fn validate(&self) -> Result<()> {
        if uuid::Uuid::parse_str(&self.id).is_err()
            || !valid_origin(self.source_snapshot_id.as_deref(), self.local)
            || self.template.validate().is_err()
        {
            return Err("invalid_imported_template");
        }
        Ok(())
    }
}
