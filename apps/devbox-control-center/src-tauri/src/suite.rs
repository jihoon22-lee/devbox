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

pub(crate) type DomainHandler = fn(
    tauri::AppHandle,
    product_contract::transport::Call,
    u64,
    Option<product_contract::query::Cancellation>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<serde_json::Value, &'static str>> + Send>,
>;
struct Suite {
    product: &'static str,
    domain: Option<DomainHandler>,
    sources: &'static [product_contract::transport::Source],
    #[cfg(windows)]
    state: Arc<Mutex<Link>>,
}
#[cfg(windows)]
struct Link {
    pending: Option<(
        String,
        Instant,
        Arc<platform::component_scope::CapturedScope>,
    )>,
    approved: Option<Arc<platform::component_scope::CapturedScope>>,
    bus: Option<platform::component_bus::Bus>,
    navigation: Arc<Mutex<product_contract::navigation::Queue>>,
    review_slots: Arc<tokio::sync::Semaphore>,
    queries: Arc<product_contract::query::Queries>,
}
#[cfg(windows)]
impl Default for Link {
    fn default() -> Self {
        Self {
            pending: None,
            approved: None,
            bus: None,
            navigation: Arc::default(),
            review_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            queries: Arc::default(),
        }
    }
}
#[cfg(windows)]
impl Drop for Link {
    fn drop(&mut self) {
        self.queries.cancel_all();
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
    Approve {
        token: String,
    },
    Disconnect,
    Probe {
        product: String,
    },
    Pending,
    Decide {
        id: String,
        revision: String,
        accept: bool,
    },
    Acknowledge {
        id: String,
    },
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
        window.app_handle().clone(),
        suite.domain,
        suite.sources,
        suite.state.clone(),
        request.method,
        request.header.deadline_ms,
        request.header.route,
    )
    .await;
    #[cfg(not(windows))]
    let result: Result<serde_json::Value, &'static str> = {
        let _ = (suite.domain, suite.sources);
        match request.method {
            Method::Approve { token } => {
                let _ = token;
            }
            Method::Probe { product } => {
                let _ = product;
            }
            Method::Decide {
                id,
                revision,
                accept,
            } => {
                let _ = (id, revision, accept);
            }
            Method::Acknowledge { id } => {
                let _ = id;
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
    app: tauri::AppHandle,
    domain: Option<DomainHandler>,
    sources: &'static [product_contract::transport::Source],
    state: Arc<Mutex<Link>>,
    method: Method,
    deadline: u64,
    route: String,
) -> Result<serde_json::Value, &'static str> {
    use platform::{component_bus, component_scope::CapturedScope};
    use serde_json::json;
    match method {
        Method::Pending => {
            let queue = state.lock().map_err(|_| "suite_busy")?.navigation.clone();
            let value = queue.lock().map_err(|_| "suite_busy")?.pending(now());
            Ok(json!(value))
        }
        Method::Decide {
            id,
            revision,
            accept,
        } => {
            let queue = state.lock().map_err(|_| "suite_busy")?.navigation.clone();
            let route =
                queue
                    .lock()
                    .map_err(|_| "suite_busy")?
                    .decide(&id, &revision, accept, now())?;
            Ok(json!({"operationId":id,"route":route}))
        }
        Method::Acknowledge { id } => {
            let queue = state.lock().map_err(|_| "suite_busy")?.navigation.clone();
            let receipt =
                queue
                    .lock()
                    .map_err(|_| "suite_busy")?
                    .acknowledge(&id, &route, now())?;
            Ok(json!(receipt))
        }
        Method::Status => {
            let state = state.lock().map_err(|_| "suite_busy")?;
            Ok(
                json!({"connected": state.approved.is_some(), "generation": state.approved.as_ref().map(|scope|&scope.id)}),
            )
        }
        Method::Preview => {
            // Capture only a recognized ancestor of our own executable. No
            // renderer/registry executable path is accepted by this boundary.
            let slots = state.lock().map_err(|_| "suite_busy")?.review_slots.clone();
            let permit = slots.try_acquire_owned().map_err(|_| "suite_review_busy")?;
            let scope = tokio::task::spawn_blocking(move || {
                let _permit = permit;
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
            let bus = component_bus::Bus::start(
                scope.clone(),
                product,
                handler(
                    product,
                    app,
                    state.navigation.clone(),
                    domain,
                    sources,
                    state.queries.clone(),
                )?,
            )?;
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
                state.queries.cancel_all();
                state.navigation.lock().map_err(|_| "suite_busy")?.revoke();
                state.bus.take()
            };
            if let Some(mut bus) = bus {
                bus.shutdown().await;
            }
            use tauri::Emitter;
            let _ = app.emit("suite-disconnected", ());
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
fn handler(
    product: &'static str,
    app: tauri::AppHandle,
    navigation: Arc<Mutex<product_contract::navigation::Queue>>,
    domain: Option<DomainHandler>,
    sources: &'static [product_contract::transport::Source],
    queries: Arc<product_contract::query::Queries>,
) -> Result<platform::component_bus::Handler, &'static str> {
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
    let routes = Arc::new(
        catalog
            .features
            .iter()
            .filter(|feature| feature.owner == product)
            .map(|feature| feature.route.clone())
            .collect::<std::collections::BTreeSet<_>>(),
    );
    Ok(Arc::new(move |_peer, call, deadline| {
        let index = index.clone();
        let routes = routes.clone();
        let app = app.clone();
        let navigation = navigation.clone();
        let queries = queries.clone();
        Box::pin(async move {
            let cancellation = match &call {
                Call::Query { query_id, .. } => Some(queries.begin(query_id, deadline, now())?),
                Call::CancelQuery { query_id } => {
                    queries.cancel(query_id, now())?;
                    return Ok(serde_json::json!({"state":"cancelRequested"}));
                }
                _ => None,
            };
            if cancellation.as_ref().is_some_and(|token| token.requested()) {
                return Err("query_cancelled");
            }
            match call {
                Call::Describe {} => Ok(
                    serde_json::json!({"product":product,"version":env!("CARGO_PKG_VERSION"),"sources":std::iter::once(Source::Commands).chain(sources.iter().cloned()).collect::<Vec<_>>()}),
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
                Call::PreviewCommand { request } => {
                    let descriptor =
                        resolve(&app, &index, domain, request.clone(), deadline).await?;
                    descriptor.validate_request(&request)?;
                    validate_route_owner(&descriptor, product, &routes)?;
                    serde_json::to_value(descriptor).map_err(|_| "suite_command_invalid")
                }
                Call::OpenCommand { request } => {
                    use tauri::Emitter;
                    let descriptor =
                        resolve(&app, &index, domain, request.clone(), deadline).await?;
                    validate_route_owner(&descriptor, product, &routes)?;
                    let receipt = navigation.lock().map_err(|_| "suite_busy")?.enqueue(
                        &descriptor,
                        &request,
                        now(),
                    )?;
                    if receipt.phase == product_contract::navigation::Phase::AwaitingReview {
                        let window = app
                            .get_webview_window("main")
                            .ok_or("suite_window_unavailable")?;
                        window.show().map_err(|_| "suite_window_unavailable")?;
                        window.set_focus().map_err(|_| "suite_window_unavailable")?;
                        window
                            .emit("suite-navigation", ())
                            .map_err(|_| "suite_window_unavailable")?;
                    }
                    Ok(serde_json::json!(receipt))
                }
                Call::CommandStatus { operation_id } => {
                    let receipt = navigation
                        .lock()
                        .map_err(|_| "suite_busy")?
                        .status(&operation_id, now())?;
                    Ok(serde_json::json!(receipt))
                }
                call => match domain {
                    Some(domain) => domain(app, call, deadline, cancellation).await,
                    None => Err("suite_method_unavailable"),
                },
            }
        })
    }))
}

#[cfg(windows)]
fn validate_route_owner(
    descriptor: &product_contract::commands::Descriptor,
    product: &str,
    routes: &std::collections::BTreeSet<String>,
) -> Result<(), &'static str> {
    let route = match &descriptor.target {
        product_contract::commands::Target::Route { route } => Some(route),
        _ => descriptor.review_route.as_ref(),
    };
    if descriptor.owner != product || route.is_none_or(|route| !routes.contains(route)) {
        return Err("suite_route_owner_mismatch");
    }
    Ok(())
}

#[cfg(windows)]
async fn resolve(
    app: &tauri::AppHandle,
    index: &product_contract::command_index::Index,
    domain: Option<DomainHandler>,
    request: product_contract::commands::Request,
    deadline: u64,
) -> Result<product_contract::commands::Descriptor, &'static str> {
    match index.resolve(&request) {
        Ok(descriptor) => Ok(descriptor.clone()),
        Err(_) => {
            let value = domain.ok_or("suite_command_unavailable")?(
                app.clone(),
                product_contract::transport::Call::PreviewCommand {
                    request: request.clone(),
                },
                deadline,
                None,
            )
            .await?;
            let descriptor: product_contract::commands::Descriptor =
                serde_json::from_value(value).map_err(|_| "suite_command_invalid")?;
            descriptor.validate_request(&request)?;
            Ok(descriptor)
        }
    }
}

#[cfg(windows)]
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
pub(crate) fn connection_ready(app: &tauri::AppHandle) -> bool {
    #[cfg(windows)]
    {
        app.try_state::<Suite>().is_some_and(|suite| {
            suite.state.lock().is_ok_and(|state| {
                state
                    .approved
                    .as_ref()
                    .is_some_and(|scope| scope.revalidate().is_ok())
            })
        })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        false
    }
}
/// Native command host calls this only after its own renderer authorization.
pub(crate) async fn remote(
    app: &tauri::AppHandle,
    product: &str,
    call: product_contract::transport::Call,
    deadline: u64,
) -> Result<serde_json::Value, &'static str> {
    #[cfg(windows)]
    {
        let suite = app.state::<Suite>();
        let scope = suite
            .state
            .lock()
            .map_err(|_| "suite_busy")?
            .approved
            .clone()
            .ok_or("suite_review_required")?;
        platform::component_bus::call(scope, product, call, deadline).await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, product, call, deadline);
        Err("suite_windows_required")
    }
}

pub(crate) fn plugin(
    product: &'static str,
    domain: Option<DomainHandler>,
    sources: &'static [product_contract::transport::Source],
) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("suite")
        .invoke_handler(tauri::generate_handler![connection])
        .setup(move |app, _| {
            app.manage(Suite {
                product,
                domain,
                sources,
                #[cfg(windows)]
                state: Arc::new(Mutex::new(Link::default())),
            });
            Ok(())
        })
        .build()
}
