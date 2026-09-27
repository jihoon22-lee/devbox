use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use tauri::{Manager, WebviewWindow};
use webhook_host::api::{self, WebhookCall};
type SelfOwner = StudioWebhookCall;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WebhookHostCall {
    SendHistoryToApi { history_id: u64 },
    SendFixtureToApi { id: String },
    SendHistoryToLogLens { history_id: u64 },
    SendFixtureToLogLens { id: String },
}
impl WebhookHostCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::SendHistoryToApi { .. } => "send_history_to_api",
            Self::SendFixtureToApi { .. } => "send_fixture_to_api",
            Self::SendHistoryToLogLens { .. } => "send_history_to_log_lens",
            Self::SendFixtureToLogLens { .. } => "send_fixture_to_log_lens",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum HostWebhookCall {
    Mock(super::mock_draft::MockDraftCall),
    Lifecycle(super::lifecycle::LifecycleCall),
    Extra(WebhookHostCall),
}
impl HostWebhookCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Mock(call) => call.method(),
            Self::Lifecycle(call) => call.method(),
            Self::Extra(call) => call.method(),
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum StudioWebhookCall {
    Host(HostWebhookCall),
    Engine(WebhookCall),
}
impl ComponentCall for StudioWebhookCall {
    const COMPONENT: &'static str = "api-studio.webhooks";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["webhooks"]
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Engine(call) => call.class(),
            Self::Host(HostWebhookCall::Lifecycle(
                super::lifecycle::LifecycleCall::QuitProduct {},
            )) => ExecutionClass::Control,
            _ => ExecutionClass::Normal,
        }
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct WebhookLogResult {
    pub handoff_id: String,
    pub producer_id: String,
    pub consumer_id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}
#[tauri::command]
pub async fn webhooks(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let method = request.method.clone();
    let args = request.args.clone();
    let (admission, request) = admit_request::<StudioWebhookCall>(&window, request)?;
    let app = window.app_handle();
    if !matches!(
        &request.call,
        StudioWebhookCall::Host(HostWebhookCall::Lifecycle(_))
    ) {
        crate::lifecycle::require_open(app)
            .map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    }
    let result = match request.call {
        StudioWebhookCall::Engine(_) => {
            crate::webhook_owner::call(app, &method, args, request.header.deadline_ms).await
        }
        StudioWebhookCall::Host(call) => {
            host(
                app,
                call,
                admission.provenance().clone(),
                request.header.deadline_ms,
            )
            .await
        }
    };
    Ok(admission.finish(result, api::classify))
}
async fn host(
    app: &tauri::AppHandle,
    call: HostWebhookCall,
    provenance: product_contract::Provenance,
    deadline: u64,
) -> Result<serde_json::Value, String> {
    match call {
        HostWebhookCall::Mock(call) => crate::mock_draft::dispatch_typed(app, call),
        HostWebhookCall::Lifecycle(call) => crate::lifecycle::dispatch_typed(app, call).await,
        HostWebhookCall::Extra(call) => match call {
            WebhookHostCall::SendHistoryToApi { history_id } => {
                send_api(
                    app,
                    webhook_host::component::HandoffSelection::History { history_id },
                    provenance,
                    deadline,
                )
                .await
            }
            WebhookHostCall::SendFixtureToApi { id } => {
                send_api(
                    app,
                    webhook_host::component::HandoffSelection::Fixture { id },
                    provenance,
                    deadline,
                )
                .await
            }
            WebhookHostCall::SendHistoryToLogLens { history_id } => {
                crate::webhook_logs::send(
                    app,
                    webhook_host::component::HandoffSelection::History { history_id },
                    provenance.request_id,
                    deadline,
                )
                .await
            }
            WebhookHostCall::SendFixtureToLogLens { id } => {
                crate::webhook_logs::send(
                    app,
                    webhook_host::component::HandoffSelection::Fixture { id },
                    provenance.request_id,
                    deadline,
                )
                .await
            }
        },
    }
}
async fn send_api(
    app: &tauri::AppHandle,
    selection: webhook_host::component::HandoffSelection,
    provenance: product_contract::Provenance,
    deadline: u64,
) -> Result<serde_json::Value, String> {
    project_then_publish(
        crate::webhook_owner::project(
            app,
            webhook_host::component::Projection::Api(selection),
            deadline,
        ),
        |payload| {
            crate::handoff::send_typed(
                app,
                SelfOwner::COMPONENT,
                crate::handoff::SendCall::Webhook(payload),
                provenance,
            )
        },
    )
    .await
}
async fn project_then_publish<F, P>(projection: F, publish: P) -> Result<serde_json::Value, String>
where
    F: std::future::Future<Output = Result<serde_json::Value, String>>,
    P: FnOnce(serde_json::Value) -> Result<serde_json::Value, String>,
{
    publish(projection.await?)
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<StudioWebhookCall>()?;
    export.register::<api::WebhookIssue>()?;
    let mut results = api::result_types(export)?;
    results.extend(super::mock_draft::result_types(export)?);
    results.extend(super::lifecycle::result_types(export)?);
    results.extend([
        (
            "send_history_to_api",
            export.register::<crate::handoff::HandoffResult>()?,
        ),
        (
            "send_fixture_to_api",
            export.register::<crate::handoff::HandoffResult>()?,
        ),
        (
            "send_history_to_log_lens",
            export.register::<WebhookLogResult>()?,
        ),
        (
            "send_fixture_to_log_lens",
            export.register::<WebhookLogResult>()?,
        ),
    ]);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn handoff_publishes_only_the_owner_projection_and_never_publishes_on_owner_failure() {
        let fixture = serde_json::json!({"method":"POST","url":"/hook","headers":[],"body":"{}"});
        let projected = fixture.clone();
        assert_eq!(
            project_then_publish(async { Ok(projected) }, |received| {
                assert_eq!(received, fixture);
                Ok(received)
            })
            .await
            .unwrap(),
            fixture
        );
        assert_eq!(
            project_then_publish(
                async { Err("webhook_agent_unavailable".into()) },
                |_| panic!("must not publish without owner projection")
            )
            .await
            .unwrap_err(),
            "webhook_agent_unavailable"
        );
    }
}
