//! Local command catalog boundary. Cross-product resolution remains a native owner task.
use crate::core::commands::{Index, Search};
use product_contract::{
    commands::{Descriptor, Request},
    Operation, OperationState, Problem, ProblemCode, RouteRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
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
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusRequest {
    header: RouteRequest,
    product: String,
    operation_id: String,
}
#[derive(Serialize)]
struct Response<T> {
    operation: Operation,
    value: T,
}

#[tauri::command]
async fn command_search(
    window: WebviewWindow,
    index: State<'_, Index>,
    request: SearchRequest,
) -> Result<Response<Search>, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.commands")?;
    let value = index.search(&request.query).map_err(|_| Problem {
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
            source: product_contract::transport::Source::Commands,
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

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("commands")
        .invoke_handler(tauri::generate_handler![
            command_search,
            command_preview,
            command_source,
            command_open,
            command_status
        ])
        .setup(|app, _| {
            let catalog =
                devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)
                    .map_err(std::io::Error::other)?;
            let index = Index::catalog(&catalog, &BTreeSet::from(["control-center".into()]))
                .map_err(std::io::Error::other)?;
            app.manage(index);
            Ok(())
        })
        .build()
}
