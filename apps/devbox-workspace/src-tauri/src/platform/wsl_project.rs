//! Linux observations remain opaque native leases; Registry receives only
//! canonical, distro-instance-bound identity after a verified helper response.
#[cfg(windows)]
mod native {
    use crate::{
        core::registry::{Binding, ObjectStamp},
        platform::wsl_helper::Connection,
    };
    use product_contract::ExecutionTarget;
    use std::{path::Path, sync::Mutex};
    type Result<T> = std::result::Result<T, &'static str>;
    pub struct WslProjectLease {
        binding: Binding,
        token: String,
        connection: Mutex<Connection>,
    }
    impl WslProjectLease {
        pub fn observe(
            resources: &Path,
            distro: &str,
            path: &str,
            allow_start: bool,
        ) -> Result<Self> {
            let parsed = devbox_filesystem::parse_safe_project_path(path).ok_or("invalid_root")?;
            if parsed.kind() != devbox_filesystem::ProjectPathKind::Posix || path == "/" {
                return Err("invalid_target");
            }
            let mut connection = Connection::connect(resources, distro, allow_start)?;
            let report = connection.observe(path)?;
            if !workspace_wsl::token(&report.token) {
                return Err("wsl_protocol_invalid");
            }
            let stamp = |source: workspace_wsl::ObjectStamp| ObjectStamp {
                scope: connection.lease().scope(&source.scope),
                object: source.object,
            };
            let binding = Binding {
                target: ExecutionTarget::Wsl {
                    distro_id: connection.lease().id().into(),
                },
                root: report.root,
                root_object: stamp(report.root_object),
                repository_object: report.repository_object.map(stamp),
            };
            binding.validate()?;
            connection.validate(&report.token)?;
            Ok(Self {
                binding,
                token: report.token,
                connection: Mutex::new(connection),
            })
        }
        pub fn binding(&self) -> &Binding {
            &self.binding
        }
        pub fn revalidate(&self) -> Result<()> {
            self.connection
                .lock()
                .map_err(|_| "wsl_connection_busy")?
                .validate(&self.token)
        }
    }
}
#[cfg(windows)]
pub use native::WslProjectLease;
