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
        if matches!(&call, Call::ReadMigrationStatus {}) {
            static READERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
                std::sync::OnceLock::new();
            let permit = READERS
                .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
                .clone()
                .try_acquire_owned()
                .map_err(|_| "migration_busy")?;
            return tokio::task::spawn_blocking(move || {
                let _permit = permit;
                store_status(&app)
            })
            .await
            .map_err(|_| "migration_unavailable")?;
        }

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

fn store_status(app: &tauri::AppHandle) -> Result<serde_json::Value, &'static str> {
    let owner = app
        .try_state::<crate::commands::PreferenceOwner>()
        .ok_or("preferences_unavailable")?;
    let _guard = owner.0.try_lock().map_err(|_| "preferences_busy")?;
    let preferences = product_contract::launcher_preferences::Preferences::load(
        &crate::commands::preferences_path(app)?,
    )
    .map_err(|_| "preferences_unavailable")?;
    let shortcuts = app
        .try_state::<crate::shortcuts::Owner>()
        .ok_or("shortcut_unavailable")?
        .load(app)
        .map_err(|_| "shortcut_unavailable")?;
    let native =
        serde_json::to_vec(&(preferences, shortcuts)).map_err(|_| "migration_unavailable")?;
    serde_json::to_value(product_contract::migration_status::Summary::new(
        "control-center",
        env!("CARGO_PKG_VERSION"),
        false,
        true,
        false,
        &native,
    )?)
    .map_err(|_| "migration_unavailable")
}
