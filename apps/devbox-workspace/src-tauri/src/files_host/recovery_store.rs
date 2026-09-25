//! Current editor recovery persistence and compare-and-swap revision.
use super::*;
use crate::core::editor_recovery::StoredRecovery;
pub(super) fn read_recovery(view: &MetadataRoot) -> Result<(StoredRecovery, String)> {
    let bytes = view.read("recovery.json")?;
    let revision = recovery_revision(view, bytes.as_deref());
    Ok((
        bytes
            .as_deref()
            .map(StoredRecovery::decode)
            .transpose()?
            .unwrap_or_default(),
        revision,
    ))
}
fn recovery_revision(view: &MetadataRoot, bytes: Option<&[u8]>) -> String {
    let mut material = b"recovery-v1\0".to_vec();
    material.push(u8::from(bytes.is_some()));
    if let Some(bytes) = bytes {
        material.extend_from_slice(bytes);
    }
    session_revision(view, Some(&material))
}
pub(super) fn write_recovery(
    view: &MetadataRoot,
    next: &StoredRecovery,
    revision: &str,
) -> Result<String> {
    let bytes = next.encode()?;
    let before = view.read("recovery.json")?;
    if recovery_revision(view, before.as_deref()) != revision {
        return Err("files_recovery_changed");
    }
    view.write("recovery.json", &bytes)?;
    Ok(recovery_revision(view, Some(&bytes)))
}
