//! Current-user Run registry adapter; callers supply verified installation ownership.
use crate::agent_autostart::RunKey;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::*,
    },
};
type Result<T> = std::result::Result<T, &'static str>;
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
impl Key {
    pub fn open() -> Result<Self> {
        let path = wide(RUN);
        let mut handle = HKEY::default();
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
                None,
            )
        } != ERROR_SUCCESS
        {
            return Err("agent_autostart_unavailable");
        }
        Ok(Self(handle))
    }
}
impl RunKey for Key {
    fn read(&self, name: &str) -> Result<Option<String>> {
        reg_text(self, name)
    }
    fn write(&mut self, name: &str, value: Option<&str>) -> Result<()> {
        if let Some(value) = value {
            return set_text(self, name, value);
        }
        let name = wide(name);
        let result = unsafe { RegDeleteValueW(self.0, PCWSTR(name.as_ptr())) };
        if result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err("agent_autostart_unavailable")
        }
    }
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
        return Err("agent_autostart_unavailable");
    }
    let length = bytes as usize / 2;
    if buffer[length - 1] != 0 || buffer[..length - 1].contains(&0) {
        return Err("agent_autostart_unavailable");
    }
    String::from_utf16(&buffer[..length - 1])
        .map(Some)
        .map_err(|_| "agent_autostart_unavailable")
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
        return Err("agent_autostart_unavailable");
    }
    Ok(())
}

/// Migrate only the old Workspace shortcut that belongs to this installation.
/// The predecessor is removed only after the replacement Run entry is durable.
pub fn retire_workspace(
    owner: &crate::agent_autostart::Owner,
    preserve_enabled: bool,
) -> Result<()> {
    use crate::agent_autostart as policy;
    use devbox_filesystem::{ensure_no_links, filesystem_identity};
    use windows::{
        core::Interface,
        Win32::Foundation::RPC_E_CHANGED_MODE,
        Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
        },
        Win32::UI::Shell::{
            FOLDERID_Startup, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT,
            SLGP_RAWPATH,
        },
    };
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if result.is_err() && result != RPC_E_CHANGED_MODE {
        return Err("agent_autostart_unavailable");
    }
    let _apartment = Apartment(result.is_ok());
    let folder = unsafe { SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DEFAULT, None) }
        .map_err(|_| "agent_autostart_unavailable")?;
    let folder_text = unsafe { folder.to_string() };
    unsafe {
        CoTaskMemFree(Some(folder.0.cast()));
    }
    let path = std::path::PathBuf::from(folder_text.map_err(|_| "agent_autostart_unavailable")?)
        .join("Devbox Workspace.lnk");
    if matches!(std::fs::symlink_metadata(&path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    ensure_no_links(&path).map_err(|_| "agent_autostart_unavailable")?;
    let _pins = super::component_scope::pin_directories(
        path.parent().ok_or("agent_autostart_unavailable")?,
    )?;
    let identity = filesystem_identity(&path, false).map_err(|_| "agent_autostart_unavailable")?;
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|_| "agent_autostart_unavailable")?;
    let persist: IPersistFile = link.cast().map_err(|_| "agent_autostart_unavailable")?;
    let wide_path = wide(path.to_str().ok_or("agent_autostart_unavailable")?);
    unsafe { persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ) }
        .map_err(|_| "agent_autostart_unavailable")?;
    let mut target = [0_u16; 32768];
    let mut args = [0_u16; 1024];
    let mut description = [0_u16; 1024];
    unsafe {
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .map_err(|_| "agent_autostart_unavailable")?;
        link.GetArguments(&mut args)
            .map_err(|_| "agent_autostart_unavailable")?;
        link.GetDescription(&mut description)
            .map_err(|_| "agent_autostart_unavailable")?;
    }
    let text = |value: &[u16]| -> Result<String> {
        let end = value
            .iter()
            .position(|c| *c == 0)
            .ok_or("agent_autostart_unavailable")?;
        String::from_utf16(&value[..end]).map_err(|_| "agent_autostart_unavailable")
    };
    if !owner.owns_workspace_shortcut(&text(&target)?, &text(&args)?, &text(&description)?) {
        return Ok(());
    }
    if preserve_enabled {
        let mut key = Key::open()?;
        let change = policy::setting(owner, policy::snapshot(&key, owner)?, true)?;
        policy::apply(&mut key, owner, &change)?;
    }
    ensure_no_links(&path).map_err(|_| "agent_autostart_unavailable")?;
    if filesystem_identity(&path, false).map_err(|_| "agent_autostart_unavailable")? != identity {
        return Err("agent_autostart_conflict");
    }
    std::fs::remove_file(&path).map_err(|_| "agent_autostart_unavailable")
}
