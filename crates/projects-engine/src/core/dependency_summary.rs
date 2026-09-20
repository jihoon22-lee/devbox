//! Read-only consumer for `repo-manager/dependency-summary/v1`.
//!
//! Repo Manager owns dependency discovery and parsing. Workbench only reads a
//! bounded, aggregate snapshot for the selected canonical project identity;
//! package names, repository paths, registry URLs, and lockfile bytes never
//! cross this integration boundary.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

const PRODUCER: &str = "repo-manager";
const SNAPSHOT_VERSION: u32 = 1;
const VIEW_KIND: &str = "dependency-summary";
const VIEW_VERSION: u32 = 1;
const SOURCE_LABEL: &str = "Repo Manager dependency-summary/v1";

pub const FRESH_MAX_MS: u64 = 24 * 60 * 60 * 1_000;
pub const EXPIRED_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

const MAX_SUMMARY_ENTRIES: usize = 256;
const MAX_PACKAGES: usize = 4_096;
const MAX_EDGES: usize = 16_384;
const MAX_INPUT_FILES: usize = 256;
const MAX_ECOSYSTEMS: usize = 5;
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PackageDependencyStatus {
    Fresh,
    Stale,
    Expired,
    Missing,
    Corrupt,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackageDependencyEcosystem {
    pub ecosystem: String,
    pub package_count: usize,
    pub direct_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackageDependencySummary {
    pub profile_id: String,
    pub source: &'static str,
    pub status: PackageDependencyStatus,
    pub producer_version: Option<String>,
    pub freshness_ms: Option<u64>,
    pub revision: Option<String>,
    pub package_count: usize,
    pub direct_count: usize,
    pub transitive_count: usize,
    pub duplicate_count: usize,
    pub unresolved_dependency_count: usize,
    pub missing_lockfile_count: usize,
    pub stale_lockfile_count: usize,
    pub unsupported_count: usize,
    pub invalid_count: usize,
    pub truncated: bool,
    pub ecosystems: Vec<PackageDependencyEcosystem>,
}

impl PackageDependencySummary {
    fn unavailable(profile_id: &str, status: PackageDependencyStatus) -> Self {
        Self {
            profile_id: profile_id.to_owned(),
            source: SOURCE_LABEL,
            status,
            producer_version: None,
            freshness_ms: None,
            revision: None,
            package_count: 0,
            direct_count: 0,
            transitive_count: 0,
            duplicate_count: 0,
            unresolved_dependency_count: 0,
            missing_lockfile_count: 0,
            stale_lockfile_count: 0,
            unsupported_count: 0,
            invalid_count: 0,
            truncated: false,
            ecosystems: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SummaryEcosystemEntry {
    ecosystem: String,
    package_count: usize,
    direct_count: usize,
    duplicate_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SummaryEntry {
    project_id: String,
    revision: String,
    scanned_at_ms: u64,
    package_count: usize,
    direct_count: usize,
    transitive_count: usize,
    duplicate_count: usize,
    unresolved_dependency_count: usize,
    missing_lockfile_count: usize,
    stale_lockfile_count: usize,
    unsupported_count: usize,
    invalid_count: usize,
    truncated: bool,
    ecosystems: Vec<SummaryEcosystemEntry>,
}

/// Reads one selected profile's aggregate package state. Missing and corrupt
/// producer states are normal, distinguishable DTO states rather than raw IPC
/// failures. `canonical_project_key` is hashed before matching and never
/// returned to the renderer.
pub fn read_package_dependency_summary_in(
    root: &Path,
    profile_id: &str,
    canonical_project_key: &str,
    now_ms: u64,
) -> PackageDependencySummary {
    let expected_project_id =
        match devbox_integration::opaque_identity("project", canonical_project_key) {
            Ok(identity) => identity,
            Err(_) => {
                return PackageDependencySummary::unavailable(
                    profile_id,
                    PackageDependencyStatus::Corrupt,
                )
            }
        };
    let envelope = match devbox_integration::read_snapshot_in(root, PRODUCER, SNAPSHOT_VERSION) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => {
            return PackageDependencySummary::unavailable(
                profile_id,
                PackageDependencyStatus::Missing,
            )
        }
        Err(_) => {
            return PackageDependencySummary::unavailable(
                profile_id,
                PackageDependencyStatus::Corrupt,
            )
        }
    };
    let producer_version = envelope.producer_version.clone();
    let views = match envelope.views() {
        Ok(views) => views,
        Err(_) => {
            return PackageDependencySummary::unavailable(
                profile_id,
                PackageDependencyStatus::Corrupt,
            )
        }
    };
    let Some(view) = views.get(VIEW_KIND) else {
        return PackageDependencySummary::unavailable(profile_id, PackageDependencyStatus::Missing);
    };
    if view.schema_version != VIEW_VERSION
        || view.freshness_ms != 0
        || view.entries.len() > MAX_SUMMARY_ENTRIES
    {
        return PackageDependencySummary::unavailable(profile_id, PackageDependencyStatus::Corrupt);
    }

    let entries = view
        .entries
        .iter()
        .cloned()
        .map(serde_json::from_value::<SummaryEntry>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(entries) = entries else {
        return PackageDependencySummary::unavailable(profile_id, PackageDependencyStatus::Corrupt);
    };
    let mut project_ids = HashSet::new();
    if entries.iter().any(|entry| {
        !project_ids.insert(entry.project_id.as_str()) || !validate_entry(entry, now_ms)
    }) {
        return PackageDependencySummary::unavailable(profile_id, PackageDependencyStatus::Corrupt);
    }
    let Some(entry) = entries
        .into_iter()
        .find(|entry| entry.project_id == expected_project_id)
    else {
        return PackageDependencySummary::unavailable(profile_id, PackageDependencyStatus::Missing);
    };

    let freshness_ms = now_ms.saturating_sub(entry.scanned_at_ms);
    let status = if freshness_ms <= FRESH_MAX_MS {
        PackageDependencyStatus::Fresh
    } else if freshness_ms <= EXPIRED_AFTER_MS {
        PackageDependencyStatus::Stale
    } else {
        PackageDependencyStatus::Expired
    };
    PackageDependencySummary {
        profile_id: profile_id.to_owned(),
        source: SOURCE_LABEL,
        status,
        producer_version: Some(producer_version),
        freshness_ms: Some(freshness_ms),
        revision: Some(entry.revision),
        package_count: entry.package_count,
        direct_count: entry.direct_count,
        transitive_count: entry.transitive_count,
        duplicate_count: entry.duplicate_count,
        unresolved_dependency_count: entry.unresolved_dependency_count,
        missing_lockfile_count: entry.missing_lockfile_count,
        stale_lockfile_count: entry.stale_lockfile_count,
        unsupported_count: entry.unsupported_count,
        invalid_count: entry.invalid_count,
        truncated: entry.truncated,
        ecosystems: entry
            .ecosystems
            .into_iter()
            .map(|ecosystem| PackageDependencyEcosystem {
                ecosystem: ecosystem.ecosystem,
                package_count: ecosystem.package_count,
                direct_count: ecosystem.direct_count,
                duplicate_count: ecosystem.duplicate_count,
            })
            .collect(),
    }
}

fn validate_entry(entry: &SummaryEntry, now_ms: u64) -> bool {
    let project_id_valid = entry.project_id.len() == "project-".len() + 64
        && entry.project_id.starts_with("project-")
        && lower_hex(&entry.project_id["project-".len()..]);
    let revision_valid = entry.revision.len() == "sha256:".len() + 64
        && entry.revision.starts_with("sha256:")
        && lower_hex(&entry.revision["sha256:".len()..]);
    if !project_id_valid
        || !revision_valid
        || entry.scanned_at_ms > now_ms.saturating_add(MAX_CLOCK_SKEW_MS)
        || entry.package_count > MAX_PACKAGES
        || entry.direct_count > entry.package_count
        || entry.transitive_count != entry.package_count.saturating_sub(entry.direct_count)
        || entry.duplicate_count > entry.package_count
        || entry.unresolved_dependency_count > MAX_EDGES
        || entry.missing_lockfile_count > MAX_INPUT_FILES
        || entry.stale_lockfile_count > MAX_INPUT_FILES
        || entry.unsupported_count > MAX_INPUT_FILES
        || entry.invalid_count > MAX_INPUT_FILES
        || entry.ecosystems.len() > MAX_ECOSYSTEMS
    {
        return false;
    }
    let mut names = HashSet::new();
    let mut package_total = 0usize;
    let mut direct_total = 0usize;
    let mut duplicate_total = 0usize;
    for ecosystem in &entry.ecosystems {
        if !matches!(
            ecosystem.ecosystem.as_str(),
            "cargo" | "pnpm" | "npm" | "python" | "gradle"
        ) || !names.insert(ecosystem.ecosystem.as_str())
            || ecosystem.package_count > entry.package_count
            || ecosystem.direct_count > ecosystem.package_count
            || ecosystem.duplicate_count > ecosystem.package_count
        {
            return false;
        }
        let Some(next_packages) = package_total.checked_add(ecosystem.package_count) else {
            return false;
        };
        let Some(next_direct) = direct_total.checked_add(ecosystem.direct_count) else {
            return false;
        };
        let Some(next_duplicates) = duplicate_total.checked_add(ecosystem.duplicate_count) else {
            return false;
        };
        package_total = next_packages;
        direct_total = next_direct;
        duplicate_total = next_duplicates;
    }
    package_total == entry.package_count
        && direct_total == entry.direct_count
        && duplicate_total == entry.duplicate_count
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let serial = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-workbench-dependency-summary-{}-{label}-{serial}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn summary_entry(canonical_key: &str, scanned_at_ms: u64) -> Value {
        json!({
            "projectId": devbox_integration::opaque_identity("project", canonical_key).unwrap(),
            "revision": format!("sha256:{}", "a".repeat(64)),
            "scannedAtMs": scanned_at_ms,
            "packageCount": 4,
            "directCount": 2,
            "transitiveCount": 2,
            "duplicateCount": 1,
            "unresolvedDependencyCount": 1,
            "missingLockfileCount": 0,
            "staleLockfileCount": 1,
            "unsupportedCount": 1,
            "invalidCount": 0,
            "truncated": false,
            "ecosystems": [
                { "ecosystem": "cargo", "packageCount": 3, "directCount": 1, "duplicateCount": 1 },
                { "ecosystem": "gradle", "packageCount": 0, "directCount": 0, "duplicateCount": 0 },
                { "ecosystem": "npm", "packageCount": 1, "directCount": 1, "duplicateCount": 0 }
            ]
        })
    }

    fn write_snapshot(root: &Path, entries: Vec<Value>) {
        let mut views = SnapshotViews::new();
        views.insert(
            VIEW_KIND.to_owned(),
            SnapshotView {
                schema_version: VIEW_VERSION,
                freshness_ms: 0,
                entries,
            },
        );
        let envelope = Envelope::with_views(PRODUCER, "0.3.0", views);
        devbox_integration::write_atomic(
            &envelope,
            &devbox_integration::snapshot_dir_in(root, PRODUCER, SNAPSHOT_VERSION),
        )
        .unwrap();
    }

    #[test]
    fn matches_only_the_hashed_project_and_returns_aggregate_fields() {
        let root = TestRoot::new("match");
        write_snapshot(
            &root.0,
            vec![
                summary_entry("win:c:/projects/other", 900),
                summary_entry("win:c:/projects/devbox", 950),
            ],
        );

        let result = read_package_dependency_summary_in(
            &root.0,
            "profile-1",
            "win:c:/projects/devbox",
            1_000,
        );
        assert_eq!(result.status, PackageDependencyStatus::Fresh);
        assert_eq!(result.profile_id, "profile-1");
        assert_eq!(result.package_count, 4);
        assert_eq!(result.duplicate_count, 1);
        assert_eq!(result.freshness_ms, Some(50));
        assert_eq!(result.producer_version.as_deref(), Some("0.3.0"));
        assert!(!format!("{result:?}").contains("projects/devbox"));
    }

    #[test]
    fn distinguishes_missing_stale_expired_and_corrupt_states() {
        let missing = TestRoot::new("missing");
        assert_eq!(
            read_package_dependency_summary_in(&missing.0, "p", "key", 1).status,
            PackageDependencyStatus::Missing
        );

        let stale = TestRoot::new("stale");
        write_snapshot(&stale.0, vec![summary_entry("key", FRESH_MAX_MS + 1)]);
        assert_eq!(
            read_package_dependency_summary_in(
                &stale.0,
                "p",
                "key",
                FRESH_MAX_MS.saturating_mul(2).saturating_add(2),
            )
            .status,
            PackageDependencyStatus::Stale
        );

        let expired = TestRoot::new("expired");
        write_snapshot(&expired.0, vec![summary_entry("key", 1)]);
        assert_eq!(
            read_package_dependency_summary_in(&expired.0, "p", "key", EXPIRED_AFTER_MS + 2,)
                .status,
            PackageDependencyStatus::Expired
        );

        let corrupt = TestRoot::new("corrupt");
        let mut invalid = summary_entry("key", 1);
        invalid["privatePath"] = json!("DO_NOT_RETURN_PRIVATE_DETAIL");
        write_snapshot(&corrupt.0, vec![invalid]);
        let result = read_package_dependency_summary_in(&corrupt.0, "p", "key", 1);
        assert_eq!(result.status, PackageDependencyStatus::Corrupt);
        assert!(!format!("{result:?}").contains("DO_NOT_RETURN_PRIVATE_DETAIL"));
    }

    #[test]
    fn one_invalid_or_duplicate_entry_rejects_the_complete_view() {
        let root = TestRoot::new("complete-view");
        let first = summary_entry("key", 1);
        let mut invalid_other = summary_entry("other", 1);
        invalid_other["packageCount"] = json!(MAX_PACKAGES + 1);
        write_snapshot(&root.0, vec![first, invalid_other]);
        assert_eq!(
            read_package_dependency_summary_in(&root.0, "p", "key", 1).status,
            PackageDependencyStatus::Corrupt
        );

        let duplicates = TestRoot::new("duplicate");
        write_snapshot(
            &duplicates.0,
            vec![summary_entry("key", 1), summary_entry("key", 1)],
        );
        assert_eq!(
            read_package_dependency_summary_in(&duplicates.0, "p", "key", 1).status,
            PackageDependencyStatus::Corrupt
        );
    }
}
