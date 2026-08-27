//! Knowledge vault image asset storage.
//!
//! The frontend sends bounded base64 bytes after an explicit paste/drop
//! action. The native command is the final authority for format, dimensions,
//! hash naming, vault boundary, collision handling, and atomic publication.

use crate::commands::docs::{
    cleanup_vault_file, publish_new_vault_file, resolve_configured_root, AppState,
};
use crate::core::assets::{self, AssetError, MAX_ASSET_BYTES, MAX_NOTE_PATH_BYTES};
use crate::core::vault::{EntryIdentity, VaultIdentity};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const IMAGE_ASSET_ERROR: &str = "이미지 자산을 저장할 수 없습니다";
const MAX_BASE64_BYTES: usize = MAX_ASSET_BYTES.div_ceil(3) * 4;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct SaveImageAssetRequest {
    pub note_rel: String,
    pub bytes_base64: String,
}

/// Keep the image IPC envelope bounded before the command allocates a second
/// copy for base64 decoding.  This mirrors the quick-capture visitors: the
/// browser must still send a bounded payload, but native remains the final
/// authority when a caller bypasses that browser path.
impl<'de> Deserialize<'de> for SaveImageAssetRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RequestVisitor;

        impl<'de> Visitor<'de> for RequestVisitor {
            type Value = SaveImageAssetRequest;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded image asset request")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut note_rel = None;
                let mut bytes_base64 = None;
                while let Some(field) = map.next_key::<AssetRequestField>()? {
                    match field {
                        AssetRequestField::NoteRel => {
                            if note_rel.is_some() {
                                return Err(de::Error::custom("duplicate image asset field"));
                            }
                            note_rel =
                                Some(map.next_value::<BoundedAssetText<MAX_NOTE_PATH_BYTES>>()?.0);
                        }
                        AssetRequestField::BytesBase64 => {
                            if bytes_base64.is_some() {
                                return Err(de::Error::custom("duplicate image asset field"));
                            }
                            bytes_base64 =
                                Some(map.next_value::<BoundedAssetText<MAX_BASE64_BYTES>>()?.0);
                        }
                    }
                }
                Ok(SaveImageAssetRequest {
                    note_rel: note_rel
                        .ok_or_else(|| de::Error::custom("missing image asset note"))?,
                    bytes_base64: bytes_base64
                        .ok_or_else(|| de::Error::custom("missing image asset bytes"))?,
                })
            }
        }

        deserializer.deserialize_map(RequestVisitor)
    }
}

enum AssetRequestField {
    NoteRel,
    BytesBase64,
}

impl<'de> Deserialize<'de> for AssetRequestField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FieldVisitor;

        impl<'de> Visitor<'de> for FieldVisitor {
            type Value = AssetRequestField;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an image asset field")
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "noteRel" => Ok(AssetRequestField::NoteRel),
                    "bytesBase64" => Ok(AssetRequestField::BytesBase64),
                    _ => Err(E::custom("unknown image asset field")),
                }
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_identifier(FieldVisitor)
    }
}

struct BoundedAssetText<const LIMIT: usize>(String);

impl<'de, const LIMIT: usize> Deserialize<'de> for BoundedAssetText<LIMIT> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TextVisitor<const LIMIT: usize>(PhantomData<()>);

        impl<'de, const LIMIT: usize> Visitor<'de> for TextVisitor<LIMIT> {
            type Value = BoundedAssetText<LIMIT>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded image asset string")
            }

            fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(value)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.len() > LIMIT {
                    return Err(E::custom("image asset field exceeds its bound"));
                }
                Ok(BoundedAssetText(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.len() > LIMIT {
                    return Err(E::custom("image asset field exceeds its bound"));
                }
                Ok(BoundedAssetText(value))
            }
        }

        deserializer.deserialize_str(TextVisitor(PhantomData))
    }
}

impl fmt::Debug for SaveImageAssetRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SaveImageAssetRequest")
            .field("note_rel", &"<redacted>")
            .field("bytes_base64_len", &self.bytes_base64.len())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedImageAsset {
    /// Root-relative path. It is always generated from a content hash.
    pub relative_path: String,
    /// Complete Markdown image node to insert at the editor selection.
    pub markdown: String,
    /// True when the same hash and bytes were already present.
    pub reused: bool,
}

#[tauri::command]
pub fn save_image_asset(
    state: tauri::State<'_, Arc<AppState>>,
    request: SaveImageAssetRequest,
) -> Result<SavedImageAsset, String> {
    if request.note_rel.len() > MAX_NOTE_PATH_BYTES || request.bytes_base64.len() > MAX_BASE64_BYTES
    {
        return Err(IMAGE_ASSET_ERROR.to_string());
    }
    let bytes = decode_bytes(&request.bytes_base64).map_err(|_| IMAGE_ASSET_ERROR.to_string())?;
    let root = {
        let conn = state.db.lock().map_err(|_| IMAGE_ASSET_ERROR.to_string())?;
        resolve_configured_root(&conn).map_err(|_| IMAGE_ASSET_ERROR.to_string())?
    };
    let vault = VaultIdentity::inspect(&root).map_err(|_| IMAGE_ASSET_ERROR.to_string())?;
    save_image_asset_at(&vault, &request.note_rel, &bytes)
        .map_err(|_| IMAGE_ASSET_ERROR.to_string())
}

fn decode_bytes(encoded: &str) -> Result<Vec<u8>, AssetError> {
    if encoded.is_empty() || encoded.len() > MAX_BASE64_BYTES {
        return Err(AssetError::EmptyInput);
    }
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| AssetError::UnsupportedFormat)?;
    if bytes.len() > MAX_ASSET_BYTES || BASE64.encode(&bytes) != encoded {
        return Err(AssetError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
fn save_image_asset_in(
    root: &Path,
    note_rel: &str,
    bytes: &[u8],
) -> Result<SavedImageAsset, AssetError> {
    let vault = VaultIdentity::inspect(root).map_err(|_| AssetError::Storage)?;
    save_image_asset_at(&vault, note_rel, bytes)
}

fn save_image_asset_at(
    vault: &VaultIdentity,
    note_rel: &str,
    bytes: &[u8],
) -> Result<SavedImageAsset, AssetError> {
    assets::validate_note_path(note_rel)?;
    let note_path = vault
        .existing_entry(note_rel)
        .map_err(|_| AssetError::InvalidNotePath)?;
    let note_metadata =
        fs::symlink_metadata(&note_path).map_err(|_| AssetError::InvalidNotePath)?;
    if !note_metadata.is_file()
        || !note_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        return Err(AssetError::InvalidNotePath);
    }

    let format = assets::inspect(bytes)?;
    let hash = assets::content_hash_hex(bytes);
    let relative_path = assets::asset_relative_path(&hash, format)?;
    let markdown = assets::markdown_link(note_rel, &relative_path)?;
    let assets_dir = ensure_assets_dir(vault)?;
    let target = vault
        .new_entry(&relative_path)
        .map_err(|_| AssetError::Storage)?;
    if target.parent() != Some(assets_dir.as_path()) {
        return Err(AssetError::Storage);
    }

    let reused = match publish_asset(vault, &relative_path, &target, bytes)? {
        PublishResult::Created => false,
        PublishResult::Reused => true,
    };
    Ok(SavedImageAsset {
        relative_path,
        markdown,
        reused,
    })
}

fn ensure_assets_dir(vault: &VaultIdentity) -> Result<PathBuf, AssetError> {
    vault.revalidate().map_err(|_| AssetError::Storage)?;
    let candidate = vault
        .new_entry(assets::ASSET_DIR)
        .map_err(|_| AssetError::Storage)?;
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AssetError::Storage);
            }
            let existing = vault
                .existing_entry(assets::ASSET_DIR)
                .map_err(|_| AssetError::Storage)?;
            if existing.is_dir() {
                return Ok(existing);
            }
            return Err(AssetError::Storage);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // `new_entry` checked the existing ancestor chain before creating
            // the fixed directory. Revalidate after creation so a concurrent
            // reparse/symlink cannot become the write destination.
            vault.revalidate().map_err(|_| AssetError::Storage)?;
            match fs::create_dir(&candidate) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(AssetError::Storage),
            }
        }
        Err(_) => return Err(AssetError::Storage),
    }

    vault
        .existing_entry(assets::ASSET_DIR)
        .map_err(|_| AssetError::Storage)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishResult {
    Created,
    Reused,
}

fn publish_asset(
    vault: &VaultIdentity,
    relative: &str,
    target: &Path,
    bytes: &[u8],
) -> Result<PublishResult, AssetError> {
    vault.revalidate().map_err(|_| AssetError::Storage)?;
    let expected = vault.new_entry(relative).map_err(|_| AssetError::Storage)?;
    if expected != target {
        return Err(AssetError::Storage);
    }
    match fs::symlink_metadata(target) {
        Ok(_) => return compare_existing(vault, target, bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(AssetError::Storage),
    }

    let (temporary, temporary_identity) =
        stage_asset_file(vault, target, bytes).map_err(|_| AssetError::Storage)?;
    if vault.revalidate().is_err() {
        cleanup_vault_file(vault, &temporary, &temporary_identity);
        return Err(AssetError::Storage);
    }
    let expected = match vault.new_entry(relative) {
        Ok(expected) => expected,
        Err(_) => {
            cleanup_vault_file(vault, &temporary, &temporary_identity);
            return Err(AssetError::Storage);
        }
    };
    if expected != target {
        cleanup_vault_file(vault, &temporary, &temporary_identity);
        return Err(AssetError::Storage);
    }

    match publish_new_vault_file(&temporary, target) {
        Ok(()) => {
            // If the root was replaced while publishing, leave the artifact
            // for bounded reconciliation rather than claiming a successful
            // save. The no-replace primitive has already made visibility
            // atomic at this point.
            vault.revalidate().map_err(|_| AssetError::Storage)?;
            let target_identity = vault
                .existing_file_identity(target)
                .map_err(|_| AssetError::Storage)?;
            if !target_identity.matches(&temporary_identity) {
                return Err(AssetError::Storage);
            }
            Ok(PublishResult::Created)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            cleanup_vault_file(vault, &temporary, &temporary_identity);
            compare_existing(vault, target, bytes)
        }
        Err(_) => {
            // Do not remove target on an ambiguous publication error: it may
            // belong to a competing writer. Only our temporary is eligible
            // for identity-checked cleanup.
            cleanup_vault_file(vault, &temporary, &temporary_identity);
            Err(AssetError::Storage)
        }
    }
}

fn stage_asset_file(
    vault: &VaultIdentity,
    target: &Path,
    bytes: &[u8],
) -> Result<(PathBuf, EntryIdentity), std::io::Error> {
    const MAX_STAGE_ATTEMPTS: u32 = 32;
    let file_name = target
        .file_name()
        .ok_or_else(|| std::io::Error::other("image asset storage"))?
        .to_string_lossy();
    for _ in 0..MAX_STAGE_ATTEMPTS {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary = target.with_file_name(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        vault
            .revalidate()
            .map_err(|_| std::io::Error::other("vault changed"))?;
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(bytes)?;
            file.flush()?;
            file.sync_all()
        })();
        if let Err(error) = result {
            let identity = file
                .metadata()
                .ok()
                .map(|metadata| VaultIdentity::entry_identity_from_metadata(&temporary, &metadata));
            drop(file);
            if let Some(identity) = identity.as_ref() {
                cleanup_vault_file(vault, &temporary, identity);
            }
            return Err(error);
        }
        let identity = file
            .metadata()
            .map(|metadata| VaultIdentity::entry_identity_from_metadata(&temporary, &metadata))?;
        drop(file);
        return Ok((temporary, identity));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many temporary image collisions",
    ))
}

fn compare_existing(
    vault: &VaultIdentity,
    target: &Path,
    bytes: &[u8],
) -> Result<PublishResult, AssetError> {
    let canonical = vault
        .existing_path(target)
        .map_err(|_| AssetError::Storage)?;
    if canonical != target {
        return Err(AssetError::Storage);
    }
    // The metadata snapshot can become stale while another writer replaces or
    // grows the content-addressed target. Keep the comparison bounded even in
    // that race; an attacker-controlled existing file must not turn a dedupe
    // check into an unbounded allocation.
    let file = fs::File::open(&canonical).map_err(|_| AssetError::Storage)?;
    let open_metadata = file.metadata().map_err(|_| AssetError::Storage)?;
    if !open_metadata.is_file() || open_metadata.len() != bytes.len() as u64 {
        return Err(AssetError::Storage);
    }
    let open_identity = VaultIdentity::entry_identity_from_metadata(&canonical, &open_metadata);
    let mut existing = Vec::new();
    file.take((MAX_ASSET_BYTES as u64).saturating_add(1))
        .read_to_end(&mut existing)
        .map_err(|_| AssetError::Storage)?;
    if existing.len() != bytes.len() {
        return Err(AssetError::Storage);
    }
    vault.revalidate().map_err(|_| AssetError::Storage)?;
    let current_identity = vault
        .existing_file_identity(&canonical)
        .map_err(|_| AssetError::Storage)?;
    if !open_identity.matches(&current_identity) {
        return Err(AssetError::Storage);
    }
    if existing == bytes {
        Ok(PublishResult::Reused)
    } else {
        // A different file at a content-addressed name is treated as a
        // collision. Never overwrite it, even if the hash collision is
        // theoretically improbable.
        Err(AssetError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("Notes")).unwrap();
        fs::write(root.path().join("Notes/note.md"), "# note\n").unwrap();
        root
    }

    #[test]
    fn saves_content_addressed_asset_and_builds_note_relative_markdown() {
        let root = fixture();
        let bytes = png(2, 3);

        let saved = save_image_asset_in(root.path(), "Notes/note.md", &bytes).unwrap();

        assert_eq!(
            saved.markdown,
            format!("![image](../{})", saved.relative_path)
        );
        assert!(saved.relative_path.starts_with("assets/"));
        assert!(root.path().join(&saved.relative_path).is_file());
        assert_eq!(
            fs::read(root.path().join(&saved.relative_path)).unwrap(),
            bytes
        );
        assert!(!saved.reused);
    }

    #[test]
    fn same_content_reuses_and_hash_name_never_uses_original_filename() {
        let root = fixture();
        let bytes = png(2, 3);

        let first = save_image_asset_in(root.path(), "Notes/note.md", &bytes).unwrap();
        let second = save_image_asset_in(root.path(), "Notes/note.md", &bytes).unwrap();

        assert_eq!(first.relative_path, second.relative_path);
        assert_eq!(first.markdown, second.markdown);
        assert!(second.reused);
        assert!(!second.relative_path.contains("note.md"));
        assert_eq!(
            fs::read_dir(root.path().join("assets"))
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            1
        );
    }

    #[test]
    fn collision_does_not_overwrite_existing_hash_file_or_change_note() {
        let root = fixture();
        let bytes = png(2, 3);
        let hash = assets::content_hash_hex(&bytes);
        let target = root.path().join("assets").join(format!("{hash}.png"));
        fs::create_dir(root.path().join("assets")).unwrap();
        fs::write(&target, b"collision").unwrap();
        let before = fs::read(root.path().join("Notes/note.md")).unwrap();

        assert!(save_image_asset_in(root.path(), "Notes/note.md", &bytes).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"collision");
        assert_eq!(fs::read(root.path().join("Notes/note.md")).unwrap(), before);
        assert_eq!(
            fs::read_dir(root.path().join("assets"))
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            1
        );
    }

    #[test]
    fn invalid_input_and_unsafe_note_fail_before_assets_or_document_change() {
        let root = fixture();
        let before = fs::read(root.path().join("Notes/note.md")).unwrap();

        for (path, input) in [
            ("../secret.md", png(1, 1)),
            ("Notes/missing.md", png(1, 1)),
            ("Notes/note.md", b"<svg>secret</svg>".to_vec()),
        ] {
            assert!(save_image_asset_in(root.path(), path, &input).is_err());
        }

        assert!(!root.path().join("assets").exists());
        assert_eq!(fs::read(root.path().join("Notes/note.md")).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_assets_symlink_escape_without_writing_outside_vault() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("assets")).unwrap();

        assert!(save_image_asset_in(root.path(), "Notes/note.md", &png(1, 1)).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_stale_vault_before_creating_assets() {
        let root = fixture();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let parent = root.path().parent().unwrap().to_path_buf();
        let root_name = root.path().file_name().unwrap().to_string_lossy();
        let moved = parent.join(format!("knowledge-image-old-{root_name}"));
        fs::rename(root.path(), &moved).unwrap();
        fs::create_dir(root.path()).unwrap();
        fs::create_dir(root.path().join("Notes")).unwrap();
        fs::write(root.path().join("Notes/note.md"), "replacement").unwrap();

        assert!(save_image_asset_at(&vault, "Notes/note.md", &png(1, 1)).is_err());
        assert!(!root.path().join("assets").exists());
        assert!(!moved.join("assets").exists());
    }

    #[test]
    fn command_error_is_fixed_and_does_not_echo_secret_path() {
        let root = fixture();
        let secret = "../private/token-value.md";
        let result = save_image_asset_in(root.path(), secret, &png(1, 1));
        let error = result.unwrap_err().to_string();
        assert_eq!(error, IMAGE_ASSET_ERROR);
        assert!(!error.contains(secret));
    }

    #[test]
    fn decodes_only_canonical_bounded_base64() {
        let encoded = BASE64.encode(png(1, 1));
        assert_eq!(decode_bytes(&encoded).unwrap(), png(1, 1));
        assert!(decode_bytes("").is_err());
        assert!(decode_bytes("not-base64").is_err());
    }

    #[test]
    fn image_request_wire_shape_is_bounded_and_rejects_unknown_fields() {
        let oversized = format!(
            r#"{{"noteRel":"note.md","bytesBase64":"{}"}}"#,
            "A".repeat(MAX_BASE64_BYTES + 1)
        );
        assert!(serde_json::from_str::<SaveImageAssetRequest>(&oversized).is_err());
        assert!(serde_json::from_str::<SaveImageAssetRequest>(
            r#"{"noteRel":"note.md","bytesBase64":"AA==","secret":"value"}"#
        )
        .is_err());
    }
}
