//! Native WSL registration and running-state inspection. Friendly names are
//! display metadata; GUID plus retained backing-directory identity owns a binding.
use serde::Serialize;
#[cfg(windows)]
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Distro {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub running: bool,
}

#[cfg(windows)]
mod native {
    use super::*;
    use devbox_filesystem::{
        ensure_no_links, filesystem_identity, open_filesystem_metadata_object,
        open_filesystem_object, FilesystemIdentity,
    };
    use sha2::{Digest, Sha256};
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME},
            System::{Registry::*, SystemInformation::GetSystemDirectoryW},
        },
    };
    const ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    const LIMIT: usize = 128;
    struct Key(HKEY);
    // A read-only registry handle may be queried from any native worker. It is
    // retained until the lease drops, so deletion/recreation cannot replace it.
    unsafe impl Send for Key {}
    unsafe impl Sync for Key {}
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn open(parent: HKEY, path: &str) -> Result<Option<Key>> {
        let path = wide(path);
        let mut key = HKEY::default();
        let result =
            unsafe { RegOpenKeyExW(parent, PCWSTR(path.as_ptr()), None, KEY_READ, &mut key) };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err("wsl_registry_unavailable");
        }
        Ok(Some(Key(key)))
    }
    fn text(key: &Key, name: &str) -> Result<String> {
        optional_text(key, name)?.ok_or("wsl_registry_invalid")
    }
    fn optional_text(key: &Key, name: &str) -> Result<Option<String>> {
        let name = wide(name);
        let mut value = vec![0_u16; 32768];
        let mut bytes = (value.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                key.0,
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS
            || bytes < 2
            || bytes as usize > value.len() * 2
            || !bytes.is_multiple_of(2)
        {
            return Err("wsl_registry_invalid");
        }
        let used = bytes as usize / 2;
        if value[used - 1] != 0 || value[..used - 1].contains(&0) {
            return Err("wsl_registry_invalid");
        }
        String::from_utf16(&value[..used - 1])
            .map(Some)
            .map_err(|_| "wsl_registry_invalid")
    }
    fn number(key: &Key, name: &str) -> Result<u32> {
        let name = wide(name);
        let mut value = 0_u32;
        let mut bytes = 4_u32;
        let result = unsafe {
            RegGetValueW(
                key.0,
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut value as *mut u32).cast()),
                Some(&mut bytes),
            )
        };
        if result != ERROR_SUCCESS || bytes != 4 {
            return Err("wsl_registry_invalid");
        }
        Ok(value)
    }
    fn revision(key: &Key) -> Result<u64> {
        let mut time = FILETIME::default();
        let result = unsafe {
            RegQueryInfoKeyW(
                key.0,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&mut time),
            )
        };
        if result != ERROR_SUCCESS {
            return Err("wsl_registry_unavailable");
        }
        Ok((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime))
    }
    fn values_digest(key: &Key) -> Result<[u8; 32]> {
        let mut values: std::collections::BTreeMap<Vec<u16>, (u32, Vec<u8>)> =
            std::collections::BTreeMap::new();
        let mut total = 0usize;
        for index in 0..=LIMIT {
            let mut name = vec![0u16; 16385];
            let mut name_size = name.len() as u32;
            let mut data = vec![0u8; 65536];
            let mut data_size = data.len() as u32;
            let mut kind = 0u32;
            let status = unsafe {
                RegEnumValueW(
                    key.0,
                    index as u32,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_size,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_size),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                let mut hash = Sha256::new();
                for (name, (kind, data)) in values {
                    hash.update((name.len() as u32).to_le_bytes());
                    for unit in name {
                        hash.update(unit.to_le_bytes());
                    }
                    hash.update(kind.to_le_bytes());
                    hash.update((data.len() as u32).to_le_bytes());
                    hash.update(data);
                }
                return Ok(hash.finalize().into());
            }
            if status != ERROR_SUCCESS
                || index == LIMIT
                || name_size as usize >= name.len()
                || data_size as usize > data.len()
            {
                return Err("wsl_registry_invalid");
            }
            name.truncate(name_size as usize);
            data.truncate(data_size as usize);
            total += name.len() * 2 + data.len();
            if total > 1024 * 1024 || values.insert(name, (kind, data)).is_some() {
                return Err("wsl_registry_invalid");
            }
        }
        Err("wsl_registry_limit")
    }
    #[derive(Clone, PartialEq, Eq)]
    struct Registration {
        id: String,
        name: String,
        version: u32,
        base: PathBuf,
        backing: Option<PathBuf>,
        values_digest: [u8; 32],
    }
    fn registration(key: &Key, id: &str) -> Result<Registration> {
        for attempt in 0..3 {
            match registration_snapshot(key, id) {
                Err("wsl_registry_read_changed") if attempt < 2 => continue,
                result => return result,
            }
        }
        Err("wsl_registry_read_changed")
    }
    fn registration_snapshot(key: &Key, id: &str) -> Result<Registration> {
        let before = revision(key)?;
        let name = text(key, "DistributionName")?;
        let version = number(key, "Version")?;
        let raw = text(key, "BasePath")?;
        if name.is_empty()
            || name.trim() != name
            || name.chars().count() > 128
            || name.chars().any(char::is_control)
            || !matches!(version, 1 | 2)
        {
            return Err("wsl_registry_invalid");
        }
        let raw = raw.strip_prefix(r"\??\").unwrap_or(&raw);
        let base = PathBuf::from(raw);
        let backing = if version == 2 {
            // WSL's native DistributionRegistration::ReadVhdFilePath uses
            // BasePath / VhdFileName, with ext4.vhdx as the missing-value default.
            let leaf = optional_text(key, "VhdFileName")?.unwrap_or_else(|| "ext4.vhdx".into());
            if leaf.is_empty()
                || leaf == "."
                || leaf == ".."
                || leaf.encode_utf16().count() > 255
                || leaf.contains(['\\', '/', ':'])
            {
                return Err("wsl_registry_invalid");
            }
            Some(base.join(leaf))
        } else {
            None
        };
        let values_digest = values_digest(key)?;
        if !base.is_absolute() {
            return Err("wsl_registry_invalid");
        }
        if revision(key)? != before {
            return Err("wsl_registry_read_changed");
        }
        Ok(Registration {
            id: id.into(),
            name,
            version,
            base,
            backing,
            values_digest,
        })
    }
    fn registrations() -> Result<Vec<Registration>> {
        for attempt in 0..3 {
            match registration_list_snapshot() {
                Err("wsl_registry_entry_missing" | "wsl_registry_list_changed") if attempt < 2 => {
                    continue
                }
                result => return result,
            }
        }
        Err("wsl_registry_list_changed")
    }
    fn registration_list_snapshot() -> Result<Vec<Registration>> {
        let Some(root) = open(HKEY_CURRENT_USER, ROOT)? else {
            return Ok(vec![]);
        };
        registrations_under(&root)
    }
    fn registrations_under(root: &Key) -> Result<Vec<Registration>> {
        let before = revision(root)?;
        let mut result = Vec::new();
        for index in 0..=LIMIT {
            let mut name = [0_u16; 256];
            let mut length = name.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    root.0,
                    index as u32,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                if revision(root)? != before {
                    return Err("wsl_registry_list_changed");
                }
                return Ok(result);
            }
            if status != ERROR_SUCCESS || index == LIMIT {
                return Err("wsl_registry_limit");
            }
            let key_name =
                String::from_utf16(&name[..length as usize]).map_err(|_| "wsl_registry_invalid")?;
            let id = uuid::Uuid::parse_str(&key_name)
                .map_err(|_| "wsl_registry_invalid")?
                .to_string();
            let key = open(root.0, &key_name)?.ok_or("wsl_registry_entry_missing")?;
            result.push(registration(&key, &id)?);
        }
        Err("wsl_registry_limit")
    }
    pub fn executable() -> Result<PathBuf> {
        let mut buffer = vec![0_u16; 32768];
        let count = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
        if count == 0 || count >= buffer.len() {
            return Err("wsl_unavailable");
        }
        let directory =
            PathBuf::from(String::from_utf16(&buffer[..count]).map_err(|_| "wsl_unavailable")?);
        let executable = directory.join("wsl.exe");
        ensure_no_links(&executable).map_err(|_| "wsl_unavailable")?;
        if !executable.is_file() {
            return Err("wsl_unavailable");
        }
        Ok(executable)
    }
    /// Executes only fixed system WSL operations supplied by this native module
    /// or the helper launcher. Captures at most 64 KiB on each stream.
    pub async fn output(args: &[String], budget: Duration) -> Result<Vec<u8>> {
        use std::process::Stdio;
        use tokio::io::AsyncReadExt;
        let executable = executable()?;
        let system = executable.parent().ok_or("wsl_unavailable")?;
        let root = system.parent().ok_or("wsl_unavailable")?;
        let mut command = tokio::process::Command::new(&executable);
        command
            .args(args)
            .current_dir(system)
            .env_clear()
            .env("SystemRoot", root)
            .env("PATH", system)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|_| "wsl_unavailable")?;
        let stdout = child.stdout.take().ok_or("wsl_unavailable")?;
        let stderr = child.stderr.take().ok_or("wsl_unavailable")?;
        async fn read(stream: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            stream
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "wsl_unavailable")?;
            if bytes.len() > 65536 {
                return Err("wsl_output_limit");
            }
            Ok(bytes)
        }
        let result = tokio::time::timeout(budget, async {
            let (out, _, status) = tokio::try_join!(read(stdout), read(stderr), async {
                child.wait().await.map_err(|_| "wsl_unavailable")
            })?;
            if !status.success() {
                return Err("wsl_operation_failed");
            }
            Ok(out)
        })
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            other => {
                let _ = child.kill().await;
                match other {
                    Ok(Err(error)) => Err(error),
                    _ => Err("wsl_timeout"),
                }
            }
        }
    }
    fn running() -> Result<Vec<String>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "wsl_unavailable")?;
        let args = ["--list", "--running", "--quiet"].map(str::to_owned);
        let bytes = runtime.block_on(output(&args, Duration::from_secs(5)))?;
        let output = devbox_wsl::output::decode_output(&bytes);
        if output.contains('\u{fffd}') {
            return Err("wsl_output_invalid");
        }
        let names = output
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if names.len() > LIMIT {
            return Err("wsl_output_limit");
        }
        Ok(names)
    }
    pub fn list() -> Result<Vec<Distro>> {
        let registrations = registrations()?;
        if registrations.is_empty() {
            return Ok(vec![]);
        }
        let active = running()?;
        Ok(registrations
            .into_iter()
            .map(|entry| Distro {
                id: entry.id,
                running: active.contains(&entry.name),
                name: entry.name,
                version: entry.version,
            })
            .collect())
    }
    pub struct Lease {
        registration: Registration,
        identity: FilesystemIdentity,
        backing: Option<(File, FilesystemIdentity)>,
        _directory: File,
        key: Key,
    }
    impl Lease {
        pub fn capture(id: &str, allow_start: bool) -> Result<Self> {
            let id = uuid::Uuid::parse_str(id)
                .map_err(|_| "wsl_distro_invalid")?
                .to_string();
            let registration = registrations()?
                .into_iter()
                .find(|entry| entry.id == id)
                .ok_or("wsl_distro_missing")?;
            if !allow_start && !running()?.contains(&registration.name) {
                return Err("wsl_distro_stopped");
            }
            super::super::windows_path::admit(&registration.base)?;
            ensure_no_links(&registration.base).map_err(|_| "wsl_storage_path_unsafe")?;
            let (directory, identity) = open_filesystem_object(&registration.base, true)
                .map_err(|_| "wsl_storage_unavailable")?;
            let backing = registration
                .backing
                .as_ref()
                .map(|path| {
                    super::super::windows_path::admit(path)?;
                    ensure_no_links(path).map_err(|_| "wsl_backing_path_unsafe")?;
                    open_filesystem_metadata_object(path, false)
                        .map_err(|_| "wsl_backing_unavailable")
                })
                .transpose()?;
            let key = open(HKEY_CURRENT_USER, &format!("{ROOT}\\{{{id}}}"))?
                .ok_or("wsl_distro_missing")?;
            let result = Self {
                registration,
                identity,
                backing,
                _directory: directory,
                key,
            };
            result.revalidate()?;
            Ok(result)
        }
        pub fn id(&self) -> &str {
            &self.registration.id
        }
        pub fn name(&self) -> &str {
            &self.registration.name
        }
        pub fn revalidate(&self) -> Result<()> {
            // The key itself must still exist. LastWriteTime is a snapshot
            // consistency check, not identity: WSL can rewrite identical values.
            if registration(&self.key, self.id())? != self.registration {
                return Err("wsl_retained_registration_changed");
            }
            let current = registrations()?
                .into_iter()
                .find(|entry| entry.id == self.registration.id)
                .ok_or("wsl_distro_missing")?;
            if current != self.registration {
                return Err("wsl_registration_changed");
            }
            ensure_no_links(&current.base).map_err(|_| "wsl_storage_path_unsafe")?;
            if filesystem_identity(&current.base, true).map_err(|_| "wsl_storage_unavailable")?
                != self.identity
            {
                return Err("wsl_storage_object_changed");
            }
            if let Some(path) = &current.backing {
                ensure_no_links(path).map_err(|_| "wsl_backing_path_unsafe")?;
                if self.backing.as_ref().map(|(_, id)| *id)
                    != Some(
                        filesystem_identity(path, false).map_err(|_| "wsl_backing_unavailable")?,
                    )
                {
                    return Err("wsl_backing_object_changed");
                }
            }
            Ok(())
        }
        pub fn scope(&self, filesystem_scope: &str) -> String {
            let (volume, object) = self.identity.components();
            let mut hash = Sha256::new();
            hash.update(self.id());
            hash.update(volume.to_le_bytes());
            hash.update(object.to_le_bytes());
            if let Some((_, backing)) = &self.backing {
                let (volume, object) = backing.components();
                hash.update([1]);
                hash.update(volume.to_le_bytes());
                hash.update(object.to_le_bytes());
            } else {
                hash.update([0]);
            }
            hash.update(filesystem_scope);
            hash.finalize()[..16]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }
        /// WSL accepts an absolute Windows --cd path. Its native launcher
        /// maps the installed resource directory; no shell or wslpath utility
        /// participates in selecting the first-party helper executable.
        pub fn helper_args(
            &self,
            directory: &Path,
            session: &str,
            allow_start: bool,
        ) -> Result<Vec<String>> {
            self.revalidate()?;
            if !allow_start && !running()?.contains(&self.registration.name) {
                return Err("wsl_distro_stopped");
            }
            if !directory.is_absolute() || uuid::Uuid::parse_str(session).is_err() {
                return Err("wsl_request_invalid");
            }
            super::super::windows_path::admit(directory)?;
            ensure_no_links(directory).map_err(|_| "wsl_helper_unavailable")?;
            Ok(vec![
                "--distribution-id".into(),
                self.id().into(),
                "--cd".into(),
                directory.to_str().ok_or("wsl_helper_unavailable")?.into(),
                "--exec".into(),
                "./devbox-workspace-wsl".into(),
                "--session".into(),
                session.into(),
            ])
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn create(path: &str) -> Key {
            let mut key = HKEY::default();
            let name = wide(path);
            assert_eq!(
                unsafe { RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr()), &mut key) },
                ERROR_SUCCESS
            );
            Key(key)
        }
        fn set(key: &Key, name: &str, kind: REG_VALUE_TYPE, bytes: &[u8]) {
            let name = wide(name);
            assert_eq!(
                unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, kind, Some(bytes)) },
                ERROR_SUCCESS
            );
        }
        fn set_text(key: &Key, name: &str, value: &str) {
            let bytes = wide(value)
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            set(key, name, REG_SZ, &bytes);
        }
        fn initialize(key: &Key, base: &Path) {
            set_text(key, "DistributionName", "native-registry-fixture");
            set_text(key, "BasePath", base.to_str().unwrap());
            for (name, value) in [
                ("Version", 1u32),
                ("DefaultUid", 1000),
                ("Flags", 7),
                ("State", 1),
            ] {
                set(key, name, REG_DWORD, &value.to_le_bytes());
            }
        }
        #[test]
        fn registry_value_identity_survives_rewrites_but_not_policy_or_key_replacement() {
            let path = format!("Software\\DevboxRegistryFixture-{}", uuid::Uuid::new_v4());
            assert!(open(HKEY_CURRENT_USER, &path).unwrap().is_none());
            struct Owned(String);
            impl Drop for Owned {
                fn drop(&mut self) {
                    let name = wide(&self.0);
                    unsafe {
                        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr()));
                    }
                }
            }
            let owned = Owned(path);
            let key = create(&owned.0);
            let directory = tempfile::tempdir().unwrap();
            assert!(registrations_under(&key).unwrap().is_empty());
            let child_id = uuid::Uuid::new_v4().to_string();
            let child_name = format!("{}\\{{{child_id}}}", owned.0);
            let child = create(&child_name);
            initialize(&child, directory.path());
            let listed = registrations_under(&key).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, child_id);
            drop(child);
            let child_name = wide(&child_name);
            assert_eq!(
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(child_name.as_ptr())) },
                ERROR_SUCCESS
            );
            assert!(registrations_under(&key).unwrap().is_empty());
            initialize(&key, directory.path());
            let before = registration(&key, "fixture").unwrap();
            set(&key, "State", REG_DWORD, &1u32.to_le_bytes());
            assert!(before == registration(&key, "fixture").unwrap());
            set(&key, "State", REG_DWORD, &2u32.to_le_bytes());
            assert!(before != registration(&key, "fixture").unwrap());
            set(&key, "State", REG_DWORD, &1u32.to_le_bytes());
            assert!(before == registration(&key, "fixture").unwrap());
            set(&key, "DefaultUid", REG_DWORD, &0u32.to_le_bytes());
            assert!(before != registration(&key, "fixture").unwrap());
            set(&key, "DefaultUid", REG_DWORD, &1000u32.to_le_bytes());
            set_text(&key, "NewExecutionProperty", "changed");
            assert!(before != registration(&key, "fixture").unwrap());
            let name = wide(&owned.0);
            assert_eq!(
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr())) },
                ERROR_SUCCESS
            );
            let replacement = create(&owned.0);
            initialize(&replacement, directory.path());
            assert!(before == registration(&replacement, "fixture").unwrap());
            assert!(registration(&key, "fixture").is_err());
        }
    }
}
#[cfg(windows)]
pub use native::{executable, list, output, Lease};
#[cfg(not(windows))]
pub fn list() -> Result<Vec<Distro>> {
    Err("windows_required")
}
