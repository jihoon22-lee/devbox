//! Native branch operations preserve the existing repository admission and lock.
use super::*;
use crate::core::branches::{parse_refs, FOR_EACH_REF_FORMAT};
pub use crate::core::branches::{Branch, BranchList};
pub type PathRequest = RepoChangesRequest;
const FAILED: &str = "branch_operation_failed";

#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BranchCreateRequest {
    pub path: String,
    pub name: String,
    pub start_point: Option<String>,
    pub checkout: bool,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SwitchRequest {
    pub path: String,
    pub branch: String,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BranchRenameRequest {
    pub path: String,
    pub from: String,
    pub to: String,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BranchDeleteRequest {
    pub path: String,
    pub name: String,
    pub operation_id: String,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DeletedBranch {
    pub name: String,
    pub commit: String,
}
fn validate_name(name: &str) -> Result<(), String> {
    valid_worktree_branch(name)
        .then_some(())
        .ok_or_else(|| "branch_name_invalid".into())
}
fn classify_branch_failure(stderr: &str) -> &'static str {
    let stderr = stderr.to_ascii_lowercase();
    if stderr.contains("already exists") {
        "branch_exists"
    } else if stderr.contains("checked out at")
        || stderr.contains("cannot delete branch")
        || stderr.contains("used by worktree")
    {
        "branch_in_use"
    } else if stderr.contains("would be overwritten by checkout")
        || stderr.contains("please commit your changes or stash them")
    {
        "switch_blocked_by_changes"
    } else if stderr.contains("not found") || stderr.contains("invalid reference") {
        "branch_missing"
    } else {
        FAILED
    }
}
fn mutate(root: &Path, args: &[&str], cancel: &AtomicBool) -> Result<(), String> {
    let target = git_target_for_path(root, FAILED)?;
    devbox_git::run_mutating_target_classified(
        args,
        &target,
        MUTATION_TIMEOUT,
        MAX_MUTATION_OUTPUT_BYTES,
        cancel,
        classify_branch_failure,
    )
    .map(|_| ())
    .map_err(|error| match error.as_str() {
        "branch_exists" | "branch_in_use" | "switch_blocked_by_changes" | "branch_missing" => error,
        _ => FAILED.into(),
    })
}
fn read(root: &Path, args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    run_git_status_bounded_with_cancel(
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        root,
        cancel,
    )
    .map_err(|_| FAILED.into())
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
pub async fn repo_branches(request: PathRequest) -> Result<BranchList, String> {
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let cancel = AtomicBool::new(false);
        let current = read(&context.worktree, &["branch", "--show-current"], &cancel)?;
        let current = (!current.trim().is_empty()).then(|| current.trim().to_owned());
        let output = read(
            &context.worktree,
            &[
                "for-each-ref",
                &format!("--format={FOR_EACH_REF_FORMAT}"),
                "--count=2001",
                "refs/heads",
                "refs/remotes",
            ],
            &cancel,
        )?;
        let truncated = output.lines().count() > 2000;
        let mut branches = parse_refs(&output, current.as_deref())?;
        branches.truncate(2000);
        revalidate_repository_context(&context, FAILED)?;
        Ok(BranchList {
            detached: current.is_none(),
            current,
            branches,
            truncated,
        })
    })
    .await
}
pub async fn repo_branch_create(request: BranchCreateRequest) -> Result<(), String> {
    validate_name(&request.name)?;
    if let Some(start) = &request.start_point {
        validate_name(start)?;
    }
    change(request.path, request.operation_id, move |root, cancel| {
        let mut args = if request.checkout {
            vec!["switch", "-c", &request.name]
        } else {
            vec!["branch", &request.name]
        };
        if let Some(start) = &request.start_point {
            args.push(start);
        }
        mutate(root, &args, cancel)
    })
    .await
}
pub async fn repo_switch(request: SwitchRequest) -> Result<(), String> {
    validate_name(&request.branch)?;
    change(request.path, request.operation_id, move |root, cancel| {
        let local = format!("refs/heads/{}", request.branch);
        let remote = format!("refs/remotes/{}", request.branch);
        if read(root, &["show-ref", "--verify", "--quiet", &local], cancel).is_ok() {
            mutate(root, &["switch", &request.branch], cancel)
        } else if read(root, &["show-ref", "--verify", "--quiet", &remote], cancel).is_ok() {
            mutate(root, &["switch", "--track", &remote], cancel)
        } else {
            Err("branch_missing".into())
        }
    })
    .await
}
pub async fn repo_branch_rename(request: BranchRenameRequest) -> Result<(), String> {
    validate_name(&request.from)?;
    validate_name(&request.to)?;
    change(request.path, request.operation_id, move |root, cancel| {
        mutate(root, &["branch", "-m", &request.from, &request.to], cancel)
    })
    .await
}
pub async fn repo_branch_delete(request: BranchDeleteRequest) -> Result<DeletedBranch, String> {
    validate_name(&request.name)?;
    change(request.path, request.operation_id, move |root, cancel| {
        let reference = format!("refs/heads/{}", request.name);
        let commit = read(root, &["rev-parse", "--verify", &reference], cancel)
            .map_err(|_| "branch_missing")?;
        let commit = commit.trim().to_owned();
        validate_commit_id(&commit).map_err(|_| FAILED)?;
        mutate(root, &["branch", "-D", "--", &request.name], cancel)?;
        Ok(DeletedBranch {
            name: request.name,
            commit,
        })
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
        host_path_spelling(&dir.canonicalize().unwrap(), "branch_operation_failed").unwrap()
    }

    #[test]
    fn remote_tracking_switch_and_invalid_start_points() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(
            tmp.path(),
            &[
                "config",
                "remote.origin.url",
                "https://example.invalid/fixture.git",
            ],
        );
        git(
            tmp.path(),
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        );
        git(
            tmp.path(),
            &["update-ref", "refs/remotes/origin/topic", "HEAD"],
        );
        block(repo_switch(SwitchRequest {
            path: path(tmp.path()),
            branch: "origin/topic".into(),
            operation_id: "track-1".into(),
        }))
        .unwrap();
        assert_eq!(
            git(
                tmp.path(),
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}"
                ]
            )
            .trim(),
            "origin/topic"
        );
        assert_eq!(
            block(repo_branch_create(BranchCreateRequest {
                path: path(tmp.path()),
                name: "invalid-start".into(),
                start_point: Some("--orphan".into()),
                checkout: false,
                operation_id: "invalid-start".into()
            }))
            .unwrap_err(),
            "branch_name_invalid"
        );
        assert_eq!(
            block(repo_switch(SwitchRequest {
                path: path(tmp.path()),
                branch: "absent".into(),
                operation_id: "absent-switch".into()
            }))
            .unwrap_err(),
            "branch_missing"
        );
    }

    #[test]
    fn diagnostics_are_projected_to_fixed_codes() {
        for (diagnostic, code) in [
            (
                "fatal: a branch named 'private' already exists",
                "branch_exists",
            ),
            (
                "error: cannot delete branch 'private' used by worktree at '/private'",
                "branch_in_use",
            ),
            (
                "error: local changes would be overwritten by checkout",
                "switch_blocked_by_changes",
            ),
            ("fatal: invalid reference: unknown", "branch_missing"),
            (
                "unrecognized diagnostic /private",
                "branch_operation_failed",
            ),
        ] {
            assert_eq!(classify_branch_failure(diagnostic), code);
        }
    }
    #[test]
    fn a_branch_checked_out_in_another_worktree_cannot_be_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        fs::create_dir(&main).unwrap();
        init_repo(&main);
        let linked = tmp.path().join("linked");
        git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        assert_eq!(
            block(repo_branch_delete(BranchDeleteRequest {
                path: path(&main),
                name: "linked".into(),
                operation_id: "linked-delete".into()
            }))
            .unwrap_err(),
            "branch_in_use"
        );
        assert!(linked.join("README.md").is_file());
    }
    #[test]
    fn create_switch_rename_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        block(repo_branch_create(BranchCreateRequest {
            path: path(tmp.path()),
            name: "feature/a".into(),
            start_point: None,
            checkout: true,
            operation_id: "b1".into(),
        }))
        .unwrap();
        assert_eq!(
            git(tmp.path(), &["branch", "--show-current"]).trim(),
            "feature/a"
        );
        block(repo_branch_rename(BranchRenameRequest {
            path: path(tmp.path()),
            from: "feature/a".into(),
            to: "feature/b".into(),
            operation_id: "b2".into(),
        }))
        .unwrap();
        block(repo_switch(SwitchRequest {
            path: path(tmp.path()),
            branch: "main".into(),
            operation_id: "b3".into(),
        }))
        .unwrap();
        let list = block(repo_branches(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert_eq!(list.current.as_deref(), Some("main"));
        assert!(list.branches.iter().any(|b| b.name == "feature/b"));
        assert_eq!(
            block(repo_branch_create(BranchCreateRequest {
                path: path(tmp.path()),
                name: "main".into(),
                start_point: None,
                checkout: false,
                operation_id: "b4".into()
            }))
            .unwrap_err(),
            "branch_exists"
        );
        assert_eq!(
            block(repo_branch_create(BranchCreateRequest {
                path: path(tmp.path()),
                name: "-bad".into(),
                start_point: None,
                checkout: false,
                operation_id: "b5".into()
            }))
            .unwrap_err(),
            "branch_name_invalid"
        );
    }

    #[test]
    fn switching_is_refused_when_local_changes_would_be_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "other"]);
        fs::write(tmp.path().join("README.md"), "other\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "other"]);
        git(tmp.path(), &["switch", "--quiet", "main"]);
        fs::write(tmp.path().join("README.md"), "local edit\n").unwrap();
        let error = block(repo_switch(SwitchRequest {
            path: path(tmp.path()),
            branch: "other".into(),
            operation_id: "s1".into(),
        }))
        .unwrap_err();
        assert_eq!(error, "switch_blocked_by_changes");
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "local edit\n"
        );
        assert_eq!(
            git(tmp.path(), &["branch", "--show-current"]).trim(),
            "main"
        );
    }

    #[test]
    fn delete_returns_the_tip_for_undo_and_refuses_branches_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "unmerged"]);
        fs::write(tmp.path().join("new.txt"), "x\n").unwrap();
        git(tmp.path(), &["add", "new.txt"]);
        git(tmp.path(), &["commit", "--quiet", "-m", "unmerged work"]);
        let tip = git(tmp.path(), &["rev-parse", "HEAD"]).trim().to_string();
        assert_eq!(
            block(repo_branch_delete(BranchDeleteRequest {
                path: path(tmp.path()),
                name: "unmerged".into(),
                operation_id: "d1".into()
            }))
            .unwrap_err(),
            "branch_in_use"
        );
        git(tmp.path(), &["switch", "--quiet", "main"]);
        let deleted = block(repo_branch_delete(BranchDeleteRequest {
            path: path(tmp.path()),
            name: "unmerged".into(),
            operation_id: "d2".into(),
        }))
        .unwrap();
        assert_eq!(deleted.commit, tip);
        block(repo_branch_create(BranchCreateRequest {
            path: path(tmp.path()),
            name: "unmerged".into(),
            start_point: Some(tip.clone()),
            checkout: false,
            operation_id: "d3".into(),
        }))
        .unwrap();
        assert_eq!(git(tmp.path(), &["rev-parse", "unmerged"]).trim(), tip);
    }

    #[test]
    fn detached_head_lists_without_a_current_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "--detach", "HEAD"]);
        let list = block(repo_branches(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert!(list.detached && list.current.is_none());
    }
}
