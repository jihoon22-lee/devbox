//! Reviewed Linux manifest publication. Creation uses retained directory
//! descriptors and never replaces a concurrent creator. Only owned staging
//! names are removed; failure can leave an empty owned .devbox directory.
use crate::{files::RootLease, project_files::ProjectFiles};
use code_pad_lib::commands::file::{self, ExpectedFileSnapshot, FileError};
use devbox_filesystem::{filesystem_identity, opened_filesystem_identity, FilesystemIdentity};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    ffi::{CStr, CString},
    fs::File,
    io::{self, Write},
    os::fd::{AsRawFd, FromRawFd},
};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) const MANIFEST: &str = ".devbox/project.json";
const LIMIT: u64 = 256 * 1024;

fn open_at(parent: &File, name: &CStr, flags: i32, mode: u32) -> io::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}
fn identity_at(parent: &File, name: &CStr) -> Option<FilesystemIdentity> {
    let file = open_at(parent, name, libc::O_PATH, 0).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    opened_filesystem_identity(&file, false).ok()
}
struct Staging<'a> {
    parent: &'a File,
    name: CString,
    identity: FilesystemIdentity,
    file: File,
}
impl Drop for Staging<'_> {
    fn drop(&mut self) {
        if identity_at(self.parent, &self.name) == Some(self.identity) {
            // The directory descriptor cannot be redirected by renaming its
            // original path, and a substituted staging leaf is never removed.
            unsafe {
                libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0);
            }
        }
    }
}
pub(crate) fn write<L: RootLease>(
    mut files: ProjectFiles<L>,
    bytes: &[u8],
    guard: &dyn Fn() -> Result<()>,
) -> Result<Option<String>> {
    crate::manifest::Manifest::parse(bytes)?;
    files.revalidate_guarded(guard)?;
    let expected = files.read_guarded(MANIFEST, true, guard)?;
    let path = files.lease().root().join(MANIFEST);
    if let Some(expected) = expected {
        let opened =
            file::open_path_limited(&path, LIMIT).map_err(|_| "project_definition_unavailable")?;
        let hash = Sha256::digest(&expected)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if opened.size > LIMIT || opened.content_hash != hash {
            return Err("project_definition_changed");
        }
        let failure = Cell::new(None);
        let validate = || {
            files.revalidate_guarded(guard).map_err(|issue| {
                failure.set(Some(issue));
                FileError::BackupIntegrity
            })
        };
        let saved = file::save_path_with_policy(
            &path,
            std::str::from_utf8(bytes).map_err(|_| "invalid_manifest")?,
            opened.encoding,
            opened.line_ending,
            ExpectedFileSnapshot {
                mtime: opened.mtime,
                size: opened.size,
                content_hash: &opened.content_hash,
                identity: Some(opened.native_identity()),
            },
            opened.lossy,
            file::SavePolicy {
                max_bytes: Some(LIMIT),
                guard: &validate,
            },
        )
        .map_err(|_| failure.get().unwrap_or("project_definition_changed"))?;
        return Ok(saved
            .durability_warning
            .map(|_| "definition_durability_warning".into()));
    }
    // Opening the already-pinned root before mkdirat prevents parent-path
    // substitution from creating .devbox in a different directory.
    guard()?;
    let (root, root_identity) =
        devbox_filesystem::open_filesystem_object(files.lease().root(), true)
            .map_err(|_| "project_definition_changed")?;
    if root_identity != files.lease().native_root_identity() {
        return Err("project_definition_changed");
    }
    files.revalidate_guarded(guard)?;
    let parent_path = files.lease().root().join(".devbox");
    let parent = match open_at(&root, c".devbox", libc::O_RDONLY | libc::O_DIRECTORY, 0) {
        Ok(parent) => parent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            files.revalidate_guarded(guard)?;
            if unsafe { libc::mkdirat(root.as_raw_fd(), c".devbox".as_ptr(), 0o755) } != 0 {
                return Err("project_definition_changed");
            }
            let created = open_at(&root, c".devbox", libc::O_RDONLY | libc::O_DIRECTORY, 0)
                .map_err(|_| "project_definition_changed")?;
            files.retain_created_directory(".devbox", &created, guard)?;
            // The newly owned parent now retains the absent manifest leaf.
            // A concurrent creator is still a conflict, never a new baseline.
            if files.read_guarded(MANIFEST, true, guard)?.is_some() {
                return Err("project_definition_changed");
            }
            created
        }
        Err(_) => return Err("project_definition_changed"),
    };
    let parent_identity =
        opened_filesystem_identity(&parent, true).map_err(|_| "project_definition_changed")?;
    let validate = || {
        files.revalidate_guarded(guard)?;
        if filesystem_identity(&parent_path, true).ok() != Some(parent_identity) {
            return Err("project_definition_changed");
        }
        Ok(())
    };
    validate()?;
    let name = CString::new(format!(".devbox-definition-{}.tmp", uuid::Uuid::new_v4()))
        .map_err(|_| "project_definition_unavailable")?;
    let file = open_at(
        &parent,
        &name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o644,
    )
    .map_err(|_| "project_definition_unavailable")?;
    let identity =
        opened_filesystem_identity(&file, false).map_err(|_| "project_definition_unavailable")?;
    let mut staging = Staging {
        parent: &parent,
        name,
        identity,
        file,
    };
    for chunk in bytes.chunks(16384) {
        guard()?;
        staging
            .file
            .write_all(chunk)
            .map_err(|_| "project_definition_unavailable")?;
    }
    staging
        .file
        .sync_all()
        .map_err(|_| "project_definition_unavailable")?;
    validate()?;
    if identity_at(&parent, &staging.name) != Some(identity) {
        return Err("project_definition_changed");
    }
    // A complete, non-overwriting publication. An interrupted reply may leave
    // the manifest committed; the Windows owner must reopen, never replay.
    if unsafe {
        libc::linkat(
            parent.as_raw_fd(),
            staging.name.as_ptr(),
            parent.as_raw_fd(),
            c"project.json".as_ptr(),
            0,
        )
    } != 0
    {
        return Err("project_definition_changed");
    }
    let warning = parent.sync_all().is_err()
        || identity_at(&parent, c"project.json") != Some(identity)
        || filesystem_identity(&parent_path, true).ok() != Some(parent_identity);
    Ok(warning.then(|| "definition_durability_warning".into()))
}
