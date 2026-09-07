use crate::core::mock_draft::Receiver;
use serde::Deserialize;
use serde_json::Value;
use tauri::Manager;
pub const COMMANDS: &[&str] = &["peek_mock_draft", "accept_mock_draft", "discard_mock_draft"];
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
pub fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    let state = app.state::<Receiver>();
    if method == "peek_mock_draft" && args.as_object().is_some_and(|v| v.is_empty()) {
        return serde_json::to_value(state.preview(now()?)?)
            .map_err(|_| "mock_draft_unavailable".into());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        id: String,
    }
    let Input { id } = serde_json::from_value(args).map_err(|_| "mock_draft_invalid")?;
    if !matches!(method, "accept_mock_draft" | "discard_mock_draft") {
        return Err("mock_draft_invalid".into());
    }
    let rule = state.finish(&id, now()?)?;
    // Explicit discard consumes the publication too; it cannot be replayed.
    if method == "discard_mock_draft" {
        return Ok(Value::Null);
    }
    serde_json::to_value(rule).map_err(|_| "mock_draft_unavailable".into())
}
