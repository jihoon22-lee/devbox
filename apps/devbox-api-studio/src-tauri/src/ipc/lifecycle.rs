use serde::Deserialize;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LifecycleCall {
    LifecycleStatus {},
    SetClosePolicy {
        policy: crate::core::lifecycle::ClosePolicy,
    },
    HideMainWindow {},
    QuitProduct {},
}
impl LifecycleCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::LifecycleStatus {} => "lifecycle_status",
            Self::SetClosePolicy { .. } => "set_close_policy",
            Self::HideMainWindow {} => "hide_main_window",
            Self::QuitProduct {} => "quit_product",
        }
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleStatus {
    pub main_window_visible: Option<bool>,
    pub policy: crate::core::lifecycle::ClosePolicy,
    pub tray_available: bool,
    pub running: bool,
    pub closing: bool,
    pub stop_failed: bool,
    pub settings_writable: bool,
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        ("lifecycle_status", export.register::<LifecycleStatus>()?),
        ("set_close_policy", export.register::<()>()?),
        ("hide_main_window", export.register::<()>()?),
        ("quit_product", export.register::<()>()?),
    ])
}
