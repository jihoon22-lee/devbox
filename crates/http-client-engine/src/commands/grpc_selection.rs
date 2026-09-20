//! Opaque, expiring native selections shared by gRPC schema and TLS commands.

use crate::core::grpc;
use devbox_filesystem::{ensure_no_links, filesystem_identity, FilesystemIdentity};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri_plugin_dialog::DialogExt;

const MAX_SELECTIONS: usize = 32;
const MAX_LABEL_BYTES: usize = 256;
const SELECTION_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum GrpcSelectionKind {
    Proto,
    ImportRoot,
    Ca,
    ClientCertificate,
    ClientKey,
}

impl GrpcSelectionKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Proto => "proto",
            Self::ImportRoot => "import-root",
            Self::Ca => "ca",
            Self::ClientCertificate => "client-cert",
            Self::ClientKey => "client-key",
        }
    }

    fn directory(self) -> bool {
        self == Self::ImportRoot
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Proto => &["proto"],
            Self::ImportRoot => &[],
            Self::Ca | Self::ClientCertificate => &["pem", "crt", "cer"],
            Self::ClientKey => &["pem", "key"],
        }
    }
}

#[derive(Debug, Clone)]
struct StoredGrpcSelection {
    kind: GrpcSelectionKind,
    label: String,
    canonical: PathBuf,
    identity: FilesystemIdentity,
    default_import_root: Option<(PathBuf, FilesystemIdentity)>,
    expires_at: Instant,
    lease: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReviewedGrpcSelection {
    pub(crate) label: String,
    pub(crate) canonical: PathBuf,
    pub(crate) identity: FilesystemIdentity,
    pub(crate) default_import_root: Option<(PathBuf, FilesystemIdentity)>,
}

pub(crate) struct GrpcSelectionClaim<'a> {
    state: &'a GrpcSelectionState,
    lease_id: String,
    selection_ids: Vec<String>,
    finished: bool,
}

impl GrpcSelectionClaim<'_> {
    pub(crate) fn finish(mut self, consume: bool) -> Result<(), &'static str> {
        let outcome = self
            .state
            .finish_claim(&self.lease_id, &self.selection_ids, consume);
        if outcome.is_ok() {
            self.finished = true;
        }
        outcome
    }
}

impl Drop for GrpcSelectionClaim<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self
                .state
                .finish_claim(&self.lease_id, &self.selection_ids, false);
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcNativeSelection {
    selection_id: String,
    kind: &'static str,
    label: String,
    expires_at_ms: u64,
}

#[derive(Default)]
pub struct GrpcSelectionState {
    inner: Mutex<HashMap<String, StoredGrpcSelection>>,
}

impl GrpcSelectionState {
    fn store(
        &self,
        selection: StoredGrpcSelection,
        label: String,
    ) -> Result<GrpcNativeSelection, &'static str> {
        let now = Instant::now();
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
        inner.retain(|_, stored| stored.expires_at > now || stored.lease.is_some());
        if inner.len() >= MAX_SELECTIONS {
            return Err(grpc::SOURCE_SELECTION_INVALID);
        }
        let remaining = selection.expires_at.saturating_duration_since(now);
        let remaining_ms =
            u64::try_from(remaining.as_millis()).map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
        let expires_at_ms = now_unix_ms()
            .and_then(|value| value.checked_add(remaining_ms))
            .ok_or(grpc::SOURCE_SELECTION_INVALID)?;
        for _ in 0..4 {
            let id = random_hex_128()?;
            if !inner.contains_key(&id) {
                let kind = selection.kind.as_str();
                inner.insert(id.clone(), selection);
                return Ok(GrpcNativeSelection {
                    selection_id: id,
                    kind,
                    label,
                    expires_at_ms,
                });
            }
        }
        Err(grpc::SOURCE_SELECTION_INVALID)
    }

    pub(crate) fn review(
        &self,
        selection_id: &str,
        expected: GrpcSelectionKind,
    ) -> Result<ReviewedGrpcSelection, &'static str> {
        validate_opaque_id(selection_id)?;
        let now = Instant::now();
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
        inner.retain(|_, stored| stored.expires_at > now || stored.lease.is_some());
        let stored = inner
            .get(selection_id)
            .filter(|stored| stored.kind == expected && stored.lease.is_none())
            .ok_or(grpc::SOURCE_SELECTION_INVALID)?;
        revalidate(stored)
    }

    pub(crate) fn claim_many(
        &self,
        selections: &[(String, GrpcSelectionKind)],
    ) -> Result<GrpcSelectionClaim<'_>, &'static str> {
        let lease_id = random_hex_128()?;
        let now = Instant::now();
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
        inner.retain(|_, stored| stored.expires_at > now || stored.lease.is_some());
        let mut unique = std::collections::BTreeSet::new();
        for (selection_id, expected) in selections {
            validate_opaque_id(selection_id)?;
            if !unique.insert(selection_id.clone())
                || !inner
                    .get(selection_id)
                    .is_some_and(|stored| stored.kind == *expected && stored.lease.is_none())
            {
                return Err(grpc::SOURCE_SELECTION_INVALID);
            }
        }
        for (selection_id, _) in selections {
            let stored = inner
                .get_mut(selection_id)
                .ok_or(grpc::SOURCE_SELECTION_INVALID)?;
            stored.lease = Some(lease_id.clone());
        }
        Ok(GrpcSelectionClaim {
            state: self,
            lease_id,
            selection_ids: selections
                .iter()
                .map(|(selection_id, _)| selection_id.clone())
                .collect(),
            finished: false,
        })
    }

    fn finish_claim(
        &self,
        lease_id: &str,
        selection_ids: &[String],
        consume: bool,
    ) -> Result<(), &'static str> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
        if selection_ids.iter().any(|selection_id| {
            !inner
                .get(selection_id)
                .is_some_and(|stored| stored.lease.as_deref() == Some(lease_id))
        }) {
            return Err(grpc::SOURCE_SELECTION_INVALID);
        }
        for selection_id in selection_ids {
            if consume {
                inner.remove(selection_id);
            } else if let Some(stored) = inner.get_mut(selection_id) {
                stored.lease = None;
            }
        }
        Ok(())
    }
}

pub(crate) async fn pick_grpc_selection(
    app: tauri::AppHandle,
    state: &GrpcSelectionState,
    kind: GrpcSelectionKind,
) -> Result<Option<GrpcNativeSelection>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let builder = app.dialog().file();
        if kind.directory() {
            builder.blocking_pick_folder()
        } else {
            builder
                .add_filter("gRPC source or credential", kind.extensions())
                .blocking_pick_file()
        }
    })
    .await
    .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    let (stored, label) =
        tauri::async_runtime::spawn_blocking(move || build_selection(&path, kind))
            .await
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())??;
    state
        .store(stored, label)
        .map(Some)
        .map_err(ToOwned::to_owned)
}

fn build_selection(
    path: &Path,
    kind: GrpcSelectionKind,
) -> Result<(StoredGrpcSelection, String), String> {
    ensure_no_links(path).map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    let identity = filesystem_identity(path, kind.directory())
        .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    let canonical = path
        .canonicalize()
        .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    ensure_no_links(&canonical).map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    if filesystem_identity(&canonical, kind.directory())
        .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?
        != identity
    {
        return Err(grpc::SOURCE_SELECTION_INVALID.into());
    }
    validate_extension(&canonical, kind)?;
    let default_import_root = if kind == GrpcSelectionKind::Proto {
        let parent = canonical
            .parent()
            .ok_or_else(|| grpc::SOURCE_SELECTION_INVALID.to_string())?
            .to_path_buf();
        ensure_no_links(&parent).map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?;
        Some((
            parent.clone(),
            filesystem_identity(&parent, true)
                .map_err(|_| grpc::SOURCE_SELECTION_INVALID.to_string())?,
        ))
    } else {
        None
    };
    let label = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= MAX_LABEL_BYTES
                && !matches!(*value, "." | "..")
                && !value.chars().any(char::is_control)
                && !value.contains(['/', '\\'])
        })
        .ok_or_else(|| grpc::SOURCE_SELECTION_INVALID.to_string())?
        .to_string();
    let expires_at = Instant::now()
        .checked_add(SELECTION_TTL)
        .ok_or_else(|| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    Ok((
        StoredGrpcSelection {
            kind,
            label: label.clone(),
            canonical,
            identity,
            default_import_root,
            expires_at,
            lease: None,
        },
        label,
    ))
}

fn validate_extension(path: &Path, kind: GrpcSelectionKind) -> Result<(), String> {
    if kind.directory() {
        return Ok(());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| grpc::SOURCE_SELECTION_INVALID.to_string())?;
    if kind
        .extensions()
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    {
        Ok(())
    } else {
        Err(grpc::SOURCE_SELECTION_INVALID.into())
    }
}

fn revalidate(stored: &StoredGrpcSelection) -> Result<ReviewedGrpcSelection, &'static str> {
    ensure_no_links(&stored.canonical).map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
    if stored
        .canonical
        .canonicalize()
        .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?
        != stored.canonical
        || filesystem_identity(&stored.canonical, stored.kind.directory())
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?
            != stored.identity
    {
        return Err(grpc::SOURCE_SELECTION_INVALID);
    }
    if let Some((root, identity)) = &stored.default_import_root {
        ensure_no_links(root).map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
        if root
            .canonicalize()
            .map_err(|_| grpc::SOURCE_SELECTION_INVALID)?
            != *root
            || filesystem_identity(root, true).map_err(|_| grpc::SOURCE_SELECTION_INVALID)?
                != *identity
        {
            return Err(grpc::SOURCE_SELECTION_INVALID);
        }
    }
    Ok(ReviewedGrpcSelection {
        label: stored.label.clone(),
        canonical: stored.canonical.clone(),
        identity: stored.identity,
        default_import_root: stored.default_import_root.clone(),
    })
}

pub(crate) fn validate_opaque_id(value: &str) -> Result<(), &'static str> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(grpc::SOURCE_SELECTION_INVALID)
    }
}

pub(crate) fn random_hex_128() -> Result<String, &'static str> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
    let mut output = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").map_err(|_| grpc::SOURCE_SELECTION_INVALID)?;
    }
    Ok(output)
}

fn now_unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn proto_selection_projects_only_safe_label_and_revalidates_default_root() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("echo.proto");
        std::fs::write(&file, "syntax = \"proto3\";").unwrap();
        let (stored, label) = build_selection(&file, GrpcSelectionKind::Proto).unwrap();
        assert_eq!(label, "echo.proto");
        let state = GrpcSelectionState::default();
        let projection = state.store(stored, label).unwrap();
        assert_eq!(projection.kind, "proto");
        assert!(!projection.selection_id.contains('/'));
        let reviewed = state
            .review(&projection.selection_id, GrpcSelectionKind::Proto)
            .unwrap();
        assert_eq!(reviewed.canonical, file.canonicalize().unwrap());
        assert!(reviewed.default_import_root.is_some());
    }

    #[test]
    fn selection_claim_is_exclusive_and_can_release_or_consume() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("echo.proto");
        std::fs::write(&file, "syntax = \"proto3\";").unwrap();
        let state = GrpcSelectionState::default();
        let (stored, label) = build_selection(&file, GrpcSelectionKind::Proto).unwrap();
        let projection = state.store(stored, label).unwrap();
        let requested = vec![(projection.selection_id.clone(), GrpcSelectionKind::Proto)];
        let claim = state.claim_many(&requested).unwrap();
        assert_eq!(
            state
                .review(&projection.selection_id, GrpcSelectionKind::Proto)
                .unwrap_err(),
            grpc::SOURCE_SELECTION_INVALID
        );
        claim.finish(false).unwrap();
        assert!(state
            .review(&projection.selection_id, GrpcSelectionKind::Proto)
            .is_ok());
        let claim = state.claim_many(&requested).unwrap();
        claim.finish(true).unwrap();
        assert_eq!(
            state
                .review(&projection.selection_id, GrpcSelectionKind::Proto)
                .unwrap_err(),
            grpc::SOURCE_SELECTION_INVALID
        );
    }

    #[test]
    fn expired_selection_is_retained_until_its_claim_finishes() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("echo.proto");
        std::fs::write(&file, "syntax = \"proto3\";").unwrap();
        let state = GrpcSelectionState::default();
        let (stored, label) = build_selection(&file, GrpcSelectionKind::Proto).unwrap();
        let projection = state.store(stored, label).unwrap();
        let requested = vec![(projection.selection_id.clone(), GrpcSelectionKind::Proto)];
        let claim = state.claim_many(&requested).unwrap();
        state
            .inner
            .lock()
            .unwrap()
            .get_mut(&projection.selection_id)
            .unwrap()
            .expires_at = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();

        assert_eq!(
            state
                .review(&projection.selection_id, GrpcSelectionKind::Proto)
                .unwrap_err(),
            grpc::SOURCE_SELECTION_INVALID
        );
        claim.finish(true).unwrap();
        assert!(!state
            .inner
            .lock()
            .unwrap()
            .contains_key(&projection.selection_id));
    }

    #[test]
    fn dropping_claim_releases_selection_after_cancelled_work() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("echo.proto");
        std::fs::write(&file, "syntax = \"proto3\";").unwrap();
        let state = GrpcSelectionState::default();
        let (stored, label) = build_selection(&file, GrpcSelectionKind::Proto).unwrap();
        let projection = state.store(stored, label).unwrap();
        let requested = vec![(projection.selection_id.clone(), GrpcSelectionKind::Proto)];

        drop(state.claim_many(&requested).unwrap());

        assert!(state
            .review(&projection.selection_id, GrpcSelectionKind::Proto)
            .is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn selection_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let file = temp.path().join("real.proto");
        let link = temp.path().join("link.proto");
        std::fs::write(&file, "syntax = \"proto3\";").unwrap();
        symlink(&file, &link).unwrap();
        assert_eq!(
            build_selection(&link, GrpcSelectionKind::Proto).unwrap_err(),
            grpc::SOURCE_SELECTION_INVALID
        );
    }
}
