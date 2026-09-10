//! Read-only ID resolution for imported metadata and native owner mappings.
//! Candidates never select a project, probe a root or grant execution authority.
use super::registry::{LegacyOwner, Registry};
use product_contract::{ExecutionTarget, ProjectContext};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub registry_revision: u64,
    pub owner: LegacyOwner,
    pub old_id: String,
    pub target: Option<ExecutionTarget>,
    /// Select an exact preserved import when keep-both retained the same old ID.
    pub imported_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    Explicit,
    ImportedProfile {
        #[serde(rename = "importedId")]
        imported_id: String,
        #[serde(rename = "sourceSnapshotId")]
        source_snapshot_id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub context: ProjectContext,
    pub origins: Vec<Origin>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Unmapped,
    Resolved,
    Ambiguous,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub schema_version: u32,
    pub registry_revision: u64,
    pub owner: LegacyOwner,
    pub old_id: String,
    pub state: State,
    pub candidates: Vec<Candidate>,
}

pub fn resolve(registry: &Registry, query: &Query) -> Result<Resolution, &'static str> {
    registry.validate()?;
    if query.registry_revision != registry.revision {
        return Err("stale_registry");
    }
    if query.old_id.trim().is_empty()
        || query.old_id.len() > 32_768
        || query.old_id.chars().any(char::is_control)
        || query.imported_id.as_ref().is_some_and(|id| {
            query.owner != LegacyOwner::Workbench || uuid::Uuid::parse_str(id).is_err()
        })
    {
        return Err("invalid_legacy_reference");
    }
    if let Some(target) = &query.target {
        ProjectContext {
            project_id: "query".into(),
            worktree_id: "query".into(),
            target: target.clone(),
            revision: 1,
        }
        .validate()
        .map_err(|_| "invalid_target")?;
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut add = |worktree_id: &str, origin: Origin| {
        let Some(tree) = registry
            .worktrees
            .iter()
            .find(|tree| tree.id == worktree_id)
        else {
            return;
        };
        if query
            .target
            .as_ref()
            .is_some_and(|target| target != &tree.binding.target)
        {
            return;
        }
        if let Some(candidate) = candidates
            .iter_mut()
            .find(|candidate| candidate.context.worktree_id == tree.id)
        {
            if !candidate.origins.contains(&origin) {
                candidate.origins.push(origin);
            }
        } else {
            candidates.push(Candidate {
                context: tree.context(),
                origins: vec![origin],
            });
        }
    };
    if query.imported_id.is_none() {
        for reference in &registry.legacy_references {
            if reference.owner == query.owner && reference.old_id == query.old_id {
                add(&reference.worktree_id, Origin::Explicit);
            }
        }
    }
    if query.owner == LegacyOwner::Workbench {
        for profile in &registry.imported_profiles {
            let Some(snapshot) = &profile.source_snapshot_id else {
                continue;
            };
            if profile.local
                || profile.source_template_id.is_some()
                || profile.profile.id != query.old_id
                || query
                    .imported_id
                    .as_ref()
                    .is_some_and(|id| id != &profile.id)
            {
                continue;
            }
            for binding in &registry.imported_profile_bindings {
                if binding.imported_id == profile.id {
                    add(
                        &binding.worktree_id,
                        Origin::ImportedProfile {
                            imported_id: profile.id.clone(),
                            source_snapshot_id: snapshot.clone(),
                        },
                    );
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.context.worktree_id.cmp(&b.context.worktree_id));
    Ok(Resolution {
        schema_version: 1,
        registry_revision: registry.revision,
        owner: query.owner.clone(),
        old_id: query.old_id.clone(),
        state: match candidates.len() {
            0 => State::Unmapped,
            1 => State::Resolved,
            _ => State::Ambiguous,
        },
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        legacy_profiles::{ImportedProfile, ProfileTarget},
        registry::{Binding, LegacyReference, ObjectStamp},
    };

    fn binding(target: ExecutionTarget, root: &str, object: &str) -> Binding {
        Binding {
            target,
            root: root.into(),
            root_object: ObjectStamp {
                scope: "aa".into(),
                object: object.into(),
            },
            repository_object: None,
        }
    }
    fn fixture() -> (Registry, Query, Vec<ProjectContext>) {
        let mut registry = Registry::default();
        let mut profile = workbench_lib::component::ProjectProfile::new("preserved profile");
        profile.windows_path = Some("C:\\fixture".into());
        profile.wsl = Some(workbench_lib::component::WslProfile {
            distro: "Fixture".into(),
            path: "/한글/Project".into(),
        });
        let old_id = profile.id.clone();
        for index in 1..=2 {
            profile.name = format!("preserved version {index}");
            registry.imported_profiles.push(ImportedProfile {
                id: uuid::Uuid::new_v4().to_string(),
                source_snapshot_id: Some(index.to_string().repeat(64)),
                local: false,
                source_template_id: None,
                profile: profile.clone(),
            });
        }
        let mut contexts = Vec::new();
        for (index, target, root) in [
            (0, ExecutionTarget::Windows, "C:\\fixture"),
            (
                0,
                ExecutionTarget::Wsl {
                    distro_id: "distro-one".into(),
                },
                "/한글/Project",
            ),
            (1, ExecutionTarget::Windows, "D:\\fixture"),
        ] {
            let context = registry
                .register(
                    registry.revision,
                    "project",
                    binding(target, root, &format!("{:02x}", contexts.len() + 1)),
                )
                .unwrap();
            let imported_id = registry.imported_profiles[index].id.clone();
            registry
                .bind_imported_profile(registry.revision, &imported_id, &context)
                .unwrap();
            contexts.push(context);
        }
        let query = Query {
            registry_revision: registry.revision,
            owner: LegacyOwner::Workbench,
            old_id,
            target: None,
            imported_id: None,
        };
        (registry, query, contexts)
    }
    #[test]
    fn keep_both_and_dual_targets_return_candidates_without_inventing_ids_or_authority() {
        let (registry, mut query, contexts) = fixture();
        let before = registry.encode().unwrap();
        let result = resolve(&registry, &query).unwrap();
        assert_eq!(result.state, State::Ambiguous);
        assert_eq!(result.candidates.len(), 3);
        assert_eq!(result.old_id, query.old_id);
        query.target = Some(ExecutionTarget::Windows);
        assert_eq!(resolve(&registry, &query).unwrap().candidates.len(), 2);
        query.imported_id = Some(registry.imported_profiles[0].id.clone());
        let result = resolve(&registry, &query).unwrap();
        assert_eq!(result.state, State::Resolved);
        assert_eq!(result.candidates[0].context, contexts[0]);
        assert_eq!(
            result.candidates[0].origins,
            vec![Origin::ImportedProfile {
                imported_id: registry.imported_profiles[0].id.clone(),
                source_snapshot_id: "1".repeat(64)
            }]
        );
        query.target = Some(ExecutionTarget::Wsl {
            distro_id: "another-distro".into(),
        });
        assert_eq!(resolve(&registry, &query).unwrap().state, State::Unmapped);
        assert_eq!(registry.encode().unwrap(), before);
        assert!(registry
            .worktrees
            .iter()
            .all(|tree| tree.trusted_digest.is_none()));
    }
    #[test]
    fn rebind_and_unlink_revoke_old_revisions_and_never_return_an_unbound_import() {
        let (mut registry, mut query, contexts) = fixture();
        query.imported_id = Some(registry.imported_profiles[0].id.clone());
        query.target = Some(ExecutionTarget::Windows);
        let moved = registry
            .rebind(
                registry.revision,
                &contexts[0],
                binding(ExecutionTarget::Windows, "E:\\renamed", "01"),
            )
            .unwrap();
        assert_eq!(resolve(&registry, &query).unwrap_err(), "stale_registry");
        query.registry_revision = registry.revision;
        assert_eq!(
            resolve(&registry, &query).unwrap().candidates[0].context,
            moved
        );
        registry
            .unbind_imported_profile(
                registry.revision,
                query.imported_id.as_ref().unwrap(),
                ProfileTarget::Windows,
            )
            .unwrap();
        query.registry_revision = registry.revision;
        assert_eq!(resolve(&registry, &query).unwrap().state, State::Unmapped);
        assert_eq!(registry.imported_profiles.len(), 2);
        assert_eq!(registry.worktrees.len(), 3);
    }
    #[test]
    fn explicit_owner_namespaces_are_exact_and_malformed_filters_cannot_widen_resolution() {
        let (mut registry, mut query, contexts) = fixture();
        for (index, owner) in [
            LegacyOwner::LifeLog,
            LegacyOwner::RepoManager,
            LegacyOwner::Terminal,
            LegacyOwner::Task,
        ]
        .into_iter()
        .enumerate()
        {
            let reference = LegacyReference {
                owner: owner.clone(),
                old_id: "preserved-old-id".into(),
                worktree_id: contexts[index % 3].worktree_id.clone(),
            };
            registry.map_legacy(registry.revision, reference).unwrap();
            query = Query {
                registry_revision: registry.revision,
                owner,
                old_id: "preserved-old-id".into(),
                target: None,
                imported_id: None,
            };
            let result = resolve(&registry, &query).unwrap();
            assert_eq!(result.state, State::Resolved);
            assert_eq!(result.candidates[0].context, contexts[index % 3]);
        }
        query.owner = LegacyOwner::Workbench;
        assert_eq!(resolve(&registry, &query).unwrap().state, State::Unmapped);
        query.imported_id = Some("not-an-import-id".into());
        assert_eq!(
            resolve(&registry, &query).unwrap_err(),
            "invalid_legacy_reference"
        );
        query.imported_id = None;
        query.old_id = "x".repeat(32_769);
        assert_eq!(
            resolve(&registry, &query).unwrap_err(),
            "invalid_legacy_reference"
        );
        query.old_id = "valid".into();
        query.target = Some(ExecutionTarget::Wsl {
            distro_id: "../foreign".into(),
        });
        assert_eq!(resolve(&registry, &query).unwrap_err(), "invalid_target");
        assert!(serde_json::from_value::<Query>(serde_json::json!({"registryRevision": registry.revision,"owner":"task","oldId":"id","execute":true})).is_err());
    }
}
