use super::state::Services;
use devbox_observe::{LogSource, ObserveError, OpenLogs, ResolvedSource, SearchLogs, SearchResult};
use devbox_protocol::{Ctx, Fail, StreamOpened};
use std::{path::PathBuf, sync::Arc};
type R<T> = Result<T, Fail<ObserveError>>;
async fn resolve(
    s: &Arc<Services>,
    sources: &[LogSource],
) -> R<(Vec<ResolvedSource>, Vec<PathBuf>)> {
    devbox_observe::validate_sources(sources)?;
    let roots = roots(s).await?;
    let mut resolved = vec![];
    for source in sources {
        let path = match source {
            LogSource::File { path } => Some(PathBuf::from(path)),
            LogSource::Container { .. } => None,
            LogSource::Run { run_id } => {
                let meta = s
                    .runs
                    .run_log_metadata(run_id)
                    .await
                    .map_err(|_| Fail::Domain(ObserveError::RunNotFound))?;
                super::problems::register(s, &meta.project_id).await?;
                s.observe.register_problem_run(run_id, &meta.project_id)?;
                Some(s.runs.log_path(run_id).await.map_err(|e| match e {
                    Fail::Domain(devbox_runtime::RuntimeError::RunNotFound { .. }) => {
                        Fail::Domain(ObserveError::RunNotFound)
                    }
                    e => Fail::Internal(format!("resolve recorded run log: {e:?}")),
                })?)
            }
        };
        resolved.push(ResolvedSource {
            source: source.clone(),
            path,
        });
    }
    Ok((resolved, roots))
}
pub(super) async fn roots(s: &Arc<Services>) -> R<Vec<PathBuf>> {
    let projects = s
        .projects
        .list(devbox_projects::ListProjects {})
        .await
        .map_err(|e| Fail::Internal(format!("read log roots: {e:?}")))?;
    let mut roots: Vec<_> = projects
        .items
        .into_iter()
        .map(|p| PathBuf::from(p.path))
        .collect();
    // D15 default; configurable notes.vault belongs to S3 Task 4.
    roots.extend([s.paths.home.join("notes"), PathBuf::from("/var/log")]);
    Ok(roots)
}
pub(super) async fn open(s: Arc<Services>, ctx: Ctx, p: OpenLogs) -> R<StreamOpened> {
    let (sources, roots) = resolve(&s, &p.sources).await?;
    s.observe.open(ctx, p, sources, roots).await
}
pub(super) async fn search(s: Arc<Services>, ctx: Ctx, p: SearchLogs) -> R<SearchResult> {
    let (sources, roots) = resolve(&s, &p.sources).await?;
    s.observe.search(ctx, p, sources, roots).await
}
