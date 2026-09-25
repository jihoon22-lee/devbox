#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MockDraftCall {
    PeekMockDraft {},
    AcceptMockDraft { id: String },
    DiscardMockDraft { id: String },
}
impl MockDraftCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::PeekMockDraft {} => "peek_mock_draft",
            Self::AcceptMockDraft { .. } => "accept_mock_draft",
            Self::DiscardMockDraft { .. } => "discard_mock_draft",
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "peek_mock_draft",
            export.register::<Option<crate::core::mock_draft::Preview>>()?,
        ),
        (
            "accept_mock_draft",
            export.register::<webhook_core::core::rules::ResponseRule>()?,
        ),
        ("discard_mock_draft", export.register::<()>()?),
    ])
}
