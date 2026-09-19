//! Fixed legacy JSON inputs, retained before parsing or DPAPI resealing. No
//! plaintext produced by a credential adapter passes through this backup.
use super::import_repository::read_file;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
const API: &str = "com.devbox.apiplayground";
const WEBHOOK: &str = "com.devbox.webhooklab";
const TOOLS: &str = "com.devbox.developertoolbox";
const MAX_FILE: usize = 20 * 1024 * 1024;
const MAX_TOTAL: usize = 128 * 1024 * 1024;
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    relative: String,
    bytes: u64,
    sha256: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    selected: Vec<String>,
    profiles: Option<Vec<String>>,
    files: Vec<Entry>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn check(started: Instant, cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Err("migration_cancelled".into());
    }
    if started.elapsed() > Duration::from_secs(30) {
        return Err("migration_backup_timeout".into());
    }
    Ok(())
}
fn choices(selected: &[String], profiles: &Option<Vec<String>>) -> Result<()> {
    if selected.is_empty()
        || selected.len() > 3
        || selected
            .iter()
            .any(|source| ![API, WEBHOOK, TOOLS].contains(&source.as_str()))
        || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
        || profiles.as_ref().is_some_and(|ids| {
            ids.len() > 64
                || ids.iter().any(|id| !uuid(id))
                || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        })
    {
        return Err("migration_backup_invalid".into());
    }
    Ok(())
}
fn relative_allowed(relative: &str, selected: &[String], profiles: &Option<Vec<String>>) -> bool {
    let Some((owner, name)) = relative.split_once('/') else {
        return false;
    };
    if !selected.iter().any(|source| source == owner) {
        return false;
    }
    match owner {
        API => matches!(name, "oauth/mcp-grants.json" | "grpc/tls-credentials.json"),
        TOOLS => name == "smart-workflows.json",
        WEBHOOK => {
            name == "fixtures.json"
                || name
                    .strip_prefix("service-profiles/")
                    .and_then(|name| name.strip_suffix(".json"))
                    .is_some_and(|id| {
                        uuid(id)
                            && profiles
                                .as_ref()
                                .is_none_or(|ids| ids.iter().any(|value| value == id))
                    })
        }
        _ => false,
    }
}
fn inventory(
    base: &Path,
    selected: &[String],
    profiles: &Option<Vec<String>>,
) -> Result<Vec<String>> {
    choices(selected, profiles)?;
    let mut names = Vec::new();
    for owner in selected {
        let fixed: &[&str] = match owner.as_str() {
            API => &["oauth/mcp-grants.json", "grpc/tls-credentials.json"],
            WEBHOOK => &["fixtures.json"],
            TOOLS => &["smart-workflows.json"],
            _ => return Err("migration_backup_invalid".into()),
        };
        names.extend(fixed.iter().map(|name| format!("{owner}/{name}")));
        if owner != WEBHOOK {
            continue;
        }
        let directory = base.join(owner).join("service-profiles");
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if profiles.as_ref().is_some_and(|ids| !ids.is_empty()) {
                    return Err("migration_source_changed".into());
                }
                continue;
            }
            Err(_) => return Err("migration_source_unavailable".into()),
            Ok(_) => {}
        }
        devbox_filesystem::ensure_no_links(&directory).map_err(|_| "migration_path_invalid")?;
        let mut present = BTreeSet::new();
        for (index, entry) in fs::read_dir(directory)
            .map_err(|_| "migration_source_unavailable")?
            .enumerate()
        {
            if index >= 256 {
                return Err("migration_store_too_large".into());
            }
            let entry = entry.map_err(|_| "migration_source_unavailable")?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "migration_path_invalid")?;
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            if !uuid(id) {
                return Err("migration_schema_invalid".into());
            }
            if profiles
                .as_ref()
                .is_some_and(|ids| !ids.iter().any(|value| value == id))
            {
                continue;
            }
            present.insert(id.to_owned());
            names.push(format!("{owner}/service-profiles/{name}"));
        }
        if profiles
            .as_ref()
            .is_some_and(|ids| ids.iter().any(|id| !present.contains(id)))
        {
            return Err("migration_source_changed".into());
        }
    }
    names.sort();
    Ok(names)
}
/// `stage` is the new destination-owned import operation, never a renderer path.
pub fn capture(
    base: &Path,
    stage: &Path,
    selected: &[String],
    profiles: &Option<Vec<String>>,
    cancelled: &AtomicBool,
) -> Result<PathBuf> {
    let started = Instant::now();
    let names = inventory(base, selected, profiles)?;
    devbox_filesystem::ensure_no_links(stage).map_err(|_| "migration_path_invalid")?;
    let target = stage.join("retained-native");
    fs::create_dir(&target).map_err(|_| "migration_backup_target_exists")?;
    let mut records = Vec::new();
    let mut total = 0;
    for relative in &names {
        check(started, cancelled)?;
        let raw = read_file(&base.join(relative), MAX_FILE)?;
        let bytes = raw.as_ref().map_or(0, String::len);
        total += bytes;
        if total > MAX_TOTAL {
            return Err("migration_store_too_large".into());
        }
        if let Some(raw) = &raw {
            let path = target.join(relative);
            fs::create_dir_all(path.parent().ok_or("migration_path_invalid")?)
                .map_err(|_| "migration_backup_unavailable")?;
            devbox_filesystem::ensure_no_links(path.parent().ok_or("migration_path_invalid")?)
                .map_err(|_| "migration_path_invalid")?;
            devbox_filesystem::atomic_write(path, raw.as_bytes())
                .map_err(|_| "migration_backup_unavailable")?;
        }
        records.push(Entry {
            relative: relative.clone(),
            bytes: bytes as u64,
            sha256: raw.as_ref().map(|raw| digest(raw.as_bytes())),
        });
    }
    if inventory(base, selected, profiles)? != names {
        return Err("migration_source_changed".into());
    }
    for record in &records {
        check(started, cancelled)?;
        let raw = read_file(&base.join(&record.relative), MAX_FILE)?;
        if raw.as_ref().map(|raw| digest(raw.as_bytes())) != record.sha256 {
            return Err("migration_source_changed".into());
        }
    }
    let manifest = Manifest {
        schema_version: 1,
        selected: selected.into(),
        profiles: profiles.clone(),
        files: records,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|_| "migration_backup_invalid")?;
    devbox_filesystem::atomic_write(target.join("snapshot.json"), &bytes)
        .map_err(|_| "migration_backup_unavailable")?;
    verify(&target, Some(&digest(&bytes)))?;
    Ok(target)
}
pub fn verify(root: &Path, expected: Option<&str>) -> Result<(String, u64)> {
    let raw =
        read_file(&root.join("snapshot.json"), 512 * 1024)?.ok_or("migration_backup_missing")?;
    let revision = digest(raw.as_bytes());
    if expected.is_some_and(|expected| expected != revision) {
        return Err("migration_backup_changed".into());
    }
    let manifest: Manifest = serde_json::from_str(&raw).map_err(|_| "migration_backup_invalid")?;
    choices(&manifest.selected, &manifest.profiles)?;
    if manifest.schema_version != 1 || manifest.files.len() > 260 {
        return Err("migration_backup_invalid".into());
    }
    let started = Instant::now();
    let token = AtomicBool::new(false);
    let mut names = BTreeSet::new();
    let mut total = 0;
    for file in &manifest.files {
        check(started, &token)?;
        if !relative_allowed(&file.relative, &manifest.selected, &manifest.profiles)
            || !names.insert(&file.relative)
        {
            return Err("migration_backup_invalid".into());
        }
        let bytes = read_file(&root.join(&file.relative), MAX_FILE)?;
        if bytes.as_ref().map(|raw| digest(raw.as_bytes())) != file.sha256
            || bytes.as_ref().map_or(0, String::len) as u64 != file.bytes
        {
            return Err("migration_backup_changed".into());
        }
        total += file.bytes;
        if total > MAX_TOTAL as u64 {
            return Err("migration_store_too_large".into());
        }
    }
    if inventory(root, &manifest.selected, &manifest.profiles)?
        != manifest
            .files
            .iter()
            .map(|file| file.relative.clone())
            .collect::<Vec<_>>()
    {
        return Err("migration_backup_changed".into());
    }
    Ok((revision, total))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_original_ciphertext_and_missing_files_without_following_references() {
        let base = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let source = base.path().join(API).join("oauth");
        fs::create_dir_all(&source).unwrap();
        let raw = b"{\"ciphertext\":\"synthetic-sealed-bytes\",\"path\":\"C:/external\"}\n";
        fs::write(source.join("mcp-grants.json"), raw).unwrap();
        let root = capture(
            base.path(),
            stage.path(),
            &[API.into()],
            &None,
            &AtomicBool::new(false),
        )
        .unwrap();
        let (revision, bytes) = verify(&root, None).unwrap();
        assert_eq!(bytes, raw.len() as u64);
        assert_eq!(
            fs::read(root.join(API).join("oauth/mcp-grants.json")).unwrap(),
            raw
        );
        assert_eq!(fs::read(source.join("mcp-grants.json")).unwrap(), raw);
        fs::write(root.join(API).join("oauth/mcp-grants.json"), b"changed").unwrap();
        assert!(verify(&root, Some(&revision)).is_err());
    }
}
