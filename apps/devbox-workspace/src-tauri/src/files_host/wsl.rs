use super::*;
use crate::platform::wsl_files::WslFiles;

pub(super) fn posix(raw: &str) -> bool {
    devbox_filesystem::parse_safe_project_path(raw)
        .is_some_and(|path| path.kind() == devbox_filesystem::ProjectPathKind::Posix)
}
impl FilesHost {
    pub(super) fn wsl_owner(&self, context: Option<&ProjectContext>) -> Result<&WslFiles> {
        self.wsl
            .iter()
            .find(|owner| Some(owner.context()) == context)
            .ok_or("file_selection_required")
    }
    pub(super) fn wsl_owner_mut(
        &mut self,
        context: Option<&ProjectContext>,
    ) -> Result<&mut WslFiles> {
        self.wsl
            .iter_mut()
            .find(|owner| Some(owner.context()) == context)
            .ok_or("file_selection_required")
    }
    pub(super) fn execute_wsl(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        method: &str,
        args: Value,
        deadline: u64,
    ) -> Result<Value> {
        let context = context.ok_or("project_selection_required")?;
        if !matches!(
            context.target,
            product_contract::ExecutionTarget::Wsl { .. }
        ) {
            return Err("file_target_unavailable");
        }
        let projects = host.projects()?;
        current_deadline(deadline)?;
        self.wsl.retain(|owner| {
            owner.documents.has_documents()
                || (owner.context() == context && owner.is_open())
                || owner.shutdown().is_err()
        });
        current_deadline(deadline)?;
        if !self.wsl.iter().any(|owner| owner.context() == context) {
            if self.wsl.len() >= 4 {
                return Err("file_context_limit");
            }
            let owner =
                WslFiles::open_until(&projects, host.helper_directory()?, context, deadline)?;
            self.wsl.push(owner);
        }
        current_deadline(deadline)?;
        self.wsl_owner_mut(Some(context))?
            .execute(&projects, context, method, args, deadline)
    }
    pub(super) fn close_wsl(&mut self, context: Option<&ProjectContext>, path: &str) -> Result<()> {
        if let Ok(owner) = self.wsl_owner_mut(context) {
            owner.close(path)?;
        }
        self.wsl
            .retain(|owner| owner.documents.has_documents() || owner.shutdown().is_err());
        Ok(())
    }
    pub(crate) fn poll_wsl(
        &mut self,
        host: &Host,
        context: &ProjectContext,
    ) -> Result<Vec<workspace_wsl::files::WatchSnapshot>> {
        let projects = host.projects()?;
        let Ok(owner) = self.wsl_owner_mut(Some(context)) else {
            return Ok(vec![]);
        };
        owner.poll(&projects, context)
    }
    pub(crate) fn retire_wsl(&mut self) -> Result<()> {
        self.wsl.retain(|owner| owner.shutdown().is_err());
        if self.wsl.is_empty() {
            Ok(())
        } else {
            Err("wsl_shutdown_unconfirmed")
        }
    }
}
