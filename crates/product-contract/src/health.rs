//! Fresh native package/shell/store observation. Renderer behavior, external
//! services and activation commit require their separate acceptance evidence.
use crate::migration_status::Summary;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub challenge: String,
    pub installation_key: String,
    pub generation: String,
    pub session_id: String,
    pub catalog_revision: u64,
    pub routes: Vec<String>,
    pub store: Summary,
}
impl Report {
    pub fn validate(
        &self,
        owner: &str,
        version: &str,
        installation_key: &str,
        generation: &str,
        challenge: &str,
    ) -> Result<(), &'static str> {
        self.store.validate(owner, version)?;
        if self.schema_version != 1
            || self.installation_key != installation_key
            || self.generation != generation
            || self.challenge != challenge
            || !crate::commands::opaque_id(challenge)
            || !crate::commands::opaque_id(&self.session_id)
            || self.catalog_revision == 0
            || self.routes.is_empty()
            || self.routes.len() > 64
            || self
                .routes
                .iter()
                .any(|route| !crate::commands::opaque_id(route))
            || self.routes.windows(2).any(|routes| routes[0] >= routes[1])
        {
            return Err("suite_health_stale");
        }
        Ok(())
    }
    pub fn store_ready(&self) -> bool {
        self.store.setup_selected && !self.store.busy && !self.store.review_required
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_generation_replayed_probe_and_incomplete_import_are_not_ready() {
        let mut report = Report {
            schema_version: 1,
            challenge: "fresh-probe".into(),
            installation_key: "a".repeat(64),
            generation: "generation".into(),
            session_id: "session".into(),
            catalog_revision: 12,
            routes: vec!["overview".into()],
            store: Summary::new("workspace", "0.8.0", false, true, false, b"store").unwrap(),
        };
        let check = |r: &Report, generation, challenge| {
            r.validate("workspace", "0.8.0", &"a".repeat(64), generation, challenge)
        };
        assert!(check(&report, "generation", "fresh-probe").is_ok());
        assert!(report.store_ready());
        assert!(check(&report, "replacement", "fresh-probe").is_err());
        assert!(check(&report, "generation", "next-probe").is_err());
        report.store.review_required = true;
        assert!(!report.store_ready());
        report.store.review_required = false;
        report.store.busy = true;
        assert!(!report.store_ready());
        report.store.owner = "knowledge".into();
        assert!(check(&report, "generation", "fresh-probe").is_err());
        report.store.owner = "workspace".into();
        report.routes.push("overview".into());
        assert!(check(&report, "generation", "fresh-probe").is_err());
    }
}
