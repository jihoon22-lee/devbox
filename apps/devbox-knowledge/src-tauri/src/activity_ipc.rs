//! Typed Activity command, preserving native startup and producer boundaries.
use activity_engine::api::{self, ActivityCall};
use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use tauri::{Manager, WebviewWindow};

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HostActivityCall {
    GetClosePolicy {},
    SetClosePolicy { close_to_tray: bool },
}
impl HostActivityCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::GetClosePolicy {} => "get_close_policy",
            Self::SetClosePolicy { .. } => "set_close_policy",
        }
    }
}
#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum KnowledgeActivityCall {
    Host(HostActivityCall),
    Engine(ActivityCall),
}
impl ComponentCall for KnowledgeActivityCall {
    const COMPONENT: &'static str = "knowledge.activity";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["activity"]
    }
}
#[tauri::command]
pub async fn activity(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<KnowledgeActivityCall>(&window, request)?;
    let app = window.app_handle();
    crate::startup::require_active(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    if matches!(
        &request.call,
        KnowledgeActivityCall::Engine(
            ActivityCall::ProjectAttribution { .. }
                | ActivityCall::GetDigest { .. }
                | ActivityCall::GetDay { .. }
                | ActivityCall::GetRange { .. }
        )
    ) {
        crate::project_provider::refresh(app, request.header.deadline_ms).await;
    }
    let result = match request.call {
        KnowledgeActivityCall::Host(call) => crate::lifecycle::dispatch_typed(app, call),
        KnowledgeActivityCall::Engine(ActivityCall::SendDigestToKnowledge {
            input,
            regenerated_from,
        }) => {
            activity_engine::component::send_product_draft_typed(
                app,
                input,
                regenerated_from,
                |draft| knowledge_vault_engine::component::offer_product_draft(app, draft),
            )
            .await
        }
        KnowledgeActivityCall::Engine(call) => {
            let method = call.method();
            api::dispatch(app, call).await.and_then(|value| {
                crate::activity_projection::validate(
                    method,
                    crate::search::associate_activity(app, method, value),
                )
            })
        }
    };
    Ok(admission.finish(result, api::classify))
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<KnowledgeActivityCall>()?;
    export.register::<api::ActivityIssue>()?;
    let mut results = api::result_types(export)?;
    use crate::activity_projection::*;
    for (method, ty) in [
        ("get_day", export.register::<ActivityDaySummary>()?),
        ("get_range", export.register::<ActivityRangeSummary>()?),
        ("get_digest", export.register::<ActivityDigestResponse>()?),
        (
            "project_attribution",
            export.register::<ActivityAttributionResult>()?,
        ),
    ] {
        results
            .iter_mut()
            .find(|entry| entry.0 == method)
            .unwrap()
            .1 = ty;
    }
    let policy = export.register::<crate::lifecycle::ClosePolicy>()?;
    results.extend([
        ("get_close_policy", policy.clone()),
        ("set_close_policy", policy),
    ]);
    Ok(results)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_and_engine_calls_preserve_the_activity_route() {
        let host: KnowledgeActivityCall =
            serde_json::from_str(r#"{"method":"get_close_policy","args":{}}"#).unwrap();
        assert!(matches!(host, KnowledgeActivityCall::Host(_)));
        let engine: KnowledgeActivityCall =
            serde_json::from_str(r#"{"method":"stop_tracking","args":{}}"#).unwrap();
        assert!(matches!(engine, KnowledgeActivityCall::Engine(_)));
        assert_eq!(engine.routes(), &["activity"]);
        assert!(
            serde_json::from_str::<KnowledgeActivityCall>(r#"{"method":"nope","args":{}}"#)
                .is_err()
        );
    }
}
