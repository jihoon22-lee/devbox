//! Life Log의 `projects/v1` snapshot용 privacy-safe 집계.
//!
//! 사용자에게 등록된 절대 프로젝트 경로와 최근 활동의 숫자 요약만 반환한다.
//! 세션의 app/title 원문은 귀속 과정에서만 사용하고 결과 구조에는 넣지 않는다.

use crate::core::attribution::{attribute_title, ProjectMatch};
use crate::core::db;
use devbox_filesystem::parse_safe_project_path;
#[cfg(test)]
use devbox_filesystem::MAX_PROJECT_PATH_BYTES;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const RECENT_ACTIVITY_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_PROJECTS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshotEntry {
    pub path: String,
    pub activity_window_start_ms: i64,
    pub last_activity_at_ms: Option<i64>,
    pub recent_session_count: u64,
    pub recent_duration_ms: i64,
}

#[derive(Debug)]
struct SafeProject {
    path: String,
    name: String,
    identity: String,
}

#[derive(Debug, Default)]
struct Activity {
    last_at_ms: Option<i64>,
    sessions: u64,
    duration_ms: i64,
}

/// 현재 설정과 최근 세션으로 bounded project summary를 만든다.
pub fn build_entries(conn: &Connection, now_ms: i64) -> Result<Vec<ProjectSnapshotEntry>, String> {
    let projects = safe_projects(&db::get_setting(conn, "projects", ""));
    if projects.is_empty() {
        return Ok(Vec::new());
    }

    let window_start = now_ms.saturating_sub(RECENT_ACTIVITY_WINDOW_MS).max(0);
    let profiles = unambiguous_profiles(&projects);
    let mut activity: HashMap<String, Activity> = projects
        .iter()
        .map(|project| (project.path.clone(), Activity::default()))
        .collect();

    let mut statement = conn
        .prepare(
            "SELECT substr(title, 1, 4096), start_ts, end_ts, duration_ms
             FROM sessions
             WHERE start_ts >= ?1 AND start_ts < ?2
             ORDER BY start_ts",
        )
        .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?;
    let mut rows = statement
        .query(rusqlite::params![window_start, now_ms])
        .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?;

    while let Some(row) = rows
        .next()
        .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?
    {
        let title: String = row
            .get(0)
            .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?;
        let start_ts: i64 = row
            .get(1)
            .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?;
        let end_ts: i64 = row
            .get(2)
            .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?;
        let duration_ms: i64 = row
            .get(3)
            .map_err(|_| "최근 프로젝트 활동을 조회할 수 없습니다".to_string())?;

        let bounded_end = end_ts.clamp(window_start, now_ms);
        let bounded_start = start_ts.max(window_start);
        if bounded_end <= bounded_start {
            continue;
        }
        let bounded_duration = duration_ms
            .max(0)
            .min(bounded_end.saturating_sub(bounded_start));

        let Some(project) = attribute_title(&title, &profiles) else {
            continue;
        };
        let Some(summary) = activity.get_mut(project.project_id.as_str()) else {
            continue;
        };
        summary.sessions = summary.sessions.saturating_add(1);
        summary.duration_ms = summary.duration_ms.saturating_add(bounded_duration);
        summary.last_at_ms = Some(
            summary
                .last_at_ms
                .map_or(bounded_end, |last| last.max(bounded_end)),
        );
    }

    let mut entries = projects
        .into_iter()
        .map(|project| {
            let summary = activity.remove(&project.path).unwrap_or_default();
            ProjectSnapshotEntry {
                path: project.path,
                activity_window_start_ms: window_start,
                last_activity_at_ms: summary.last_at_ms,
                recent_session_count: summary.sessions,
                recent_duration_ms: summary.duration_ms,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .last_activity_at_ms
            .cmp(&left.last_activity_at_ms)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(entries)
}

fn safe_projects(raw: &str) -> Vec<SafeProject> {
    let mut identities = HashSet::new();
    raw.lines()
        .filter_map(safe_project)
        .filter(|project| identities.insert(project.identity.clone()))
        .take(MAX_PROJECTS)
        .collect()
}

fn safe_project(raw: &str) -> Option<SafeProject> {
    let path = parse_safe_project_path(raw)?;
    Some(SafeProject {
        path: path.as_str().to_string(),
        name: path.name().to_string(),
        identity: path.identity().to_string(),
    })
}

/// 같은 basename이 여러 경로에 있으면 창 제목만으로 구분할 수 없으므로 모두 미귀속한다.
fn unambiguous_profiles(projects: &[SafeProject]) -> Vec<ProjectMatch> {
    let mut frequencies = HashMap::new();
    for project in projects {
        *frequencies
            .entry(project.name.to_lowercase())
            .or_insert(0usize) += 1;
    }
    projects
        .iter()
        .filter(|project| frequencies[&project.name.to_lowercase()] == 1)
        .map(|project| ProjectMatch {
            project_id: project.path.clone(),
            basenames: vec![project.name.clone()],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::ClosedSession;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        connection
    }

    fn session(connection: &Connection, title: &str, start: i64, end: i64) {
        db::insert_session(
            connection,
            &ClosedSession {
                app: "Code".into(),
                title: title.into(),
                start_ts: start,
                end_ts: end,
            },
        )
        .unwrap();
    }

    #[test]
    fn emits_safe_absolute_projects_and_deduplicates_windows_spelling() {
        let connection = database();
        db::set_setting(
            &connection,
            "projects",
            "C:\\work\\devbox\\\nc:/work/devbox\n/mnt/e/projects/api\nrelative/path\nC:\\work\\..\\escape\n/",
        );

        let entries = build_entries(&connection, RECENT_ACTIVITY_WINDOW_MS).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "/mnt/e/projects/api");
        assert_eq!(entries[1].path, "C:\\work\\devbox");
    }

    #[test]
    fn aggregates_only_recent_activity_without_exporting_titles() {
        let connection = database();
        db::set_setting(&connection, "projects", "C:\\work\\devbox");
        let now = RECENT_ACTIVITY_WINDOW_MS + 10_000;
        session(&connection, "devbox — normal", now - 9_000, now - 8_000);
        session(
            &connection,
            "devbox — Bearer raw-credential-must-not-leak",
            now - 7_000,
            now - 5_000,
        );
        session(&connection, "other", now - 4_000, now - 3_000);
        session(&connection, "devbox — old", 1, 2);

        let entries = build_entries(&connection, now).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].last_activity_at_ms, Some(now - 5_000));
        assert_eq!(entries[0].recent_session_count, 2);
        assert_eq!(entries[0].recent_duration_ms, 3_000);
        let serialized = serde_json::to_string(&entries).unwrap();
        assert!(!serialized.contains("Bearer"));
        assert!(!serialized.contains("raw-credential"));
        assert!(!serialized.contains("normal"));
        assert!(!serialized.contains("Code"));
    }

    #[test]
    fn longest_unique_basename_wins_and_duplicate_names_stay_unattributed() {
        let connection = database();
        db::set_setting(
            &connection,
            "projects",
            "C:\\work\\devbox\nC:\\work\\devbox-api\nD:\\one\\shared\nE:\\two\\shared",
        );
        let now = RECENT_ACTIVITY_WINDOW_MS + 10_000;
        session(&connection, "devbox-api — lib.rs", now - 3_000, now - 2_000);
        session(&connection, "shared — ambiguous", now - 2_000, now - 1_000);

        let entries = build_entries(&connection, now).unwrap();
        let api = entries
            .iter()
            .find(|entry| entry.path.ends_with("devbox-api"))
            .unwrap();
        assert_eq!(api.recent_session_count, 1);
        assert!(entries
            .iter()
            .filter(|entry| entry.path.ends_with("shared"))
            .all(|entry| entry.recent_session_count == 0));
    }

    #[test]
    fn clamps_future_activity_and_skips_reversed_intervals() {
        let connection = database();
        db::set_setting(&connection, "projects", "C:\\work\\devbox");
        let now = RECENT_ACTIVITY_WINDOW_MS + 10_000;
        session(
            &connection,
            "devbox — future end",
            now - 1_000,
            now + 10_000,
        );
        session(&connection, "devbox — reversed", now - 500, now - 1_000);

        let entries = build_entries(&connection, now).unwrap();
        assert_eq!(entries[0].last_activity_at_ms, Some(now));
        assert_eq!(entries[0].recent_session_count, 1);
        assert_eq!(entries[0].recent_duration_ms, 1_000);
    }

    #[test]
    fn malformed_sessions_schema_returns_a_safe_failure() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO settings VALUES ('projects', 'C:\\work\\devbox');",
            )
            .unwrap();
        assert_eq!(
            build_entries(&connection, RECENT_ACTIVITY_WINDOW_MS).unwrap_err(),
            "최근 프로젝트 활동을 조회할 수 없습니다"
        );
    }

    #[test]
    fn rejects_device_unc_roots_controls_and_windows_alias_components() {
        for path in [
            "\\\\?\\C:\\project",
            "\\\\.\\C:\\project",
            "//?/C:/project",
            "//./C:/project",
            "\\\\server\\share",
            "C:\\project.\\child",
            "C:\\project \\child",
            "C:\\project:stream\\child",
            "C:\\wild*card\\child",
            "C:\\CON.txt\\child",
            "C:\\LPT1\\child",
            "C:\\bad\0path",
        ] {
            assert!(safe_project(path).is_none(), "accepted {path:?}");
        }
        assert!(safe_project("\\\\server\\share\\project").is_some());
        assert!(safe_project(&format!("/{}", "x".repeat(MAX_PROJECT_PATH_BYTES))).is_none());
    }

    #[test]
    fn bounds_the_number_of_published_projects() {
        let raw = (0..=MAX_PROJECTS)
            .map(|index| format!("/projects/project-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let projects = safe_projects(&raw);
        assert_eq!(projects.len(), MAX_PROJECTS);
        assert_eq!(projects.first().unwrap().path, "/projects/project-0");
        assert_eq!(
            projects.last().unwrap().path,
            format!("/projects/project-{}", MAX_PROJECTS - 1)
        );
    }
}
