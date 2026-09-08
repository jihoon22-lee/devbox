//! Three independent SQLite stores are selected through one native-owned
//! generation pointer. Vault bytes live outside generations and are never moved.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub generation: String,
}
fn valid_generation(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub fn read(root: &Path) -> Result<Option<Manifest>, String> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "store_path_invalid")?;
    let path = root.join("active-stores.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("store_unavailable".into()),
        Ok(metadata) if !metadata.is_file() || metadata.len() > 4096 => {
            return Err("store_manifest_invalid".into())
        }
        _ => {}
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "store_path_invalid")?;
    let (mut file, identity) =
        devbox_filesystem::open_filesystem_object(&path, false).map_err(|_| "store_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "store_unavailable")?;
    if bytes.len() > 4096
        || devbox_filesystem::filesystem_identity(&path, false).map_err(|_| "store_unavailable")?
            != identity
    {
        return Err("store_manifest_invalid".into());
    }
    let value: Manifest = serde_json::from_slice(&bytes).map_err(|_| "store_manifest_invalid")?;
    if value.schema_version != 1 {
        return Err("store_future_schema".into());
    }
    if !valid_generation(&value.generation) {
        return Err("store_manifest_invalid".into());
    }
    Ok(Some(value))
}
pub fn directory(root: &Path, manifest: &Manifest, component: &str) -> Result<PathBuf, String> {
    if manifest.schema_version != 1
        || !valid_generation(&manifest.generation)
        || !matches!(component, "notes" | "activity" | "search")
    {
        return Err("store_manifest_invalid".into());
    }
    let directory = root
        .join("stores")
        .join(&manifest.generation)
        .join(component);
    devbox_filesystem::ensure_no_links(&directory).map_err(|_| "store_path_invalid")?;
    if !directory.is_dir() {
        return Err("store_unavailable".into());
    }
    devbox_filesystem::ensure_no_links(directory.join("data.db"))
        .map_err(|_| "store_path_invalid")?;
    if !directory.join("data.db").is_file() {
        return Err("store_unavailable".into());
    }
    Ok(directory)
}
/// A cancelled/failed preparation remains unselected. Existing active stores
/// are never replaced by the new-user action.
pub fn create_empty(root: &Path) -> Result<Manifest, String> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "store_path_invalid")?;
    let lock_path = root.join("store-activation.lock");
    if lock_path.exists() {
        devbox_filesystem::ensure_no_links(&lock_path).map_err(|_| "store_path_invalid")?;
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|_| "store_unavailable")?;
    lock.try_lock().map_err(|_| "store_busy")?;
    if let Some(active) = read(root)? {
        return Ok(active);
    }
    let stores = root.join("stores");
    fs::create_dir_all(&stores).map_err(|_| "store_unavailable")?;
    devbox_filesystem::ensure_no_links(&stores).map_err(|_| "store_path_invalid")?;
    let manifest = Manifest {
        schema_version: 1,
        generation: uuid::Uuid::new_v4().to_string(),
    };
    let generation = stores.join(&manifest.generation);
    fs::create_dir(&generation).map_err(|_| "store_unavailable")?;
    for component in ["notes", "activity", "search"] {
        fs::create_dir(generation.join(component)).map_err(|_| "store_unavailable")?;
    }
    knowledge_base_lib::component::create_empty_store(
        &generation.join("notes/data.db"),
        &root.join("notes-vault"),
    )?;
    life_log_lib::component::create_empty_store(&generation.join("activity/data.db"))?;
    everything_plus_lib::component::create_empty_store(&generation.join("search/data.db"))?;
    for component in ["notes", "activity", "search"] {
        directory(root, &manifest, component)?;
    }
    if read(root)?.is_some() {
        return Err("store_busy".into());
    }
    devbox_filesystem::atomic_write(
        root.join("active-stores.json"),
        &serde_json::to_vec(&manifest).map_err(|_| "store_manifest_invalid")?,
    )
    .map_err(|_| "store_unavailable")?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_activation_is_repeatable_and_selects_three_independent_stores() {
        let root = tempfile::tempdir().unwrap();
        let first = create_empty(root.path()).unwrap();
        assert_eq!(create_empty(root.path()).unwrap(), first);
        assert_eq!(read(root.path()).unwrap(), Some(first.clone()));
        let paths: Vec<_> = ["notes", "activity", "search"]
            .into_iter()
            .map(|c| directory(root.path(), &first, c).unwrap())
            .collect();
        assert_ne!(paths[0], paths[1]);
        assert_ne!(paths[1], paths[2]);
        assert!(
            !root.path().join("notes-vault").exists(),
            "preparation must not initialize a vault"
        );
    }
    #[test]
    fn future_or_corrupt_manifest_is_preserved_and_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("active-stores.json");
        for value in [
            r#"{"schemaVersion":2,"generation":"future"}"#,
            r#"{"schemaVersion":1,"generation":"../source"}"#,
            "broken",
        ] {
            fs::write(&path, value).unwrap();
            assert!(create_empty(root.path()).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), value);
            assert!(!root.path().join("stores").exists());
        }
    }
}
