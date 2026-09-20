//! Per-owner native source adapter. SQLite locks are never held during source IO.
use crate::{
    core::{
        models::TargetKind,
        workspace_tasks::{
            self as tasks, WorkspaceTaskError as Error, WorkspaceTaskExecution, WorkspaceTaskPlan,
        },
    },
    storage::DatabaseState,
};
use std::{path::Path, sync::Arc};

pub trait NativeTaskSources: Send + Sync {
    fn wsl_target(
        &self,
        _distro: &str,
        _execution: Option<&WorkspaceTaskExecution>,
    ) -> Result<Option<crate::platform::wsl::Target>, Error> {
        Ok(None)
    }

    /// Some owns the source; None preserves the existing local source reader.
    fn preview(
        &self,
        root: &Path,
        target: TargetKind,
        distro: Option<&str>,
    ) -> Result<Option<WorkspaceTaskPlan>, Error>;
}
pub fn preview(
    database: &DatabaseState,
    root: &Path,
    target: TargetKind,
    distro: Option<&str>,
) -> Result<WorkspaceTaskPlan, Error> {
    if let Some(provider) = database.task_sources.get() {
        if let Some(plan) = provider.preview(root, target, distro)? {
            return Ok(plan);
        }
    }
    tasks::preview_workspace_tasks(root, target, distro)
}
#[allow(clippy::too_many_arguments)]
pub fn verify_plan(
    database: &DatabaseState,
    root: &Path,
    target: TargetKind,
    distro: Option<&str>,
    source_root: &str,
    identity: &str,
    revision: &str,
) -> Result<WorkspaceTaskPlan, Error> {
    let plan = preview(database, root, target, distro)?;
    plan.validate_claim(source_root, identity, revision)?;
    Ok(plan)
}
pub fn verify(
    database: &DatabaseState,
    executions: &[WorkspaceTaskExecution],
    require_trust: bool,
) -> Result<WorkspaceTaskPlan, Error> {
    let first = executions.first().ok_or(Error::SourceChanged)?;
    if executions.len() > tasks::MAX_TASKS
        || executions.iter().any(|execution| {
            execution.source_id != first.source_id
                || (require_trust
                    && (!execution.available
                        || !execution.trusted
                        || execution.task_kind == tasks::WorkspaceTaskKind::Shell
                            && !execution.shell_trusted))
        })
    {
        return Err(Error::SourceChanged);
    }
    let plan = verify_plan(
        database,
        Path::new(&first.source_root),
        first.target_kind,
        first.target_distro.as_deref(),
        &first.source_root,
        &first.project_identity,
        &first.revision,
    )?;
    tasks::verify_projected_executions(&plan, executions)?;
    Ok(plan)
}
pub(crate) type Provider = Arc<dyn NativeTaskSources>;

pub fn wsl_target(
    database: &DatabaseState,
    distro: &str,
    execution: Option<&WorkspaceTaskExecution>,
) -> Result<crate::platform::wsl::Target, Error> {
    if let Some(provider) = database.task_sources.get() {
        if let Some(target) = provider.wsl_target(distro, execution)? {
            return Ok(target);
        }
        if execution.is_some_and(|task| task.source_root.starts_with('/')) {
            return Err(Error::SourceChanged);
        }
    }
    Ok(crate::platform::wsl::Target::direct(distro))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Source(AtomicBool);
    impl NativeTaskSources for Source {
        fn preview(
            &self,
            root: &Path,
            target: TargetKind,
            distro: Option<&str>,
        ) -> Result<Option<WorkspaceTaskPlan>, Error> {
            let revision = if self.0.load(Ordering::Acquire) {
                "b".repeat(64)
            } else {
                "a".repeat(64)
            };
            let bytes=br#"{"version":"2.0.0","tasks":[{"label":"fixture","type":"process","command":"wrong","linux":{"command":"native-tool"},"args":["PLACEHOLDER/input"]}]}"#;
            let bytes = std::str::from_utf8(bytes)
                .unwrap()
                .replace("PLACEHOLDER", &format!("{}{}", '$', "{workspaceFolder}"));
            tasks::project_workspace_tasks(tasks::TaskProjection {
                source_root: root.to_str().unwrap(),
                target_root: root.to_str().unwrap(),
                project_identity: &"c".repeat(64),
                revision: &revision,
                target_kind: target,
                target_distro: distro,
                bytes: bytes.as_bytes(),
            })
            .map(Some)
        }
    }
    #[test]
    fn native_projection_is_untrusted_until_approved_and_never_falls_back_to_host_paths() {
        let database = DatabaseState::open_in_memory().unwrap();
        let source = Arc::new(Source(AtomicBool::new(false)));
        assert!(database.task_sources.set(source.clone()).is_ok());
        let root = Path::new("/__devbox_native_fixture_not_on_host__");
        let plan = preview(&database, root, TargetKind::Wsl, Some("Fixture")).unwrap();
        assert_eq!(plan.items[0].command.as_deref(), Some("native-tool"));
        assert_eq!(
            plan.items[0].args,
            vec!["/__devbox_native_fixture_not_on_host__/input"]
        );
        let applied = database
            .apply_workspace_task_import_at(&plan, &[plan.items[0].id.clone()], 100)
            .unwrap();
        let job = database
            .list_workspace_task_states()
            .unwrap()
            .remove(0)
            .job_id;
        let untrusted = database
            .get_workspace_task_execution(&job)
            .unwrap()
            .unwrap();
        assert!(verify(&database, std::slice::from_ref(&untrusted), true).is_err());
        database
            .trust_workspace_task_source_at(&applied.source_id, &plan.revision, 101)
            .unwrap();
        let trusted = database
            .get_workspace_task_execution(&job)
            .unwrap()
            .unwrap();
        assert!(verify(&database, std::slice::from_ref(&trusted), true).is_ok());
        assert!(wsl_target(&database, "Fixture", Some(&trusted)).is_err());
        source.0.store(true, Ordering::Release);
        assert!(verify(&database, std::slice::from_ref(&trusted), true).is_err());
    }
}
