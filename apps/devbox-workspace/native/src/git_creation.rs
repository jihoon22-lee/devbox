//! One-use native worktree destinations. Review never creates a directory or Git job.
use crate::git_worktree::WorktreeTarget;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) struct Creation {
    id: String,
    target: Arc<WorktreeTarget>,
    branch: String,
    created: Instant,
}
impl Creation {
    pub(crate) fn capture(
        root: &Path,
        forbidden: &[PathBuf],
        branch: String,
        path: &str,
        deadline: u64,
    ) -> Result<Self> {
        if !repo_manager_lib::component::source_branch_valid(&branch) {
            return Err("worktree_branch_invalid");
        }
        let target = WorktreeTarget::capture_with_admission(
            root,
            path,
            forbidden,
            deadline,
            crate::linux_files::admit,
        )?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            target: Arc::new(target),
            branch,
            created: Instant::now(),
        })
    }
    pub(crate) fn view(&self) -> Value {
        json!({"previewId":self.id,"branch":self.branch,"targetDir":self.target.path()})
    }
    pub(crate) fn consume(
        self,
        args: Value,
        deadline: u64,
    ) -> Result<(
        Arc<WorktreeTarget>,
        repo_manager_lib::component::SourceCreation,
    )> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            preview_id: String,
            operation_id: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "wsl_request_invalid")?;
        if input.preview_id != self.id || self.created.elapsed() >= Duration::from_secs(180) {
            return Err("worktree_preview_stale");
        }
        self.target.revalidate(deadline)?;
        let path = self
            .target
            .path()
            .to_str()
            .ok_or("worktree_target_invalid")?
            .to_owned();
        let target = self.target.clone();
        let creation = repo_manager_lib::component::SourceCreation::new(
            self.branch,
            path,
            input.operation_id,
            move || target.revalidate(deadline).map_err(str::to_owned),
        )
        .map_err(|_| "worktree_target_invalid")?;
        Ok((self.target, creation))
    }
}
