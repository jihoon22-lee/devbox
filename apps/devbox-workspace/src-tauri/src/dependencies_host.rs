//! Native inventory follows the selected project target. Windows retains its
//! private summary/cache and the existing explicit remote-enrichment approval.
use crate::{host::Host, private_metadata::MetadataRoot};
use product_contract::ProjectContext;
use repo_manager_lib::component::DependencyAccess;
use std::sync::Arc;
type Result<T> = std::result::Result<T, &'static str>;

pub(crate) fn access<T: Send + Sync + 'static>(
    host: Arc<Host>,
    context: ProjectContext,
    deadline: u64,
    retained: T,
) -> Result<DependencyAccess> {
    crate::files_host::current_deadline(deadline)?;
    let owner = host.projects()?;
    let common = MetadataRoot::open(&host.component("common")?)?;
    let key = serde_json::to_string(&context).map_err(|_| "invalid_context")?;
    #[cfg(windows)]
    if matches!(
        context.target,
        product_contract::ExecutionTarget::Wsl { .. }
    ) {
        let lease = Arc::new(owner.admit_wsl(host.helper_directory()?, &context)?);
        lease.file_request_until(&context, "files_attach", serde_json::json!({}), deadline)?;
        let root = lease.binding().root.clone();
        let reader = lease.clone();
        let reader_context = context.clone();
        return DependencyAccess::for_native_inventory(
            root,
            key,
            common.path().into(),
            move || {
                let _retained = &retained;
                crate::files_host::current_deadline(deadline).map_err(str::to_owned)?;
                if host.component("common").map_err(str::to_owned)? != common.path()
                    || owner.binding(&context).map_err(str::to_owned)? != *lease.binding()
                {
                    return Err("dependency_context_changed".into());
                }
                common.revalidate().map_err(str::to_owned)?;
                lease.revalidate().map_err(str::to_owned)?;
                crate::files_host::current_deadline(deadline).map_err(str::to_owned)
            },
            move |budget| {
                crate::files_host::current_deadline(deadline).map_err(str::to_owned)?;
                reader
                    .file_request_until(
                        &reader_context,
                        "dependency_inventory",
                        serde_json::json!({"budgetMs": budget.as_millis().min(10_000) as u64}),
                        deadline,
                    )
                    .map_err(str::to_owned)
            },
        )
        .map_err(|error| repo_manager_lib::component::dependency_issue(&error));
    }
    let lease = owner.admit(&context)?;
    let root = std::path::PathBuf::from(&lease.binding().root);
    DependencyAccess::for_project(root, key, common.path().into(), move || {
        let _retained = &retained;
        crate::files_host::current_deadline(deadline).map_err(str::to_owned)?;
        if host.component("common").map_err(str::to_owned)? != common.path()
            || owner.binding(&context).map_err(str::to_owned)? != *lease.binding()
        {
            return Err("dependency_context_changed".into());
        }
        common.revalidate().map_err(str::to_owned)?;
        lease.revalidate().map_err(str::to_owned)?;
        crate::files_host::current_deadline(deadline).map_err(str::to_owned)
    })
    .map_err(|error| repo_manager_lib::component::dependency_issue(&error))
}
