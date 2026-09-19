//! The native target owns definition IO. Windows never opens a WSL POSIX root
//! through UNC, and helper responses never become renderer-selected file grants.
use crate::{core::registry::Binding, host::Host};
use product_contract::ProjectContext;
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) enum EditDestination {
    Native(super::definition_write::DefinitionTarget),
    #[cfg(windows)]
    Wsl,
}
pub(crate) enum DefinitionFiles {
    Native(Box<super::project_files::ProjectFiles>),
    #[cfg(windows)]
    Wsl {
        context: ProjectContext,
        lease: Box<super::wsl_project::WslProjectLease>,
    },
}
impl From<super::project_files::ProjectFiles> for DefinitionFiles {
    fn from(files: super::project_files::ProjectFiles) -> Self {
        Self::Native(Box::new(files))
    }
}
impl DefinitionFiles {
    pub(crate) fn open(host: &Host, context: &ProjectContext, deadline: u64) -> Result<Self> {
        crate::files_host::current_deadline(deadline)?;
        let projects = host.projects()?;
        #[cfg(windows)]
        if matches!(
            context.target,
            product_contract::ExecutionTarget::Wsl { .. }
        ) {
            let lease = projects.admit_wsl(host.helper_directory()?, context)?;
            lease.file_request_until(
                context,
                "definitions_attach",
                serde_json::json!({}),
                deadline,
            )?;
            return Ok(Self::Wsl {
                context: context.clone(),
                lease: Box::new(lease),
            });
        }
        let files = super::project_files::ProjectFiles::new(projects.admit(context)?)?;
        Ok(Self::Native(Box::new(files)))
    }
    #[cfg(windows)]
    pub(crate) fn native_task_launch(
        &self,
        cwd: &str,
        source_digest: String,
    ) -> Result<workspace_wsl::task_contract::TaskLaunch> {
        let Self::Wsl { lease, .. } = self else {
            return Err("invalid_target");
        };
        let launch = workspace_wsl::task_contract::TaskLaunch {
            schema_version: 1,
            root: lease.binding().root.clone(),
            cwd: cwd.into(),
            root_object: lease.native_root_object(),
            source_digest,
        };
        workspace_wsl::task_contract::validate(&launch)?;
        Ok(launch)
    }
    pub(crate) fn binding(&self) -> &Binding {
        match self {
            Self::Native(files) => files.lease().binding(),
            #[cfg(windows)]
            Self::Wsl { lease, .. } => lease.binding(),
        }
    }
    pub(crate) fn read(
        &mut self,
        path: &str,
        optional: bool,
        deadline: u64,
    ) -> Result<Option<Vec<u8>>> {
        crate::files_host::current_deadline(deadline)?;
        match self {
            Self::Native(files) => files.read_guarded(path, optional, &|| {
                crate::files_host::current_deadline(deadline)
            }),
            #[cfg(windows)]
            Self::Wsl { context, lease } => {
                let value = lease.file_request_until(
                    context,
                    "definitions_read",
                    serde_json::json!({"path":path,"optional":optional}),
                    deadline,
                )?;
                let bytes: Option<Vec<u8>> =
                    serde_json::from_value(value).map_err(|_| "wsl_protocol_invalid")?;
                if bytes
                    .as_ref()
                    .is_some_and(|value| value.len() > 2 * 1024 * 1024)
                {
                    return Err("wsl_protocol_invalid");
                }
                Ok(bytes)
            }
        }
    }
    pub(crate) fn revalidate(&self, deadline: u64) -> Result<()> {
        crate::files_host::current_deadline(deadline)?;
        match self {
            Self::Native(files) => {
                files.revalidate_guarded(&|| crate::files_host::current_deadline(deadline))
            }
            #[cfg(windows)]
            Self::Wsl { context, lease } => lease
                .file_request_until(
                    context,
                    "definitions_validate",
                    serde_json::json!({}),
                    deadline,
                )
                .map(|_| ()),
        }
    }
    pub(crate) fn prepare_project_write(&self, expected: Option<&[u8]>) -> Result<EditDestination> {
        match self {
            Self::Native(files) => Ok(EditDestination::Native(
                super::definition_write::DefinitionTarget::capture(
                    &std::path::Path::new(&files.lease().binding().root)
                        .join(".devbox/project.json"),
                    expected,
                )?,
            )),
            #[cfg(windows)]
            Self::Wsl { .. } => Ok(EditDestination::Wsl),
        }
    }
    #[cfg(windows)]
    pub(crate) fn write_project(&self, bytes: &[u8], deadline: u64) -> Result<Option<String>> {
        let Self::Wsl { context, lease } = self else {
            return Err("invalid_target");
        };
        let content = std::str::from_utf8(bytes).map_err(|_| "invalid_manifest")?;
        let value = lease.file_request_until(
            context,
            "definitions_write",
            serde_json::json!({"content":content}),
            deadline,
        )?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Saved {
            warning: Option<String>,
        }
        let saved: Saved = serde_json::from_value(value).map_err(|_| "wsl_protocol_invalid")?;
        if saved
            .warning
            .as_deref()
            .is_some_and(|warning| warning != "definition_durability_warning")
        {
            return Err("wsl_protocol_invalid");
        }
        Ok(saved.warning)
    }
}
