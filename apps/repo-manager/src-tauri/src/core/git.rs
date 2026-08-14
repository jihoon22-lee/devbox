//! git 출력 파싱 (순수). branch·dirty·ahead/behind.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchState {
    pub current: String,
    pub ahead: i64,
    pub behind: i64,
    pub dirty: bool,
    pub detached: bool,
}

/// `git status --porcelain --branch` 출력 파싱.
pub fn parse_status(path: &str, input: &str) -> RepoSnapshot {
    let mut branch = "(detached)".to_string();
    let mut ahead = 0i64;
    let mut behind = 0i64;
    let mut changes = 0u32;
    let mut detached = false;
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if rest.starts_with("HEAD (no branch)") || rest.starts_with("HEAD detached") {
                detached = true;
                branch = "(detached)".to_string();
            } else {
                let base = rest.split("...").next().unwrap_or(rest);
                branch = base.to_string();
            }
            // ahead/behind 추출: `...origin/main [ahead 2, behind 1]`
            if let Some(bracket) = rest.split('[').nth(1) {
                for part in bracket.trim_end_matches(']').split(',') {
                    let part = part.trim();
                    if let Some(n) = part.strip_prefix("ahead ") {
                        ahead = n.trim().parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix("behind ") {
                        behind = n.trim().parse().unwrap_or(0);
                    }
                }
            }
        } else if !line.trim().is_empty() {
            changes += 1;
        }
    }
    RepoSnapshot {
        path: path.to_string(),
        branch: BranchState {
            current: branch,
            ahead,
            behind,
            dirty: changes > 0,
            detached,
        },
        changes,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepoSnapshot {
    pub path: String,
    pub branch: BranchState,
    pub changes: u32,
}

/// worktree 목록 (`git worktree list --porcelain`).
pub fn parse_worktrees(input: &str) -> Vec<String> {
    input
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(|p| p.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_branch() {
        let s = parse_status("C:/p", "## main...origin/main\n");
        assert_eq!(s.branch.current, "main");
        assert!(!s.branch.dirty);
        assert_eq!(s.changes, 0);
    }

    #[test]
    fn parses_ahead_behind() {
        let s = parse_status("C:/p", "## dev...origin/dev [ahead 2, behind 1]\n M a.rs\n");
        assert_eq!(s.branch.ahead, 2);
        assert_eq!(s.branch.behind, 1);
        assert!(s.branch.dirty);
        assert_eq!(s.changes, 1);
    }

    #[test]
    fn parses_detached() {
        let s = parse_status("C:/p", "## HEAD (no branch)\n");
        assert!(s.branch.detached);
    }

    #[test]
    fn parses_worktrees() {
        let out = "worktree C:/a\nHEAD 123\n\nworktree C:/b\nHEAD 456\n";
        assert_eq!(parse_worktrees(out), vec!["C:/a", "C:/b"]);
    }
}
