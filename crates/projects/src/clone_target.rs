use crate::{Fail, ProjectsError, R};
use nix::{
    fcntl::{OFlag, open, openat},
    sys::stat::{Mode, mkdirat},
};
use std::{
    fs::File,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

pub(crate) struct CloneTarget {
    pub(crate) path: PathBuf,
    pub(crate) directory: File,
}

const FLAGS: OFlag = OFlag::O_RDONLY
    .union(OFlag::O_CLOEXEC)
    .union(OFlag::O_NOFOLLOW)
    .union(OFlag::O_DIRECTORY);

fn invalid(path: &Path) -> Fail<ProjectsError> {
    ProjectsError::PathInvalid {
        path: path.display().to_string(),
    }
    .into()
}

impl CloneTarget {
    pub(crate) fn reserve(path: &Path) -> R<Self> {
        let mut parent = open(Path::new("/"), FLAGS, Mode::empty()).map_err(|_| invalid(path))?;
        let mut normalized = PathBuf::from("/");
        let components: Vec<_> = path
            .components()
            .filter(|part| !matches!(part, Component::RootDir))
            .collect();
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(invalid(path));
            };
            normalized.push(name);
            let final_component = index + 1 == components.len();
            if final_component {
                mkdirat(&parent, Path::new(name), Mode::from_bits_truncate(0o755)).map_err(
                    |error| {
                        if error == nix::errno::Errno::EEXIST {
                            ProjectsError::CloneTargetExists {
                                path: normalized.display().to_string(),
                            }
                            .into()
                        } else {
                            invalid(path)
                        }
                    },
                )?;
            }
            let next = match openat(&parent, Path::new(name), FLAGS, Mode::empty()) {
                Ok(directory) => directory,
                Err(nix::errno::Errno::ENOENT) if !final_component => {
                    match mkdirat(&parent, Path::new(name), Mode::from_bits_truncate(0o755)) {
                        Ok(()) | Err(nix::errno::Errno::EEXIST) => (),
                        Err(_) => return Err(invalid(path)),
                    }
                    openat(&parent, Path::new(name), FLAGS, Mode::empty())
                        .map_err(|_| invalid(path))?
                }
                Err(_) => return Err(invalid(path)),
            };
            parent = next;
        }
        if components.is_empty() {
            return Err(ProjectsError::CloneTargetExists {
                path: path.display().to_string(),
            }
            .into());
        }
        let result = Self {
            path: normalized,
            directory: File::from(parent),
        };
        result.verify()?;
        Ok(result)
    }

    pub(crate) fn verify(&self) -> Result<(), ProjectsError> {
        let changed = || ProjectsError::CloneFailed {
            path: self.path.display().to_string(),
            message: "Clone destination changed; preserved resources were not registered.".into(),
        };
        let mut observed = open(Path::new("/"), FLAGS, Mode::empty()).map_err(|_| changed())?;
        for component in self.path.components() {
            if let Component::Normal(name) = component {
                observed = openat(&observed, Path::new(name), FLAGS, Mode::empty())
                    .map_err(|_| changed())?;
            }
        }
        let current = File::from(observed).metadata().map_err(|_| changed())?;
        let reserved = self.directory.metadata().map_err(|_| changed())?;
        if current.dev() != reserved.dev() || current.ino() != reserved.ino() {
            return Err(changed());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clone_reservation_rejects_existing_targets_and_symlinked_parents() {
        let root = tempfile::tempdir().expect("owned clone reservation fixture");
        let existing = root.path().join("existing");
        std::fs::create_dir(&existing).expect("owned clone reservation fixture");
        std::fs::write(existing.join("precious"), "preserve")
            .expect("owned clone reservation fixture");
        assert!(matches!(
            CloneTarget::reserve(&existing),
            Err(Fail::Domain(ProjectsError::CloneTargetExists { .. }))
        ));
        let external = root.path().join("outside");
        std::fs::create_dir(&external).expect("owned clone reservation fixture");
        std::os::unix::fs::symlink(&external, root.path().join("alias"))
            .expect("owned clone reservation fixture");
        assert!(CloneTarget::reserve(&root.path().join("alias/new/clone")).is_err());
        assert!(!external.join("new").exists());
        assert_eq!(
            std::fs::read_to_string(existing.join("precious"))
                .expect("owned clone reservation fixture"),
            "preserve"
        );
    }

    #[test]
    fn clone_reservation_detects_replacement_directory_without_following_links() {
        let root = tempfile::tempdir().expect("owned clone reservation fixture");
        let path = root.path().join("target");
        let reservation = CloneTarget::reserve(&path).expect("owned clone reservation fixture");
        let retained = root.path().join("retained");
        std::fs::rename(&path, &retained).expect("owned clone reservation fixture");
        std::fs::create_dir(&path).expect("owned clone reservation fixture");
        assert!(matches!(
            reservation.verify(),
            Err(ProjectsError::CloneFailed { .. })
        ));
        std::fs::remove_dir(&path).expect("owned clone reservation fixture");
        std::os::unix::fs::symlink(&retained, &path).expect("owned clone reservation fixture");
        assert!(reservation.verify().is_err());
        assert!(retained.is_dir());
    }
}
