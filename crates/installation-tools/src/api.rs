use crate::commands::{
    dev_setup::{DevSetupApplyRequest, DevSetupPreviewRequest},
    diagnostics::CancelDiagnosticsRequest,
    related_tools::RelatedToolInstallRequest,
};
use product_ipc::ExecutionClass;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum ToolsCall {
    RunDiagnosis {},
    PreviewSupportBundle { operation_id: String },
    CancelSupportBundle { request: CancelDiagnosticsRequest },
    ExportSupportBundle { preview_id: String },
    DevSetupAudit {},
    ImportDevSetupConfiguration {},
    DiscardDevSetupConfiguration { request: DevSetupPreviewRequest },
    ExportDevSetupConfiguration { request: DevSetupPreviewRequest },
    ApplyDevSetupConfiguration { request: DevSetupApplyRequest },
    CancelDevSetupApply {},
    RelatedTools {},
    InstallRelatedTool { request: RelatedToolInstallRequest },
    LaunchRelatedTool { tool_id: String },
    OpenRelatedUrl { url: String },
}
pub const TOOLS_METHODS: &[&str] = &[
    "run_diagnosis",
    "preview_support_bundle",
    "cancel_support_bundle",
    "export_support_bundle",
    "dev_setup_audit",
    "import_dev_setup_configuration",
    "discard_dev_setup_configuration",
    "export_dev_setup_configuration",
    "apply_dev_setup_configuration",
    "cancel_dev_setup_apply",
    "related_tools",
    "install_related_tool",
    "launch_related_tool",
    "open_related_url",
];
impl ToolsCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::RunDiagnosis { .. } => "run_diagnosis",
            Self::PreviewSupportBundle { .. } => "preview_support_bundle",
            Self::CancelSupportBundle { .. } => "cancel_support_bundle",
            Self::ExportSupportBundle { .. } => "export_support_bundle",
            Self::DevSetupAudit { .. } => "dev_setup_audit",
            Self::ImportDevSetupConfiguration { .. } => "import_dev_setup_configuration",
            Self::DiscardDevSetupConfiguration { .. } => "discard_dev_setup_configuration",
            Self::ExportDevSetupConfiguration { .. } => "export_dev_setup_configuration",
            Self::ApplyDevSetupConfiguration { .. } => "apply_dev_setup_configuration",
            Self::CancelDevSetupApply { .. } => "cancel_dev_setup_apply",
            Self::RelatedTools { .. } => "related_tools",
            Self::InstallRelatedTool { .. } => "install_related_tool",
            Self::LaunchRelatedTool { .. } => "launch_related_tool",
            Self::OpenRelatedUrl { .. } => "open_related_url",
        }
    }
    pub fn routes(&self) -> &'static [&'static str] {
        match self {
            Self::RunDiagnosis { .. } => &["diagnostics", "recovery"],
            Self::PreviewSupportBundle { .. } => &["diagnostics", "recovery"],
            Self::CancelSupportBundle { .. } => &["diagnostics", "recovery"],
            Self::ExportSupportBundle { .. } => &["diagnostics", "recovery"],
            Self::DevSetupAudit { .. } => &["environment"],
            Self::ImportDevSetupConfiguration { .. } => &["environment"],
            Self::DiscardDevSetupConfiguration { .. } => &["environment"],
            Self::ExportDevSetupConfiguration { .. } => &["environment"],
            Self::ApplyDevSetupConfiguration { .. } => &["environment"],
            Self::CancelDevSetupApply { .. } => &["environment"],
            Self::RelatedTools { .. } => &["tools"],
            Self::InstallRelatedTool { .. } => &["tools"],
            Self::LaunchRelatedTool { .. } => &["tools"],
            Self::OpenRelatedUrl { .. } => &["tools"],
        }
    }
    pub fn class(&self) -> ExecutionClass {
        match self {
            Self::CancelSupportBundle { .. } | Self::CancelDevSetupApply {} => {
                ExecutionClass::Control
            }
            _ => ExecutionClass::Normal,
        }
    }
}
pub async fn dispatch(app: &tauri::AppHandle, call: ToolsCall) -> Result<Value, String> {
    match call {
        ToolsCall::RunDiagnosis {} => {
            value(crate::commands::doctor::run_diagnosis(app.clone()).await)
        }
        ToolsCall::PreviewSupportBundle { operation_id } => value(
            crate::commands::diagnostics::preview_support_bundle(
                app.clone(),
                app.state(),
                operation_id,
            )
            .await,
        ),
        ToolsCall::CancelSupportBundle { request } => value(
            crate::commands::diagnostics::cancel_support_bundle(app.state(), request),
        ),
        ToolsCall::ExportSupportBundle { preview_id } => value(
            crate::commands::diagnostics::export_support_bundle(app.state(), preview_id).await,
        ),
        ToolsCall::DevSetupAudit {} => {
            value(crate::commands::related_tools::dev_setup_audit().await)
        }
        ToolsCall::ImportDevSetupConfiguration {} => value(
            crate::commands::dev_setup::import_dev_setup_configuration(app.clone(), app.state())
                .await,
        ),
        ToolsCall::DiscardDevSetupConfiguration { request } => value(
            crate::commands::dev_setup::discard_dev_setup_configuration(app.state(), request),
        ),
        ToolsCall::ExportDevSetupConfiguration { request } => value(
            crate::commands::dev_setup::export_dev_setup_configuration(app.state(), request),
        ),
        ToolsCall::ApplyDevSetupConfiguration { request } => value(
            crate::commands::dev_setup::apply_dev_setup_configuration(
                app.clone(),
                app.state(),
                request,
            )
            .await,
        ),
        ToolsCall::CancelDevSetupApply {} => value(
            crate::commands::dev_setup::cancel_dev_setup_apply(app.state()),
        ),
        ToolsCall::RelatedTools {} => value(crate::commands::related_tools::related_tools().await),
        ToolsCall::InstallRelatedTool { request } => {
            value(crate::commands::related_tools::install_related_tool(request).await)
        }
        ToolsCall::LaunchRelatedTool { tool_id } => {
            value(crate::commands::related_tools::launch_related_tool(tool_id).await)
        }
        ToolsCall::OpenRelatedUrl { url } => {
            if !crate::core::related_tools::CURATED_TOOLS
                .iter()
                .any(|tool| tool.official_url == url || tool.license_url == url)
            {
                return Err("manager_url_denied".into());
            }
            use tauri_plugin_opener::OpenerExt;
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|_| "manager_url_unavailable")?;
            Ok(Value::Null)
        }
    }
}
fn value<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
    serde_json::to_value(result?).map_err(|_| "manager_response_invalid".into())
}
product_ipc::issue_codes! {
    pub enum ToolsIssue {
    ManagerArgsInvalid = "manager_args_invalid",
    ManagerMethodDenied = "manager_method_denied",
    ManagerResponseInvalid = "manager_response_invalid",
    ManagerStateConflict = "manager_state_conflict",
    ManagerUrlDenied = "manager_url_denied",
    ManagerUrlUnavailable = "manager_url_unavailable",
    Unavailable = "unavailable",
    }
}
pub fn classify(error: &str) -> &'static str {
    ToolsIssue::from_code(error)
        .unwrap_or(ToolsIssue::Unavailable)
        .code()
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "run_diagnosis",
            export.register::<Vec<crate::commands::doctor::DiagnosisItem>>()?,
        ),
        (
            "preview_support_bundle",
            export.register::<crate::core::support_bundle::SupportBundlePreview>()?,
        ),
        (
            "cancel_support_bundle",
            export.register::<crate::commands::diagnostics::SupportBundleStatus>()?,
        ),
        (
            "export_support_bundle",
            export.register::<crate::core::support_bundle::SupportBundleExport>()?,
        ),
        (
            "dev_setup_audit",
            export.register::<crate::commands::related_tools::DevSetupAuditView>()?,
        ),
        (
            "import_dev_setup_configuration",
            export
                .register::<Option<crate::commands::dev_setup::DevSetupConfigurationReviewView>>(
                )?,
        ),
        ("discard_dev_setup_configuration", export.register::<()>()?),
        (
            "export_dev_setup_configuration",
            export.register::<crate::commands::dev_setup::DevSetupConfigurationExportView>()?,
        ),
        (
            "apply_dev_setup_configuration",
            export.register::<crate::commands::dev_setup::DevSetupApplyView>()?,
        ),
        ("cancel_dev_setup_apply", export.register::<()>()?),
        (
            "related_tools",
            export.register::<Vec<crate::commands::related_tools::RelatedToolView>>()?,
        ),
        (
            "install_related_tool",
            export.register::<crate::commands::related_tools::RelatedToolActionView>()?,
        ),
        (
            "launch_related_tool",
            export.register::<crate::commands::related_tools::RelatedToolActionView>()?,
        ),
        ("open_related_url", export.register::<()>()?),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_tools_preserve_native_routes_and_cancel_priority() {
        let audit: ToolsCall =
            serde_json::from_str(r#"{"method":"dev_setup_audit","args":{}}"#).unwrap();
        assert_eq!(audit.routes(), &["environment"]);
        let cancel: ToolsCall =
            serde_json::from_str(r#"{"method":"cancel_dev_setup_apply","args":{}}"#).unwrap();
        assert_eq!(cancel.class(), ExecutionClass::Control);
        assert_eq!(TOOLS_METHODS.len(), 14);
        assert_eq!(classify("C:/private/credential"), "unavailable");
    }
}
