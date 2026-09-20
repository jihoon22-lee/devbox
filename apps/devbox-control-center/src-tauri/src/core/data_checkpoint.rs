//! Complete copies of quiesced product namespaces, including SQLite WAL and
//! closed WebView files. The native caller retains the exclusive suite writer
//! gate throughout acquisition. This never snapshots external vault/repository
//! paths referenced by preferences, and is not a live SQLite backup API.
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_FILES: usize = 50_000;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
/// Metadata-only room estimate under the same closed namespace boundary used
/// for capture. The actual copy still detects concurrent changes and I/O errors.
pub fn required_space(sources: &BTreeMap<String, PathBuf>) -> Result<u64> {
    let mut total = 0u64;
    let mut entries = 0usize;
    for root in sources.values() {
        match fs::symlink_metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("checkpoint_source_unavailable"),
            Ok(_) => {}
        }
        let (directories, files) = listing(root)?;
        entries += directories.len() + files.len();
        if entries > MAX_FILES {
            return Err("checkpoint_file_limit");
        }
        for name in files {
            total = total
                .checked_add(
                    fs::metadata(root.join(name))
                        .map_err(|_| "checkpoint_source_unavailable")?
                        .len(),
                )
                .filter(|value| *value <= MAX_BYTES)
                .ok_or("checkpoint_size_limit")?;
        }
    }
    Ok(total)
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub relative: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Product {
    pub owner: String,
    pub present: bool,
    pub directories: Vec<String>,
    pub files: Vec<Entry>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub installation_key: String,
    pub generation: String,
    pub id: String,
    pub acquisition: String,
    pub products: Vec<Product>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub id: String,
    pub revision: String,
    pub bytes: u64,
    pub files: usize,
}
struct Directory {
    path: PathBuf,
    identity: FilesystemIdentity,
    _handle: File,
}
impl Directory {
    fn open(path: &Path) -> Result<Self> {
        ensure_no_links(path).map_err(|_| "checkpoint_path_unsafe")?;
        #[cfg(windows)]
        let (_handle, identity) = {
            use std::os::windows::fs::OpenOptionsExt;
            // Retain directory identity: no other process can rename/delete it
            // while the checkpoint uses path-based children beneath this handle.
            let file = OpenOptions::new()
                .access_mode(0x80)
                .share_mode(3)
                .custom_flags(0x0220_0000)
                .open(path)
                .map_err(|_| "checkpoint_source_unavailable")?;
            let identity = devbox_filesystem::opened_filesystem_identity(&file, true)
                .map_err(|_| "checkpoint_source_unavailable")?;
            (file, identity)
        };
        #[cfg(not(windows))]
        let (_handle, identity) =
            open_filesystem_object(path, true).map_err(|_| "checkpoint_source_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle,
        })
    }
    fn check(&self) -> Result<()> {
        ensure_no_links(&self.path).map_err(|_| "checkpoint_source_changed")?;
        if filesystem_identity(&self.path, true).map_err(|_| "checkpoint_source_changed")?
            != self.identity
        {
            return Err("checkpoint_source_changed");
        }
        Ok(())
    }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn bounded(started: Instant, cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Err("checkpoint_cancelled");
    }
    if started.elapsed() > Duration::from_secs(120) {
        return Err("checkpoint_expired");
    }
    Ok(())
}
fn listing(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let root_handle = Directory::open(root)?;
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let directory = Directory::open(&path)?;
        for entry in fs::read_dir(&path).map_err(|_| "checkpoint_source_unavailable")? {
            let path = entry.map_err(|_| "checkpoint_source_unavailable")?.path();
            ensure_no_links(&path).map_err(|_| "checkpoint_path_unsafe")?;
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| "checkpoint_source_unavailable")?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "checkpoint_path_unsafe")?
                .to_str()
                .ok_or("checkpoint_path_unsafe")?
                .replace('\\', "/");
            // Windows disallows ':' in names; reject its alternate stream syntax
            // even in cross-platform fixtures. No path can escape a copied root.
            if relative.len() > 4096
                || relative.split('/').any(|part| {
                    part.is_empty() || part == "." || part == ".." || part.contains(':')
                })
            {
                return Err("checkpoint_path_unsafe");
            }
            if metadata.is_dir() {
                directories.push(relative);
                pending.push(path);
            } else if metadata.is_file() {
                files.push(relative);
            } else {
                return Err("checkpoint_path_unsafe");
            }
            if directories.len() + files.len() > MAX_FILES {
                return Err("checkpoint_file_limit");
            }
        }
        directory.check()?;
    }
    root_handle.check()?;
    directories.sort();
    files.sort();
    Ok((directories, files))
}
fn copy_or_hash(
    source: &Path,
    destination: Option<&Path>,
    started: Instant,
    cancelled: &AtomicBool,
) -> Result<(Entry, FilesystemIdentity)> {
    bounded(started, cancelled)?;
    ensure_no_links(source).map_err(|_| "checkpoint_path_unsafe")?;
    let (mut input, identity) =
        open_filesystem_object(source, false).map_err(|_| "checkpoint_source_unavailable")?;
    let before = input
        .metadata()
        .map_err(|_| "checkpoint_source_unavailable")?;
    if before.len() > MAX_BYTES {
        return Err("checkpoint_size_limit");
    }
    let mut output = destination
        .map(|path| {
            ensure_no_links(path.parent().ok_or("checkpoint_path_unsafe")?)
                .map_err(|_| "checkpoint_path_unsafe")?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| "checkpoint_copy_failed")
        })
        .transpose()?;
    let mut bytes = 0_u64;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        bounded(started, cancelled)?;
        let count = input
            .read(&mut buffer)
            .map_err(|_| "checkpoint_source_unavailable")?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_BYTES {
            return Err("checkpoint_size_limit");
        }
        digest.update(&buffer[..count]);
        if let Some(output) = &mut output {
            output
                .write_all(&buffer[..count])
                .map_err(|_| "checkpoint_copy_failed")?;
        }
    }
    let after = input.metadata().map_err(|_| "checkpoint_source_changed")?;
    if bytes != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || filesystem_identity(source, false).map_err(|_| "checkpoint_source_changed")? != identity
    {
        return Err("checkpoint_source_changed");
    }
    if let Some(output) = output {
        output.sync_all().map_err(|_| "checkpoint_copy_failed")?;
    }
    Ok((
        Entry {
            relative: String::new(),
            bytes,
            sha256: digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        },
        identity,
    ))
}
/// Caller derives every source from this installation's native identifiers and
/// owns the exclusive suite-writers gate. A failure leaves an unaccepted partial
/// directory; no completion marker is published, no source file is removed.
pub fn acquire_quiesced(
    sources: &BTreeMap<String, PathBuf>,
    parent: &Path,
    installation_key: &str,
    generation: &str,
    cancelled: &AtomicBool,
) -> Result<Receipt> {
    if sources.keys().map(String::as_str).collect::<Vec<_>>()
        != product_contract::installation::PRODUCTS
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        || !product_contract::commands::revision(installation_key)
        || !product_contract::commands::opaque_id(generation)
    {
        return Err("checkpoint_owner_invalid");
    }
    let parent_handle = Directory::open(parent)?;
    let id = uuid::Uuid::new_v4().to_string();
    let target = parent.join(&id);
    fs::create_dir(&target).map_err(|_| "checkpoint_destination_exists")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
            .map_err(|_| "checkpoint_copy_failed")?;
    }
    let target_handle = Directory::open(&target)?;
    let started = Instant::now();
    let mut products = Vec::new();
    let mut original = BTreeMap::new();
    let mut retained = Vec::new();
    let mut total = 0_u64;
    let mut file_count = 0;
    for (owner, source) in sources {
        bounded(started, cancelled)?;
        if source.starts_with(parent) || parent.starts_with(source) {
            return Err("checkpoint_path_overlap");
        }
        match fs::symlink_metadata(source) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                products.push(Product {
                    owner: owner.clone(),
                    present: false,
                    directories: vec![],
                    files: vec![],
                });
                continue;
            }
            Err(_) => return Err("checkpoint_source_unavailable"),
            Ok(_) => {}
        }
        retained.push(Directory::open(source)?);
        let (directories, files) = listing(source)?;
        file_count += files.len();
        if file_count > MAX_FILES {
            return Err("checkpoint_file_limit");
        }
        for relative in &directories {
            retained.push(Directory::open(&source.join(relative))?);
        }
        let destination = target.join(owner);
        fs::create_dir(&destination).map_err(|_| "checkpoint_copy_failed")?;
        retained.push(Directory::open(&destination)?);
        for relative in &directories {
            let directory = destination.join(relative);
            fs::create_dir(&directory).map_err(|_| "checkpoint_copy_failed")?;
            retained.push(Directory::open(&directory)?);
        }
        let mut entries = Vec::new();
        for relative in files {
            let (mut entry, identity) = copy_or_hash(
                &source.join(&relative),
                Some(&destination.join(&relative)),
                started,
                cancelled,
            )?;
            total += entry.bytes;
            if total > MAX_BYTES {
                return Err("checkpoint_size_limit");
            }
            original.insert((owner.clone(), relative.clone()), identity);
            entry.relative = relative;
            entries.push(entry);
        }
        products.push(Product {
            owner: owner.clone(),
            present: true,
            directories,
            files: entries,
        });
    }
    // The retained writer gate is the consistency boundary. This second pass
    // additionally rejects changed source identities, bytes and directory sets.
    for product in &products {
        let source = &sources[&product.owner];
        if !product.present {
            if !matches!(fs::symlink_metadata(source), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("checkpoint_source_changed");
            }
            continue;
        }
        let (directories, files) = listing(source)?;
        if directories != product.directories
            || files
                != product
                    .files
                    .iter()
                    .map(|entry| entry.relative.clone())
                    .collect::<Vec<_>>()
        {
            return Err("checkpoint_source_changed");
        }
        for expected in &product.files {
            let (mut actual, identity) =
                copy_or_hash(&source.join(&expected.relative), None, started, cancelled)?;
            actual.relative = expected.relative.clone();
            if &actual != expected
                || original[&(product.owner.clone(), expected.relative.clone())] != identity
            {
                return Err("checkpoint_source_changed");
            }
            let (mut copy, _) = copy_or_hash(
                &target.join(&product.owner).join(&expected.relative),
                None,
                started,
                cancelled,
            )?;
            copy.relative = expected.relative.clone();
            if &copy != expected {
                return Err("checkpoint_copy_changed");
            }
        }
    }
    for directory in retained {
        directory.check()?;
    }
    parent_handle.check()?;
    target_handle.check()?;
    bounded(started, cancelled)?;
    let manifest = Manifest {
        schema_version: 1,
        installation_key: installation_key.into(),
        generation: generation.into(),
        id: id.clone(),
        acquisition: "quiesced-product-copy/v1".into(),
        products,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|_| "checkpoint_manifest_invalid")?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err("checkpoint_manifest_limit");
    }
    let pending = target.join("checkpoint.pending");
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "checkpoint_publish_failed")?;
    marker
        .write_all(&bytes)
        .and_then(|()| marker.sync_all())
        .map_err(|_| "checkpoint_publish_failed")?;
    drop(marker);
    target_handle.check()?;
    fs::hard_link(&pending, target.join("checkpoint.json"))
        .map_err(|_| "checkpoint_publish_failed")?;
    // Removing only our pending name never removes the accepted hardlink.
    let _ = fs::remove_file(pending);
    Ok(Receipt {
        id,
        revision: hash(&bytes),
        bytes: total,
        files: file_count,
    })
}

/// Verify a journal-selected checkpoint before any future restore review. Paths
/// and contents stay private; only the native journal digest selects the record.
pub fn verify(
    parent: &Path,
    expected: &Receipt,
    installation_key: &str,
    generation: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    verify_manifest(parent, expected, installation_key, generation, cancelled).map(|_| ())
}

fn verify_manifest(
    parent: &Path,
    expected: &Receipt,
    installation_key: &str,
    generation: &str,
    cancelled: &AtomicBool,
) -> Result<Manifest> {
    if !uuid::Uuid::parse_str(&expected.id).is_ok_and(|id| id.to_string() == expected.id)
        || !product_contract::commands::revision(&expected.revision)
    {
        return Err("checkpoint_manifest_invalid");
    }
    let parent_handle = Directory::open(parent)?;
    let target = parent.join(&expected.id);
    let target_handle = Directory::open(&target)?;
    let marker_path = target.join("checkpoint.json");
    ensure_no_links(&marker_path).map_err(|_| "checkpoint_path_unsafe")?;
    let (marker, identity) =
        open_filesystem_object(&marker_path, false).map_err(|_| "checkpoint_manifest_missing")?;
    let mut bytes = Vec::new();
    marker
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "checkpoint_manifest_invalid")?;
    if bytes.len() as u64 > MAX_MANIFEST
        || hash(&bytes) != expected.revision
        || filesystem_identity(&marker_path, false).map_err(|_| "checkpoint_manifest_changed")?
            != identity
    {
        return Err("checkpoint_manifest_changed");
    }
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| "checkpoint_manifest_invalid")?;
    if manifest.schema_version != 1
        || manifest.id != expected.id
        || manifest.installation_key != installation_key
        || manifest.generation != generation
        || manifest.acquisition != "quiesced-product-copy/v1"
        || manifest.products.len() != 4
    {
        return Err("checkpoint_owner_invalid");
    }
    let mut owners = std::collections::BTreeSet::new();
    let started = Instant::now();
    let mut total = 0_u64;
    let mut count = 0;
    let mut retained = Vec::new();
    for product in &manifest.products {
        if !product_contract::installation::PRODUCTS.contains(&product.owner.as_str())
            || !owners.insert(&product.owner)
        {
            return Err("checkpoint_owner_invalid");
        }
        let root = target.join(&product.owner);
        if !product.present {
            if !product.directories.is_empty()
                || !product.files.is_empty()
                || !matches!(fs::symlink_metadata(root), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("checkpoint_copy_changed");
            }
            continue;
        }
        retained.push(Directory::open(&root)?);
        let (directories, files) = listing(&root)?;
        if directories != product.directories
            || files
                != product
                    .files
                    .iter()
                    .map(|entry| entry.relative.clone())
                    .collect::<Vec<_>>()
        {
            return Err("checkpoint_copy_changed");
        }
        for directory in &directories {
            retained.push(Directory::open(&root.join(directory))?);
        }
        for entry in &product.files {
            let (mut actual, _) =
                copy_or_hash(&root.join(&entry.relative), None, started, cancelled)?;
            actual.relative = entry.relative.clone();
            if &actual != entry {
                return Err("checkpoint_copy_changed");
            }
            total = total
                .checked_add(entry.bytes)
                .ok_or("checkpoint_size_limit")?;
            count += 1;
            if total > MAX_BYTES || count > MAX_FILES {
                return Err("checkpoint_size_limit");
            }
        }
    }
    if total != expected.bytes || count != expected.files {
        return Err("checkpoint_manifest_invalid");
    }
    for entry in fs::read_dir(&target).map_err(|_| "checkpoint_copy_changed")? {
        let entry = entry.map_err(|_| "checkpoint_copy_changed")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "checkpoint_path_unsafe")?;
        if name == "checkpoint.pending" {
            let (pending, _) = copy_or_hash(&entry.path(), None, started, cancelled)?;
            if pending.sha256 != expected.revision || pending.bytes != bytes.len() as u64 {
                return Err("checkpoint_copy_changed");
            }
            continue;
        }
        if name != "checkpoint.json" && !owners.iter().any(|owner| owner.as_str() == name) {
            return Err("checkpoint_copy_changed");
        }
    }
    for directory in retained {
        directory.check()?;
    }
    parent_handle.check()?;
    target_handle.check()?;
    bounded(started, cancelled)?;
    Ok(manifest)
}

/// Require that closed live namespaces still equal the preimage reviewed by the
/// user. A later edit, added file, removed store or changed WAL requires review.
pub fn matches_quiesced_sources(
    sources: &BTreeMap<String, PathBuf>,
    parent: &Path,
    expected: &Receipt,
    installation_key: &str,
    generation: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    let manifest = verify_manifest(parent, expected, installation_key, generation, cancelled)?;
    if sources.len() != manifest.products.len() {
        return Err("checkpoint_owner_invalid");
    }
    let started = Instant::now();
    let mut retained = Vec::new();
    for product in &manifest.products {
        bounded(started, cancelled)?;
        let source = sources
            .get(&product.owner)
            .ok_or("checkpoint_owner_invalid")?;
        if !product.present {
            if !matches!(fs::symlink_metadata(source), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("checkpoint_source_changed");
            }
            continue;
        }
        retained.push(Directory::open(source)?);
        let (directories, files) = listing(source)?;
        if directories != product.directories
            || files
                != product
                    .files
                    .iter()
                    .map(|entry| entry.relative.clone())
                    .collect::<Vec<_>>()
        {
            return Err("checkpoint_source_changed");
        }
        for relative in &directories {
            retained.push(Directory::open(&source.join(relative))?);
        }
        for entry in &product.files {
            let (mut actual, _) =
                copy_or_hash(&source.join(&entry.relative), None, started, cancelled)?;
            actual.relative = entry.relative.clone();
            if &actual != entry {
                return Err("checkpoint_source_changed");
            }
        }
    }
    for directory in retained {
        directory.check()?;
    }
    bounded(started, cancelled)
}

/// Materialize a reviewed checkpoint into a new private operation directory.
/// This never writes the live namespaces; activation must separately preserve
/// their current data and journal before swapping any namespace into place.
pub fn materialize(
    parent: &Path,
    expected: &Receipt,
    installation_key: &str,
    generation: &str,
    target: &Path,
    cancelled: &AtomicBool,
) -> Result<()> {
    if target.file_name().and_then(|name| name.to_str()) != Some(expected.id.as_str()) {
        return Err("checkpoint_destination_invalid");
    }
    let manifest = verify_manifest(parent, expected, installation_key, generation, cancelled)?;
    let target_parent = Directory::open(target.parent().ok_or("checkpoint_path_unsafe")?)?;
    fs::create_dir(target).map_err(|_| "checkpoint_destination_exists")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target, fs::Permissions::from_mode(0o700))
            .map_err(|_| "checkpoint_copy_failed")?;
    }
    let target_handle = Directory::open(target)?;
    let started = Instant::now();
    let source = parent.join(&expected.id);
    let mut retained = Vec::new();
    for product in &manifest.products {
        bounded(started, cancelled)?;
        if !product.present {
            continue;
        }
        let destination = target.join(&product.owner);
        fs::create_dir(&destination).map_err(|_| "checkpoint_copy_failed")?;
        retained.push(Directory::open(&destination)?);
        for relative in &product.directories {
            let directory = destination.join(relative);
            fs::create_dir(&directory).map_err(|_| "checkpoint_copy_failed")?;
            retained.push(Directory::open(&directory)?);
        }
        for entry in &product.files {
            let (mut actual, _) = copy_or_hash(
                &source.join(&product.owner).join(&entry.relative),
                Some(&destination.join(&entry.relative)),
                started,
                cancelled,
            )?;
            actual.relative = entry.relative.clone();
            if &actual != entry {
                return Err("checkpoint_copy_changed");
            }
        }
        let (directories, files) = listing(&destination)?;
        if directories != product.directories
            || files
                != product
                    .files
                    .iter()
                    .map(|entry| entry.relative.clone())
                    .collect::<Vec<_>>()
        {
            return Err("checkpoint_copy_changed");
        }
        for entry in &product.files {
            let (mut actual, _) =
                copy_or_hash(&destination.join(&entry.relative), None, started, cancelled)?;
            actual.relative = entry.relative.clone();
            if &actual != entry {
                return Err("checkpoint_copy_changed");
            }
        }
    }
    // Publish readiness only after the selected immutable source and every copy
    // still match. Incomplete directories remain private recovery evidence.
    verify(parent, expected, installation_key, generation, cancelled)?;
    for directory in retained {
        directory.check()?;
    }
    target_parent.check()?;
    target_handle.check()?;
    bounded(started, cancelled)?;
    let bytes = serde_json::to_vec(&manifest).map_err(|_| "checkpoint_manifest_invalid")?;
    let pending = target.join("checkpoint.pending");
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "checkpoint_publish_failed")?;
    marker
        .write_all(&bytes)
        .and_then(|()| marker.sync_all())
        .map_err(|_| "checkpoint_publish_failed")?;
    drop(marker);
    target_handle.check()?;
    fs::hard_link(&pending, target.join("checkpoint.json"))
        .map_err(|_| "checkpoint_publish_failed")?;
    let _ = fs::remove_file(pending);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> std::io::Result<Temp> {
        let path = std::env::temp_dir().join(format!("devbox-checkpoint-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Temp(path))
    }

    fn sources(root: &Path) -> BTreeMap<String, PathBuf> {
        product_contract::installation::PRODUCTS
            .iter()
            .map(|owner| (owner.to_string(), root.join(owner)))
            .collect()
    }
    #[test]
    fn restore_preparation_keeps_later_data_and_rejects_a_stale_preimage() {
        let root = tempdir().unwrap();
        let backup = tempdir().unwrap();
        let stage = tempdir().unwrap();
        let sources = sources(root.path());
        fs::create_dir(&sources["knowledge"]).unwrap();
        let note = sources["knowledge"].join("original.txt");
        fs::write(&note, b"checkpoint version").unwrap();
        let key = "a".repeat(64);
        let cancelled = AtomicBool::new(false);
        let receipt =
            acquire_quiesced(&sources, backup.path(), &key, "generation", &cancelled).unwrap();
        matches_quiesced_sources(
            &sources,
            backup.path(),
            &receipt,
            &key,
            "generation",
            &cancelled,
        )
        .unwrap();
        fs::write(&note, b"new user data after the checkpoint").unwrap();
        let target = stage.path().join(&receipt.id);
        materialize(
            backup.path(),
            &receipt,
            &key,
            "generation",
            &target,
            &cancelled,
        )
        .unwrap();
        verify(stage.path(), &receipt, &key, "generation", &cancelled).unwrap();
        assert_eq!(
            fs::read(target.join("knowledge/original.txt")).unwrap(),
            b"checkpoint version"
        );
        assert_eq!(
            fs::read(&note).unwrap(),
            b"new user data after the checkpoint"
        );
        assert!(matches_quiesced_sources(
            &sources,
            backup.path(),
            &receipt,
            &key,
            "generation",
            &cancelled
        )
        .is_err());
        assert!(materialize(
            backup.path(),
            &receipt,
            &key,
            "generation",
            &target,
            &cancelled
        )
        .is_err());
        assert_eq!(
            fs::read(&note).unwrap(),
            b"new user data after the checkpoint"
        );
    }
    #[test]
    fn corrupt_restore_input_never_publishes_a_prepared_directory() {
        let root = tempdir().unwrap();
        let backup = tempdir().unwrap();
        let stage = tempdir().unwrap();
        let sources = sources(root.path());
        fs::create_dir(&sources["workspace"]).unwrap();
        fs::write(sources["workspace"].join("state.json"), b"original").unwrap();
        let key = "b".repeat(64);
        let cancelled = AtomicBool::new(false);
        let receipt =
            acquire_quiesced(&sources, backup.path(), &key, "generation", &cancelled).unwrap();
        fs::write(
            backup.path().join(&receipt.id).join("workspace/state.json"),
            b"tampered",
        )
        .unwrap();
        let target = stage.path().join(&receipt.id);
        assert!(materialize(
            backup.path(),
            &receipt,
            &key,
            "generation",
            &target,
            &cancelled
        )
        .is_err());
        assert!(!target.exists());
        assert_eq!(
            fs::read(sources["workspace"].join("state.json")).unwrap(),
            b"original"
        );
    }
    #[test]
    fn closed_wal_and_browser_bytes_are_preserved_and_missing_products_are_explicit() {
        let root = tempdir().unwrap();
        let backup = tempdir().unwrap();
        let sources = sources(root.path());
        let knowledge = &sources["knowledge"];
        fs::create_dir(knowledge).unwrap();
        let db = knowledge.join("data.db");
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('committed-only-in-wal');").unwrap();
        // Capture a synthetic abruptly closed SQLite image without changing the
        // real source under test: files are copied while the synthetic writer is
        // idle, then all fixture handles are closed before acquire_quiesced.
        let staged = tempdir().unwrap();
        fs::copy(&db, staged.path().join("data.db")).unwrap();
        fs::copy(
            knowledge.join("data.db-wal"),
            staged.path().join("data.db-wal"),
        )
        .unwrap();
        drop(connection);
        fs::copy(staged.path().join("data.db"), &db).unwrap();
        fs::copy(
            staged.path().join("data.db-wal"),
            knowledge.join("data.db-wal"),
        )
        .unwrap();
        fs::create_dir(knowledge.join("empty")).unwrap();
        fs::create_dir(knowledge.join("EBWebView")).unwrap();
        fs::write(
            knowledge.join("EBWebView").join("closed-leveldb.log"),
            b"closed browser fixture",
        )
        .unwrap();
        let before = fs::read(&db).unwrap();
        let wal = fs::read(knowledge.join("data.db-wal")).unwrap();
        let receipt = acquire_quiesced(
            &sources,
            backup.path(),
            &"a".repeat(64),
            "generation",
            &AtomicBool::new(false),
        )
        .unwrap();
        verify(
            backup.path(),
            &receipt,
            &"a".repeat(64),
            "generation",
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(verify(
            backup.path(),
            &receipt,
            &"b".repeat(64),
            "generation",
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(verify(
            backup.path(),
            &receipt,
            &"a".repeat(64),
            "other-generation",
            &AtomicBool::new(false)
        )
        .is_err());
        let copied = backup.path().join(&receipt.id).join("knowledge");
        assert_eq!(fs::read(&db).unwrap(), before);
        assert_eq!(fs::read(knowledge.join("data.db-wal")).unwrap(), wal);
        let restore_fixture = tempdir().unwrap();
        fs::copy(
            copied.join("data.db"),
            restore_fixture.path().join("data.db"),
        )
        .unwrap();
        fs::copy(
            copied.join("data.db-wal"),
            restore_fixture.path().join("data.db-wal"),
        )
        .unwrap();
        let restored = rusqlite::Connection::open(restore_fixture.path().join("data.db")).unwrap();
        assert_eq!(
            restored
                .query_row("SELECT value FROM retained", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "committed-only-in-wal"
        );
        assert!(copied.join("empty").is_dir());
        assert_eq!(
            fs::read(copied.join("EBWebView").join("closed-leveldb.log")).unwrap(),
            b"closed browser fixture"
        );
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(backup.path().join(&receipt.id).join("checkpoint.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest
                .products
                .iter()
                .filter(|product| product.present)
                .count(),
            1
        );
        assert_eq!(manifest.products.len(), 4);
        fs::write(
            copied.join("EBWebView").join("closed-leveldb.log"),
            b"changed",
        )
        .unwrap();
        assert!(verify(
            backup.path(),
            &receipt,
            &"a".repeat(64),
            "generation",
            &AtomicBool::new(false)
        )
        .is_err());
    }
    #[test]
    fn cancellation_publishes_no_accepted_checkpoint_and_keeps_originals() {
        let root = tempdir().unwrap();
        let backup = tempdir().unwrap();
        assert!(acquire_quiesced(
            &sources(root.path()),
            backup.path(),
            &"a".repeat(64),
            "generation",
            &AtomicBool::new(true)
        )
        .is_err());
        for entry in fs::read_dir(backup.path()).unwrap() {
            assert!(!entry.unwrap().path().join("checkpoint.json").exists());
        }
    }
    #[cfg(unix)]
    #[test]
    fn outside_links_are_rejected_without_copying_the_target() {
        let root = tempdir().unwrap();
        let backup = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("private"), b"outside").unwrap();
        let sources = sources(root.path());
        fs::create_dir(&sources["knowledge"]).unwrap();
        std::os::unix::fs::symlink(outside.path(), sources["knowledge"].join("vault")).unwrap();
        assert!(acquire_quiesced(
            &sources,
            backup.path(),
            &"a".repeat(64),
            "generation",
            &AtomicBool::new(false)
        )
        .is_err());
        assert_eq!(
            fs::read(outside.path().join("private")).unwrap(),
            b"outside"
        );
    }
}
