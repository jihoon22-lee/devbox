//! Pure parsing of bounded native Git ref listings.
use serde::Serialize;
pub const FOR_EACH_REF_FORMAT: &str = "%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(worktreepath)%00%(committerdate:unix)%00%(contents:subject)";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub remote: bool,
    pub commit: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub worktree: Option<String>,
    pub committed_at: i64,
    pub subject: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BranchList {
    pub current: Option<String>,
    pub detached: bool,
    pub branches: Vec<Branch>,
    pub truncated: bool,
}
pub fn parse_track(value: &str) -> (u32, u32) {
    let mut counts = (0, 0);
    for part in value.split(", ") {
        if let Some(count) = part.strip_prefix("ahead ") {
            counts.0 = count.parse().unwrap_or(0);
        } else if let Some(count) = part.strip_prefix("behind ") {
            counts.1 = count.parse().unwrap_or(0);
        }
    }
    counts
}
pub fn parse_refs(output: &str, _current: Option<&str>) -> Result<Vec<Branch>, String> {
    let mut branches = Vec::new();
    for line in output.lines() {
        let fields = line.split('\0').collect::<Vec<_>>();
        let [reference, commit, upstream, track, worktree, time, subject] = fields.as_slice()
        else {
            return Err("branch_operation_failed".into());
        };
        let (name, remote) = if let Some(name) = reference.strip_prefix("refs/heads/") {
            (name, false)
        } else if let Some(name) = reference.strip_prefix("refs/remotes/") {
            if name.ends_with("/HEAD") {
                continue;
            }
            (name, true)
        } else {
            return Err("branch_operation_failed".into());
        };
        if name.is_empty() || commit.is_empty() {
            return Err("branch_operation_failed".into());
        }
        let (ahead, behind) = parse_track(track);
        branches.push(Branch {
            name: name.into(),
            remote,
            commit: (*commit).into(),
            upstream: (!upstream.is_empty()).then(|| (*upstream).into()),
            ahead,
            behind,
            worktree: (!worktree.is_empty()).then(|| (*worktree).into()),
            committed_at: time.parse().map_err(|_| "branch_operation_failed")?,
            subject: (*subject).into(),
        });
    }
    Ok(branches)
}

#[cfg(test)]
mod tests {
    use super::*;
    const REFS: &str = concat!(
        "refs/heads/main\0aaaa\0origin/main\0ahead 2, behind 1\0/home/me/devbox\x001790000000\0Merge agent/fix\n",
        "refs/heads/agent/fix\0bbbb\0\0\0/home/me/devbox-fix\x001790000100\0agent change\n",
        "refs/remotes/origin/main\0cccc\0\0\0\x001789999000\0upstream\n",
        "refs/remotes/origin/HEAD\0cccc\0\0\0\x001789999000\0upstream\n",
    );
    #[test]
    fn refs_parse_into_local_and_remote_branches() {
        let branches = parse_refs(REFS, Some("main")).unwrap();
        assert_eq!(branches.len(), 3);
        assert_eq!(
            (
                branches[0].name.as_str(),
                branches[0].ahead,
                branches[0].behind
            ),
            ("main", 2, 1)
        );
        assert_eq!(branches[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!(branches[1].worktree.as_deref(), Some("/home/me/devbox-fix"));
        assert!(branches[2].remote && branches[2].name == "origin/main");
    }
    #[test]
    fn tracking_text_variants() {
        for (text, expected) in [
            ("", (0, 0)),
            ("ahead 3", (3, 0)),
            ("behind 4", (0, 4)),
            ("gone", (0, 0)),
        ] {
            assert_eq!(parse_track(text), expected);
        }
    }
    #[test]
    fn malformed_records_are_rejected_and_detached_is_supported() {
        assert!(parse_refs("refs/heads/main\0only-two-fields\n", None).is_err());
        assert!(parse_refs("refs/tags/v1\0a\0\0\0\x001\0s\n", None).is_err());
        assert!(parse_refs("refs/heads/x\0a\0\0\0\0bad-time\0s\n", None).is_err());
        assert_eq!(parse_refs(REFS, None).unwrap().len(), 3);
        assert!(parse_refs("", None).unwrap().is_empty());
    }
}
