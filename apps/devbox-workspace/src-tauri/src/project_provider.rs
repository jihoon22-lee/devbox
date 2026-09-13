//! On-demand native registry projection. It never starts a scheduler or a stopped
//! distro. Noncurrent roots are metadata only; the consumer marks them unverified.
use crate::core::registry::Registry;
use product_contract::{
    project_provider::{Availability, Delivery, Project, Snapshot, MAX_BYTES},
    ExecutionTarget,
};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
struct Published {
    epoch: String,
    revision: u64,
    snapshot: Option<Snapshot>,
}
pub(crate) fn snapshot(
    app: &tauri::AppHandle,
    registry: &Registry,
    verify_current: bool,
) -> Result<Value, &'static str> {
    let window = app
        .get_webview_window("main")
        .ok_or("workspace_window_unavailable")?;
    let selected = product_shell_tauri::workspace_context(&window)?;
    if registry.worktrees.len() > 256 {
        return Err("project_provider_limit");
    }
    let has_wsl = registry
        .worktrees
        .iter()
        .any(|tree| matches!(tree.binding.target, ExecutionTarget::Wsl { .. }));
    let distros = if has_wsl {
        crate::platform::wsl_distro::list()?
    } else {
        vec![]
    };
    let host = crate::component::provider_host(app)?;
    let owner = host.projects()?;
    let mut projects = Vec::new();
    for tree in &registry.worktrees {
        let context = tree.context();
        let (root, mut availability) = match &tree.binding.target {
            ExecutionTarget::Windows => (tree.binding.root.clone(), Availability::Unverified),
            ExecutionTarget::Wsl { distro_id } => {
                let Some(distro) = distros.iter().find(|distro| &distro.id == distro_id) else {
                    continue;
                };
                (
                    format!(
                        "//wsl$/{}/{}",
                        distro.name,
                        tree.binding.root.trim_start_matches('/')
                    ),
                    if distro.running {
                        Availability::Unverified
                    } else {
                        Availability::Offline
                    },
                )
            }
        };
        if verify_current
            && selected.as_ref() == Some(&context)
            && availability != Availability::Offline
        {
            availability = match &context.target {
                ExecutionTarget::Windows => match owner.admit(&context) {
                    Ok(lease) if lease.revalidate().is_ok() => Availability::Available,
                    _ => Availability::Missing,
                },
                ExecutionTarget::Wsl { .. } => {
                    #[cfg(windows)]
                    {
                        match host
                            .helper_directory()
                            .ok()
                            .and_then(|resources| owner.admit_wsl(resources, &context).ok())
                        {
                            Some(lease) if lease.revalidate().is_ok() => Availability::Available,
                            _ => Availability::Offline,
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        Availability::Offline
                    }
                }
            };
        }
        let mut activity_paths = vec![root.clone()];
        for alias in &tree.aliases {
            if devbox_filesystem::parse_safe_project_path(alias).is_some()
                && !activity_paths.contains(alias)
            {
                if activity_paths.len() == 16 {
                    return Err("project_provider_limit");
                }
                activity_paths.push(alias.clone());
            }
        }
        projects.push(Project {
            context,
            root,
            availability,
            activity_paths,
        });
    }
    if owner.snapshot()?.revision != registry.revision
        || product_shell_tauri::workspace_context(&window)? != selected
    {
        return Err("project_provider_stale");
    }
    let current =
        selected.filter(|context| projects.iter().any(|project| &project.context == context));
    let mut snapshot = Snapshot {
        schema_version: 1,
        revision: 1,
        current,
        projects,
    };
    static PUBLISHED: OnceLock<Mutex<Published>> = OnceLock::new();
    let mut published = PUBLISHED
        .get_or_init(|| {
            Mutex::new(Published {
                epoch: uuid::Uuid::new_v4().to_string(),
                revision: 0,
                snapshot: None,
            })
        })
        .lock()
        .map_err(|_| "project_provider_busy")?;
    // Compare content independently of the transport counter. Context-only changes
    // also advance it; a producer restart gets a different native epoch.
    snapshot.revision = published.revision;
    if published.snapshot.as_ref() != Some(&snapshot) {
        published.revision = published
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= 9_007_199_254_740_991)
            .ok_or("project_provider_limit")?;
        snapshot.revision = published.revision;
        published.snapshot = Some(snapshot.clone());
    }
    let delivery = Delivery {
        epoch: published.epoch.clone(),
        snapshot,
    };
    if serde_json::to_vec(&delivery)
        .map_err(|_| "project_provider_invalid")?
        .len()
        > MAX_BYTES
    {
        return Err("project_provider_limit");
    }
    serde_json::to_value(delivery).map_err(|_| "project_provider_invalid")
}

/// Selection notifications revoke Knowledge references. Delivery is best-effort;
/// every later query/open independently refreshes the producer snapshot as well.
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri::Listener;
    tauri::plugin::Builder::new("workspace-project-provider")
        .setup(|app, _| {
            let owner = app.clone();
            app.listen("workspace-context-changed", move |_| {
                let app = owner.clone();
                tauri::async_runtime::spawn(async move {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64;
                    let _ = crate::suite::remote(
                        &app,
                        "knowledge",
                        product_contract::transport::Call::InvalidateProjectSnapshot {},
                        now.saturating_add(2000),
                    )
                    .await;
                });
            });
            Ok(())
        })
        .build()
}
