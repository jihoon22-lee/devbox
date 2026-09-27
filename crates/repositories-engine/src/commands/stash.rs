//! Stash operations run through the admitted repository and shared Git lock.
use super::*;
pub use crate::core::stash::StashEntry;
use crate::core::stash::{parse_list, STASH_FORMAT};
const FAILED: &str = "stash_operation_failed";
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StashPushRequest {
    pub path: String,
    pub message: Option<String>,
    pub include_untracked: bool,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StashApplyRequest {
    pub path: String,
    pub index: u32,
    pub pop: bool,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StashDropRequest {
    pub path: String,
    pub index: u32,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StashStoreRequest {
    pub path: String,
    pub commit: String,
    pub message: String,
    pub operation_id: String,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StashApplyResult {
    pub applied: bool,
    pub conflicts: Vec<String>,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DroppedStash {
    pub commit: String,
    pub message: String,
}
fn read(root: &Path, args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    run_git_status_bounded_with_cancel(
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        root,
        cancel,
    )
    .map_err(|_| FAILED.into())
}
fn mutate(root: &Path, args: &[&str], cancel: &AtomicBool) -> Result<(), String> {
    run_git_mutation_with_cancel(
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        root,
        cancel,
    )
    .map_err(|_| FAILED.into())
}
fn entries(
    root: &Path,
    cancel: &AtomicBool,
    index: Option<u32>,
) -> Result<Vec<StashEntry>, String> {
    let format = format!("--format={STASH_FORMAT}");
    let skip = format!("--skip={}", index.unwrap_or(0));
    let max = if index.is_some() {
        "--max-count=1"
    } else {
        "--max-count=500"
    };
    parse_list(&read(
        root,
        &["stash", "list", &format, max, &skip],
        cancel,
    )?)
}
fn selected(root: &Path, cancel: &AtomicBool, index: u32) -> Result<StashEntry, String> {
    entries(root, cancel, Some(index))?
        .into_iter()
        .find(|entry| entry.index == index)
        .ok_or_else(|| "stash_missing".into())
}
fn validate_index(index: u32) -> Result<(), String> {
    if index > 999 {
        Err("stash_missing".into())
    } else {
        Ok(())
    }
}
async fn change<T: Send + 'static>(
    path: String,
    operation_id: String,
    action: impl FnOnce(&Path, &AtomicBool) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let operation = begin_git_operation(&operation_id, FAILED, FAILED)?;
    spawn_git_task(FAILED, move || {
        let mut operation = operation;
        let context = validated_repository_context(&path, FAILED)?;
        operation.bind_repository(context.common_git_identity, FAILED, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        action(&context.worktree, operation.cancellation.as_ref())
    })
    .await
}
pub async fn repo_stash_list(request: PathRequest) -> Result<Vec<StashEntry>, String> {
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let result = entries(&context.worktree, &AtomicBool::new(false), None)?;
        revalidate_repository_context(&context, FAILED)?;
        Ok(result)
    })
    .await
}
pub async fn repo_stash_push(request: StashPushRequest) -> Result<(), String> {
    if let Some(message) = &request.message {
        if message.len() > 200 {
            return Err(FAILED.into());
        }
        validate_commit_message(message).map_err(|_| FAILED)?;
    }
    change(request.path, request.operation_id, move |root, cancel| {
        let untracked = if request.include_untracked {
            "--untracked-files=all"
        } else {
            "--untracked-files=no"
        };
        if read(root, &["status", "--porcelain", "-z", untracked], cancel)?.is_empty() {
            return Err("stash_empty".into());
        }
        let mut args = vec!["stash", "push"];
        if request.include_untracked {
            args.push("--include-untracked");
        }
        if let Some(message) = &request.message {
            args.extend(["--message", message]);
        }
        mutate(root, &args, cancel)
    })
    .await
}
pub async fn repo_stash_apply(request: StashApplyRequest) -> Result<StashApplyResult, String> {
    validate_index(request.index)?;
    change(request.path, request.operation_id, move |root, cancel| {
        selected(root, cancel, request.index)?;
        let selector = format!("stash@{{{}}}", request.index);
        if mutate(
            root,
            &[
                "stash",
                if request.pop { "pop" } else { "apply" },
                &selector,
            ],
            cancel,
        )
        .is_ok()
        {
            return Ok(StashApplyResult {
                applied: true,
                conflicts: vec![],
            });
        }
        let output = read(
            root,
            &["diff", "--name-only", "--diff-filter=U", "-z"],
            cancel,
        )?;
        let conflicts = output
            .split_terminator('\0')
            .filter(|path| !path.is_empty())
            .take(200)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if conflicts.is_empty() {
            Err(FAILED.into())
        } else {
            Ok(StashApplyResult {
                applied: false,
                conflicts,
            })
        }
    })
    .await
}
pub async fn repo_stash_drop(request: StashDropRequest) -> Result<DroppedStash, String> {
    validate_index(request.index)?;
    change(request.path, request.operation_id, move |root, cancel| {
        let entry = selected(root, cancel, request.index)?;
        mutate(
            root,
            &["stash", "drop", &format!("stash@{{{}}}", request.index)],
            cancel,
        )?;
        Ok(DroppedStash {
            commit: entry.commit,
            message: entry.message,
        })
    })
    .await
}
pub async fn repo_stash_store(request: StashStoreRequest) -> Result<(), String> {
    validate_commit_id(&request.commit).map_err(|_| FAILED)?;
    validate_commit_message(&request.message).map_err(|_| FAILED)?;
    change(request.path, request.operation_id, move |root, cancel| {
        mutate(
            root,
            &[
                "stash",
                "store",
                "--message",
                &request.message,
                &request.commit,
            ],
            cancel,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    use std::fs;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        crate::runtime::block_on(future)
    }
    fn path(dir: &std::path::Path) -> String {
        host_path_spelling(&dir.canonicalize().unwrap(), "stash_operation_failed").unwrap()
    }

    #[test]
    fn push_list_pop_round_trip_including_untracked_files() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "edit\n").unwrap();
        fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
        block(repo_stash_push(StashPushRequest {
            path: path(tmp.path()),
            message: Some("wip login".into()),
            include_untracked: true,
            operation_id: "s1".into(),
        }))
        .unwrap();
        assert!(git(tmp.path(), &["status", "--porcelain"]).is_empty());
        let list = block(repo_stash_list(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert!(list[0].message.ends_with("wip login"));
        let result = block(repo_stash_apply(StashApplyRequest {
            path: path(tmp.path()),
            index: 0,
            pop: true,
            operation_id: "s2".into(),
        }))
        .unwrap();
        assert!(result.applied && result.conflicts.is_empty());
        assert_eq!(
            fs::read_to_string(tmp.path().join("new.txt")).unwrap(),
            "untracked\n"
        );
        assert!(block(repo_stash_list(PathRequest {
            path: path(tmp.path())
        }))
        .unwrap()
        .is_empty());
        assert_eq!(
            block(repo_stash_push(StashPushRequest {
                path: path(tmp.path()),
                message: None,
                include_untracked: false,
                operation_id: "s3".into()
            }))
            .map(|_| ()),
            Ok(())
        );
    }

    #[test]
    fn message_controls_and_untracked_exclusion_are_explicit() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
        assert_eq!(
            block(repo_stash_push(StashPushRequest {
                path: path(tmp.path()),
                message: None,
                include_untracked: false,
                operation_id: "untracked-only".into()
            }))
            .unwrap_err(),
            "stash_empty"
        );
        fs::write(tmp.path().join("README.md"), "tracked\n").unwrap();
        block(repo_stash_push(StashPushRequest {
            path: path(tmp.path()),
            message: Some("first line\nsecond line".into()),
            include_untracked: false,
            operation_id: "multiline-stash".into(),
        }))
        .unwrap();
        assert!(tmp.path().join("new.txt").exists());
        assert_eq!(
            block(repo_stash_list(PathRequest {
                path: path(tmp.path())
            }))
            .unwrap()
            .len(),
            1
        );
    }
    #[test]
    fn clean_tree_has_nothing_to_stash() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        assert_eq!(
            block(repo_stash_push(StashPushRequest {
                path: path(tmp.path()),
                message: None,
                include_untracked: false,
                operation_id: "c1".into()
            }))
            .unwrap_err(),
            "stash_empty"
        );
    }

    #[test]
    fn conflicting_pop_keeps_the_stash_and_reports_files() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "stashed\n").unwrap();
        git(tmp.path(), &["stash", "push", "--quiet"]);
        fs::write(tmp.path().join("README.md"), "committed\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "conflicting"]);
        let result = block(repo_stash_apply(StashApplyRequest {
            path: path(tmp.path()),
            index: 0,
            pop: true,
            operation_id: "p1".into(),
        }))
        .unwrap();
        assert!(!result.applied);
        assert_eq!(result.conflicts, vec!["README.md".to_string()]);
        assert_eq!(
            block(repo_stash_list(PathRequest {
                path: path(tmp.path())
            }))
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn dropped_stash_can_be_stored_back() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "keep me\n").unwrap();
        git(tmp.path(), &["stash", "push", "--quiet", "-m", "keep"]);
        let dropped = block(repo_stash_drop(StashDropRequest {
            path: path(tmp.path()),
            index: 0,
            operation_id: "r1".into(),
        }))
        .unwrap();
        assert!(block(repo_stash_list(PathRequest {
            path: path(tmp.path())
        }))
        .unwrap()
        .is_empty());
        block(repo_stash_store(StashStoreRequest {
            path: path(tmp.path()),
            commit: dropped.commit.clone(),
            message: dropped.message.clone(),
            operation_id: "r2".into(),
        }))
        .unwrap();
        let list = block(repo_stash_list(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert_eq!(list[0].commit, dropped.commit);
        assert_eq!(
            block(repo_stash_drop(StashDropRequest {
                path: path(tmp.path()),
                index: 5,
                operation_id: "r3".into()
            }))
            .unwrap_err(),
            "stash_missing"
        );
    }
}
