//! Typed Activity command, preserving native startup and producer boundaries.
use activity_engine::api::{self, ActivityCall};
use product_contract::{
    agent_settings::{AgentAutostartStatus, AgentSettingsCall},
    Problem, ProblemCode,
};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use tauri::{Manager, WebviewWindow};

#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum KnowledgeActivityCall {
    Settings(AgentSettingsCall),
    Engine(ActivityCall),
}
impl ComponentCall for KnowledgeActivityCall {
    const COMPONENT: &'static str = "knowledge.activity";
    fn method(&self) -> &'static str {
        match self {
            Self::Settings(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Settings(_) => ExecutionClass::Normal,
            Self::Engine(call) => call.class(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["activity"]
    }
}
#[tauri::command]
pub async fn activity(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let args = request.args.clone();
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
        KnowledgeActivityCall::Settings(call) => {
            product_shell_tauri::agent_settings::call(
                app,
                "knowledge",
                call,
                request.header.deadline_ms,
            )
            .await
        }
        KnowledgeActivityCall::Engine(ActivityCall::SendDigestToKnowledge {
            input,
            regenerated_from,
        }) => {
            if crate::collector_owner::installed(app)
                .map_err(|_| admission.problem(ProblemCode::Unavailable))?
            {
                crate::collector_owner::send_draft(app, args, request.header.deadline_ms).await
            } else {
                activity_engine::component::send_product_draft_typed(
                    app,
                    input,
                    regenerated_from,
                    |draft| knowledge_vault_engine::component::offer_product_draft(app, draft),
                )
                .await
            }
        }
        KnowledgeActivityCall::Engine(ActivityCall::RegenerateKnowledgeDraft { handoff_id }) => {
            if crate::collector_owner::installed(app)
                .map_err(|_| admission.problem(ProblemCode::Unavailable))?
            {
                crate::collector_owner::send_draft(app, args, request.header.deadline_ms).await
            } else {
                activity_engine::component::regenerate_product_draft(app, handoff_id, |draft| {
                    knowledge_vault_engine::component::offer_product_draft(app, draft)
                })
                .await
            }
        }
        KnowledgeActivityCall::Engine(call) => {
            let method = call.method();
            crate::collector_owner::call(
                app,
                "knowledge.activity",
                method,
                args,
                request.header.deadline_ms,
            )
            .await
            .and_then(|value| {
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
    for method in ["autostart_status", "set_autostart"] {
        results.push((method, export.register::<AgentAutostartStatus>()?));
    }
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
    Ok(results)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_and_engine_calls_preserve_the_activity_route() {
        assert!(serde_json::from_str::<KnowledgeActivityCall>(
            r#"{"method":"get_close_policy","args":{}}"#
        )
        .is_err());
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
