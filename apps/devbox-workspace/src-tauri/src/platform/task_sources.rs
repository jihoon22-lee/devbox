//! Linux source bytes remain behind Registry/distro/helper admission.
use crate::host::Host;
use run_manager_lib::{
    core::{
        models::TargetKind,
        workspace_tasks::{WorkspaceTaskError as Error, WorkspaceTaskExecution, WorkspaceTaskPlan},
    },
    workspace_sources::NativeTaskSources,
};
use std::{path::Path, sync::Arc};
pub(crate) struct Sources {
    pub host: Arc<Host>,
}
impl NativeTaskSources for Sources {
    fn preview(
        &self,
        root: &Path,
        target: TargetKind,
        distro: Option<&str>,
    ) -> Result<Option<WorkspaceTaskPlan>, Error> {
        let root = root.to_str().ok_or(Error::InvalidRoot)?;
        if target != TargetKind::Wsl || !root.starts_with('/') {
            return Ok(None);
        }
        #[cfg(windows)]
        {
            self.capture(root, distro.ok_or(Error::InvalidTarget)?)
                .map(|snapshot| Some(snapshot.plan))
        }
        #[cfg(not(windows))]
        {
            let _ = (&self.host, distro);
            Err(Error::SourceUnavailable)
        }
    }
    fn wsl_target(
        &self,
        distro: &str,
        execution: Option<&WorkspaceTaskExecution>,
    ) -> Result<Option<run_manager_lib::platform::wsl::Target>, Error> {
        if execution.is_none_or(|task| !task.source_root.starts_with('/')) {
            return Ok(None);
        }
        #[cfg(windows)]
        {
            let guard = if let Some(task) =
                execution.filter(|task| task.source_root.starts_with('/'))
            {
                let snapshot = self.capture(&task.source_root, distro)?;
                run_manager_lib::core::workspace_tasks::verify_projected_executions(
                    &snapshot.plan,
                    std::slice::from_ref(task),
                )?;
                let mut launch = snapshot.launch;
                launch.cwd = task.cwd.clone();
                workspace_wsl::task_contract::validate(&launch).map_err(|_| Error::InvalidRoot)?;
                Some(launch)
            } else {
                None
            };
            let deadline = now().saturating_add(30_000);
            let lease = super::terminal_launch::capture_runtime(&self.host, distro, deadline)
                .map_err(|_| Error::SourceChanged)?;
            let artifact = if guard.is_some() {
                Some(
                    super::wsl_helper::Artifact::open(
                        self.host
                            .helper_directory()
                            .map_err(|_| Error::SourceUnavailable)?,
                    )
                    .map_err(|_| Error::SourceUnavailable)?,
                )
            } else {
                None
            };
            let binding = Bound {
                lease,
                guard,
                directory: self
                    .host
                    .helper_directory()
                    .map_err(|_| Error::SourceUnavailable)?
                    .into(),
                _artifact: artifact,
            };
            Ok(Some(run_manager_lib::platform::wsl::Target::bound(
                distro,
                Arc::new(binding),
            )))
        }
        #[cfg(not(windows))]
        {
            let _ = (&self.host, distro, execution);
            Err(Error::SourceUnavailable)
        }
    }
}
#[cfg(windows)]
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .unwrap_or(0)
}
#[cfg(windows)]
struct Snapshot {
    plan: WorkspaceTaskPlan,
    launch: workspace_wsl::task_contract::TaskLaunch,
}
#[cfg(windows)]
impl Sources {
    fn capture(&self, root: &str, distro: &str) -> Result<Snapshot, Error> {
        // Some synchronous callers are inside Handle::block_on. The private helper
        // owns its own runtime, so create and retire it outside that entered context.
        std::thread::scope(|scope| scope.spawn(|| self.capture_native(root, distro)).join())
            .map_err(|_| Error::SourceUnavailable)?
    }
    fn capture_native(&self, root: &str, distro: &str) -> Result<Snapshot, Error> {
        let projects = self.host.projects().map_err(|_| Error::SourceUnavailable)?;
        let registry = projects.snapshot().map_err(|_| Error::SourceUnavailable)?;
        let distros = super::wsl_distro::list().map_err(|_| Error::SourceUnavailable)?;
        let distro_id = &distros
            .iter()
            .find(|entry| entry.name == distro)
            .ok_or(Error::InvalidTarget)?
            .id;
        let mut matching=registry.worktrees.iter().filter(|tree|tree.binding.root==root
            &&matches!(&tree.binding.target,product_contract::ExecutionTarget::Wsl{distro_id:id} if id==distro_id));
        let tree = matching.next().ok_or(Error::InvalidRoot)?;
        if matching.next().is_some() {
            return Err(Error::InvalidRoot);
        }
        let context = tree.context();
        let deadline = now().saturating_add(30_000);
        let mut files =
            super::definition_files::DefinitionFiles::open(&self.host, &context, deadline)
                .map_err(|_| Error::SourceUnavailable)?;
        let bytes = files
            .read(".vscode/tasks.json", false, deadline)
            .map_err(|_| Error::SourceUnavailable)?
            .ok_or(Error::SourceUnavailable)?;
        if bytes.len() > 512 * 1024 {
            return Err(Error::SourceTooLarge);
        }
        let source_digest = crate::definitions::digest(&bytes);
        let identity = crate::definitions::digest(
            &serde_json::to_vec(&(context, files.binding())).map_err(|_| Error::InvalidRoot)?,
        );
        let revision = crate::definitions::digest(
            &serde_json::to_vec(&(&identity, &source_digest)).map_err(|_| Error::InvalidRoot)?,
        );
        let plan = run_manager_lib::core::workspace_tasks::project_workspace_tasks(
            run_manager_lib::core::workspace_tasks::TaskProjection {
                source_root: root,
                target_root: root,
                project_identity: &identity,
                revision: &revision,
                target_kind: TargetKind::Wsl,
                target_distro: Some(distro),
                bytes: &bytes,
            },
        )?;
        files
            .revalidate(deadline)
            .map_err(|_| Error::SourceChanged)?;
        let launch = files
            .native_task_launch(root, source_digest)
            .map_err(|_| Error::SourceChanged)?;
        Ok(Snapshot { plan, launch })
    }
}
#[cfg(windows)]
struct Bound {
    lease: Arc<dyn run_manager_lib::platform::wsl::CommandBinding>,
    guard: Option<workspace_wsl::task_contract::TaskLaunch>,
    directory: std::path::PathBuf,
    _artifact: Option<super::wsl_helper::Artifact>,
}
#[cfg(windows)]
impl run_manager_lib::platform::wsl::CommandBinding for Bound {
    fn bind(
        &self,
        argv: Vec<String>,
    ) -> Result<Vec<String>, run_manager_lib::platform::wsl::WslExecutionError> {
        self.lease.bind(argv)
    }
    fn bind_launch(
        &self,
        argv: Vec<String>,
    ) -> Result<Vec<String>, run_manager_lib::platform::wsl::WslExecutionError> {
        let Some(guard) = &self.guard else {
            return self.lease.bind_launch(argv);
        };
        let boundary = argv
            .iter()
            .position(|arg| arg == "--exec")
            .ok_or_else(|| std::io::Error::other("runtime-target-invalid"))?;
        if boundary < 3 {
            return Err(std::io::Error::other("runtime-target-invalid").into());
        }
        let mut next = argv[..3].to_vec();
        next.extend([
            "--cd".into(),
            self.directory
                .to_str()
                .ok_or_else(|| std::io::Error::other("runtime-helper-unavailable"))?
                .into(),
            "--exec".into(),
            "./devbox-workspace-wsl".into(),
            "--task-exec".into(),
            serde_json::to_string(guard)
                .map_err(|_| std::io::Error::other("runtime-target-invalid"))?,
            "--".into(),
        ]);
        next.extend(argv[boundary + 1..].iter().cloned());
        self.lease.bind_launch(next)
    }
}

pub(crate) fn context_identity(
    host: &Host,
    context: &product_contract::ProjectContext,
) -> Result<String, &'static str> {
    let binding = host.projects()?.binding(context)?;
    Ok(crate::definitions::digest(
        &serde_json::to_vec(&(context, binding)).map_err(|_| "invalid_context")?,
    ))
}
pub(crate) fn matches_context(
    host: &Host,
    context: &product_contract::ProjectContext,
    root: &str,
    identity: &str,
) -> bool {
    host.projects()
        .and_then(|projects| projects.binding(context))
        .is_ok_and(|binding| {
            binding.root == root
                && (!matches!(
                    context.target,
                    product_contract::ExecutionTarget::Wsl { .. }
                ) || context_identity(host, context).is_ok_and(|expected| expected == identity))
        })
}
pub(crate) fn diagnostic_matches(
    app: &tauri::AppHandle,
    host: &Host,
    context: &product_contract::ProjectContext,
    run: &str,
) -> bool {
    run_manager_lib::component::diagnostic_scope(app, run)
        .is_ok_and(|(root, identity)| matches_context(host, context, &root, &identity))
}
