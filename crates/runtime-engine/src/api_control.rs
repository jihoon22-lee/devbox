//! Native durable control input. Public RuntimeCall exposes this only inside runtime_control.
use serde::{Deserialize, Serialize};
use tauri::Manager as _;
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub operation_id: String,
    #[serde(flatten)]
    pub action: ControlAction,
}
#[derive(Deserialize, Serialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ControlAction {
    RunJobNow { id: String },
    StopActiveRun { id: String },
    StartService { id: String },
    StopService { id: String },
    RestartService { id: String },
    RunWorkspaceTaskOperation { id: String, fail_fast: bool },
    StopWorkspaceTaskOperation { operation_id: String },
}
impl ControlAction {
    pub fn method(&self) -> &'static str {
        match self {
            Self::RunJobNow { .. } => "run_job_now",
            Self::StopActiveRun { .. } => "stop_active_run",
            Self::StartService { .. } => "start_service",
            Self::StopService { .. } => "stop_service",
            Self::RestartService { .. } => "restart_service",
            Self::RunWorkspaceTaskOperation { .. } => "run_workspace_task_operation",
            Self::StopWorkspaceTaskOperation { .. } => "stop_workspace_task_operation",
        }
    }
    pub fn lane(&self) -> product_ipc::workspace::Lane {
        match self {
            Self::StopActiveRun { .. }
            | Self::StopService { .. }
            | Self::StopWorkspaceTaskOperation { .. } => product_ipc::workspace::Lane::EngineStop,
            _ => product_ipc::workspace::Lane::Engine,
        }
    }
    pub(crate) async fn execute(
        self,
        _component_app: &tauri::AppHandle,
    ) -> Result<serde_json::Value, String> {
        use crate::commands::*;
        match self {
            Self::RunJobNow { id } => {
                let value = run_job_now(
                    id,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )
                .await?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
            Self::StopActiveRun { id } => {
                let value = stop_active_run(
                    id,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )
                .await?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
            Self::StartService { id } => {
                let value = start_service(
                    id,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )
                .await?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
            Self::StopService { id } => {
                let value = stop_service(
                    id,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )
                .await?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
            Self::RestartService { id } => {
                let value = restart_service(
                    id,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )
                .await?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
            Self::RunWorkspaceTaskOperation { id, fail_fast } => {
                let value = run_workspace_task_operation(
                    id,
                    fail_fast,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
            Self::StopWorkspaceTaskOperation { operation_id } => {
                let value = stop_workspace_task_operation(
                    operation_id,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                    _component_app
                        .try_state()
                        .ok_or("component_state_unavailable")?,
                )
                .await?;
                serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
            }
        }
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<ControlAction>()?;
    Ok(vec![
        (
            "run_job_now",
            export.register::<crate::core::models::RunView>()?,
        ),
        (
            "stop_active_run",
            export.register::<Option<crate::core::models::RunView>>()?,
        ),
        (
            "start_service",
            export.register::<crate::core::models::ServiceInstanceView>()?,
        ),
        (
            "stop_service",
            export.register::<Option<crate::core::models::ServiceInstanceView>>()?,
        ),
        (
            "restart_service",
            export.register::<crate::core::models::ServiceInstanceView>()?,
        ),
        (
            "run_workspace_task_operation",
            export
                .register::<crate::core::workspace_orchestration::WorkspaceTaskOperationView>()?,
        ),
        (
            "stop_workspace_task_operation",
            export
                .register::<crate::core::workspace_orchestration::WorkspaceTaskOperationView>()?,
        ),
    ])
}
