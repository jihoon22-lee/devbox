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
        assert_eq!(catalog.catalog_revision, Some(1));
        assert_eq!(catalog.apps.len(), 13);
    }

    #[test]
    fn adapter_error_does_not_echo_untrusted_catalog_values() {
        let secret = "TOP_SECRET_CATALOG_VALUE";
        let error = parse_catalog(&format!("{{not-json:{secret}}}")).unwrap_err();
        assert!(!error.contains(secret));
    }
}
