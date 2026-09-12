//! Descendant listeners are correlated by the live execution owner, including
//! process-mode tasks without a configured service health port. The same exact
//! identity/endpoint/run reference is reconstructed before each navigation.
use super::*;
use port_manager_lib::component::{
    CorrelationConfidence, ListenerIdentity, PortCorrelation, PortObservationSnapshot,
    ProductPortAction,
};
use std::collections::BTreeMap;
fn process(identity: &ListenerIdentity) -> Option<run_manager_lib::scheduler::ObservedProcess> {
    match identity {
        ListenerIdentity::Windows { pid, start_time } => {
            Some(run_manager_lib::scheduler::ObservedProcess::Windows {
                pid: *pid,
                creation_filetime: start_time.parse().ok()?,
            })
        }
        ListenerIdentity::Wsl {
            distro,
            pid,
            start_tick,
        } => Some(run_manager_lib::scheduler::ObservedProcess::Wsl {
            distro: distro.clone(),
            pid: *pid,
            start_tick: *start_tick,
        }),
        _ => None,
    }
}
pub(super) async fn observe(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: Option<&ProjectContext>,
    deadline: u64,
) -> Result<(PortObservationSnapshot, BTreeMap<String, ProductPortAction>)> {
    let mut snapshot = port_manager_lib::component::observe_product(bindings(
        app,
        host,
        definitions,
        context,
        deadline,
    ))
    .map_err(issue)?;
    let mut actions = BTreeMap::new();
    let mut cache = BTreeMap::new();
    let mut correlations = snapshot
        .rows
        .iter()
        .map(|entry| entry.correlations.len())
        .sum::<usize>();
    for entry in &mut snapshot.rows {
        if actions.len() >= 256 || correlations >= 4096 || entry.correlations.len() >= 64 {
            snapshot.correlations_truncated = true;
            continue;
        }
        let Some(identity) = entry.row.identity.as_ref() else {
            continue;
        };
        let Some(process) = process(identity) else {
            continue;
        };
        let key = serde_json::to_string(identity).map_err(|_| "invalid_response")?;
        if !cache.contains_key(&key) {
            if cache.len() >= 256
                || crate::files_host::current_deadline(deadline.saturating_sub(1000)).is_err()
            {
                snapshot.correlations_truncated = true;
                continue;
            }
            let owner = run_manager_lib::component::process_owner(app, process).await;
            if owner.is_err() {
                snapshot.correlations_truncated = true;
            }
            cache.insert(key.clone(), owner.ok().flatten());
        }
        let Some(owner) = cache.get(&key).and_then(Option::as_ref) else {
            continue;
        };
        let canonical=serde_json::to_string(&json!({"identity":identity,"endpoint":{ "proto":entry.row.port.proto,"address":entry.row.port.local_addr,"port":entry.row.port.port,"state":entry.row.port.state},"taskId":owner.task_id,"runId":owner.run_id,"context":context})).map_err(|_|"invalid_response")?;
        let action_key = devbox_integration::opaque_identity("runtime-port", &canonical)
            .map_err(|_| "invalid_response")?;
        let confidence = if matches!(identity, ListenerIdentity::Windows { .. }) {
            CorrelationConfidence::Verified
        } else {
            CorrelationConfidence::Declared
        };
        // Prefer a proved execution reference over its weaker configured-port
        // hint for this same task, retaining independent project suggestions.
        entry.correlations.retain(|correlation| {
            !(correlation.source_app == "run-manager" && correlation.target_id == owner.task_id)
        });
        correlations += 1;
        entry.correlations.push(PortCorrelation {
            source_app: "run-manager".into(),
            target_kind: "task".into(),
            target_id: owner.task_id.clone(),
            label: owner.label.clone(),
            confidence,
            action_key: action_key.clone(),
            logs_available: owner.logs_available,
        });
        entry
            .correlations
            .sort_by(|a, b| a.action_key.cmp(&b.action_key));
        actions.insert(
            action_key,
            ProductPortAction {
                owner: ProductPortOwner::Task {
                    id: owner.task_id.clone(),
                },
                run_id: Some(owner.run_id.clone()),
                logs_available: owner.logs_available,
                confidence,
            },
        );
    }
    Ok((snapshot, actions))
}
pub(super) async fn resolve(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: Option<&ProjectContext>,
    deadline: u64,
    key: &str,
) -> Result<ProductPortAction> {
    if let Some(identity) = key.strip_prefix("runtime-port-") {
        if identity.len() != 64
            || !identity
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("invalid_request");
        }
        let (_, mut actions) = observe(app, host, definitions, context, deadline).await?;
        actions.remove(key).ok_or("process_action_stale")
    } else {
        port_manager_lib::component::resolve_product_action(
            bindings(app, host, definitions, context, deadline),
            key,
        )
        .map_err(issue)
    }
}
