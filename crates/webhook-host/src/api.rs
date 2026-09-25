//! Typed engine API; session ownership and admission remain in the product host.
use product_ipc::ExecutionClass;
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
pub enum WebhookCall {
    ServerStatus {},
    StartServer {
        bind: Option<String>,
        port: u16,
        allow_lan: Option<bool>,
    },
    StopServer {},
    ListHistory {},
    ClearHistory {},
    CopyMaskedHistory {
        id: u64,
    },
    CopyRawHistory {
        id: u64,
    },
    CopyHistoryHeaders {
        id: u64,
    },
    DeleteHistory {
        id: u64,
    },
    ReplayHistory {
        history_id: u64,
    },
    ListFixtures {},
    SaveFixture {
        history_id: u64,
    },
    DeleteFixture {
        id: String,
    },
    ClearFixtures {},
    FixtureToRule {
        id: String,
    },
    ReplayFixture {
        id: String,
    },
    ListRules {},
    PreviewRuleConflicts {
        rule: webhook_core::core::rules::ResponseRule,
    },
    SetRule {
        rule: webhook_core::core::rules::ResponseRule,
        confirm_conflicts: bool,
    },
    DeleteRule {
        id: String,
    },
    ResetRuleSequence {
        id: String,
    },
    ExportRunServiceDefinition {},
}
pub const WEBHOOK_METHODS: &[&str] = &[
    "server_status",
    "start_server",
    "stop_server",
    "list_history",
    "clear_history",
    "copy_masked_history",
    "copy_raw_history",
    "copy_history_headers",
    "delete_history",
    "replay_history",
    "list_fixtures",
    "save_fixture",
    "delete_fixture",
    "clear_fixtures",
    "fixture_to_rule",
    "replay_fixture",
    "list_rules",
    "preview_rule_conflicts",
    "set_rule",
    "delete_rule",
    "reset_rule_sequence",
    "export_run_service_definition",
];
impl WebhookCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::ServerStatus { .. } => "server_status",
            Self::StartServer { .. } => "start_server",
            Self::StopServer { .. } => "stop_server",
            Self::ListHistory { .. } => "list_history",
            Self::ClearHistory { .. } => "clear_history",
            Self::CopyMaskedHistory { .. } => "copy_masked_history",
            Self::CopyRawHistory { .. } => "copy_raw_history",
            Self::CopyHistoryHeaders { .. } => "copy_history_headers",
            Self::DeleteHistory { .. } => "delete_history",
            Self::ReplayHistory { .. } => "replay_history",
            Self::ListFixtures { .. } => "list_fixtures",
            Self::SaveFixture { .. } => "save_fixture",
            Self::DeleteFixture { .. } => "delete_fixture",
            Self::ClearFixtures { .. } => "clear_fixtures",
            Self::FixtureToRule { .. } => "fixture_to_rule",
            Self::ReplayFixture { .. } => "replay_fixture",
            Self::ListRules { .. } => "list_rules",
            Self::PreviewRuleConflicts { .. } => "preview_rule_conflicts",
            Self::SetRule { .. } => "set_rule",
            Self::DeleteRule { .. } => "delete_rule",
            Self::ResetRuleSequence { .. } => "reset_rule_sequence",
            Self::ExportRunServiceDefinition { .. } => "export_run_service_definition",
        }
    }
    pub fn class(&self) -> ExecutionClass {
        match self {
            Self::StopServer { .. } => ExecutionClass::Control,
            _ => ExecutionClass::Normal,
        }
    }
}
pub async fn dispatch(
    component_app: &tauri::AppHandle,
    call: WebhookCall,
) -> Result<serde_json::Value, String> {
    match call {
        WebhookCall::ServerStatus {} => {
            use crate::commands::*;
            let value = server_status(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::StartServer {
            bind,
            port,
            allow_lan,
        } => {
            use crate::commands::*;
            let value = start_server(component_app.state(), bind, port, allow_lan)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::StopServer {} => {
            use crate::commands::*;
            let value = stop_server(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ListHistory {} => {
            use crate::commands::*;
            let value = list_history(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ClearHistory {} => {
            use crate::commands::*;
            clear_history(component_app.state())?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::CopyMaskedHistory { id } => {
            use crate::commands::*;
            let value = copy_masked_history(component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::CopyRawHistory { id } => {
            use crate::commands::*;
            let value = copy_raw_history(component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::CopyHistoryHeaders { id } => {
            use crate::commands::*;
            let value = copy_history_headers(component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::DeleteHistory { id } => {
            use crate::commands::*;
            delete_history(component_app.state(), id)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ReplayHistory { history_id } => {
            use crate::commands::*;
            let value = replay_history(component_app.state(), history_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ListFixtures {} => {
            use crate::commands::*;
            let value = list_fixtures(component_app.clone(), component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::SaveFixture { history_id } => {
            use crate::commands::*;
            let value = save_fixture(component_app.clone(), component_app.state(), history_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::DeleteFixture { id } => {
            use crate::commands::*;
            delete_fixture(component_app.clone(), component_app.state(), id)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ClearFixtures {} => {
            use crate::commands::*;
            clear_fixtures(component_app.clone(), component_app.state())?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::FixtureToRule { id } => {
            use crate::commands::*;
            let value = fixture_to_rule(component_app.clone(), component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ReplayFixture { id } => {
            use crate::commands::*;
            let value = replay_fixture(component_app.clone(), component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ListRules {} => {
            use crate::commands::*;
            let value = list_rules(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::PreviewRuleConflicts { rule } => {
            use crate::commands::*;
            let value = preview_rule_conflicts(component_app.state(), rule)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::SetRule {
            rule,
            confirm_conflicts,
        } => {
            use crate::commands::*;
            let value = set_rule(component_app.state(), rule, confirm_conflicts)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::DeleteRule { id } => {
            use crate::commands::*;
            delete_rule(component_app.state(), id)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ResetRuleSequence { id } => {
            use crate::commands::*;
            reset_rule_sequence(component_app.state(), id)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        WebhookCall::ExportRunServiceDefinition {} => {
            let mut definition = crate::commands::export_run_service_definition(
                component_app.clone(),
                component_app.state(),
            )?;
            for service in &mut definition.services {
                service.name = service.name.replacen("Webhook Lab", "API Studio", 1);
            }
            serde_json::to_value(definition).map_err(|_| "component_response_invalid".into())
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "server_status",
            export.register::<crate::commands::ServerStatus>()?,
        ),
        (
            "start_server",
            export.register::<crate::commands::ServerStatus>()?,
        ),
        (
            "stop_server",
            export.register::<crate::commands::ServerStatus>()?,
        ),
        (
            "list_history",
            export.register::<Vec<crate::core::history::RequestRecord>>()?,
        ),
        ("clear_history", export.register::<()>()?),
        ("copy_masked_history", export.register::<String>()?),
        ("copy_raw_history", export.register::<String>()?),
        ("copy_history_headers", export.register::<String>()?),
        ("delete_history", export.register::<()>()?),
        (
            "replay_history",
            export.register::<crate::commands::ReplayResult>()?,
        ),
        (
            "list_fixtures",
            export.register::<Vec<webhook_core::core::fixtures::CapturedFixture>>()?,
        ),
        (
            "save_fixture",
            export.register::<webhook_core::core::fixtures::CapturedFixture>()?,
        ),
        ("delete_fixture", export.register::<()>()?),
        ("clear_fixtures", export.register::<()>()?),
        (
            "fixture_to_rule",
            export.register::<webhook_core::core::rules::ResponseRule>()?,
        ),
        (
            "replay_fixture",
            export.register::<crate::commands::ReplayResult>()?,
        ),
        (
            "list_rules",
            export.register::<Vec<webhook_core::core::rules::ResponseRule>>()?,
        ),
        (
            "preview_rule_conflicts",
            export.register::<webhook_core::core::rules::RuleConflictPreview>()?,
        ),
        ("set_rule", export.register::<String>()?),
        ("delete_rule", export.register::<()>()?),
        ("reset_rule_sequence", export.register::<()>()?),
        (
            "export_run_service_definition",
            export.register::<crate::core::service_profile::RunDefinitionExport>()?,
        ),
    ])
}
product_ipc::issue_codes! {pub enum WebhookIssue {
ComponentArgsInvalid="component_args_invalid",
ComponentClosing="component_closing",
ComponentResponseInvalid="component_response_invalid",
ComponentStateConflict="component_state_conflict",
ComponentStateUnavailable="component_state_unavailable",
ComponentStorageUnavailable="component_storage_unavailable",
ComponentUnavailable="component_unavailable",
LifecycleHideUnavailable="lifecycle_hide_unavailable",
LifecycleSettingsChanged="lifecycle_settings_changed",
LifecycleTrayUnavailable="lifecycle_tray_unavailable",
MockDraftBusy="mock_draft_busy",
MockDraftExpired="mock_draft_expired",
MockDraftInvalid="mock_draft_invalid",
MockDraftUnavailable="mock_draft_unavailable",
NativeError066631821755="native_error_066631821755",
NativeError0C550C94B33E="native_error_0c550c94b33e",
NativeError0Dd534D09121="native_error_0dd534d09121",
NativeError16Fd76Aa4F0F="native_error_16fd76aa4f0f",
NativeError1F7D166202Fc="native_error_1f7d166202fc",
NativeError22Fb1Af497E2="native_error_22fb1af497e2",
NativeError342B52315581="native_error_342b52315581",
NativeError446A67110D9D="native_error_446a67110d9d",
NativeError495902Fcb259="native_error_495902fcb259",
NativeError53Cc11F43034="native_error_53cc11f43034",
NativeError580Bb9625064="native_error_580bb9625064",
NativeError59626Ff68249="native_error_59626ff68249",
NativeError5Ad1257B8A81="native_error_5ad1257b8a81",
NativeError60C27F0116D2="native_error_60c27f0116d2",
NativeError64E2Eeabf510="native_error_64e2eeabf510",
NativeError7148C6Ab83Dd="native_error_7148c6ab83dd",
NativeError720404Dc8Aa5="native_error_720404dc8aa5",
NativeError7A45151676E3="native_error_7a45151676e3",
NativeError7B9580926Bef="native_error_7b9580926bef",
NativeError7F1146A75Ff9="native_error_7f1146a75ff9",
NativeError8043A49376D3="native_error_8043a49376d3",
NativeError8C893A1A9859="native_error_8c893a1a9859",
NativeError974Bbe3Cd37F="native_error_974bbe3cd37f",
NativeError9B26C2D9449D="native_error_9b26c2d9449d",
NativeError9C743Bd82693="native_error_9c743bd82693",
NativeErrorAb70140B4069="native_error_ab70140b4069",
NativeErrorB115Da98Ce90="native_error_b115da98ce90",
NativeErrorB38Fea85B6B5="native_error_b38fea85b6b5",
NativeErrorB6737Ef0Ac28="native_error_b6737ef0ac28",
NativeErrorCff919689Ee1="native_error_cff919689ee1",
NativeErrorD4Eb3A2200E7="native_error_d4eb3a2200e7",
NativeErrorD6437725F788="native_error_d6437725f788",
NativeErrorD99E2Ca963Ed="native_error_d99e2ca963ed",
NativeErrorDfa0A355Cca6="native_error_dfa0a355cca6",
NativeErrorE453907853A4="native_error_e453907853a4",
NativeErrorE499457B4657="native_error_e499457b4657",
NativeErrorEb088Ae5663F="native_error_eb088ae5663f",
NativeErrorEc1Af1045A13="native_error_ec1af1045a13",
NativeErrorFc8823B89Dce="native_error_fc8823b89dce",
NativeErrorFd4Ee7A85942="native_error_fd4ee7a85942",
Unavailable="unavailable",
}}
/// Fixed pre-existing messages are matched exactly, never by substrings.
/// Content-derived IDs keep these legacy translations stable across reordering.
pub fn classify(error: &str) -> &'static str {
    match error {
"API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다"=>WebhookIssue::NativeError446A67110D9D.code(),
"API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다"=>WebhookIssue::NativeErrorD4Eb3A2200E7.code(),
"API Playground를 실행하지 못했습니다. handoff를 안전하게 정리했으며 클립보드로 자동 전환하지 않습니다"=>WebhookIssue::NativeError9C743Bd82693.code(),
"LAN 공개를 시작하려면 명시적인 확인이 필요합니다"=>WebhookIssue::NativeError59626Ff68249.code(),
"Log Lens handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다"=>WebhookIssue::NativeErrorD6437725F788.code(),
"Log Lens를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다"=>WebhookIssue::NativeError0Dd534D09121.code(),
"Log Lens를 실행하지 못했습니다. handoff를 안전하게 정리했으며 클립보드로 자동 전환하지 않습니다"=>WebhookIssue::NativeErrorCff919689Ee1.code(),
"Logs 연결을 사용할 수 없습니다. 수신 요청과 저장한 fixture는 유지됩니다."=>WebhookIssue::NativeError495902Fcb259.code(),
"Webhook service profile 개수 제한에 도달했습니다"=>WebhookIssue::NativeErrorB115Da98Ce90.code(),
"Webhook service profile을 만들 수 없습니다"=>WebhookIssue::NativeError5Ad1257B8A81.code(),
"Webhook service profile을 읽을 수 없습니다"=>WebhookIssue::NativeError580Bb9625064.code(),
"Workspace Logs에 연결하지 못했습니다. 원본 요청과 fixture는 유지됩니다."=>WebhookIssue::NativeError9B26C2D9449D.code(),
"credential 형태의 응답이 포함된 규칙은 service profile로 내보낼 수 없습니다"=>WebhookIssue::NativeErrorB6737Ef0Ac28.code(),
"fixture 입력이 유효하지 않습니다"=>WebhookIssue::NativeError16Fd76Aa4F0F.code(),
"fixture 저장소 크기 제한을 초과했습니다"=>WebhookIssue::NativeErrorD99E2Ca963Ed.code(),
"fixture 저장소가 다른 작업에서 사용 중입니다. 잠시 후 다시 시도하세요"=>WebhookIssue::NativeError720404Dc8Aa5.code(),
"fixture 저장소가 다른 작업으로 변경되었습니다. 다시 시도하세요"=>WebhookIssue::NativeErrorEb088Ae5663F.code(),
"fixture 저장소를 읽을 수 없습니다"=>WebhookIssue::NativeErrorFd4Ee7A85942.code(),
"fixture 저장소를 저장할 수 없습니다"=>WebhookIssue::NativeErrorDfa0A355Cca6.code(),
"fixture를 찾을 수 없습니다"=>WebhookIssue::NativeErrorAb70140B4069.code(),
"handoff 요청에 사용할 fixture가 유효하지 않습니다"=>WebhookIssue::NativeError8043A49376D3.code(),
"localhost 서버가 실행 중이 아니거나 주소가 유효하지 않습니다"=>WebhookIssue::NativeError60C27F0116D2.code(),
"replay 요청을 보내지 못했습니다"=>WebhookIssue::NativeErrorB38Fea85B6B5.code(),
"replay 요청이 너무 많습니다. 잠시 후 다시 시도하세요"=>WebhookIssue::NativeError7A45151676E3.code(),
"replay 응답을 읽지 못했습니다"=>WebhookIssue::NativeError1F7D166202Fc.code(),
"replay 입력이 유효하지 않습니다"=>WebhookIssue::NativeError64E2Eeabf510.code(),
"response sequence를 초기화하지 못했습니다"=>WebhookIssue::NativeError8C893A1A9859.code(),
"겹치는 규칙을 저장하려면 충돌 확인이 필요합니다"=>WebhookIssue::NativeError0C550C94B33E.code(),
"규칙 입력이 유효하지 않습니다"=>WebhookIssue::NativeError342B52315581.code(),
"대상 앱 실행 실패 후 handoff를 정리하지 못했습니다. 잠시 후 다시 시도하세요"=>WebhookIssue::NativeError53Cc11F43034.code(),
"바이너리 본문 fixture는 API 요청으로 보낼 수 없습니다"=>WebhookIssue::NativeError974Bbe3Cd37F.code(),
"서버 bind에 실패했습니다"=>WebhookIssue::NativeError066631821755.code(),
"서버 내부 상태를 읽을 수 없습니다"=>WebhookIssue::NativeError7F1146A75Ff9.code(),
"실행 중인 loopback 서버만 Run Manager 서비스로 내보낼 수 있습니다"=>WebhookIssue::NativeError22Fb1Af497E2.code(),
"요청 시간이 초과되었습니다"=>WebhookIssue::NativeError7148C6Ab83Dd.code(),
"요청 크기가 허용 범위를 초과했습니다"=>WebhookIssue::NativeErrorFc8823B89Dce.code(),
"요청 헤더가 허용 범위를 초과했습니다"=>WebhookIssue::NativeErrorE499457B4657.code(),
"요청이 너무 많습니다. 잠시 후 다시 시도하세요"=>WebhookIssue::NativeError7B9580926Bef.code(),
"포트는 1~65535 범위여야 합니다"=>WebhookIssue::NativeErrorE453907853A4.code(),
"허용되지 않은 bind 주소입니다"=>WebhookIssue::NativeErrorEc1Af1045A13.code(),
_=>WebhookIssue::from_code(error).unwrap_or(WebhookIssue::Unavailable).code(),
}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_methods_are_unique_and_unknown_errors_are_private() {
        let mut names = WEBHOOK_METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 22);
        assert_eq!(classify("synthetic remote secret"), "unavailable");
    }
}
