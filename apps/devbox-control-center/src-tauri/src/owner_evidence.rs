//! Record native owner observations without granting activation permission.
use crate::core::{delivery::OwnerEvidence, delivery_store::Store};
use product_contract::health::Report;
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
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EvidenceKind {
    Owner,
    Health,
}
pub(crate) fn evidence_kind(
    phase: Option<product_contract::activation::Phase>,
) -> Result<EvidenceKind> {
    use product_contract::activation::Phase;
    match phase {
        Some(Phase::Import) => Ok(EvidenceKind::Owner),
        Some(Phase::Health) => Ok(EvidenceKind::Health),
        _ => Err("suite_owner_phase_invalid"),
    }
}
pub(crate) async fn record(
    app: tauri::AppHandle,
    owner: String,
    deadline: u64,
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
    let kind = evidence_kind(product_shell_tauri::suite_activation_phase(&app)?)?;
    match kind {
        EvidenceKind::Health => journal.record_health(
            journal.revision,
            crate::core::delivery::HealthCheck {
                observed_ms: now(),
                report: before.clone(),
            },
        )?,
        EvidenceKind::Owner => journal.record_owner(
            journal.revision,
            OwnerEvidence {
                sources: None,
                summary: before.store.clone(),
                backups: Vec::new(),
            },
        )?,
    }
    let ready = before.store.setup_selected && !before.store.review_required && !before.store.busy;
    let result = serde_json::json!({"owner":owner,"recorded":true,"revision":journal.revision,
        "activationReady":false,"nativeStoreReady":ready,"report":before});
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

#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::activation::Phase;
    #[test]
    fn phase_selects_owner_or_health_evidence() {
        assert_eq!(evidence_kind(Some(Phase::Import)), Ok(EvidenceKind::Owner));
        assert_eq!(evidence_kind(Some(Phase::Health)), Ok(EvidenceKind::Health));
        assert!(evidence_kind(Some(Phase::Committed)).is_err());
        assert!(evidence_kind(None).is_err());
    }
}
