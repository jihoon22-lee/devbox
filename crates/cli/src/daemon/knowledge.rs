//! S3 knowledge composition: document storage and the roots it may write.
use devbox_core::{Hub, Paths};
use devbox_documents::{DocumentsConfig, DocumentsService, Hook, Root, Roots};
use devbox_projects::ProjectsService;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub(super) struct Knowledge {
    pub documents: Arc<DocumentsService>,
}

/// `<parent>/<name>-agents`, the folder `devbox_agents::plan::worktree_dir` uses.
fn agents_dir(project: &Path) -> Option<PathBuf> {
    let name = project.file_name()?.to_str()?;
    Some(project.parent()?.join(format!("{name}-agents")))
}

/// Registered projects, their agent folders and the vault, recomputed per
/// request (like `logs::roots`). The data folder is not a root: no Task 2
/// caller writes there, and a later task adds a dedicated subfolder if needed.
fn roots(paths: &Paths, projects: Arc<ProjectsService>) -> Roots {
    // D15 default; the configurable notes.vault belongs to S3 Task 4.
    let vault = paths.home.join("notes");
    Arc::new(move || {
        let (projects, vault) = (projects.clone(), vault.clone());
        Box::pin(async move {
            let mut roots = vec![Root {
                id: "vault".into(),
                path: vault,
            }];
            match projects.list(devbox_projects::ListProjects {}).await {
                Ok(list) => {
                    for project in list.items {
                        let path = PathBuf::from(&project.path);
                        if let Some(agents) = agents_dir(&path) {
                            roots.push(Root {
                                id: format!("agents:{}", project.id),
                                path: agents,
                            });
                        }
                        roots.push(Root {
                            id: format!("project:{}", project.id),
                            path,
                        });
                    }
                }
                Err(_) => tracing::warn!("document roots computed without projects"),
            }
            roots
        })
    })
}

/// Test instances may pause claims after the disk read: the hook writes
/// `<gate>/reached` and waits for `<gate>/release` (daemon integration tests).
fn test_gate(paths: &Paths) -> Option<Hook> {
    let gate = PathBuf::from(std::env::var_os("DEVBOX_TEST_DOCUMENTS_GATE")?);
    if !paths.instance.is_test() {
        return None;
    }
    Some(Arc::new(move || {
        let gate = gate.clone();
        Box::pin(async move {
            let _ = std::fs::write(gate.join("reached"), b"");
            while !gate.join("release").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    }))
}

/// Daemon-owned files stay out of reach even when a project contains them: data,
/// state, the installed binary and this instance's systemd user units (the names
/// `setup::instance_units` and `render_units` publish). `config_dir` is not listed:
/// it holds user-edited `config.toml`, and S3 Task 4 settings editing goes there.
fn private(paths: &Paths) -> Vec<PathBuf> {
    let channel = paths.instance.channel().config();
    let (daemon, tmux, instance) = (
        channel.daemon_unit,
        channel.tmux_unit,
        paths.instance.as_str(),
    );
    let units = paths.xdg_config_home.join("systemd/user");
    let mut private = vec![
        paths.data_dir.clone(),
        paths.state_dir.clone(),
        paths.bin_dir.clone(),
    ];
    private.extend(
        [
            format!("{daemon}@.socket"),
            format!("{daemon}@.service"),
            format!("{tmux}@.service"),
            format!("{daemon}@{instance}.service.d"),
            format!("{daemon}@{instance}.socket.d"),
            format!("{tmux}@{instance}.service.d"),
            format!("sockets.target.wants/{daemon}@{instance}.socket"),
            format!("default.target.wants/{daemon}@{instance}.service"),
            format!("default.target.wants/{tmux}@{instance}.service"),
        ]
        .into_iter()
        .map(|name| units.join(name)),
    );
    private
}

pub(super) fn build(paths: &Paths, hub: &Arc<Hub>, projects: &Arc<ProjectsService>) -> Knowledge {
    let mut config = DocumentsConfig::new(
        paths.data_dir.clone(),
        roots(paths, projects.clone()),
        hub.clone(),
    );
    config.hooks.before_lease_insert = test_gate(paths);
    config.private = private(paths);
    Knowledge {
        documents: Arc::new(DocumentsService::new(config)),
    }
}
