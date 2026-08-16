//! Repo Manager command — 저장소 탐색·상태·worktree.

use crate::core::git::{parse_status, parse_worktrees, RepoSnapshot};
use serde::Serialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoEntry {
    pub path: String,
    pub canonical_key: String,
    pub has_worktrees: bool,
}

/// root 아래 Git repository를 재귀 탐색한다 (canonical identity로 중복 제거).
#[tauri::command]
pub fn scan_root(root: String) -> Result<Vec<RepoEntry>, String> {
    let mut repos = Vec::new();
    walk(Path::new(&root), &mut repos);
    // canonical key로 중복 제거
    let mut seen = std::collections::HashMap::new();
    for entry in repos {
        let key = entry.canonical_key.clone();
        seen.entry(key).or_insert(entry);
    }
    let mut out: Vec<RepoEntry> = seen.into_values().collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<RepoEntry>) {
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
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        }
    }
}

async fn git(args: &[&str], cwd: &str) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", cwd]).args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000);
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[tauri::command]
pub async fn repo_status(path: String) -> Result<RepoSnapshot, String> {
    let status = git(&["status", "--porcelain", "--branch"], &path).await?;
    Ok(parse_status(&path, &status))
}

#[tauri::command]
pub async fn worktrees(path: String) -> Result<Vec<String>, String> {
    let out = git(&["worktree", "list", "--porcelain"], &path).await?;
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
    let out = git(&["worktree", "add", "-b", &branch, &target_dir], &repo_path).await?;
    let _ = out;
    Ok(WorktreeCreate { path: target_dir })
}

/// remove 전 uncommitted/untracked 검사. 없으면 true.
#[tauri::command]
pub async fn worktree_clean(path: String) -> Result<bool, String> {
    let status = git(&["status", "--porcelain"], &path).await?;
    Ok(status.trim().is_empty())
}

/// Code Pad / WSL Desktop / Workbench로 연다 (best-effort).
/// app_id는 카탈로그 id(code-pad·wsl-desktop·workbench). 설치된 exe 경로는
/// 공용 `crates/launch`가 Manager 설치 layout에서 해석한다.
#[tauri::command]
pub fn open_in(app_id: String, path: String) -> Result<(), String> {
    let app_id = app_id.to_lowercase();
    if !matches!(app_id.as_str(), "code-pad" | "wsl-desktop" | "workbench") {
        return Err("알 수 없는 앱".into());
    }
    devbox_launch::launch(&app_id, &[&path]).map(|_| ())
}
