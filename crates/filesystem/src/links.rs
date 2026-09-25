//! Link rules shared by every user-content path check.
//!
//! A *link* names another location: a Unix symlink, or a Windows reparse
//! point whose tag has the name-surrogate bit (symbolic link, junction, mount
//! point). Storage-layer reparse points — OneDrive/Cloud Files placeholders,
//! WOF compression, data deduplication — are ordinary files and directories.

/// `metadata` must come from `symlink_metadata`. On Windows the standard
/// library reports `is_symlink()` exactly for name-surrogate reparse points.
pub fn is_link_metadata(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// `IsReparseTagNameSurrogate` from the Windows SDK.
pub const fn is_name_surrogate_tag(tag: u32) -> bool {
    tag & 0x2000_0000 != 0
}

/// Fold a 128-bit `FILE_ID_INFO` identifier into the 64-bit identity
/// component. NTFS identifiers fit in the low half and keep their exact
/// value; ReFS (Dev Drive) identifiers use all 128 bits.
pub fn object_from_file_id(id: [u8; 16]) -> u64 {
    let low = u64::from_le_bytes(id[..8].try_into().expect("8 bytes"));
    let high = u64::from_le_bytes(id[8..].try_into().expect("8 bytes"));
    if high == 0 {
        return low;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_name_surrogate_tags_are_links() {
        assert!(is_name_surrogate_tag(0xA000_000C)); // IO_REPARSE_TAG_SYMLINK
        assert!(is_name_surrogate_tag(0xA000_0003)); // IO_REPARSE_TAG_MOUNT_POINT (junction)
        assert!(!is_name_surrogate_tag(0x9000_001A)); // IO_REPARSE_TAG_CLOUD
        assert!(!is_name_surrogate_tag(0x9000_601A)); // IO_REPARSE_TAG_CLOUD_6 (OneDrive)
        assert!(!is_name_surrogate_tag(0x8000_0017)); // IO_REPARSE_TAG_WOF
        assert!(!is_name_surrogate_tag(0x8000_0013)); // IO_REPARSE_TAG_DEDUP
    }

    #[test]
    fn ntfs_ids_keep_their_value_and_refs_ids_use_all_bits() {
        let mut ntfs = [0u8; 16];
        ntfs[..8].copy_from_slice(&42u64.to_le_bytes());
        assert_eq!(object_from_file_id(ntfs), 42);

        let mut first = [0u8; 16];
        let mut second = [0u8; 16];
        first[..8].copy_from_slice(&7u64.to_le_bytes());
        second[..8].copy_from_slice(&7u64.to_le_bytes());
        first[8..].copy_from_slice(&1u64.to_le_bytes());
        second[8..].copy_from_slice(&2u64.to_le_bytes());
        assert_ne!(object_from_file_id(first), object_from_file_id(second));
        assert_eq!(object_from_file_id(first), object_from_file_id(first));
    }

    #[cfg(unix)]
    #[test]
    fn unix_symlinks_are_links_and_files_are_not() {
        let dir = std::env::temp_dir().join(format!("links-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("file"), dir.join("link")).unwrap();
        assert!(!is_link_metadata(
            &std::fs::symlink_metadata(dir.join("file")).unwrap()
        ));
        assert!(is_link_metadata(
            &std::fs::symlink_metadata(dir.join("link")).unwrap()
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
