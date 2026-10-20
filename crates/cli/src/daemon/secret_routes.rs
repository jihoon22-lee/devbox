use super::state::Services;
use devbox_protocol::{CommonError, Ctx, DispatchError, Fail, StreamOpened};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::sync::Arc;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunSecretRequest {
    pub run_id: String,
}
pub(crate) fn native(method: &str) -> bool {
    matches!(
        method,
        "secrets.key_status" | "secrets.bootstrap" | "secrets.unlock"
    )
}
pub(crate) fn internal(method: &str) -> bool {
    native(method) || method == "secrets.run"
}
fn input<T: serde::de::DeserializeOwned>(raw: &RawValue) -> Result<T, DispatchError> {
    serde_json::from_str(raw.get()).map_err(|_| {
        DispatchError::common(CommonError::InvalidParams {
            reason: "invalid secret request".into(),
        })
    })
}
fn output<T: Serialize>(value: T) -> Result<Box<RawValue>, DispatchError> {
    serde_json::value::to_raw_value(&value)
        .map_err(|_| DispatchError::common(CommonError::Internal))
}
fn error<E: Serialize>(error: Fail<E>) -> DispatchError {
    match error {
        Fail::Domain(e) => DispatchError {
            error: devbox_protocol::to_wire(&e),
            diagnostic_id: None,
        },
        Fail::Internal(_) => DispatchError::common(CommonError::Internal),
    }
}
pub(crate) async fn dispatch(
    services: Arc<Services>,
    method: &str,
    ctx: Ctx,
    raw: Box<RawValue>,
) -> Result<Box<RawValue>, DispatchError> {
    match method {
        "secrets.key_status" => {
            let _: devbox_secrets::Empty = input(&raw)?;
            output(services.secrets.status().await.map_err(error)?)
        }
        "secrets.bootstrap" => output(
            services
                .secrets
                .bootstrap(input(&raw)?)
                .await
                .map_err(error)?,
        ),
        "secrets.unlock" => {
            let key: devbox_protocol::secrets::Unlock = input(&raw)?;
            // Own and wipe the decoded JSON buffer before asynchronous key exchange.
            use zeroize::Zeroize;
            let text: Box<str> = raw.into();
            let mut text = String::from(text);
            text.zeroize();
            output(services.secrets.unlock(key).await.map_err(error)?)
        }
        "secrets.run" => {
            let request: RunSecretRequest = input(&raw)?;
            let payload = services
                .runs
                .resolve_run_secrets(&request.run_id)
                .await
                .map_err(error)?;
            let ends = ctx
                .open_stream()
                .map_err(|_| DispatchError::common(CommonError::Internal))?;
            let id = ends.id;
            tokio::spawn(async move {
                let send = async {
                    ends.tx.write(&(payload.len() as u32).to_le_bytes()).await?;
                    ends.tx.write(&payload).await
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(10), send).await;
                ends.tx.finish().await;
            });
            output(StreamOpened { stream_id: id })
        }
        _ => Err(DispatchError::common(CommonError::MethodNotFound {
            method: method.into(),
        })),
    }
}
pub(crate) fn register(
    router: devbox_protocol::Router<Services>,
) -> devbox_protocol::Router<Services> {
    router
        .register::<devbox_secrets::List, _, _>(|s, _, _| async move { s.secrets.list().await })
        .register::<devbox_secrets::Set, _, _>(|s, _, p| async move { s.secrets.set(p).await })
        .register::<devbox_secrets::Delete, _, _>(
            |s, _, p| async move { s.secrets.delete(p).await },
        )
        .register::<devbox_secrets::ExportKey, _, _>(|s, _, _| async move {
            s.secrets.export_key().await
        })
        .register::<devbox_secrets::ImportKey, _, _>(|s, _, p| async move {
            s.secrets.import_key(p).await
        })
}
