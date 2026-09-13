//! Development product topology. The v2 public release selector is unchanged.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SOURCE: &str = include_str!("../../../apps/products.json");

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductCatalog {
    pub schema_version: u32,
    pub catalog_revision: u64,
    pub channel: String,
    pub products: Vec<Product>,
    pub components: Vec<Component>,
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Product {
    pub id: String,
    pub label: String,
    pub identifier: String,
    pub default_route: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Component {
    pub id: String,
    pub owner: String,
    pub authority: String,
    pub lifecycle: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub owner: String,
    pub route: String,
    pub label: String,
    pub component: String,
    pub command: String,
    pub authority: String,
    pub status: String,
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn component_authority(owner: &str, id: &str, authority: &str) -> bool {
    if id == format!("{owner}.shell") {
        return authority == "shell-read";
    }
    matches!(
        (owner, id, authority),
        ("api-studio", "api-studio.api", "request-network")
            | ("api-studio", "api-studio.webhooks", "listener-network")
            | ("api-studio", "api-studio.transforms", "transform-local")
            | ("api-studio", "api-studio.migration", "legacy-import")
            | ("knowledge", "knowledge.notes", "note-writer")
            | ("knowledge", "knowledge.activity", "activity-collector")
            | ("knowledge", "knowledge.search", "search-read")
            | ("knowledge", "knowledge.search-settings", "search-admin")
            | ("knowledge", "knowledge.opener", "result-open")
            | ("knowledge", "knowledge.migration", "legacy-import")
            | ("workspace", "workspace.definitions", "project-definition")
            | ("workspace", "workspace.dependencies", "dependency-review")
            | ("workspace", "workspace.source", "git-execution")
            | ("workspace", "workspace.registry", "project-registry")
            | ("workspace", "workspace.migration", "legacy-import")
            | ("workspace", "workspace.files", "file-edit")
            | ("workspace", "workspace.lsp", "language-service")
            | ("workspace", "workspace.runtime", "runtime-execution")
            | ("workspace", "workspace.processes", "process-observation")
            | (
                "workspace",
                "workspace.process-actions",
                "external-process-action"
            )
            | ("workspace", "workspace.logs", "log-read")
            | ("workspace", "workspace.terminal", "terminal-session")
            | ("workspace", "workspace.problems", "problem-read")
    )
}

impl ProductCatalog {
    pub fn parse(source: &str) -> Result<Self, &'static str> {
        if source.len() > 256 * 1024 {
            return Err("product catalog exceeds limit");
        }
        let catalog: Self = serde_json::from_str(source).map_err(|_| "invalid product catalog")?;
        if catalog.schema_version != 3
            || catalog.catalog_revision == 0
            || catalog.channel != "development"
        {
            return Err("unsupported product catalog revision or channel");
        }
        let expected = ["workspace", "api-studio", "knowledge", "control-center"];
        let mut products = HashSet::new();
        let mut identifiers = HashSet::new();
        for p in &catalog.products {
            if !expected.contains(&p.id.as_str())
                || !products.insert(p.id.as_str())
                || !identifiers.insert(p.identifier.as_str())
                || p.label.trim().is_empty()
                || p.identifier != format!("com.devbox.v08.{}", p.id.replace('-', ""))
                || !slug(&p.default_route)
            {
                return Err("invalid or duplicate product identity");
            }
        }
        if products.len() != expected.len() {
            return Err("four products are required");
        }
        let mut components = HashSet::new();
        for c in &catalog.components {
            if !products.contains(c.owner.as_str())
                || !component_authority(&c.owner, &c.id, &c.authority)
                || !components.insert(c.id.as_str())
                || c.lifecycle != "product"
                || c.protocol_version != 1
            {
                return Err("invalid component owner, lifecycle or authority");
            }
        }
        let mut ids = HashSet::new();
        let mut routes = HashSet::new();
        let mut commands = HashSet::new();
        for f in &catalog.features {
            if !products.contains(f.owner.as_str())
                || !slug(&f.route)
                || f.id != format!("{}.{}", f.owner, f.route)
                || !ids.insert(f.id.as_str())
                || !routes.insert((&f.owner, &f.route))
                || !commands.insert(f.command.as_str())
                || f.command != format!("{}.open-{}", f.owner, f.route)
                || f.component != format!("{}.shell", f.owner)
                || !components.contains(f.component.as_str())
                || f.authority != "shell-read"
                || f.status != "foundation"
                || f.label.trim().is_empty()
            {
                return Err("invalid feature route, owner, command or authority");
            }
        }
        if catalog
            .products
            .iter()
            .any(|p| !routes.contains(&(&p.id, &p.default_route)))
        {
            return Err("missing default route");
        }
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_topology_is_complete_and_hidden() {
        let c = ProductCatalog::parse(SOURCE).unwrap();
        assert_eq!(c.products.len(), 4);
        assert!(c.products.iter().all(|p| c
            .features
            .iter()
            .any(|f| f.owner == p.id && f.route == p.default_route)));
    }
    #[test]
    fn rejects_escalation_ambiguity_and_future_contracts() {
        let original: serde_json::Value = serde_json::from_str(SOURCE).unwrap();
        let registry = original["components"]
            .as_array()
            .unwrap()
            .iter()
            .position(|component| component["id"] == "workspace.registry")
            .unwrap();
        for (field, value) in [
            ("owner", "api-studio"),
            ("authority", "request-network"),
            ("id", "workspace.unreviewed"),
        ] {
            let mut changed = original.clone();
            changed["components"][registry][field] = value.into();
            assert!(
                ProductCatalog::parse(&changed.to_string()).is_err(),
                "{field}"
            );
        }
        for (field, value) in [
            ("owner", "knowledge"),
            ("route", "../notes"),
            ("authority", "runtime"),
            ("component", "knowledge.shell"),
            ("command", "exec"),
            ("status", "ready"),
        ] {
            let mut v = original.clone();
            v["features"][0][field] = value.into();
            assert!(ProductCatalog::parse(&v.to_string()).is_err(), "{field}");
        }
        let mut v = original.clone();
        v["features"][1] = v["features"][0].clone();
        assert!(ProductCatalog::parse(&v.to_string()).is_err());
        let mut v = original;
        v["schemaVersion"] = 4.into();
        assert!(ProductCatalog::parse(&v.to_string()).is_err());
    }
}
