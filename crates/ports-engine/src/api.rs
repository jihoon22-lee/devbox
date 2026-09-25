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
pub enum PortsCall {
    ListPortObservations {},
    OpenPortOwner {
        action_key: String,
    },
    OpenPortLog {
        action_key: String,
        stream: devbox_applink::LogSourceStream,
    },
    ListPorts {},
    KillListener {
        request: crate::core::listeners::KillListenerRequest,
    },
    HandoffContainerStop {
        request: crate::core::listeners::KillListenerRequest,
    },
    GetProcessInfo {
        pid: u32,
    },
    RevealProcess {
        pid: u32,
    },
    OpenBrowser {
        url: String,
    },
    LoadPortManagerPreferences {},
    SavePortManagerPreferences {
        preferences: crate::core::preferences::PortManagerPreferences,
    },
}
pub const METHODS: &[&str] = &[
    "list_port_observations",
    "open_port_owner",
    "open_port_log",
    "list_ports",
    "kill_listener",
    "handoff_container_stop",
    "get_process_info",
    "reveal_process",
    "open_browser",
    "load_port_manager_preferences",
    "save_port_manager_preferences",
];
impl PortsCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::ListPortObservations { .. } => "list_port_observations",
            Self::OpenPortOwner { .. } => "open_port_owner",
            Self::OpenPortLog { .. } => "open_port_log",
            Self::ListPorts { .. } => "list_ports",
            Self::KillListener { .. } => "kill_listener",
            Self::HandoffContainerStop { .. } => "handoff_container_stop",
            Self::GetProcessInfo { .. } => "get_process_info",
            Self::RevealProcess { .. } => "reveal_process",
            Self::OpenBrowser { .. } => "open_browser",
            Self::LoadPortManagerPreferences { .. } => "load_port_manager_preferences",
            Self::SavePortManagerPreferences { .. } => "save_port_manager_preferences",
        }
    }
    pub fn lane(&self) -> Lane {
        Lane::Engine
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        LONG_BUDGET_MS
    }
}
#[cfg(feature = "desktop")]
async fn execute(
    _component_app: &tauri::AppHandle,
    call: PortsCall,
) -> Result<serde_json::Value, String> {
    match call {
        PortsCall::ListPortObservations {} => {
            use crate::commands::correlation::*;
            let value = list_port_observations().await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::OpenPortOwner { action_key } => {
            use crate::commands::correlation::*;
            open_port_owner(action_key).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::OpenPortLog { action_key, stream } => {
            use crate::commands::correlation::*;
            let value = open_port_log(action_key, stream).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::ListPorts {} => {
            use crate::commands::ports::*;
            let value = list_ports().await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::KillListener { request } => {
            use crate::commands::ports::*;
            let value = kill_listener(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::HandoffContainerStop { request } => {
            use crate::commands::ports::*;
            let value = handoff_container_stop(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::GetProcessInfo { pid } => {
            use crate::commands::ports::*;
            let value = get_process_info(pid)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::RevealProcess { pid } => {
            use crate::commands::ports::*;
            reveal_process(_component_app.clone(), pid).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::OpenBrowser { url } => {
            use crate::commands::ports::*;
            open_browser(_component_app.clone(), url).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        PortsCall::LoadPortManagerPreferences {} => serde_json::to_value(
            crate::core::product_preferences::load(&crate::component::data_root(_component_app)?)
                .map_err(str::to_owned)?
                .preferences,
        )
        .map_err(|_| "component_response_invalid".into()),
        PortsCall::SavePortManagerPreferences { preferences } => {
            crate::core::product_preferences::save(
                &crate::component::data_root(_component_app)?,
                preferences,
            )
            .map_err(str::to_owned)?;
            Ok(serde_json::Value::Null)
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "list_port_observations",
            export.register::<crate::commands::correlation::PortObservationSnapshot>()?,
        ),
        ("open_port_owner", export.register::<()>()?),
        (
            "open_port_log",
            export.register::<crate::commands::correlation::LogLensDispatch>()?,
        ),
        (
            "list_ports",
            export.register::<Vec<crate::commands::ports::PortRow>>()?,
        ),
        (
            "kill_listener",
            export.register::<crate::commands::ports::ListenerActionResult>()?,
        ),
        (
            "handoff_container_stop",
            export.register::<crate::core::listeners::ContainerStopHandoff>()?,
        ),
        (
            "get_process_info",
            export.register::<crate::commands::ports::ProcessInfo>()?,
        ),
        ("reveal_process", export.register::<()>()?),
        ("open_browser", export.register::<()>()?),
        (
            "load_port_manager_preferences",
            export.register::<crate::core::preferences::PortManagerPreferences>()?,
        ),
        ("save_port_manager_preferences", export.register::<()>()?),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_methods_and_extra_arguments_are_rejected() {
        assert!(serde_json::from_str::<PortsCall>(r#"{"method":"unknown","args":{}}"#).is_err());
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
    call: PortsCall,
) -> Result<serde_json::Value, String> {
    crate::component::data_root(app)?;
    let result = execute(app, call).await;
    crate::component::data_root(app)?;
    result
}
