//! Repo Manager의 Launcher용 `repositories/v1` named snapshot producer.
//!
//! Repo Manager의 검색 결과는 process-local 상태로만 유지한다. 전체 repository
//! scan은 상태를 교체하고, worktree 조회는 발견한 항목을 보강한다. 공용 snapshot
//! 경계를 넘는 값은 opaque repository id, 안전한 basename label, 고정 detail과
//! 다시 검증할 수 있는 path뿐이다. canonical key 자체나 branch/status/Git 출력은
//! snapshot에 기록하지 않는다.

use crate::commands::RepoEntry;
use devbox_applink::contains_sensitive_value;
use devbox_filesystem::{parse_safe_project_path, SafeProjectPath, MAX_PROJECT_PATH_BYTES};
use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
use std::path::Path;

pub const PRODUCER_ID: &str = "repo-manager";
pub const VIEW_KIND: &str = "repositories";
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const VIEW_SCHEMA_VERSION: u32 = 1;
pub const PAYLOAD_VERSION: u32 = 1;
pub const MAX_REPOSITORIES: usize = 2_048;

const STATIC_REPOSITORY_LABEL: &str = "Repo Manager repository";
const STATIC_REPOSITORY_DETAIL: &str = "Repo Manager · repository";
const STATIC_WORKTREE_DETAIL: &str = "Repo Manager · worktree";
const SNAPSHOT_ERROR: &str = "Repo Manager repository snapshot을 만들 수 없습니다";

/// Keep the integration projection bounded independently from the UI scan
/// result. The key is the validated canonical identity, never the display path.
type RepositoryMap = BTreeMap<String, RepositoryRecord>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryRecord {
    entry: RepoEntry,
    safe_path: String,
    id: String,
    is_worktree: bool,
}

fn repository_map() -> &'static Mutex<RepositoryMap> {
    static MAP: OnceLock<Mutex<RepositoryMap>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherRepositoryEntry {
    id: String,
    label: String,
    detail: &'static str,
    target_app: &'static str,
    target_kind: &'static str,
    payload_version: u32,
    payload: LauncherRepositoryPayload,
}

#[derive(Debug, Clone, Serialize)]
struct LauncherRepositoryPayload {
    path: String,
}

/// Replace the complete repository projection after a finished scan.
///
/// Publication is intentionally best-effort: a filesystem/snapshot failure
/// must not turn an otherwise successful Repo Manager scan into an error.
pub(crate) fn replace_repositories(entries: Vec<RepoEntry>) {
    let mut map = repository_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    replace_map(&mut map, entries);
    publish_locked(&map);
}

/// Add validated repository/worktree results after a successful worktree query.
/// Existing canonical identities are merged deterministically and the bounded
/// projection is republished as one atomic named view.
pub(crate) fn add_worktree_repositories(entries: Vec<RepoEntry>) {
    let mut map = repository_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    add_map(&mut map, entries, true);
    publish_locked(&map);
}

fn replace_map(map: &mut RepositoryMap, entries: Vec<RepoEntry>) {
    map.clear();
    for record in normalize_records(entries, false) {
        map.insert(record.entry.canonical_key.clone(), record);
    }
    retain_bound(map);
}

fn add_map(map: &mut RepositoryMap, entries: Vec<RepoEntry>, is_worktree: bool) {
    for record in normalize_records(entries, is_worktree) {
        let key = record.entry.canonical_key.clone();
        match map.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let selected = choose_record(slot.get(), &record);
                if selected != *slot.get() {
                    slot.insert(selected);
                }
            }
        }
    }
    retain_bound(map);
}

fn normalize_records(entries: Vec<RepoEntry>, is_worktree: bool) -> Vec<RepositoryRecord> {
    let mut records = entries
        .into_iter()
        .filter_map(|entry| repository_record(entry, is_worktree))
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.entry
            .canonical_key
            .cmp(&right.entry.canonical_key)
            .then_with(|| left.safe_path.cmp(&right.safe_path))
    });

    let mut unique: Vec<RepositoryRecord> = Vec::with_capacity(records.len());
    for record in records {
        if let Some(previous) = unique.last_mut() {
            if previous.entry.canonical_key == record.entry.canonical_key {
                let selected = choose_record(previous, &record);
                *previous = selected;
                continue;
            }
        }
        unique.push(record);
    }
    unique
}

fn choose_record(left: &RepositoryRecord, right: &RepositoryRecord) -> RepositoryRecord {
    // A scan's primary repository record is more informative than a worktree
    // observation for the same canonical key. Otherwise choose path spelling
    // lexicographically so an unordered caller remains deterministic.
    if left.is_worktree != right.is_worktree {
        return if left.is_worktree {
            right.clone()
        } else {
            left.clone()
        };
    }
    if left.safe_path <= right.safe_path {
        left.clone()
    } else {
        right.clone()
    }
}

fn retain_bound(map: &mut RepositoryMap) {
    while map.len() > MAX_REPOSITORIES {
        let Some(last_key) = map.keys().next_back().cloned() else {
            break;
        };
        map.remove(&last_key);
    }
}

fn repository_record(entry: RepoEntry, is_worktree: bool) -> Option<RepositoryRecord> {
    let safe_path = safe_path(&entry.path)?;
    let id = devbox_integration::opaque_identity(PRODUCER_IDENTITY_NAMESPACE, &entry.canonical_key)
        .ok()?;
    Some(RepositoryRecord {
        entry,
        safe_path,
        id,
        is_worktree,
    })
}

const PRODUCER_IDENTITY_NAMESPACE: &str = "repository";

fn safe_path(path: &str) -> Option<String> {
    let parsed = parse_safe_project_path(path)?;
    // Producers must not silently rewrite a path into a different target. The
    // parser's trimmed/normalized spelling is accepted only when it is the
    // exact spelling already validated by Repo Manager.
    (parsed.as_str() == path).then(|| parsed.into_string())
}

fn publish_locked(map: &RepositoryMap) {
    let Ok(envelope) = build_envelope(map) else {
        return;
    };
    let _ = devbox_integration::write_named_view_snapshot_atomic(
        &envelope,
        &devbox_integration::integration_root(),
        VIEW_KIND,
    );
}

fn build_envelope(map: &RepositoryMap) -> Result<Envelope, String> {
    if map.len() > MAX_REPOSITORIES {
        return Err(SNAPSHOT_ERROR.to_owned());
    }

    let mut entries = map
        .values()
        .map(snapshot_entry)
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });

    let mut views = SnapshotViews::new();
    views.insert(
        VIEW_KIND.to_owned(),
        SnapshotView {
            schema_version: VIEW_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    );
    let envelope = Envelope::with_views(PRODUCER_ID, env!("CARGO_PKG_VERSION"), views);
    if envelope.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(SNAPSHOT_ERROR.to_owned());
    }
    Ok(envelope)
}

fn snapshot_entry(record: &RepositoryRecord) -> Result<serde_json::Value, String> {
    let safe =
        parse_safe_project_path(&record.safe_path).ok_or_else(|| SNAPSHOT_ERROR.to_owned())?;
    if safe.as_str() != record.safe_path
        || record.entry.canonical_key.is_empty()
        || record.entry.canonical_key.len() > MAX_PROJECT_PATH_BYTES
        || record.entry.canonical_key.chars().any(char::is_control)
    {
        return Err(SNAPSHOT_ERROR.to_owned());
    }

    serde_json::to_value(LauncherRepositoryEntry {
        id: record.id.clone(),
        label: safe_repository_label(&safe),
        detail: if record.is_worktree {
            STATIC_WORKTREE_DETAIL
        } else {
            STATIC_REPOSITORY_DETAIL
        },
        target_app: PRODUCER_ID,
        target_kind: "path",
        payload_version: PAYLOAD_VERSION,
        payload: LauncherRepositoryPayload {
            path: record.safe_path.clone(),
        },
    })
    .map_err(|_| SNAPSHOT_ERROR.to_owned())
}

fn safe_repository_label(path: &SafeProjectPath) -> String {
    let candidate = path.name();
    if valid_public_label(candidate) {
        candidate.to_owned()
    } else {
        STATIC_REPOSITORY_LABEL.to_owned()
    }
}

fn valid_public_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
        && !contains_sensitive_value(value)
        && !looks_like_path(value)
}

/// A basename normally cannot contain a path separator, but keep the same
/// conservative path-shaped label rejection used by other producers. This
/// prevents a malformed POSIX basename from becoming a path preview.
fn looks_like_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.starts_with("~/")
        || value.starts_with("~\\")
        || value.starts_with("./")
        || value.starts_with(".\\")
        || value.starts_with("../")
        || value.starts_with("..\\")
        || value
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

#[cfg(test)]
fn replace_repositories_in(root: &Path, entries: Vec<RepoEntry>) -> Result<(), String> {
    let mut map = RepositoryMap::new();
    replace_map(&mut map, entries);
    write_envelope_in(root, &map)
}

#[cfg(test)]
fn add_worktree_repositories_in(
    root: &Path,
    map: &mut RepositoryMap,
    entries: Vec<RepoEntry>,
) -> Result<(), String> {
    add_map(map, entries, true);
    write_envelope_in(root, map)
}

#[cfg(test)]
fn write_envelope_in(root: &Path, map: &RepositoryMap) -> Result<(), String> {
    let envelope = build_envelope(map)?;
    devbox_integration::write_named_view_snapshot_atomic(&envelope, root, VIEW_KIND)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_integration::{
        named_view_snapshot_path_in, opaque_identity, read_named_view_snapshot_in,
    };
    use std::collections::BTreeSet;

    fn entry(path: &str, canonical_key: &str) -> RepoEntry {
        RepoEntry {
            path: path.to_owned(),
            canonical_key: canonical_key.to_owned(),
            has_worktrees: false,
        }
    }

    fn records(entries: Vec<RepoEntry>) -> RepositoryMap {
        normalize_records(entries, false)
            .into_iter()
            .map(|record| (record.entry.canonical_key.clone(), record))
            .collect()
    }

    fn snapshot_entries(envelope: &Envelope) -> Vec<serde_json::Value> {
        envelope
            .data
            .get("views")
            .and_then(|views| views.get(VIEW_KIND))
            .and_then(|view| view.get("entries"))
            .and_then(serde_json::Value::as_array)
            .expect("repository entries")
            .clone()
    }

    #[test]
    fn entry_shape_uses_opaque_identity_and_safe_path_only() {
        let repository = entry("C:\\Projects\\Devbox", "win:c:/projects/devbox");
        let map = records(vec![repository]);
        let envelope = build_envelope(&map).unwrap();
        let entries = snapshot_entries(&envelope);
        assert_eq!(entries.len(), 1);

        let expected_id =
            opaque_identity(PRODUCER_IDENTITY_NAMESPACE, "win:c:/projects/devbox").unwrap();
        assert_eq!(entries[0]["id"], expected_id);
        assert_eq!(entries[0]["label"], "Devbox");
        assert_eq!(entries[0]["detail"], STATIC_REPOSITORY_DETAIL);
        assert_eq!(entries[0]["targetApp"], PRODUCER_ID);
        assert_eq!(entries[0]["targetKind"], "path");
        assert_eq!(entries[0]["payloadVersion"], PAYLOAD_VERSION);
        assert_eq!(
            entries[0]["payload"],
            serde_json::json!({
                "path": "C:\\Projects\\Devbox"
            })
        );
        assert_eq!(
            entries[0]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "detail".into(),
                "id".into(),
                "label".into(),
                "payload".into(),
                "payloadVersion".into(),
                "targetApp".into(),
                "targetKind".into(),
            ])
        );
        let serialized = serde_json::to_string(&envelope).unwrap();
        assert!(!serialized.contains("canonicalKey"));
        assert!(!serialized.contains("branch"));
        assert!(!serialized.contains("status"));
    }

    #[test]
    fn worktree_detail_is_static_and_sensitive_basename_falls_back() {
        let mut worktree = entry(
            "//wsl.localhost/Ubuntu/home/jihoon/projects/password=do-not-copy",
            "wsl:ubuntu:/home/jihoon/projects/password=do-not-copy",
        );
        worktree.has_worktrees = true;
        let mut map = RepositoryMap::new();
        add_map(&mut map, vec![worktree], true);
        let envelope = build_envelope(&map).unwrap();
        let values = snapshot_entries(&envelope);
        assert_eq!(values[0]["label"], STATIC_REPOSITORY_LABEL);
        assert_eq!(values[0]["detail"], STATIC_WORKTREE_DETAIL);
    }

    #[test]
    fn invalid_paths_and_identity_sources_are_rejected_before_projection() {
        for path in [
            "relative/repository",
            "/home/jihoon/projects/../private",
            r"\\?\C:\private\repository",
            "/home/jihoon/projects/line\nfeed",
        ] {
            assert!(
                repository_record(entry(path, "win:c:/safe"), false).is_none(),
                "{path:?}"
            );
        }
        assert!(repository_record(entry("C:\\Projects\\Safe", "bad\nkey"), false).is_none());
    }

    #[test]
    fn map_deduplicates_and_keeps_a_deterministic_bound() {
        let mut entries = vec![
            entry("C:\\Projects\\same", "win:c:/projects/same"),
            entry("c:/projects/same", "win:c:/projects/same"),
        ];
        entries.extend((0..=MAX_REPOSITORIES).map(|index| {
            entry(
                &format!("C:\\Projects\\repo-{index}"),
                &format!("win:c:/projects/repo-{index}"),
            )
        }));
        let mut map = RepositoryMap::new();
        replace_map(&mut map, entries);
        assert_eq!(map.len(), MAX_REPOSITORIES);
        let envelope = build_envelope(&map).unwrap();
        let values = snapshot_entries(&envelope);
        assert_eq!(values.len(), MAX_REPOSITORIES);
        let ids = values
            .iter()
            .map(|value| value["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            map.values()
                .filter(|record| record.entry.canonical_key == "win:c:/projects/same")
                .count(),
            0,
            "the deterministic bound removes the lexicographically last keys"
        );
    }

    #[test]
    fn named_snapshot_replaces_scan_and_adds_worktree_results() {
        let root = tempfile::tempdir().unwrap();
        let first = entry(
            "/home/jihoon/projects/first",
            "wsl:ubuntu:/home/jihoon/projects/first",
        );
        replace_repositories_in(root.path(), vec![first]).unwrap();
        let mut map = records(vec![entry(
            "/home/jihoon/projects/first",
            "wsl:ubuntu:/home/jihoon/projects/first",
        )]);
        add_worktree_repositories_in(
            root.path(),
            &mut map,
            vec![entry(
                "/home/jihoon/projects/first-wt",
                "wsl:ubuntu:/home/jihoon/projects/first-wt",
            )],
        )
        .unwrap();

        let path = named_view_snapshot_path_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            VIEW_KIND,
        )
        .unwrap();
        assert!(path.ends_with("repo-manager/v1/repositories.json"));
        assert!(path.is_file());
        assert!(!path.with_file_name("summary.json").exists());
        let envelope = read_named_view_snapshot_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            VIEW_KIND,
        )
        .unwrap()
        .unwrap();
        assert_eq!(snapshot_entries(&envelope).len(), 2);

        replace_repositories_in(
            root.path(),
            vec![entry(
                "/home/jihoon/projects/replaced",
                "wsl:ubuntu:/home/jihoon/projects/replaced",
            )],
        )
        .unwrap();
        let replaced = read_named_view_snapshot_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            VIEW_KIND,
        )
        .unwrap()
        .unwrap();
        let replaced_entries = snapshot_entries(&replaced);
        assert_eq!(replaced_entries.len(), 1);
        assert_eq!(
            replaced_entries[0]["payload"]["path"],
            "/home/jihoon/projects/replaced"
        );
    }

    #[test]
    fn windows_unc_and_posix_paths_share_the_safe_path_parser() {
        for (path, key) in [
            ("C:\\Projects\\Devbox", "win:c:/projects/devbox"),
            (
                "\\\\wsl.localhost\\Ubuntu\\home\\jihoon\\projects\\devbox",
                "wsl:ubuntu:/home/jihoon/projects/devbox",
            ),
            (
                "/home/jihoon/projects/devbox",
                "wsl:ubuntu:/home/jihoon/projects/devbox",
            ),
        ] {
            assert!(
                repository_record(entry(path, key), false).is_some(),
                "{path}"
            );
        }
    }
}
