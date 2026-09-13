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
fn runtime_metadata(
    app: &tauri::AppHandle,
    source: &Source,
) -> Result<(Index, bool), &'static str> {
    use run_manager_lib::component::search as runtime;
    let (kind, entity, selected) = match source {
        Source::Tasks => ("task", EntityKind::Task, runtime::Source::Tasks),
        Source::Services => ("service", EntityKind::Service, runtime::Source::Services),
        Source::Runs => ("run", EntityKind::Run, runtime::Source::Runs),
        _ => return Err("workspace_source_unavailable"),
    };
    let host = crate::component::provider_host(app)?;
    let root = host.component("runtime")?;
    let snapshot = runtime::read(&root, selected).map_err(|_| "workspace_runtime_unavailable")?;
    if host.component("runtime")? != root {
        return Err("workspace_runtime_stale");
    }
    let mut index = Index::default();
    for entry in snapshot.entries {
        if !commands::opaque_id(&entry.id) || !commands::opaque_id(&entry.job_id) {
            return Err("workspace_runtime_invalid");
        }
        let name = if entry.name.len() > 210
            || entry.name.chars().any(char::is_control)
            || devbox_applink::contains_sensitive_value(&entry.name)
        {
            "이름 숨김"
        } else {
            &entry.name
        };
        let revision =
            Sha256::digest(serde_json::to_vec(&entry).map_err(|_| "workspace_runtime_invalid")?)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
        index.insert(Descriptor {
            id: format!("workspace.{kind}-{}", entry.id),
            owner: "workspace".into(),
            component: "workspace.runtime".into(),
            label: format!("{name} · {}", &entry.id[..entry.id.len().min(8)]),
            revision,
            target: Target::Entity {
                entity: entity.clone(),
                id: entry.id,
            },
            review_route: Some("tasks".into()),
            required_context: ContextRequirement::None,
            context: None,
            destructive: false,
            requires_review: true,
            disabled_reason: None,
        })?;
    }
    Ok((index, snapshot.truncated))
}
fn terminal_metadata(app: &tauri::AppHandle) -> Result<Index, &'static str> {
    let host = crate::component::provider_host(app)?;
    let catalog = crate::component::terminal_owner(app)?.command_catalog(app, &host)?;
    let mut index = Index::default();
    for window in catalog["windows"]
        .as_array()
        .ok_or("workspace_terminal_invalid")?
    {
        let id = window["id"]
            .as_str()
            .filter(|id| commands::opaque_id(id))
            .ok_or("workspace_terminal_invalid")?;
        let context: Option<ProjectContext> = serde_json::from_value(window["context"].clone())
            .map_err(|_| "workspace_terminal_invalid")?;
        let revision =
            Sha256::digest(serde_json::to_vec(window).map_err(|_| "workspace_terminal_invalid")?)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
        index.insert(Descriptor {
            id: format!("workspace.summon-terminal-{id}"),
            owner: "workspace".into(),
            component: "workspace.terminal".into(),
            label: format!("터미널 표시·숨김 · {}", &id[..id.len().min(8)]),
            revision,
            target: Target::Entity {
                entity: EntityKind::TerminalWindow,
                id: id.into(),
            },
            review_route: Some("terminal".into()),
            required_context: if context.is_some() {
                ContextRequirement::Project
            } else {
                ContextRequirement::None
            },
            context,
            destructive: false,
            requires_review: false,
            disabled_reason: None,
        })?;
    }
    for profile in catalog["profiles"]
        .as_array()
        .ok_or("workspace_terminal_invalid")?
    {
        let id = profile["id"]
            .as_str()
            .filter(|id| commands::opaque_id(id))
            .ok_or("workspace_terminal_invalid")?;
        let name = profile["name"]
            .as_str()
            .ok_or("workspace_terminal_invalid")?;
        let label = if name.len() > 200 || devbox_applink::contains_sensitive_value(name) {
            "저장한 터미널 프로필"
        } else {
            name
        };
        index.insert(Descriptor {
            id: format!("workspace.terminal-profile-{id}"),
            owner: "workspace".into(),
            component: "workspace.terminal".into(),
            label: label.into(),
            revision: profile["revision"]
                .as_str()
                .ok_or("workspace_terminal_invalid")?
                .into(),
            target: Target::Entity {
                entity: EntityKind::TerminalProfile,
                id: id.into(),
            },
            review_route: Some("terminal".into()),
            required_context: ContextRequirement::None,
            context: None,
            destructive: false,
            requires_review: true,
            disabled_reason: None,
        })?;
    }
    Ok(index)
}
fn shortcut(
    app: &tauri::AppHandle,
    registry: &Registry,
    command: &str,
) -> Result<Descriptor, &'static str> {
    use tauri::Manager;
    let window = app
        .get_webview_window("main")
        .ok_or("workspace_window_unavailable")?;
    let context = product_shell_tauri::workspace_context(&window)?;
    if command == "workspace.open-current-project" {
        let context = context.ok_or("workspace_context_required")?;
        let mut index = metadata(registry, &Source::Projects)?;
        index.retain_context(&context);
        return index
            .search("")?
            .results
            .into_iter()
            .next()
            .ok_or("workspace_context_stale");
    }
    if command != "workspace.summon-terminal" {
        return Err("workspace_shortcut_unavailable");
    }
    let windows = terminal_metadata(app)?
        .search("")?
        .results
        .into_iter()
        .filter(|row| {
            matches!(
                row.target,
                Target::Entity {
                    entity: EntityKind::TerminalWindow,
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    if let Some(focused)=windows.iter().find(|row|matches!(&row.target,Target::Entity{id,..} if app.get_webview_window(&format!("terminal-{id}")).is_some_and(|window|window.is_focused().unwrap_or(false)))){return Ok(focused.clone());}
    let current = windows
        .into_iter()
        .filter(|row| row.context == context)
        .collect::<Vec<_>>();
    if current.len() == 1 {
        return Ok(current[0].clone());
    }
    let catalog =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?;
    Index::catalog(
        &catalog,
        &std::collections::BTreeSet::from(["workspace".into()]),
    )?
    .search("")?
    .results
    .into_iter()
    .find(|row| matches!(&row.target,Target::Route{route} if route=="terminal"))
    .ok_or("workspace_shortcut_unavailable")
}
pub(crate) fn handle(
    app: tauri::AppHandle,
    call: Call,
    _deadline: u64,
    cancellation: Option<product_contract::query::Cancellation>,
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
            if cancellation.as_ref().is_some_and(|token|token.requested()){return Err("query_cancelled");}
            let registry=crate::component::provider_host(&app)?.projects()?.snapshot()?;
            match call {
                Call::ReadSessionSummary { source_id } => serde_json::to_value(crate::session_summary::delivery(&app, &source_id).map_err(|_| "workspace_summary_stale")?).map_err(|_| "workspace_summary_invalid"),
                Call::LegacyCommandMappings { ids } => {
                    use crate::core::{legacy_references, registry::LegacyOwner};
                    let mut mappings = std::collections::BTreeMap::new();
                    for id in ids {
                        let parsed = id.strip_prefix("snapshot/workbench/").map(|old| (old, LegacyOwner::Workbench, "project"))
                            .or_else(|| id.strip_prefix("snapshot/repo-manager/").map(|old| (old, LegacyOwner::RepoManager, "worktree")));
                        let Some((old_id, owner, kind)) = parsed else { continue; };
                        let resolved = legacy_references::resolve(&registry, &legacy_references::Query { registry_revision: registry.revision, owner, old_id: old_id.into(), target: None, imported_id: None })?;
                        if resolved.state == legacy_references::State::Resolved {
                            let context = &resolved.candidates[0].context;
                            if kind == "worktree" && !registry.worktrees.iter().any(|tree| tree.id == context.worktree_id && tree.repo_id.is_some()) { continue; }
                            mappings.insert(id, format!("workspace.{kind}-{}", context.worktree_id));
                        }
                    }
                    serde_json::to_value(mappings).map_err(|_| "workspace_mapping_invalid")
                }
                Call::ProjectSnapshot { verify_current } => crate::project_provider::snapshot(&app, &registry, verify_current),
                Call::ResolveShortcut{command}=>serde_json::to_value(shortcut(&app,&registry,&command)?).map_err(|_|"workspace_command_invalid"),
                Call::Query{source:Source::Commands,query,generation,..}=>{
                    let mut index=terminal_metadata(&app)?;
                    if let Ok(current)=shortcut(&app,&registry,"workspace.open-current-project"){index.insert(current)?;}
                    Ok(json!({"generation":generation,"source":"commands","owner":"workspace","result":index.search(&query)?}))
                }
                Call::OpenCommand{request} if request.command_id.starts_with("workspace.summon-terminal-")=>{
                    let index=terminal_metadata(&app)?;let selected=index.resolve(&request)?;
                    let Target::Entity{entity:EntityKind::TerminalWindow,id}=&selected.target else{return Err("workspace_command_invalid");};
                    crate::component::terminal_owner(&app)?.summon(&app,id,selected.context.as_ref(),&request.operation_id,_deadline)
                }
                Call::Query {source:source @ (Source::Projects|Source::Repositories),query,generation,context,mode,..}=>{
                    if mode!=product_contract::transport::QueryMode::Name{return Err("workspace_query_mode_unavailable");}
                    commands::validate_query(&query)?;
                    if let Some(context)=&context {if !registry.worktrees.iter().any(|tree|tree.context()==*context){return Err("stale_context");}}
                    let mut index=metadata(&registry,&source)?;if let Some(context)=&context{index.retain_context(context);}
                    let result=index.search(&query)?;
                    if cancellation.as_ref().is_some_and(|token|token.requested()){return Err("query_cancelled");}
                    Ok(json!({"generation":generation,"source":source,"owner":"workspace","result":result}))
                }
                Call::Query{source:source @ (Source::Tasks|Source::Services|Source::Runs),query,generation,mode,..}=>{
                    if mode!=product_contract::transport::QueryMode::Name{return Err("workspace_query_mode_unavailable");}
                    let (index,truncated)=runtime_metadata(&app,&source)?;let mut result=index.search(&query)?;result.truncated|=truncated;
                    if cancellation.as_ref().is_some_and(|token|token.requested()){return Err("query_cancelled");}
                    Ok(json!({"generation":generation,"source":source,"owner":"workspace","result":result}))
                }
                Call::PreviewCommand{request}=>{
                    if request.command_id.starts_with("workspace.summon-terminal-")||request.command_id.starts_with("workspace.terminal-profile-"){
                        return serde_json::to_value(terminal_metadata(&app)?.resolve(&request)?).map_err(|_|"workspace_command_invalid");
                    }
                    for (prefix,source) in [("workspace.task-",Source::Tasks),("workspace.service-",Source::Services),("workspace.run-",Source::Runs)]{
                        if request.command_id.starts_with(prefix){return serde_json::to_value(runtime_metadata(&app,&source)?.0.resolve(&request)?).map_err(|_|"workspace_command_invalid");}
                    }

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
