//! File content stays on this distro's native root filesystem. A POSIX spelling
//! alone cannot authorize Windows/drvfs or another distro's mounted filesystem.
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_MOUNTS_BYTES: u64 = 4 * 1024 * 1024;
fn bounded_file(path: &Path, maximum: u64) -> Result<String> {
    let file = File::open(path).map_err(|_| "wsl_filesystem_unavailable")?;
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "wsl_filesystem_unavailable")?;
    if bytes.len() as u64 > maximum {
        return Err("wsl_filesystem_unavailable");
    }
    String::from_utf8(bytes).map_err(|_| "wsl_filesystem_unavailable")
}
fn mount_path(raw: &str) -> Result<PathBuf> {
    let mut output = Vec::new();
    let bytes = raw.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            let code = bytes
                .get(cursor + 1..cursor + 4)
                .ok_or("wsl_filesystem_unavailable")?;
            output.push(match code {
                b"040" => b' ',
                b"011" => b'\t',
                b"012" => b'\n',
                b"134" => b'\\',
                _ => return Err("wsl_filesystem_unavailable"),
            });
            cursor += 4;
        } else {
            output.push(bytes[cursor]);
            cursor += 1;
        }
    }
    let path = PathBuf::from(String::from_utf8(output).map_err(|_| "wsl_filesystem_unavailable")?);
    if !path.is_absolute() {
        return Err("wsl_filesystem_unavailable");
    }
    Ok(path)
}
fn native_mount(mounts: &str, path: &Path, device: u64, id: Option<u64>) -> Result<()> {
    let mut selected: Option<(usize, bool)> = None;
    let major = libc::major(device);
    let minor = libc::minor(device);
    for line in mounts.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let separator = fields
            .iter()
            .position(|field| *field == "-")
            .ok_or("wsl_filesystem_unavailable")?;
        if separator < 6 || fields.len() != separator + 4 {
            return Err("wsl_filesystem_unavailable");
        }
        let mount_id = fields[0]
            .parse::<u64>()
            .map_err(|_| "wsl_filesystem_unavailable")?;
        if id.is_some_and(|expected| expected != mount_id) {
            continue;
        }
        let mount = mount_path(fields[4])?;
        if !path.starts_with(&mount) {
            continue;
        }
        let (mounted_major, mounted_minor) = fields[2]
            .split_once(':')
            .ok_or("wsl_filesystem_unavailable")?;
        if mounted_major.parse::<u32>().ok() != Some(major)
            || mounted_minor.parse::<u32>().ok() != Some(minor)
        {
            continue;
        }
        let allowed = matches!(
            fields[separator + 1],
            "ext4" | "ext3" | "ext2" | "btrfs" | "xfs" | "f2fs" | "lxfs" | "wslfs"
        );
        let depth = mount.components().count();
        match selected {
            Some((previous, _)) if previous > depth => {}
            Some((previous, _)) if previous == depth => return Err("wsl_filesystem_unavailable"),
            _ => selected = Some((depth, allowed)),
        }
    }
    match selected {
        Some((_, true)) => Ok(()),
        _ => Err("wsl_native_filesystem_required"),
    }
}
fn metadata_object(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "wsl_filesystem_unavailable")
}
fn fsid(file: &File) -> Result<(Vec<u8>, bool)> {
    let mut info = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(file.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return Err("wsl_filesystem_unavailable");
    }
    let info = unsafe { info.assume_init() };
    let mut id = unsafe {
        std::slice::from_raw_parts(
            (&info.f_fsid as *const libc::fsid_t).cast::<u8>(),
            std::mem::size_of::<libc::fsid_t>(),
        )
    }
    .to_vec();
    id.extend_from_slice(&info.f_type.to_le_bytes());
    Ok((id, info.f_type == 0x5346_4846))
}
fn mount_id(file: &File, wslfs: bool) -> Result<Option<u64>> {
    let path = format!("/proc/self/fdinfo/{}", file.as_raw_fd());
    let input = match File::open(path) {
        Ok(input) => input,
        // WSL1 exposes mountinfo, but no per-descriptor fdinfo files. The
        // retained descriptor's device/fsid plus unambiguous mountinfo remain
        // mandatory. Other filesystems cannot use this compatibility branch.
        Err(error) if wslfs && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("wsl_filesystem_unavailable"),
    };
    let mut bytes = Vec::new();
    input
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "wsl_filesystem_unavailable")?;
    if bytes.len() > 4096 {
        return Err("wsl_filesystem_unavailable");
    }
    let info = String::from_utf8(bytes).map_err(|_| "wsl_filesystem_unavailable")?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .map(|id| {
            id.trim()
                .parse::<u64>()
                .map_err(|_| "wsl_filesystem_unavailable")
        })
        .transpose()
}
pub fn admit(path: &Path) -> Result<()> {
    super::engine::admit(path)?;
    // Missing leaves (new .git metadata or rename targets) inherit only their
    // nearest existing parent. No content handle is opened by this admission.
    let mut existing = path;
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                existing = existing.parent().ok_or("wsl_filesystem_unavailable")?
            }
            Err(_) => return Err("wsl_filesystem_unavailable"),
        }
    }
    devbox_filesystem::ensure_no_links(existing).map_err(|_| "unsafe_file_path")?;
    let object = metadata_object(existing)?;
    let metadata = object
        .metadata()
        .map_err(|_| "wsl_filesystem_unavailable")?;
    if !(metadata.is_file() || metadata.is_dir()) {
        return Err("wsl_native_filesystem_required");
    }
    let root = metadata_object(Path::new("/"))?;
    let filesystem = fsid(&object)?;
    if metadata.dev()
        != root
            .metadata()
            .map_err(|_| "wsl_filesystem_unavailable")?
            .dev()
        || filesystem != fsid(&root)?
    {
        return Err("wsl_native_filesystem_required");
    }
    let mount_id = mount_id(&object, filesystem.1)?;
    let mounts = bounded_file(Path::new("/proc/self/mountinfo"), MAX_MOUNTS_BYTES)?;
    native_mount(&mounts, existing, metadata.dev(), mount_id)?;
    if devbox_filesystem::filesystem_identity(existing, metadata.is_dir())
        .map_err(|_| "wsl_filesystem_unavailable")?
        .components()
        != (metadata.dev(), metadata.ino())
    {
        return Err("file_changed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mount_type_and_identity_reject_windows_and_ambiguous_overmounts() {
        let device = libc::makedev(8, 1);
        let native = "1 0 8:1 / / rw - ext4 /dev/sda rw\n";
        assert!(native_mount(native, Path::new("/home/project"), device, Some(1)).is_ok());
        for filesystem in ["drvfs", "9p", "cifs", "overlay", "fuse"] {
            let mounts = format!("{native}2 1 8:1 / /home/project rw - {filesystem} source rw\n");
            assert_eq!(
                native_mount(&mounts, Path::new("/home/project/file"), device, Some(2)),
                Err("wsl_native_filesystem_required")
            );
            assert_eq!(
                native_mount(&mounts, Path::new("/home/project/file"), device, None),
                Err("wsl_native_filesystem_required")
            );
        }
        let ambiguous = format!("{native}2 0 8:1 / / rw - ext4 /dev/sda rw\n");
        assert!(native_mount(&ambiguous, Path::new("/home"), device, None).is_err());
        assert!(native_mount(native, Path::new("/home"), libc::makedev(8, 2), Some(1)).is_err());
        assert!(native_mount(native, Path::new("/home"), device, Some(9)).is_err());
        assert_eq!(
            mount_path(r"/home/한글\040project").unwrap(),
            Path::new("/home/한글 project")
        );
    }
    #[test]
    fn native_observation_never_grants_links_devices_or_foreign_mounts() {
        let directory = tempfile::Builder::new()
            .prefix(".wsl-files-fixture-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap();
        let file = directory.path().join("한글.txt");
        std::fs::write(&file, b"original").unwrap();
        admit(&file).unwrap();
        admit(&directory.path().join("missing")).unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(admit(&link).is_err());
        assert!(admit(Path::new("/proc/self/status")).is_err());
        assert!(admit(Path::new("/dev/null")).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"original");
    }
}
