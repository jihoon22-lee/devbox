//! Native delivery observation. Only destination Opened receipts write recency;
//! polling never resends the command and the worker sleeps when there is no work.
use product_contract::{
    commands::Request,
    navigation::{Phase, Receipt},
    transport::Call,
};
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
#[derive(Clone)]
struct Pending {
    product: String,
    command: Request,
    expires: u64,
    recorded: bool,
    phase: Option<Phase>,
}
#[derive(Default)]
pub(crate) struct Owner {
    entries: Mutex<BTreeMap<String, Pending>>,
    wake: tokio::sync::Notify,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
impl Owner {
    fn reserve(&self, product: &str, command: &Request) -> Result<(), &'static str> {
        product_contract::transport::validate_call(&Call::OpenCommand {
            request: command.clone(),
        })?;
        let mut entries = self.entries.lock().map_err(|_| "command_receipts_busy")?;
        entries.retain(|_, entry| entry.expires > now());
        if let Some(entry) = entries.get(&command.operation_id) {
            if entry.product != product || entry.command != *command {
                return Err("command_operation_conflict");
            }
            return Ok(());
        }
        if entries.len() >= 32 {
            return Err("command_receipts_full");
        }
        entries.insert(
            command.operation_id.clone(),
            Pending {
                product: product.into(),
                command: command.clone(),
                expires: now().saturating_add(180_000),
                recorded: false,
                phase: None,
            },
        );
        self.wake.notify_one();
        Ok(())
    }
    pub(crate) fn disconnect(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
        self.wake.notify_one();
    }
    pub(crate) fn clear_recents(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            for entry in entries.values_mut() {
                entry.recorded = true;
            }
        }
    }
}
pub(crate) async fn open(
    app: &tauri::AppHandle,
    product: &str,
    command: Request,
    deadline: u64,
) -> Result<Receipt, &'static str> {
    let operation_id = command.operation_id.clone();
    app.state::<Owner>().reserve(product, &command)?;
    let value = crate::suite::remote(
        app,
        product,
        Call::OpenCommand { request: command },
        deadline,
    )
    .await?;
    let receipt: Receipt = serde_json::from_value(value).map_err(|_| "command_reply_invalid")?;
    if receipt.operation_id != operation_id {
        return Err("command_reply_mismatch");
    }
    Ok(receipt)
}
pub(crate) async fn run(app: tauri::AppHandle) {
    loop {
        let owner = app.state::<Owner>();
        let pending = {
            let Ok(mut entries) = owner.entries.lock() else {
                return;
            };
            entries.retain(|_, entry| entry.expires > now());
            entries
                .values()
                .filter(|entry| {
                    !entry.recorded
                        && !matches!(entry.phase, Some(Phase::Rejected | Phase::Expired))
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        if pending.is_empty() {
            owner.wake.notified().await;
            continue;
        }
        for pending in pending {
            let reply = crate::suite::remote(
                &app,
                &pending.product,
                Call::CommandStatus {
                    operation_id: pending.command.operation_id.clone(),
                },
                now().saturating_add(1500),
            )
            .await;
            let Ok(receipt) = reply.and_then(|value| {
                serde_json::from_value::<Receipt>(value).map_err(|_| "command_reply_invalid")
            }) else {
                continue;
            };
            if receipt.operation_id != pending.command.operation_id {
                continue;
            }
            let app = app.clone();
            // Preference IO uses the same single owner as renderer preferences.
            // Disconnect/clear wins under the queue lock before persistence.
            let _ = tokio::task::spawn_blocking(move || {
                let owner = app.state::<Owner>();
                let Ok(mut entries) = owner.entries.lock() else {
                    return;
                };
                let Some(entry) = entries.get_mut(&receipt.operation_id) else {
                    return;
                };
                if entry.command != pending.command
                    || entry.product != pending.product
                    || entry.expires <= now()
                {
                    return;
                }
                entry.phase = Some(receipt.phase.clone());
                if !entry.recorded
                    && receipt.phase == Phase::Opened
                    && crate::commands::record_remote_recent(&app, &entry.command.command_id)
                        .is_ok()
                {
                    entry.recorded = true;
                }
            })
            .await;
        }
        tokio::select! { _ = owner.wake.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command() -> Request {
        Request {
            operation_id: "operation-one".into(),
            command_id: "workspace.open-files".into(),
            revision: "a".repeat(64),
            context: None,
            selection_id: None,
        }
    }
    #[test]
    fn retries_keep_the_exact_command_and_clear_disconnect_revoke_pending_recency() {
        let owner = Owner::default();
        let mut request = command();
        owner.reserve("workspace", &request).unwrap();
        owner.reserve("workspace", &request).unwrap();
        request.command_id = "workspace.open-source".into();
        assert_eq!(
            owner.reserve("workspace", &request),
            Err("command_operation_conflict")
        );
        assert_eq!(owner.entries.lock().unwrap().len(), 1);
        owner.clear_recents();
        assert!(owner
            .entries
            .lock()
            .unwrap()
            .values()
            .all(|entry| entry.recorded));
        owner.disconnect();
        assert!(owner.entries.lock().unwrap().is_empty());
    }
    #[test]
    fn observation_capacity_is_bounded_before_dispatch() {
        let owner = Owner::default();
        for i in 0..32 {
            let mut request = command();
            request.operation_id = format!("operation-{i}");
            owner.reserve("workspace", &request).unwrap();
        }
        assert_eq!(
            owner.reserve("workspace", &command()),
            Err("command_receipts_full")
        );
    }
}
