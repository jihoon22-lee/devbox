//! Agent Hub mutations use the existing repository lock and native Git scope.
use super::*;
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeRequest {
    pub path: String,
    pub branch: String,
    pub operation_id: String,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub merged: bool,
    pub head: String,
    pub conflicts: Vec<String>,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveAgentWorktreeRequest {
    pub path: String,
    pub worktree: String,
    pub branch: String,
    pub force: bool,
    pub operation_id: String,
}
const MERGE_FAILED: &str = "source_merge_failed";
const NOT_AGENT: &str = "worktree_not_agent";
fn read(
    root: &Path,
    args: &[&str],
    cancellation: &AtomicBool,
    error: &'static str,
) -> Result<String, String> {
    let args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    run_git_status_bounded_with_cancel(&args, root, cancellation).map_err(|_| error.into())
}
fn mutate(
    root: &Path,
    args: &[&str],
    cancellation: &AtomicBool,
    error: &'static str,
) -> Result<(), String> {
    let args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    run_git_mutation_with_cancel(&args, root, cancellation).map_err(|_| error.into())
}
fn head(root: &Path, cancellation: &AtomicBool) -> Result<String, String> {
    let value = read(
        root,
        &["rev-parse", "--verify", "HEAD"],
        cancellation,
        MERGE_FAILED,
    )?;
    let value = value.trim();
    validate_commit_id(value).map_err(|_| MERGE_FAILED)?;
    Ok(value.into())
}
pub async fn repo_merge(request: MergeRequest) -> Result<MergeResult, String> {
    if !valid_worktree_branch(&request.branch) {
        return Err("worktree_branch_invalid".into());
    }
    let operation = begin_git_operation(&request.operation_id, MERGE_FAILED, MERGE_FAILED)?;
    spawn_git_task(MERGE_FAILED, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, MERGE_FAILED)?;
        operation.bind_repository(context.common_git_identity, MERGE_FAILED, MERGE_FAILED)?;
        revalidate_repository_context(&context, MERGE_FAILED)?;
        let cancellation = operation.cancellation.as_ref();
        // Never abort an operation that was already present before our merge.
        if remote_operation_in_progress(&context, Some(cancellation), Some(&request.operation_id))
            .map_err(|_| MERGE_FAILED)?
        {
            return Err(MERGE_FAILED.into());
        }
        if !run_git_status_bounded_with_cancel(
            &git_status_changes_args(),
            &context.worktree,
            cancellation,
        )
        .map_err(|_| MERGE_FAILED)?
        .is_empty()
        {
            return Err("source_merge_dirty".into());
        }
        let before = head(&context.worktree, cancellation)?;
        let reference = format!("refs/heads/{}", request.branch);
        if mutate(
            &context.worktree,
            &["--no-pager", "merge", "--no-ff", "--no-edit", &reference],
            cancellation,
            MERGE_FAILED,
        )
        .is_ok()
        {
            return Ok(MergeResult {
                merged: true,
                head: head(&context.worktree, cancellation)?,
                conflicts: vec![],
            });
        }
        let output = read(
            &context.worktree,
            &["--no-pager", "diff", "--name-only", "--diff-filter=U", "-z"],
            cancellation,
            MERGE_FAILED,
        );
        let conflicts = output
            .as_deref()
            .unwrap_or_default()
            .split_terminator('\0')
            .filter(|path| !path.is_empty())
            .take(200)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if remote_marker_exists(&context.worktree, "MERGE_HEAD", Some(cancellation))
            .map_err(|_| MERGE_FAILED)?
        {
            mutate(
                &context.worktree,
                &["merge", "--abort"],
                cancellation,
                MERGE_FAILED,
            )?;
        }
        // Report a rolled-back conflict only after proving HEAD and clean state.
        let after = head(&context.worktree, cancellation)?;
        if conflicts.is_empty()
            || after != before
            || !run_git_status_bounded_with_cancel(
                &git_status_changes_args(),
                &context.worktree,
                cancellation,
            )
            .map_err(|_| MERGE_FAILED)?
            .is_empty()
            || remote_marker_exists(&context.worktree, "MERGE_HEAD", Some(cancellation))
                .map_err(|_| MERGE_FAILED)?
        {
            return Err(MERGE_FAILED.into());
        }
        Ok(MergeResult {
            merged: false,
            head: after,
            conflicts,
        })
    })
    .await
}
fn path_key(path: &Path) -> Result<String, String> {
    let spelling = host_path_spelling(path, NOT_AGENT)?;
    devbox_filesystem::parse_safe_project_path(&spelling)
        .map(|path| path.identity().into())
        .ok_or_else(|| NOT_AGENT.into())
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectAgentWorktreeRequest {
    pub path: String,
    pub worktree: String,
    pub branch: String,
}
#[derive(Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentWorktreePresence {
    Present,
    Absent,
}
/// Observe only the admitted base repository's Git metadata. Never adopt a
/// folder based on existence alone or inspect an unapproved target repository.
pub async fn inspect_agent_worktree(
    request: InspectAgentWorktreeRequest,
) -> Result<AgentWorktreePresence, String> {
    if !request.branch.starts_with("agent/") || !valid_worktree_branch(&request.branch) {
        return Err(NOT_AGENT.into());
    }
    spawn_git_task(NOT_AGENT, move || {
        let context = validated_repository_context(&request.path, NOT_AGENT)?;
        let cancellation = AtomicBool::new(false);
        let output = read(
            &context.worktree,
            &["worktree", "list", "--porcelain", "-z"],
            &cancellation,
            NOT_AGENT,
        )?;
        let records = parse_worktree_records(&output).map_err(|_| NOT_AGENT)?;
        let requested = path_key(Path::new(&request.worktree))?;
        if requested == path_key(&context.worktree)? {
            return Err(NOT_AGENT.into());
        }
        let mut present = false;
        for (index, record) in records.iter().enumerate() {
            let path = host_path_from_git(&context.worktree, &record.path, NOT_AGENT)?;
            if path_key(&path)? == requested {
                if index == 0
                    || record.branch.as_deref() != Some(&request.branch)
                    || record.bare
                    || record.locked
                    || record.prunable
                {
                    return Err(NOT_AGENT.into());
                }
                present = true;
            } else if record.branch.as_deref() == Some(&request.branch) {
                return Err(NOT_AGENT.into());
            }
        }
        let result = if present {
            AgentWorktreePresence::Present
        } else {
            let reference = format!("refs/heads/{}", request.branch);
            let refs = read(
                &context.worktree,
                &["for-each-ref", "--format=%(refname)", &reference],
                &cancellation,
                NOT_AGENT,
            )?;
            if refs.lines().any(|line| line == reference) {
                return Err(NOT_AGENT.into());
            }
            AgentWorktreePresence::Absent
        };
        revalidate_repository_context(&context, NOT_AGENT)?;
        Ok(result)
    })
    .await
}
pub async fn remove_agent_worktree(request: RemoveAgentWorktreeRequest) -> Result<(), String> {
    if !request.branch.starts_with("agent/") || !valid_worktree_branch(&request.branch) {
        return Err(NOT_AGENT.into());
    }
    let operation = begin_git_operation(&request.operation_id, NOT_AGENT, NOT_AGENT)?;
    spawn_git_task(NOT_AGENT, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, NOT_AGENT)?;
        operation.bind_repository(context.common_git_identity, NOT_AGENT, NOT_AGENT)?;
        revalidate_repository_context(&context, NOT_AGENT)?;
        let cancellation = operation.cancellation.as_ref();
        let output = read(
            &context.worktree,
            &["worktree", "list", "--porcelain", "-z"],
            cancellation,
            NOT_AGENT,
        )?;
        let records = parse_worktree_records(&output).map_err(|_| NOT_AGENT)?;
        let requested = path_key(Path::new(&request.worktree))?;
        let primary = host_path_from_git(
            &context.worktree,
            &records.first().ok_or(NOT_AGENT)?.path,
            NOT_AGENT,
        )?;
        if requested == path_key(&primary)? || requested == path_key(&context.worktree)? {
            return Err(NOT_AGENT.into());
        }
        let branch = format!("refs/heads/{}", request.branch);
        let mut target = None;
        for record in records.iter().skip(1) {
            let path = host_path_from_git(&context.worktree, &record.path, NOT_AGENT)?;
            if path_key(&path)? == requested {
                if record.branch.as_deref() != Some(request.branch.as_str())
                    || record.bare
                    || record.locked
                    || record.prunable
                {
                    return Err(NOT_AGENT.into());
                }
                target = Some(path);
                break;
            }
        }
        let target = target.ok_or(NOT_AGENT)?;
        if !request.force
            && read(
                &context.worktree,
                &["merge-base", "--is-ancestor", &branch, "HEAD"],
                cancellation,
                "worktree_branch_unmerged",
            )
            .is_err()
        {
            return Err("worktree_branch_unmerged".into());
        }
        let target = git_path_from_host(&context.worktree, &target, NOT_AGENT)?;
        let mut args = vec!["worktree", "remove"];
        if request.force {
            args.push("--force");
        }
        args.extend(["--", target.as_str()]);
        mutate(
            &context.worktree,
            &args,
            cancellation,
            "worktree_remove_dirty",
        )?;
        mutate(
            &context.worktree,
            &[
                "branch",
                if request.force { "-D" } else { "-d" },
                "--",
                &request.branch,
            ],
            cancellation,
            "worktree_branch_unmerged",
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    #[test]
    fn inspection_recovers_created_and_removed_worktrees_without_adopting_another_branch() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        let inspect = |worktree: &Path, branch: &str| {
            crate::runtime::block_on(inspect_agent_worktree(InspectAgentWorktreeRequest {
                path: main.to_string_lossy().into(),
                worktree: worktree.to_string_lossy().into(),
                branch: branch.into(),
            }))
        };
        assert_eq!(
            inspect(&agent, "agent/fix").unwrap(),
            AgentWorktreePresence::Present
        );
        assert!(inspect(&main, "agent/fix").is_err());
        assert!(inspect(&agent, "agent/other").is_err());
        git(&main, &["worktree", "remove", agent.to_str().unwrap()]);
        // A partial cleanup retains a branch, so do not guess that removal completed.
        assert!(inspect(&agent, "agent/fix").is_err());
        git(&main, &["branch", "-D", "agent/fix"]);
        assert_eq!(
            inspect(&agent, "agent/fix").unwrap(),
            AgentWorktreePresence::Absent
        );
    }

    #[test]
    fn worktree_keys_keep_windows_prefixes_and_posix_case_separate() {
        assert_eq!(
            path_key(Path::new(r"\\?\C:\Work\Repo")).unwrap(),
            path_key(Path::new("c:/work/repo")).unwrap()
        );
        assert_ne!(
            path_key(Path::new("/work/Repo")).unwrap(),
            path_key(Path::new("/work/repo")).unwrap()
        );
        assert!(path_key(Path::new("/work/../other")).is_err());
    }

    #[test]
    fn an_existing_merge_is_not_aborted_by_a_new_request() {
        let (_tmp, main, _) = repo_with_agent_branch("agent\n", None);
        git(&main, &["merge", "--no-commit", "--no-ff", "agent/fix"]);
        let marker = fs::read(main.join(".git/MERGE_HEAD")).unwrap();
        let result = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(),
            branch: "agent/fix".into(),
            operation_id: "preexisting-merge".into(),
        }));
        assert!(result.is_err());
        assert_eq!(fs::read(main.join(".git/MERGE_HEAD")).unwrap(), marker);
        assert_eq!(
            fs::read_to_string(main.join("shared.txt")).unwrap(),
            "agent\n"
        );
    }
    #[test]
    fn removal_preserves_the_calling_worktree_and_other_branches() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        assert_eq!(
            crate::runtime::block_on(remove_agent_worktree(RemoveAgentWorktreeRequest {
                path: agent.to_string_lossy().into(),
                worktree: agent.to_string_lossy().into(),
                branch: "agent/fix".into(),
                force: true,
                operation_id: "self-removal".into()
            }))
            .unwrap_err(),
            "worktree_not_agent"
        );
        git(&agent, &["branch", "-m", "ordinary"]);
        assert_eq!(
            crate::runtime::block_on(remove_agent_worktree(RemoveAgentWorktreeRequest {
                path: main.to_string_lossy().into(),
                worktree: agent.to_string_lossy().into(),
                branch: "agent/fix".into(),
                force: true,
                operation_id: "foreign-branch-removal".into()
            }))
            .unwrap_err(),
            "worktree_not_agent"
        );
        assert!(agent.exists());
        assert!(!git(&main, &["branch", "--list", "ordinary"]).is_empty());
    }

    fn repo_with_agent_branch(
        file_on_agent: &str,
        file_on_main: Option<&str>,
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        // Use the same canonical DOS spelling as native Registry admission;
        // Windows temporary directories may otherwise use an 8.3 alias.
        let root = PathBuf::from(
            host_path_spelling(&tmp.path().canonicalize().unwrap(), NOT_AGENT).unwrap(),
        );
        let main = root.join("devbox");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "--quiet", "-b", "main"]);
        for (key, value) in [
            ("user.email", "hub@example.test"),
            ("user.name", "Hub"),
            ("core.autocrlf", "false"),
        ] {
            git(&main, &["config", key, value]);
        }
        fs::write(main.join("shared.txt"), "base\n").unwrap();
        git(&main, &["add", "shared.txt"]);
        git(&main, &["commit", "--quiet", "-m", "base"]);
        let agent = root.join("devbox-fix");
        git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "agent/fix",
                agent.to_str().unwrap(),
            ],
        );
        fs::write(agent.join("shared.txt"), file_on_agent).unwrap();
        git(&agent, &["commit", "--quiet", "-am", "agent change"]);
        if let Some(content) = file_on_main {
            fs::write(main.join("shared.txt"), content).unwrap();
            git(&main, &["commit", "--quiet", "-am", "main change"]);
        }
        (tmp, main, agent)
    }

    #[test]
    fn merge_creates_a_merge_commit_on_the_base_worktree() {
        let (_tmp, main, _agent) = repo_with_agent_branch("agent\n", None);
        let result = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(),
            branch: "agent/fix".into(),
            operation_id: "merge-1".into(),
        }))
        .unwrap();
        assert!(result.merged && result.conflicts.is_empty());
        assert_eq!(
            fs::read_to_string(main.join("shared.txt")).unwrap(),
            "agent\n"
        );
        assert_eq!(
            git(&main, &["rev-list", "--count", "--merges", "HEAD"]).trim(),
            "1"
        );
    }

    #[test]
    fn conflicting_merge_is_aborted_and_reports_paths() {
        let (_tmp, main, _agent) = repo_with_agent_branch("agent\n", Some("main\n"));
        let before = git(&main, &["rev-parse", "HEAD"]);
        let result = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(),
            branch: "agent/fix".into(),
            operation_id: "merge-2".into(),
        }))
        .unwrap();
        assert!(!result.merged);
        assert_eq!(result.conflicts, vec!["shared.txt".to_string()]);
        assert_eq!(git(&main, &["rev-parse", "HEAD"]), before);
        assert!(git(&main, &["status", "--porcelain"]).is_empty());
        assert!(!main.join(".git/MERGE_HEAD").exists());
    }

    #[test]
    fn merge_refuses_a_dirty_base_worktree() {
        let (_tmp, main, _agent) = repo_with_agent_branch("agent\n", None);
        fs::write(main.join("wip.txt"), "wip\n").unwrap();
        let error = crate::runtime::block_on(repo_merge(MergeRequest {
            path: main.to_string_lossy().into(),
            branch: "agent/fix".into(),
            operation_id: "merge-3".into(),
        }))
        .unwrap_err();
        assert_eq!(error, "source_merge_dirty");
    }

    #[test]
    fn removal_is_limited_to_agent_branches_and_linked_worktrees() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        let request = |worktree: &Path, branch: &str, force: bool| RemoveAgentWorktreeRequest {
            path: main.to_string_lossy().into(),
            worktree: worktree.to_string_lossy().into(),
            branch: branch.into(),
            force,
            operation_id: "remove-1".into(),
        };
        assert_eq!(
            crate::runtime::block_on(remove_agent_worktree(request(&main, "main", true)))
                .unwrap_err(),
            "worktree_not_agent"
        );
        assert_eq!(
            crate::runtime::block_on(remove_agent_worktree(request(&main, "agent/fix", true)))
                .unwrap_err(),
            "worktree_not_agent"
        );
        git(&main, &["merge", "--quiet", "--no-edit", "agent/fix"]);
        fs::write(agent.join("uncommitted.txt"), "x\n").unwrap();
        assert_eq!(
            crate::runtime::block_on(remove_agent_worktree(request(&agent, "agent/fix", false)))
                .unwrap_err(),
            "worktree_remove_dirty"
        );
        assert!(agent.exists());
        crate::runtime::block_on(remove_agent_worktree(request(&agent, "agent/fix", true)))
            .unwrap();
        assert!(!agent.exists());
        assert!(git(&main, &["branch", "--list", "agent/fix"]).is_empty());
    }

    #[test]
    fn a_merged_clean_worktree_is_removed_without_force() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        git(&main, &["merge", "--quiet", "--no-edit", "agent/fix"]);
        crate::runtime::block_on(remove_agent_worktree(RemoveAgentWorktreeRequest {
            path: main.to_string_lossy().into(),
            worktree: agent.to_string_lossy().into(),
            branch: "agent/fix".into(),
            force: false,
            operation_id: "remove-3".into(),
        }))
        .unwrap();
        assert!(!agent.exists());
        assert!(git(&main, &["branch", "--list", "agent/fix"]).is_empty());
    }

    #[test]
    fn safe_removal_keeps_an_unmerged_branch() {
        let (_tmp, main, agent) = repo_with_agent_branch("agent\n", None);
        let error = crate::runtime::block_on(remove_agent_worktree(RemoveAgentWorktreeRequest {
            path: main.to_string_lossy().into(),
            worktree: agent.to_string_lossy().into(),
            branch: "agent/fix".into(),
            force: false,
            operation_id: "remove-2".into(),
        }))
        .unwrap_err();
        assert_eq!(error, "worktree_branch_unmerged");
        assert!(!git(&main, &["branch", "--list", "agent/fix"]).is_empty());
        assert!(agent.exists(), "an unmerged branch keeps its worktree too");
    }
}
