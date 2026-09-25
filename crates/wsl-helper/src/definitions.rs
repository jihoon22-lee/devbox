//! Linux-owned definition evidence. This opens regular files only; reading or
//! reviewing definitions grants no command, package, Git or language-server work.
use crate::{files::RootLease, project_files::ProjectFiles};
use devbox_filesystem::{project::ProjectObservation, FilesystemIdentity};
use product_contract::{ExecutionTarget, ProjectContext};
use serde::Deserialize;
use std::path::Path;
type Result<T> = std::result::Result<T, &'static str>;

struct Lease {
    observation: ProjectObservation,
    target: ExecutionTarget,
}
impl RootLease for Lease {
    fn root(&self) -> &Path {
        self.observation.root()
    }
    fn target(&self) -> &ExecutionTarget {
        &self.target
    }
    fn native_root_identity(&self) -> FilesystemIdentity {
        self.observation.root_identity()
    }
    fn revalidate(&self) -> Result<()> {
        self.observation.revalidate()
    }
}
pub(crate) struct Definitions {
    context: ProjectContext,
    files: Option<ProjectFiles<Lease>>,
    manifest_read: bool,
}
impl Definitions {
    pub(crate) fn context(&self) -> &ProjectContext {
        &self.context
    }
    pub(crate) fn capture(
        original: &ProjectObservation,
        context: ProjectContext,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        guard()?;
        original.revalidate()?;
        let observation = ProjectObservation::capture(original.root(), crate::linux_files::admit)?;
        if observation.root_identity() != original.root_identity()
            || observation.repository_identity() != original.repository_identity()
        {
            return Err("project_object_changed");
        }
        original.revalidate()?;
        let lease = Lease {
            observation,
            target: context.target.clone(),
        };
        let files = ProjectFiles::new_with_admission(lease, crate::linux_files::admit, guard)?;
        Ok(Self {
            context,
            files: Some(files),
            manifest_read: false,
        })
    }
    pub(crate) fn read(
        &mut self,
        relative: &str,
        optional: bool,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Option<Vec<u8>>> {
        let result = self
            .files
            .as_mut()
            .ok_or("definition_preview_stale")?
            .read_guarded(relative, optional, guard);
        if result.is_ok() && relative == crate::definition_write::MANIFEST {
            self.manifest_read = true;
        }
        result
    }
    pub(crate) fn revalidate(&self, guard: &dyn Fn() -> Result<()>) -> Result<()> {
        self.files
            .as_ref()
            .ok_or("definition_preview_stale")?
            .revalidate_guarded(guard)
    }
    pub(crate) fn write(
        &mut self,
        content: &str,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Option<String>> {
        if !self.manifest_read {
            return Err("wsl_context_required");
        }
        // Consume before IO, including failure. Neither a new pipe sequence
        // nor another attach may replay a possibly committed write.
        let files = self.files.take().ok_or("definition_preview_stale")?;
        crate::definition_write::write(files, content.as_bytes(), guard)
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Method {
    #[serde(rename = "definitions_attach")]
    Attach { context: ProjectContext },
    #[serde(rename = "definitions_read")]
    Read {
        context: ProjectContext,
        path: String,
        optional: bool,
    },
    #[serde(rename = "definitions_validate")]
    Validate { context: ProjectContext },
    #[serde(rename = "definitions_write")]
    Write {
        context: ProjectContext,
        content: String,
    },
}
impl Method {
    pub(crate) fn context(&self) -> &ProjectContext {
        match self {
            Self::Attach { context }
            | Self::Read { context, .. }
            | Self::Validate { context }
            | Self::Write { context, .. } => context,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, fs};
    const MANIFEST: &str = crate::definition_write::MANIFEST;
    fn fixture() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".wsl-definition-write-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
    }
    fn capture(path: &Path) -> Definitions {
        let observation = ProjectObservation::capture(path, crate::engine::admit).unwrap();
        Definitions::capture(
            &observation,
            ProjectContext {
                project_id: "project".into(),
                worktree_id: "tree".into(),
                revision: 1,
                target: ExecutionTarget::Wsl {
                    distro_id: uuid::Uuid::new_v4().to_string(),
                },
            },
            &|| Ok(()),
        )
        .unwrap()
    }
    fn reviewed(path: &Path) -> Definitions {
        let mut definitions = capture(path);
        definitions.read(MANIFEST, true, &|| Ok(())).unwrap();
        definitions
    }
    #[test]
    fn new_manifest_is_complete_and_each_review_can_write_only_once() {
        let root = fixture();
        let mut definitions = capture(root.path());
        let content = "{\"schemaVersion\":1,\"expectedPorts\":[8080]}";
        assert_eq!(
            definitions.write(content, &|| Ok(())),
            Err("wsl_context_required")
        );
        assert!(!root.path().join(".devbox").exists());
        definitions.read(MANIFEST, true, &|| Ok(())).unwrap();
        definitions.write(content, &|| Ok(())).unwrap();
        assert_eq!(
            fs::read(root.path().join(MANIFEST)).unwrap(),
            content.as_bytes()
        );
        assert_eq!(
            fs::read_dir(root.path().join(".devbox")).unwrap().count(),
            1
        );
        assert_eq!(
            definitions.write(content, &|| Ok(())),
            Err("definition_preview_stale")
        );
        assert_eq!(
            definitions.revalidate(&|| Ok(())),
            Err("definition_preview_stale")
        );
    }
    #[test]
    fn concurrent_parent_and_leaf_creators_are_preserved() {
        let root = fixture();
        let mut definitions = reviewed(root.path());
        fs::create_dir(root.path().join(".devbox")).unwrap();
        assert_eq!(
            definitions.write("{\"schemaVersion\":1}", &|| Ok(())),
            Err("project_definition_changed")
        );
        assert!(!root.path().join(MANIFEST).exists());
        let mut definitions = reviewed(root.path());
        fs::write(root.path().join(MANIFEST), b"external creator").unwrap();
        assert_eq!(
            definitions.write("{\"schemaVersion\":1}", &|| Ok(())),
            Err("project_definition_changed")
        );
        assert_eq!(
            fs::read(root.path().join(MANIFEST)).unwrap(),
            b"external creator"
        );
    }
    #[test]
    fn existing_crlf_survives_and_source_changes_before_replacement_abort() {
        let root = fixture();
        fs::create_dir(root.path().join(".devbox")).unwrap();
        let path = root.path().join(MANIFEST);
        fs::write(&path, b"{\r\n  \"schemaVersion\": 1\r\n}\r\n").unwrap();
        let mut definitions = reviewed(root.path());
        definitions
            .write(
                "{\n  \"schemaVersion\": 1,\n  \"expectedPorts\": [8080]\n}\n",
                &|| Ok(()),
            )
            .unwrap();
        let expected = fs::read(&path).unwrap();
        assert_eq!(
            expected,
            b"{\r\n  \"schemaVersion\": 1,\r\n  \"expectedPorts\": [8080]\r\n}\r\n"
        );
        let source = root.path().join("package.json");
        fs::write(&source, b"original source").unwrap();
        let mut definitions = reviewed(root.path());
        definitions.read("package.json", false, &|| Ok(())).unwrap();
        let changed = Cell::new(false);
        let guard = || {
            let staged = fs::read_dir(root.path().join(".devbox"))
                .unwrap()
                .flatten()
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".code-pad-")
                });
            if staged && !changed.replace(true) {
                fs::write(&source, b"changed source").unwrap();
            }
            Ok(())
        };
        assert_eq!(
            definitions.write("{\"schemaVersion\":1}", &guard),
            Err("project_definition_changed")
        );
        assert!(changed.get());
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(fs::read(&source).unwrap(), b"changed source");
        assert_eq!(
            fs::read_dir(root.path().join(".devbox")).unwrap().count(),
            1
        );
    }
    #[test]
    fn cancelling_staged_creation_keeps_the_manifest_absent_and_cleans_only_owned_data() {
        let root = fixture();
        let mut definitions = reviewed(root.path());
        let cancelled = Cell::new(false);
        let guard = || {
            if root.path().join(".devbox").read_dir().is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".devbox-definition-")
                })
            }) {
                cancelled.set(true);
                return Err("wsl_request_cancelled");
            }
            Ok(())
        };
        assert_eq!(
            definitions.write("{\"schemaVersion\":1}", &guard),
            Err("wsl_request_cancelled")
        );
        assert!(cancelled.get());
        assert!(!root.path().join(MANIFEST).exists());
        assert_eq!(
            fs::read_dir(root.path().join(".devbox")).unwrap().count(),
            0
        );
        assert_eq!(
            definitions.write("{\"schemaVersion\":1}", &|| Ok(())),
            Err("definition_preview_stale")
        );
    }
    #[test]
    fn changed_parent_during_creation_does_not_publish_outside_the_reviewed_root() {
        let root = fixture();
        let outside = fixture();
        fs::create_dir(root.path().join(".devbox")).unwrap();
        let mut definitions = reviewed(root.path());
        let changed = Cell::new(false);
        let guard = || {
            if root.path().join(".devbox").read_dir().is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".devbox-definition-")
                })
            }) && !changed.replace(true)
            {
                fs::rename(root.path().join(".devbox"), root.path().join("previous")).unwrap();
                std::os::unix::fs::symlink(outside.path(), root.path().join(".devbox")).unwrap();
            }
            Ok(())
        };
        assert!(definitions.write("{\"schemaVersion\":1}", &guard).is_err());
        assert!(changed.get());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert_eq!(
            fs::read_dir(root.path().join("previous")).unwrap().count(),
            0
        );
    }
}
