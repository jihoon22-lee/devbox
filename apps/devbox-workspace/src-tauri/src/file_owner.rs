//! The Windows host and Linux helper share native grants and file conflict checks.
pub use workspace_wsl::files::{EditorSnapshot, FileOwner, Scope};
impl workspace_wsl::files::RootLease for crate::platform::project_probe::ProjectLease {
    fn root(&self) -> &std::path::Path {
        std::path::Path::new(&self.binding().root)
    }
    fn target(&self) -> &product_contract::ExecutionTarget {
        &self.binding().target
    }
    fn native_root_identity(&self) -> devbox_filesystem::FilesystemIdentity {
        self.native_root_identity()
    }
    fn revalidate(&self) -> Result<(), &'static str> {
        self.revalidate()
    }
}
