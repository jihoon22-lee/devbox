//! Local command catalog boundary. Cross-product resolution remains a native owner task.
use crate::core::commands::{Index, Search};
use product_contract::launcher_preferences::{Preferences, PREFERENCES_FILE};
use product_contract::{
    commands::{Descriptor, Request},
    Operation, OperationState, Problem, ProblemCode, RouteRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Mutex;
use tauri::{Manager, State, WebviewWindow};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchRequest {
    header: RouteRequest,
    query: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewRequest {
    header: RouteRequest,
    command: Request,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceRequest {
    header: RouteRequest,
    product: String,
    query: String,
    generation: u64,
    source: Option<product_contract::transport::Source>,
    mode: Option<product_contract::transport::QueryMode>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusRequest {
    header: RouteRequest,
    product: String,
    operation_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreferenceRequest {
    header: RouteRequest,
    action: PreferenceAction,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum PreferenceAction {
    Read,
    ClearRecents,
    Favorite { command: Request, favorite: bool },
    Visit { command: Request },
}
#[derive(Default)]
struct PreferenceOwner(Mutex<()>);
fn preferences_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, &'static str> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "preferences_unavailable")?;
    let parent = root.parent().ok_or("preferences_unavailable")?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| "preferences_unavailable")?;
    match std::fs::create_dir(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("preferences_unavailable"),
    }
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "preferences_unavailable")?;
    Ok(root.join(PREFERENCES_FILE))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShortcutRequest {
    header: RouteRequest,
    config: Option<crate::shortcuts::Config>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelRequest {
    header: RouteRequest,
    product: String,
    query_id: String,
}
#[derive(Serialize)]
struct Response<T> {
    operation: Operation,
    value: T,
}

#[tauri::command]
async fn command_search(
    window: WebviewWindow,
    request: SearchRequest,
) -> Result<Response<Search>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let catalog = devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE);
    let index = catalog.and_then(|catalog| {
        Index::catalog(
            &catalog,
            &crate::suite::installed_products(window.app_handle()),
        )
    });
    let value = index
        .and_then(|index| index.search(&request.query))
        .map_err(|_| Problem {
            code: ProblemCode::InvalidRequest,
            provenance: provenance.clone(),
        })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

#[tauri::command]
async fn command_preview(
    window: WebviewWindow,
    index: State<'_, Index>,
    request: PreviewRequest,
) -> Result<Response<Descriptor>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let owner = request
        .command
        .command_id
        .split('.')
        .next()
        .unwrap_or("")
        .to_owned();
    let result = if owner == "control-center" {
        index.resolve(&request.command).cloned()
    } else {
        match crate::suite::remote(
            window.app_handle(),
            &owner,
            product_contract::transport::Call::PreviewCommand {
                request: request.command.clone(),
            },
            request.header.deadline_ms,
        )
        .await
        {
            Ok(value) => serde_json::from_value::<Descriptor>(value).map_err(|_| "command_invalid"),
            Err(error) => Err(error),
        }
    };
    let value = result
        .and_then(|descriptor| {
            if descriptor.owner != owner {
                return Err("command_owner_mismatch");
            }
            descriptor.validate_request(&request.command)?;
            Ok(descriptor)
        })
        .map_err(|_| Problem {
            code: ProblemCode::Unavailable,
            provenance: provenance.clone(),
        })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

#[tauri::command]
async fn command_source(
    window: WebviewWindow,
    request: SourceRequest,
) -> Result<Response<serde_json::Value>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let value = crate::suite::remote(
        window.app_handle(),
        &request.product,
        product_contract::transport::Call::Query {
            query_id: request.header.request_id.clone(),
            mode: request.mode.unwrap_or_default(),
            source: request
                .source
                .unwrap_or(product_contract::transport::Source::Commands),
            query: request.query,
            generation: request.generation,
            context: None,
        },
        request.header.deadline_ms,
    )
    .await
    .map_err(|_| Problem {
        code: ProblemCode::Unavailable,
        provenance: provenance.clone(),
    })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}
#[tauri::command]
async fn command_open(
    window: WebviewWindow,
    request: PreviewRequest,
) -> Result<Response<product_contract::navigation::Receipt>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let owner = request
        .command
        .command_id
        .split('.')
        .next()
        .unwrap_or("")
        .to_owned();
    let operation_id = request.command.operation_id.clone();
    let value = crate::suite::remote(
        window.app_handle(),
        &owner,
        product_contract::transport::Call::OpenCommand {
            request: request.command,
        },
        request.header.deadline_ms,
    )
    .await
    .and_then(|value| {
        serde_json::from_value::<product_contract::navigation::Receipt>(value)
            .map_err(|_| "command_reply_invalid")
    })
    .and_then(|receipt| {
        if receipt.operation_id == operation_id {
            Ok(receipt)
        } else {
            Err("command_reply_mismatch")
        }
    })
    .map_err(|_| Problem {
        code: ProblemCode::Unavailable,
        provenance: provenance.clone(),
    })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}
#[tauri::command]
async fn command_status(
    window: WebviewWindow,
    request: StatusRequest,
) -> Result<Response<product_contract::navigation::Receipt>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let value = crate::suite::remote(
        window.app_handle(),
        &request.product,
        product_contract::transport::Call::CommandStatus {
            operation_id: request.operation_id.clone(),
        },
        request.header.deadline_ms,
    )
    .await
    .and_then(|value| {
        serde_json::from_value::<product_contract::navigation::Receipt>(value)
            .map_err(|_| "command_reply_invalid")
    })
    .and_then(|receipt| {
        if receipt.operation_id == request.operation_id {
            Ok(receipt)
        } else {
            Err("command_reply_mismatch")
        }
    })
    .map_err(|_| Problem {
        code: ProblemCode::Unavailable,
        provenance: provenance.clone(),
    })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

#[tauri::command]
async fn command_preferences(
    window: WebviewWindow,
    index: State<'_, Index>,
    owner: State<'_, PreferenceOwner>,
    request: PreferenceRequest,
) -> Result<Response<Preferences>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let failure = || Problem {
        code: ProblemCode::Unavailable,
        provenance: provenance.clone(),
    };
    let change = match request.action {
        PreferenceAction::Favorite {
            command,
            favorite: false,
        } => {
            product_contract::launcher_preferences::validate_result_id(&command.command_id)
                .map_err(|_| failure())?;
            Some((command.command_id, Some(false)))
        }
        PreferenceAction::Favorite { command, favorite } => {
            let destination = command
                .command_id
                .split('.')
                .next()
                .unwrap_or("")
                .to_owned();
            let descriptor = if destination == "control-center" {
                index.resolve(&command).cloned().map_err(|_| failure())?
            } else {
                let value = crate::suite::remote(
                    window.app_handle(),
                    &destination,
                    product_contract::transport::Call::PreviewCommand {
                        request: command.clone(),
                    },
                    request.header.deadline_ms,
                )
                .await
                .map_err(|_| failure())?;
                serde_json::from_value::<Descriptor>(value).map_err(|_| failure())?
            };
            descriptor
                .validate_request(&command)
                .map_err(|_| failure())?;
            Some((command.command_id, Some(favorite)))
        }
        PreferenceAction::Visit { command } => {
            let descriptor = index.resolve(&command).map_err(|_| failure())?;
            if !matches!(&descriptor.target,product_contract::commands::Target::Route{route} if route==&request.header.route)
                || descriptor.owner != "control-center"
            {
                return Err(failure());
            }
            Some((command.command_id, None))
        }
        PreferenceAction::ClearRecents => Some((String::new(), None)),
        PreferenceAction::Read => None,
    };
    let _guard = owner.0.lock().map_err(|_| failure())?;
    let path = preferences_path(window.app_handle()).map_err(|_| failure())?;
    let mut preferences = Preferences::load(&path).map_err(|_| failure())?;
    if let Some((id, favorite)) = change {
        match favorite {
            Some(favorite) => preferences.set_favorite(&id, favorite),
            None if id.is_empty() => {
                preferences.clear_recents();
                Ok(())
            }
            None => preferences.record_recent(&id),
        }
        .map_err(|_| failure())?;
        preferences.save(&path).map_err(|_| failure())?;
    }
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value: preferences,
    })
}

#[tauri::command]
async fn command_shortcut(
    window: WebviewWindow,
    owner: State<'_, crate::shortcuts::Owner>,
    request: ShortcutRequest,
) -> Result<Response<crate::shortcuts::View>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let result = if let Some(config) = request.config {
        if config.enabled && !crate::suite::connection_ready(window.app_handle()) {
            Err("suite_review_required".into())
        } else {
            owner.configure(window.app_handle(), config)
        }
    } else {
        owner.load(window.app_handle())
    };
    let value = result.map_err(|_| Problem {
        code: ProblemCode::Unavailable,
        provenance: provenance.clone(),
    })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TriggerRequest {
    header: RouteRequest,
    command: String,
    operation_id: String,
}
#[tauri::command]
async fn command_trigger_shortcut(
    window: WebviewWindow,
    request: TriggerRequest,
) -> Result<Response<serde_json::Value>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let result = async {
        if uuid::Uuid::parse_str(&request.operation_id).is_err() {
            return Err("shortcut_operation_invalid");
        }
        let call = product_contract::transport::Call::ResolveShortcut {
            command: request.command.clone(),
        };
        product_contract::transport::validate_call(&call)?;
        let product = request
            .command
            .split('.')
            .next()
            .ok_or("shortcut_command_invalid")?;
        let value = crate::suite::remote(
            window.app_handle(),
            product,
            call,
            request.header.deadline_ms,
        )
        .await?;
        let descriptor: Descriptor =
            serde_json::from_value(value).map_err(|_| "shortcut_command_invalid")?;
        if descriptor.owner != product {
            return Err("shortcut_owner_invalid");
        }
        let command = Request {
            operation_id: request.operation_id,
            command_id: descriptor.id.clone(),
            revision: descriptor.revision.clone(),
            context: descriptor.context.clone(),
            selection_id: None,
        };
        descriptor.validate_request(&command)?;
        crate::suite::remote(
            window.app_handle(),
            product,
            product_contract::transport::Call::OpenCommand { request: command },
            request.header.deadline_ms,
        )
        .await
    }
    .await;
    let value = result.map_err(|_| {
        let _ = window.show();
        let _ = window.set_focus();
        Problem {
            code: ProblemCode::Unavailable,
            provenance: provenance.clone(),
        }
    })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

#[tauri::command]
async fn command_cancel(
    window: WebviewWindow,
    request: CancelRequest,
) -> Result<Response<serde_json::Value>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let value = crate::suite::remote(
        window.app_handle(),
        &request.product,
        product_contract::transport::Call::CancelQuery {
            query_id: request.query_id,
        },
        request.header.deadline_ms,
    )
    .await
    .map_err(|_| Problem {
        code: ProblemCode::Unavailable,
        provenance: provenance.clone(),
    })?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("commands")
        .invoke_handler(tauri::generate_handler![
            command_search,
            command_preview,
            command_source,
            command_cancel,
            command_open,
            command_status,
            command_preferences,
            command_shortcut,
            command_trigger_shortcut
        ])
        .setup(|app, _| {
            let catalog =
                devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)
                    .map_err(std::io::Error::other)?;
            let index = Index::catalog(&catalog, &BTreeSet::from(["control-center".into()]))
                .map_err(std::io::Error::other)?;
            app.manage(index);
            app.manage(PreferenceOwner::default());
            app.manage(crate::shortcuts::Owner::default());
            use tauri::Listener;
            let handle = app.clone();
            app.listen("suite-disconnected", move |_| {
                handle.state::<crate::shortcuts::Owner>().stop();
            });
            let handle = app.clone();
            app.listen("suite-connected", move |_| {
                let handle = handle.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = handle.state::<crate::shortcuts::Owner>().resume(&handle);
                });
            });
            Ok(())
        })
        .on_event(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<crate::shortcuts::Owner>().stop();
            }
        })
        .build()
}
