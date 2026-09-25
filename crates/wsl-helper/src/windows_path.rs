//! Resolve DOS drive aliases before path IO. QueryDosDevice reads the native
//! device namespace without connecting a share or starting a WSL distribution.
use std::path::Path;
type Result<T> = std::result::Result<T, &'static str>;

#[cfg_attr(not(windows), allow(dead_code))]
fn drive_path(raw: &str) -> Result<String> {
    let raw = raw.replace('/', "\\");
    let bytes = raw.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return Err("native_path_transport_denied");
    }
    if bytes.len() > 3 && devbox_filesystem::parse_safe_project_path(&raw).is_none() {
        return Err("native_path_transport_denied");
    }
    Ok(raw)
}
#[cfg_attr(not(windows), allow(dead_code))]
fn resolve_drive(raw: &str, mut query: impl FnMut(&str) -> Result<String>) -> Result<String> {
    let mut path = drive_path(raw)?;
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..8 {
        let drive = path[..2].to_ascii_uppercase();
        if !visited.insert(drive.clone()) {
            return Err("native_path_transport_denied");
        }
        let target = query(&drive)?;
        let lower = target.to_ascii_lowercase();
        // Fixed/removable disks and optical media resolve to local device
        // objects. Redirector, MUP, WebDAV and unknown providers are not probed.
        if [
            r"\device\harddiskvolume",
            r"\device\cdrom",
            r"\device\floppy",
        ]
        .iter()
        .any(|prefix| {
            lower.strip_prefix(prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        }) {
            return Ok(path);
        }
        let alias = target
            .strip_prefix(r"\??\")
            .ok_or("native_path_transport_denied")?;
        let base = drive_path(alias)?;
        path = drive_path(&format!("{}\\{}", base.trim_end_matches('\\'), &path[3..]))?;
    }
    Err("native_path_transport_denied")
}
#[cfg(windows)]
fn query_device(device: &str) -> Result<String> {
    use windows::{core::PCWSTR, Win32::Storage::FileSystem::QueryDosDeviceW};
    let name = device.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut buffer = vec![0u16; 32768];
    let count = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut buffer)) };
    if count == 0 {
        return Err("native_path_transport_denied");
    }
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .ok_or("native_path_transport_denied")?;
    String::from_utf16(&buffer[..end]).map_err(|_| "native_path_transport_denied")
}
/// Ordinary UNC paths are explicit Windows scopes. WSL UNC and drive aliases
/// that resolve to remote/unknown providers require another native owner.
pub fn admit(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let text = path.to_str().ok_or("native_path_transport_denied")?;
        let text = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            text.strip_prefix(r"\\?\").unwrap_or(text).to_owned()
        };
        let normalized = text.replace('/', "\\");
        if let Some(unc) = normalized.strip_prefix(r"\\") {
            let server = unc.split('\\').next().unwrap_or_default();
            return if server.eq_ignore_ascii_case("wsl.localhost")
                || server.eq_ignore_ascii_case("wsl$")
                || matches!(server, "?" | "." | "")
            {
                Err("native_path_transport_denied")
            } else {
                Ok(())
            };
        }
        let resolved = resolve_drive(&normalized, query_device)?;
        if resolved != normalized {
            // A SUBST root hides its original ancestors. Inspect the resolved
            // physical spelling from the root down before using the alias.
            match devbox_filesystem::ensure_no_links(Path::new(&resolved)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("native_path_transport_denied"),
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_subst_chains_resolve_without_filesystem_or_distro_calls() {
        let mut seen = Vec::new();
        let path = resolve_drive(r"X:\한글 space\file.rs", |drive| {
            seen.push(drive.to_owned());
            Ok(match drive {
                "X:" => r"\??\Y:\first",
                "Y:" => r"\??\C:\second",
                "C:" => r"\Device\HarddiskVolume3",
                _ => panic!(),
            }
            .into())
        })
        .unwrap();
        assert_eq!(path, r"C:\second\first\한글 space\file.rs");
        assert_eq!(seen, ["X:", "Y:", "C:"]);
    }
    #[test]
    fn remote_wsl_unknown_and_looping_drive_aliases_are_rejected_before_io() {
        for target in [
            r"\Device\Mup\wsl.localhost\Ubuntu",
            r"\Device\LanmanRedirector\server\share",
            r"\Device\WebDavRedirector\server",
            r"\??\UNC\wsl$\Ubuntu",
            r"\Device\UnreviewedVolume",
            r"\??\X:\loop",
        ] {
            assert!(
                resolve_drive(r"X:\project", |_| Ok(target.into())).is_err(),
                "{target}"
            );
        }
    }
}

#[cfg(all(test, windows))]
mod native_device_tests {
    use super::*;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            DefineDosDeviceW, DDD_EXACT_MATCH_ON_REMOVE, DDD_RAW_TARGET_PATH, DDD_REMOVE_DEFINITION,
        },
    };
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    #[test]
    fn private_native_device_mapping_is_classified_without_opening_its_wsl_target() {
        // A unique DOS object, not a drive letter or an existing user mapping.
        let name = wide(&format!("DevboxPathFixture-{}", uuid::Uuid::new_v4()));
        let target = wide(r"\Device\Mup\wsl.localhost\UnstartedDevboxFixture");
        unsafe {
            DefineDosDeviceW(
                DDD_RAW_TARGET_PATH,
                PCWSTR(name.as_ptr()),
                PCWSTR(target.as_ptr()),
            )
            .unwrap();
        }
        struct Remove {
            name: Vec<u16>,
            target: Vec<u16>,
        }
        impl Drop for Remove {
            fn drop(&mut self) {
                unsafe {
                    let _ = DefineDosDeviceW(
                        DDD_RAW_TARGET_PATH | DDD_REMOVE_DEFINITION | DDD_EXACT_MATCH_ON_REMOVE,
                        PCWSTR(self.name.as_ptr()),
                        PCWSTR(self.target.as_ptr()),
                    );
                }
            }
        }
        let owned = Remove { name, target };
        let name = String::from_utf16(&owned.name[..owned.name.len() - 1]).unwrap();
        let mapped = query_device(&name).unwrap();
        assert_eq!(mapped, r"\Device\Mup\wsl.localhost\UnstartedDevboxFixture");
        assert!(resolve_drive(r"X:\never-opened", |_| Ok(mapped.clone())).is_err());
        drop(owned);
        assert!(query_device(&name).is_err());
    }
}
