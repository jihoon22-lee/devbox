//! Retained native project/distro identity for every Terminal launch/probe.
use crate::host::Host;
use product_contract::ProjectContext;
use std::sync::Arc;
use wsl_desktop_lib::component::{TerminalLaunchFactory, TerminalLaunchLease};

#[cfg(any(windows, test))]
use super::runtime_bridge::outside_runtime;

pub(crate) struct Factory<'a> {
    pub host: &'a Host,
    pub context: Option<&'a ProjectContext>,
    pub deadline: u64,
}
impl TerminalLaunchFactory for Factory<'_> {
    fn capture(&self, distro: &str) -> Result<Arc<dyn TerminalLaunchLease>, String> {
        #[cfg(windows)]
        {
            outside_runtime(|| native::capture(self, distro, true))
                .and_then(|result| result)
                .map(|lease| Arc::new(lease) as Arc<dyn TerminalLaunchLease>)
                .map_err(str::to_owned)
        }
        #[cfg(not(windows))]
        {
            let _ = (self.host, self.context, self.deadline, distro);
            Err("terminal_windows_required".into())
        }
    }
}

/// A Runtime process keeps the distro/executable binding beyond the request deadline.
/// Cleanup does not depend on a mutable project selection and never starts a stopped distro.
#[cfg(windows)]
pub(crate) fn capture_runtime(
    host: &Host,
    distro: &str,
    deadline: u64,
) -> Result<Arc<dyn run_manager_lib::platform::wsl::CommandBinding>, String> {
    outside_runtime(|| {
        native::capture_runtime(
            &Factory {
                host,
                context: None,
                deadline,
            },
            distro,
        )
    })
    .and_then(|result| result)
    .map_err(str::to_owned)
}

pub(crate) fn capture_running(
    host: &Host,
    distro: &str,
    deadline: u64,
) -> Result<Arc<dyn TerminalLaunchLease>, String> {
    let factory = Factory {
        host,
        context: None,
        deadline,
    };
    #[cfg(windows)]
    let inner = outside_runtime(|| native::capture(&factory, distro, false))
        .and_then(|result| result)
        .map(|lease| Arc::new(lease) as Arc<dyn TerminalLaunchLease>)
        .map_err(str::to_owned)?;
    #[cfg(not(windows))]
    let inner = factory.capture(distro)?;
    Ok(Arc::new(ManagementLease { inner, deadline }))
}
struct ManagementLease {
    inner: Arc<dyn TerminalLaunchLease>,
    deadline: u64,
}
impl TerminalLaunchLease for ManagementLease {
    fn revalidate(&self) -> Result<(), String> {
        crate::files_host::current_deadline(self.deadline).map_err(str::to_owned)?;
        self.inner.revalidate()
    }
    fn bind_argv(&self, argv: Vec<String>) -> Result<Vec<String>, String> {
        self.revalidate()?;
        self.inner.bind_argv(argv)
    }
    fn retire(&self) -> Result<(), String> {
        self.inner.retire()
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use crate::{
        core::registry::Binding,
        platform::{project_probe::ProjectLease, wsl_distro, wsl_project::WslProjectLease},
        project_owner::ProjectOwner,
    };
    use devbox_filesystem::{filesystem_identity, open_filesystem_object, FilesystemIdentity};
    use product_contract::ExecutionTarget;
    use std::{fs::File, path::PathBuf};
    type Result<T> = std::result::Result<T, &'static str>;
    enum Project {
        Windows(Box<ProjectLease>),
        Wsl(Box<WslProjectLease>),
    }
    pub(super) struct Admission {
        projects: Arc<ProjectOwner>,
        context: Option<ProjectContext>,
        binding: Option<Binding>,
        project: Option<Project>,
        distro: wsl_distro::Lease,
        allow_start: bool,
        executable: PathBuf,
        executable_identity: FilesystemIdentity,
        _executable: File,
    }
    pub(super) fn capture(
        factory: &Factory<'_>,
        name: &str,
        allow_start: bool,
    ) -> Result<Admission> {
        crate::files_host::current_deadline(factory.deadline)?;
        let distro = wsl_distro::list()?
            .into_iter()
            .find(|distro| distro.name == name)
            .ok_or("wsl_distro_missing")?;
        // Capture allows a stopped target for this explicit terminal start. It
        // opens registration/backing handles and performs no start itself.
        let distro = wsl_distro::Lease::capture(&distro.id, allow_start)?;
        let projects = factory.host.projects()?;
        let binding = factory
            .context
            .map(|context| projects.binding(context))
            .transpose()?;
        let project = match factory.context.map(|context| &context.target) {
            None => None,
            Some(ExecutionTarget::Windows) => Some(Project::Windows(Box::new(
                projects.admit(factory.context.ok_or("invalid_context")?)?,
            ))),
            Some(ExecutionTarget::Wsl { distro_id }) => {
                if distro.id() != distro_id {
                    return Err("terminal_distro_mismatch");
                }
                let lease = WslProjectLease::observe(
                    factory.host.helper_directory()?,
                    distro_id,
                    &binding.as_ref().ok_or("invalid_context")?.root,
                    true,
                )?;
                if Some(lease.binding()) != binding.as_ref() {
                    let _ = lease.shutdown();
                    return Err("project_binding_changed");
                }
                Some(Project::Wsl(Box::new(lease)))
            }
        };
        let executable = wsl_distro::executable()?;
        let (file, executable_identity) =
            open_filesystem_object(&executable, false).map_err(|_| "wsl_executable_unavailable")?;
        let admission = Admission {
            projects,
            context: factory.context.cloned(),
            binding,
            project,
            distro,
            allow_start,
            executable,
            executable_identity,
            _executable: file,
        };
        admission.check()?;
        crate::files_host::current_deadline(factory.deadline)?;
        Ok(admission)
    }
    struct RuntimeAdmission(Admission);
    impl run_manager_lib::platform::wsl::CommandBinding for RuntimeAdmission {
        fn bind(
            &self,
            argv: Vec<String>,
        ) -> std::result::Result<Vec<String>, run_manager_lib::platform::wsl::WslExecutionError>
        {
            super::outside_runtime(|| self.0.distro.require_running())
                .and_then(|result| result)
                .map_err(|_| std::io::Error::other("runtime-target-unavailable"))?;
            self.0
                .bind_argv(argv)
                .map_err(|_| std::io::Error::other("runtime-target-changed").into())
        }
        fn bind_launch(
            &self,
            argv: Vec<String>,
        ) -> std::result::Result<Vec<String>, run_manager_lib::platform::wsl::WslExecutionError>
        {
            self.0
                .bind_argv(argv)
                .map_err(|_| std::io::Error::other("runtime-target-changed").into())
        }
    }
    pub(super) fn capture_runtime(
        factory: &Factory<'_>,
        distro: &str,
    ) -> Result<Arc<dyn run_manager_lib::platform::wsl::CommandBinding>> {
        Ok(Arc::new(RuntimeAdmission(capture(factory, distro, true)?)))
    }
    impl Drop for Admission {
        fn drop(&mut self) {
            if let Some(project) = self.project.take() {
                let _ = super::outside_runtime(|| drop(project));
            }
        }
    }
    impl Admission {
        fn check(&self) -> Result<()> {
            super::outside_runtime(|| self.check_native())?
        }
        fn check_native(&self) -> Result<()> {
            if let Some(context) = &self.context {
                if Some(self.projects.binding(context)?) != self.binding {
                    return Err("project_binding_changed");
                }
            }
            self.distro.revalidate()?;
            if filesystem_identity(&self.executable, false)
                .map_err(|_| "wsl_executable_unavailable")?
                != self.executable_identity
            {
                return Err("wsl_executable_changed");
            }
            match &self.project {
                Some(Project::Windows(lease)) => lease.revalidate(),
                Some(Project::Wsl(lease)) => lease.revalidate(),
                None => Ok(()),
            }
        }
    }
    impl TerminalLaunchLease for Admission {
        fn revalidate(&self) -> std::result::Result<(), String> {
            self.check().map_err(str::to_owned)
        }
        fn bind_argv(&self, mut argv: Vec<String>) -> std::result::Result<Vec<String>, String> {
            super::outside_runtime(|| {
                self.revalidate()?;
                if !self.allow_start {
                    self.distro.require_running().map_err(str::to_owned)?;
                }
                if argv.len() < 3
                    || argv[0] != "wsl.exe"
                    || argv[1] != "-d"
                    || argv[2] != self.distro.name()
                {
                    return Err("terminal_target_invalid".into());
                }
                argv[0] = self
                    .executable
                    .to_str()
                    .ok_or("wsl_executable_unavailable")?
                    .into();
                argv[1] = "--distribution-id".into();
                argv[2] = self.distro.id().into();
                Ok(argv)
            })
            .map_err(str::to_owned)?
        }
        fn retire(&self) -> std::result::Result<(), String> {
            super::outside_runtime(|| {
                if let Some(Project::Wsl(lease)) = &self.project {
                    if lease.is_open() {
                        return lease.shutdown();
                    }
                }
                Ok(())
            })
            .and_then(|result| result)
            .map_err(str::to_owned)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::outside_runtime;
    #[test]
    fn synchronous_lease_runtime_and_drop_complete_outside_the_pty_runtime() {
        let outer = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        outer.block_on(async {
            let value = outside_runtime(|| {
                assert!(tokio::runtime::Handle::try_current().is_err());
                let lease = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let result = lease.block_on(async { 42 });
                drop(lease);
                result
            })
            .unwrap();
            assert_eq!(value, 42);
        });
    }
}
