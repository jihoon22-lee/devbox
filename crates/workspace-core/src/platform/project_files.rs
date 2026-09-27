//! Native Windows lease adapter for shared bounded definition snapshots.
pub type ProjectFiles =
    workspace_wsl::project_files::ProjectFiles<super::project_probe::ProjectLease>;

#[cfg(test)]
mod tests {
    use super::super::project_probe::probe_fixture;
    use super::*;
    use std::fs::{self, File};
    const MAX_FILE: u64 = 2 * 1024 * 1024;
    #[test]
    fn same_object_changed_bytes_invalidate_a_reviewed_source() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), b"first").unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert_eq!(
            files.read("package.json", false).unwrap().unwrap(),
            b"first"
        );
        fs::write(root.path().join("package.json"), b"other").unwrap();
        assert_eq!(
            files.revalidate().unwrap_err(),
            "project_definition_changed"
        );
    }
    #[test]
    fn creating_an_absent_manifest_parent_requires_a_new_review() {
        let root = tempfile::tempdir().unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert!(files.read(".devbox/project.json", true).unwrap().is_none());
        fs::create_dir(root.path().join(".devbox")).unwrap();
        assert_eq!(
            files.revalidate().unwrap_err(),
            "project_definition_changed"
        );
    }
    #[test]
    fn rejects_escape_and_bounds_source_contents() {
        let root = tempfile::tempdir().unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert!(files.read("../outside", false).is_err());
        assert!(files.read("C:/outside", false).is_err());
        let file = File::create(root.path().join("large.json")).unwrap();
        file.set_len(MAX_FILE + 1).unwrap();
        assert_eq!(
            files.read("large.json", false).unwrap_err(),
            "project_definition_limit"
        );
    }
    #[cfg(unix)]
    #[test]
    fn linked_parent_is_rejected_before_reading_its_target() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("package.json"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert!(files.read("linked/package.json", false).is_err());
    }
}
