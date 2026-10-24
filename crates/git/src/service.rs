use crate::{GitError, model::*, remote};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, OwnedMutexGuard, oneshot};
pub type ChangedSink = Arc<dyn Fn(GitChanged) + Send + Sync>;
pub type OperationSink = Arc<dyn Fn(GitOperation) + Send + Sync>;
pub enum RemoteAction {
    Fetch,
    Pull { expect: RemoteExpect, rebase: bool },
    Push { expect: RemoteExpect, force: bool },
}
struct OperationEntry {
    view: GitOperation,
    cancel: Option<oneshot::Sender<()>>,
}
pub struct GitService {
    locks: Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>>,
    operations: Mutex<BTreeMap<String, OperationEntry>>,
    watchers: Mutex<BTreeMap<String, (PathBuf, crate::watch::GitWatcher)>>,
    changed: ChangedSink,
    operation_changed: OperationSink,
}
impl GitService {
    pub fn new(changed: ChangedSink, operation_changed: OperationSink) -> Self {
        Self {
            locks: Mutex::new(BTreeMap::new()),
            operations: Mutex::new(BTreeMap::new()),
            watchers: Mutex::new(BTreeMap::new()),
            changed,
            operation_changed,
        }
    }
    pub async fn lock(&self, dir: &Path) -> Result<OwnedMutexGuard<()>, SourceGitError> {
        let common = crate::run::git(
            dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await
        .map_err(remote::failure)?;
        let key = tokio::fs::canonicalize(common.trim_end_matches('\n'))
            .await
            .map_err(|_| SourceGitError::InvalidTarget)?;
        let lock = self.locks.lock().await.entry(key).or_default().clone();
        lock.try_lock_owned()
            .map_err(|_| SourceGitError::SourceBusy)
    }
    pub fn changed(&self, target: &SourceTarget) {
        (self.changed)(GitChanged {
            project_id: target.project_id.clone(),
            worktree_id: target.worktree_id.clone(),
        });
    }
    pub async fn observe(&self, dir: &Path, target: &SourceTarget) -> Result<(), SourceGitError> {
        let key = format!(
            "{}:{}",
            target.project_id,
            target.worktree_id.as_deref().unwrap_or("")
        );
        let mut watches = self.watchers.lock().await;
        if watches.get(&key).is_some_and(|(path, _)| path == dir) {
            return Ok(());
        }
        if watches.len() >= 128 {
            return Err(SourceGitError::SourceLimitExceeded);
        }
        let sink = self.changed.clone();
        let target = target.clone();
        let watcher = crate::watch::watch(dir, move || {
            sink(GitChanged {
                project_id: target.project_id.clone(),
                worktree_id: target.worktree_id.clone(),
            })
        })
        .await
        .map_err(remote::failure)?;
        watches.insert(key, (dir.into(), watcher));
        Ok(())
    }
    pub async fn begin(
        self: &Arc<Self>,
        dir: PathBuf,
        target: SourceTarget,
        action: RemoteAction,
    ) -> Result<GitOperation, SourceGitError> {
        let guard = self.lock(&dir).await?;
        match &action {
            RemoteAction::Fetch => (),
            RemoteAction::Pull { expect, .. } | RemoteAction::Push { expect, .. } => {
                remote::preflight(&dir, expect).await?
            }
        }
        let push_identity = match &action {
            RemoteAction::Push { expect, .. } => remote::push_identity(&dir, expect).await.ok(),
            _ => None,
        };
        let id = uuid::Uuid::new_v4().simple().to_string();
        let view = GitOperation {
            id: id.clone(),
            target: target.clone(),
            state: GitOperationState::Running,
            error: None,
            current: Some(remote::state(&dir).await.map_err(remote::failure)?),
            remote_oid: None,
            completion_unknown: false,
        };
        let (cancel, mut cancelled) = oneshot::channel();
        {
            let mut operations = self.operations.lock().await;
            if operations.len() >= 128 {
                let old = operations
                    .iter()
                    .find(|(_, entry)| entry.view.state != GitOperationState::Running)
                    .map(|(id, _)| id.clone());
                if let Some(old) = old {
                    operations.remove(&old);
                } else {
                    return Err(SourceGitError::SourceLimitExceeded);
                }
            }
            operations.insert(
                id.clone(),
                OperationEntry {
                    view: view.clone(),
                    cancel: Some(cancel),
                },
            );
        }
        (self.operation_changed)(view.clone());
        let service = self.clone();
        tokio::spawn(async move {
            let _guard = guard;
            let push_expect = match &action {
                RemoteAction::Push { expect, .. } => Some(expect.clone()),
                _ => None,
            };
            let result = tokio::select! {
                biased;
                result = async {
                    match action {
                        RemoteAction::Fetch => remote::fetch(&dir).await,
                        RemoteAction::Pull { expect, rebase } => remote::pull(&dir, &expect, rebase).await,
                        RemoteAction::Push { expect, force } => remote::push(&dir, &expect, force).await,
                    }
                } => Some(result),
                _ = &mut cancelled => None,
            };
            // Owned command futures have been dropped/killed before re-reading actual state.
            let current = remote::state(&dir).await.ok();
            let reconciliation = if result.is_none() {
                if let Some(expect) = &push_expect {
                    Some(match &push_identity {
                        Some(identity) => remote::remote_oid(&dir, expect, identity).await,
                        None => Err(SourceGitError::UnsafeSource),
                    })
                } else {
                    None
                }
            } else {
                None
            };
            let view = {
                let mut operations = service.operations.lock().await;
                let entry = operations.get_mut(&id).expect("running operation retained");
                entry.cancel = None;
                entry.view.current = current;
                match result {
                    Some(Ok(())) => entry.view.state = GitOperationState::Succeeded,
                    Some(Err(error)) => {
                        entry.view.state = GitOperationState::Failed;
                        entry.view.error = Some(error);
                    }
                    None => {
                        entry.view.state = GitOperationState::Cancelled;
                        if let Some(reconciliation) = reconciliation {
                            match reconciliation {
                                Ok(oid) => {
                                    if oid.as_ref()
                                        == push_expect.as_ref().and_then(|e| e.head_oid.as_ref())
                                    {
                                        entry.view.state = GitOperationState::Succeeded;
                                    }
                                    entry.view.remote_oid = oid;
                                }
                                Err(_) => entry.view.completion_unknown = true,
                            }
                        } else {
                            // A readable snapshot does not prove whether fetch/pull
                            // already applied changes before its process was cancelled.
                            entry.view.completion_unknown = true;
                        }
                    }
                };
                entry.view.clone()
            };
            service.changed(&target);
            (service.operation_changed)(view);
        });
        Ok(view)
    }
    pub async fn operation(&self, p: &GitOperationQuery) -> Result<GitOperation, SourceGitError> {
        self.operations
            .lock()
            .await
            .get(&p.operation_id)
            .filter(|entry| entry.view.target == p.target)
            .map(|entry| entry.view.clone())
            .ok_or(SourceGitError::InvalidTarget)
    }
    pub async fn cancel(&self, p: &GitOperationQuery) -> Result<GitOperation, SourceGitError> {
        {
            let mut operations = self.operations.lock().await;
            let entry = operations
                .get_mut(&p.operation_id)
                .filter(|e| e.view.target == p.target)
                .ok_or(SourceGitError::InvalidTarget)?;
            if let Some(cancel) = entry.cancel.take() {
                let _ = cancel.send(());
            }
        }
        loop {
            let view = self.operation(p).await?;
            if view.state != GitOperationState::Running {
                return Ok(view);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    pub async fn shutdown(&self) {
        self.watchers.lock().await.clear();
        let queries = {
            let mut operations = self.operations.lock().await;
            operations
                .values_mut()
                .filter(|entry| entry.view.state == GitOperationState::Running)
                .map(|entry| {
                    if let Some(cancel) = entry.cancel.take() {
                        let _ = cancel.send(());
                    }
                    GitOperationQuery {
                        target: entry.view.target.clone(),
                        operation_id: entry.view.id.clone(),
                    }
                })
                .collect::<Vec<_>>()
        };
        for query in queries {
            let _ = self.cancel(&query).await;
        }
    }
}
pub async fn failure(dir: &Path, error: GitError) -> SourceGitError {
    match error {
        GitError::StateChanged => match remote::state(dir).await {
            Ok(current) => SourceGitError::StateChanged { current },
            Err(_) => SourceGitError::SourceFailed,
        },
        error => remote::failure(error),
    }
}
