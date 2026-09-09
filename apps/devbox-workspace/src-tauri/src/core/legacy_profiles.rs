//! Reviewed Workbench metadata import. Stored paths remain unbound proposals;
//! importing a profile neither admits a project object nor grants execution.
use super::{legacy_inventory::digest, registry::Registry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use workbench_lib::component::{ProfileStore, ProjectProfile};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedProfile {
    pub id: String,
    pub source_snapshot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_template_id: Option<String>,
    pub profile: ProjectProfile,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileTarget {
    Windows,
    Wsl,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileBinding {
    pub imported_id: String,
    pub target: ProfileTarget,
    pub worktree_id: String,
}
impl ProfileTarget {
    pub fn of(target: &product_contract::ExecutionTarget) -> Self {
        match target {
            product_contract::ExecutionTarget::Windows => Self::Windows,
            product_contract::ExecutionTarget::Wsl { .. } => Self::Wsl,
        }
    }
}
impl ImportedProfile {
    pub fn validate(&self) -> Result<()> {
        if uuid::Uuid::parse_str(&self.id).is_err()
            || !snapshot_id(&self.source_snapshot_id)
            || self.profile.validate().is_err()
        {
            return Err("invalid_imported_profile");
        }
        Ok(())
    }
}
pub(super) fn snapshot_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    New,
    Identical,
    Conflict,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub profile: ProjectProfile,
    pub disposition: Disposition,
    pub existing_ids: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub registry_revision: u64,
    pub source_snapshot_id: String,
    pub rows: Vec<Row>,
    #[serde(skip)]
    before_digest: String,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Import,
    KeepBoth,
    Reuse,
    Skip,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Choice {
    pub source_id: String,
    pub decision: Decision,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub source_id: String,
    pub imported_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub added: usize,
    pub reused: usize,
    pub skipped: usize,
    pub mappings: Vec<Mapping>,
}
fn fingerprint(profiles: &[ImportedProfile]) -> Result<String> {
    Ok(digest(
        &serde_json::to_vec(profiles).map_err(|_| "invalid_imported_profile")?,
    ))
}
fn same_path(a: &str, b: &str) -> bool {
    match (
        devbox_filesystem::parse_safe_project_path(a),
        devbox_filesystem::parse_safe_project_path(b),
    ) {
        (Some(a), Some(b)) => a.identity() == b.identity(),
        _ => a == b,
    }
}
fn collides(a: &ProjectProfile, b: &ProjectProfile) -> bool {
    a.id == b.id
        || a.windows_path
            .as_ref()
            .zip(b.windows_path.as_ref())
            .is_some_and(|(a, b)| same_path(a, b))
        || a.wsl
            .as_ref()
            .zip(b.wsl.as_ref())
            .is_some_and(|(a, b)| a.distro == b.distro && same_path(&a.path, &b.path))
}
impl Plan {
    pub fn build(
        source_snapshot_id: String,
        source: ProfileStore,
        registry: &Registry,
    ) -> Result<Self> {
        if !snapshot_id(&source_snapshot_id) {
            return Err("invalid_legacy_snapshot");
        }
        source.validate().map_err(|_| "invalid_imported_profile")?;
        registry.validate()?;
        let rows = source
            .profiles
            .into_iter()
            .map(|profile| {
                let identical = registry
                    .imported_profiles
                    .iter()
                    .find(|saved| saved.profile == profile);
                let (disposition, existing_ids) = if let Some(saved) = identical {
                    (Disposition::Identical, vec![saved.id.clone()])
                } else {
                    let ids = registry
                        .imported_profiles
                        .iter()
                        .filter(|saved| collides(&saved.profile, &profile))
                        .map(|saved| saved.id.clone())
                        .collect::<Vec<_>>();
                    (
                        if ids.is_empty() {
                            Disposition::New
                        } else {
                            Disposition::Conflict
                        },
                        ids,
                    )
                };
                Row {
                    profile,
                    disposition,
                    existing_ids,
                }
            })
            .collect();
        Ok(Self {
            registry_revision: registry.revision,
            source_snapshot_id,
            rows,
            before_digest: fingerprint(&registry.imported_profiles)?,
        })
    }
    pub fn apply(&self, registry: &mut Registry, choices: Vec<Choice>) -> Result<Applied> {
        registry.validate()?;
        if registry.revision != self.registry_revision
            || fingerprint(&registry.imported_profiles)? != self.before_digest
        {
            return Err("stale_registry");
        }
        if choices.len() > self.rows.len() {
            return Err("invalid_legacy_choices");
        }
        let mut selected = BTreeSet::new();
        for choice in &choices {
            if !selected.insert(&choice.source_id)
                || !self
                    .rows
                    .iter()
                    .any(|row| row.profile.id == choice.source_id)
            {
                return Err("invalid_legacy_choices");
            }
        }
        let mut next = registry.clone();
        let mut result = Applied {
            added: 0,
            reused: 0,
            skipped: 0,
            mappings: vec![],
        };
        for row in &self.rows {
            let decision = choices
                .iter()
                .find(|choice| choice.source_id == row.profile.id)
                .map(|choice| choice.decision)
                .unwrap_or(Decision::Skip);
            let imported_id = match (row.disposition, decision) {
                (_, Decision::Skip) => {
                    result.skipped += 1;
                    None
                }
                (Disposition::Identical, Decision::Reuse) => {
                    result.reused += 1;
                    Some(row.existing_ids[0].clone())
                }
                (Disposition::New, Decision::Import)
                | (Disposition::Conflict, Decision::KeepBoth) => {
                    let id = uuid::Uuid::new_v4().to_string();
                    next.imported_profiles.push(ImportedProfile {
                        id: id.clone(),
                        source_snapshot_id: self.source_snapshot_id.clone(),
                        profile: row.profile.clone(),
                        source_template_id: None,
                    });
                    result.added += 1;
                    Some(id)
                }
                _ => return Err("invalid_legacy_choices"),
            };
            result.mappings.push(Mapping {
                source_id: row.profile.id.clone(),
                imported_id,
            });
        }
        if result.added > 0 {
            next.revision = next.revision.checked_add(1).ok_or("invalid_revision")?;
            next.encode()?;
            *registry = next;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> ProjectProfile {
        serde_json::from_value(serde_json::json!({
            "id":uuid::Uuid::new_v4().to_string(),"name":"보관할 프로젝트",
            "windowsPath":"C:\\fixture\\project","wsl":{"distro":"Missing fixture distro","path":"/home/fixture/project"},
            "gitRoot":"C:\\fixture\\project","expectedPorts":[3000,5173],"runManagerServiceIds":["old-service"],
            "environment":{"enabled":false,"source":".env.local","revision":"a".repeat(64),"variables":[]}
        })).unwrap()
    }
    fn source(profile: ProjectProfile) -> ProfileStore {
        ProfileStore {
            version: 1,
            profiles: vec![profile],
        }
    }
    fn choose(profile: &ProjectProfile, decision: Decision) -> Vec<Choice> {
        vec![Choice {
            source_id: profile.id.clone(),
            decision,
        }]
    }
    #[test]
    fn metadata_import_preserves_old_ids_and_repeats_without_minting_projects_or_authority() {
        let profile = profile();
        let mut registry = Registry::default();
        let plan = Plan::build("a".repeat(64), source(profile.clone()), &registry).unwrap();
        let result = plan
            .apply(&mut registry, choose(&profile, Decision::Import))
            .unwrap();
        assert_eq!((result.added, result.reused, result.skipped), (1, 0, 0));
        assert_eq!(result.mappings[0].source_id, profile.id);
        assert_ne!(
            result.mappings[0].imported_id.as_ref().unwrap(),
            &profile.id
        );
        assert_eq!(registry.imported_profiles[0].profile, profile);
        assert!(
            registry.projects.is_empty()
                && registry.worktrees.is_empty()
                && registry.imported_profile_bindings.is_empty()
        );
        let saved = Registry::parse(&registry.encode().unwrap()).unwrap();
        let retry = Plan::build("b".repeat(64), source(profile.clone()), &saved).unwrap();
        assert_eq!(retry.rows[0].disposition, Disposition::Identical);
        let repeated = retry
            .apply(&mut registry, choose(&profile, Decision::Reuse))
            .unwrap();
        assert_eq!((repeated.added, repeated.reused), (0, 1));
        assert_eq!(
            repeated.mappings[0].imported_id,
            result.mappings[0].imported_id
        );
        assert_eq!(registry, saved);
    }
    #[test]
    fn conflicting_revisions_require_keep_both_and_never_overwrite_the_existing_record() {
        let mut profile = profile();
        let mut registry = Registry::default();
        Plan::build("a".repeat(64), source(profile.clone()), &registry)
            .unwrap()
            .apply(&mut registry, choose(&profile, Decision::Import))
            .unwrap();
        let saved = registry.clone();
        profile.expected_ports = vec![8080];
        let plan = Plan::build("b".repeat(64), source(profile.clone()), &registry).unwrap();
        assert_eq!(plan.rows[0].disposition, Disposition::Conflict);
        assert!(matches!(
            plan.apply(&mut registry, choose(&profile, Decision::Import)),
            Err("invalid_legacy_choices")
        ));
        assert_eq!(registry, saved);
        plan.apply(&mut registry, choose(&profile, Decision::Skip))
            .unwrap();
        assert_eq!(registry, saved);
        let result = plan
            .apply(&mut registry, choose(&profile, Decision::KeepBoth))
            .unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(registry.imported_profiles[0], saved.imported_profiles[0]);
        assert_eq!(registry.imported_profiles[1].profile, profile);
        assert_ne!(
            registry.imported_profiles[0].id,
            registry.imported_profiles[1].id
        );
    }
    #[test]
    fn changed_destination_and_foreign_or_duplicate_selections_are_atomic_failures() {
        let profile = profile();
        let mut registry = Registry::default();
        let plan = Plan::build("a".repeat(64), source(profile.clone()), &registry).unwrap();
        let before = registry.clone();
        let mut duplicate = choose(&profile, Decision::Import);
        duplicate.extend(choose(&profile, Decision::Import));
        for choices in [
            duplicate,
            vec![Choice {
                source_id: "foreign".into(),
                decision: Decision::Import,
            }],
        ] {
            assert!(matches!(
                plan.apply(&mut registry, choices),
                Err("invalid_legacy_choices")
            ));
            assert_eq!(registry, before);
        }
        registry.revision += 1;
        let changed = registry.clone();
        assert!(matches!(
            plan.apply(&mut registry, choose(&profile, Decision::Import)),
            Err("stale_registry")
        ));
        assert_eq!(registry, changed);
    }
    #[test]
    fn binding_and_unlink_preserve_imported_metadata_and_protect_referenced_worktrees() {
        use crate::core::registry::{Binding, ObjectStamp};
        let profile = profile();
        let mut registry = Registry::default();
        Plan::build("a".repeat(64), source(profile.clone()), &registry)
            .unwrap()
            .apply(&mut registry, choose(&profile, Decision::Import))
            .unwrap();
        let id = registry.imported_profiles[0].id.clone();
        let context = registry
            .register(
                registry.revision,
                "native project",
                Binding {
                    target: product_contract::ExecutionTarget::Windows,
                    root: profile.windows_path.clone().unwrap(),
                    root_object: ObjectStamp {
                        scope: "1".into(),
                        object: "2".into(),
                    },
                    repository_object: None,
                },
            )
            .unwrap();
        registry
            .bind_imported_profile(registry.revision, &id, &context)
            .unwrap();
        let before = registry.clone();
        registry
            .bind_imported_profile(registry.revision, &id, &context)
            .unwrap();
        assert_eq!(registry, before);
        assert_eq!(
            registry
                .imported_profile_for(&context)
                .unwrap()
                .unwrap()
                .profile,
            profile
        );
        assert!(registry.worktrees[0].trusted_digest.is_none());
        assert_eq!(
            registry.remove(registry.revision, &context),
            Err("referenced_worktree")
        );
        registry
            .unbind_imported_profile(registry.revision, &id, ProfileTarget::Windows)
            .unwrap();
        assert!(registry.imported_profile_for(&context).unwrap().is_none());
        registry.remove(registry.revision, &context).unwrap();
        assert_eq!(registry.imported_profiles[0], before.imported_profiles[0]);
    }
}
