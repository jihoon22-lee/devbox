//! Remove only an owned canonical app-channel installation, never its data/config.
mod owned;
use devbox_core::Paths;
use devbox_protocol::channel::AppChannel;
use owned::{Directory, OwnedFile};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    future::Future,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
#[derive(Debug, thiserror::Error)]
pub enum UninstallError {
    #[error("정리는 prod·accept·dev 앱 채널에서만 가능합니다")]
    UnsupportedInstance,
    #[error("소유한 설치 파일만 정리할 수 있습니다")]
    ForeignFile,
    #[error("이 채널의 설치 파일을 다른 작업이 사용 중입니다")]
    Busy,
    #[error("이 채널의 서비스 정리를 완료하지 못했습니다")]
    UnitControl,
    #[error("설치 파일 정리를 완료하지 못했습니다")]
    Io(#[from] std::io::Error),
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    pub removed_files: usize,
}
pub trait UnitController: Sync {
    fn disable(&self, units: &[String]) -> impl Future<Output = Result<(), UninstallError>> + Send;
    fn reload(&self) -> impl Future<Output = Result<(), UninstallError>> + Send;
}
struct Systemd;
impl UnitController for Systemd {
    async fn disable(&self, units: &[String]) -> Result<(), UninstallError> {
        let mut args = vec!["--user", "disable", "--now"];
        args.extend(units.iter().map(String::as_str));
        super::command("systemctl", &args)
            .await
            .map_err(|_| UninstallError::UnitControl)?;
        Ok(())
    }
    async fn reload(&self) -> Result<(), UninstallError> {
        super::command("systemctl", &["--user", "daemon-reload"])
            .await
            .map_err(|_| UninstallError::UnitControl)?;
        Ok(())
    }
}
fn no_links(path: &Path) -> Result<(), UninstallError> {
    if !path.is_absolute() {
        return Err(UninstallError::ForeignFile);
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(UninstallError::ForeignFile),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn regular(path: &Path, limit: u64) -> Result<(File, fs::Metadata), UninstallError> {
    no_links(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    use devbox_core::Env;
    if !meta.is_file() || meta.len() > limit || meta.uid() != devbox_core::OsEnv.uid() {
        return Err(UninstallError::ForeignFile);
    }
    Ok((file, meta))
}
fn unit_file(path: PathBuf) -> Result<Option<OwnedFile>, UninstallError> {
    no_links(&path)?;
    if !path.exists() {
        return Ok(None);
    }
    let (file, meta) = regular(&path, 64 * 1024)?;
    let mut header = String::new();
    file.take(super::MARKER.len() as u64)
        .read_to_string(&mut header)?;
    if header != super::MARKER {
        return Err(UninstallError::ForeignFile);
    }
    Ok(Some(OwnedFile::new(path, &meta)?))
}
fn binaries(bin: &Path) -> Result<(Option<File>, Vec<OwnedFile>), UninstallError> {
    no_links(bin)?;
    let meta = match fs::metadata(bin) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((None, vec![])),
        Err(error) => return Err(error.into()),
    };
    use devbox_core::Env;
    if !meta.is_dir()
        || meta.uid() != devbox_core::OsEnv.uid()
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err(UninstallError::ForeignFile);
    }
    let lock = File::open(bin)?;
    lock.try_lock().map_err(|_| UninstallError::Busy)?;
    let owner = bin.join(".owner");
    let (mut file, _) = regular(&owner, 128)?;
    let mut marker = String::new();
    file.read_to_string(&mut marker)?;
    if marker.trim_end() != "devbox-desktop-bin-v1" {
        return Err(UninstallError::ForeignFile);
    }
    let mut files = Vec::new();
    for (index, item) in fs::read_dir(bin)?.enumerate() {
        if index >= 256 {
            return Err(UninstallError::ForeignFile);
        }
        let item = item?;
        let name = item.file_name();
        let name = name.to_str().ok_or(UninstallError::ForeignFile)?;
        let meta = fs::symlink_metadata(item.path())?;
        if name == "devbox" {
            if !meta.file_type().is_symlink() {
                return Err(UninstallError::ForeignFile);
            }
            let target = fs::read_link(item.path())?;
            let target = target.to_str().ok_or(UninstallError::ForeignFile)?;
            if !binary_name(target) || !bin.join(target).is_file() {
                return Err(UninstallError::ForeignFile);
            }
        } else if name != ".owner" {
            if !binary_name(name) {
                return Err(UninstallError::ForeignFile);
            }
            let (mut file, _) = regular(&item.path(), 512 * 1024 * 1024)?;
            let mut digest = Sha256::new();
            let mut bytes = [0u8; 64 * 1024];
            loop {
                let count = file.read(&mut bytes)?;
                if count == 0 {
                    break;
                }
                digest.update(&bytes[..count]);
            }
            if format!("devbox-{:x}", digest.finalize()) != name {
                return Err(UninstallError::ForeignFile);
            }
        }
        files.push(OwnedFile::new(item.path(), &meta)?);
    }
    Ok((Some(lock), files))
}
fn binary_name(name: &str) -> bool {
    name.strip_prefix("devbox-").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
pub async fn uninstall_with(
    paths: &Paths,
    controller: &impl UnitController,
) -> Result<UninstallReport, UninstallError> {
    let channel = AppChannel::parse(paths.instance.as_str())
        .ok_or(UninstallError::UnsupportedInstance)?
        .config();
    let root = paths.xdg_config_home.join("systemd/user");
    no_links(&root)?;
    let units = vec![
        format!("{}@{}.socket", channel.daemon_unit, paths.instance.as_str()),
        format!(
            "{}@{}.service",
            channel.daemon_unit,
            paths.instance.as_str()
        ),
        format!("{}@{}.service", channel.tmux_unit, paths.instance.as_str()),
    ];
    let mut files = Vec::new();
    let mut directories = Vec::new();
    for name in [
        format!("{}@.socket", channel.daemon_unit),
        format!("{}@.service", channel.daemon_unit),
        format!("{}@.service", channel.tmux_unit),
    ] {
        if let Some(file) = unit_file(root.join(name))? {
            files.push(file);
        }
    }
    for unit in &units {
        let directory = root.join(format!("{unit}.d"));
        if let Some(file) = unit_file(directory.join("paths.conf"))? {
            files.push(file);
            directories.push(Directory::open(&directory)?);
        }
    }
    // Validate every candidate and acquire the same directory lock used by the
    // installer before stopping services or unlinking anything.
    let (bin_lock, binary_files) = binaries(&paths.bin_dir)?;
    if files.is_empty() && bin_lock.is_none() {
        return Ok(UninstallReport { removed_files: 0 });
    }
    let bin_directory = bin_lock
        .as_ref()
        .map(|_| Directory::open(&paths.bin_dir))
        .transpose()?;
    controller.disable(&units).await?;
    // Validate the entire plan after the await, before deleting even the first file.
    for file in files.iter().chain(&binary_files) {
        file.validate()?;
    }
    for directory in directories.iter().chain(bin_directory.iter()) {
        directory.validate()?;
    }
    let removed_files = files.len() + binary_files.len();
    for file in files.into_iter().chain(binary_files) {
        file.remove()?;
    }
    for directory in directories {
        match directory.remove() {
            Ok(()) => {}
            Err(UninstallError::Io(error))
                if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => return Err(error.into()),
        }
    }
    if let Some(directory) = bin_directory {
        directory.remove()?;
    }
    controller.reload().await?;
    Ok(UninstallReport { removed_files })
}
pub async fn run(json: bool) -> anyhow::Result<()> {
    let report = uninstall_with(&crate::paths()?, &Systemd).await?;
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        println!("이 채널의 서비스와 실행 파일을 정리했습니다. 데이터와 설정은 보존했습니다.");
    }
    Ok(())
}
