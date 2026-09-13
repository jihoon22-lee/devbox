//! Native suite cutover marker. A renderer cannot turn a route into permission
//! to write while a generation is awaiting import/health/commit.
use crate::installation::Manifest;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Import,
    Health,
    Committed,
    Recover,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Activation {
    pub schema_version: u32,
    pub installation_id: String,
    pub generation: String,
    pub operation_id: String,
    pub revision: u64,
    pub phase: Phase,
}
impl Activation {
    pub fn validate(&self, manifest: &Manifest) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.installation_id != manifest.installation_id
            || self.generation != manifest.generation
            || !crate::commands::opaque_id(&self.operation_id)
        {
            return Err("suite_activation_invalid");
        }
        Ok(())
    }
    pub fn allows(&self, product: &str, component: &str) -> bool {
        if self.phase == Phase::Committed {
            return true;
        }
        if component == format!("{product}.shell") {
            return true;
        }
        if product == "control-center" && component == "control-center.delivery" {
            return true;
        }
        self.phase == Phase::Import && component == format!("{product}.migration")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_and_import_do_not_enable_background_or_business_writers() {
        let mut a = Activation {
            schema_version: 1,
            installation_id: "fixture".into(),
            generation: "next".into(),
            operation_id: "fixture-operation".into(),
            revision: 0,
            phase: Phase::Import,
        };
        assert!(a.allows("knowledge", "knowledge.migration"));
        assert!(a.allows("workspace", "workspace.shell"));
        for component in ["workspace.runtime", "workspace.terminal", "workspace.files"] {
            assert!(!a.allows("workspace", component));
        }
        assert!(!a.allows("knowledge", "knowledge.activity"));
        assert!(!a.allows("api-studio", "api-studio.webhooks"));
        a.phase = Phase::Health;
        assert!(!a.allows("knowledge", "knowledge.migration"));
        a.phase = Phase::Committed;
        assert!(a.allows("knowledge", "knowledge.activity"));
    }
}
