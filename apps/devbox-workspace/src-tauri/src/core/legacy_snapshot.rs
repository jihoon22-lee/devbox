//! Bounded, no-link acquisition of fixed v0.7 JSON sources. A complete snapshot
//! consists of stable reads of every listed file followed by a second pass.
//! This detects concurrent changes; it is not a transaction between legacy apps.
use super::legacy_inventory::{digest, inspect, Inventory, Source};
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
type Result<T> = std::result::Result<T, &'static str>;

struct Directory {
    path: PathBuf,
    identity: FilesystemIdentity,
    _handle: File,
}
impl Directory {
    fn open(path: &Path) -> Result<Self> {
        ensure_no_links(path).map_err(|_| "legacy_path_unavailable")?;
        let (handle, identity) =
            open_filesystem_object(path, true).map_err(|_| "legacy_path_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    fn check(&self) -> Result<()> {
        ensure_no_links(&self.path).map_err(|_| "legacy_source_changed")?;
        if filesystem_identity(&self.path, true).map_err(|_| "legacy_source_changed")?
            != self.identity
        {
            return Err("legacy_source_changed");
        }
        Ok(())
    }
}
#[derive(PartialEq, Eq)]
struct Stamp {
    identity: FilesystemIdentity,
    size: u64,
    modified: SystemTime,
    hash: String,
}
struct Captured {
    name: &'static str,
    limit: usize,
    stamp: Stamp,
    bytes: Vec<u8>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub source: Source,
    pub files: Vec<Inventory>,
    pub missing: Vec<String>,
}
pub struct Snapshot {
    pub manifest: Manifest,
    files: Vec<Captured>,
}
/// Catalog entries describe completion markers only. A caller must load and
/// verify all snapshot bytes before using any record from a listed entry.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub manifest: Option<Manifest>,
    pub issue: Option<&'static str>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub snapshots: Vec<CatalogEntry>,
    pub unrecognized: usize,
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn manifest(root: &Directory, id: &str) -> Result<Manifest> {
    let (_, bytes) = read(root, "snapshot.json", 64 * 1024)?.ok_or("legacy_snapshot_incomplete")?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| "invalid_legacy_snapshot")?;
    let specs = manifest.source.files();
    let names = manifest
        .files
        .iter()
        .map(|file| file.name.as_str())
        .chain(manifest.missing.iter().map(String::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    if manifest.schema_version != 1
        || digest(&serde_json::to_vec(&manifest).map_err(|_| "invalid_legacy_snapshot")?) != id
        || manifest.files.len() + manifest.missing.len() != specs.len()
        || names.len() != specs.len()
        || specs.iter().any(|spec| !names.contains(spec.name))
        || manifest.files.iter().any(|file| {
            !valid_id(&file.sha256)
                || file.records.is_some() != file.issue.is_none()
                || !specs
                    .iter()
                    .any(|spec| spec.name == file.name && file.bytes <= spec.limit)
        })
    {
        return Err("invalid_legacy_snapshot");
    }
    Ok(manifest)
}

fn read(root: &Directory, name: &str, limit: usize) -> Result<Option<(Stamp, Vec<u8>)>> {
    root.check()?;
    let path = root.path.join(name);
    // ensure_no_links also validates an existing parent when the leaf is absent.
    let mut current = root.path.clone();
    for segment in name.split('/') {
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(_) => ensure_no_links(&current).map_err(|_| "legacy_path_unavailable")?,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                root.check()?;
                return Ok(None);
            }
            Err(_) => return Err("legacy_path_unavailable"),
        }
    }
    let (mut handle, identity) =
        open_filesystem_object(&path, false).map_err(|_| "legacy_source_changed")?;
    let before = handle.metadata().map_err(|_| "legacy_source_changed")?;
    if before.len() > limit as u64 {
        return Err("legacy_file_limit");
    }
    let modified = before.modified().map_err(|_| "legacy_source_changed")?;
    let mut bytes = Vec::new();
    (&mut handle)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "legacy_source_changed")?;
    let after = handle.metadata().map_err(|_| "legacy_source_changed")?;
    if bytes.len() > limit
        || bytes.len() as u64 != before.len()
        || after.len() != before.len()
        || after.modified().ok() != Some(modified)
        || filesystem_identity(&path, false).map_err(|_| "legacy_source_changed")? != identity
    {
        return Err("legacy_source_changed");
    }
    root.check()?;
    Ok(Some((
        Stamp {
            identity,
            size: before.len(),
            modified,
            hash: digest(&bytes),
        },
        bytes,
    )))
}
impl Snapshot {
    pub fn catalog(destination: &Path, check: impl Fn() -> Result<()>) -> Result<Catalog> {
        check()?;
        let destination = Directory::open(destination)?;
        let mut result = Catalog::default();
        for (index, entry) in fs::read_dir(&destination.path)
            .map_err(|_| "legacy_snapshot_unavailable")?
            .enumerate()
        {
            check()?;
            if index >= 32 {
                return Err("legacy_snapshot_limit");
            }
            let entry = entry.map_err(|_| "legacy_snapshot_unavailable")?;
            let name = entry.file_name();
            let Some(id) = name.to_str().filter(|id| valid_id(id)) else {
                result.unrecognized += 1;
                continue;
            };
            let described = Self::describe(&destination.path, id);
            let (manifest, issue) = match described {
                Ok(value) => (Some(value), None),
                Err(issue) => (None, Some(issue)),
            };
            result.snapshots.push(CatalogEntry {
                id: id.into(),
                manifest,
                issue,
            });
        }
        result.snapshots.sort_by(|a, b| a.id.cmp(&b.id));
        destination.check()?;
        check()?;
        Ok(result)
    }
    pub fn describe(destination: &Path, id: &str) -> Result<Manifest> {
        if !valid_id(id) {
            return Err("invalid_legacy_snapshot");
        }
        let destination = Directory::open(destination)?;
        let root = Directory::open(&destination.path.join(id))?;
        let manifest = manifest(&root, id)?;
        root.check()?;
        destination.check()?;
        Ok(manifest)
    }
    pub fn load(destination: &Path, id: &str) -> Result<Self> {
        Self::load_checked(destination, id, || Ok(()))
    }
    pub fn load_checked(
        destination: &Path,
        id: &str,
        check: impl Fn() -> Result<()>,
    ) -> Result<Self> {
        check()?;
        if !valid_id(id) {
            return Err("invalid_legacy_snapshot");
        }
        let destination = Directory::open(destination)?;
        let root = Directory::open(&destination.path.join(id))?;
        let manifest = manifest(&root, id)?;
        let mut files = vec![];
        let mut inventories = vec![];
        let mut missing = vec![];
        // Walk the compiled source inventory, never paths from snapshot JSON.
        for spec in manifest.source.files() {
            check()?;
            match read(&root, spec.name, spec.limit)? {
                Some((stamp, bytes)) => {
                    inventories.push(inspect(manifest.source, spec.name, &bytes)?);
                    files.push(Captured {
                        name: spec.name,
                        limit: spec.limit,
                        stamp,
                        bytes,
                    });
                }
                None => missing.push(spec.name.to_owned()),
            }
        }
        if manifest.files != inventories || manifest.missing != missing {
            return Err("legacy_snapshot_changed");
        }
        root.check()?;
        destination.check()?;
        check()?;
        Ok(Self { manifest, files })
    }
    /// `base` is supplied by native startup (or a disposable test fixture),
    /// never by renderer input. Only Source's fixed identifier/files are read.
    pub fn acquire(base: &Path, source: Source, check: impl Fn() -> Result<()>) -> Result<Self> {
        check()?;
        if !base.is_absolute() {
            return Err("legacy_path_unavailable");
        }
        let base = Directory::open(base)?;
        let root_path = base.path.join(source.identifier());
        let mut result = Self {
            manifest: Manifest {
                schema_version: 1,
                source,
                files: vec![],
                missing: vec![],
            },
            files: vec![],
        };
        match fs::symlink_metadata(&root_path) {
            Err(e) if e.kind() == ErrorKind::NotFound => {
                result.manifest.missing = source.files().iter().map(|s| s.name.into()).collect();
                base.check()?;
                check()?;
                return Ok(result);
            }
            Err(_) => return Err("legacy_path_unavailable"),
            Ok(_) => {}
        }
        let root = Directory::open(&root_path)?;
        for spec in source.files() {
            check()?;
            match read(&root, spec.name, spec.limit)? {
                Some((stamp, bytes)) => {
                    result
                        .manifest
                        .files
                        .push(inspect(source, spec.name, &bytes)?);
                    result.files.push(Captured {
                        name: spec.name,
                        limit: spec.limit,
                        stamp,
                        bytes,
                    });
                }
                None => result.manifest.missing.push(spec.name.into()),
            }
        }
        for spec in source.files() {
            check()?;
            let next = read(&root, spec.name, spec.limit)?;
            let old = result.files.iter().find(|file| file.name == spec.name);
            if next.as_ref().map(|(stamp, _)| stamp) != old.map(|file| &file.stamp) {
                return Err("legacy_source_changed");
            }
        }
        root.check()?;
        base.check()?;
        check()?;
        Ok(result)
    }
    pub fn id(&self) -> Result<String> {
        Ok(digest(
            &serde_json::to_vec(&self.manifest).map_err(|_| "invalid_legacy_snapshot")?,
        ))
    }
    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|file| file.name == name)
            .map(|file| file.bytes.as_slice())
    }
    /// Native migration single-writer lock must cover persistence. Incomplete
    /// snapshots stay uncommitted and resume only when every existing byte
    /// matches. Unknown or externally changed files are never overwritten.
    pub fn persist(&self, destination: &Path, check: impl Fn() -> Result<()>) -> Result<String> {
        check()?;
        let destination = Directory::open(destination)?;
        let id = self.id()?;
        let path = destination.path.join(&id);
        let absent = match fs::symlink_metadata(&path) {
            Ok(_) => false,
            Err(error) if error.kind() == ErrorKind::NotFound => true,
            Err(_) => return Err("legacy_snapshot_unavailable"),
        };
        if absent {
            for (index, entry) in fs::read_dir(&destination.path)
                .map_err(|_| "legacy_snapshot_unavailable")?
                .enumerate()
            {
                entry.map_err(|_| "legacy_snapshot_unavailable")?;
                if index >= 31 {
                    return Err("legacy_snapshot_limit");
                }
            }
        }
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err("legacy_snapshot_unavailable"),
        }
        let root = Directory::open(&path)?;
        for captured in &self.files {
            check()?;
            destination.check()?;
            root.check()?;
            let mut parent = path.clone();
            let segments = captured.name.split('/').collect::<Vec<_>>();
            for segment in &segments[..segments.len() - 1] {
                parent.push(segment);
                match fs::create_dir(&parent) {
                    Ok(()) => {}
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                    Err(_) => return Err("legacy_snapshot_unavailable"),
                }
                ensure_no_links(&parent).map_err(|_| "legacy_snapshot_changed")?;
            }
            write_exact(&root, captured.name, &captured.bytes, captured.limit)?;
        }
        // Manifest is the only completion marker, written after byte read-back.
        check()?;
        destination.check()?;
        root.check()?;
        let manifest = serde_json::to_vec(&self.manifest).map_err(|_| "invalid_legacy_snapshot")?;
        write_exact(&root, "snapshot.json", &manifest, 64 * 1024)?;
        destination.check()?;
        root.check()?;
        Ok(id)
    }
}
fn write_exact(root: &Directory, name: &str, bytes: &[u8], limit: usize) -> Result<()> {
    if let Some((_, current)) = read(root, name, limit)? {
        if current != bytes {
            return Err("legacy_snapshot_changed");
        }
        return Ok(());
    }
    root.check()?;
    let target = root.path.join(name);
    let parent = Directory::open(target.parent().ok_or("legacy_snapshot_unavailable")?)?;
    let temporary = parent
        .path
        .join(format!(".snapshot-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "legacy_snapshot_changed")?;
    let temporary_identity =
        filesystem_identity(&temporary, false).map_err(|_| "legacy_snapshot_changed")?;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "legacy_snapshot_unavailable")?;
        root.check()?;
        parent.check()?;
        if filesystem_identity(&temporary, false).ok() != Some(temporary_identity) {
            return Err("legacy_snapshot_changed");
        }
        // A no-clobber hard-link publishes complete bytes atomically. On a
        // crash, an uncommitted unique temporary is preserved; the final name
        // is either absent or complete and can be safely checked on resume.
        fs::hard_link(&temporary, &target).map_err(|_| "legacy_snapshot_changed")
    })();
    drop(file);
    if parent.check().is_ok()
        && filesystem_identity(&temporary, false).ok() == Some(temporary_identity)
    {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    if read(root, name, limit)?.map(|(_, bytes)| bytes).as_deref() != Some(bytes) {
        return Err("legacy_snapshot_changed");
    }
    root.check()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let base = tempfile::tempdir().unwrap();
        let source = base.path().join(Source::Workbench.identifier());
        fs::create_dir(&source).unwrap();
        let destination = base.path().join("workspace-snapshots");
        fs::create_dir(&destination).unwrap();
        fs::write(
            source.join("project-profiles.json"),
            br#"{"version":1,"profiles":[]}"#,
        )
        .unwrap();
        fs::write(
            source.join("profile-templates.json"),
            br#"{"version":1,"templates":[]}"#,
        )
        .unwrap();
        (base, source, destination)
    }
    #[test]
    fn stable_snapshot_repeats_without_changing_source_or_importing_invalid_data() {
        let (base, source, destination) = fixture();
        let bytes = b"{future-or-broken";
        fs::write(source.join("profile-templates.json"), bytes).unwrap();
        let snapshot = Snapshot::acquire(base.path(), Source::Workbench, || Ok(())).unwrap();
        assert_eq!(snapshot.manifest.files[1].records, None);
        let id = snapshot.persist(&destination, || Ok(())).unwrap();
        assert_eq!(snapshot.persist(&destination, || Ok(())).unwrap(), id);
        let loaded = Snapshot::load(&destination, &id).unwrap();
        assert_eq!(loaded.manifest, snapshot.manifest);
        assert_eq!(loaded.bytes("profile-templates.json").unwrap(), bytes);
        assert_eq!(
            fs::read(source.join("profile-templates.json")).unwrap(),
            bytes
        );
        assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
    }
    #[test]
    fn changed_source_between_files_never_produces_a_complete_snapshot() {
        let (base, source, _) = fixture();
        let calls = Cell::new(0);
        let result = Snapshot::acquire(base.path(), Source::Workbench, || {
            calls.set(calls.get() + 1);
            if calls.get() == 4 {
                fs::write(source.join("project-profiles.json"), "external edit").unwrap();
            }
            Ok(())
        });
        assert!(matches!(result, Err("legacy_source_changed")));
        assert_eq!(
            fs::read_to_string(source.join("project-profiles.json")).unwrap(),
            "external edit"
        );
    }
    #[test]
    fn cancel_before_commit_retains_partial_bytes_and_resumes_once() {
        let (base, _, destination) = fixture();
        let snapshot = Snapshot::acquire(base.path(), Source::Workbench, || Ok(())).unwrap();
        let calls = Cell::new(0);
        assert_eq!(
            snapshot.persist(&destination, || {
                calls.set(calls.get() + 1);
                if calls.get() == 3 {
                    Err("cancelled")
                } else {
                    Ok(())
                }
            }),
            Err("cancelled")
        );
        let id = snapshot.id().unwrap();
        assert!(matches!(
            Snapshot::load(&destination, &id),
            Err("legacy_snapshot_incomplete")
        ));
        assert_eq!(snapshot.persist(&destination, || Ok(())).unwrap(), id);
        assert_eq!(
            Snapshot::load(&destination, &id).unwrap().manifest,
            snapshot.manifest
        );
        fs::write(
            destination.join(&id).join("project-profiles.json"),
            "external edit",
        )
        .unwrap();
        assert!(snapshot.persist(&destination, || Ok(())).is_err());
        assert!(Snapshot::load(&destination, &id).is_err());
        assert_eq!(
            fs::read_to_string(destination.join(id).join("project-profiles.json")).unwrap(),
            "external edit"
        );
    }
    #[test]
    fn catalog_preserves_incomplete_unknown_and_changed_snapshots_without_claiming_integrity() {
        let (base, _, destination) = fixture();
        let snapshot = Snapshot::acquire(base.path(), Source::Workbench, || Ok(())).unwrap();
        let id = snapshot.persist(&destination, || Ok(())).unwrap();
        fs::create_dir(destination.join("f".repeat(64))).unwrap();
        fs::create_dir(destination.join("unknown-retained-folder")).unwrap();
        let catalog = Snapshot::catalog(&destination, || Ok(())).unwrap();
        assert_eq!(catalog.unrecognized, 1);
        assert_eq!(catalog.snapshots.len(), 2);
        assert_eq!(
            catalog
                .snapshots
                .iter()
                .find(|entry| entry.id == id)
                .unwrap()
                .manifest
                .as_ref(),
            Some(&snapshot.manifest)
        );
        assert_eq!(
            catalog
                .snapshots
                .iter()
                .find(|entry| entry.id != id)
                .unwrap()
                .issue,
            Some("legacy_snapshot_incomplete")
        );
        fs::write(
            destination.join(&id).join("project-profiles.json"),
            "changed after listing",
        )
        .unwrap();
        // A valid completion marker is not a verification of all saved files.
        assert!(Snapshot::describe(&destination, &id).is_ok());
        assert!(matches!(
            Snapshot::load(&destination, &id),
            Err("legacy_snapshot_changed")
        ));
        assert!(matches!(
            Snapshot::load_checked(&destination, &id, || Err("cancelled")),
            Err("cancelled")
        ));
        assert!(matches!(
            Snapshot::describe(&destination, "../foreign"),
            Err("invalid_legacy_snapshot")
        ));
        assert!(destination.join("unknown-retained-folder").is_dir());
    }
    #[test]
    #[cfg(unix)]
    fn catalog_never_follows_foreign_links_and_counts_them_toward_the_bound() {
        let (base, source, destination) = fixture();
        let id = "a".repeat(64);
        std::os::unix::fs::symlink(&source, destination.join(&id)).unwrap();
        let catalog = Snapshot::catalog(&destination, || Ok(())).unwrap();
        assert_eq!(catalog.snapshots[0].issue, Some("legacy_path_unavailable"));
        assert!(!source.join("snapshot.json").exists());
        for index in 0..32 {
            fs::create_dir(destination.join(format!("retained-{index}"))).unwrap();
        }
        assert!(matches!(
            Snapshot::catalog(&destination, || Ok(())),
            Err("legacy_snapshot_limit")
        ));
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 33);
        assert!(base.path().exists());
    }
    #[test]
    #[cfg(unix)]
    fn dangling_destination_is_an_existing_link_not_a_new_snapshot() {
        let (base, _, destination) = fixture();
        let snapshot = Snapshot::acquire(base.path(), Source::Workbench, || Ok(())).unwrap();
        for index in 0..31 {
            fs::create_dir(destination.join(format!("retained-{index}"))).unwrap();
        }
        std::os::unix::fs::symlink(
            base.path().join("absent"),
            destination.join(snapshot.id().unwrap()),
        )
        .unwrap();
        assert_eq!(
            snapshot.persist(&destination, || Ok(())),
            Err("legacy_path_unavailable")
        );
        assert!(!base.path().join("absent").exists());
    }
    #[test]
    #[cfg(unix)]
    fn linked_sources_and_destination_objects_are_rejected() {
        let (base, source, destination) = fixture();
        let bytes = source.join("project-profiles.json");
        let moved = source.join("retained.json");
        fs::rename(&bytes, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &bytes).unwrap();
        assert!(Snapshot::acquire(base.path(), Source::Workbench, || Ok(())).is_err());
        fs::remove_file(&bytes).unwrap();
        fs::rename(moved, &bytes).unwrap();
        let snapshot = Snapshot::acquire(base.path(), Source::Workbench, || Ok(())).unwrap();
        let id = snapshot.id().unwrap();
        std::os::unix::fs::symlink(&source, destination.join(id)).unwrap();
        assert!(snapshot.persist(&destination, || Ok(())).is_err());
        assert!(!source.join("snapshot.json").exists());
    }
}
