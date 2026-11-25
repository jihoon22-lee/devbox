use super::state::Services;
use devbox_observe::{ObserveError, PortInfo, ProcessInfo, ProcessesQuery, ProjectRoot, UnitRun};
use devbox_protocol::{Fail, Internal};
use std::sync::Arc;
type R<T> = Result<T, Fail<ObserveError>>;
/// Registered project folders: the composition input of every project attribution.
async fn project_roots<E>(s: &Services) -> Result<Vec<ProjectRoot>, Fail<E>> {
    let projects = s
        .projects
        .list(devbox_projects::ListProjects {})
        .await
        .map_err(|e| Fail::Internal(format!("read observation projects: {e:?}")))?;
    Ok(projects
        .items
        .into_iter()
        .map(|p| ProjectRoot {
            id: p.id,
            path: p.path.into(),
        })
        .collect())
}
async fn context(s: &Services) -> R<(Vec<ProjectRoot>, Vec<UnitRun>)> {
    let projects = project_roots(s).await?;
    let runs = s
        .runs
        .live_unit_runs()
        .await
        .map_err(|e| Fail::Internal(format!("read observation runs: {e:?}")))?;
    Ok((
        projects,
        runs.into_iter()
            .map(|r| UnitRun {
                unit: r.unit,
                run_id: r.run_id,
                project_id: r.project_id,
            })
            .collect(),
    ))
}
pub(super) async fn containers(
    s: Arc<Services>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<devbox_runtime::ContainersResult, Fail<devbox_runtime::ContainerError>> {
    // Attribution only groups the list: without projects the containers are still listed.
    let projects = project_roots::<ObserveError>(&s).await.unwrap_or_else(|_| {
        tracing::warn!("containers listed without projects");
        vec![]
    });
    s.containers.list(&projects, cancel).await
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
#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, sync::Arc};

    /// A fake CLI that prints one Compose container whose working directory is `dir`.
    fn fake_docker(home: &std::path::Path, dir: &str) -> devbox_runtime::DockerContainers {
        let path = home.join("fake-docker");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{{\"ID\":\"abc\",\"Names\":\"app-web-1\",\"Image\":\"fake\",\"State\":\"running\",\"Status\":\"Up\",\"Ports\":\"\",\"Labels\":\"com.docker.compose.project.working_dir={dir}\"}}'\n"
            ),
        )
        .expect("owned fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("owned fixture");
        devbox_runtime::DockerContainers::new(path)
    }
    #[tokio::test]
    async fn containers_are_listed_without_attribution_when_projects_cannot_be_read() {
        let temp = tempfile::tempdir().expect("owned fixture");
        let folder = temp.path().join("app");
        std::fs::create_dir(&folder).expect("owned fixture");
        let mut services = crate::daemon::build_services_for_test(temp.path());
        let project = services
            .projects
            .add(devbox_projects::AddProject {
                path: folder.to_string_lossy().into_owned(),
            })
            .await
            .expect("owned fixture");
        Arc::get_mut(&mut services)
            .expect("owned fixture")
            .containers = fake_docker(temp.path(), &project.path);
        let list = || super::containers(services.clone(), Default::default());
        let listed = list().await.expect("listing with projects");
        assert_eq!(
            listed.items[0].project_id.as_deref(),
            Some(project.id.as_str())
        );
        // The projects read now fails; the Docker listing itself is unaffected.
        services
            .db
            .write(|tx| tx.execute("ALTER TABLE projects_projects RENAME TO gone", []))
            .await
            .expect("owned fixture");
        assert!(
            services
                .projects
                .list(devbox_projects::ListProjects {})
                .await
                .is_err()
        );
        let listed = list().await.expect("the listing survives the failed read");
        assert!(listed.available && listed.reason.is_none());
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].name, "app-web-1");
        assert_eq!(listed.items[0].project_id, None);
    }
}
