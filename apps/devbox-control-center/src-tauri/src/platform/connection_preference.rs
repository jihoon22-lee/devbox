//! Explicit product-local permission to reconnect the same reviewed package.
//! A changed root identity, member digest, generation or version needs new review.
use super::component_scope::CapturedScope;
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf};
use tauri::Manager;
const FILE: &str = "suite-connection-v1.json";
#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Preference {
    schema: u32,
    product: String,
    installation: String,
    generation: String,
}
impl Preference {
    pub(crate) fn matches(&self, product: &str, scope: &CapturedScope) -> bool {
        self == &Self::for_scope(product, scope)
    }
    fn for_scope(product: &str, scope: &CapturedScope) -> Self {
        Self {
            schema: 1,
            product: product.into(),
            installation: scope.installation_key.clone(),
            generation: scope.id.clone(),
        }
    }
}
fn root(app: &tauri::AppHandle) -> Result<PathBuf, &'static str> {
    app.path()
        .app_local_data_dir()
        .map_err(|_| "suite_preference_unavailable")
}
pub(crate) fn read(app: &tauri::AppHandle) -> Result<Option<Preference>, &'static str> {
    let root = root(app)?;
    if !root
        .try_exists()
        .map_err(|_| "suite_preference_unavailable")?
    {
        return Ok(None);
    }
    ensure_no_links(&root).map_err(|_| "suite_preference_unavailable")?;
    let _directories = super::component_scope::pin_directories(&root)?;
    let path = root.join(FILE);
    if !path
        .try_exists()
        .map_err(|_| "suite_preference_unavailable")?
    {
        return Ok(None);
    }
    ensure_no_links(&path).map_err(|_| "suite_preference_unavailable")?;
    let (mut file, identity) =
        open_filesystem_object(&path, false).map_err(|_| "suite_preference_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "suite_preference_unavailable")?;
    if bytes.len() > 4096 || filesystem_identity(&path, false).ok() != Some(identity) {
        return Err("suite_preference_invalid");
    }
    let value: Preference =
        serde_json::from_slice(&bytes).map_err(|_| "suite_preference_invalid")?;
    if value.schema != 1
        || !product_contract::installation::PRODUCTS.contains(&value.product.as_str())
        || !product_contract::commands::revision(&value.installation)
        || !product_contract::commands::revision(&value.generation)
    {
        return Err("suite_preference_invalid");
    }
    Ok(Some(value))
}
pub(crate) fn write(
    app: &tauri::AppHandle,
    product: &str,
    scope: Option<&CapturedScope>,
) -> Result<(), &'static str> {
    let root = root(app)?;
    if !root
        .try_exists()
        .map_err(|_| "suite_preference_unavailable")?
    {
        if scope.is_none() {
            return Ok(());
        }
        ensure_no_links(root.parent().ok_or("suite_preference_unavailable")?)
            .map_err(|_| "suite_preference_unavailable")?;
        std::fs::create_dir(&root).map_err(|_| "suite_preference_unavailable")?;
    }
    ensure_no_links(&root).map_err(|_| "suite_preference_unavailable")?;
    let (_handle, identity) =
        open_filesystem_object(&root, true).map_err(|_| "suite_preference_unavailable")?;
    let _directories = super::component_scope::pin_directories(&root)?;
    let path = root.join(FILE);
    if let Some(scope) = scope {
        scope.revalidate()?;
        let bytes = serde_json::to_vec(&Preference::for_scope(product, scope))
            .map_err(|_| "suite_preference_invalid")?;
        devbox_filesystem::atomic_write(&path, &bytes)
            .map_err(|_| "suite_preference_unavailable")?;
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("suite_preference_unavailable"),
        }
    }
    if filesystem_identity(&root, true).ok() != Some(identity) {
        return Err("suite_preference_changed");
    }
    Ok(())
}
