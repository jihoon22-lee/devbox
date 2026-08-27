//! Devbox Manager's build-time catalog adapter.
//!
//! Runtime-copy persistence and install-root resolution are intentionally
//! wired by the follow-up Manager feature. Parsing and validation already use
//! the shared crate so moving the repository catalog to schema v2 cannot leave
//! Manager on a divergent private schema.

pub use devbox_catalog::{Catalog, CatalogApp};

pub fn parse_catalog(input: &str) -> Result<Catalog, String> {
    devbox_catalog::parse_catalog(input).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD_CATALOG: &str = include_str!("../../../../catalog.json");

    #[test]
    fn parses_the_repository_v2_catalog_through_the_shared_contract() {
        let catalog = parse_catalog(BUILD_CATALOG).unwrap();
        assert_eq!(catalog.schema_version, 2);
        assert_eq!(catalog.catalog_revision, Some(8));
        assert_eq!(catalog.apps.len(), 14);
        let knowledge = catalog
            .apps
            .iter()
            .find(|app| app.id == "knowledge-base")
            .expect("Knowledge must remain in the repository catalog");
        assert_eq!(
            knowledge.accepts,
            vec!["path", "query", "handoff:knowledge-draft/v1"]
        );
        let everything = catalog
            .apps
            .iter()
            .find(|app| app.id == "everything-plus")
            .expect("Everything+ must remain in the repository catalog");
        assert_eq!(everything.accepts, vec!["query"]);
        let wsl_desktop = catalog
            .apps
            .iter()
            .find(|app| app.id == "wsl-desktop")
            .expect("WSL Desktop must remain in the repository catalog");
        assert_eq!(wsl_desktop.accepts, vec!["path", "profile"]);
        let workbench = catalog
            .apps
            .iter()
            .find(|app| app.id == "workbench")
            .expect("Workbench must remain in the repository catalog");
        assert_eq!(workbench.accepts, vec!["path", "profile"]);
        let repo_manager = catalog
            .apps
            .iter()
            .find(|app| app.id == "repo-manager")
            .expect("Repo Manager must remain in the repository catalog");
        assert_eq!(repo_manager.accepts, vec!["path"]);
        let life_log = catalog
            .apps
            .iter()
            .find(|app| app.id == "life-log")
            .expect("Life Log must remain in the repository catalog");
        assert_eq!(
            life_log.produces,
            vec![
                "snapshot:life-log/projects/v1",
                "handoff:knowledge-draft/v1"
            ]
        );
        assert_eq!(life_log.actions.len(), 1);
        assert_eq!(life_log.actions[0].target, "knowledge-base");
    }

    #[test]
    fn adapter_error_does_not_echo_untrusted_catalog_values() {
        let secret = "TOP_SECRET_CATALOG_VALUE";
        let error = parse_catalog(&format!("{{not-json:{secret}}}")).unwrap_err();
        assert!(!error.contains(secret));
    }
}
