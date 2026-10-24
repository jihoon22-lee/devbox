//! Read-only development environment diagnostics.
pub mod doctor;
mod error;
pub mod install;
pub mod support;
pub type R<T> = Result<T, devbox_protocol::Fail<ToolsError>>;
use devbox_protocol::{Exporter, method};
pub use error::ToolsError;
pub use install::{CheckRef, InstallPlan, WindowsPackage};
pub use support::SupportBundle;
#[derive(serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct ToolsEmpty {}
#[derive(serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ToolsDiagnosticsInfo {
    pub log_dir: String,
}
method!(pub ToolsSupportBundle = "tools.support_bundle", mutation, ToolsEmpty => SupportBundle, ToolsError);
method!(pub ToolsDiagnostics = "tools.diagnostics", query, ToolsEmpty => ToolsDiagnosticsInfo, ToolsError);
method!(pub ToolsInstallCommand = "tools.install_command", query, CheckRef => InstallPlan, ToolsError);
pub fn export<X: Exporter>(exporter: &mut X) {
    exporter.method::<ToolsInstallCommand>();
    exporter.method::<ToolsSupportBundle>();
    exporter.method::<ToolsDiagnostics>();
}

#[cfg(test)]
mod install_tests;
