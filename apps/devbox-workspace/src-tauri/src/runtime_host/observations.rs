//! Preserve native correlation in the execution owner, with UI navigation local.
use super::*;
use ports_engine::component::{PortObservationSnapshot, ProductPortAction};
use std::collections::BTreeMap;
pub(super) async fn observe(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: Option<&ProjectContext>,
    deadline: u64,
) -> Result<(PortObservationSnapshot, BTreeMap<String, ProductPortAction>)> {
    if !crate::runtime_owner::installed(app)? {
        return workspace_core::runtime_observations::observe(
            app,
            host,
            definitions,
            context,
            deadline,
        )
        .await;
    }
    let snapshot = crate::runtime_owner::query(
        app,
        host,
        definitions,
        workspace_core::runtime_queries::Call::ObservePorts {
            context: context.cloned(),
        },
        deadline,
    )
    .await?;
    Ok((snapshot, BTreeMap::new()))
}
pub(super) async fn resolve(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: Option<&ProjectContext>,
    deadline: u64,
    key: &str,
) -> Result<ProductPortAction> {
    crate::runtime_owner::query(
        app,
        host,
        definitions,
        workspace_core::runtime_queries::Call::ResolvePort {
            context: context.cloned(),
            key: key.into(),
        },
        deadline,
    )
    .await
}
