//! Retained native project/distro identity for every Terminal launch/probe.
use crate::host::Host;
use product_contract::ProjectContext;
use std::sync::Arc;
use wsl_desktop_lib::component::{TerminalLaunchFactory, TerminalLaunchLease};

pub(crate) struct Factory<'a> {
    pub host: &'a Host,
    pub context: &'a ProjectContext,
    pub deadline: u64,
}
impl TerminalLaunchFactory for Factory<'_> {
    fn capture(&self, distro: &str) -> Result<Arc<dyn TerminalLaunchLease>, String> {
        #[cfg(windows)]
        {
            native::capture(self, distro)
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
        Windows(ProjectLease),
        Wsl(WslProjectLease),
    }
    pub(super) struct Admission {
        projects: Arc<ProjectOwner>,
        context: ProjectContext,
        binding: Binding,
        project: Project,
        distro: wsl_distro::Lease,
        executable: PathBuf,
        executable_identity: FilesystemIdentity,
        _executable: File,
    }
    pub(super) fn capture(factory: &Factory<'_>, name: &str) -> Result<Admission> {
        crate::files_host::current_deadline(factory.deadline)?;
        let distro = wsl_distro::list()?
            .into_iter()
            .find(|distro| distro.name == name)
            .ok_or("wsl_distro_missing")?;
        // Capture allows a stopped target for this explicit terminal start. It
        // opens registration/backing handles and performs no start itself.
        let distro = wsl_distro::Lease::capture(&distro.id, true)?;
        let projects = factory.host.projects()?;
        let binding = projects.binding(factory.context)?;
        let project = match &factory.context.target {
            ExecutionTarget::Windows => Project::Windows(projects.admit(factory.context)?),
            ExecutionTarget::Wsl { distro_id } => {
                if distro.id() != distro_id {
                    return Err("terminal_distro_mismatch");
                }
                let lease = WslProjectLease::observe(
                    factory.host.helper_directory()?,
                    distro_id,
                    &binding.root,
                    true,
                )?;
                if lease.binding() != &binding {
                    let _ = lease.shutdown();
                    return Err("project_binding_changed");
                }
                Project::Wsl(lease)
            }
        };
        let executable = wsl_distro::executable()?;
        let (file, executable_identity) =
            open_filesystem_object(&executable, false).map_err(|_| "wsl_executable_unavailable")?;
        let admission = Admission {
            projects,
            context: factory.context.clone(),
            binding,
            project,
            distro,
            executable,
            executable_identity,
            _executable: file,
        };
        admission.check()?;
        crate::files_host::current_deadline(factory.deadline)?;
        Ok(admission)
    }
    impl Admission {
        fn check(&self) -> Result<()> {
            if self.projects.binding(&self.context)? != self.binding {
                return Err("project_binding_changed");
            }
            self.distro.revalidate()?;
            if filesystem_identity(&self.executable, false)
                .map_err(|_| "wsl_executable_unavailable")?
                != self.executable_identity
            {
                return Err("wsl_executable_changed");
            }
            match &self.project {
                Project::Windows(lease) => lease.revalidate(),
                Project::Wsl(lease) => lease.revalidate(),
            }
        }
    }
    impl TerminalLaunchLease for Admission {
        fn revalidate(&self) -> std::result::Result<(), String> {
            self.check().map_err(str::to_owned)
        }
        fn bind_argv(&self, mut argv: Vec<String>) -> std::result::Result<Vec<String>, String> {
            self.revalidate()?;
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
        }
        fn retire(&self) -> std::result::Result<(), String> {
            if let Project::Wsl(lease) = &self.project {
                if lease.is_open() {
                    return lease.shutdown().map_err(str::to_owned);
                }
            }
            Ok(())
        }
    }
}
