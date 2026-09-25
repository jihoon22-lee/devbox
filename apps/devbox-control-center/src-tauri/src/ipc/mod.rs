pub mod delivery;
pub mod tools;
#[cfg(windows)]
use tauri::Manager;
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("control-center")
        .invoke_handler(tauri::generate_handler![tools::tools, delivery::delivery])
        .setup(|app, _| {
            #[cfg(windows)]
            app.manage(crate::updates::Updates::default());
            installation_tools::component::initialize(app).map_err(std::io::Error::other)?;
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::{ComponentCall, ExecutionClass};
    #[test]
    fn delivery_routes_and_cancellation_preserve_the_native_registry() {
        let cancel: delivery::DeliveryCall = serde_json::from_value(
            serde_json::json!({"method":"cancel_suite_update","args":{"id":"a".repeat(64)}}),
        )
        .unwrap();
        assert_eq!(cancel.class(), ExecutionClass::Control);
        assert_eq!(cancel.routes(), &["updates"]);
        let restore: delivery::DeliveryCall =
            serde_json::from_str(r#"{"method":"restore_inventory","args":{}}"#).unwrap();
        assert_eq!(restore.routes(), &["updates", "recovery", "products"]);
        let health: delivery::DeliveryCall = serde_json::from_str(
            r#"{"method":"record_suite_health","args":{"product":"knowledge"}}"#,
        )
        .unwrap();
        assert_eq!(health.routes(), &["updates", "recovery"]);
        assert!(serde_json::from_str::<delivery::DeliveryCall>(
            r#"{"method":"cancel_suite_update","args":{}}"#
        )
        .is_err());
        let audit: tools::ControlToolsCall =
            serde_json::from_str(r#"{"method":"dev_setup_audit","args":{}}"#).unwrap();
        assert_eq!(audit.routes(), &["environment"]);
    }
}
