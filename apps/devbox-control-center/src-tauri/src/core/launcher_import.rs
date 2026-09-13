//! Exact legacy identifiers only. Missing dynamic identities remain unresolved;
//! similarity of labels, paths or command names is never a migration rule.
use product_contract::launcher_preferences::{Preferences, MAX_FAVORITES, MAX_RECENTS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyShortcut {
    pub accelerator: String,
    pub enabled: bool,
}
impl LegacyShortcut {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !["Ctrl+Alt+Space", "Ctrl+Alt+L", "Ctrl+Alt+J"].contains(&self.accelerator.as_str()) {
            return Err("launcher_import_invalid");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub preferences: Preferences,
    pub unresolved_favorites: Vec<String>,
    pub unresolved_recents: Vec<String>,
    pub capacity_favorites: Vec<String>,
    pub capacity_recents: Vec<String>,
    pub legacy_shortcut: Option<LegacyShortcut>,
    pub proposed_shortcut: Option<product_contract::shortcuts::Config>,
    pub launcher_terminal_conflict: bool,
}
fn catalog(id: &str) -> Option<&'static str> {
    Some(match id.strip_prefix("catalog/app/")? {
        "port-manager" => "workspace.open-runtime",
        "developer-toolbox" => "api-studio.open-transforms",
        "wsl-desktop" => "workspace.open-terminal",
        "api-playground" => "api-studio.open-requests",
        "everything-plus" => "knowledge.open-search",
        "knowledge-base" => "knowledge.open-notes",
        "life-log" => "knowledge.open-activity",
        "devbox-manager" => "control-center.open-products",
        "code-pad" => "workspace.open-files",
        "run-manager" => "workspace.open-tasks",
        "workbench" => "workspace.open-overview",
        "webhook-lab" => "api-studio.open-webhooks",
        "repo-manager" => "workspace.open-source",
        "log-lens" => "workspace.open-logs",
        _ => return None,
    })
}
fn merge(
    existing: &[String],
    source: &[String],
    exact: &BTreeMap<String, String>,
    limit: usize,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut next = existing.to_vec();
    let mut unresolved = Vec::new();
    let mut capacity = Vec::new();
    for id in source {
        let mapped = catalog(id)
            .map(str::to_owned)
            .or_else(|| exact.get(id).cloned());
        match mapped {
            Some(mapped) if next.contains(&mapped) => {}
            Some(mapped) if next.len() < limit => next.push(mapped),
            Some(_) => capacity.push(id.clone()),
            None => unresolved.push(id.clone()),
        }
    }
    (next, unresolved, capacity)
}
pub fn prepare(
    source: &Preferences,
    destination: &Preferences,
    shortcut: Option<LegacyShortcut>,
    existing_shortcut: bool,
    exact: &BTreeMap<String, String>,
) -> Result<Plan, &'static str> {
    source.validate().map_err(|_| "launcher_import_invalid")?;
    destination
        .validate()
        .map_err(|_| "launcher_import_invalid")?;
    let catalog_commands =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?
            .features
            .into_iter()
            .map(|feature| feature.command)
            .collect::<BTreeSet<_>>();
    for target in source
        .favorites
        .iter()
        .chain(source.recents.iter())
        .filter_map(|id| catalog(id))
    {
        if !catalog_commands.contains(target) {
            return Err("launcher_import_catalog_changed");
        }
    }
    for (old, new) in exact {
        product_contract::launcher_preferences::validate_result_id(old)
            .map_err(|_| "launcher_import_invalid")?;
        product_contract::launcher_preferences::validate_result_id(new)
            .map_err(|_| "launcher_import_invalid")?;
        if !old.starts_with("snapshot/")
            || !["workspace.", "knowledge.", "api-studio."]
                .iter()
                .any(|prefix| new.starts_with(prefix))
        {
            return Err("launcher_import_mapping_invalid");
        }
    }
    if let Some(shortcut) = &shortcut {
        shortcut.validate()?;
    }
    let (favorites, unresolved_favorites, capacity_favorites) = merge(
        &destination.favorites,
        &source.favorites,
        exact,
        MAX_FAVORITES,
    );
    let (recents, unresolved_recents, capacity_recents) =
        merge(&destination.recents, &source.recents, exact, MAX_RECENTS);
    let proposed_shortcut = shortcut
        .as_ref()
        .filter(|_| !existing_shortcut)
        .map(|shortcut| product_contract::shortcuts::Config {
            accelerator: shortcut.accelerator.clone(),
            // Migration does not acquire global input. The common settings surface
            // explicitly enables the reviewed installation after showing conflicts.
            enabled: false,
            terminal: false,
            capture: false,
            project: false,
        });
    let launcher_terminal_conflict = shortcut
        .as_ref()
        .is_some_and(|shortcut| shortcut.enabled && shortcut.accelerator == "Ctrl+Alt+Space");
    Ok(Plan {
        preferences: Preferences {
            version: 1,
            favorites,
            recents,
        },
        unresolved_favorites,
        unresolved_recents,
        capacity_favorites,
        capacity_recents,
        legacy_shortcut: shortcut,
        proposed_shortcut,
        launcher_terminal_conflict,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_mapping_preserves_destination_order_and_unresolved_identity_on_repeat() {
        let source = Preferences {
            version: 1,
            favorites: vec![
                "catalog/app/code-pad".into(),
                "snapshot/workbench/same-name".into(),
                "catalog/app/code-pad-old".into(),
            ],
            recents: vec!["catalog/app/run-manager".into()],
        };
        let destination = Preferences {
            version: 1,
            favorites: vec!["knowledge.open-notes".into()],
            recents: vec![],
        };
        let plan = prepare(
            &source,
            &destination,
            Some(LegacyShortcut {
                accelerator: "Ctrl+Alt+Space".into(),
                enabled: true,
            }),
            false,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            plan.preferences.favorites,
            vec!["knowledge.open-notes", "workspace.open-files"]
        );
        assert_eq!(
            plan.unresolved_favorites,
            vec!["snapshot/workbench/same-name", "catalog/app/code-pad-old"]
        );
        assert!(plan.launcher_terminal_conflict);
        assert!(!plan.proposed_shortcut.unwrap().enabled);
        assert_eq!(
            prepare(&source, &plan.preferences, None, true, &BTreeMap::new())
                .unwrap()
                .preferences,
            plan.preferences
        );
    }
}
