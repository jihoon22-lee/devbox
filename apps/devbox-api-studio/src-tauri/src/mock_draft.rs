use crate::core::mock_draft::Receiver;
use serde_json::Value;
use tauri::Manager;
fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|v| u64::try_from(v.as_millis()).ok())
        .ok_or_else(|| "mock_draft_unavailable".into())
}
pub fn initialize(app: &tauri::AppHandle, store: applink::HandoffStore) -> Result<(), String> {
    if !app.manage(Receiver::new(store)) {
        return Err("mock_draft_unavailable".into());
    }
    Ok(())
}
pub fn deliver(app: &tauri::AppHandle, request: applink::OpenRequest) -> Result<(), String> {
    app.state::<Receiver>().offer(request, now()?)
}
pub fn dispatch_typed(
    app: &tauri::AppHandle,
    call: crate::ipc::mock_draft::MockDraftCall,
) -> Result<Value, String> {
    use crate::ipc::mock_draft::MockDraftCall;
    let state = app.state::<Receiver>();
    match call {
        MockDraftCall::PeekMockDraft {} => serde_json::to_value(state.preview(now()?)?)
            .map_err(|_| "mock_draft_unavailable".into()),
        MockDraftCall::AcceptMockDraft { id } => serde_json::to_value(state.finish(&id, now()?)?)
            .map_err(|_| "mock_draft_unavailable".into()),
        MockDraftCall::DiscardMockDraft { id } => {
            state.finish(&id, now()?)?;
            Ok(Value::Null)
        }
    }
}
