//! Explicit indexed-file action. Export only an opaque reference/revision;
//! Workspace alone can resolve the native path/object proof after its review.
use product_contract::{file_reference::Proof, transport::Call};
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    app_id: String,
    reference: String,
}
pub(crate) async fn send(
    app: &tauri::AppHandle,
    args: Value,
    operation_id: &str,
    deadline: u64,
) -> Result<Value, String> {
    let input: Input = serde_json::from_value(args).map_err(|_| "file_reference_invalid")?;
    if input.app_id != "devbox-workspace" {
        return Err("provider_unavailable".into());
    }
    let proof: Proof = serde_json::from_value(
        crate::search::open(
            app,
            "native_file_reference",
            json!({"reference":input.reference}),
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
