//! Explicit indexed-file action. Export only an opaque reference/revision;
//! Workspace alone can resolve the native path/object proof after its review.
use product_contract::{file_reference::Proof, transport::Call};
use serde::Deserialize;
use serde_json::Value;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    app_id: String,
    reference: String,
}
pub(crate) async fn send_typed(
    app: &tauri::AppHandle,
    app_id: String,
    reference: String,
    operation_id: &str,
    deadline: u64,
) -> Result<Value, String> {
    let input = Input { app_id, reference };
    if input.app_id != "devbox-workspace" {
        return Err("provider_unavailable".into());
    }
    let proof: Proof = serde_json::from_value(
        crate::search::open_typed(
            app,
            crate::search::OpenKind::NativeFileReference,
            input.reference,
        )
        .await?,
    )
    .map_err(|_| "file_reference_invalid")?;
    let result = crate::suite::remote(
        app,
        "workspace",
        Call::DeliverFileReference {
            reference: proof.reference.clone(),
            operation_id: operation_id.into(),
            revision: proof.revision()?,
        },
        deadline,
    )
    .await?;
    Ok(result)
}
