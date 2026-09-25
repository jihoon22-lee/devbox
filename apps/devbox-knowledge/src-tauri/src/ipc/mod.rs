//! Component-owned typed commands with shared native admission.
pub mod activity;
pub mod commands;
pub mod notes;
pub mod search;
pub mod setup;
#[cfg(windows)]
use tauri::Manager;
pub(crate) fn typed<T: serde::de::DeserializeOwned + serde::Serialize>(
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let parsed: T = serde_json::from_value(value).map_err(|_| "component_response_invalid")?;
    serde_json::to_value(parsed).map_err(|_| "component_response_invalid".into())
}
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("knowledge")
        .invoke_handler(tauri::generate_handler![
            activity::activity,
            notes::notes,
            search::search,
            search::search_settings,
            search::opener,
            setup::setup,
            commands::commands
        ])
        .setup(|app, _| {
            #[cfg(windows)]
            if let (Some(digest), Some(bytes)) = (
                option_env!("DEVBOX_WSL_HELPER_SHA256"),
                option_env!("DEVBOX_WSL_HELPER_BYTES"),
            ) {
                knowledge_vault_engine::component::configure_document_helper(
                    app.path().resource_dir()?.join("resources/wsl"),
                    digest,
                    bytes.parse()?,
                );
            }
            crate::lifecycle::initialize(app);
            crate::startup::initialize(app).map_err(Into::into)
        })
        .on_event(crate::lifecycle::on_event)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::ComponentCall;
    #[test]
    fn components_keep_routes_and_installation_review_boundaries() {
        let notes: notes::KnowledgeNotesCall =
            serde_json::from_str(r#"{"method":"list_tags","args":{}}"#).unwrap();
        assert_eq!(notes.routes(), &["notes", "daily"]);
        let clipboard: notes::KnowledgeNotesCall =
            serde_json::from_str(r#"{"method":"read_clipboard_text","args":{}}"#).unwrap();
        assert!(matches!(clipboard, notes::KnowledgeNotesCall::Host(_)));
        let setup: setup::SetupCall =
            serde_json::from_str(r#"{"method":"start_empty","args":{}}"#).unwrap();
        assert_eq!(setup::SetupCall::COMPONENT, "knowledge.setup");
        assert_eq!(setup.routes(), &["notes"]);
        assert!(commands::QuitCall::INSTALLATION_REVIEW);
    }
    #[test]
    fn search_cannot_read_unowned_files_or_mutate_other_components() {
        for method in ["search_files", "search_content"] {
            let call: search::KnowledgeSearchCall = serde_json::from_value(
                serde_json::json!({"method":method,"args":{"query":"synthetic"}}),
            )
            .unwrap();
            assert!(call.routes().is_empty());
        }
        for method in [
            "add_root",
            "index_now",
            "open_file",
            "write_file",
            "schedule_vault_change",
        ] {
            assert!(serde_json::from_value::<search::KnowledgeSearchCall>(
                serde_json::json!({"method":method,"args":{}})
            )
            .is_err());
        }
    }
    #[test]
    fn old_execute_plugin_is_removed() {
        assert!(!include_str!("../lib.rs").contains("component::plugin"));
    }
}
