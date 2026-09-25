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
pub enum ToolboxCall {
    TakePendingOpen {},
    PreviewToolboxText {
        handoff_id: String,
    },
    AcceptToolboxText {
        handoff_id: String,
    },
    DiscardToolboxText {
        handoff_id: String,
    },
    RenewToolboxText {
        handoff_id: String,
    },
    GenerateQr {
        request: transforms_core::core::qr::GenerateQrRequest,
    },
    Hash {
        data: String,
        algorithm: String,
    },
    HmacGenerate {
        request: transforms_core::core::hmac::HmacRequest,
    },
    HmacVerify {
        request: transforms_core::core::hmac::HmacVerifyRequest,
    },
    GenerateUuid {},
    GenerateIds {
        request: crate::commands::tools::GenerateIdsRequest,
    },
    RegexTest {
        pattern: String,
        text: String,
    },
    Diff {
        a: String,
        b: String,
    },
    JwtVerify {
        request: transforms_core::core::jwt::JwtVerifyRequest,
    },
    LoadWorkflowMetadata {},
    SaveWorkflowMetadata {
        serialized_metadata: String,
    },
}
pub const TOOLBOX_METHODS: &[&str] = &[
    "take_pending_open",
    "preview_toolbox_text",
    "accept_toolbox_text",
    "discard_toolbox_text",
    "renew_toolbox_text",
    "generate_qr",
    "hash",
    "hmac_generate",
    "hmac_verify",
    "generate_uuid",
    "generate_ids",
    "regex_test",
    "diff",
    "jwt_verify",
    "load_workflow_metadata",
    "save_workflow_metadata",
];
impl ToolboxCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::PreviewToolboxText { .. } => "preview_toolbox_text",
            Self::AcceptToolboxText { .. } => "accept_toolbox_text",
            Self::DiscardToolboxText { .. } => "discard_toolbox_text",
            Self::RenewToolboxText { .. } => "renew_toolbox_text",
            Self::GenerateQr { .. } => "generate_qr",
            Self::Hash { .. } => "hash",
            Self::HmacGenerate { .. } => "hmac_generate",
            Self::HmacVerify { .. } => "hmac_verify",
            Self::GenerateUuid { .. } => "generate_uuid",
            Self::GenerateIds { .. } => "generate_ids",
            Self::RegexTest { .. } => "regex_test",
            Self::Diff { .. } => "diff",
            Self::JwtVerify { .. } => "jwt_verify",
            Self::LoadWorkflowMetadata { .. } => "load_workflow_metadata",
            Self::SaveWorkflowMetadata { .. } => "save_workflow_metadata",
        }
    }
    pub fn class(&self) -> ExecutionClass {
        ExecutionClass::Normal
    }
}
pub async fn dispatch(
    component_app: &tauri::AppHandle,
    call: ToolboxCall,
) -> Result<serde_json::Value, String> {
    match call {
        ToolboxCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::PreviewToolboxText { handoff_id } => {
            use crate::commands::text_handoff::*;
            let result = preview_toolbox_text(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<PendingToolboxText>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            let value = result?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::AcceptToolboxText { handoff_id } => {
            use crate::commands::text_handoff::*;
            let result = accept_toolbox_text(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<PendingToolboxText>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            let value = result?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::DiscardToolboxText { handoff_id } => {
            use crate::commands::text_handoff::*;
            let result = discard_toolbox_text(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<PendingToolboxText>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            result?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::RenewToolboxText { handoff_id } => {
            use crate::commands::text_handoff::*;
            let result = renew_toolbox_text(component_app.state(), handoff_id.clone());
            if !component_app
                .state::<PendingToolboxText>()
                .has_claim(&handoff_id)
            {
                component_app
                    .state::<crate::applink::PendingOpen>()
                    .release(&handoff_id);
            }
            let value = result?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::GenerateQr { request } => {
            use crate::commands::qr::*;
            let value = generate_qr(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::Hash { data, algorithm } => {
            use crate::commands::tools::*;
            let value = hash(data, algorithm)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::HmacGenerate { request } => {
            use crate::commands::tools::*;
            let value = hmac_generate(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::HmacVerify { request } => {
            use crate::commands::tools::*;
            let value = hmac_verify(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::GenerateUuid {} => {
            use crate::commands::tools::*;
            let value = generate_uuid()?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::GenerateIds { request } => {
            use crate::commands::tools::*;
            let value = generate_ids(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::RegexTest { pattern, text } => {
            use crate::commands::tools::*;
            let value = regex_test(pattern, text)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::Diff { a, b } => {
            use crate::commands::tools::*;
            let value = diff(a, b);
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::JwtVerify { request } => {
            use crate::commands::tools::*;
            let value = jwt_verify(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::LoadWorkflowMetadata {} => {
            use crate::commands::workflows::*;
            let value = load_workflow_metadata(component_app.clone());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        ToolboxCall::SaveWorkflowMetadata {
            serialized_metadata,
        } => {
            use crate::commands::workflows::*;
            save_workflow_metadata(component_app.clone(), serialized_metadata)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "take_pending_open",
            export.register::<Option<devbox_applink::OpenRequest>>()?,
        ),
        (
            "preview_toolbox_text",
            export.register::<crate::commands::text_handoff::ToolboxTextPreview>()?,
        ),
        ("accept_toolbox_text", export.register::<String>()?),
        ("discard_toolbox_text", export.register::<()>()?),
        (
            "renew_toolbox_text",
            export.register::<crate::commands::text_handoff::RenewToolboxTextResult>()?,
        ),
        (
            "generate_qr",
            export.register::<transforms_core::core::qr::QrResult>()?,
        ),
        ("hash", export.register::<String>()?),
        ("hmac_generate", export.register::<String>()?),
        ("hmac_verify", export.register::<bool>()?),
        ("generate_uuid", export.register::<String>()?),
        ("generate_ids", export.register::<Vec<String>>()?),
        (
            "regex_test",
            export.register::<Vec<crate::commands::tools::RegexMatch>>()?,
        ),
        (
            "diff",
            export.register::<Vec<crate::commands::tools::DiffHunk>>()?,
        ),
        ("jwt_verify", export.register::<bool>()?),
        (
            "load_workflow_metadata",
            export.register::<transforms_core::core::workflows::WorkflowLoadResult>()?,
        ),
        ("save_workflow_metadata", export.register::<()>()?),
    ])
}
product_ipc::issue_codes! {pub enum ToolboxIssue {
ComponentArgsInvalid="component_args_invalid",
ComponentDeliveryBusy="component_delivery_busy",
ComponentDeliveryInvalid="component_delivery_invalid",
ComponentDeliveryUnavailable="component_delivery_unavailable",
ComponentResponseInvalid="component_response_invalid",
ComponentStateConflict="component_state_conflict",
ComponentStorageUnavailable="component_storage_unavailable",
ComponentUnavailable="component_unavailable",
KnowledgeDraftInvalid="knowledge_draft_invalid",
KnowledgeOwnerInvalid="knowledge_owner_invalid",
KnowledgeStorageFull="knowledge_storage_full",
KnowledgeStorageUnavailable="knowledge_storage_unavailable",
MockDraftBusy="mock_draft_busy",
MockDraftExpired="mock_draft_expired",
MockDraftInvalid="mock_draft_invalid",
MockDraftUnavailable="mock_draft_unavailable",
NativeError103A413E24F8="native_error_103a413e24f8",
NativeError184E42B0F97C="native_error_184e42b0f97c",
NativeError20F163616A48="native_error_20f163616a48",
NativeError2931063733D3="native_error_2931063733d3",
NativeError2E177F197F6B="native_error_2e177f197f6b",
NativeError446A67110D9D="native_error_446a67110d9d",
NativeError4B8F3C1E2449="native_error_4b8f3c1e2449",
NativeError5C4537Feee0E="native_error_5c4537feee0e",
NativeError7B47Bde22Cd3="native_error_7b47bde22cd3",
NativeError84F3635651E5="native_error_84f3635651e5",
NativeError880Caf8288Ea="native_error_880caf8288ea",
NativeError8Eb0326Aefbb="native_error_8eb0326aefbb",
NativeError95913153Dce4="native_error_95913153dce4",
NativeErrorA22E2614Df1F="native_error_a22e2614df1f",
NativeErrorA34Bb1Ec3D7C="native_error_a34bb1ec3d7c",
NativeErrorB3C273A8111A="native_error_b3c273a8111a",
NativeErrorB6Af3F8C3364="native_error_b6af3f8c3364",
NativeErrorC9A8491603Aa="native_error_c9a8491603aa",
NativeErrorD265251361E9="native_error_d265251361e9",
NativeErrorD4Eb3A2200E7="native_error_d4eb3a2200e7",
NativeErrorDd5Ec2C72Ee5="native_error_dd5ec2c72ee5",
NativeErrorE5Ef35A5A0Cf="native_error_e5ef35a5a0cf",
TransformExportDenied="transform_export_denied",
Unavailable="unavailable",
}}
/// Fixed pre-existing messages are matched exactly, never by substrings.
/// Content-derived IDs keep these legacy translations stable across reordering.
pub fn classify(error: &str) -> &'static str {
    match error {
"API Playground handoff를 만들지 못했습니다. 클립보드로 자동 전환하지 않습니다"=>ToolboxIssue::NativeError446A67110D9D.code(),
"API Playground로 전달할 텍스트가 유효하지 않습니다"=>ToolboxIssue::NativeErrorDd5Ec2C72Ee5.code(),
"API Playground를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다"=>ToolboxIssue::NativeErrorD4Eb3A2200E7.code(),
"API Playground를 실행하지 못했습니다. 전달 데이터는 폐기했습니다. 클립보드로 자동 전환하지 않습니다"=>ToolboxIssue::NativeErrorE5Ef35A5A0Cf.code(),
"HMAC 입력을 처리할 수 없습니다."=>ToolboxIssue::NativeError8Eb0326Aefbb.code(),
"JWT 검증을 처리할 수 없습니다."=>ToolboxIssue::NativeError103A413E24F8.code(),
"Knowledge draft를 만들거나 전달하지 못했습니다. 클립보드로 자동 전환하지 않습니다"=>ToolboxIssue::NativeError95913153Dce4.code(),
"Knowledge를 사용할 수 없습니다. 설치 또는 업데이트 후 다시 시도하세요. 클립보드로 자동 전환하지 않습니다"=>ToolboxIssue::NativeError84F3635651E5.code(),
"QR 버전이 올바르지 않습니다."=>ToolboxIssue::NativeError7B47Bde22Cd3.code(),
"QR 여백이 올바르지 않습니다."=>ToolboxIssue::NativeError20F163616A48.code(),
"QR 오류 보정 수준이 올바르지 않습니다."=>ToolboxIssue::NativeErrorA22E2614Df1F.code(),
"QR 용량을 초과했습니다. 버전 또는 오류 보정 수준을 조정하세요."=>ToolboxIssue::NativeError4B8F3C1E2449.code(),
"QR 이미지를 생성하지 못했습니다."=>ToolboxIssue::NativeError184E42B0F97C.code(),
"QR 입력 형식이 올바르지 않습니다."=>ToolboxIssue::NativeError2E177F197F6B.code(),
"QR 입력은 비어 있을 수 없습니다."=>ToolboxIssue::NativeError2931063733D3.code(),
"QR 입력이 너무 깁니다."=>ToolboxIssue::NativeErrorB6Af3F8C3364.code(),
"QR 크기가 버전과 여백에 비해 작습니다."=>ToolboxIssue::NativeErrorB3C273A8111A.code(),
"QR 크기가 올바르지 않습니다."=>ToolboxIssue::NativeErrorC9A8491603Aa.code(),
"Toolbox workflow metadata를 저장할 수 없습니다."=>ToolboxIssue::NativeErrorD265251361E9.code(),
"Wi-Fi 설정이 올바르지 않습니다."=>ToolboxIssue::NativeError880Caf8288Ea.code(),
"식별자 생성 순서를 유지할 수 없습니다."=>ToolboxIssue::NativeErrorA34Bb1Ec3D7C.code(),
"암호학적으로 안전한 난수를 사용할 수 없습니다."=>ToolboxIssue::NativeError5C4537Feee0E.code(),
_=>ToolboxIssue::from_code(error).unwrap_or(ToolboxIssue::Unavailable).code(),
}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_methods_are_unique_and_unknown_errors_are_private() {
        let mut names = TOOLBOX_METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 16);
        assert_eq!(classify("synthetic remote secret"), "unavailable");
    }
}
