use super::state::Services;
use devbox_observe::{ObserveError, PortInfo, ProcessInfo, ProcessesQuery, ProjectRoot, UnitRun};
use devbox_protocol::{Fail, Internal};
use std::sync::Arc;
type R<T> = Result<T, Fail<ObserveError>>;
async fn context(s: &Services) -> R<(Vec<ProjectRoot>, Vec<UnitRun>)> {
    let projects = s
        .projects
        .list(devbox_projects::ListProjects {})
        .await
        .map_err(|e| Fail::Internal(format!("read observation projects: {e:?}")))?;
    let runs = s
        .runs
        .live_unit_runs()
        .await
        .map_err(|e| Fail::Internal(format!("read observation runs: {e:?}")))?;
    Ok((
        projects
            .items
            .into_iter()
            .map(|p| ProjectRoot {
                id: p.id,
                path: p.path.into(),
            })
            .collect(),
        runs.into_iter()
            .map(|r| UnitRun {
                unit: r.unit,
                run_id: r.run_id,
                project_id: r.project_id,
            })
            .collect(),
    ))
}
pub(super) async fn ports(s: Arc<Services>) -> R<Vec<PortInfo>> {
    let (projects, runs) = context(&s).await?;
    s.observe.ports(projects, runs).await
}
pub(super) async fn processes(s: Arc<Services>, p: ProcessesQuery) -> R<Vec<ProcessInfo>> {
    let (projects, runs) = context(&s).await?;
    s.observe.processes(p, projects, runs).await
}
/// Production composition is shared by the daemon and its owned-service integration tests.
pub fn readiness() -> devbox_runtime::Readiness {
    Arc::new(|check| {
        Box::pin(async move {
            if check.ports.is_empty() {
                return true;
            }
            tokio::task::spawn_blocking(move || {
                devbox_observe::run_ports_listening_in(
                    std::path::Path::new("/proc"),
                    &check.ports,
                    &check.unit,
                    check.pid,
                )
            })
            .await
            .internal::<ObserveError>("read readiness ports")
            .unwrap_or(false)
        })
    })
}
