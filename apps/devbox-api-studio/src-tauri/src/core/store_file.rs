//! Bounded identity-checked reads of current product files.
use std::{fs, io::Read, path::Path};
pub fn read_file(path: &Path, limit: usize) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("store_storage_unavailable".into()),
        Ok(_) => {}
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| "store_path_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "store_storage_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "store_storage_unavailable")?;
    if bytes.len() > limit
        || devbox_filesystem::filesystem_identity(path, false)
            .map_err(|_| "store_storage_unavailable")?
            != identity
    {
        return Err("store_source_changed_or_large".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "store_schema_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_utf8_with_a_hard_byte_limit_and_preserves_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert_eq!(read_file(&path, 4).unwrap(), None);
        fs::write(&path, "text").unwrap();
        assert_eq!(read_file(&path, 4).unwrap(), Some("text".into()));
        assert!(read_file(&path, 3).is_err());
        fs::write(&path, [255]).unwrap();
        assert!(read_file(&path, 4).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn refuses_a_link_to_a_current_store() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.json");
        let link = dir.path().join("link");
        fs::write(&file, "{}").unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(read_file(&link, 4).is_err());
    }
}
