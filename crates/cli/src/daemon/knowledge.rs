//! S3 knowledge composition: document storage, the roots it may write, the
//! note vault on top of it and search, fed by the note index.
use devbox_core::{Db, Hub, IndexDb, Paths};
use devbox_documents::{DocumentsConfig, DocumentsService, Hook, Root, Roots};
use devbox_notes::{NoteSink, NotesConfig, NotesService, NotesSettings, PcName, SinkNote};
use devbox_projects::ProjectsService;
use devbox_protocol::router::BoxFuture;
use devbox_search::{
    IndexedNote, ProjectRoot, ProjectRoots, SearchConfig, SearchService, SearchSettings,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub(super) struct Knowledge {
    pub documents: Arc<DocumentsService>,
    pub notes: Arc<NotesService>,
    pub search: Arc<SearchService>,
}

/// Note index results go to search (the two crates do not depend on each other).
struct SearchSink(Arc<SearchService>);
impl NoteSink for SearchSink {
    fn indexed(&self, notes: Vec<SinkNote>) -> BoxFuture<()> {
        let search = self.0.clone();
        let notes = notes
            .into_iter()
            .map(|n| IndexedNote {
                path: n.path,
                title: n.title,
                body: n.body,
            })
            .collect();
        Box::pin(async move { search.index_notes(notes).await })
    }
    fn removed(&self, paths: Vec<String>) -> BoxFuture<()> {
        let search = self.0.clone();
        Box::pin(async move { search.remove_notes(paths).await })
    }
    fn retain(&self, paths: Vec<String>) -> BoxFuture<()> {
        let search = self.0.clone();
        Box::pin(async move { search.retain(paths).await })
    }
}

/// Registered projects for code and file-name search, read per query.
fn project_roots(projects: Arc<ProjectsService>) -> ProjectRoots {
    Arc::new(move || {
        let projects = projects.clone();
        Box::pin(async move {
            match projects.list(devbox_projects::ListProjects {}).await {
                Ok(list) => list
                    .items
                    .into_iter()
                    .map(|p| ProjectRoot {
                        id: p.id,
                        path: PathBuf::from(p.path),
                    })
                    .collect(),
                Err(_) => {
                    tracing::warn!("search ran without projects");
                    Vec::new()
                }
            }
        })
    })
}

/// `<parent>/<name>-agents`, the folder `devbox_agents::plan::worktree_dir` uses.
fn agents_dir(project: &Path) -> Option<PathBuf> {
    let name = project.file_name()?.to_str()?;
    Some(project.parent()?.join(format!("{name}-agents")))
}

/// Registered projects, their agent folders and the configured vault, recomputed
/// per request (like `logs::roots`). The data folder is not a root: no Task 2
/// caller writes there, and a later task adds a dedicated subfolder if needed.
fn roots(vault: PathBuf, projects: Arc<ProjectsService>) -> Roots {
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

/// Debug builds of test instances may stop a note rename at a crash point
/// (`DEVBOX_TEST_NOTES_STOP`, see `devbox_notes::StopAt::parse`).
fn test_stop(paths: &Paths) -> Option<devbox_notes::StopAt> {
    if !cfg!(debug_assertions) || !paths.instance.is_test() {
        return None;
    }
    devbox_notes::StopAt::parse(&std::env::var("DEVBOX_TEST_NOTES_STOP").ok()?)
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

/// `[pc] name`, else the host name, for `{{pc}}`.
fn pc_name(configured: Option<String>) -> PcName {
    Arc::new(move || {
        let configured = configured.clone();
        Box::pin(async move {
            match configured {
                Some(name) => name,
                None => super::hostname::local_hostname()
                    .await
                    .unwrap_or_else(|| "pc".into()),
            }
        })
    })
}

/// Registered project folder names as activity attribution candidates.
fn project_names(projects: Arc<ProjectsService>) -> devbox_activity::ProjectNames {
    Arc::new(move || {
        let projects = projects.clone();
        Box::pin(async move {
            match projects.list(devbox_projects::ListProjects {}).await {
                Ok(list) => list
                    .items
                    .into_iter()
                    .filter_map(|p| {
                        let name = Path::new(&p.path).file_name()?.to_str()?.to_owned();
                        Some(devbox_activity::model::attribution::ProjectMatch {
                            project_id: p.id,
                            basenames: vec![name],
                        })
                    })
                    .collect(),
                Err(_) => {
                    tracing::warn!("activity attributed without projects");
                    Vec::new()
                }
            }
        })
    })
}

/// Activity from `config.toml [activity]` (unreadable rules store no titles),
/// stored in `devbox.db`, exported to `<data>/exports`.
pub(super) fn activity(
    paths: &Paths,
    settings: devbox_activity::ActivitySettings,
    db: &Db,
    hub: &Arc<Hub>,
    projects: &Arc<ProjectsService>,
) -> Arc<devbox_activity::ActivityService> {
    if !settings.readable {
        tracing::warn!("[activity] settings are unreadable; titles are not stored");
    }
    Arc::new(devbox_activity::ActivityService::new(
        devbox_activity::ActivityConfig::new(
            db.clone(),
            hub.clone(),
            settings,
            paths.config_dir.clone(),
            paths.data_dir.join("exports"),
            project_names(projects.clone()),
        ),
    ))
}

/// `pc` is `config.pc.name`.
pub(super) fn build(
    paths: &Paths,
    (settings, search, pc): (NotesSettings, SearchSettings, Option<String>),
    db: &Db,
    index: &IndexDb,
    hub: &Arc<Hub>,
    projects: &Arc<ProjectsService>,
) -> anyhow::Result<Knowledge> {
    let mut documents_config = DocumentsConfig::new(
        paths.data_dir.clone(),
        roots(settings.vault.clone(), projects.clone()),
        hub.clone(),
    );
    documents_config.hooks.before_lease_insert = test_gate(paths);
    documents_config.private = private(paths);
    let documents = Arc::new(DocumentsService::new(documents_config));
    let mut search_config = SearchConfig::new(
        search,
        index.clone(),
        db.clone(),
        hub.clone(),
        project_roots(projects.clone()),
    );
    search_config.private = private(paths);
    search_config.replay_marker = Some(paths.data_dir.join("search-reindex"));
    let search = Arc::new(SearchService::new(search_config)?);
    let mut notes_config = NotesConfig::new(
        settings,
        documents.clone(),
        index.clone(),
        hub.clone(),
        pc_name(pc),
        paths.data_dir.join("notes-rename"),
    );
    notes_config.stop = test_stop(paths);
    notes_config.sink = Arc::new(SearchSink(search.clone()));
    // A new or rebuilt search domain gets every note after the first scan.
    notes_config.replay_sink = search.rebuilt();
    let notes = NotesService::new(notes_config)?;
    Ok(Knowledge {
        documents,
        notes: Arc::new(notes),
        search,
    })
}
