//! Other reviewed products use the same native shortcut owner/settings surface.
use product_contract::{query::Cancellation, transport::Call};
use tauri::Manager;
pub(crate) fn handle(
    app: tauri::AppHandle,
    call: Call,
    _deadline: u64,
    _cancel: Option<Cancellation>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<serde_json::Value, &'static str>> + Send>,
> {
    Box::pin(async move {
        if matches!(
            &call,
            Call::ReadOperations {} | Call::ReviewOperation { .. }
        ) {
            return crate::suite::project_operations(
                &app,
                &call,
                app.state::<crate::command_receipts::Owner>()
                    .operation_rows()?,
            );
        }
        static WORKERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
            std::sync::OnceLock::new();
        let permit = WORKERS
            .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| "shortcut_busy")?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if !crate::suite::connection_ready(&app) {
                return Err("suite_review_required");
            }
            let owner = app
                .try_state::<crate::shortcuts::Owner>()
                .ok_or("shortcut_unavailable")?;
            let status = match call {
                Call::ShortcutStatus {} => owner.load(&app),
                Call::ConfigureShortcuts { config } => owner.configure(&app, config),
                _ => return Err("suite_method_unavailable"),
            }
            .map_err(|_| "shortcut_unavailable")?;
            serde_json::to_value(status).map_err(|_| "shortcut_unavailable")
        })
        .await
        .map_err(|_| "shortcut_unavailable")?
    })
}
