//! Explicit per-process connection to this product's exact native package review.
//! The file declaration alone never activates a listener or launches a product.
#[cfg(windows)]
#[path = "platform/mod.rs"]
mod platform;
use product_contract::{Operation, OperationState, Problem, ProblemCode, RouteRequest};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, State, WebviewWindow};

struct Suite {
    product: &'static str,
    #[cfg(windows)]
    state: Arc<Mutex<Link>>,
}
#[cfg(windows)]
#[derive(Default)]
struct Link {
    pending: Option<(
        String,
        Instant,
        Arc<platform::component_scope::CapturedScope>,
    )>,
    approved: Option<Arc<platform::component_scope::CapturedScope>>,
    bus: Option<platform::component_bus::Bus>,
}
#[cfg(windows)]
impl Drop for Link {
    fn drop(&mut self) {
        if let Some(scope) = &self.approved {
            scope.retire();
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    header: RouteRequest,
    method: Method,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Method {
    Status,
    Preview,
    Approve { token: String },
    Disconnect,
    Probe { product: String },
}
#[derive(Serialize)]
struct Response {
    operation: Operation,
    value: serde_json::Value,
}

#[tauri::command]
async fn connection(
    window: WebviewWindow,
    suite: State<'_, Suite>,
    request: Input,
) -> Result<Response, Problem> {
    let provenance = product_shell_tauri::authorize(
        &window,
        &request.header,
        &format!("{}.commands", suite.product),
    )?;
    #[cfg(windows)]
    let result = execute(
        suite.product,
        suite.state.clone(),
        request.method,
        request.header.deadline_ms,
    )
    .await;
    #[cfg(not(windows))]
    let result: Result<serde_json::Value, &'static str> = {
        match request.method {
            Method::Approve { token } => {
                let _ = token;
            }
            Method::Probe { product } => {
                let _ = product;
            }
            _ => {}
        }
        Err("suite_windows_required")
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

#[cfg(windows)]
async fn execute(
    product: &'static str,
    state: Arc<Mutex<Link>>,
    method: Method,
    deadline: u64,
) -> Result<serde_json::Value, &'static str> {
    use platform::{component_bus, component_scope::CapturedScope};
    use serde_json::json;
    match method {
        Method::Status => {
            let state = state.lock().map_err(|_| "suite_busy")?;
            Ok(
                json!({"connected": state.approved.is_some(), "generation": state.approved.as_ref().map(|scope|&scope.id)}),
            )
        }
        Method::Preview => {
            // Capture only a recognized ancestor of our own executable. No
            // renderer/registry executable path is accepted by this boundary.
            let scope = tokio::task::spawn_blocking(move || {
                let image = std::env::current_exe().map_err(|_| "suite_image_unavailable")?;
                let root = image
                    .parent()
                    .ok_or("suite_image_unavailable")?
                    .ancestors()
                    .take(6)
                    .find(|root| root.join("devbox-installation.json").is_file())
                    .ok_or("suite_package_unavailable")?;
                CapturedScope::capture(root, product, &image, env!("CARGO_PKG_VERSION"))
            })
            .await
            .map_err(|_| "suite_review_unavailable")??;
            let token = uuid::Uuid::new_v4().to_string();
            let value = json!({"token": token, "version": scope.manifest.suite_version,
                "installationId": scope.manifest.installation_id, "generation": scope.id,
                "root": scope.review_root(), "products": scope.manifest.members.iter().map(|member|
                    json!({"product":member.product,"available":!scope.issues.contains_key(&member.product)})).collect::<Vec<_>>()});
            state.lock().map_err(|_| "suite_busy")?.pending =
                Some((token, Instant::now(), Arc::new(scope)));
            Ok(value)
        }
        Method::Approve { token } => {
            let mut state = state.lock().map_err(|_| "suite_busy")?;
            if state.bus.is_some() {
                return Err("suite_already_connected");
            }
            let (expected, started, scope) = state.pending.take().ok_or("suite_review_missing")?;
            if token != expected || started.elapsed() > Duration::from_secs(120) {
                return Err("suite_review_expired");
            }
            scope.revalidate()?;
            let bus = component_bus::Bus::start(scope.clone(), product, handler(product)?)?;
            let generation = scope.id.clone();
            state.bus = Some(bus);
            state.approved = Some(scope);
            Ok(json!({"connected":true,"generation":generation}))
        }
        Method::Disconnect => {
            let bus = {
                let mut state = state.lock().map_err(|_| "suite_busy")?;
                if let Some(scope) = state.approved.take() {
                    scope.retire();
                }
                state.pending.take();
                state.bus.take()
            };
            if let Some(mut bus) = bus {
                bus.shutdown().await;
            }
            Ok(json!({"connected":false,"generation":null}))
        }
        Method::Probe {
            product: destination,
        } => {
            let scope = state
                .lock()
                .map_err(|_| "suite_busy")?
                .approved
                .clone()
                .ok_or("suite_review_required")?;
            component_bus::call(
                scope,
                &destination,
                product_contract::transport::Call::Describe {},
                deadline,
            )
            .await
        }
    }
}
#[cfg(windows)]
fn handler(product: &'static str) -> Result<platform::component_bus::Handler, &'static str> {
    use product_contract::{
        command_index::Index,
        transport::{Call, Source},
    };
    let catalog =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?;
    let mut index = Index::catalog(
        &catalog,
        &std::collections::BTreeSet::from([product.into()]),
    )?;
    index.retain_owner(product);
    let index = Arc::new(index);
    Ok(Arc::new(move |_peer, call, _deadline| {
        let index = index.clone();
        Box::pin(async move {
            match call {
                Call::Describe {} => Ok(
                    serde_json::json!({"product":product,"version":env!("CARGO_PKG_VERSION"),"sources":["commands"]}),
                ),
                Call::Query {
                    source: Source::Commands,
                    query,
                    generation,
                    ..
                } => {
                    let result = index.search(&query)?;
                    Ok(
                        serde_json::json!({"generation":generation,"source":"commands","owner":product,"result":result}),
                    )
                }
                Call::PreviewCommand { request } => serde_json::to_value(index.resolve(&request)?)
                    .map_err(|_| "suite_command_invalid"),
                // Opening a route/entity requires a separate destination UI
                // delivery receipt. Metadata support never implies it happened.
                _ => Err("suite_method_unavailable"),
            }
        })
    }))
}

pub(crate) fn plugin(product: &'static str) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("suite")
        .invoke_handler(tauri::generate_handler![connection])
        .setup(move |app, _| {
            app.manage(Suite {
                product,
                #[cfg(windows)]
                state: Arc::new(Mutex::new(Link::default())),
            });
            Ok(())
        })
        .build()
}
