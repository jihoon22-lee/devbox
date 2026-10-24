//! Read-only development environment diagnostics.
pub mod doctor;
mod error;
pub mod install;
use devbox_protocol::{Exporter, method};
pub use error::ToolsError;
pub use install::{CheckRef, InstallPlan, WindowsPackage};
method!(pub ToolsInstallCommand = "tools.install_command", query, CheckRef => InstallPlan, ToolsError);
pub fn export<X: Exporter>(exporter: &mut X) {
    exporter.method::<ToolsInstallCommand>();
}

#[cfg(test)]
mod install_tests;
