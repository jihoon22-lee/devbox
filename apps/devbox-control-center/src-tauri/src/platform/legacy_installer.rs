//! Bounded, read-only ARP discovery. Registry strings are declarations, never a
//! command to execute or authority to remove a directory.
use serde::{Deserialize, Serialize};
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
    #[serde(skip)]
    pub registration: Registration,
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
    let Ok(reference) = crate::core::legacy_installation::reference(app, version) else {
        return false;
    };
    let mut resource_directories = Vec::new();
    for file in reference.files {
        let path = root.join(file.name);
        let Some(parent) = path.parent() else {
            return false;
        };
        let Ok(pins) = crate::suite::platform::component_scope::pin_directories(parent) else {
            return false;
        };
        resource_directories.extend(pins);
    }
    crate::core::legacy_installation::verify(&root, app, version).is_ok()
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
            || text(&key, "DisplayVersion")?
                .filter(|s| s.len() <= 64 && !s.chars().any(char::is_control))
                != version
            || values
                != (
                    text(&key, "InstallLocation")?,
                    text(&key, "UninstallString")?,
                    text(&key, "DisplayIcon")?,
                )
        {
            return Err("legacy_registry_changed");
        }
        let registration = Registration {
            name: name.clone(),
            app: app.id.clone(),
            version: version.clone(),
            scope: scope.into(),
            architecture: architecture.into(),
            location: values.0.clone(),
            uninstall: values.1.clone(),
            icon: values.2.clone(),
        };
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
            registration,
            cleanup: if binary == "verified" {
                "requiresCommittedSuiteAndReview"
            } else {
                "requiresVerifiedInstaller"
            },
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

/// Private native identity. No renderer receives registry keys, paths or commands.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Registration {
    name: String,
    pub app: String,
    pub version: Option<String>,
    scope: String,
    architecture: String,
    pub location: Option<String>,
    uninstall: Option<String>,
    icon: Option<String>,
}
impl Registration {
    pub fn machine(&self) -> bool {
        self.scope == "machine"
    }
    pub fn revision(&self) -> Result<String> {
        let values = (&self.location, &self.uninstall, &self.icon);
        let bytes = serde_json::to_vec(&(
            &self.scope,
            &self.architecture,
            &self.name,
            &self.app,
            &self.version,
            values,
        ))
        .map_err(|_| "legacy_registry_invalid")?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    fn target(&self) -> Result<(HKEY, REG_SAM_FLAGS, String)> {
        if self.name.is_empty() || self.name.len() > 255 || self.name.contains(['\\', '/', '\0']) {
            return Err("legacy_registry_invalid");
        }
        let hive = match self.scope.as_str() {
            "currentUser" => HKEY_CURRENT_USER,
            "machine" => HKEY_LOCAL_MACHINE,
            _ => return Err("legacy_registry_invalid"),
        };
        let view = match self.architecture.as_str() {
            "x64" => KEY_WOW64_64KEY,
            "x86" => KEY_WOW64_32KEY,
            _ => return Err("legacy_registry_invalid"),
        };
        Ok((hive, view, format!("{ROOT}\\{}", self.name)))
    }
    pub fn present(&self) -> Result<bool> {
        let (hive, view, path) = self.target()?;
        let Some(key) = open(hive, &path, view)? else {
            return Ok(false);
        };
        let catalog = devbox_catalog::parse_catalog(include_str!("../../../../catalog.json"))
            .map_err(|_| "legacy_catalog_invalid")?;
        let app = catalog
            .apps
            .iter()
            .find(|app| app.id == self.app && !app.identifier.starts_with("com.devbox.v08."))
            .ok_or("legacy_registry_invalid")?;
        let display = text(&key, "DisplayName")?.ok_or("legacy_registry_changed")?;
        if !(display.eq_ignore_ascii_case(&app.product_name)
            || display.eq_ignore_ascii_case(&app.id))
            || text(&key, "DisplayVersion")? != self.version
            || text(&key, "InstallLocation")? != self.location
            || text(&key, "UninstallString")? != self.uninstall
            || text(&key, "DisplayIcon")? != self.icon
        {
            return Err("legacy_registry_changed");
        }
        Ok(true)
    }
    pub fn remove(&self) -> Result<()> {
        if !self.present()? {
            return Ok(());
        }
        let (hive, view, path) = self.target()?;
        let path = wide(&path);
        let status = unsafe { RegDeleteKeyExW(hive, PCWSTR(path.as_ptr()), view.0, None) };
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            return Err("legacy_registry_cleanup_pending");
        }
        Ok(())
    }
}
pub(crate) fn resolve(id: &str) -> Result<Registration> {
    if !product_contract::commands::revision(id) {
        return Err("legacy_registry_invalid");
    }
    let report = inventory()?;
    let mut entries = report
        .entries
        .into_iter()
        .filter(|entry| entry.registration_id == id);
    let entry = entries.next().ok_or("legacy_registry_changed")?;
    if entries.next().is_some() || entry.binary != "verified" {
        return Err("legacy_installer_verification_required");
    }
    if entry.registration.revision()? != id {
        return Err("legacy_registry_changed");
    }
    Ok(entry.registration)
}
