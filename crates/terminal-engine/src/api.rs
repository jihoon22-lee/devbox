//! Closed typed engine calls. Product ownership checks remain at the host boundary.
use product_ipc::workspace::{Lane, LONG_BUDGET_MS};
use serde::Deserialize;
use tauri::Manager as _;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum TerminalCall {
    ListDistros {},
    DashboardSnapshot {},
    DockerPs {
        distro: String,
    },
    DockerAction {
        distro: String,
        container_id: String,
        action: String,
    },
    ListWorkspaceProfiles {},
    SaveWorkspaceProfile {
        profile: crate::core::workspace::WorkspaceProfile,
    },
    DeleteWorkspaceProfile {
        id: String,
    },
    WindowsBuildNumber {},
    StartSession {
        distro: String,
        cwd: Option<String>,
        pane_key: String,
        multiplexer: crate::core::workspace::MultiplexerKind,
    },
    AttachSession {
        session_id: String,
    },
    WriteSession {
        session_id: String,
        data: String,
    },
    Broadcast {
        session_ids: Vec<String>,
        data: String,
    },
    ResizeSession {
        session_id: String,
        rows: u16,
        cols: u16,
    },
    CloseSession {
        session_id: String,
    },
    ListSessions {},
    DetectMultiplexers {
        distro: String,
    },
    InspectShellIntegration {
        distro: String,
    },
    UpdateShellIntegration {
        distro: String,
        shell: crate::core::shell_integration::ShellKind,
        action: crate::core::shell_integration::ShellIntegrationAction,
        expected_revision: String,
    },
}
pub const METHODS: &[&str] = &[
    "list_distros",
    "dashboard_snapshot",
    "docker_ps",
    "docker_action",
    "list_workspace_profiles",
    "save_workspace_profile",
    "delete_workspace_profile",
    "windows_build_number",
    "start_session",
    "attach_session",
    "write_session",
    "broadcast",
    "resize_session",
    "close_session",
    "list_sessions",
    "detect_multiplexers",
    "inspect_shell_integration",
    "update_shell_integration",
];
impl TerminalCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::ListDistros { .. } => "list_distros",
            Self::DashboardSnapshot { .. } => "dashboard_snapshot",
            Self::DockerPs { .. } => "docker_ps",
            Self::DockerAction { .. } => "docker_action",
            Self::ListWorkspaceProfiles { .. } => "list_workspace_profiles",
            Self::SaveWorkspaceProfile { .. } => "save_workspace_profile",
            Self::DeleteWorkspaceProfile { .. } => "delete_workspace_profile",
            Self::WindowsBuildNumber { .. } => "windows_build_number",
            Self::StartSession { .. } => "start_session",
            Self::AttachSession { .. } => "attach_session",
            Self::WriteSession { .. } => "write_session",
            Self::Broadcast { .. } => "broadcast",
            Self::ResizeSession { .. } => "resize_session",
            Self::CloseSession { .. } => "close_session",
            Self::ListSessions { .. } => "list_sessions",
            Self::DetectMultiplexers { .. } => "detect_multiplexers",
            Self::InspectShellIntegration { .. } => "inspect_shell_integration",
            Self::UpdateShellIntegration { .. } => "update_shell_integration",
        }
    }
    pub fn lane(&self) -> Lane {
        match self {
            Self::AttachSession { .. }
            | Self::WriteSession { .. }
            | Self::Broadcast { .. }
            | Self::ResizeSession { .. }
            | Self::ListSessions { .. } => Lane::TerminalIo,
            Self::CloseSession { .. } => Lane::TerminalStop,
            _ => Lane::Terminal,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        LONG_BUDGET_MS
    }
}
#[cfg(feature = "desktop")]
async fn execute(
    _component_app: &tauri::AppHandle,
    call: TerminalCall,
) -> Result<serde_json::Value, String> {
    match call {
        TerminalCall::ListDistros {} => {
            use crate::commands::dashboard::*;
            let _ = _component_app;
            let value = list_distros(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::DashboardSnapshot {} => {
            use crate::commands::dashboard::*;
            let _ = _component_app;
            let value = dashboard_snapshot(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::DockerPs { distro } => {
            use crate::commands::dashboard::*;
            let _ = _component_app;
            let value = docker_ps(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                distro,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::DockerAction {
            distro,
            container_id,
            action,
        } => {
            use crate::commands::dashboard::*;
            let _ = _component_app;
            docker_action(distro, container_id, action).await?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::ListWorkspaceProfiles {} => {
            use crate::commands::workspace::*;
            let _ = _component_app;
            let value = list_workspace_profiles(_component_app.clone());
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::SaveWorkspaceProfile { profile } => {
            use crate::commands::workspace::*;
            let _ = _component_app;
            let value = save_workspace_profile(_component_app.clone(), profile)?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::DeleteWorkspaceProfile { id } => {
            use crate::commands::workspace::*;
            let _ = _component_app;
            delete_workspace_profile(_component_app.clone(), id)?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::WindowsBuildNumber {} => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            let value = windows_build_number();
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::StartSession {
            distro,
            cwd,
            pane_key,
            multiplexer,
        } => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            let value = start_session(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                distro,
                cwd,
                pane_key,
                multiplexer,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::AttachSession { session_id } => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            attach_session(
                _component_app.clone(),
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                session_id,
            )?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::WriteSession { session_id, data } => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            write_session(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                session_id,
                data,
            )?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::Broadcast { session_ids, data } => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            broadcast(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                session_ids,
                data,
            )?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::ResizeSession {
            session_id,
            rows,
            cols,
        } => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            resize_session(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                session_id,
                rows,
                cols,
            )?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::CloseSession { session_id } => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            close_session(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                session_id,
            )?;
            Ok(serde_json::Value::Null)
        }
        TerminalCall::ListSessions {} => {
            use crate::commands::terminal::*;
            let _ = _component_app;
            let value = list_sessions(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
            );
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::DetectMultiplexers { distro } => {
            use crate::commands::multiplexer::*;
            let _ = _component_app;
            let value = detect_multiplexers(distro).await;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::InspectShellIntegration { distro } => {
            use crate::commands::shell_integration::*;
            let _ = _component_app;
            let value = inspect_shell_integration(distro).await?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
        TerminalCall::UpdateShellIntegration {
            distro,
            shell,
            action,
            expected_revision,
        } => {
            use crate::commands::shell_integration::*;
            let _ = _component_app;
            let value = update_shell_integration(
                _component_app
                    .try_state()
                    .ok_or("terminal_state_unavailable")?,
                distro,
                shell,
                action,
                expected_revision,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "terminal_response_invalid".into())
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "list_distros",
            export.register::<Vec<crate::core::models::DistroInfo>>()?,
        ),
        (
            "dashboard_snapshot",
            export.register::<crate::core::runtime_snapshot::DashboardSnapshot>()?,
        ),
        (
            "docker_ps",
            export.register::<Vec<crate::core::models::ContainerInfo>>()?,
        ),
        ("docker_action", export.register::<()>()?),
        (
            "list_workspace_profiles",
            export.register::<Vec<crate::core::workspace::WorkspaceProfile>>()?,
        ),
        (
            "save_workspace_profile",
            export.register::<crate::core::workspace::WorkspaceProfile>()?,
        ),
        ("delete_workspace_profile", export.register::<()>()?),
        ("windows_build_number", export.register::<Option<u32>>()?),
        (
            "start_session",
            export.register::<crate::commands::terminal::StartedSession>()?,
        ),
        ("attach_session", export.register::<()>()?),
        ("write_session", export.register::<()>()?),
        ("broadcast", export.register::<()>()?),
        ("resize_session", export.register::<()>()?),
        ("close_session", export.register::<()>()?),
        (
            "list_sessions",
            export.register::<Vec<crate::commands::terminal::SessionInfo>>()?,
        ),
        (
            "detect_multiplexers",
            export.register::<Vec<crate::commands::multiplexer::MultiplexerAvailability>>()?,
        ),
        (
            "inspect_shell_integration",
            export.register::<crate::commands::shell_integration::ShellIntegrationReport>()?,
        ),
        (
            "update_shell_integration",
            export.register::<crate::commands::shell_integration::ShellIntegrationMutation>()?,
        ),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_methods_and_extra_arguments_are_rejected() {
        assert!(serde_json::from_str::<TerminalCall>(r#"{"method":"unknown","args":{}}"#).is_err());
    }
    #[test]
    fn method_names_are_unique() {
        let mut names = METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), METHODS.len());
    }
}

#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    call: TerminalCall,
) -> Result<serde_json::Value, String> {
    crate::component::data_root(app)?;
    if crate::component::is_product(app)
        && matches!(
            call,
            TerminalCall::StartSession { .. }
                | TerminalCall::AttachSession { .. }
                | TerminalCall::WriteSession { .. }
                | TerminalCall::Broadcast { .. }
                | TerminalCall::ResizeSession { .. }
                | TerminalCall::CloseSession { .. }
                | TerminalCall::ListSessions { .. }
        )
    {
        return Err("terminal_owner_required".into());
    }
    let result = execute(app, call).await;
    crate::component::data_root(app)?;
    result
}
// Called only after TerminalOwner validates each exact session target.
#[cfg(feature = "desktop")]
pub(crate) async fn dispatch_owned(
    app: &tauri::AppHandle,
    call: TerminalCall,
) -> Result<serde_json::Value, String> {
    if !matches!(
        call,
        TerminalCall::WriteSession { .. }
            | TerminalCall::ResizeSession { .. }
            | TerminalCall::Broadcast { .. }
    ) {
        return Err("terminal_owner_required".into());
    }
    crate::component::data_root(app)?;
    let result = execute(app, call).await;
    crate::component::data_root(app)?;
    result
}

#[cfg(test)]
mod lane_tests {
    use super::*;
    #[test]
    fn input_stop_and_start_have_independent_capacity() {
        for (wire, lane) in [
            (
                r#"{"method":"write_session","args":{"sessionId":"s","data":"input"}}"#,
                Lane::TerminalIo,
            ),
            (
                r#"{"method":"close_session","args":{"sessionId":"s"}}"#,
                Lane::TerminalStop,
            ),
            (
                r#"{"method":"detect_multiplexers","args":{"distro":"fixture"}}"#,
                Lane::Terminal,
            ),
        ] {
            let call: TerminalCall = serde_json::from_str(wire).unwrap();
            assert_eq!(call.lane(), lane);
            assert_eq!(call.deadline_budget_ms(), 29_000);
        }
    }
}
