use serde::{Deserialize, Serialize};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WorkspaceCall {
    ApiWorkspaceState {},
    SaveOpenapiDefinition(crate::core::openapi_definitions::Create),
    ListOpenapiDefinitions {},
    GetOpenapiDefinition {
        id: String,
    },
    DeleteOpenapiDefinition {
        id: String,
    },
    SaveApiWorkspace(crate::core::api_workspace::Save),
    SelectApiWorkspace {
        expected_revision: u64,
        id: Option<String>,
    },
    DeleteApiWorkspace {
        expected_revision: u64,
        id: String,
    },
}
impl WorkspaceCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::ApiWorkspaceState { .. } => "api_workspace_state",
            Self::SaveOpenapiDefinition(..) => "save_openapi_definition",
            Self::ListOpenapiDefinitions { .. } => "list_openapi_definitions",
            Self::GetOpenapiDefinition { .. } => "get_openapi_definition",
            Self::DeleteOpenapiDefinition { .. } => "delete_openapi_definition",
            Self::SaveApiWorkspace(..) => "save_api_workspace",
            Self::SelectApiWorkspace { .. } => "select_api_workspace",
            Self::DeleteApiWorkspace { .. } => "delete_api_workspace",
        }
    }
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceState {
    pub document: crate::core::api_workspace::Document,
    pub current_project_id: Option<String>,
    pub mock_profiles: Vec<MockProfileChoice>,
}
#[derive(Serialize, ts_rs::TS)]
pub struct MockProfileChoice {
    pub id: String,
    pub label: String,
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "api_workspace_state",
            export.register::<ApiWorkspaceState>()?,
        ),
        (
            "save_openapi_definition",
            export.register::<crate::core::openapi_definitions::Definition>()?,
        ),
        (
            "list_openapi_definitions",
            export.register::<Vec<crate::core::openapi_definitions::Summary>>()?,
        ),
        (
            "get_openapi_definition",
            export.register::<crate::core::openapi_definitions::Definition>()?,
        ),
        ("delete_openapi_definition", export.register::<()>()?),
        (
            "save_api_workspace",
            export.register::<crate::core::api_workspace::Document>()?,
        ),
        (
            "select_api_workspace",
            export.register::<crate::core::api_workspace::Document>()?,
        ),
        (
            "delete_api_workspace",
            export.register::<crate::core::api_workspace::Document>()?,
        ),
    ])
}
