//! Workspace maps retained native root/Git observations into Registry evidence.
use crate::core::registry::{Binding, ObjectStamp};
#[cfg(windows)]
use devbox_filesystem::ensure_no_links;
use devbox_filesystem::{project::ProjectObservation, FilesystemIdentity};
use product_contract::ExecutionTarget;
#[cfg(any(windows, test))]
use std::fs;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
type Result<T> = std::result::Result<T, &'static str>;

pub struct ProjectLease {
    binding: Binding,
    observation: ProjectObservation,
}
impl ProjectLease {
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn native_root_identity(&self) -> FilesystemIdentity {
        self.observation.root_identity()
    }
    pub fn git_directories(&self) -> Option<(&Path, &Path)> {
        self.observation.git_directories()
    }
    pub fn revalidate(&self) -> Result<()> {
        self.observation.revalidate()
    }
}
/// Windows-local/ordinary UNC discovery only. WSL requires a separate native
/// distro binding and running-state admission before even opening its UNC root.
pub fn probe_windows(root: &str) -> Result<ProjectLease> {
    #[cfg(windows)]
    {
        let parsed = devbox_filesystem::parse_safe_project_path(root).ok_or("invalid_root")?;
        if !matches!(
            parsed.kind(),
            devbox_filesystem::ProjectPathKind::WindowsDrive
                | devbox_filesystem::ProjectPathKind::WindowsUnc
        ) || devbox_wsl::path::parse_wsl_unc_path(root)
            .map_err(|_| "invalid_root")?
            .is_some()
        {
            return Err("invalid_target");
        }
        let root = PathBuf::from(root);
        super::windows_path::admit(&root)?;
        ensure_no_links(&root).map_err(|_| "unsafe_project_object")?;
        let canonical = fs::canonicalize(&root).map_err(|_| "project_object_unavailable")?;
        let spelling = canonical.to_str().ok_or("invalid_root")?;
        let spelling = if let Some(unc) = spelling.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            spelling
                .strip_prefix(r"\\?\")
                .unwrap_or(spelling)
                .to_owned()
        };
        probe(
            Path::new(&spelling),
            ExecutionTarget::Windows,
            spelling.clone(),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Err("windows_required")
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn probe(root: &Path, target: ExecutionTarget, spelling: String) -> Result<ProjectLease> {
    let observation = ProjectObservation::capture(root, super::windows_path::admit)?;
    let stamp = |id: FilesystemIdentity| {
        let (scope, object) = id.components();
        ObjectStamp {
            scope: format!("{scope:x}"),
            object: format!("{object:x}"),
        }
    };
    let binding = Binding {
        target,
        root: spelling,
        root_object: stamp(observation.root_identity()),
        repository_object: observation.repository_identity().map(stamp),
    };
    binding.validate()?;
    Ok(ProjectLease {
        binding,
        observation,
    })
}
#[cfg(test)]
pub(crate) fn probe_fixture(root: &Path) -> Result<ProjectLease> {
    #[cfg(unix)]
    let target = ExecutionTarget::Wsl {
        distro_id: "native-test-fixture".into(),
    };
    #[cfg(windows)]
    let target = ExecutionTarget::Windows;
    probe(root, target, root.to_str().unwrap().into())
}

#[cfg(test)]
mod tests {
    use super::probe_fixture as observe;
    use super::*;
    fn repository(root: &Path) {
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }
    #[test]
    fn directory_replacement_and_new_git_metadata_revoke_observation() {
        let owner = tempfile::tempdir().unwrap();
        let root = owner.path().join("프로젝트 space");
        fs::create_dir(&root).unwrap();
        let plain = observe(&root).unwrap();
        assert!(plain.binding().repository_object.is_none());
        repository(&root);
        assert!(plain.revalidate().is_err());
        let repo = observe(&root).unwrap();
        assert!(repo.binding().repository_object.is_some());
        let before = repo.binding().clone();
        match fs::rename(&root, owner.path().join("old")) {
            Ok(()) => {
                repository(&root);
                assert!(repo.revalidate().is_err());
            }
            #[cfg(windows)]
            Err(error) => {
                // Windows can forbid renaming a directory while its Git
                // children have open handles. The reviewed objects remain
                // valid; release the native lease before testing replacement.
                assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
                repo.revalidate().unwrap();
                drop(repo);
                drop(plain);
                fs::rename(&root, owner.path().join("old")).unwrap();
                repository(&root);
            }
            #[cfg(not(windows))]
            Err(error) => panic!("fixture rename failed: {error}"),
        }
        assert_ne!(&before, observe(&root).unwrap().binding());
    }
    #[test]
    fn linked_worktrees_share_common_object_and_pointer_edits_are_detected() {
        let owner = tempfile::tempdir().unwrap();
        let main = owner.path().join("main");
        repository(&main);
        let linked = owner.path().join("linked 한글");
        fs::create_dir(&linked).unwrap();
        let metadata = main.join(".git/worktrees/linked");
        fs::create_dir_all(&metadata).unwrap();
        fs::write(
            linked.join(".git"),
            "gitdir: ../main/.git/worktrees/linked\n",
        )
        .unwrap();
        fs::write(metadata.join("commondir"), "../..\n").unwrap();
        fs::write(metadata.join("gitdir"), "../../../../linked 한글/.git\n").unwrap();
        fs::write(metadata.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        let first = observe(&main).unwrap();
        let second = observe(&linked).unwrap();
        assert_eq!(
            first.binding().repository_object,
            second.binding().repository_object
        );
        assert_ne!(first.binding().root_object, second.binding().root_object);
        fs::write(metadata.join("commondir"), "../../elsewhere\n").unwrap();
        assert!(second.revalidate().is_err());
        assert!(observe(&linked).is_err());
    }
    #[test]
    fn invalid_git_metadata_does_not_fall_back_to_plain_folder() {
        let owner = tempfile::tempdir().unwrap();
        for content in [
            "gitdir: missing\n",
            "gitdir: ../a\ninjected\n",
            "arbitrary\n",
        ] {
            fs::write(owner.path().join(".git"), content).unwrap();
            assert!(observe(owner.path()).is_err());
        }
        fs::write(
            owner.path().join(".git"),
            vec![b'a'; devbox_filesystem::project::MAX_GIT_POINTER_BYTES as usize + 1],
        )
        .unwrap();
        assert!(observe(owner.path()).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_roots_and_git_pointers_fail_without_following_them() {
        use std::os::unix::fs::symlink;
        let owner = tempfile::tempdir().unwrap();
        let real = owner.path().join("real");
        repository(&real);
        let alias = owner.path().join("alias");
        symlink(&real, &alias).unwrap();
        assert!(observe(&alias).is_err());
        let linked = owner.path().join("linked");
        fs::create_dir(&linked).unwrap();
        fs::write(linked.join(".git"), "gitdir: ../alias/.git\n").unwrap();
        assert!(observe(&linked).is_err());
    }
}
