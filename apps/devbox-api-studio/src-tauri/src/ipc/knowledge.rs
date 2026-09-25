use product_ipc::TypeExporter;
use serde::Deserialize;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum KnowledgeCall {
    SaveKnowledgeDraft {
        output: String,
        source: Option<transforms_core::core::export_policy::OutputSource>,
    },
    SendKnowledgeDraft {
        id: String,
    },
    ListKnowledgeDrafts {},
    GetKnowledgeDraft {
        id: String,
    },
    DeleteKnowledgeDraft {
        id: String,
    },
}
impl KnowledgeCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::SaveKnowledgeDraft { .. } => "save_knowledge_draft",
            Self::SendKnowledgeDraft { .. } => "send_knowledge_draft",
            Self::ListKnowledgeDrafts {} => "list_knowledge_drafts",
            Self::GetKnowledgeDraft { .. } => "get_knowledge_draft",
            Self::DeleteKnowledgeDraft { .. } => "delete_knowledge_draft",
        }
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
pub struct SavedKnowledgeDraft {
    pub delivery: DraftDelivery,
    pub draft: product_contract::knowledge_draft::Draft,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum DraftDelivery {
    Stored,
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "save_knowledge_draft",
            export.register::<SavedKnowledgeDraft>()?,
        ),
        (
            "send_knowledge_draft",
            export.register::<product_contract::navigation::Receipt>()?,
        ),
        (
            "list_knowledge_drafts",
            export.register::<Vec<product_contract::knowledge_draft::Summary>>()?,
        ),
        (
            "get_knowledge_draft",
            export.register::<product_contract::knowledge_draft::Draft>()?,
        ),
        ("delete_knowledge_draft", export.register::<()>()?),
    ])
}
