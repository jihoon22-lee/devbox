use crate::{R, ToolsError};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use zeroize::Zeroizing;
pub(super) const MAX_FILE: usize = 4 * 1024 * 1024;
pub(super) const MAX_TOTAL: usize = 32 * 1024 * 1024;
pub(super) fn no_symlinks(path: &Path) -> R<()> {
    if !path.is_absolute() {
        return Err(ToolsError::SupportFailed.into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(ToolsError::SupportFailed.into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(super::failed(error)),
        }
    }
    Ok(())
}
pub(super) fn read(path: &Path, limit: usize) -> R<Zeroizing<String>> {
    no_symlinks(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(super::failed)?;
    let meta = file.metadata().map_err(super::failed)?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(ToolsError::SupportTooLarge.into());
    }
    let mut text = Zeroizing::new(String::new());
    file.take(limit as u64 + 1)
        .read_to_string(&mut text)
        .map_err(super::failed)?;
    if text.len() > limit {
        return Err(ToolsError::SupportTooLarge.into());
    }
    Ok(text)
}
pub(super) fn directory(data: &Path) -> R<PathBuf> {
    let path = data.join("support");
    no_symlinks(&path)?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(super::failed(error)),
    }
    no_symlinks(&path)?;
    let meta = fs::metadata(&path).map_err(super::failed)?;
    if !meta.is_dir() {
        return Err(ToolsError::SupportFailed.into());
    }
    #[cfg(unix)]
    {
        use devbox_core::Env;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.uid() != devbox_core::OsEnv.uid() || meta.permissions().mode() & 0o077 != 0 {
            return Err(ToolsError::SupportFailed.into());
        }
    }
    Ok(path)
}
pub(super) fn candidates(log_dir: &Path) -> R<(Vec<(PathBuf, bool)>, usize)> {
    no_symlinks(log_dir)?;
    let mut candidates = Vec::new();
    let mut omitted = 0;
    let oldest = SystemTime::now()
        .checked_sub(Duration::from_secs(7 * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for (index, item) in fs::read_dir(log_dir).map_err(super::failed)?.enumerate() {
        if index >= 256 {
            omitted += 1;
            break;
        }
        let item = item.map_err(super::failed)?;
        let name = item.file_name();
        let name = name.to_string_lossy();
        let crash = name.starts_with("crash-") && name.ends_with(".log");
        let daemon = name.starts_with("daemon") && name.ends_with(".log");
        if !crash && !daemon {
            continue;
        }
        let meta = fs::symlink_metadata(item.path()).map_err(super::failed)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_FILE as u64 {
            omitted += 1;
            continue;
        }
        let modified = meta.modified().map_err(super::failed)?;
        if daemon && modified < oldest {
            continue;
        }
        candidates.push((modified, item.path(), crash));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    if candidates.len() > 64 {
        omitted += candidates.len() - 64;
        candidates.truncate(64);
    }
    Ok((
        candidates
            .into_iter()
            .map(|(_, path, crash)| (path, crash))
            .collect(),
        omitted,
    ))
}
pub(super) fn private(file: &File) -> R<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(super::failed)?;
    }
    Ok(())
}
