//! Native retained approval and project path authority. Windows supplies a
//! fresh one-use callback for each driven request; it owns durable approval.
use crate::{engine::SourceAuthorization, files::RootLease, lsp_review::Review};
use code_pad_lib::lsp::{
    DocumentError, LspConfig, LspExecutionAuthority, LspManagerError, ResolvedProcess,
};
use devbox_filesystem::{ensure_no_links, filesystem_identity, project::ProjectObservation};
use product_contract::ProjectContext;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) struct Lease {
    pub(crate) context: ProjectContext,
    pub(crate) observation: ProjectObservation,
}
impl RootLease for Lease {
    fn root(&self) -> &Path {
        self.observation.root()
    }
    fn target(&self) -> &product_contract::ExecutionTarget {
        &self.context.target
    }
    fn native_root_identity(&self) -> devbox_filesystem::FilesystemIdentity {
        self.observation.root_identity()
    }
    fn revalidate(&self) -> Result<()> {
        self.observation.revalidate()
    }
}
#[derive(Clone)]
pub(crate) struct RequestScope {
    pub(crate) authorize: SourceAuthorization,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) expires: Instant,
}
impl RequestScope {
    pub(crate) fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("lsp_operation_cancelled");
        }
        if Instant::now() >= self.expires {
            return Err("request_expired");
        }
        Ok(())
    }
}
pub(crate) struct Authority {
    pub(crate) review: Arc<Review>,
    pub(crate) lease: Arc<Lease>,
    current: Mutex<Option<RequestScope>>,
    shutdown: AtomicBool,
}
impl Authority {
    pub(crate) fn new(review: Arc<Review>, lease: Arc<Lease>) -> Self {
        Self {
            review,
            lease,
            current: Mutex::new(None),
            shutdown: AtomicBool::new(false),
        }
    }
    pub(crate) fn bind(&self, scope: RequestScope) -> Result<()> {
        *self.current.lock().map_err(|_| "lsp_unavailable")? = Some(scope);
        Ok(())
    }
    pub(crate) fn clear(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
    }
    pub(crate) fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.clear();
    }
    pub(crate) fn scope(&self) -> Result<RequestScope> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err("lsp_operation_cancelled");
        }
        let scope = self
            .current
            .lock()
            .map_err(|_| "lsp_unavailable")?
            .clone()
            .ok_or("lsp_execution_approval_required")?;
        scope.check()?;
        Ok(scope)
    }
    pub(crate) fn check(&self) -> Result<()> {
        self.scope()?.check()
    }
    pub(crate) fn document(&self, path: &Path) -> Result<()> {
        self.check()?;
        let root = self.lease.root();
        if !path.starts_with(root) {
            return Err("lsp_document_denied");
        }
        crate::linux_files::admit(path)?;
        ensure_no_links(path).map_err(|_| "lsp_document_denied")?;
        self.lease.revalidate()?;
        let canonical = path.canonicalize().map_err(|_| "lsp_document_denied")?;
        if canonical != path || !canonical.starts_with(root) {
            return Err("lsp_document_denied");
        }
        crate::linux_files::admit(&canonical)?;
        let parent = if path == root {
            path
        } else {
            path.parent().ok_or("lsp_document_denied")?
        };
        let mut found = false;
        for parent in parent.ancestors() {
            if filesystem_identity(parent, true).ok() == Some(self.lease.native_root_identity()) {
                found = true;
                break;
            }
            if parent == root {
                break;
            }
        }
        if !found {
            return Err("lsp_document_denied");
        }
        self.lease.revalidate()?;
        self.check()
    }
}
impl LspExecutionAuthority for Authority {
    fn begin_start(&self) -> std::result::Result<Option<Box<dyn Send>>, LspManagerError> {
        let scope = self
            .scope()
            .map_err(|_| LspManagerError::ExecutionApprovalRequired)?;
        // The Windows callback retains its context/filesystem permits until
        // this whole request's startup-retirement barrier acknowledges them.
        (scope.authorize)(
            self.lease
                .root()
                .to_str()
                .ok_or(LspManagerError::ExecutionApprovalRequired)?,
        )
        .map_err(|_| LspManagerError::ExecutionApprovalRequired)?;
        scope
            .check()
            .map_err(|_| LspManagerError::ExecutionApprovalRequired)?;
        Ok(None)
    }
    fn validate_config(
        &self,
        language: &str,
        config: &LspConfig,
    ) -> std::result::Result<(), LspManagerError> {
        if self.check().is_err()
            || self.review.config() != config
            || !self.review.processes.contains_key(language)
            || self.lease.revalidate().is_err()
        {
            return Err(LspManagerError::ExecutionApprovalRequired);
        }
        Ok(())
    }
    fn validate_process(
        &self,
        process: &ResolvedProcess,
    ) -> std::result::Result<(), LspManagerError> {
        let validate = || -> Result<()> {
            let scope = self.scope()?;
            if !self
                .review
                .processes
                .values()
                .any(|expected| expected == process)
            {
                return Err("lsp_execution_approval_required");
            }
            self.review
                .revalidate(self.review.digest(), &|| self.check())?;
            (scope.authorize)(
                self.lease
                    .root()
                    .to_str()
                    .ok_or("lsp_source_path_invalid")?,
            )?;
            self.lease.revalidate()?;
            self.check()
        };
        validate().map_err(|_| LspManagerError::ExecutionApprovalRequired)
    }
    fn startup_cancelled(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            while self.check().is_ok() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    }
    fn validate_document_path(&self, path: &Path) -> std::result::Result<(), DocumentError> {
        self.document(path).map_err(|_| DocumentError::AccessDenied)
    }
    fn validate_document_write_path(&self, _: &Path) -> std::result::Result<(), DocumentError> {
        Err(DocumentError::AccessDenied)
    }
}
