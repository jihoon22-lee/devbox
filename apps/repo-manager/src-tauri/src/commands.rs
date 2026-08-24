//! Repo Manager command — 저장소 탐색·상태·worktree.

use crate::core::git::{parse_status, parse_worktrees, RepoSnapshot};
use crate::core::open_targets::{select_repo_open_targets, RepoOpenTarget};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoEntry {
    pub path: String,
    pub canonical_key: String,
    pub has_worktrees: bool,
}

/// 탐색 깊이 상한. 순환(예: Windows junction) 방어와 병목 방지 둘 다 겸한다 —
/// 재귀가 사이클을 타도 depth는 호출마다 증가하므로 결국 이 상한에서 멎는다.
const MAX_SCAN_DEPTH: usize = 12;
/// 방문 디렉터리 총량 상한. 얕지만 매우 넓은 트리(수만 개 형제 디렉터리)를 방어한다.
const MAX_VISITED_DIRS: usize = 20_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub repos: Vec<RepoEntry>,
    /// 깊이·방문 상한에 걸려 일부 디렉터리를 건너뛰었으면 true (조용한 누락 금지).
    pub truncated: bool,
}

/// root 아래 Git repository를 재귀 탐색한다 (canonical identity로 중복 제거).
/// node_modules·target·AppData 등 흔한 비-repo 디렉터리는 진입 전에 가지치기한다.
#[tauri::command]
pub fn scan_root(root: String) -> Result<ScanResult, String> {
    let mut repos = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;
    walk(
        Path::new(&root),
        &mut repos,
        0,
        &mut visited,
        &mut truncated,
    );
    // canonical key로 중복 제거
    let mut seen = std::collections::HashMap::new();
    for entry in repos {
        let key = entry.canonical_key.clone();
        seen.entry(key).or_insert(entry);
    }
    let mut out: Vec<RepoEntry> = seen.into_values().collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanResult {
        repos: out,
        truncated,
    })
}

fn walk(
    dir: &Path,
    out: &mut Vec<RepoEntry>,
    depth: usize,
    visited: &mut usize,
    truncated: &mut bool,
) {
    if *visited >= MAX_VISITED_DIRS {
        *truncated = true;
        return;
    }
    *visited += 1;

    if dir.join(".git").exists() {
        let canonical_key =
            devbox_wsl::path::canonical_project_key(Some(&dir.to_string_lossy()), None)
                .unwrap_or_else(|_| dir.to_string_lossy().into_owned());
        let worktrees = dir.join(".git").join("worktrees").is_dir();
        out.push(RepoEntry {
            path: dir.to_string_lossy().into_owned(),
            canonical_key,
            has_worktrees: worktrees,
        });
        return; // 중첩 repo는 건너뛴다
    }
    if depth >= MAX_SCAN_DEPTH {
        *truncated = true;
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if devbox_filesystem::is_ignored_dir(name) {
                continue;
            }
        }
        walk(&path, out, depth + 1, visited, truncated);
    }
}

fn git(args: &[&str], cwd: &str) -> Result<String, String> {
    devbox_git::run(args, cwd)
}

#[tauri::command]
pub async fn repo_status(path: String) -> Result<RepoSnapshot, String> {
    let status = git(&["status", "--porcelain", "--branch"], &path)?;
    Ok(parse_status(&path, &status))
}

#[tauri::command]
pub async fn worktrees(path: String) -> Result<Vec<String>, String> {
    let out = git(&["worktree", "list", "--porcelain"], &path)?;
    Ok(parse_worktrees(&out))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreate {
    pub path: String,
}

#[tauri::command]
pub async fn create_worktree(
    repo_path: String,
    branch: String,
    target_dir: String,
) -> Result<WorktreeCreate, String> {
    git(&["worktree", "add", "-b", &branch, &target_dir], &repo_path)?;
    Ok(WorktreeCreate { path: target_dir })
}

/// remove 전 uncommitted/untracked 검사. 없으면 true.
#[tauri::command]
pub async fn worktree_clean(path: String) -> Result<bool, String> {
    let status = git(&["status", "--porcelain"], &path)?;
    Ok(status.trim().is_empty())
}

fn available_open_targets() -> Vec<RepoOpenTarget> {
    select_repo_open_targets(
        "repo-manager",
        devbox_launch::installed_targets("path"),
        devbox_launch::installed_targets("workspace"),
    )
}

/// Catalog capability와 실제 설치 executable의 교집합만 반환한다. executable
/// 경로는 frontend에 노출하지 않는다.
#[tauri::command]
pub fn open_targets() -> Vec<RepoOpenTarget> {
    available_open_targets()
}

fn validated_repository_path(path: &str) -> Result<String, &'static str> {
    let raw = Path::new(path);
    if path.is_empty()
        || !raw.is_absolute()
        || path
            .split(['/', '\\'])
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err("repository 경로가 올바르지 않습니다");
    }
    let canonical = raw
        .canonicalize()
        .map_err(|_| "repository를 찾을 수 없습니다")?;
    if !canonical.is_dir() || !canonical.join(".git").exists() {
        return Err("repository를 찾을 수 없습니다");
    }
    Ok(path.to_string())
}

#[tauri::command]
pub fn open_in(app_id: String, path: String) -> Result<(), String> {
    let app_id = app_id.to_lowercase();
    let target = available_open_targets()
        .into_iter()
        .find(|target| target.id == app_id)
        .ok_or_else(|| "사용 가능한 대상 앱이 아닙니다".to_string())?;
    let path = validated_repository_path(&path).map_err(str::to_string)?;
    let req = target.request(path);
    devbox_launch::launch_open(&target.id, &req).map(|_| ())
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use std::fs;

    #[test]
    fn validated_repository_path_accepts_only_existing_absolute_git_directories() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(tmp.path());
        let path = tmp.path().to_string_lossy().into_owned();

        assert_eq!(validated_repository_path(&path), Ok(path.clone()));
        assert_eq!(
            validated_repository_path("relative/repository"),
            Err("repository 경로가 올바르지 않습니다")
        );
        assert_eq!(
            validated_repository_path(&format!("{path}/child/../repository")),
            Err("repository 경로가 올바르지 않습니다")
        );

        let non_repo = tempfile::tempdir().unwrap();
        assert_eq!(
            validated_repository_path(&non_repo.path().to_string_lossy()),
            Err("repository를 찾을 수 없습니다")
        );

        let secret = "repo-path-secret-must-not-appear";
        let error = validated_repository_path(&format!("{path}/{secret}"))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "repository를 찾을 수 없습니다");
        assert!(!error.contains(secret));
    }

    fn init_git_dir(dir: &Path) {
        fs::create_dir_all(dir.join(".git")).unwrap();
    }

    #[test]
    fn finds_repo_at_root() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(tmp.path());
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(result.repos.len(), 1);
        assert!(!result.truncated);
    }

    #[test]
    fn prunes_ignored_dirs_before_recursing() {
        let tmp = tempfile::tempdir().unwrap();
        // node_modules 아래 숨은 .git은 "발견"이 아니라 "가지치기"돼야 한다 — 이 트리를 실제로
        // 걸어 들어가면(진짜 프로젝트의 node_modules는 수만 개 파일이라) 느려지거나 멎는다.
        init_git_dir(&tmp.path().join("node_modules/some-pkg"));
        init_git_dir(&tmp.path().join("real-repo"));
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        let paths: Vec<&str> = result.repos.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(result.repos.len(), 1);
        assert!(paths.iter().any(|p| p.ends_with("real-repo")));
        assert!(!result.truncated);
    }

    #[test]
    fn depth_cap_stops_and_marks_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        // MAX_SCAN_DEPTH(12)를 넘는 중첩 아래에 repo를 둔다.
        let mut deep = tmp.path().to_path_buf();
        for i in 0..(MAX_SCAN_DEPTH + 3) {
            deep = deep.join(format!("d{i}"));
        }
        init_git_dir(&deep);
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert!(result.repos.is_empty(), "상한 밖의 repo는 발견되면 안 된다");
        assert!(
            result.truncated,
            "상한에 걸렸으면 truncated=true여야 한다 (조용한 누락 금지)"
        );
    }

    #[test]
    fn shallow_tree_within_limits_is_not_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(&tmp.path().join("a/b/c"));
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(result.repos.len(), 1);
        assert!(!result.truncated);
    }
}
