//! One-time native reviews of preserved Port/Logs settings. Each owner commits
//! its data and receipt atomically; no transaction across owners is claimed.
use super::*;
use crate::core::legacy_inventory::Source;
use log_lens_lib::core::{product_saved_views, runtime_views, saved_views::SavedViewsDocument};
use port_manager_lib::component::{product_preferences, PortManagerPreferences};
use std::time::{Duration, Instant};
pub(super) enum Data {
    Preferences(PortManagerPreferences),
    Views(SavedViewsDocument),
}
pub(super) struct Review {
    token: String,
    component: String,
    snapshot: String,
    revision: u64,
    data: Data,
    expires: Instant,
}
fn revision(app: &tauri::AppHandle, run: &str, imported: bool) -> Option<String> {
    let lease = if imported {
        run_manager_lib::component::imported_log_descriptor(app, run)
    } else {
        run_manager_lib::component::log_descriptor(app, run)
    }
    .ok()?;
    lease.revalidate().ok()?;
    Some(lease.revision().into())
}
pub(super) fn execute(
    app: &tauri::AppHandle,
    host: &Arc<Host>,
    owners: &Owners,
    component: &str,
    method: &str,
    value: Value,
    deadline: u64,
) -> Result<Value> {
    let is_ports = component == "workspace.processes";
    if is_ports {
        owners.initialize_processes(app, host)?;
    } else {
        owners.initialize_logs(app, host)?;
    }
    let root = host.component(if is_ports { "processes" } else { "logs" })?;
    match method {
        "preview_legacy_runtime_settings" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                job_id: String,
            }
            let input: Input = args(value)?;
            let (snapshot, bytes) = host.legacy.runtime_settings_source(
                &input.job_id,
                if is_ports {
                    Source::PortManager
                } else {
                    Source::LogLens
                },
            )?;
            let (data, revision, before, after, unavailable) = if is_ports {
                let data: PortManagerPreferences =
                    serde_json::from_slice(&bytes).map_err(|_| "runtime_settings_invalid")?;
                data.validate().map_err(|_| "runtime_settings_invalid")?;
                let current = product_preferences::load(&root)?;
                let before = json!({"favorites":current.preferences.favorite_ports.len()+current.preferences.favorite_processes.len(),"refreshIntervalMs":current.preferences.refresh_interval_ms});
                let after = json!({"favorites":data.favorite_ports.len()+data.favorite_processes.len(),"refreshIntervalMs":data.refresh_interval_ms});
                (Data::Preferences(data), current.revision, before, after, 0)
            } else {
                let mut data: SavedViewsDocument =
                    serde_json::from_slice(&bytes).map_err(|_| "runtime_settings_invalid")?;
                data.validate().map_err(|_| "runtime_settings_invalid")?;
                let mut missing = 0;
                for view in &mut data.views {
                    missing += runtime_views::rebase(
                        &mut view.sources,
                        &mut view.filter,
                        |run, imported| revision(app, run, imported),
                    )?;
                }
                data.validate().map_err(|_| "runtime_settings_invalid")?;
                let current = product_saved_views::load(&root)?;
                let before = json!({"views":current.views.views.len()});
                let after = json!({"views":data.views.len()});
                (
                    Data::Views(data),
                    current.views.revision,
                    before,
                    after,
                    missing,
                )
            };
            crate::files_host::current_deadline(deadline)?;
            let token = uuid::Uuid::new_v4().simple().to_string();
            *owners
                .settings
                .lock()
                .map_err(|_| "runtime_settings_busy")? = Some(Review {
                token: token.clone(),
                component: component.into(),
                snapshot,
                revision,
                data,
                expires: Instant::now() + Duration::from_secs(120),
            });
            Ok(
                json!({"token":token,"before":before,"after":after,"unavailableSources":unavailable}),
            )
        }
        "apply_legacy_runtime_settings" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                token: String,
                replace: bool,
            }
            let input: Input = args(value)?;
            let mut slot = owners
                .settings
                .lock()
                .map_err(|_| "runtime_settings_busy")?;
            if !slot.as_ref().is_some_and(|review| {
                review.token == input.token
                    && review.component == component
                    && review.expires > Instant::now()
            }) {
                return Err("runtime_settings_stale");
            }
            let review = slot.take().ok_or("runtime_settings_stale")?;
            crate::files_host::current_deadline(deadline)?;
            let already = match review.data {
                Data::Preferences(data) => product_preferences::import(
                    &root,
                    review.revision,
                    &review.snapshot,
                    data,
                    input.replace,
                )?,
                Data::Views(data) => product_saved_views::import(
                    &root,
                    review.revision,
                    &review.snapshot,
                    data,
                    input.replace,
                )?,
            };
            host.component(if is_ports { "processes" } else { "logs" })?;
            Ok(json!({"alreadyImported":already}))
        }
        "reconnect_runtime_sources" if !is_ports => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                sources: Vec<SourceSpec>,
                filter: log_lens_lib::core::FilterSpec,
            }
            let mut input: Input = args(value)?;
            log_lens_lib::core::validate_source_list(&input.sources)
                .map_err(|_| "runtime_settings_invalid")?;
            input
                .filter
                .validate()
                .map_err(|_| "runtime_settings_invalid")?;
            let unavailable =
                runtime_views::rebase(&mut input.sources, &mut input.filter, |run, imported| {
                    revision(app, run, imported)
                })?;
            crate::files_host::current_deadline(deadline)?;
            Ok(
                json!({"sources":input.sources,"filter":input.filter,"unavailableSources":unavailable}),
            )
        }
        _ => Err("invalid_request"),
    }
}
