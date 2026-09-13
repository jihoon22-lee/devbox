//! Explicit per-process connection to this product's exact native package review.
//! The file declaration alone never activates a listener or launches a product.
#[cfg(windows)]
#[path = "platform/mod.rs"]
pub(crate) mod platform;
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
    epoch: u64,
    remembered: bool,
    launches: std::collections::BTreeMap<String, Arc<tokio::sync::Mutex<()>>>,
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
            epoch: 0,
            remembered: false,
            launches: product_contract::installation::PRODUCTS
                .iter()
                .map(|product| (product.to_string(), Arc::new(tokio::sync::Mutex::new(()))))
                .collect(),
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
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Method {
    ReadOperations {
        product: String,
    },
    ReviewOperation {
        product: String,
        id: String,
        revision: String,
        operation_id: String,
    },
    Status,
    Preview,
    Approve {
        token: String,
        #[serde(default)]
        remember: bool,
    },
    Disconnect,
    Probe {
        product: String,
    },
    SendSessionSummary {
        source_id: String,
        operation_id: String,
    },
    OpenReceivedFile {
        reference: String,
    },
    ShortcutStatus,
    ConfigureShortcuts {
        config: product_contract::shortcuts::Config,
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
            Method::ReadOperations { product } => {
                let _ = product;
            }
            Method::ReviewOperation {
                product,
                id,
                revision,
                operation_id,
            } => {
                let _ = (product, id, revision, operation_id);
            }
            Method::OpenReceivedFile { reference } => {
                let _ = reference;
            }
            Method::SendSessionSummary {
                source_id,
                operation_id,
            } => {
                let _ = (source_id, operation_id);
            }
            Method::ConfigureShortcuts { config } => {
                let _ = config;
            }
            Method::Approve { token, remember } => {
                let _ = (token, remember);
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
fn capture_own(product: &str) -> Result<platform::component_scope::CapturedScope, &'static str> {
    let image = std::env::current_exe().map_err(|_| "suite_image_unavailable")?;
    let root = image
        .parent()
        .ok_or("suite_image_unavailable")?
        .ancestors()
        .take(6)
        .find(|root| root.join("devbox-installation.json").is_file())
        .ok_or("suite_package_unavailable")?;
    platform::component_scope::CapturedScope::capture(
        root,
        product,
        &image,
        env!("CARGO_PKG_VERSION"),
    )
}
#[cfg(windows)]
async fn resume(
    app: tauri::AppHandle,
    product: &'static str,
    domain: Option<DomainHandler>,
    sources: &'static [product_contract::transport::Source],
    state: Arc<Mutex<Link>>,
) {
    let Ok(permit) = state.lock().map_err(|_| ()).and_then(|state| {
        state
            .review_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ())
    }) else {
        return;
    };
    let storage_app = app.clone();
    let captured = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let Some(preference) = platform::connection_preference::read(&storage_app)? else {
            return Ok::<_, &'static str>(None);
        };
        let scope = capture_own(product)?;
        if !preference.matches(product, &scope) {
            return Err("suite_review_required");
        }
        Ok(Some(Arc::new(scope)))
    })
    .await;
    let Ok(Ok(Some(scope))) = captured else {
        return;
    };
    // A manual review/disconnect wins over a late startup capture.
    let Ok(mut state) = state.lock() else {
        return;
    };
    if state.epoch != 0 || state.approved.is_some() {
        return;
    }
    let Ok(handler) = handler(
        product,
        app.clone(),
        state.navigation.clone(),
        domain,
        sources,
        state.queries.clone(),
    ) else {
        return;
    };
    let Ok(bus) = platform::component_bus::Bus::start(scope.clone(), product, handler) else {
        return;
    };
    state.approved = Some(scope);
    state.bus = Some(bus);
    state.remembered = true;
    drop(state);
    use tauri::Emitter;
    let _ = app.emit("suite-connected", ());
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
    use platform::component_bus;
    use serde_json::json;
    match method {
        Method::ReadOperations { product: target } => {
            if target == product {
                match domain {
                    Some(domain) => {
                        domain(
                            app,
                            product_contract::transport::Call::ReadOperations {},
                            deadline,
                            None,
                        )
                        .await
                    }
                    None => Ok(serde_json::json!([])),
                }
            } else if product == "control-center" {
                remote(
                    &app,
                    &target,
                    product_contract::transport::Call::ReadOperations {},
                    deadline,
                )
                .await
            } else {
                Err("suite_operation_denied")
            }
        }
        Method::ReviewOperation {
            product: target,
            id,
            revision,
            operation_id,
        } => {
            let call = product_contract::transport::Call::ReviewOperation {
                id,
                revision,
                operation_id,
            };
            if target == product {
                domain.ok_or("suite_method_unavailable")?(app, call, deadline, None).await
            } else if product == "control-center" {
                remote(&app, &target, call, deadline).await
            } else {
                Err("suite_operation_denied")
            }
        }
        Method::OpenReceivedFile { reference } => {
            if product != "workspace" {
                return Err("suite_file_denied");
            }
            domain.ok_or("suite_method_unavailable")?(
                app.clone(),
                product_contract::transport::Call::ReadFileReference { reference },
                deadline,
                None,
            )
            .await
        }
        Method::SendSessionSummary {
            source_id,
            operation_id,
        } => {
            if product != "workspace"
                || uuid::Uuid::parse_str(&source_id).is_err()
                || uuid::Uuid::parse_str(&operation_id).is_err()
            {
                return Err("suite_summary_denied");
            }
            let value = domain.ok_or("suite_method_unavailable")?(
                app.clone(),
                product_contract::transport::Call::ReadSessionSummary {
                    source_id: source_id.clone(),
                },
                deadline,
                None,
            )
            .await?;
            use sha2::{Digest, Sha256};
            let metadata: product_contract::session_summary::Metadata =
                serde_json::from_value(value).map_err(|_| "suite_summary_invalid")?;
            let revision =
                Sha256::digest(serde_json::to_vec(&metadata).map_err(|_| "suite_summary_invalid")?)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
            remote(
                &app,
                "knowledge",
                product_contract::transport::Call::DeliverSessionSummary {
                    source_id,
                    operation_id,
                    revision,
                },
                deadline,
            )
            .await
        }
        Method::ShortcutStatus | Method::ConfigureShortcuts { .. } => {
            let call = match method {
                Method::ConfigureShortcuts { config } => {
                    product_contract::transport::Call::ConfigureShortcuts { config }
                }
                _ => product_contract::transport::Call::ShortcutStatus {},
            };
            remote(&app, "control-center", call, deadline).await
        }
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
                json!({"connected": state.approved.is_some(), "generation": state.approved.as_ref().map(|scope|&scope.id),"remembered":state.remembered}),
            )
        }
        Method::Preview => {
            // Capture only a recognized ancestor of our own executable. No
            // renderer/registry executable path is accepted by this boundary.
            let slots = state.lock().map_err(|_| "suite_busy")?.review_slots.clone();
            let permit = slots.try_acquire_owned().map_err(|_| "suite_review_busy")?;
            let scope = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                capture_own(product)
            })
            .await
            .map_err(|_| "suite_review_unavailable")??;
            let token = uuid::Uuid::new_v4().to_string();
            let value = json!({"token": token, "version": scope.manifest.suite_version,
                "installationId": scope.manifest.installation_id, "generation": scope.id,
                "root": scope.review_root(), "products": scope.manifest.members.iter().map(|member|
                    json!({"product":member.product,"available":!scope.issues.contains_key(&member.product)})).collect::<Vec<_>>()});
            let mut state = state.lock().map_err(|_| "suite_busy")?;
            state.epoch = state.epoch.wrapping_add(1);
            state.pending = Some((token, Instant::now(), Arc::new(scope)));
            Ok(value)
        }
        Method::Approve { token, remember } => tokio::task::spawn_blocking(move || {
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
                    app.clone(),
                    state.navigation.clone(),
                    domain,
                    sources,
                    state.queries.clone(),
                )?,
            )?;
            platform::connection_preference::write(
                &app,
                product,
                remember.then_some(scope.as_ref()),
            )?;
            state.epoch = state.epoch.wrapping_add(1);
            state.remembered = remember;
            let generation = scope.id.clone();
            state.bus = Some(bus);
            state.approved = Some(scope);
            drop(state);
            use tauri::Emitter;
            let _ = app.emit("suite-connected", ());
            Ok(json!({"connected":true,"generation":generation,"remembered":remember}))
        })
        .await
        .map_err(|_| "suite_worker_unavailable")?,
        Method::Disconnect => {
            let storage_app = app.clone();
            let bus = tokio::task::spawn_blocking(move || {
                let mut state = state.lock().map_err(|_| "suite_busy")?;
                platform::connection_preference::write(&storage_app, product, None)?;
                state.epoch = state.epoch.wrapping_add(1);
                state.remembered = false;
                if let Some(scope) = state.approved.take() {
                    scope.retire();
                }
                state.pending.take();
                state.queries.cancel_all();
                state.navigation.lock().map_err(|_| "suite_busy")?.revoke();
                Ok::<_, &'static str>(state.bus.take())
            })
            .await
            .map_err(|_| "suite_worker_unavailable")??;
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
                    query_id,
                    source: Source::Commands,
                    query,
                    generation,
                    context,
                    mode,
                } => {
                    let mut combined = (*index).clone();
                    if let Some(domain) = domain {
                        if let Ok(extra) = domain(
                            app.clone(),
                            Call::Query {
                                query_id,
                                source: Source::Commands,
                                query: query.clone(),
                                generation,
                                context,
                                mode,
                            },
                            deadline,
                            cancellation,
                        )
                        .await
                        {
                            let rows: Vec<product_contract::commands::Descriptor> =
                                serde_json::from_value(extra["result"]["results"].clone())
                                    .map_err(|_| "suite_source_invalid")?;
                            if rows.len() > 256 {
                                return Err("suite_source_limit");
                            }
                            for row in rows {
                                validate_route_owner(&row, product, &routes)?;
                                combined.insert(row)?;
                            }
                        }
                    }
                    Ok(
                        serde_json::json!({"generation":generation,"source":"commands","owner":product,"result":combined.search(&query)?}),
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
                    if !descriptor.requires_review
                        && matches!(
                            descriptor.target,
                            product_contract::commands::Target::Entity {
                                entity: product_contract::commands::EntityKind::TerminalWindow,
                                ..
                            }
                        )
                    {
                        let (fresh, receipt) = navigation
                            .lock()
                            .map_err(|_| "suite_busy")?
                            .begin_terminal_action(&descriptor, &request, now())?;
                        if !fresh {
                            return Ok(serde_json::json!(receipt));
                        }
                        domain.ok_or("suite_method_unavailable")?(
                            app,
                            Call::OpenCommand {
                                request: request.clone(),
                            },
                            deadline,
                            None,
                        )
                        .await?;
                        return Ok(serde_json::json!(navigation
                            .lock()
                            .map_err(|_| "suite_busy")?
                            .finish_terminal_action(&request.operation_id)?));
                    }
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
#[allow(dead_code)]
pub(crate) fn installed_products(app: &tauri::AppHandle) -> std::collections::BTreeSet<String> {
    #[allow(unused_mut)]
    let mut products = std::collections::BTreeSet::from(["control-center".to_owned()]);
    #[cfg(windows)]
    {
        if let Some(suite) = app.try_state::<Suite>() {
            if let Ok(state) = suite.state.lock() {
                if let Some(scope) = &state.approved {
                    if scope.revalidate().is_ok() {
                        products.extend(
                            scope
                                .manifest
                                .members
                                .iter()
                                .filter(|member| scope.member(&member.product).is_ok())
                                .map(|member| member.product.clone()),
                        );
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = app;
    products
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
        if matches!(
            call,
            product_contract::transport::Call::PreviewCommand { .. }
                | product_contract::transport::Call::OpenCommand { .. }
                | product_contract::transport::Call::ResolveShortcut { .. }
                | product_contract::transport::Call::ShortcutStatus { .. }
                | product_contract::transport::Call::ConfigureShortcuts { .. }
                | product_contract::transport::Call::DeliverSessionSummary { .. }
                | product_contract::transport::Call::DeliverFileReference { .. }
                | product_contract::transport::Call::DeliverKnowledgeDraft { .. }
                | product_contract::transport::Call::DeliverTransformSelection { .. }
        ) {
            let launch = suite
                .state
                .lock()
                .map_err(|_| "suite_busy")?
                .launches
                .get(product)
                .cloned()
                .ok_or("suite_product_invalid")?;
            let _guard = tokio::time::timeout(
                Duration::from_millis(deadline.saturating_sub(now())),
                launch.lock(),
            )
            .await
            .map_err(|_| "suite_activation_timeout")?;
            activate(scope.clone(), product, deadline).await?;
        }
        // The actual command is sent once. A lost reply remains an unknown
        // operation receipt; only read-only readiness probes are retried.
        platform::component_bus::call(scope, product, call, deadline).await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, product, call, deadline);
        Err("suite_windows_required")
    }
}

#[cfg(windows)]
async fn activate(
    scope: Arc<platform::component_scope::CapturedScope>,
    product: &str,
    deadline: u64,
) -> Result<(), &'static str> {
    use product_contract::transport::Call;
    match platform::component_bus::call(scope.clone(), product, Call::Describe {}, deadline).await {
        Ok(_) => return Ok(()),
        Err("peer_provider_unavailable") => {}
        Err(error) => return Err(error),
    }
    let launch_scope = scope.clone();
    let destination = product.to_owned();
    let mut child = tokio::task::spawn_blocking(move || {
        use std::os::windows::process::CommandExt;
        if now() >= deadline {
            return Err("suite_activation_timeout");
        }
        launch_scope.revalidate()?;
        let (_, image, _) = launch_scope.member(&destination)?;
        let child = std::process::Command::new(image)
            .current_dir(image.parent().ok_or("suite_image_unavailable")?)
            // A product's WebView profile/debug override belongs to that
            // process. Inheriting it would reopen the sender's browser store.
            .env_remove("WEBVIEW2_USER_DATA_FOLDER")
            .env_remove("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| "suite_activation_unavailable")?;
        Ok(child)
    })
    .await
    .map_err(|_| "suite_activation_unavailable")??;
    while now() < deadline {
        // Readiness verifies the actual server PID/image and native session each time.
        match platform::component_bus::call(scope.clone(), product, Call::Describe {}, deadline)
            .await
        {
            Ok(_) => return Ok(()),
            Err("peer_provider_unavailable") => {}
            Err(error) => return Err(error),
        }
        if child
            .try_wait()
            .map_err(|_| "suite_activation_unavailable")?
            .is_some_and(|status| !status.success())
        {
            return Err("suite_activation_unavailable");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("suite_activation_timeout")
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
            #[cfg(windows)]
            {
                let state = app.state::<Suite>().state.clone();
                tauri::async_runtime::spawn(resume(app.clone(), product, domain, sources, state));
            }
            Ok(())
        })
        .build()
}

/// Shared native adapter used by product-specific incoming Artifact consumers.
#[allow(dead_code)]
pub(crate) fn enqueue_review(
    app: &tauri::AppHandle,
    descriptor: &product_contract::commands::Descriptor,
    request: &product_contract::commands::Request,
) -> Result<serde_json::Value, &'static str> {
    #[cfg(windows)]
    {
        use tauri::Emitter;
        let suite = app.state::<Suite>();
        if descriptor.owner != suite.product {
            return Err("suite_route_owner_mismatch");
        }
        let queue = suite
            .state
            .lock()
            .map_err(|_| "suite_busy")?
            .navigation
            .clone();
        let receipt =
            queue
                .lock()
                .map_err(|_| "suite_busy")?
                .enqueue(descriptor, request, now())?;
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
        serde_json::to_value(receipt).map_err(|_| "suite_reply_invalid")
    }
    #[cfg(not(windows))]
    {
        let _ = (app, descriptor, request);
        Err("suite_windows_required")
    }
}
#[allow(dead_code)]
pub(crate) fn require_reviewed(
    app: &tauri::AppHandle,
    id: &str,
    revision: &str,
    route: &str,
    target: &product_contract::commands::Target,
) -> Result<(), &'static str> {
    #[cfg(windows)]
    {
        let suite = app.state::<Suite>();
        let queue = suite
            .state
            .lock()
            .map_err(|_| "suite_busy")?
            .navigation
            .clone();
        let result = queue.lock().map_err(|_| "suite_busy")?.require_reviewed(
            id,
            revision,
            route,
            target,
            now(),
        );
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (app, id, revision, route, target);
        Err("suite_windows_required")
    }
}

pub(crate) fn project_operations(
    app: &tauri::AppHandle,
    call: &product_contract::transport::Call,
    rows: Vec<product_contract::operations::Row>,
) -> Result<serde_json::Value, &'static str> {
    if rows.len() > 128 {
        return Err("operation_limit");
    }
    match call {
        product_contract::transport::Call::ReadOperations {} => {
            serde_json::to_value(rows).map_err(|_| "operation_invalid")
        }
        product_contract::transport::Call::ReviewOperation {
            id,
            revision,
            operation_id,
        } => {
            let row = rows
                .into_iter()
                .find(|row| row.id == *id && row.revision == *revision)
                .ok_or("operation_stale")?;
            let (descriptor, request) = row.review(operation_id);
            enqueue_review(app, &descriptor, &request)
        }
        _ => Err("operation_invalid"),
    }
}
