//! Workspace-owned metadata queries never start the Runtime scheduler or probe
//! project files. The existing Registry UI owns subsequent selection and trust.
use crate::core::registry::Registry;
use product_contract::{
    command_index::Index,
    commands::{self, ContextRequirement, Descriptor, EntityKind, Target},
    transport::{Call, Source},
    ProjectContext,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};
fn metadata(registry: &Registry, source: &Source) -> Result<Index, &'static str> {
    let mut index = Index::default();
    for tree in &registry.worktrees {
        if matches!(source, Source::Repositories) && tree.repo_id.is_none() {
            continue;
        }
        let project = registry
            .projects
            .iter()
            .find(|project| project.id == tree.project_id)
            .ok_or("registry_invalid")?;
        let prefix = if matches!(source, Source::Repositories) {
            "worktree"
        } else {
            "project"
        };
        let revision = Sha256::digest(
            serde_json::to_vec(&(registry.revision, &tree.id, tree.revision, &project.name))
                .map_err(|_| "registry_invalid")?,
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
        let name = if project.name.len() > 220
            || project.name.chars().any(char::is_control)
            || devbox_applink::contains_sensitive_value(&project.name)
        {
            "이름 숨김"
        } else {
            &project.name
        };
        let context = ProjectContext {
            project_id: tree.project_id.clone(),
            worktree_id: tree.id.clone(),
            target: tree.binding.target.clone(),
            revision: tree.revision,
        };
        index.insert(Descriptor {
            id: format!("workspace.{prefix}-{}", tree.id),
            owner: "workspace".into(),
            component: "workspace.registry".into(),
            label: format!("{name} · {}", &tree.id[..tree.id.len().min(8)]),
            revision,
            target: Target::Entity {
                entity: if prefix == "project" {
                    EntityKind::Project
                } else {
                    EntityKind::Worktree
                },
                id: if prefix == "project" {
                    tree.project_id.clone()
                } else {
                    tree.id.clone()
                },
            },
            review_route: Some("overview".into()),
            required_context: ContextRequirement::Project,
            context: Some(context),
            destructive: false,
            requires_review: true,
            disabled_reason: None,
        })?;
    }
    Ok(index)
}
pub(crate) fn handle(
    app: tauri::AppHandle,
    call: Call,
    _deadline: u64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, &'static str>> + Send>> {
    Box::pin(async move {
        static READERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
        let permit = READERS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace_source_busy")?;
        tokio::task::spawn_blocking(move||{
            let _permit=permit;
            let registry=crate::component::provider_host(&app)?.projects()?.snapshot()?;
            match call {
                Call::Query {source:source @ (Source::Projects|Source::Repositories),query,generation,context}=>{
                    commands::validate_query(&query)?;
                    if let Some(context)=&context {if !registry.worktrees.iter().any(|tree|tree.context()==*context){return Err("stale_context");}}
                    let mut index=metadata(&registry,&source)?;if let Some(context)=&context{index.retain_context(context);}
                    let result=index.search(&query)?;
                    Ok(json!({"generation":generation,"source":source,"owner":"workspace","result":result}))
                }
                Call::PreviewCommand{request}=>{
                    let source=if request.command_id.starts_with("workspace.project-"){Source::Projects}else if request.command_id.starts_with("workspace.worktree-"){Source::Repositories}else{return Err("workspace_command_unavailable");};
                    let index=metadata(&registry,&source)?;
                    serde_json::to_value(index.resolve(&request)?).map_err(|_|"workspace_command_invalid")
                }
                _=>Err("workspace_source_unavailable"),
            }
        }).await.map_err(|_|"workspace_source_unavailable")?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::{Binding, ObjectStamp};
    use product_contract::ExecutionTarget;
    fn binding(root: &str, object: &str, repo: Option<&str>) -> Binding {
        Binding {
            target: ExecutionTarget::Windows,
            root: root.into(),
            root_object: ObjectStamp {
                scope: "1".into(),
                object: object.into(),
            },
            repository_object: repo.map(|id| ObjectStamp {
                scope: "1".into(),
                object: id.into(),
            }),
        }
    }
    #[test]
    fn same_name_projects_keep_distinct_native_targets_and_no_paths_in_metadata() {
        let mut registry = Registry::default();
        let first = registry
            .register(1, "same", binding("C:/fixture-one", "a", Some("aa")))
            .unwrap();
        registry
            .register(
                registry.revision,
                "same",
                binding("C:/fixture-two", "b", None),
            )
            .unwrap();
        let result = metadata(&registry, &Source::Projects)
            .unwrap()
            .search("same")
            .unwrap();
        assert_eq!(result.results.len(), 2);
        assert_ne!(result.results[0].id, result.results[1].id);
        assert_ne!(result.results[0].context, result.results[1].context);
        assert!(!serde_json::to_string(&result).unwrap().contains("C:/"));
        assert!(result
            .results
            .iter()
            .all(|row| row.requires_review && row.review_route.as_deref() == Some("overview")));
        assert_eq!(
            metadata(&registry, &Source::Repositories)
                .unwrap()
                .search("")
                .unwrap()
                .results
                .len(),
            1
        );
        let descriptor = result
            .results
            .iter()
            .find(|row| row.context.as_ref() == Some(&first))
            .unwrap();
        let request = commands::Request {
            operation_id: "fixture-open".into(),
            command_id: descriptor.id.clone(),
            revision: descriptor.revision.clone(),
            context: descriptor.context.clone(),
            selection_id: None,
        };
        registry
            .rename(registry.revision, &first.project_id, "renamed")
            .unwrap();
        assert!(metadata(&registry, &Source::Projects)
            .unwrap()
            .resolve(&request)
            .is_err());
    }
}
