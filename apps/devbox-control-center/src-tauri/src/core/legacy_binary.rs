//! Legacy image identity comes from the pinned public v0.7 release, never from
//! an executable name or ARP display string. This does not authorize uninstall.
use devbox_manager_lib::core::{asset::validate_manifest_artifacts, manifest::parse_manifest};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
const MANIFEST: &[u8] = include_bytes!("../../resources/legacy-v0.7-manifest.json");
const PIN: &str = "e92dda897d40de1891d8a204d20f28524674c0b336f09923d52381fd764217ab";
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn verify(app: &str, version: &str, path: &Path) -> Result<File, &'static str> {
    if hash(MANIFEST) != PIN {
        return Err("legacy_manifest_changed");
    }
    let manifest =
        parse_manifest(std::str::from_utf8(MANIFEST).map_err(|_| "legacy_manifest_invalid")?)
            .map_err(|_| "legacy_manifest_invalid")?;
    validate_manifest_artifacts(&manifest).map_err(|_| "legacy_manifest_invalid")?;
    if manifest.release_tag != "v0.7.0" || manifest.apps.len() != 15 {
        return Err("legacy_manifest_invalid");
    }
    let asset = manifest
        .apps
        .iter()
        .find(|entry| entry.id == app && entry.version == version)
        .ok_or("legacy_version_unverified")?;
    if asset.portable.name != format!("{app}.exe")
        || asset.portable.size <= 0
        || asset.portable.size > 512 * 1024 * 1024
    {
        return Err("legacy_image_invalid");
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| "legacy_image_unsafe")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "legacy_image_unavailable")?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "legacy_image_unavailable")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > asset.portable.size as u64 {
            return Err("legacy_image_changed");
        }
        hasher.update(&buffer[..count]);
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != asset.portable.sha256
        || total != asset.portable.size as u64
        || devbox_filesystem::filesystem_identity(path, false)
            .map_err(|_| "legacy_image_changed")?
            != identity
    {
        return Err("legacy_image_changed");
    }
    Ok(file)
}
