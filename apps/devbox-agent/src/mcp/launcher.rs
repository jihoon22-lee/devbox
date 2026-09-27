pub use suite_runtime::mcp_launcher::{refresh, stable_path};
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_copies_once_and_replaces_a_changed_copy() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("agent-new.exe");
        std::fs::write(&exe, b"agent v2").unwrap();
        let stable = refresh(root.path(), &exe).unwrap();
        assert_eq!(stable, root.path().join("bin").join("devbox-mcp.exe"));
        assert_eq!(std::fs::read(&stable).unwrap(), b"agent v2");
        let before = std::fs::metadata(&stable).unwrap().modified().unwrap();
        refresh(root.path(), &exe).unwrap();
        assert_eq!(
            std::fs::metadata(&stable).unwrap().modified().unwrap(),
            before,
            "same content is not rewritten"
        );
        std::fs::write(&exe, b"agent v3").unwrap();
        refresh(root.path(), &exe).unwrap();
        assert_eq!(std::fs::read(&stable).unwrap(), b"agent v3");
        let leftovers: Vec<_> = std::fs::read_dir(root.path().join("bin"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != "devbox-mcp.exe")
            .collect();
        assert!(
            leftovers.is_empty(),
            "old copies are removed when they are not in use: {leftovers:?}"
        );
    }
}
