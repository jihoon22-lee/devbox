//! Exact installed files acquired from the pinned v0.7 installers on two fresh
//! hosted roots. Installed executable bytes can differ from portable assets.
//! These observations establish file ownership, not permission to uninstall.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};
type Result<T> = std::result::Result<T, &'static str>;
const REFERENCE: &[u8] = include_bytes!("../../resources/legacy-v0.7-installed-files.json");
const PIN: &str = "77122ef8c459d0d09cc0c36174745c7a8a8afb92dd6b2c36298c722f7c4e0945";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    schema_version: u32,
    baseline_tag: String,
    manifest_sha256: String,
    result: String,
    apps: Vec<InstalledApp>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    pub id: String,
    pub version: String,
    pub files: Vec<InstalledFile>,
    two_roots_identical: bool,
}
#[derive(Deserialize)]
pub struct InstalledFile {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn reference(app: &str, version: &str) -> Result<InstalledApp> {
    if digest(REFERENCE) != PIN {
        return Err("legacy_reference_changed");
    }
    let reference: Reference =
        serde_json::from_slice(REFERENCE).map_err(|_| "legacy_reference_invalid")?;
    if reference.schema_version != 1
        || reference.baseline_tag != "v0.7.0"
        || reference.manifest_sha256
            != "e92dda897d40de1891d8a204d20f28524674c0b336f09923d52381fd764217ab"
        || reference.result != "passed"
        || reference.apps.len() != 15
    {
        return Err("legacy_reference_invalid");
    }
    let selected = reference
        .apps
        .into_iter()
        .find(|entry| entry.id == app && entry.version == version)
        .ok_or("legacy_version_unverified")?;
    if !selected.two_roots_identical
        || selected.files.len() > 512
        || !selected
            .files
            .iter()
            .any(|file| file.name == format!("{app}.exe"))
        || !selected
            .files
            .iter()
            .any(|file| file.name == "uninstall.exe")
    {
        return Err("legacy_reference_invalid");
    }
    let mut names = BTreeSet::new();
    for file in &selected.files {
        if file.name.len() > 256
            || !names.insert(file.name.to_ascii_lowercase())
            || file.name.split('/').any(|part| {
                part.is_empty()
                    || part == "."
                    || part == ".."
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            })
            || file.size == 0
            || file.size > 512 * 1024 * 1024
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("legacy_reference_invalid");
        }
    }
    Ok(selected)
}
/// The Windows caller pins ancestor directories. Retain file handles through
/// the caller's decision; sharing permits reads only, so live writers fail shut.
pub fn verify(root: &Path, app: &str, version: &str) -> Result<Vec<File>> {
    let selected = reference(app, version)?;
    let mut handles = Vec::new();
    for asset in selected.files {
        let path = root.join(&asset.name);
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "legacy_installation_unsafe")?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| "legacy_installation_unavailable")?;
        let identity = devbox_filesystem::opened_filesystem_identity(&file, false)
            .map_err(|_| "legacy_installation_unsafe")?;
        let mut hasher = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 65536];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|_| "legacy_installation_unavailable")?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > asset.size {
                return Err("legacy_installation_changed");
            }
            hasher.update(&buffer[..count]);
        }
        let hash: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if bytes != asset.size
            || hash != asset.sha256
            || devbox_filesystem::filesystem_identity(&path, false)
                .map_err(|_| "legacy_installation_changed")?
                != identity
        {
            return Err("legacy_installation_changed");
        }
        handles.push(file);
    }
    Ok(handles)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_reference_includes_uninstaller_resources_and_companion() {
        let app = reference("code-pad", "0.5.1").unwrap();
        assert_eq!(app.files.len(), 4);
        assert!(app
            .files
            .iter()
            .any(|file| file.name == "fake-lsp-server.exe"));
        assert!(reference("code-pad", "0.5.2").is_err());
        assert!(reference("../code-pad", "0.5.1").is_err());
    }
}
