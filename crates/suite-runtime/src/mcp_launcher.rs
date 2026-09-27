//! Owned stable MCP copies. Shared by agent startup and Suite removal.
use devbox_filesystem::{atomic_write, ensure_no_links, filesystem_identity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
const RECEIPT: &str = "mcp-launcher-owned.json";
const STABLE: &str = "devbox-mcp.exe";
const MAX_IMAGE: u64 = 512 * 1024 * 1024;
static SERIAL: Mutex<()> = Mutex::new(());
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Owned {
    schema_version: u32,
    root_identity: (u64, u64),
    files: BTreeMap<String, String>,
}
fn invalid() -> io::Error {
    io::Error::other("mcp_launcher_invalid")
}
pub fn stable_path(root: &Path) -> PathBuf {
    root.join("bin").join(STABLE)
}
fn managed_name(name: &str) -> bool {
    name == STABLE
        || [(".devbox-mcp.old-", ".exe"), (".devbox-mcp.", ".tmp")]
            .iter()
            .any(|(prefix, suffix)| {
                name.strip_prefix(prefix)
                    .and_then(|s| s.strip_suffix(suffix))
                    .is_some_and(|id| {
                        uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
                    })
            })
}
fn digest(path: &Path) -> io::Result<String> {
    ensure_no_links(path)?;
    let mut file = fs::File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_IMAGE {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut bytes = [0; 64 * 1024];
    let mut total = 0;
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_IMAGE {
            return Err(invalid());
        }
        hash.update(&bytes[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn load(root: &Path) -> io::Result<Owned> {
    ensure_no_links(root)?;
    let root_identity = filesystem_identity(root, true)?.components();
    let path = root.join(RECEIPT);
    let bytes = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Owned {
                schema_version: 1,
                root_identity,
                files: BTreeMap::new(),
            })
        }
        Ok(meta) if meta.is_file() && meta.len() <= 64 * 1024 => {
            ensure_no_links(&path)?;
            let mut bytes = Vec::new();
            fs::File::open(&path)?
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            bytes
        }
        _ => return Err(invalid()),
    };
    if bytes.len() > 64 * 1024 {
        return Err(invalid());
    }
    let owned: Owned = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if owned.schema_version != 1
        || owned.root_identity != root_identity
        || owned.files.len() > 64
        || owned.files.iter().any(|(name, hash)| {
            !managed_name(name)
                || hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
    {
        return Err(invalid());
    }
    Ok(owned)
}
fn save(root: &Path, owned: &Owned) -> io::Result<()> {
    atomic_write(
        root.join(RECEIPT),
        &serde_json::to_vec(owned).map_err(|_| invalid())?,
    )
}
fn collect(root: &Path, owned: &mut Owned) -> io::Result<()> {
    for (name, expected) in owned.files.clone() {
        if name == STABLE {
            continue;
        }
        let path = root.join("bin").join(&name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                owned.files.remove(&name);
            }
            Ok(_) => {
                if digest(&path)? != expected {
                    return Err(invalid());
                }
                // A running Windows image remains recorded for a later startup.
                if fs::remove_file(&path).is_ok() {
                    owned.files.remove(&name);
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
pub fn refresh(root: &Path, current_exe: &Path) -> io::Result<PathBuf> {
    let _serial = SERIAL.lock().map_err(|_| invalid())?;
    let mut owned = load(root)?;
    let expected = digest(current_exe)?;
    let bin = root.join("bin");
    match fs::create_dir(&bin) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    ensure_no_links(&bin)?;
    let stable = stable_path(root);
    let prior = match fs::symlink_metadata(&stable) {
        Ok(_) => Some(digest(&stable)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if prior
        .as_ref()
        .is_some_and(|hash| hash != &expected && !owned.files.values().any(|known| known == hash))
    {
        return Err(invalid());
    }
    // Check an interrupted replacement before collecting its absent old-name slot.
    collect(root, &mut owned)?;
    if prior.as_ref() == Some(&expected) {
        owned.files.insert(STABLE.into(), expected);
        save(root, &owned)?;
        return Ok(stable);
    }
    if owned.files.len() > 60 {
        return Err(invalid());
    }
    let temporary_name = format!(".devbox-mcp.{}.tmp", uuid::Uuid::new_v4());
    let temporary = bin.join(&temporary_name);
    let mut source = fs::File::open(current_exe)?.take(MAX_IMAGE + 1);
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if io::copy(&mut source, &mut target)? > MAX_IMAGE {
        return Err(invalid());
    }
    target.flush()?;
    target.sync_all()?;
    drop(target);
    if digest(&temporary)? != expected {
        return Err(invalid());
    }
    owned.files.insert(temporary_name.clone(), expected.clone());
    let retired = prior.map(|hash| {
        let name = format!(".devbox-mcp.old-{}.exe", uuid::Uuid::new_v4());
        owned.files.insert(name.clone(), hash);
        bin.join(name)
    });
    owned.files.insert(STABLE.into(), expected);
    save(root, &owned)?;
    if let Some(retired) = &retired {
        fs::rename(&stable, retired)?;
    }
    if let Err(error) = fs::rename(&temporary, &stable) {
        if let Some(retired) = &retired {
            let _ = fs::rename(retired, &stable);
        }
        return Err(error);
    }
    owned.files.remove(&temporary_name);
    collect(root, &mut owned)?;
    save(root, &owned)?;
    Ok(stable)
}
/// Only the recorded copies are MCP identities; ordinary Suite membership is unchanged.
pub fn verify_image(root: &Path, image: &Path) -> io::Result<()> {
    let owned = load(root)?;
    ensure_no_links(image)?;
    let image = image.canonicalize()?;
    if filesystem_identity(image.parent().ok_or_else(invalid)?, true)?
        != filesystem_identity(root.join("bin"), true)?
    {
        return Err(invalid());
    }
    let name = image
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid)?;
    if name.ends_with(".tmp") {
        return Err(invalid());
    }
    let expected = owned.files.get(name).ok_or_else(invalid)?;
    if digest(&image)? != *expected {
        return Err(invalid());
    }
    Ok(())
}
/// Feed the existing identity-pinned uninstall plan, preserving all unrecorded files.
pub fn owned_files(root: &Path) -> io::Result<Vec<String>> {
    let owned = load(root)?;
    let mut files = Vec::new();
    for (name, expected) in owned.files {
        let path = root.join("bin").join(&name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                if digest(&path)? != expected {
                    return Err(invalid());
                }
                files.push(format!("bin/{name}"));
            }
            Err(error) => return Err(error),
        }
    }
    if root.join(RECEIPT).try_exists()? {
        files.push(RECEIPT.into());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_image_has_a_distinct_role_and_foreign_files_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let image = root.path().join("agent.exe");
        fs::write(&image, b"verified synthetic image").unwrap();
        let stable = refresh(root.path(), &image).unwrap();
        verify_image(root.path(), &stable).unwrap();
        let foreign = root.path().join("bin/foreign.exe");
        fs::copy(&image, &foreign).unwrap();
        assert!(verify_image(root.path(), &foreign).is_err());
        assert_eq!(
            owned_files(root.path()).unwrap(),
            vec!["bin/devbox-mcp.exe", RECEIPT]
        );
        fs::write(&stable, b"foreign replacement").unwrap();
        assert!(verify_image(root.path(), &stable).is_err());
        assert!(refresh(root.path(), &image).is_err());
        assert!(owned_files(root.path()).is_err());
        assert_eq!(fs::read(&stable).unwrap(), b"foreign replacement");
        assert!(foreign.exists());
    }
    #[test]
    fn replacement_resumes_after_the_receipt_precedes_the_rename() {
        let root = tempfile::tempdir().unwrap();
        let image = root.path().join("agent.exe");
        fs::write(&image, b"v1").unwrap();
        let stable = refresh(root.path(), &image).unwrap();
        let old_hash = digest(&stable).unwrap();
        fs::write(&image, b"v2").unwrap();
        let mut owned = load(root.path()).unwrap();
        owned.files.insert(
            format!(".devbox-mcp.old-{}.exe", uuid::Uuid::new_v4()),
            old_hash,
        );
        owned.files.insert(STABLE.into(), digest(&image).unwrap());
        save(root.path(), &owned).unwrap();
        refresh(root.path(), &image).unwrap();
        assert_eq!(fs::read(&stable).unwrap(), b"v2");
        verify_image(root.path(), &stable).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn bin_links_are_never_followed() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let image = root.path().join("agent.exe");
        fs::write(&image, b"fixture").unwrap();
        std::os::unix::fs::symlink(external.path(), root.path().join("bin")).unwrap();
        assert!(refresh(root.path(), &image).is_err());
        assert!(fs::read_dir(external.path()).unwrap().next().is_none());
    }
}
