//! Companion calls retain the native peer/session authority, independent of main routes.
use product_ipc::workspace::Lane;
use product_ipc::ComponentCall;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum CompanionCall {
    Host(CompanionHost),
    Engine(Box<terminal_engine::api::TerminalCall>),
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum CompanionHost {
    TerminalWindowPolicy {},
    TerminalPreferences {},
    SetTerminalPreference {
        key: String,
        expected: Option<String>,
        value: String,
    },
    TerminalLayout {},
    SaveTerminalLayout {
        expected_revision: String,
        layout: Option<terminal_engine::component::WorkspaceProfile>,
    },
    ResetFailedPane {
        pane_key: String,
    },
    TerminalOutput {
        session_id: String,
        after: u64,
    },
    WriteInitialCommand {
        session_id: String,
        data: String,
    },
    DockerAction {
        operation_id: String,
        distro: String,
        container_id: String,
        action: String,
    },
    WslControlStatus {
        operation_id: String,
    },
    OpenWslFileInLogLens {
        distro: String,
        wsl_path: String,
    },
    OpenWslJournalInLogLens {
        distro: String,
        unit: Option<String>,
    },
    ListWorkspaceProfiles {},
    SaveWorkspaceProfile {
        expected_revision: String,
        profile: terminal_engine::component::WorkspaceProfile,
    },
    DeleteWorkspaceProfile {
        expected_revision: String,
        id: String,
    },
}
impl CompanionHost {
    pub fn method(&self) -> &'static str {
        match self {
            Self::TerminalWindowPolicy { .. } => "terminal_window_policy",
            Self::TerminalPreferences { .. } => "terminal_preferences",
            Self::SetTerminalPreference { .. } => "set_terminal_preference",
            Self::TerminalLayout { .. } => "terminal_layout",
            Self::SaveTerminalLayout { .. } => "save_terminal_layout",
            Self::ResetFailedPane { .. } => "reset_failed_pane",
            Self::TerminalOutput { .. } => "terminal_output",
            Self::WriteInitialCommand { .. } => "write_initial_command",
            Self::DockerAction { .. } => "docker_action",
            Self::WslControlStatus { .. } => "wsl_control_status",
            Self::OpenWslFileInLogLens { .. } => "open_wsl_file_in_log_lens",
            Self::OpenWslJournalInLogLens { .. } => "open_wsl_journal_in_log_lens",
            Self::ListWorkspaceProfiles { .. } => "list_workspace_profiles",
            Self::SaveWorkspaceProfile { .. } => "save_workspace_profile",
            Self::DeleteWorkspaceProfile { .. } => "delete_workspace_profile",
        }
    }
}
impl ComponentCall for CompanionCall {
    const COMPONENT: &'static str = "workspace.terminal";
    const MAX_ARGUMENT_BYTES: usize = 2 * 1024 * 1024;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["terminal"]
    }
}
impl CompanionCall {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Engine(call) => call.lane(),
            Self::Host(
                CompanionHost::WriteInitialCommand { .. } | CompanionHost::TerminalOutput { .. },
            ) => Lane::TerminalIo,
            _ => Lane::Terminal,
        }
    }
}
pub const fn deadline_budget_for(method: &str) -> u64 {
    if matches_output(method) {
        1000
    } else {
        30_000
    }
}
const fn matches_output(method: &str) -> bool {
    let bytes = method.as_bytes();
    let expected = b"terminal_output";
    if bytes.len() != expected.len() {
        return false;
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != expected[i] {
            return false;
        };
        i += 1;
    }
    true
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWindowPolicy {
    pub shortcut_registered: bool,
    pub active_shortcut: (),
    pub tray_enabled: bool,
    pub close_behavior: CloseBehavior,
    pub issues: Vec<String>,
    pub visible: bool,
    pub focused: bool,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum CloseBehavior {
    HideToTray,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalLayout {
    pub revision: String,
    pub layout: Option<terminal_engine::component::WorkspaceProfile>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct OwnedTerminalSession {
    pub id: String,
    pub distro: String,
    pub pane_key: String,
    pub multiplexer: terminal_engine::core::workspace::MultiplexerKind,
    pub resumed: bool,
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<CompanionCall>()?;
    let mut results = terminal_engine::api::result_types(export)?;
    results.retain(|(name, _)| *name != "terminal_window_policy");
    results.push((
        "terminal_window_policy",
        export.register::<TerminalWindowPolicy>()?,
    ));
    results.retain(|(name, _)| *name != "terminal_preferences");
    results.push((
        "terminal_preferences",
        export.register::<std::collections::BTreeMap<String, String>>()?,
    ));
    results.retain(|(name, _)| *name != "set_terminal_preference");
    results.push(("set_terminal_preference", export.register::<()>()?));
    results.retain(|(name, _)| *name != "terminal_layout");
    results.push(("terminal_layout", export.register::<TerminalLayout>()?));
    results.retain(|(name, _)| *name != "save_terminal_layout");
    results.push(("save_terminal_layout", export.register::<TerminalLayout>()?));
    results.retain(|(name, _)| *name != "reset_failed_pane");
    results.push(("reset_failed_pane", export.register::<()>()?));
    results.retain(|(name, _)| *name != "terminal_output");
    results.push((
        "terminal_output",
        export.register::<terminal_engine::core::terminal_output::OutputBatch>()?,
    ));
    results.retain(|(name, _)| *name != "write_initial_command");
    results.push(("write_initial_command", export.register::<()>()?));
    results.retain(|(name, _)| *name != "docker_action");
    results.push(("docker_action", export.register::<()>()?));
    results.retain(|(name, _)| *name != "wsl_control_status");
    results.push((
        "wsl_control_status",
        export.register::<super::results::WslControlStatus>()?,
    ));
    results.retain(|(name, _)| *name != "open_wsl_file_in_log_lens");
    results.push(("open_wsl_file_in_log_lens", export.register::<()>()?));
    results.retain(|(name, _)| *name != "open_wsl_journal_in_log_lens");
    results.push(("open_wsl_journal_in_log_lens", export.register::<()>()?));
    results.retain(|(name, _)| *name != "list_workspace_profiles");
    results.push((
        "list_workspace_profiles",
        export.register::<super::results::TerminalProfiles>()?,
    ));
    results.retain(|(name, _)| *name != "save_workspace_profile");
    results.push((
        "save_workspace_profile",
        export.register::<super::results::SavedTerminalProfile>()?,
    ));
    results.retain(|(name, _)| *name != "delete_workspace_profile");
    results.push((
        "delete_workspace_profile",
        export.register::<super::results::DeletedTerminalProfile>()?,
    ));
    results.retain(|(name, _)| *name != "list_sessions");
    results.push((
        "list_sessions",
        export.register::<Vec<OwnedTerminalSession>>()?,
    ));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::workspace::Lane;
    use product_ipc::ComponentCall;
    #[test]
    fn companion_io_is_typed_without_granting_main_window_factory_commands() {
        let write: CompanionCall = serde_json::from_str(
            r#"{"method":"write_session","args":{"sessionId":"s","data":"text"}}"#,
        )
        .unwrap();
        assert_eq!(write.lane(), Lane::TerminalIo);
        assert_eq!(write.method(), "write_session");
        assert!(serde_json::from_str::<CompanionCall>(
            r#"{"method":"open_terminal","args":{"operationId":"x"}}"#
        )
        .is_err());
        assert_eq!(deadline_budget_for("terminal_output"), 1000);
    }
}

#[derive(Serialize, ts_rs::TS)]
pub struct SavedTerminalLayout {
    pub revision: String,
}
