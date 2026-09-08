//! A one-time worktree destination retains its existing parent and absence.
//! Capturing this boundary never creates a directory or invokes Git.
use super::git_files;
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, parse_safe_project_path,
    FilesystemIdentity,
};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;

pub struct WorktreeTarget {
    project: PathBuf,
    path: PathBuf,
    parent: PathBuf,
    parent_identity: FilesystemIdentity,
    _parent_handle: File,
}
fn key(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("worktree_target_invalid")?;
    let text = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(text).to_owned()
    };
    Ok(parse_safe_project_path(&text)
        .ok_or("worktree_target_invalid")?
        .identity()
        .replace('\\', "/"))
}
fn inside(parent: &str, child: &str) -> bool {
    child == parent
        || child
            .strip_prefix(parent.trim_end_matches('/'))
            .is_some_and(|tail| tail.starts_with('/'))
}
impl WorktreeTarget {
    pub fn capture(
        project: &Path,
        raw: &str,
        forbidden: &[PathBuf],
        deadline: u64,
    ) -> Result<Self> {
        if raw.len() > 32768 || !Path::new(raw).is_absolute() {
            return Err("worktree_target_invalid");
        }
        let path = git_files::resolve(project, project, Path::new(raw), deadline)?;
        let target_key = key(&path)?;
        // Git metadata is not a place to create another working tree. Product
        // authority-storage admission is additionally checked by the caller.
        if target_key
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
            || forbidden
                .iter()
                .any(|root| key(root).map_or(true, |root| inside(&root, &target_key)))
        {
            return Err("worktree_target_invalid");
        }
        let original_parent = path.parent().ok_or("worktree_target_invalid")?.to_owned();
        git_files::transport(project, &original_parent)?;
        ensure_no_links(&original_parent).map_err(|_| "worktree_target_invalid")?;
        let (handle, identity) = open_filesystem_object(&original_parent, true)
            .map_err(|_| "worktree_target_unavailable")?;
        let parent = super::storage_paths::display(
            &fs::canonicalize(&original_parent).map_err(|_| "worktree_target_unavailable")?,
        )?;
        git_files::transport(project, &parent)?;
        ensure_no_links(&parent).map_err(|_| "worktree_target_invalid")?;
        if filesystem_identity(&parent, true).ok() != Some(identity) {
            return Err("worktree_target_changed");
        }
        let path = parent.join(path.file_name().ok_or("worktree_target_invalid")?);
        let target_key = key(&path)?;
        if target_key
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
            || forbidden
                .iter()
                .any(|root| key(root).map_or(true, |root| inside(&root, &target_key)))
        {
            return Err("worktree_target_invalid");
        }
        let target = Self {
            project: project.into(),
            path,
            parent,
            parent_identity: identity,
            _parent_handle: handle,
        };
        target.revalidate(deadline)?;
        Ok(target)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn revalidate(&self, deadline: u64) -> Result<()> {
        crate::files_host::current_deadline(deadline)?;
        git_files::transport(&self.project, &self.path)?;
        ensure_no_links(&self.parent).map_err(|_| "worktree_target_changed")?;
        if filesystem_identity(&self.parent, true).ok() != Some(self.parent_identity)
            || !matches!(fs::symlink_metadata(&self.path),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("worktree_target_changed");
        }
        crate::files_host::current_deadline(deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_creates_nothing_and_rejects_a_concurrent_destination() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("새 작업 폴더");
        let target =
            WorktreeTarget::capture(root.path(), path.to_str().unwrap(), &[], u64::MAX).unwrap();
        assert!(!path.exists());
        fs::create_dir(&path).unwrap();
        assert_eq!(target.revalidate(u64::MAX), Err("worktree_target_changed"));
        assert!(path.is_dir());
    }
    #[test]
    fn missing_parent_and_git_or_private_destination_are_not_created() {
        let root = tempfile::tempdir().unwrap();
        for name in ["missing/new", ".git/new"] {
            assert!(WorktreeTarget::capture(
                root.path(),
                root.path().join(name).to_str().unwrap(),
                &[],
                u64::MAX
            )
            .is_err());
        }
        let store = root.path().join("private");
        fs::create_dir(&store).unwrap();
        assert!(WorktreeTarget::capture(
            root.path(),
            store.join("new").to_str().unwrap(),
            std::slice::from_ref(&store),
            u64::MAX
        )
        .is_err());
        assert_eq!(fs::read_dir(&store).unwrap().count(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn a_parent_replaced_by_a_link_never_redirects_creation() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let target = WorktreeTarget::capture(
            root.path(),
            parent.join("new").to_str().unwrap(),
            &[],
            u64::MAX,
        )
        .unwrap();
        fs::rename(&parent, root.path().join("old")).unwrap();
        std::os::unix::fs::symlink(outside.path(), &parent).unwrap();
        assert_eq!(target.revalidate(u64::MAX), Err("worktree_target_changed"));
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
