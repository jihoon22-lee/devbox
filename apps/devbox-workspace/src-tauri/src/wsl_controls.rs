//! Durable receipts for the existing WSL Docker start/stop/restart controls.
//! The actual distro/executable lease stays native through observation and action.
use crate::{host::Host, private_metadata::MetadataRoot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, &'static str>;
const FILE: &str = "wsl-control-receipts.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    operation_id: String,
    distro: String,
    container_id: String,
    action: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    fingerprint: String,
    state: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    schema_version: u32,
    receipts: BTreeMap<String, Receipt>,
}
fn valid_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn read(root: &MetadataRoot) -> Result<Store> {
    let store: Store = match root.read(FILE)? {
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| "wsl_controls_invalid")?,
        None => Store {
            schema_version: 1,
            receipts: BTreeMap::new(),
        },
    };
    if store.schema_version != 1
        || store.receipts.len() > 4096
        || store.receipts.iter().any(|(id, receipt)| {
            !valid_id(id)
                || receipt.fingerprint.len() != 64
                || !receipt
                    .fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                || !matches!(receipt.state.as_str(), "pending" | "completed" | "failed")
        })
    {
        return Err("wsl_controls_invalid");
    }
    Ok(store)
}
fn write(root: &MetadataRoot, store: &Store) -> Result<()> {
    root.write(
        FILE,
        &serde_json::to_vec(store).map_err(|_| "wsl_controls_invalid")?,
    )
}
#[derive(Default)]
pub(crate) struct Controls {
    gate: tokio::sync::Mutex<()>,
}
impl Controls {
    pub(crate) fn status(&self, root: &MetadataRoot, args: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            operation_id: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "wsl_control_invalid")?;
        if !valid_id(&input.operation_id) {
            return Err("wsl_control_invalid");
        }
        let store = read(root)?;
        Ok(
            json!({"state":store.receipts.get(&input.operation_id).map(|receipt|receipt.state.as_str()).unwrap_or("missing")}),
        )
    }
    pub(crate) async fn execute(
        &self,
        root: &MetadataRoot,
        host: &Host,
        args: Value,
        deadline: u64,
    ) -> Result<Value> {
        let input: Input = serde_json::from_value(args).map_err(|_| "wsl_control_invalid")?;
        if !valid_id(&input.operation_id)
            || input.distro.len() > 128
            || devbox_wsl::distro::validate_distro_name(&input.distro).is_err()
            || input.container_id.len() != 64
            || !input
                .container_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            || !matches!(input.action.as_str(), "start" | "stop" | "restart")
        {
            return Err("wsl_control_invalid");
        }
        let fingerprint = crate::definitions::digest(
            &serde_json::to_vec(&(&input.distro, &input.container_id, &input.action))
                .map_err(|_| "wsl_control_invalid")?,
        );
        let _guard = self.gate.lock().await;
        crate::files_host::current_deadline(deadline)?;
        let mut store = read(root)?;
        if let Some(receipt) = store.receipts.get(&input.operation_id) {
            if receipt.fingerprint != fingerprint {
                return Err("wsl_control_conflict");
            }
            return if receipt.state == "completed" {
                Ok(Value::Null)
            } else {
                Err("wsl_control_review_required")
            };
        }
        if store.receipts.len() >= 4096 {
            return Err("wsl_control_limit");
        }
        store.receipts.insert(
            input.operation_id.clone(),
            Receipt {
                fingerprint: fingerprint.clone(),
                state: "pending".into(),
            },
        );
        write(root, &store)?;
        let result = async {
            let lease =
                crate::platform::terminal_launch::capture_running(host, &input.distro, deadline)
                    .map_err(|_| "wsl_target_unavailable")?;
            let action = wsl_desktop_lib::component::docker_action_owned(
                &input.distro,
                &input.container_id,
                &input.action,
                lease.as_ref(),
            )
            .await
            .map_err(|_| "wsl_container_action_failed");
            let retired = lease.retire().map_err(|_| "wsl_target_retirement_pending");
            action.and(retired)
        }
        .await;
        // Preserve edits or a changed store instead of overwriting them after
        // a completed external action. Its pending receipt requires review.
        let mut current = read(root)?;
        let receipt = current
            .receipts
            .get_mut(&input.operation_id)
            .ok_or("wsl_control_changed")?;
        if receipt.fingerprint != fingerprint || receipt.state != "pending" {
            return Err("wsl_control_changed");
        }
        receipt.state = if result.is_ok() {
            "completed"
        } else {
            "failed"
        }
        .into();
        write(root, &current)?;
        result?;
        Ok(Value::Null)
    }
}
