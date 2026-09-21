//! Record native owner observations without granting activation permission.
use crate::core::{delivery::OwnerEvidence, delivery_store::Store};
use product_contract::{
    health::Report,
    migration_backup::{Descriptor, Verified},
};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
async fn health(app: &tauri::AppHandle, owner: &str, deadline: u64) -> Result<Report> {
    let value = crate::suite::health::read(
        app.clone(),
        owner,
        Some(crate::federation::handle),
        deadline,
    )
    .await?;
    serde_json::from_value(value.get("report").cloned().ok_or("suite_health_invalid")?)
        .map_err(|_| "suite_health_invalid")
}
async fn catalog(app: &tauri::AppHandle, owner: &str, deadline: u64) -> Result<Vec<Descriptor>> {
    let value = crate::suite::health::backups(
        app.clone(),
        owner,
        Some(crate::federation::handle),
        None,
        deadline,
    )
    .await?;
    let mut rows: Vec<Descriptor> =
        serde_json::from_value(value).map_err(|_| "migration_backup_invalid")?;
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(rows)
}
pub(crate) async fn record(
    app: tauri::AppHandle,
    owner: String,
    deadline: u64,
    health_only: bool,
) -> Result<serde_json::Value> {
    if !product_contract::installation::PRODUCTS.contains(&owner.as_str()) || now() >= deadline {
        return Err("suite_owner_invalid");
    }
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "suite_store_unavailable")?;
    let initial_root = root.clone();
    let (scope, mut journal, digest) = tauri::async_runtime::spawn_blocking(move || {
        let scope = crate::suite::capture_own("control-center", env!("CARGO_PKG_VERSION"))?;
        let (journal, digest) = Store::inspect(&initial_root)?.ok_or("suite_journal_missing")?;
        if journal.installation_key != scope.installation_key || journal.candidate != scope.manifest
        {
            return Err("suite_journal_changed");
        }
        Ok::<_, &'static str>((scope, journal, digest))
    })
    .await
    .map_err(|_| "suite_store_unavailable")??;
    let previous_revision = journal.revision;
    let before = health(&app, &owner, deadline).await?;
    if before.installation_key != journal.installation_key
        || before.generation != journal.candidate.generation
        || before.store.busy
    {
        return Err("suite_owner_changed");
    }
    let mut health_result = None;
    if health_only {
        if product_shell_tauri::suite_activation_phase(&app)?
            != Some(product_contract::activation::Phase::Health)
        {
            return Err("suite_health_phase_invalid");
        }
        journal.record_health(
            journal.revision,
            crate::core::delivery::HealthCheck {
                observed_ms: now(),
                report: before.clone(),
            },
        )?;
        health_result = Some(before);
    } else {
        let listed = catalog(&app, &owner, deadline).await?;
        let mut backups = Vec::with_capacity(listed.len());
        for descriptor in &listed {
            let value = crate::suite::health::backups(
                app.clone(),
                &owner,
                Some(crate::federation::handle),
                Some(descriptor.id.clone()),
                deadline,
            )
            .await?;
            let verified: Verified =
                serde_json::from_value(value).map_err(|_| "migration_backup_invalid")?;
            if verified.acquisition != descriptor.acquisition {
                return Err("suite_owner_changed");
            }
            backups.push(verified);
        }
        let before_sources = capture_sources(owner.clone()).await?;
        let sources = crate::suite::health::sources(
            app.clone(),
            &owner,
            Some(crate::federation::handle),
            deadline,
        )
        .await?;
        let namespaces = capture_sources(owner.clone()).await?;
        if namespaces != before_sources {
            return Err("migration_source_changed");
        }
        let after = health(&app, &owner, deadline).await?;
        if before.session_id != after.session_id
            || before.store != after.store
            || listed != catalog(&app, &owner, deadline).await?
        {
            return Err("suite_owner_changed");
        }
        let evidence = OwnerEvidence {
            sources: Some(crate::core::delivery::SourceEvidence {
                sources,
                namespaces,
            }),
            summary: after.store,
            backups,
        };
        journal.record_owner(journal.revision, evidence)?;
    }
    let result = serde_json::json!({"owner":owner,"recorded":true,"revision":journal.revision,
        "activationReady":false,"nativeStoreReady":health_result.is_some(),"report":health_result});
    tauri::async_runtime::spawn_blocking(move || {
        scope.revalidate()?;
        if now() >= deadline {
            return Err("suite_owner_expired");
        }
        let store = Store::open(&root)?;
        if journal.revision == previous_revision {
            // Identical observation: check CAS without inventing a revision or
            // asking Store::write to persist an invalid no-op transition.
            if store
                .read()?
                .as_ref()
                .map(|(_, revision)| revision.as_str())
                != Some(digest.as_str())
            {
                return Err("suite_journal_stale");
            }
        } else {
            store.write(Some(&digest), &journal)?;
        }
        Ok::<_, &'static str>(())
    })
    .await
    .map_err(|_| "suite_store_unavailable")??;
    Ok(result)
}

async fn capture_sources(owner: String) -> Result<Vec<crate::core::legacy_sources::Namespace>> {
    tauri::async_runtime::spawn_blocking(move || {
        let base = dirs::data_local_dir().ok_or("source_inventory_unavailable")?;
        Ok(crate::core::legacy_sources::capture(
            &base,
            product_contract::migration_source::identifiers(&owner),
            false,
        )?
        .namespaces)
    })
    .await
    .map_err(|_| "source_inventory_unavailable")?
}
