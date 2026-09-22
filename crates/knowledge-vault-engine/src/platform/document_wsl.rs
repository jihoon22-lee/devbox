//! Knowledge's filesystem-only use of the pinned static Linux helper.
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};
use workspace_wsl::document::{Method, Operation};
struct Config {
    directory: PathBuf,
    digest: &'static str,
    bytes: u64,
}
static CONFIG: OnceLock<Config> = OnceLock::new();
pub fn configure(directory: PathBuf, digest: &'static str, bytes: u64) {
    let _ = CONFIG.set(Config {
        directory,
        digest,
        bytes,
    });
}

pub fn invoke(method: Method, paths: &[&Path]) -> io::Result<()> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
    let config = CONFIG.get().ok_or(io::ErrorKind::Unsupported)?;
    if config.bytes < 64 || config.bytes > 64 * 1024 * 1024 {
        return Err(invalid());
    }
    let parsed = paths
        .iter()
        .map(|path| {
            devbox_wsl::path::parse_wsl_unc_path(&path.to_string_lossy())
                .map_err(|_| invalid())?
                .ok_or_else(invalid)
        })
        .collect::<io::Result<Vec<_>>>()?;
    let distro = parsed.first().ok_or_else(invalid)?.distro();
    if parsed
        .iter()
        .any(|path| !path.distro().eq_ignore_ascii_case(distro))
    {
        return Err(invalid());
    }
    let (registration, _registry) = registration(distro)?;
    let operation = Operation {
        version: 1,
        method,
        paths: parsed.iter().map(|p| p.linux_path().into()).collect(),
    };
    let bytes = serde_json::to_vec(&operation).map_err(io::Error::other)?;
    if bytes.len() > 16_384 {
        return Err(invalid());
    }
    let helper = config.directory.join("devbox-workspace-wsl");
    devbox_filesystem::ensure_no_links(&helper)?;
    let mut pins = Vec::new();
    for parent in config
        .directory
        .ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let file = OpenOptions::new()
            .access_mode(0x80)
            .share_mode(3)
            .custom_flags(0x02000000 | 0x00200000)
            .open(parent)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
        pins.push(file);
    }
    let mut binary = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x00200000)
        .open(&helper)?;
    let metadata = binary.metadata()?;
    if !metadata.is_file()
        || metadata.len() != config.bytes
        || metadata.file_attributes() & 0x400 != 0
    {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = binary.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let digest = hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if digest != config.digest {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let mut system = vec![0u16; 32768];
    let count = unsafe {
        windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut system))
    } as usize;
    if count == 0 || count >= system.len() {
        return Err(invalid());
    }
    let system = PathBuf::from(String::from_utf16(&system[..count]).map_err(|_| invalid())?);
    let executable = system.join("wsl.exe");
    devbox_filesystem::ensure_no_links(&executable)?;
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut child = Process(
        Command::new(executable)
            .args(["--distribution-id", &registration, "--cd"])
            .arg(&config.directory)
            .args(["--exec", "./devbox-workspace-wsl", "--document-operation"])
            .current_dir(&system)
            .env_clear()
            .env("SystemRoot", system.parent().ok_or_else(invalid)?)
            .env("PATH", &system)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()?,
    );
    // Sending must not block the host deadline while WSL starts or reads stdin.
    let mut input = match child.0.stdin.take() {
        Some(input) => input,
        None => {
            let _ = child.0.kill();
            return Err(invalid());
        }
    };
    std::thread::Builder::new()
        .name("knowledge-document-input".into())
        .spawn(move || {
            let _ = input.write_all(&bytes);
        })?;
    loop {
        if let Some(status) = child.0.try_wait()? {
            return match status.code() {
                Some(0) => Ok(()),
                Some(21) => Err(io::ErrorKind::AlreadyExists.into()),
                _ => Err(io::Error::other(
                    "document helper did not confirm completion",
                )),
            };
        }
        if Instant::now() >= deadline {
            let _ = child.0.kill();
            // Do not wait without a bound. Linux's watchdog also retires the
            // operation; the caller retains all evidence on this unknown state.
            let _ = child.0.try_wait();
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.try_wait();
        }
    }
}

struct Key(windows::Win32::System::Registry::HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Registry::RegCloseKey(self.0);
        }
    }
}
fn registration(distro: &str) -> io::Result<(String, Key)> {
    use windows::{
        core::{w, PCWSTR, PWSTR},
        Win32::{
            Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
            System::Registry::*,
        },
    };
    let error = || io::Error::other("WSL registration unavailable");
    let mut root = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Lxss"),
            Some(0),
            KEY_READ,
            &mut root,
        )
    }
    .ok()
    .map_err(|_| error())?;
    let root = Key(root);
    for index in 0..128 {
        let mut name = [0u16; 128];
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
            break;
        }
        if status != ERROR_SUCCESS {
            return Err(error());
        }
        let id = String::from_utf16(&name[..length as usize]).map_err(|_| error())?;
        if id.len() != 38
            || !id.starts_with('{')
            || !id.ends_with('}')
            || !id[1..37]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b'-')
        {
            continue;
        }
        name[length as usize] = 0;
        let mut key = HKEY::default();
        unsafe { RegOpenKeyExW(root.0, PCWSTR(name.as_ptr()), Some(0), KEY_READ, &mut key) }
            .ok()
            .map_err(|_| error())?;
        let key = Key(key);
        let mut value = [0u16; 256];
        let mut bytes = (value.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                key.0,
                PCWSTR::null(),
                w!("DistributionName"),
                RRF_RT_REG_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if result != ERROR_SUCCESS
            || bytes < 2
            || bytes as usize > value.len() * 2
            || !bytes.is_multiple_of(2)
        {
            continue;
        }
        let length = bytes as usize / 2;
        if value[length - 1] != 0 || value[..length - 1].contains(&0) {
            continue;
        }
        if String::from_utf16(&value[..length - 1])
            .map_err(|_| error())?
            .eq_ignore_ascii_case(distro)
        {
            return Ok((id, key));
        }
    }
    Err(error())
}
