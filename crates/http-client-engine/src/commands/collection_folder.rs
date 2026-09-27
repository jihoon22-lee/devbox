use super::import_files::ImportFile;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
const MARKER: &str = "collection.devbox.json";
const INVALID: &str = "folder_path_invalid";
const LARGE: &str = "folder_too_large";
const EXPIRED: &str = "folder_grant_expired";
const LIMIT: usize = 1024 * 1024;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Debug, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct FolderFile {
    pub relative_path: String,
    pub text: String,
}
#[derive(Debug, Serialize, ts_rs::TS)]
pub struct WriteResult {
    pub written: u32,
    pub stale: Vec<String>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
pub struct FolderGrant {
    pub grant_id: String,
    pub name: String,
}
struct Grant {
    root: PathBuf,
    identity: devbox_filesystem::FilesystemIdentity,
    expires: Instant,
}
#[derive(Default)]
pub struct CollectionFolderState(Mutex<HashMap<String, Grant>>);
fn root_identity(root: &Path) -> Result<devbox_filesystem::FilesystemIdentity> {
    if !root.is_absolute() {
        return Err(INVALID);
    }
    devbox_filesystem::ensure_no_links(root).map_err(|_| INVALID)?;
    devbox_filesystem::filesystem_identity(root, true).map_err(|_| INVALID)
}
fn relative(path: &str, write: bool) -> Result<()> {
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > 9
        || path.len() > 2048
        || parts.iter().any(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            part.is_empty()
                || matches!(*part, "." | "..")
                || part.ends_with(['.', ' '])
                || part
                    .chars()
                    .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
        || !(path == MARKER
            || path.ends_with(".request.json")
            || (!write && path.ends_with(".bru")))
    {
        return Err(INVALID);
    }
    Ok(())
}
fn walk(
    root: &Path,
    folder: &Path,
    depth: usize,
    scanned: &mut usize,
    paths: &mut Vec<String>,
) -> Result<()> {
    if depth > 8 {
        return Err(LARGE);
    }
    devbox_filesystem::ensure_no_links(folder).map_err(|_| INVALID)?;
    for entry in std::fs::read_dir(folder).map_err(|_| INVALID)? {
        *scanned += 1;
        if *scanned > 4000 {
            return Err(LARGE);
        }
        let entry = entry.map_err(|_| INVALID)?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| INVALID)?;
        if metadata.file_type().is_symlink() || devbox_filesystem::ensure_no_links(&path).is_err() {
            continue;
        }
        if metadata.is_dir() {
            walk(root, &path, depth + 1, scanned, paths)?;
        } else if metadata.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|_| INVALID)?
                .to_str()
                .ok_or(INVALID)?
                .replace('\\', "/");
            if name == MARKER || name.ends_with(".request.json") || name.ends_with(".bru") {
                relative(&name, false)?;
                paths.push(name);
                if paths.len() > 1000 {
                    return Err(LARGE);
                }
            }
        }
    }
    Ok(())
}
pub fn read_folder(root: &Path) -> Result<Vec<ImportFile>> {
    let identity = root_identity(root)?;
    let mut paths = vec![];
    walk(root, root, 0, &mut 0, &mut paths)?;
    paths.sort();
    let mut total = 0usize;
    let files = paths
        .into_iter()
        .map(|relative_path| {
            let bytes = super::transfer::read_bounded(&root.join(&relative_path), LIMIT).map_err(
                |error| match error {
                    super::transfer::ReadError::TooLarge => LARGE,
                    _ => INVALID,
                },
            )?;
            total = total.checked_add(bytes.len()).ok_or(LARGE)?;
            if total > 32 * LIMIT {
                return Err(LARGE);
            }
            let name = relative_path.rsplit('/').next().ok_or(INVALID)?.to_owned();
            let text = super::import_files::decode_utf8(bytes).map_err(|_| INVALID)?;
            Ok(ImportFile {
                name,
                relative_path,
                text,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if root_identity(root)? != identity {
        return Err(INVALID);
    }
    Ok(files)
}
fn valid_marker(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok_and(|value| {
        value["schema"] == "devbox.api-studio.file-collection" && value["schemaVersion"] == 1
    })
}
fn validate_destination(root: &Path, relative: &str) -> Result<()> {
    let parts = relative.split('/').collect::<Vec<_>>();
    let mut path = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                devbox_filesystem::ensure_no_links(&path).map_err(|_| INVALID)?;
                devbox_filesystem::filesystem_identity(&path, index + 1 < parts.len())
                    .map_err(|_| INVALID)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(INVALID),
        }
    }
    Ok(())
}
pub fn write_folder(root: &Path, files: &[FolderFile]) -> Result<WriteResult> {
    let identity = root_identity(root)?;
    if files.len() > 1000 {
        return Err(LARGE);
    }
    let mut seen = BTreeSet::new();
    let mut total = 0usize;
    // Validate the complete request before the first filesystem mutation.
    for file in files {
        relative(&file.relative_path, true)?;
        if !seen.insert(file.relative_path.to_lowercase()) {
            return Err(INVALID);
        }
        total = total.checked_add(file.text.len()).ok_or(LARGE)?;
        if file.text.len() > LIMIT || total > 32 * LIMIT {
            return Err(LARGE);
        }
        validate_destination(root, &file.relative_path)?;
    }
    for file in files {
        let mut parent = Path::new(&file.relative_path).parent();
        while let Some(path) = parent {
            if !path.as_os_str().is_empty()
                && seen.contains(&path.to_string_lossy().replace('\\', "/").to_lowercase())
            {
                return Err(INVALID);
            }
            parent = path.parent();
        }
    }
    if !files
        .iter()
        .any(|file| file.relative_path == MARKER && valid_marker(&file.text))
    {
        return Err("folder_not_collection");
    }
    if std::fs::read_dir(root)
        .map_err(|_| INVALID)?
        .next()
        .is_some()
    {
        let marker = super::transfer::read_bounded(&root.join(MARKER), LIMIT)
            .map_err(|_| "folder_not_collection")?;
        if !std::str::from_utf8(&marker).is_ok_and(valid_marker) {
            return Err("folder_not_collection");
        }
    }
    let mut existing = vec![];
    walk(root, root, 0, &mut 0, &mut existing)?;
    let mut stale = existing
        .into_iter()
        .filter(|name| name.ends_with(".request.json") && !seen.contains(&name.to_lowercase()))
        .collect::<Vec<_>>();
    stale.sort();
    for file in files {
        if root_identity(root)? != identity {
            return Err(INVALID);
        }
        let mut parent = root.to_path_buf();
        let parts = file.relative_path.split('/').collect::<Vec<_>>();
        for part in &parts[..parts.len() - 1] {
            parent.push(part);
            match std::fs::symlink_metadata(&parent) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(INVALID),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&parent).map_err(|_| INVALID)?
                }
                Err(_) => return Err(INVALID),
            }
            devbox_filesystem::ensure_no_links(&parent).map_err(|_| INVALID)?;
        }
        let path = root.join(&file.relative_path);
        super::transfer::validate_file_path(&path, false).map_err(|_| INVALID)?;
        devbox_filesystem::atomic_write(&path, file.text.as_bytes()).map_err(|_| INVALID)?;
    }
    if root_identity(root)? != identity {
        return Err(INVALID);
    }
    Ok(WriteResult {
        written: files.len() as u32,
        stale,
    })
}
impl CollectionFolderState {
    fn insert(&self, root: PathBuf) -> Result<FolderGrant> {
        let identity = root_identity(&root)?;
        let mut grants = self.0.lock().map_err(|_| EXPIRED)?;
        grants.retain(|_, grant| grant.expires > Instant::now());
        if grants.len() >= 8 {
            return Err(EXPIRED);
        }
        let grant_id = super::grpc_selection::random_hex_128().map_err(|_| EXPIRED)?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(INVALID)?
            .to_owned();
        grants.insert(
            grant_id.clone(),
            Grant {
                root,
                identity,
                expires: Instant::now() + Duration::from_secs(600),
            },
        );
        Ok(FolderGrant { grant_id, name })
    }
    fn root(&self, id: &str) -> Result<PathBuf> {
        let mut grants = self.0.lock().map_err(|_| EXPIRED)?;
        grants.retain(|_, grant| grant.expires > Instant::now());
        let grant = grants.get(id).ok_or(EXPIRED)?;
        if root_identity(&grant.root)? != grant.identity {
            return Err(EXPIRED);
        }
        Ok(grant.root.clone())
    }
}
pub async fn pick_collection_folder(
    app: AppHandle,
) -> std::result::Result<Option<FolderGrant>, String> {
    let state = app.state::<Arc<CollectionFolderState>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let root = selected.into_path().map_err(|_| INVALID)?;
        state.insert(root).map(Some)
    })
    .await
    .map_err(|_| INVALID)?
    .map_err(str::to_owned)
}
pub async fn read_collection_folder(
    state: Arc<CollectionFolderState>,
    id: String,
) -> std::result::Result<Vec<ImportFile>, String> {
    tauri::async_runtime::spawn_blocking(move || read_folder(&state.root(&id)?))
        .await
        .map_err(|_| INVALID)?
        .map_err(str::to_owned)
}
pub async fn write_collection_folder(
    state: Arc<CollectionFolderState>,
    id: String,
    files: Vec<FolderFile>,
) -> std::result::Result<WriteResult, String> {
    tauri::async_runtime::spawn_blocking(move || write_folder(&state.root(&id)?, &files))
        .await
        .map_err(|_| INVALID)?
        .map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn file(path: &str, text: &str) -> FolderFile {
        FolderFile {
            relative_path: path.into(),
            text: text.into(),
        }
    }
    fn marker() -> FolderFile {
        file("collection.devbox.json", "{\"schema\":\"devbox.api-studio.file-collection\",\"schemaVersion\":1,\"name\":\"Demo\"}\n")
    }
    #[test]
    fn grants_are_bounded_expire_and_reject_replaced_roots() {
        let state = CollectionFolderState::default();
        let root = tempfile::tempdir().unwrap();
        let first = state.insert(root.path().to_path_buf()).unwrap();
        assert_eq!(first.grant_id.len(), 32);
        for _ in 1..8 {
            state.insert(root.path().to_path_buf()).unwrap();
        }
        assert_eq!(
            state.insert(root.path().to_path_buf()).unwrap_err(),
            EXPIRED
        );
        state
            .0
            .lock()
            .unwrap()
            .get_mut(&first.grant_id)
            .unwrap()
            .expires = Instant::now();
        assert_eq!(state.root(&first.grant_id).unwrap_err(), EXPIRED);
        let selected = root.path().join("selected");
        fs::create_dir(&selected).unwrap();
        let grant = state.insert(selected.clone()).unwrap();
        fs::rename(&selected, root.path().join("old")).unwrap();
        fs::create_dir(&selected).unwrap();
        assert_eq!(state.root(&grant.grant_id).unwrap_err(), EXPIRED);
    }

    #[test]
    fn writes_only_empty_or_marked_folders_and_preserves_stale_files() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            write_folder(
                root.path(),
                &[
                    marker(),
                    file("Ops/Health.request.json", "{}"),
                    file("Old.request.json", "{}")
                ]
            )
            .unwrap()
            .written,
            3
        );
        let second = write_folder(
            root.path(),
            &[marker(), file("Ops/Health.request.json", "{\"v\":2}")],
        )
        .unwrap();
        assert_eq!(second.stale, vec!["Old.request.json"]);
        assert!(root.path().join("Old.request.json").exists());
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("notes.txt"), "mine").unwrap();
        assert_eq!(
            write_folder(other.path(), &[marker()]).unwrap_err(),
            "folder_not_collection"
        );
    }
    #[test]
    fn rejects_escaping_paths_duplicates_and_links_before_writing() {
        for bad in [
            "../x.request.json",
            "/abs.request.json",
            "a/../../x.request.json",
            "C:\\x.request.json",
            "a.txt",
            "CON.request.json",
            "a//x.request.json",
        ] {
            let root = tempfile::tempdir().unwrap();
            assert_eq!(
                write_folder(root.path(), &[marker(), file(bad, "{}")]).unwrap_err(),
                "folder_path_invalid"
            );
            assert!(!root.path().join("collection.devbox.json").exists());
        }
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            write_folder(
                root.path(),
                &[
                    marker(),
                    file("A.request.json", "{}"),
                    file("a.request.json", "{}")
                ]
            )
            .unwrap_err(),
            "folder_path_invalid"
        );
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
            assert_eq!(
                write_folder(
                    root.path(),
                    &[marker(), file("linked/x.request.json", "{}")]
                )
                .unwrap_err(),
                "folder_path_invalid"
            );
        }
    }
    #[test]
    fn rejects_file_directory_collisions_before_creating_any_output() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            write_folder(
                root.path(),
                &[
                    marker(),
                    file("a.request.json", "{}"),
                    file("a.request.json/b.request.json", "{}")
                ]
            )
            .unwrap_err(),
            INVALID
        );
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
    #[test]
    fn oversized_files_are_rejected_and_links_are_skipped_on_read() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.request.json"), vec![b'x'; LIMIT + 1]).unwrap();
        assert_eq!(read_folder(root.path()).unwrap_err(), LARGE);
        fs::remove_file(root.path().join("a.request.json")).unwrap();
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("secret.request.json"), "private").unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
            assert!(read_folder(root.path()).unwrap().is_empty());
        }
    }

    #[test]
    fn reads_nested_requests_and_bruno_but_skips_other_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("users/admin")).unwrap();
        fs::create_dir(root.path().join("environments")).unwrap();
        for (name, text) in [
            ("users/admin/a.bru", "meta {\n}\n"),
            ("b.request.json", "{}"),
            ("environments/local.bru", "vars {\n}\n"),
            ("ignored.txt", "x"),
        ] {
            fs::write(root.path().join(name), text).unwrap();
        }
        let paths: Vec<_> = read_folder(root.path())
            .unwrap()
            .into_iter()
            .map(|f| f.relative_path)
            .collect();
        assert_eq!(
            paths,
            vec![
                "b.request.json",
                "environments/local.bru",
                "users/admin/a.bru"
            ]
        );
    }
}
