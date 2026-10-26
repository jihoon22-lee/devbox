use crate::*;
use devbox_core::{Db, Hub, current_rev};
use devbox_projects::{
    ProjectsError, ProjectsService,
    run_config::{RunConfigChange, RunConfigEdit},
};
use devbox_protocol::{Done, Fail, Internal};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

pub(crate) const PAGE_BYTES: usize = 768 * 1024;
const PAGE_ROWS: usize = 256;

pub struct RuntimeService {
    pub(crate) db: Db,
    pub(crate) hub: Arc<Hub>,
    pub(crate) projects: Arc<ProjectsService>,
    pub(crate) execution: Option<crate::ExecutionConfig>,
    pub(crate) secrets: Option<Arc<devbox_secrets::SecretsService>>,
    pub(crate) run_locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    pub(crate) observation: AsyncMutex<HashMap<String, String>>,
    pub(crate) run_watcher: AsyncMutex<Option<crate::watch::WatchHandle>>,
    pub(crate) scheduler: AsyncMutex<Option<crate::watch::WatchHandle>>,
    pub(crate) watcher: AsyncMutex<Option<crate::watch::WatchHandle>>,
    pub(crate) copy_tasks: tokio_util::task::TaskTracker,
    pub(crate) stopping: tokio_util::sync::CancellationToken,
}

impl RuntimeService {
    /// Construction stays synchronous; the daemon explicitly starts and stops watching.
    pub fn new(db: Db, hub: Arc<Hub>, projects: Arc<ProjectsService>) -> Self {
        Self {
            db,
            hub,
            projects,
            execution: None,
            secrets: None,
            run_locks: Mutex::default(),
            locks: Mutex::default(),
            observation: AsyncMutex::default(),
            watcher: AsyncMutex::default(),
            scheduler: AsyncMutex::default(),
            run_watcher: AsyncMutex::default(),
            copy_tasks: tokio_util::task::TaskTracker::new(),
            stopping: tokio_util::sync::CancellationToken::new(),
        }
    }

    pub(crate) async fn project_lock(&self, id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.locks.lock().expect("runtime project locks");
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(id).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(id.into(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }

    pub(crate) fn changed(&self, project_id: &str, rev: u64) {
        self.hub.publish::<RuntimeChanged>(
            rev,
            &RuntimeChange {
                rev: rev.to_string(),
                project_id: project_id.into(),
            },
        );
    }

    pub(crate) async fn local(&self, id: &str) -> R<(Vec<RunDef>, u64)> {
        let id = id.to_owned();
        self.db
            .read(move |tx| Ok((store::definitions(tx, &id)?, current_rev(tx)?)))
            .await
            .internal("read local execution definitions")
    }

    pub async fn list(&self, p: ListRuns) -> R<DefinitionList> {
        if p.cursor
            .as_ref()
            .is_some_and(|c| c.id.is_empty() || c.id.len() > 128 || c.id.contains('\0'))
        {
            return Err(RuntimeError::InvalidCursor.into());
        }
        let _guard = self.project_lock(&p.project_id).await;
        let state = self
            .projects
            .run_config_status(&p.project_id)
            .await
            .map_err(project_error)?;
        let (local, rev) = self.local(&p.project_id).await?;
        let merged = merge(state.config.definitions(), local)?;
        let mut rows: Vec<_> = merged
            .defs
            .into_iter()
            .map(|d| (false, d))
            .chain(merged.shadowed.into_iter().map(|d| (true, d)))
            .collect();
        rows.sort_by(|(sa, a), (sb, b)| (a.def.id(), sa).cmp(&(b.def.id(), sb)));
        let mut rows = rows
            .into_iter()
            .filter(|(shadowed, def)| {
                p.cursor.as_ref().is_none_or(|cursor| {
                    (def.def.id(), *shadowed) > (cursor.id.as_str(), cursor.shadowed)
                })
            })
            .peekable();
        // Each bounded definition page carries its current live invocation (or
        // most recent result). A long-lived service must not fall out of a global
        // recent-history window when unrelated tasks run frequently.
        let candidates: Vec<_> = rows.by_ref().take(PAGE_ROWS + 1).collect();
        let ids: Vec<_> = candidates
            .iter()
            .map(|(_, d)| d.def.id().to_owned())
            .collect();
        let project = p.project_id.clone();
        let (representatives, run_rev) = self
            .db
            .read(move |tx| {
                let mut runs = HashMap::new();
                for id in ids {
                    if let Some(run) = crate::run_store::representative(tx, &project, &id)? {
                        runs.insert(id, run.run);
                    }
                }
                Ok((runs, current_rev(tx)?))
            })
            .await
            .internal("read definition invocation snapshot")?;
        let mut rows = candidates.into_iter().peekable();
        let mut page = DefinitionList {
            rev: run_rev.max(rev).to_string(),
            defs: vec![],
            shadowed: vec![],
            runs: vec![],
            trust: state.trust,
            next_cursor: None,
        };
        let mut accepted = 0;
        let mut previous_cursor = None;
        while let Some((shadowed, def)) = rows.next() {
            let cursor = DefinitionCursor {
                id: def.def.id().into(),
                shadowed,
            };
            let run = (!shadowed)
                .then(|| representatives.get(def.def.id()))
                .flatten()
                .cloned();
            if let Some(run) = &run {
                page.runs.push(run.clone());
            }
            if shadowed {
                page.shadowed.push(def);
            } else {
                page.defs.push(def);
            }
            page.next_cursor = Some(cursor.clone());
            let bytes = serde_json::to_vec(&page)
                .internal("measure execution list")?
                .len();
            if bytes > PAGE_BYTES {
                if run.is_some() {
                    page.runs.pop();
                }
                if shadowed {
                    page.shadowed.pop();
                } else {
                    page.defs.pop();
                }
                if accepted == 0 {
                    return Err(RuntimeError::DefInvalid {
                        id: cursor.id,
                        reason: "definition exceeds list response budget".into(),
                    }
                    .into());
                }
                page.next_cursor = previous_cursor;
                break;
            }
            accepted += 1;
            previous_cursor = Some(cursor);
            if rows.peek().is_none() {
                page.next_cursor = None;
                break;
            }
            if accepted == PAGE_ROWS {
                break;
            }
        }
        Ok(page)
    }

    pub async fn define(&self, p: DefineRun) -> R<DefineResult> {
        validate_definition(&p.def)?;
        let _guard = self.project_lock(&p.project_id).await;
        let state = self
            .projects
            .run_config_status(&p.project_id)
            .await
            .map_err(project_error)?;
        let mut file = state.config.definitions();
        let (mut local, _) = self.local(&p.project_id).await?;
        let target = if p.save_to == SaveTo::Project {
            &mut file
        } else {
            &mut local
        };
        target.retain(|d| d.id() != p.def.id());
        target.push(p.def.clone());
        merge(file, local)?;
        let rev = match p.save_to {
            SaveTo::Project => {
                let updated = self
                    .projects
                    .update_run_config(
                        &p.project_id,
                        RunConfigEdit {
                            expected_digest: state.digest,
                            change: RunConfigChange::Define(p.def.clone()),
                        },
                    )
                    .await
                    .map_err(project_error)?;
                // The helper owns file publication and its approval/revision transaction.
                self.observation.lock().await.insert(
                    p.project_id.clone(),
                    crate::watch::fingerprint(&updated.digest, &updated.trust),
                );
                let rev = updated
                    .rev
                    .parse::<u64>()
                    .internal("parse project write revision")?;
                self.changed(&p.project_id, rev);
                rev
            }
            SaveTo::ThisPc => {
                let project = p.project_id.clone();
                let def = p.def.clone();
                let hub = self.hub.clone();
                let event_project = project.clone();
                let (_, rev) = self
                    .db
                    .write_checked_with_commit(
                        move |tx| {
                            store::define(tx, &project, &def)?;
                            Ok(Ok::<_, RuntimeError>(()))
                        },
                        move |_, rev| publish(&hub, &event_project, rev),
                    )
                    .await
                    .internal("save local execution definition")??;
                rev
            }
        };
        Ok(DefineResult {
            rev: rev.to_string(),
            definition: Definition {
                source: if p.save_to == SaveTo::Project {
                    DefinitionSource::ProjectFile
                } else {
                    DefinitionSource::ThisPc
                },
                def: p.def,
            },
        })
    }

    pub async fn undefine(&self, p: UndefineRun) -> R<Done> {
        let _guard = self.project_lock(&p.project_id).await;
        let state = self
            .projects
            .run_config_status(&p.project_id)
            .await
            .map_err(project_error)?;
        let removed_id = p.id.clone();
        let mut file = state.config.definitions();
        let (mut local, rev) = self.local(&p.project_id).await?;
        let in_file = file.iter().any(|d| d.id() == p.id);
        let in_local = local.iter().any(|d| d.id() == p.id);
        if !in_file && !in_local {
            return Ok(Done {
                rev: rev.to_string(),
            });
        }
        if in_file {
            file.retain(|d| d.id() != p.id);
        } else {
            local.retain(|d| d.id() != p.id);
        }
        merge(file, local)?;
        let rev = if in_file {
            let updated = self
                .projects
                .update_run_config(
                    &p.project_id,
                    RunConfigEdit {
                        expected_digest: state.digest,
                        change: RunConfigChange::Undefine { id: p.id },
                    },
                )
                .await
                .map_err(project_error)?;
            self.observation.lock().await.insert(
                p.project_id.clone(),
                crate::watch::fingerprint(&updated.digest, &updated.trust),
            );
            let rev = updated
                .rev
                .parse::<u64>()
                .internal("parse project write revision")?;
            self.changed(&p.project_id, rev);
            rev
        } else {
            let project = p.project_id.clone();
            let hub = self.hub.clone();
            let event_project = project.clone();
            let (_, rev) = self
                .db
                .write_checked_with_commit(
                    move |tx| {
                        store::undefine(tx, &project, &p.id)?;
                        Ok(Ok::<_, RuntimeError>(()))
                    },
                    move |_, rev| publish(&hub, &event_project, rev),
                )
                .await
                .internal("delete local execution definition")??;
            rev
        };
        self.remove_schedule_state(&p.project_id, &removed_id)
            .await?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
}

fn publish(hub: &Hub, project_id: &str, rev: u64) {
    hub.publish::<RuntimeChanged>(
        rev,
        &RuntimeChange {
            rev: rev.to_string(),
            project_id: project_id.into(),
        },
    );
}

pub(crate) fn project_error(error: Fail<ProjectsError>) -> Fail<RuntimeError> {
    match error {
        Fail::Domain(ProjectsError::TrustRequired { id }) => {
            RuntimeError::ProjectTrustRequired { id }.into()
        }
        Fail::Domain(ProjectsError::TrustPreviewStale { id }) => {
            RuntimeError::ProjectConfigChanged { id }.into()
        }
        Fail::Domain(ProjectsError::NotFound { id }) => RuntimeError::ProjectNotFound { id }.into(),
        Fail::Domain(ProjectsError::ConfigurationInvalid { message }) => {
            RuntimeError::ProjectConfigInvalid { message }.into()
        }
        Fail::Domain(error) => Fail::Internal(format!("project execution configuration: {error}")),
        Fail::Internal(error) => Fail::Internal(error),
    }
}
