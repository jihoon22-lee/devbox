//! One current-user Suite entry and four owned Start Menu links.
use super::*;
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            Registry::*,
        },
        UI::Shell::{
            FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
        },
    },
};
const PRODUCTS: [(&str, &str); 4] = [
    ("workspace", "Workspace"),
    ("api-studio", "API Studio"),
    ("knowledge", "Knowledge"),
    ("control-center", "Control Center"),
];
const ARP: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration {
    schema_version: u32,
    installation_key: String,
    root_identity: (u64, u64),
    payload_revision: String,
    shortcut_directory: PathBuf,
    shortcut_identity: (u64, u64),
    #[serde(default)]
    shortcut_plan: Option<crate::core::suite_removal::Plan>,
    #[serde(default)]
    uninstaller: Option<crate::core::suite_removal::Plan>,
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
struct Key(HKEY);
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
fn save(root: &Path, value: &Registration) -> Result<()> {
    devbox_filesystem::atomic_write(
        root.join("suite-registration.json"),
        &serde_json::to_vec(value).map_err(|_| "suite_registration_invalid")?,
    )
    .map_err(|_| "suite_registration_unavailable")
}
fn read_registration(root: &Path, key: &str) -> Result<Option<Registration>> {
    let path = root.join("suite-registration.json");
    if matches!(fs::symlink_metadata(&path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let value: Registration = serde_json::from_slice(&read(&path, 2 * 1024 * 1024)?)
        .map_err(|_| "suite_registration_invalid")?;
    if value.schema_version != 1
        || value.installation_key != key
        || value.root_identity
            != filesystem_identity(root, true)
                .map_err(|_| "bootstrap_root_changed")?
                .components()
        || value
            .shortcut_directory
            .file_name()
            .and_then(|name| name.to_str())
            != Some(format!("Devbox Suite ({})", &key[..12]).as_str())
    {
        return Err("suite_registration_changed");
    }
    if matches!(fs::symlink_metadata(&value.shortcut_directory),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        return Ok(Some(value));
    }
    ensure_no_links(&value.shortcut_directory).map_err(|_| "suite_shortcut_directory_changed")?;
    if filesystem_identity(&value.shortcut_directory, true)
        .map_err(|_| "suite_shortcut_directory_changed")?
        .components()
        != value.shortcut_identity
    {
        return Err("suite_shortcut_directory_changed");
    }
    Ok(Some(value))
}
fn reg_text(key: &Key, name: &str) -> Result<Option<String>> {
    let name = wide(name);
    let mut buffer = [0u16; 4096];
    let mut bytes = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            key.0,
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS
        || bytes < 2
        || bytes as usize > buffer.len() * 2
        || !bytes.is_multiple_of(2)
    {
        return Err("suite_registry_unavailable");
    }
    let length = bytes as usize / 2;
    if buffer[length - 1] != 0 || buffer[..length - 1].contains(&0) {
        return Err("suite_registry_unavailable");
    }
    String::from_utf16(&buffer[..length - 1])
        .map(Some)
        .map_err(|_| "suite_registry_unavailable")
}
fn set_text(key: &Key, name: &str, value: &str) -> Result<()> {
    let name = wide(name);
    let bytes = wide(value)
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect::<Vec<_>>();
    if unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) }
        != ERROR_SUCCESS
    {
        return Err("suite_registry_unavailable");
    }
    Ok(())
}
fn registry(root: &Path, key: &str, version: &str) -> Result<()> {
    let path = wide(&format!(r"{ARP}\DevboxSuite.{key}"));
    let mut handle = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    if unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
            None,
            &mut handle,
            Some(&mut disposition),
        )
    } != ERROR_SUCCESS
    {
        return Err("suite_registry_unavailable");
    }
    let handle = Key(handle);
    if disposition != REG_CREATED_NEW_KEY {
        let owner = reg_text(&handle, "DevboxInstallationKey")?;
        let location = reg_text(&handle, "InstallLocation")?;
        let ours = owner.as_deref() == Some(key)
            && location.as_deref() == Some(root.to_string_lossy().as_ref());
        // An interrupted creation may contain only our owner value. Never
        // take over an entry that declares another application or location.
        let pending = (owner.is_none() || owner.as_deref() == Some(key))
            && location.is_none()
            && reg_text(&handle, "DisplayName")?.is_none()
            && reg_text(&handle, "UninstallString")?.is_none();
        if !ours && !pending {
            return Err("suite_registry_foreign");
        }
    }
    set_text(&handle, "DevboxInstallationKey", key)?;
    set_text(
        &handle,
        "InstallLocation",
        root.to_str().ok_or("bootstrap_root_unsafe")?,
    )?;
    set_text(&handle, "DisplayName", "Devbox Suite")?;
    set_text(&handle, "DisplayVersion", version)?;
    set_text(&handle, "Publisher", "Devbox")?;
    let uninstaller = root.join("Uninstall.exe");
    set_text(
        &handle,
        "UninstallString",
        &format!("\"{}\"", uninstaller.to_string_lossy()),
    )?;
    set_text(
        &handle,
        "QuietUninstallString",
        &format!("\"{}\" /S", uninstaller.to_string_lossy()),
    )?;
    for name in ["NoModify", "NoRepair"] {
        let name = wide(name);
        if unsafe {
            RegSetValueExW(
                handle.0,
                PCWSTR(name.as_ptr()),
                None,
                REG_DWORD,
                Some(&1u32.to_le_bytes()),
            )
        } != ERROR_SUCCESS
        {
            return Err("suite_registry_unavailable");
        }
    }
    Ok(())
}
pub(super) fn register(root: &Path, payload_path: &Path, image: &Path) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate(&root, false)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    let root_identity = filesystem_identity(&root, true)
        .map_err(|_| "bootstrap_root_changed")?
        .components();
    if owner.schema_version != 1
        || owner.root_identity != root_identity
        || owner.payload_revision != revision
    {
        return Err("bootstrap_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(root_identity, &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let mut registration = match read_registration(&root, &key)? {
        Some(value) if value.payload_revision == revision => value,
        Some(_) => return Err("suite_registration_changed"),
        None => {
            let programs =
                unsafe { SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None) }
                    .map_err(|_| "suite_shortcut_directory_unavailable")?;
            let path = unsafe { programs.to_string() };
            unsafe {
                CoTaskMemFree(Some(programs.0.cast()));
            }
            let parent = PathBuf::from(path.map_err(|_| "suite_shortcut_directory_unavailable")?);
            ensure_no_links(&parent).map_err(|_| "suite_shortcut_directory_unsafe")?;
            let directory = parent.join(format!("Devbox Suite ({})", &key[..12]));
            fs::create_dir(&directory).map_err(|_| "suite_shortcut_directory_conflict")?;
            let value = Registration {
                schema_version: 1,
                installation_key: key.clone(),
                root_identity,
                payload_revision: revision.clone(),
                shortcut_identity: filesystem_identity(&directory, true)
                    .map_err(|_| "suite_shortcut_directory_unavailable")?
                    .components(),
                shortcut_directory: directory,
                shortcut_plan: None,
                uninstaller: None,
            };
            save(&root, &value)?;
            value
        }
    };
    if matches!(fs::symlink_metadata(&registration.shortcut_directory),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        ensure_no_links(
            registration
                .shortcut_directory
                .parent()
                .ok_or("suite_shortcut_directory_unsafe")?,
        )
        .map_err(|_| "suite_shortcut_directory_unsafe")?;
        fs::create_dir(&registration.shortcut_directory)
            .map_err(|_| "suite_shortcut_directory_conflict")?;
        registration.shortcut_identity =
            filesystem_identity(&registration.shortcut_directory, true)
                .map_err(|_| "suite_shortcut_directory_unavailable")?
                .components();
        registration.shortcut_plan = None;
        save(&root, &registration)?;
    }
    let _shortcut_pins =
        crate::suite::platform::component_scope::pin_directories(&registration.shortcut_directory)?;
    let cached = root.join("setup").join(&revision);
    let helper = cached.join("devbox-suite-bootstrap.exe");
    verify_payload_owner(&payload, &helper)?;
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|_| "suite_shortcut_com_unavailable")?;
    let _com = Com;
    let mut names = Vec::new();
    for (id, label) in PRODUCTS {
        let name = format!("{label}.lnk");
        let destination = registration.shortcut_directory.join(&name);
        if !destination.exists() {
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|_| "suite_shortcut_unavailable")?;
            let target = wide(helper.to_str().ok_or("bootstrap_root_unsafe")?);
            let args = wide(&format!(
                "--open-{id} \"{}\" \"{}\"",
                root.to_string_lossy(),
                cached.join("suite-payload.json").to_string_lossy()
            ));
            let description = wide(&format!("Devbox {label}"));
            unsafe {
                link.SetPath(PCWSTR(target.as_ptr()))
                    .and_then(|()| link.SetArguments(PCWSTR(args.as_ptr())))
                    .and_then(|()| link.SetDescription(PCWSTR(description.as_ptr())))
            }
            .map_err(|_| "suite_shortcut_unavailable")?;
            let persist: IPersistFile = link.cast().map_err(|_| "suite_shortcut_unavailable")?;
            let temporary = registration
                .shortcut_directory
                .join(format!(".{}.lnk", uuid::Uuid::new_v4()));
            let path = wide(
                temporary
                    .to_str()
                    .ok_or("suite_shortcut_directory_unsafe")?,
            );
            unsafe { persist.Save(PCWSTR(path.as_ptr()), true) }
                .map_err(|_| "suite_shortcut_unavailable")?;
            fs::hard_link(&temporary, &destination).map_err(|_| "suite_shortcut_conflict")?;
            fs::remove_file(&temporary).map_err(|_| "suite_shortcut_unavailable")?;
        } else {
            // A prior interrupted publication is inspected below before it can
            // be accepted. It must resolve to this exact helper and closed args.
            verify_link(
                &destination,
                &helper,
                &format!(
                    "--open-{id} \"{}\" \"{}\"",
                    root.to_string_lossy(),
                    cached.join("suite-payload.json").to_string_lossy()
                ),
            )?;
        }
        names.push(name);
    }
    registration.shortcut_plan = Some(crate::core::suite_removal::Plan::capture(
        &registration.shortcut_directory,
        &key,
        &names,
    )?);
    {
        use std::io::{Seek, SeekFrom};
        let (mut file, _) = open_filesystem_object(root.join("Uninstall.exe"), false)
            .map_err(|_| "suite_uninstaller_missing")?;
        let size = file
            .metadata()
            .map_err(|_| "suite_uninstaller_invalid")?
            .len();
        let mut dos = [0u8; 64];
        file.read_exact(&mut dos)
            .map_err(|_| "suite_uninstaller_invalid")?;
        let offset = u32::from_le_bytes(
            dos[60..64]
                .try_into()
                .map_err(|_| "suite_uninstaller_invalid")?,
        ) as u64;
        if &dos[..2] != b"MZ"
            || !(1024..=512 * 1024 * 1024).contains(&size)
            || offset > size.saturating_sub(4)
            || offset > 1024 * 1024
        {
            return Err("suite_uninstaller_invalid");
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| "suite_uninstaller_invalid")?;
        let mut pe = [0u8; 4];
        file.read_exact(&mut pe)
            .map_err(|_| "suite_uninstaller_invalid")?;
        if &pe != b"PE\0\0" {
            return Err("suite_uninstaller_invalid");
        }
    }
    if let Some(plan) = &registration.uninstaller {
        plan.verify_remaining(&root)?;
    }
    registration.uninstaller = Some(crate::core::suite_removal::Plan::capture(
        &root,
        &key,
        &["Uninstall.exe".into()],
    )?);
    save(&root, &registration)?;
    registry(&root, &key, &payload.suite_version)?;
    Ok(StageResult {
        state: "suiteRegisteredActivationPending",
        operation_id: None,
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}
fn verify_link(path: &Path, expected: &Path, arguments: &str) -> Result<()> {
    use windows::Win32::{System::Com::STGM_READ, UI::Shell::SLGP_RAWPATH};
    ensure_no_links(path).map_err(|_| "suite_shortcut_unsafe")?;
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|_| "suite_shortcut_unavailable")?;
    let persist: IPersistFile = link.cast().map_err(|_| "suite_shortcut_unavailable")?;
    let path = wide(path.to_str().ok_or("suite_shortcut_unsafe")?);
    unsafe { persist.Load(PCWSTR(path.as_ptr()), STGM_READ) }
        .map_err(|_| "suite_shortcut_unavailable")?;
    let mut target = [0u16; 4096];
    let mut args = [0u16; 4096];
    unsafe {
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .and_then(|()| link.GetArguments(&mut args))
    }
    .map_err(|_| "suite_shortcut_unavailable")?;
    let text = |value: &[u16]| -> Result<String> {
        let length = value
            .iter()
            .position(|c| *c == 0)
            .ok_or("suite_shortcut_invalid")?;
        String::from_utf16(&value[..length]).map_err(|_| "suite_shortcut_invalid")
    };
    if !text(&target)?.eq_ignore_ascii_case(expected.to_str().ok_or("suite_shortcut_invalid")?)
        || text(&args)? != arguments
    {
        return Err("suite_shortcut_foreign");
    }
    Ok(())
}
pub(super) fn remove(root: &Path, key: &str, apply: bool) -> Result<()> {
    let Some(registration) = read_registration(root, key)? else {
        return Ok(());
    };
    let links = registration
        .shortcut_plan
        .as_ref()
        .ok_or("suite_registration_incomplete")?;
    let shortcuts_present = !matches!(fs::symlink_metadata(&registration.shortcut_directory),Err(error) if error.kind()==std::io::ErrorKind::NotFound);
    if shortcuts_present {
        links.verify_remaining(&registration.shortcut_directory)?;
    }
    let uninstaller = registration
        .uninstaller
        .as_ref()
        .ok_or("suite_registration_incomplete")?;
    uninstaller.verify_remaining(root)?;
    let path = wide(&format!(r"{ARP}\DevboxSuite.{key}"));
    let mut handle = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &mut handle,
        )
    };
    if status == ERROR_SUCCESS {
        let handle = Key(handle);
        if reg_text(&handle, "DevboxInstallationKey")?.as_deref() != Some(key)
            || reg_text(&handle, "InstallLocation")?.as_deref()
                != Some(root.to_string_lossy().as_ref())
        {
            return Err("suite_registry_foreign");
        }
    } else if status != ERROR_FILE_NOT_FOUND {
        return Err("suite_registry_unavailable");
    }
    if apply {
        if shortcuts_present {
            links.remove(&registration.shortcut_directory)?;
        }
        // Keep the owned shortcut directory as an empty resume anchor until the
        // NSIS owner has completed the package and registration transaction.
        if status == ERROR_SUCCESS
            && unsafe {
                RegDeleteKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(path.as_ptr()),
                    KEY_WOW64_64KEY.0,
                    None,
                )
            } != ERROR_SUCCESS
        {
            return Err("suite_registry_unavailable");
        }
        // Keep an executable recovery entrypoint until ARP cleanup succeeds.
        // A registry failure must not leave its entry pointing at a deleted file.
        uninstaller.remove(root)?;
    }
    Ok(())
}
