use super::state::Services;
use devbox_observe::{ObserveError, PortInfo, ProcessInfo, ProcessesQuery, ProjectRoot, UnitRun};
use devbox_protocol::{Fail, Internal};
use std::sync::Arc;
type R<T> = Result<T, Fail<ObserveError>>;
/// Registered project folders: the composition input of every project attribution.
async fn read_roots(
    s: &Services,
) -> Result<Vec<ProjectRoot>, Fail<devbox_projects::ProjectsError>> {
    let projects = s.projects.list(devbox_projects::ListProjects {}).await?;
    Ok(projects
        .items
        .into_iter()
        .map(|p| ProjectRoot {
            id: p.id,
            path: p.path.into(),
        })
        .collect())
}
async fn project_roots(s: &Services) -> R<Vec<ProjectRoot>> {
    read_roots(s)
        .await
        .map_err(|e| Fail::Internal(format!("read observation projects: {e:?}")))
}
/// Roots for a listing that projects only group: a failed read costs the grouping, not the
/// listing, and is logged by its cause.
async fn roots_or_none(s: &Services, message: &'static str) -> Vec<ProjectRoot> {
    read_roots(s).await.unwrap_or_else(|error| {
        tracing::warn!(cause = %cause(&error), "{message}");
        vec![]
    })
}
/// What a degraded listing logs about a failed projects read: the error's code. The text of
/// an internal error is free-form and may hold a path, so it is named only by its kind.
fn cause(error: &Fail<devbox_projects::ProjectsError>) -> String {
    match error {
        Fail::Domain(error) => error.to_string(),
        Fail::Internal(_) => "internal".into(),
    }
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
    let projects = roots_or_none(&s, "containers listed without projects").await;
    s.containers.list(&projects, cancel).await
}
/// The user's own systemd user units. Like containers, they are listed without attribution
/// when the projects cannot be read.
pub(super) async fn units(s: Arc<Services>) -> R<devbox_observe::UnitsResult> {
    let projects = roots_or_none(&s, "user units listed without projects").await;
    let unit_dir = s.paths.xdg_config_home.join("systemd/user");
    s.observe.units(projects, unit_dir).await
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
    use std::{os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

    #[test]
    fn a_failed_projects_read_is_logged_by_its_code_without_its_text() {
        use devbox_projects::ProjectsError;
        use devbox_protocol::Fail;
        assert_eq!(
            super::cause(&Fail::Domain(ProjectsError::PathInvalid {
                path: "/home/u/projects/app".into()
            })),
            "projects.path_invalid"
        );
        assert_eq!(
            super::cause(&Fail::Internal(
                "list projects: /home/u/projects/app".into()
            )),
            "internal"
        );
    }

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
    /// A fake `systemctl` with one user unit file whose working directory is `dir`, next to a
    /// packaged unit and Devbox's own. The real one is never run by a test.
    fn fake_systemctl(home: &std::path::Path, unit_dir: &std::path::Path, dir: &str) -> PathBuf {
        let path = home.join("fake-systemctl");
        let unit_dir = unit_dir.display();
        std::fs::write(
            &path,
            format!(
                r#"#!/bin/sh
case "$*" in
  '--user list-units --type=service,timer --all --no-pager --output=json')
    printf '%s' '[{{"unit":"app-web.service"}},{{"unit":"dbus.service"}},{{"unit":"devbox@test-r.service"}}]' ;;
  '--user show --no-pager --property=Id,Description,LoadState,ActiveState,SubState,FragmentPath,WorkingDirectory,NextElapseUSecRealtime,NextElapseUSecMonotonic,Triggers -- app-web.service dbus.service')
    printf '%s\n' 'Id=app-web.service' 'Description=App web server' 'LoadState=loaded' 'ActiveState=active' 'SubState=running' 'FragmentPath={unit_dir}/app-web.service' 'WorkingDirectory={dir}/web' '' 'Id=dbus.service' 'LoadState=loaded' 'FragmentPath=/usr/lib/systemd/user/dbus.service' ;;
  *) exit 9 ;;
esac
"#
            ),
        )
        .expect("owned fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("owned fixture");
        path
    }
    #[tokio::test]
    async fn user_units_are_listed_read_only_with_the_project_of_their_working_directory() {
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
        let unit_dir = services.paths.xdg_config_home.join("systemd/user");
        let systemctl = fake_systemctl(temp.path(), &unit_dir, &project.path);
        Arc::get_mut(&mut services).expect("owned fixture").observe =
            Arc::new(devbox_observe::ObserveService::default().with_systemctl(systemctl));
        let listed = super::units(services.clone()).await.expect("listing");
        assert!(listed.available && listed.reason.is_none() && !listed.truncated);
        assert_eq!(listed.items.len(), 1, "only the user's own unit file");
        assert_eq!(listed.items[0].name, "app-web.service");
        assert_eq!(
            listed.items[0].project_id.as_deref(),
            Some(project.id.as_str())
        );
        // Without a readable projects table the units are still listed.
        services
            .db
            .write(|tx| tx.execute("ALTER TABLE projects_projects RENAME TO gone", []))
            .await
            .expect("owned fixture");
        let listed = super::units(services.clone()).await.expect("listing");
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].project_id, None);
        // No systemctl: a typed reason, not an error.
        Arc::get_mut(&mut services).expect("owned fixture").observe = Arc::new(
            devbox_observe::ObserveService::default().with_systemctl(temp.path().join("missing")),
        );
        let listed = super::units(services).await.expect("typed");
        assert!(!listed.available && listed.items.is_empty());
        assert_eq!(
            listed.reason,
            Some(devbox_observe::UnitsUnavailableReason::CliMissing)
        );
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
