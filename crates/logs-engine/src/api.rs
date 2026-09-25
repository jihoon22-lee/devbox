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
pub enum LogsCall {
    SendSelectionToToolbox {
        text: String,
    },
    SummarizeSource {
        source: crate::core::model::SourceSpec,
    },
    ReceiveLogSource {
        reference: crate::core::model::LogSourceRef,
    },
    FixedAdapter {
        source: crate::core::model::SourceSpec,
    },
    ReadSource {
        source: crate::core::model::SourceSpec,
        cursor: Option<crate::core::model::FileCursor>,
        sequence_start: u64,
        generation: u64,
        operation_id: String,
    },
    ReadSources {
        sources: Vec<crate::core::model::SourceSpec>,
        cursors: Vec<Option<crate::core::model::FileCursor>>,
        sequence_starts: Vec<u64>,
        generation: u64,
        operation_id: String,
    },
    CancelRead {
        operation_id: String,
    },
    FilterLogRecords {
        records: Vec<crate::core::model::LogRecord>,
        filter: crate::core::model::FilterSpec,
    },
    ExportLogRecords {
        records: Vec<crate::core::model::LogRecord>,
    },
    PreviewLogSource {
        id: String,
        handoff_kind: String,
    },
    AcceptLogSource {
        id: String,
    },
    DiscardLogSource {
        id: String,
    },
    RenewLogSource {
        id: String,
    },
    TakePendingOpen {},
    ListSavedViews {},
    SaveSavedView {
        expected_revision: u64,
        view: crate::core::SavedView,
    },
    DeleteSavedView {
        expected_revision: u64,
        name: String,
    },
}
pub const METHODS: &[&str] = &[
    "delete_saved_view",
    "save_saved_view",
    "list_saved_views",
    "send_selection_to_toolbox",
    "summarize_source",
    "receive_log_source",
    "fixed_adapter",
    "read_source",
    "read_sources",
    "cancel_read",
    "filter_log_records",
    "export_log_records",
    "preview_log_source",
    "accept_log_source",
    "discard_log_source",
    "renew_log_source",
    "take_pending_open",
];
impl LogsCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::DeleteSavedView { .. } => "delete_saved_view",
            Self::SaveSavedView { .. } => "save_saved_view",
            Self::ListSavedViews { .. } => "list_saved_views",
            Self::SendSelectionToToolbox { .. } => "send_selection_to_toolbox",
            Self::SummarizeSource { .. } => "summarize_source",
            Self::ReceiveLogSource { .. } => "receive_log_source",
            Self::FixedAdapter { .. } => "fixed_adapter",
            Self::ReadSource { .. } => "read_source",
            Self::ReadSources { .. } => "read_sources",
            Self::CancelRead { .. } => "cancel_read",
            Self::FilterLogRecords { .. } => "filter_log_records",
            Self::ExportLogRecords { .. } => "export_log_records",
            Self::PreviewLogSource { .. } => "preview_log_source",
            Self::AcceptLogSource { .. } => "accept_log_source",
            Self::DiscardLogSource { .. } => "discard_log_source",
            Self::RenewLogSource { .. } => "renew_log_source",
            Self::TakePendingOpen { .. } => "take_pending_open",
        }
    }
    pub fn lane(&self) -> Lane {
        match self {
            Self::CancelRead { .. } => Lane::EngineStop,
            _ => Lane::Engine,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        LONG_BUDGET_MS
    }
}
#[cfg(feature = "desktop")]
async fn execute(
    _component_app: &tauri::AppHandle,
    call: LogsCall,
) -> Result<serde_json::Value, String> {
    match call {
        LogsCall::DeleteSavedView {
            expected_revision,
            name,
        } => serde_json::to_value(
            crate::core::product_saved_views::delete(
                &crate::component::data_root(_component_app)?,
                expected_revision,
                &name,
            )
            .map_err(str::to_owned)?,
        )
        .map_err(|_| "component_response_invalid".into()),
        LogsCall::SaveSavedView {
            expected_revision,
            view,
        } => serde_json::to_value(
            crate::core::product_saved_views::save(
                &crate::component::data_root(_component_app)?,
                expected_revision,
                view,
            )
            .map_err(str::to_owned)?,
        )
        .map_err(|_| "component_response_invalid".into()),
        LogsCall::ListSavedViews {} => serde_json::to_value(
            crate::core::product_saved_views::load(&crate::component::data_root(_component_app)?)
                .map_err(str::to_owned)?
                .views,
        )
        .map_err(|_| "component_response_invalid".into()),
        LogsCall::SendSelectionToToolbox { text } => {
            use crate::commands::*;
            let value = send_selection_to_toolbox(text)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::SummarizeSource { source } => {
            use crate::commands::*;
            let value = summarize_source(source)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::ReceiveLogSource { reference } => {
            use crate::commands::*;
            let value = receive_log_source(reference)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::FixedAdapter { source } => {
            use crate::commands::*;
            let value = fixed_adapter(source)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::ReadSource {
            source,
            cursor,
            sequence_start,
            generation,
            operation_id,
        } => {
            use crate::commands::*;
            let value = read_source(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                source,
                cursor,
                sequence_start,
                generation,
                operation_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::ReadSources {
            sources,
            cursors,
            sequence_starts,
            generation,
            operation_id,
        } => {
            use crate::commands::*;
            let value = read_sources(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                sources,
                cursors,
                sequence_starts,
                generation,
                operation_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::CancelRead { operation_id } => {
            use crate::commands::*;
            let value = cancel_read(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                operation_id,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::FilterLogRecords { records, filter } => {
            use crate::commands::*;
            let value = filter_log_records(records, filter)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::ExportLogRecords { records } => {
            use crate::commands::*;
            let value = export_log_records(records)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::PreviewLogSource { id, handoff_kind } => {
            use crate::handoff::*;
            let value = preview_log_source(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                id,
                handoff_kind,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::AcceptLogSource { id } => {
            use crate::handoff::*;
            let value = accept_log_source(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                id,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::DiscardLogSource { id } => {
            use crate::handoff::*;
            discard_log_source(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                id,
            )?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::RenewLogSource { id } => {
            use crate::handoff::*;
            let value = renew_log_source(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                id,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LogsCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            );
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "delete_saved_view",
            export.register::<crate::core::saved_views::SavedViewsDocument>()?,
        ),
        (
            "save_saved_view",
            export.register::<crate::core::saved_views::SavedViewsDocument>()?,
        ),
        (
            "list_saved_views",
            export.register::<crate::core::saved_views::SavedViewsDocument>()?,
        ),
        (
            "send_selection_to_toolbox",
            export.register::<crate::commands::ToolboxDispatch>()?,
        ),
        (
            "summarize_source",
            export.register::<crate::core::model::SourceSummary>()?,
        ),
        (
            "receive_log_source",
            export.register::<crate::core::model::SourceSpec>()?,
        ),
        (
            "fixed_adapter",
            export.register::<Option<crate::core::AdapterPlan>>()?,
        ),
        (
            "read_source",
            export.register::<crate::core::model::SourceSnapshot>()?,
        ),
        (
            "read_sources",
            export.register::<crate::commands::SourcesSnapshot>()?,
        ),
        (
            "cancel_read",
            export.register::<crate::commands::CancelResponse>()?,
        ),
        (
            "filter_log_records",
            export.register::<Vec<crate::core::model::LogRecord>>()?,
        ),
        (
            "export_log_records",
            export.register::<crate::core::parser::ExportedText>()?,
        ),
        (
            "preview_log_source",
            export.register::<crate::core::handoff::LogSourcePreview>()?,
        ),
        (
            "accept_log_source",
            export.register::<crate::core::model::SourceSpec>()?,
        ),
        ("discard_log_source", export.register::<()>()?),
        (
            "renew_log_source",
            export.register::<crate::handoff::RenewLogSourceResult>()?,
        ),
        (
            "take_pending_open",
            export.register::<Option<devbox_applink::OpenRequest>>()?,
        ),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_methods_and_extra_arguments_are_rejected() {
        assert!(serde_json::from_str::<LogsCall>(r#"{"method":"unknown","args":{}}"#).is_err());
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
pub async fn dispatch(app: &tauri::AppHandle, call: LogsCall) -> Result<serde_json::Value, String> {
    crate::component::data_root(app)?;
    let result = execute(app, call).await;
    crate::component::data_root(app)?;
    result
}
