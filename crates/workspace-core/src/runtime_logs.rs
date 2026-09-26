use crate::Host;
use logs_engine::core::{CoreError, RuntimeLogLease, RuntimeLogProvider, SourceSpec};
use std::sync::Arc;
pub fn provider(
    app: &tauri::AppHandle,
    host: Arc<Host>,
) -> Result<Arc<dyn RuntimeLogProvider>, &'static str> {
    let protected = crate::platform::storage_paths::from_host(app, &host)?;
    Ok(Arc::new(RuntimeLogs {
        app: app.clone(),
        host,
        protected,
    }))
}
struct RuntimeLogs {
    app: tauri::AppHandle,
    host: Arc<Host>,
    protected: crate::platform::storage_paths::ProtectedStorage,
}
struct OwnedLog {
    host: Arc<Host>,
    lease: runtime_engine::component::OwnedRunLog,
}
impl RuntimeLogProvider for RuntimeLogs {
    fn validate_source(&self, source: &SourceSpec) -> std::result::Result<(), CoreError> {
        if matches!(source, SourceSpec::Run { .. }) {
            return Err(CoreError::InvalidSource);
        }
        if let SourceSpec::LocalFile { path } | SourceSpec::Directory { path, .. } = source {
            let path = std::path::Path::new(path);
            self.protected
                .ensure_user_path(path)
                .map_err(|_| CoreError::InvalidSource)?;
            devbox_filesystem::ensure_no_links(path).map_err(|_| CoreError::InvalidSource)?;
            if let Ok(canonical) = std::fs::canonicalize(path) {
                self.protected
                    .ensure_user_path(&canonical)
                    .map_err(|_| CoreError::InvalidSource)?;
            }
        }
        Ok(())
    }

    fn resolve(
        &self,
        run_id: &str,
        revision: &str,
    ) -> std::result::Result<Box<dyn RuntimeLogLease>, CoreError> {
        self.host
            .component("runtime")
            .map_err(|_| CoreError::AdapterUnavailable)?;
        let lease = runtime_engine::component::log_descriptor(&self.app, run_id)
            .map_err(|_| CoreError::AdapterUnavailable)?;
        if lease.revision() != revision {
            return Err(CoreError::StaleOperation);
        }
        Ok(Box::new(OwnedLog {
            host: self.host.clone(),
            lease,
        }))
    }
}
impl RuntimeLogLease for OwnedLog {
    fn data_root(&self) -> &std::path::Path {
        self.lease.data_root()
    }
    fn revalidate(&self) -> std::result::Result<(), CoreError> {
        self.host
            .component("runtime")
            .map_err(|_| CoreError::StaleOperation)?;
        self.lease
            .revalidate()
            .map_err(|_| CoreError::StaleOperation)
    }
}
