//! PTY streaming keeps companion authority; main routes cannot acquire a peer's output.
use product_ipc::ComponentCall;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TerminalStreamCall {
    Subscribe { session_id: String, after: u64 },
}
impl ComponentCall for TerminalStreamCall {
    const COMPONENT: &'static str = "workspace.terminal";
    const MAX_ARGUMENT_BYTES: usize = 1024;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        "subscribe"
    }
    fn routes(&self) -> &'static [&'static str] {
        &["terminal"]
    }
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub subscription_id: String,
}
#[tauri::command]
pub(crate) async fn terminal_output_stream(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
    channel: tauri::ipc::Channel<terminal_engine::core::terminal_output::OutputBatch>,
) -> Result<Subscription, String> {
    if runtime.shutting_down() {
        return Err("request_cancelled".into());
    }
    let request = request
        .decode::<TerminalStreamCall>()
        .map_err(|_| "terminal_args_invalid")?;
    runtime.terminals.authorize(&window, &request.header)?;
    let _permit = runtime
        .lanes
        .try_enter(product_ipc::workspace::Lane::TerminalIo)?;
    let host = runtime.host()?;
    let TerminalStreamCall::Subscribe { session_id, after } = request.call;
    runtime
        .terminals
        .subscribe_output(&window, &host, &request.header, session_id, after, channel)
        .map_err(str::to_owned)
}
