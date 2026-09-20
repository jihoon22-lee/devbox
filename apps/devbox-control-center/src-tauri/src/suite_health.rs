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
        env!("CARGO_PKG_VERSION"),
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
pub(crate) async fn read(
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
        env!("CARGO_PKG_VERSION"),
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

pub(crate) async fn backups(
    app: tauri::AppHandle,
    product: &str,
    domain: Option<DomainHandler>,
    id: Option<String>,
    deadline: u64,
) -> Result<serde_json::Value> {
    let scope = approved(&app)?;
    scope.member(product)?;
    let call = match &id {
        Some(id) => Call::VerifyMigrationBackup { id: id.clone() },
        None => Call::ListMigrationBackups {},
    };
    product_contract::transport::validate_call(&call)?;
    let value = if product == "control-center" {
        domain.ok_or("migration_unavailable")?(app.clone(), call, deadline, None).await?
    } else {
        super::remote(&app, product, call, deadline).await?
    };
    let result = if let Some(id) = id {
        let verified: product_contract::migration_backup::Verified =
            serde_json::from_value(value).map_err(|_| "migration_backup_invalid")?;
        verified.validate(product, &id)?;
        serde_json::to_value(verified).map_err(|_| "migration_backup_invalid")?
    } else {
        let rows: Vec<product_contract::migration_backup::Descriptor> =
            serde_json::from_value(value).map_err(|_| "migration_backup_invalid")?;
        let mut ids = std::collections::BTreeSet::new();
        if rows.len() > 128 {
            return Err("migration_backup_limit");
        }
        for row in &rows {
            row.validate()?;
            if !ids.insert(&row.id) {
                return Err("migration_backup_invalid");
            }
        }
        serde_json::to_value(rows).map_err(|_| "migration_backup_invalid")?
    };
    scope.revalidate()?;
    if super::now() >= deadline {
        return Err("migration_backup_expired");
    }
    Ok(result)
}

pub(crate) async fn sources(
    app: tauri::AppHandle,
    product: &str,
    domain: Option<DomainHandler>,
    deadline: u64,
) -> Result<Vec<product_contract::migration_source::Source>> {
    let scope = approved(&app)?;
    scope.member(product)?;
    let call = Call::VerifyMigrationSources {};
    let value = if product == "control-center" {
        domain.ok_or("migration_unavailable")?(app.clone(), call, deadline, None).await?
    } else {
        super::remote(&app, product, call, deadline).await?
    };
    let rows = serde_json::from_value::<Vec<product_contract::migration_source::Source>>(value)
        .map_err(|_| "migration_source_invalid")?;
    product_contract::migration_source::validate(product, &rows)?;
    scope.revalidate()?;
    if super::now() >= deadline {
        return Err("migration_source_expired");
    }
    Ok(rows)
}
