use product_ipc::workspace::{Lane, LONG_BUDGET_MS};
use product_ipc::{ComponentCall, ExecutionClass};
use serde::Deserialize;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WorkspaceTerminalCall {
    DashboardSnapshot {},
    DockerAction {
        operation_id: String,
        distro: String,
        container_id: String,
        action: String,
    },
    WslControlStatus {
        operation_id: String,
    },
    OpenDistroTerminal {
        operation_id: String,
        distro: String,
    },
    OpenWslFileInLogLens {
        distro: String,
        wsl_path: String,
    },
    OpenWslJournalInLogLens {
        distro: String,
        unit: Option<String>,
    },
    TerminalSessions {},
    TerminalCommands {},
    SummonTerminal {
        operation_id: String,
        terminal_id: String,
        deadline_ms: u64,
    },
    OpenTerminalProfile {
        operation_id: String,
        profile_id: String,
        revision: String,
    },
    RestoreTerminal {
        id: String,
        operation_id: String,
        expected_generation: u64,
    },
    ReadTerminalLog {},
    AckTerminalLog {
        id: String,
    },
    OpenTerminal {
        operation_id: String,
    },
    FocusTerminal {
        id: String,
    },
    StopTerminal {
        id: String,
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
    DevelopmentCandidates {},
    DevelopmentSessions {},
    ArchiveDevelopmentSession {
        id: String,
    },
    PrepareSessionSummary {
        operation_id: String,
        session_id: String,
        revision: u64,
        #[serde(default)]
        #[ts(as = "Option<bool>", optional)]
        include_problems: bool,
    },
    PrepareDevelopmentSession {
        operation_id: String,
        jobs: Vec<String>,
        terminal_profile: Option<String>,
    },
    StartDevelopmentSession {
        id: String,
        revision: u64,
        plan_revision: String,
        mode: crate::core::development_sessions::Mode,
    },
    StopDevelopmentSession {
        id: String,
    },
}
pub const METHODS: &[&str] = &[
    "ack_terminal_log",
    "archive_development_session",
    "dashboard_snapshot",
    "delete_workspace_profile",
    "development_candidates",
    "development_sessions",
    "docker_action",
    "focus_terminal",
    "list_workspace_profiles",
    "open_distro_terminal",
    "open_terminal",
    "open_terminal_profile",
    "open_wsl_file_in_log_lens",
    "open_wsl_journal_in_log_lens",
    "prepare_development_session",
    "prepare_session_summary",
    "read_terminal_log",
    "restore_terminal",
    "save_workspace_profile",
    "start_development_session",
    "stop_development_session",
    "stop_terminal",
    "summon_terminal",
    "terminal_commands",
    "terminal_sessions",
    "wsl_control_status",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "ack_terminal_log" => &["terminal"],
        "archive_development_session" => &["terminal"],
        "dashboard_snapshot" => &["runtime", "terminal"],
        "delete_workspace_profile" => &["terminal"],
        "development_candidates" => &["terminal"],
        "development_sessions" => &["terminal"],
        "docker_action" => &["runtime", "terminal"],
        "focus_terminal" => &["terminal"],
        "list_workspace_profiles" => &["terminal"],
        "open_distro_terminal" => &["runtime", "terminal"],
        "open_terminal" => &["terminal"],
        "open_terminal_profile" => &["terminal"],
        "open_wsl_file_in_log_lens" => &["runtime", "terminal"],
        "open_wsl_journal_in_log_lens" => &["runtime", "terminal"],
        "prepare_development_session" => &["terminal"],
        "prepare_session_summary" => &["terminal"],
        "read_terminal_log" => &["terminal"],
        "restore_terminal" => &["terminal"],
        "save_workspace_profile" => &["terminal"],
        "start_development_session" => &["terminal"],
        "stop_development_session" => &["terminal"],
        "stop_terminal" => &["terminal"],
        "summon_terminal" => &["terminal"],
        "terminal_commands" => &["terminal"],
        "terminal_sessions" => &["terminal"],
        "wsl_control_status" => &["runtime", "terminal"],
        _ => &[],
    }
}
impl ComponentCall for WorkspaceTerminalCall {
    const COMPONENT: &'static str = "workspace.terminal";
    const MAX_ARGUMENT_BYTES: usize = 65536;
    const SHARED_REQUEST_LIMIT: bool = false;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::DashboardSnapshot { .. } => "dashboard_snapshot",
            Self::DockerAction { .. } => "docker_action",
            Self::WslControlStatus { .. } => "wsl_control_status",
            Self::OpenDistroTerminal { .. } => "open_distro_terminal",
            Self::OpenWslFileInLogLens { .. } => "open_wsl_file_in_log_lens",
            Self::OpenWslJournalInLogLens { .. } => "open_wsl_journal_in_log_lens",
            Self::TerminalSessions { .. } => "terminal_sessions",
            Self::TerminalCommands { .. } => "terminal_commands",
            Self::SummonTerminal { .. } => "summon_terminal",
            Self::OpenTerminalProfile { .. } => "open_terminal_profile",
            Self::RestoreTerminal { .. } => "restore_terminal",
            Self::ReadTerminalLog { .. } => "read_terminal_log",
            Self::AckTerminalLog { .. } => "ack_terminal_log",
            Self::OpenTerminal { .. } => "open_terminal",
            Self::FocusTerminal { .. } => "focus_terminal",
            Self::StopTerminal { .. } => "stop_terminal",
            Self::ListWorkspaceProfiles { .. } => "list_workspace_profiles",
            Self::SaveWorkspaceProfile { .. } => "save_workspace_profile",
            Self::DeleteWorkspaceProfile { .. } => "delete_workspace_profile",
            Self::DevelopmentCandidates { .. } => "development_candidates",
            Self::DevelopmentSessions { .. } => "development_sessions",
            Self::ArchiveDevelopmentSession { .. } => "archive_development_session",
            Self::PrepareSessionSummary { .. } => "prepare_session_summary",
            Self::PrepareDevelopmentSession { .. } => "prepare_development_session",
            Self::StartDevelopmentSession { .. } => "start_development_session",
            Self::StopDevelopmentSession { .. } => "stop_development_session",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
    fn class(&self) -> ExecutionClass {
        if matches!(self.lane(), Lane::EngineStop | Lane::TerminalStop) {
            ExecutionClass::Control
        } else {
            ExecutionClass::Normal
        }
    }
}
impl WorkspaceTerminalCall {
    pub fn lane(&self) -> Lane {
        match self {
            Self::StopTerminal { .. } | Self::StopDevelopmentSession { .. } => Lane::TerminalStop,
            _ => Lane::Terminal,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
#[tauri::command]
pub(crate) async fn terminal(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceTerminalCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceTerminalCall {
    fn into_call(self) -> super::Call {
        super::Call::Terminal(self)
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    use super::results::*;
    export.register::<WorkspaceTerminalCall>()?;
    let mut results = Vec::new();
    results.retain(|(method, _)| METHODS.contains(method));
    results.retain(|(method, _)| *method != "dashboard_snapshot");
    results.push((
        "dashboard_snapshot",
        export.register::<terminal_engine::component::DashboardSnapshot>()?,
    ));
    results.retain(|(method, _)| *method != "docker_action");
    results.push(("docker_action", export.register::<serde_json::Value>()?));
    results.retain(|(method, _)| *method != "wsl_control_status");
    results.push(("wsl_control_status", export.register::<WslControlStatus>()?));
    results.retain(|(method, _)| *method != "open_distro_terminal");
    results.push((
        "open_distro_terminal",
        export.register::<crate::terminal_host::Record>()?,
    ));
    results.retain(|(method, _)| *method != "open_wsl_file_in_log_lens");
    results.push(("open_wsl_file_in_log_lens", export.register::<()>()?));
    results.retain(|(method, _)| *method != "open_wsl_journal_in_log_lens");
    results.push(("open_wsl_journal_in_log_lens", export.register::<()>()?));
    results.retain(|(method, _)| *method != "terminal_sessions");
    results.push((
        "terminal_sessions",
        export.register::<Vec<crate::terminal_host::Record>>()?,
    ));
    results.retain(|(method, _)| *method != "terminal_commands");
    results.push(("terminal_commands", export.register::<TerminalCommands>()?));
    results.retain(|(method, _)| *method != "summon_terminal");
    results.push(("summon_terminal", export.register::<TerminalSummon>()?));
    results.retain(|(method, _)| *method != "open_terminal_profile");
    results.push((
        "open_terminal_profile",
        export.register::<crate::terminal_host::Record>()?,
    ));
    results.retain(|(method, _)| *method != "restore_terminal");
    results.push((
        "restore_terminal",
        export.register::<crate::terminal_host::Record>()?,
    ));
    results.retain(|(method, _)| *method != "read_terminal_log");
    results.push((
        "read_terminal_log",
        export.register::<Option<crate::terminal_host::PendingLog>>()?,
    ));
    results.retain(|(method, _)| *method != "ack_terminal_log");
    results.push(("ack_terminal_log", export.register::<()>()?));
    results.retain(|(method, _)| *method != "open_terminal");
    results.push((
        "open_terminal",
        export.register::<crate::terminal_host::Record>()?,
    ));
    results.retain(|(method, _)| *method != "focus_terminal");
    results.push(("focus_terminal", export.register::<()>()?));
    results.retain(|(method, _)| *method != "stop_terminal");
    results.push(("stop_terminal", export.register::<()>()?));
    results.retain(|(method, _)| *method != "list_workspace_profiles");
    results.push((
        "list_workspace_profiles",
        export.register::<TerminalProfiles>()?,
    ));
    results.retain(|(method, _)| *method != "save_workspace_profile");
    results.push((
        "save_workspace_profile",
        export.register::<SavedTerminalProfile>()?,
    ));
    results.retain(|(method, _)| *method != "delete_workspace_profile");
    results.push((
        "delete_workspace_profile",
        export.register::<DeletedTerminalProfile>()?,
    ));
    results.retain(|(method, _)| *method != "development_candidates");
    results.push((
        "development_candidates",
        export.register::<DevelopmentCandidates>()?,
    ));
    results.retain(|(method, _)| *method != "development_sessions");
    results.push((
        "development_sessions",
        export.register::<DevelopmentSessions>()?,
    ));
    results.retain(|(method, _)| *method != "archive_development_session");
    results.push(("archive_development_session", export.register::<()>()?));
    results.retain(|(method, _)| *method != "prepare_session_summary");
    results.push((
        "prepare_session_summary",
        export.register::<SessionSummaryPreview>()?,
    ));
    results.retain(|(method, _)| *method != "prepare_development_session");
    results.push((
        "prepare_development_session",
        export.register::<DevelopmentPlan>()?,
    ));
    results.retain(|(method, _)| *method != "start_development_session");
    results.push((
        "start_development_session",
        export.register::<crate::core::development_sessions::Session>()?,
    ));
    results.retain(|(method, _)| *method != "stop_development_session");
    results.push((
        "stop_development_session",
        export.register::<crate::core::development_sessions::Session>()?,
    ));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
pub const fn deadline_budget_for(_method: &str) -> u64 {
    LONG_BUDGET_MS
}
