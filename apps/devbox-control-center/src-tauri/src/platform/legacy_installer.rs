//! Bounded, read-only ARP discovery. Registry strings are declarations, never a
//! command to execute or authority to remove a directory.
use serde::Serialize;
use sha2::{Digest, Sha256};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::Registry::*,
    },
};
type Result<T> = std::result::Result<T, &'static str>;
const ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn open(parent: HKEY, path: &str, view: REG_SAM_FLAGS) -> Result<Option<Key>> {
    let path = wide(path);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            parent,
            PCWSTR(path.as_ptr()),
            None,
            KEY_READ | view,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err("legacy_registry_unavailable");
    }
    Ok(Some(Key(key)))
}
fn text(key: &Key, name: &str) -> Result<Option<String>> {
    let name = wide(name);
    let mut data = [0u16; 4096];
    let mut bytes = (data.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            key.0,
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS
        || bytes < 2
        || bytes as usize > data.len() * 2
        || !bytes.is_multiple_of(2)
    {
        return Err("legacy_registry_value_invalid");
    }
    let size = bytes as usize / 2;
    if data[size - 1] != 0 || data[..size - 1].contains(&0) {
        return Err("legacy_registry_value_invalid");
    }
    String::from_utf16(&data[..size - 1])
        .map(Some)
        .map_err(|_| "legacy_registry_value_invalid")
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub app: String,
    pub version: Option<String>,
    pub scope: &'static str,
    pub architecture: &'static str,
    pub registration_id: String,
    pub binary: &'static str,
    pub cleanup: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Inventory {
    pub entries: Vec<Entry>,
    pub complete: bool,
    pub other_users: &'static str,
}
fn verified_binary(
    app: &str,
    version: Option<&str>,
    location: Option<&str>,
    icon: Option<&str>,
) -> bool {
    let (Some(version), Some(location), Some(icon)) = (version, location, icon) else {
        return false;
    };
    let root = std::path::Path::new(location.trim_matches('"'));
    let Ok(root) = devbox_manager_lib::core::custom_root::verify_suite_directory(root) else {
        return false;
    };
    let Ok(_directories) = crate::suite::platform::component_scope::pin_directories(&root) else {
        return false;
    };
    let expected = root.join(format!("{app}.exe"));
    let icon = icon.strip_suffix(",0").unwrap_or(icon).trim_matches('"');
    let Ok(icon) = std::path::Path::new(icon).canonicalize() else {
        return false;
    };
    let Ok(canonical) = expected.canonicalize() else {
        return false;
    };
    if !icon
        .to_string_lossy()
        .eq_ignore_ascii_case(&canonical.to_string_lossy())
    {
        return false;
    }
    crate::core::legacy_binary::verify(app, version, &expected).is_ok()
}

fn scan(
    hive: HKEY,
    scope: &'static str,
    view: REG_SAM_FLAGS,
    architecture: &'static str,
    catalog: &devbox_catalog::Catalog,
) -> Result<Vec<Entry>> {
    let Some(root) = open(hive, ROOT, view)? else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for index in 0..=4096u32 {
        let mut name = [0u16; 256];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                root.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(entries);
        }
        if status != ERROR_SUCCESS || index == 4096 || length as usize >= name.len() {
            return Err("legacy_registry_limit");
        }
        let name =
            String::from_utf16(&name[..length as usize]).map_err(|_| "legacy_registry_invalid")?;
        let Some(key) = open(root.0, &name, view)? else {
            return Err("legacy_registry_changed");
        };
        let Some(display) = text(&key, "DisplayName")? else {
            continue;
        };
        let Some(app) = catalog.apps.iter().find(|app| {
            display.eq_ignore_ascii_case(&app.product_name) || display.eq_ignore_ascii_case(&app.id)
        }) else {
            continue;
        };
        let version = text(&key, "DisplayVersion")?
            .filter(|s| s.len() <= 64 && !s.chars().any(char::is_control));
        let values = (
            text(&key, "InstallLocation")?,
            text(&key, "UninstallString")?,
            text(&key, "DisplayIcon")?,
        );
        let binary = if verified_binary(
            &app.id,
            version.as_deref(),
            values.0.as_deref(),
            values.2.as_deref(),
        ) {
            "verified"
        } else {
            "unknown"
        };
        // Repeat the bounded metadata observation before publishing its revision.
        if text(&key, "DisplayName")?.as_deref() != Some(display.as_str())
            || values
                != (
                    text(&key, "InstallLocation")?,
                    text(&key, "UninstallString")?,
                    text(&key, "DisplayIcon")?,
                )
        {
            return Err("legacy_registry_changed");
        }
        let bytes = serde_json::to_vec(&(scope, architecture, &name, &app.id, &version, values))
            .map_err(|_| "legacy_registry_invalid")?;
        let registration_id = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        entries.push(Entry {
            app: app.id.clone(),
            version,
            scope,
            architecture,
            registration_id,
            binary,
            cleanup: "requiresVerifiedInstaller",
        });
        if entries.len() > 60 {
            return Err("legacy_registry_limit");
        }
    }
    Err("legacy_registry_limit")
}
pub(crate) fn inventory() -> Result<Inventory> {
    let catalog = devbox_catalog::parse_catalog(include_str!("../../../../catalog.json"))
        .map_err(|_| "legacy_catalog_invalid")?;
    let mut report = Inventory {
        entries: Vec::new(),
        complete: true,
        other_users: "notInspected",
    };
    for (hive, scope) in [
        (HKEY_CURRENT_USER, "currentUser"),
        (HKEY_LOCAL_MACHINE, "machine"),
    ] {
        for (view, architecture) in [(KEY_WOW64_64KEY, "x64"), (KEY_WOW64_32KEY, "x86")] {
            match scan(hive, scope, view, architecture, &catalog) {
                Ok(entries) => report.entries.extend(entries),
                Err(_) => report.complete = false,
            }
        }
    }
    Ok(report)
}
