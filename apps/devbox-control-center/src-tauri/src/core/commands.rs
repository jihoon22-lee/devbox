//! Catalog commands and bounded metadata search; no executable paths or owner stores.
use devbox_catalog::products::ProductCatalog;
use product_contract::commands::{
    self, ContextRequirement, Descriptor, DisabledReason, Request, Target,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, &'static str>;
#[derive(Default)]
pub struct Index {
    commands: BTreeMap<String, Descriptor>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    pub results: Vec<Descriptor>,
    pub truncated: bool,
}
impl Index {
    /// Installation availability comes from a native resolver, never a renderer.
    pub fn catalog(catalog: &ProductCatalog, available: &BTreeSet<String>) -> Result<Self> {
        let mut index = Self::default();
        for feature in &catalog.features {
            let revision = Sha256::digest(
                serde_json::to_vec(&(catalog.catalog_revision, feature))
                    .map_err(|_| "command_catalog_invalid")?,
            )
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
            index.insert(Descriptor {
                id: feature.command.clone(),
                owner: feature.owner.clone(),
                component: feature.component.clone(),
                label: feature.label.clone(),
                revision,
                target: Target::Route {
                    route: feature.route.clone(),
                },
                required_context: ContextRequirement::None,
                context: None,
                destructive: false,
                requires_review: false,
                disabled_reason: (!available.contains(&feature.owner))
                    .then_some(DisabledReason::ProviderUnavailable),
            })?;
        }
        Ok(index)
    }
    pub fn insert(&mut self, command: Descriptor) -> Result<()> {
        command.validate()?;
        if self.commands.len() >= commands::MAX_COMMANDS || self.commands.contains_key(&command.id)
        {
            return Err("command_catalog_limit_or_duplicate");
        }
        self.commands.insert(command.id.clone(), command);
        Ok(())
    }
    pub fn search(&self, query: &str) -> Result<Search> {
        commands::validate_query(query)?;
        let needle = query.trim().to_lowercase();
        let mut matches: Vec<_> = self
            .commands
            .values()
            .filter_map(|command| {
                commands::match_fields(
                    [
                        command.label.as_str(),
                        command.owner.as_str(),
                        command.id.as_str(),
                    ],
                    &needle,
                )
                .map(|score| (score, command))
            })
            .collect();
        matches.sort_by(|(a, left), (b, right)| {
            a.cmp(b)
                .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
                .then_with(|| left.id.cmp(&right.id))
        });
        let truncated = matches.len() > commands::MAX_RESULTS;
        Ok(Search {
            truncated,
            results: matches
                .into_iter()
                .take(commands::MAX_RESULTS)
                .map(|(_, value)| value.clone())
                .collect(),
        })
    }
    pub fn resolve(&self, request: &Request) -> Result<&Descriptor> {
        let command = self
            .commands
            .get(&request.command_id)
            .ok_or("command_missing")?;
        command.validate_request(request)?;
        Ok(command)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_catalog_routes_have_exact_revisions_and_missing_owners_stay_visible() {
        let catalog = ProductCatalog::parse(devbox_catalog::products::SOURCE).unwrap();
        let index = Index::catalog(&catalog, &BTreeSet::from(["control-center".into()])).unwrap();
        let search = index.search("").unwrap();
        assert_eq!(search.results.len(), catalog.features.len());
        let workspace = search
            .results
            .iter()
            .find(|row| row.id == "workspace.open-files")
            .unwrap();
        assert_eq!(
            workspace.disabled_reason,
            Some(DisabledReason::ProviderUnavailable)
        );
        let request = Request {
            operation_id: "fixture".into(),
            command_id: workspace.id.clone(),
            revision: workspace.revision.clone(),
            context: None,
            selection_id: None,
        };
        assert_eq!(index.resolve(&request).err(), Some("command_disabled"));
        let available = Index::catalog(&catalog, &BTreeSet::from(["workspace".into()])).unwrap();
        assert!(available.resolve(&request).is_ok());
        let mut changed = request;
        changed.revision = "f".repeat(64);
        assert_eq!(
            available.resolve(&changed).err(),
            Some("command_request_stale")
        );
    }
    #[test]
    fn duplicate_provider_ids_cannot_replace_an_existing_command() {
        let catalog = ProductCatalog::parse(devbox_catalog::products::SOURCE).unwrap();
        let mut index = Index::catalog(&catalog, &BTreeSet::new()).unwrap();
        let existing = index.search("").unwrap().results.remove(0);
        assert!(index.insert(existing).is_err());
    }
}
