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
    let value = index
        .resolve(&request.command)
        .map_err(|_| Problem {
            code: ProblemCode::Unavailable,
            provenance: provenance.clone(),
        })?
        .clone();
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
        .invoke_handler(tauri::generate_handler![command_search, command_preview])
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
