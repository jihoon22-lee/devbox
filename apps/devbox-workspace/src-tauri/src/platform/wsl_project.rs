//! Linux observations remain opaque native leases; Registry receives only
//! canonical, distro-instance-bound identity after a verified helper response.
#[cfg(windows)]
mod native {
    use crate::{
        core::registry::{Binding, ObjectStamp},
        platform::wsl_helper::Connection,
    };
    use product_contract::ExecutionTarget;
    use product_contract::ProjectContext;
    use serde_json::Value;
    use std::{path::Path, sync::Mutex};
    type Result<T> = std::result::Result<T, &'static str>;
    pub struct WslProjectLease {
        binding: Binding,
        native_root_object: workspace_wsl::ObjectStamp,
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
            let native_root_object = report.root_object.clone();
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
                native_root_object,
                token: report.token,
                connection: Mutex::new(connection),
            })
        }
        pub(crate) fn native_root_object(&self) -> workspace_wsl::ObjectStamp {
            self.native_root_object.clone()
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
        pub fn shutdown(&self) -> Result<()> {
            self.connection
                .lock()
                .map_err(|_| "wsl_connection_busy")?
                .shutdown()
        }
        pub fn is_open(&self) -> bool {
            self.connection
                .lock()
                .is_ok_and(|connection| connection.is_open())
        }
        pub fn distro_name(&self) -> Result<String> {
            let mut connection = self.connection.lock().map_err(|_| "wsl_connection_busy")?;
            connection.validate(&self.token)?;
            Ok(connection.lease().name().to_owned())
        }
        pub fn reveal_admitted_file(
            &self,
            native: &str,
            reveal: &dyn Fn(&Path) -> Result<()>,
        ) -> Result<()> {
            let mut connection = self.connection.lock().map_err(|_| "wsl_connection_busy")?;
            connection.validate(&self.token)?;
            let target = crate::core::wsl_files::explorer_path(
                connection.lease().name(),
                &self.binding.root,
                native,
            )?;
            reveal(Path::new(&target))?;
            connection.validate(&self.token)
        }
        pub fn execute_source(
            &self,
            context: &ProjectContext,
            mut args: Value,
            expires: std::time::Instant,
            cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
            authorize: &dyn Fn(&str) -> Result<()>,
        ) -> Result<Value> {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err("source_cancelled");
            }
            context.validate().map_err(|_| "wsl_context_invalid")?;
            if context.target != self.binding.target
                || !args.is_object()
                || args.get("context").is_some()
            {
                return Err("wsl_context_invalid");
            }
            args["context"] = serde_json::to_value(context).map_err(|_| "wsl_context_invalid")?;
            let mut connection = self.connection.lock().map_err(|_| "wsl_connection_busy")?;
            connection.validate(&self.token)?;
            connection.execute_source(&self.token, args, expires, cancelled, authorize)
        }
        pub fn execute_lsp(
            &self,
            context: &ProjectContext,
            mut args: Value,
            expires: std::time::Instant,
            cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
            authorize: &dyn Fn(&str) -> Result<()>,
        ) -> Result<Value> {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err("lsp_operation_cancelled");
            }
            context.validate().map_err(|_| "wsl_context_invalid")?;
            if context.target != self.binding.target
                || !args.is_object()
                || args.get("context").is_some()
            {
                return Err("wsl_context_invalid");
            }
            args["context"] = serde_json::to_value(context).map_err(|_| "wsl_context_invalid")?;
            let mut connection = self.connection.lock().map_err(|_| "wsl_connection_busy")?;
            connection.execute_lsp(&self.token, args, expires, cancelled, authorize)
        }
        pub fn file_request(
            &self,
            context: &ProjectContext,
            method: &str,
            args: Value,
        ) -> Result<Value> {
            self.file_request_until(context, method, args, u64::MAX)
        }
        pub fn file_request_until(
            &self,
            context: &ProjectContext,
            method: &str,
            mut args: Value,
            deadline: u64,
        ) -> Result<Value> {
            context.validate().map_err(|_| "wsl_context_invalid")?;
            if context.target != self.binding.target
                || !args.is_object()
                || args.get("context").is_some()
            {
                return Err("wsl_context_invalid");
            }
            args["context"] = serde_json::to_value(context).map_err(|_| "wsl_context_invalid")?;
            let mut connection = self.connection.lock().map_err(|_| "wsl_connection_busy")?;
            connection.validate(&self.token)?;
            let result = connection.file_request_until(method, &self.token, args, deadline)?;
            if method == "files_open" {
                workspace_wsl::file_transfer::receive(result, |token, offset| {
                    connection.file_request_until(
                        "files_open_chunk",
                        &self.token,
                        serde_json::json!({"context":context,"token":token,"offset":offset}),
                        deadline,
                    )
                })
            } else {
                Ok(result)
            }
        }
    }
}
#[cfg(windows)]
pub use native::WslProjectLease;
