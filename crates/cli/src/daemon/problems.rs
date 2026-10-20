use super::state::Services;
use devbox_observe::{ObserveError, ProblemRun, ProblemsQuery, ProblemsResult};
use devbox_protocol::{Fail, Internal};
use notify::{RecursiveMode, Watcher};
use std::{path::PathBuf, sync::Arc};
type R<T> = Result<T, Fail<ObserveError>>;

pub(super) async fn query(s: Arc<Services>, p: ProblemsQuery) -> R<ProblemsResult> {
    if p.limit.is_some_and(|limit| limit == 0 || limit > 100) {
        return Err(ObserveError::InvalidInput.into());
    }
    register(&s, &p.project_id).await?;
    refresh(&s).await?;
    s.observe.problem_result(p)
}
pub(super) async fn register(s: &Arc<Services>, project: &str) -> R<()> {
    let projects = s
        .projects
        .list(devbox_projects::ListProjects {})
        .await
        .map_err(|e| Fail::Internal(format!("read problem projects: {e:?}")))?;
    if !projects.items.iter().any(|p| p.id == project) {
        return Err(ObserveError::ProjectNotFound.into());
    }
    s.observe.register_problem_project(project)?;
    let weak = Arc::downgrade(s);
    let observe = s.observe.clone();
    let hub = s.hub.clone();
    let root = s.paths.data_dir.clone();
    let stop = observe.cancellation();
    let (ready, started) = tokio::sync::oneshot::channel();
    let launched = s.observe.start_problem_watch(async move {
        let (send, mut events) = tokio::sync::mpsc::channel(1);
        let logs = root.join("runs");
        let watched_logs = logs.clone();
        let mut watcher =
            match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.as_ref().is_ok_and(|e| {
                    !e.kind.is_access() && e.paths.iter().any(|p| p.starts_with(&logs))
                }) {
                    let _ = send.try_send(());
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::debug!(?e, "problem watcher unavailable");
                    let _ = ready.send(Err(ObserveError::SourceUnavailable));
                    return;
                }
            };
        if let Err(e) = watcher.watch(&root, RecursiveMode::NonRecursive) {
            tracing::debug!(?e, "problem log watch unavailable");
            let _ = ready.send(Err(ObserveError::SourceUnavailable));
            return;
        }
        let mut watching_runs = false;
        if watched_logs.is_dir() {
            if let Err(e) = watcher.watch(&watched_logs, RecursiveMode::NonRecursive) {
                tracing::debug!(?e, "problem run directory watch unavailable");
                let _ = ready.send(Err(ObserveError::SourceUnavailable));
                return;
            }
            watching_runs = true;
        }
        let mut runtime = hub.subscribe("runtime.changed");
        let _ = ready.send(Ok(()));
        loop {
            tokio::select! {
                _=stop.cancelled()=>break,
                _=events.recv()=>{},
                _=runtime.recv()=>{},
                _=observe.problem_wake()=>{},
            }
            if !watched_logs.is_dir() {
                watching_runs = false;
            }
            if !watching_runs && watched_logs.is_dir() {
                if let Err(e) = watcher.watch(&watched_logs, RecursiveMode::NonRecursive) {
                    tracing::debug!(?e, "watch new run directory");
                    continue;
                }
                watching_runs = true;
            }
            let Some(s) = weak.upgrade() else {
                break;
            };
            let outcome = tokio::select! {_=stop.cancelled()=>break,result=refresh(&s)=>result};
            if let Err(e) = outcome {
                tracing::debug!(?e, "problem log refresh failed");
            }
        }
    });
    if launched {
        started.await.internal("start problem log watcher")??;
    }
    Ok(())
}
async fn refresh(s: &Services) -> R<()> {
    let _guard = s.observe.problem_refresh_lock().await;
    let projects = s
        .projects
        .list(devbox_projects::ListProjects {})
        .await
        .map_err(|e| Fail::Internal(format!("read problem roots: {e:?}")))?;
    for id in s.observe.problem_projects() {
        let Some(project) = projects.items.iter().find(|p| p.id == id) else {
            continue;
        };
        let mut page = s
            .runs
            .problem_runs(&id)
            .await
            .map_err(|e| Fail::Internal(format!("read problem history: {e:?}")))?;
        if page.truncated {
            s.observe.mark_problem_history_truncated(&id, &s.hub);
        }
        for run_id in s.observe.explicit_problem_runs(&id) {
            if !page.items.iter().any(|r| r.run_id == run_id) {
                page.items.push(
                    s.runs.run_log_metadata(&run_id).await.map_err(|e| {
                        Fail::Internal(format!("read explicitly observed run: {e:?}"))
                    })?,
                );
            }
        }
        for initial in page.items {
            let mut meta = initial;
            let mut stable = false;
            for _ in 0..3 {
                let run = ProblemRun {
                    run_id: meta.run_id.clone(),
                    project_id: meta.project_id.clone(),
                    root: PathBuf::from(&project.path),
                    source_cwd: meta.source_cwd.clone(),
                    windows_directory: meta.windows_directory.clone(),
                    path: meta.path.clone(),
                    attempt_id: meta.attempt_id.clone(),
                    started_ms: meta.started_ms,
                };
                let batch = s.observe.read_problem_batch(&run).await?;
                let current =
                    s.runs.run_log_metadata(&meta.run_id).await.map_err(|e| {
                        Fail::Internal(format!("validate problem invocation: {e:?}"))
                    })?;
                if current.attempt_id != meta.attempt_id || current.started_ms != meta.started_ms {
                    s.observe.discard_problem_reader(&meta.run_id).await;
                    meta = current;
                    continue;
                }
                s.observe
                    .commit_problem_batch(run, batch, current.live, &s.hub)
                    .await?;
                stable = true;
                break;
            }
            if !stable {
                return Err(ObserveError::StateChanged.into());
            }
        }
    }
    Ok(())
}
