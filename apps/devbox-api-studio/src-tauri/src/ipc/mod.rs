pub mod api;
pub mod knowledge;
pub mod lifecycle;
pub mod mock_draft;
pub mod transforms;
pub mod webhooks;
pub mod workspace;
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("api-studio")
        .setup(|app, _| {
            let store = crate::handoff::initialize(app).map_err(std::io::Error::other)?;
            crate::mock_draft::initialize(app, store.clone()).map_err(std::io::Error::other)?;
            http_client_engine::component::initialize(app, store.clone())
                .map_err(std::io::Error::other)?;
            webhook_host::component::initialize(app).map_err(std::io::Error::other)?;
            toolbox_engine::component::initialize(app, store).map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api::api,
            webhooks::webhooks,
            transforms::transforms
        ])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::{ComponentCall, ExecutionClass};
    #[test]
    fn component_routes_and_control_classes_are_not_renderer_choices() {
        let draft: api::StudioApiCall = serde_json::from_str(
            r#"{"method":"save_knowledge_draft","args":{"output":"synthetic"}}"#,
        )
        .unwrap();
        assert_eq!(draft.routes(), &["requests", "history"]);
        let workspace: api::StudioApiCall =
            serde_json::from_str(r#"{"method":"api_workspace_state","args":{}}"#).unwrap();
        assert!(workspace.routes().contains(&"protocols"));
        let stop: webhooks::StudioWebhookCall =
            serde_json::from_str(r#"{"method":"stop_server","args":{}}"#).unwrap();
        assert_eq!(stop.class(), ExecutionClass::Control);
        let quit: webhooks::StudioWebhookCall =
            serde_json::from_str(r#"{"method":"quit_product","args":{}}"#).unwrap();
        assert_eq!(quit.class(), ExecutionClass::Control);
        assert!(serde_json::from_str::<transforms::StudioTransformCall>(
            r#"{"method":"send_request","args":{}}"#
        )
        .is_err());
    }
}
