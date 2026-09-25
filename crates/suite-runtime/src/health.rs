//! Read-only native health observations over the approved installation bus.
//! This does not launch products, run user commands, or advance activation.
use super::{platform::component_scope::CapturedScope, DomainHandler, Suite};
use product_contract::{health::Report, migration_status::Summary, transport::Call};
use std::sync::Arc;
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
fn approved(app: &tauri::AppHandle) -> Result<Arc<CapturedScope>> {
    let suite = app.state::<Suite>();
    let scope = suite
        .state
        .lock()
        .map_err(|_| "suite_busy")?
        .approved
        .clone()
        .ok_or("suite_review_required")?;
    scope.revalidate()?;
    Ok(scope)
}
fn routes(product: &str) -> Result<(u64, Vec<String>)> {
    let catalog =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?;
    let mut routes: Vec<_> = catalog
        .features
        .into_iter()
        .filter(|row| row.owner == product)
        .map(|row| row.route)
        .collect();
    routes.sort();
    routes.dedup();
    Ok((catalog.catalog_revision, routes))
}
pub(super) async fn observe(
    app: tauri::AppHandle,
    product: &str,
    domain: Option<DomainHandler>,
    challenge: String,
    deadline: u64,
) -> Result<serde_json::Value> {
    product_contract::transport::validate_call(&Call::ReadHealthStatus {
        challenge: challenge.clone(),
    })?;
    let scope = approved(&app)?;
    scope.member(product)?;
    let session_id = product_shell_tauri::health_session(&app, product)?;
    let value = domain.ok_or("health_owner_unavailable")?(
        app.clone(),
        Call::ReadMigrationStatus {},
        deadline,
        None,
    )
    .await?;
    let store: Summary = serde_json::from_value(value).map_err(|_| "health_store_invalid")?;
    let (catalog_revision, routes) = routes(product)?;
    let report = Report {
        schema_version: 1,
        challenge: challenge.clone(),
        installation_key: scope.installation_key.clone(),
        generation: scope.manifest.generation.clone(),
        session_id,
        catalog_revision,
        routes,
        store,
    };
    report.validate(
        product,
        super::host_version(&app),
        &scope.installation_key,
        &scope.manifest.generation,
        &challenge,
    )?;
    scope.revalidate()?;
    if super::now() >= deadline
        || product_shell_tauri::health_session(&app, product)? != report.session_id
    {
        return Err("suite_health_stale");
    }
    serde_json::to_value(report).map_err(|_| "suite_health_invalid")
}
pub async fn read(
    app: tauri::AppHandle,
    product: &str,
    domain: Option<DomainHandler>,
    deadline: u64,
) -> Result<serde_json::Value> {
    let scope = approved(&app)?;
    scope.member(product)?;
    let challenge = uuid::Uuid::new_v4().to_string();
    let value = if product == "control-center" {
        observe(app.clone(), product, domain, challenge.clone(), deadline).await?
    } else {
        super::remote(
            &app,
            product,
            Call::ReadHealthStatus {
                challenge: challenge.clone(),
            },
            deadline,
        )
        .await?
    };
    let report: Report = serde_json::from_value(value).map_err(|_| "suite_health_invalid")?;
    report.validate(
        product,
        super::host_version(&app),
        &scope.installation_key,
        &scope.manifest.generation,
        &challenge,
    )?;
    let (revision, expected) = routes(product)?;
    if report.catalog_revision != revision || report.routes != expected || super::now() >= deadline
    {
        return Err("suite_health_incompatible");
    }
    scope.revalidate()?;
    Ok(serde_json::json!({"nativeStoreReady":report.store_ready(),"report":report}))
}

// The shared health module is also compiled by products that only answer this
// request; Control Center alone collects source evidence for suite cutover.
