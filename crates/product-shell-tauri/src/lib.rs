//! Product session and navigation boundary. Domain plugins separately declare
//! their command allowlists and reuse this native session authorization.
use catalog::products::{Feature, Product, ProductCatalog, SOURCE};
use product_contract::{
    Handshake, Operation, OperationState, Problem, ProblemCode, ProjectContext, Provenance,
    RouteRequest, RouteStatus, SessionGuard,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Manager, State, WebviewWindow};

struct ShellState {
    catalog: ProductCatalog,
    product: String,
    session: Mutex<SessionGuard>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Description {
    handshake: Handshake,
    product: Product,
    features: Vec<Feature>,
    context: Option<ProjectContext>,
}

fn local_main(window: &WebviewWindow) -> bool {
    window.label() == "main"
        && window.url().is_ok_and(|url| {
            let packaged = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"));
            let dev = cfg!(debug_assertions)
                && url.scheme() == "http"
                && url.host_str() == Some("localhost")
                && matches!(url.port(), Some(1430..=1433));
            packaged || dev
        })
}

#[tauri::command]
fn describe(window: WebviewWindow, state: State<'_, ShellState>) -> Result<Description, String> {
    if !local_main(&window) {
        return Err("허용되지 않은 창입니다.".into());
    }
    let product = state
        .catalog
        .products
        .iter()
        .find(|p| p.id == state.product)
        .ok_or("제품을 찾을 수 없습니다.")?
        .clone();
    let session = state
        .session
        .lock()
        .map_err(|_| "세션을 사용할 수 없습니다.")?;
    let handshake = session.handshake().clone();
    let context = session.context().cloned();
    Ok(Description {
        handshake,
        context,
        product,
        features: state
            .catalog
            .features
            .iter()
            .filter(|f| f.owner == state.product)
            .cloned()
            .collect(),
    })
}

#[tauri::command]
fn route_status(
    window: WebviewWindow,
    state: State<'_, ShellState>,
    request: RouteRequest,
) -> Result<RouteStatus, Problem> {
    let provenance = authorize(&window, &request, &format!("{}.shell", state.product))?;
    Ok(RouteStatus {
        route: request.route,
        availability: "foundation".into(),
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
    })
}

/// Validate native caller, owner, context, deadline and replay before a domain
/// adapter touches data. The component is supplied by native code only.
pub fn authorize(
    window: &WebviewWindow,
    request: &RouteRequest,
    component: &str,
) -> Result<Provenance, Problem> {
    let state = window.state::<ShellState>();
    let provenance = Provenance {
        product: state.product.clone(),
        component: component.into(),
        request_id: if request.request_id.len() <= 64
            && request
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            request.request_id.clone()
        } else {
            "rejected".into()
        },
        revision: state.catalog.catalog_revision,
    };
    let problem = |code| Problem {
        code,
        provenance: provenance.clone(),
    };
    if !state
        .catalog
        .components
        .iter()
        .any(|entry| entry.id == component && entry.owner == state.product)
    {
        return Err(problem(ProblemCode::Unauthorized));
    }
    let routes: Vec<&str> = state
        .catalog
        .features
        .iter()
        .filter(|f| f.owner == state.product)
        .map(|f| f.route.as_str())
        .collect();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| problem(ProblemCode::Unavailable))?
        .as_millis();
    let now = u64::try_from(now).map_err(|_| problem(ProblemCode::Unavailable))?;
    // WebView2 URL lookup can synchronously dispatch to the UI thread. A
    // synchronous describe command on that thread also takes this mutex, so
    // collect native caller information before entering the session lock.
    let caller_window = window.label().to_string();
    let local_origin = local_main(window);
    state
        .session
        .lock()
        .map_err(|_| problem(ProblemCode::Unavailable))?
        .authorize(&caller_window, local_origin, request, now, &routes)
        .map_err(problem)?;
    Ok(provenance)
}

pub fn builder(product: &'static str) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("product-shell")
                .invoke_handler(tauri::generate_handler![describe, route_status])
                .build(),
        )
        .setup(move |app| {
            let catalog = ProductCatalog::parse(SOURCE).map_err(std::io::Error::other)?;
            if !catalog.products.iter().any(|p| p.id == product) {
                return Err("unknown product".into());
            }
            let executable = std::env::current_exe()?.canonicalize()?;
            let installation_id = Sha256::digest(executable.to_string_lossy().as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let session = SessionGuard::new(Handshake {
                protocol_version: 1,
                product: product.into(),
                installation_id,
                session_id: uuid::Uuid::new_v4().to_string(),
            });
            app.manage(ShellState {
                catalog,
                product: product.into(),
                session: Mutex::new(session),
            });
            window_state_tauri::restore_main_window(app.handle());
            Ok(())
        })
        .on_window_event(window_state_tauri::handle_window_event)
}

/// Hidden development installations use an executable-location namespace for
/// data and single-instance identity. This does not claim suite registration or
/// portable migration support; WP08 replaces it with verified install records.
pub fn run(product: &'static str, context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
    run_with(product, context, |builder| builder)
}

pub fn run_with(
    product: &'static str,
    mut context: tauri::Context<tauri::Wry>,
    configure: impl FnOnce(tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry>,
) -> tauri::Result<()> {
    if cfg!(debug_assertions) {
        let catalog = ProductCatalog::parse(SOURCE).map_err(std::io::Error::other)?;
        let routes: Vec<&str> = catalog
            .features
            .iter()
            .filter(|f| f.owner == product)
            .map(|f| f.route.as_str())
            .collect();
        if let Some(route) = product_contract::development_route(std::env::args().skip(1), &routes)
            .map_err(std::io::Error::other)?
        {
            // Select the relative application URL before creating WebView2.
            // Reading its current URL during setup can observe about:blank.
            let window = context
                .config_mut()
                .app
                .windows
                .iter_mut()
                .find(|window| window.label == "main")
                .ok_or_else(|| std::io::Error::other("missing main window"))?;
            window.url = tauri::WebviewUrl::App(format!("index.html?route={route}").into());
        }
    }
    isolate_installation(&mut context)?;
    configure(builder(product)).run(context)
}

/// Shared by the product UI and its explicitly owned import worker. This only
/// selects this executable installation's namespace; it grants no IPC authority.
pub fn isolate_installation(context: &mut tauri::Context<tauri::Wry>) -> tauri::Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let suffix: String = Sha256::digest(executable.to_string_lossy().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    context.config_mut().identifier = format!("{}.i{}", context.config().identifier, suffix);
    Ok(())
}
