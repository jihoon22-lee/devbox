//! No process launch, secret, filesystem selection or migration mutation is
//! exposed by this shell. Domain adapters require their own authority review.
use catalog::products::{Feature, Product, ProductCatalog, SOURCE};
use product_contract::{
    Handshake, ProjectContext, Provenance, RouteRequest, RouteStatus, SessionGuard,
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
) -> Result<RouteStatus, String> {
    let routes: Vec<&str> = state
        .catalog
        .features
        .iter()
        .filter(|f| f.owner == state.product)
        .map(|f| f.route.as_str())
        .collect();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "시계를 확인해 주세요.")?
        .as_millis();
    let now = u64::try_from(now).map_err(|_| "시계를 확인해 주세요.")?;
    state
        .session
        .lock()
        .map_err(|_| "세션을 사용할 수 없습니다.")?
        .authorize(window.label(), local_main(&window), &request, now, &routes)
        .map_err(str::to_owned)?;
    Ok(RouteStatus {
        route: request.route,
        availability: "foundation".into(),
        provenance: Provenance {
            product: state.product.clone(),
            component: format!("{}.shell", state.product),
            request_id: request.request_id,
            revision: state.catalog.catalog_revision,
        },
    })
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
pub fn run(product: &'static str, mut context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let suffix: String = Sha256::digest(executable.to_string_lossy().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    context.config_mut().identifier = format!("{}.i{}", context.config().identifier, suffix);
    builder(product).run(context)
}
