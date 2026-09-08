//! Native lifetime ownership. Legacy SQLite write handles must quiesce before
//! binding their vault; independent product installations share one vault lease.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub struct VaultOwner {
    _legacy: Option<File>,
    _exclusive: File,
    _root: Option<File>,
}
fn key(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.len() > 32768 || raw.chars().any(char::is_control) {
        return Err("vault_binding_invalid".into());
    }
    let raw = raw.replace('\\', "/");
    let raw = if raw
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("//?/UNC/"))
    {
        format!("//{}", &raw[8..])
    } else if let Some(path) = raw.strip_prefix("//?/") {
        path.to_owned()
    } else {
        raw
    };
    if let Some(wsl) =
        devbox_wsl::path::parse_wsl_unc_path(&raw).map_err(|_| "vault_binding_invalid")?
    {
        let linux = wsl.linux_path();
        let mount = linux.strip_prefix("/mnt/").filter(|rest| {
            rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && (rest.len() == 1 || rest.as_bytes().get(1) == Some(&b'/'))
        });
        if let Some(mount) = mount {
            return Ok(format!(
                "win:{}:/{}",
                mount[..1].to_ascii_lowercase(),
                mount[1..].trim_matches('/').to_ascii_lowercase()
            ));
        }
        return Ok(wsl.identity());
    }
    #[cfg(unix)]
    if raw.starts_with('/') && !raw.starts_with("//") {
        return Ok(format!("unix:{}", raw.trim_end_matches('/')));
    }
    devbox_wsl::path::canonical_project_key(Some(&raw), None)
        .map(|key| key.to_ascii_lowercase())
        .map_err(|_| "vault_binding_invalid".into())
}
pub fn same_vault(left: &Path, right: &Path) -> bool {
    key(&left.to_string_lossy())
        .ok()
        .zip(key(&right.to_string_lossy()).ok())
        .is_some_and(|(left, right)| left == right)
}
fn directories(base: &Path) -> Result<PathBuf, String> {
    devbox_filesystem::ensure_no_links(base).map_err(|_| "vault_owner_unavailable")?;
    let mut path = base.to_owned();
    for part in ["devbox", "integration", "knowledge-vault-leases"] {
        path.push(part);
        if !path.exists() {
            fs::create_dir(&path).map_err(|_| "vault_owner_unavailable")?;
        }
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "vault_owner_unavailable")?;
        if !path.is_dir() {
            return Err("vault_owner_unavailable".into());
        }
    }
    Ok(path)
}
#[cfg(windows)]
fn legacy_guard(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    devbox_filesystem::ensure_no_links(path).map_err(|_| "vault_binding_invalid")?;
    // Share-read only rejects an already-open legacy read/write connection and
    // denies a new SQLite writer until this product releases ownership.
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|_| "legacy_writer_active".into())
}
#[cfg(not(windows))]
fn legacy_guard(_path: &Path) -> Result<File, String> {
    Err("vault_guard_platform_unavailable".into())
}
#[cfg(windows)]
fn root_guard(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    OpenOptions::new()
        // Attribute-only opens do not participate in Windows share-access
        // checks. Directory read access is required to deny rename/delete.
        .access_mode((FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES).0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0)
        .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0)
        .open(path)
        .map_err(|_| "vault_owner_unavailable".into())
}
#[cfg(not(windows))]
fn root_guard(path: &Path) -> Result<File, String> {
    File::open(path).map_err(|_| "vault_owner_unavailable".into())
}
pub fn acquire(base: &Path, vault: &Path, legacy: Option<&Path>) -> Result<VaultOwner, String> {
    let legacy = legacy.map(legacy_guard).transpose()?;
    // WSL transport availability must not block product startup. A lexical
    // lease unifies its UNC and /mnt/drive aliases; every actual note mutation
    // still validates VaultIdentity after the source reconnects.
    let wsl = devbox_wsl::path::parse_wsl_unc_path(&vault.to_string_lossy())
        .map_err(|_| "vault_binding_invalid")?
        .is_some();
    let (canonical, root) = if wsl {
        (vault.to_owned(), None)
    } else {
        match vault.canonicalize() {
            Ok(canonical) => {
                devbox_filesystem::ensure_no_links(vault).map_err(|_| "vault_binding_invalid")?;
                devbox_filesystem::ensure_no_links(&canonical)
                    .map_err(|_| "vault_binding_invalid")?;
                let identity = devbox_filesystem::filesystem_identity(&canonical, true)
                    .map_err(|_| "vault_binding_invalid")?;
                let root = root_guard(&canonical)?;
                if devbox_filesystem::filesystem_identity(vault, true)
                    .map_err(|_| "vault_binding_invalid")?
                    != identity
                {
                    return Err("vault_binding_invalid".into());
                }
                (canonical, Some(root))
            }
            Err(_) => return Err("vault_binding_unavailable".into()),
        }
    };
    let hash: String = Sha256::digest(key(&canonical.to_string_lossy())?.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let path = directories(base)?.join(format!("{hash}.lock"));
    if path.exists() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "vault_owner_unavailable")?;
    }
    let exclusive = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|_| "vault_owner_unavailable")?;
    exclusive.try_lock().map_err(|_| "vault_owner_busy")?;
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "vault_owner_unavailable")?;
    Ok(VaultOwner {
        _legacy: legacy,
        _exclusive: exclusive,
        _root: root,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_share_one_key_while_linux_paths_keep_case() {
        assert_eq!(
            key(r"C:\Vault\Notes").unwrap(),
            key("c:/vault/notes/").unwrap()
        );
        assert_eq!(
            key(r"\\wsl$\Ubuntu\home\user\Vault").unwrap(),
            key(r"\\wsl.localhost\ubuntu\home\user\Vault").unwrap()
        );
        assert_ne!(
            key(r"\\wsl$\Ubuntu\home\user\Vault").unwrap(),
            key(r"\\wsl$\Ubuntu\home\user\vault").unwrap()
        );
        assert_eq!(
            key(r"\\wsl.localhost\Ubuntu\mnt\c\Vault\Notes").unwrap(),
            key("c:/vault/notes").unwrap()
        );
        assert_eq!(
            key(r"\\?\C:\Vault\Notes").unwrap(),
            key("c:/vault/notes").unwrap()
        );
    }
    #[test]
    fn two_product_installations_cannot_own_one_vault() {
        let base = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let owner = acquire(base.path(), vault.path(), None).unwrap();
        assert!(
            matches!(acquire(base.path(),vault.path(),None),Err(error) if error=="vault_owner_busy")
        );
        drop(owner);
        assert!(acquire(base.path(), vault.path(), None).is_ok());
    }
    #[test]
    fn unavailable_wsl_binding_acquires_only_a_local_lease_without_source_io() {
        let base = tempfile::tempdir().unwrap();
        let path = Path::new(r"\\wsl.localhost\DevboxMissingFixture\home\fixture\vault");
        let owner = acquire(base.path(), path, None).unwrap();
        assert!(owner._root.is_none());
        let alias = Path::new(r"\\wsl$\devboxmissingfixture\home\fixture\vault");
        assert!(
            matches!(acquire(base.path(), alias, None), Err(error) if error == "vault_owner_busy")
        );
    }
    #[cfg(windows)]
    #[test]
    fn windows_legacy_write_handles_and_vault_rename_are_blocked_for_owner_lifetime() {
        let base = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let db = base.path().join("legacy.db");
        fs::write(&db, []).unwrap();
        let writer = OpenOptions::new().read(true).write(true).open(&db).unwrap();
        assert!(acquire(base.path(), vault.path(), Some(&db)).is_err());
        drop(writer);
        let owner = acquire(base.path(), vault.path(), Some(&db)).unwrap();
        assert!(OpenOptions::new().read(true).write(true).open(&db).is_err());
        assert!(fs::rename(vault.path(), vault.path().with_extension("moved")).is_err());
        drop(owner);
        assert!(OpenOptions::new().read(true).write(true).open(&db).is_ok());
        let moved = vault.path().with_extension("moved");
        fs::rename(vault.path(), &moved).unwrap();
        fs::rename(&moved, vault.path()).unwrap();
    }
}
