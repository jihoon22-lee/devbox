//! 프로젝트 목록 도메인.
mod actions;
#[cfg(target_os = "linux")]
mod clone_target;
mod cloning;
pub mod config;
pub mod model;
mod prompts;
mod store;

use devbox_core::{Db, Hub, Migration, current_rev, expand_home};
use devbox_protocol::{Done, Exporter, Fail, Internal, method, topic};
pub use model::*;
use std::path::PathBuf;
use std::sync::Arc;

pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "projects",
    version: 1,
    sql: store::MIGRATION_1,
}];

method!(pub ProjectsList = "projects.list", query, ListProjects => ProjectList, ProjectsError);
method!(pub ProjectsAdd = "projects.add", mutation, AddProject => Project, ProjectsError);
method!(pub ProjectsRemove = "projects.remove", mutation(always), RemoveProject => Done, ProjectsError);
method!(pub ProjectsScan = "projects.scan", query, ScanProjects => ScanResult, ProjectsError);
method!(pub ProjectsConfig = "projects.config", query, GetProjectConfig => ProjectConfigResult, ProjectsError);
method!(pub ProjectsSummary = "projects.summary", query, SummaryProjects => ProjectSummaryList, ProjectsError);
method!(pub ProjectsLaunch = "projects.launch", mutation, LaunchProject => LaunchResult, ProjectsError);
method!(pub ProjectsClone = "projects.clone", mutation, CloneProject => Project, ProjectsError);
method!(pub ProjectsSetFavorite = "projects.set_favorite", mutation, SetFavorite => Done, ProjectsError);
method!(pub ProjectsPrompts = "projects.prompts", query, ProjectPrompts => PromptList, ProjectsError);
topic!(pub ProjectsChangedTopic = "projects.changed", ProjectsChanged);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ProjectsList>();
    x.method::<ProjectsAdd>();
    x.method::<ProjectsRemove>();
    x.method::<ProjectsScan>();
    x.method::<ProjectsConfig>();
    x.method::<ProjectsSummary>();
    x.method::<ProjectsLaunch>();
    x.method::<ProjectsClone>();
    x.method::<ProjectsSetFavorite>();
    x.method::<ProjectsPrompts>();
    x.topic::<ProjectsChangedTopic>();
}

type R<T> = Result<T, Fail<ProjectsError>>;

pub struct ProjectsService {
    db: Db,
    hub: Arc<Hub>,
    home: PathBuf,
    roots: Vec<String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn canonical_project(path: String) -> R<Project> {
    let invalid = || ProjectsError::PathInvalid { path: path.clone() };
    let raw = PathBuf::from(&path);
    if !raw.is_absolute() {
        return Err(invalid().into());
    }
    let real = std::fs::canonicalize(raw).map_err(|_| invalid())?;
    if !real.is_dir() {
        return Err(ProjectsError::NotDirectory { path }.into());
    }
    let canonical = real.to_str().ok_or_else(invalid)?.to_owned();
    let name = real
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&canonical)
        .to_owned();
    Ok(Project {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name,
        path: canonical,
        favorite: false,
        added_ms: now_ms(),
    })
}

fn scan_candidates(
    roots: Vec<String>,
    home: PathBuf,
    registered: std::collections::HashSet<String>,
) -> ScanResult {
    let mut seen_roots = std::collections::HashSet::new();
    let mut seen_paths = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    for root in roots {
        let Ok(dir) = std::fs::canonicalize(expand_home(&root, &home)) else {
            continue;
        };
        if !seen_roots.insert(dir.clone()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let Some(name) = filename.to_str() else {
                continue;
            };
            if name.starts_with('.') || name.ends_with("-agents") {
                continue;
            }
            let Ok(real) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            if !real.is_dir() {
                continue;
            }
            let Some(path) = real.to_str() else { continue };
            let Some(name) = real.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.')
                || name.ends_with("-agents")
                || !seen_paths.insert(path.to_owned())
            {
                continue;
            }
            candidates.push(Candidate {
                path: path.to_owned(),
                name: name.to_owned(),
                is_git: real.join(".git").exists(),
                registered: registered.contains(path),
            });
        }
    }
    candidates.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    ScanResult { candidates }
}

fn publish_changed(hub: &Hub, rev: u64) {
    hub.publish::<ProjectsChangedTopic>(
        rev,
        &ProjectsChanged {
            rev: rev.to_string(),
        },
    );
}

impl ProjectsService {
    pub fn new(db: Db, hub: Arc<Hub>, home: PathBuf, roots: Vec<String>) -> Self {
        Self {
            db,
            hub,
            home,
            roots,
        }
    }

    pub async fn get(&self, id: &str) -> R<Option<Project>> {
        let id = id.to_owned();
        self.db
            .read(move |tx| store::get(tx, &id))
            .await
            .internal("get project")
    }

    pub async fn list(&self, _: ListProjects) -> R<ProjectList> {
        let (items, rev) = self
            .db
            .read(|tx| Ok((store::all(tx)?, current_rev(tx)?)))
            .await
            .internal("list projects")?;
        Ok(ProjectList {
            rev: rev.to_string(),
            items,
        })
    }

    pub async fn add(&self, p: AddProject) -> R<Project> {
        let project = tokio::task::spawn_blocking(move || canonical_project(p.path))
            .await
            .internal("validate project path")??;
        let row = project.clone();
        let hub = self.hub.clone();
        let _ = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::exists_path(tx, &row.path)? {
                        return Ok(Err(ProjectsError::AlreadyExists {
                            path: row.path.clone(),
                        }));
                    }
                    store::insert(tx, &row)?;
                    Ok(Ok(()))
                },
                move |_, rev| publish_changed(&hub, rev),
            )
            .await
            .internal("add project")??;
        Ok(project)
    }

    pub async fn remove(&self, p: RemoveProject) -> R<Done> {
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if store::get(tx, &p.id)?.is_none() {
                        return Ok(Err(ProjectsError::NotFound { id: p.id }));
                    }
                    if store::has_dependents(tx, &p.id)? {
                        return Ok(Err(ProjectsError::InUse { id: p.id }));
                    }
                    if store::delete(tx, &p.id)? == 0 {
                        return Ok(Err(ProjectsError::NotFound { id: p.id }));
                    }
                    Ok(Ok(()))
                },
                move |_, rev| publish_changed(&hub, rev),
            )
            .await
            .internal("remove project")??;
        Ok(Done {
            rev: rev.to_string(),
        })
    }

    pub async fn scan(&self, _: ScanProjects) -> R<ScanResult> {
        let registered: std::collections::HashSet<String> = self
            .db
            .read(store::all)
            .await
            .internal("scan projects")?
            .into_iter()
            .map(|p| p.path)
            .collect();
        let roots = self.roots.clone();
        let home = self.home.clone();
        tokio::task::spawn_blocking(move || scan_candidates(roots, home, registered))
            .await
            .internal("scan project directories")
    }
}

#[cfg(test)]
mod tests;
