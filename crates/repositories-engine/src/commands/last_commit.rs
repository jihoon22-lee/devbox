//! Last-commit projection for an explicit amend; remote state is advisory only.
use super::*;
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LastCommit {
    pub id: String,
    pub message: String,
    pub pushed: bool,
    pub merge: bool,
}
pub async fn repo_last_commit(request: PathRequest) -> Result<LastCommit, String> {
    spawn_git_task(GIT_VIEW_ERROR, move || {
        let context = validated_repository_context(&request.path, GIT_VIEW_ERROR)?;
        revalidate_repository_context(&context, GIT_VIEW_ERROR)?;
        let cancel = AtomicBool::new(false);
        if !repository_has_head(&context.worktree, &cancel)? {
            return Err("amend_no_commit".into());
        }
        let target = git_target_for_path(&context.worktree, GIT_VIEW_ERROR)?;
        let output = devbox_git::run_bounded_target_with_cancel(
            &["log", "-1", "--format=%H%x00%P%x00%B"],
            &target,
            Duration::from_secs(5),
            MAX_DETAIL_OUTPUT_BYTES,
            &cancel,
        )
        .map_err(|_| GIT_VIEW_ERROR)?;
        let parts = output.splitn(3, '\0').collect::<Vec<_>>();
        let [id, parents, message] = parts.as_slice() else {
            return Err(GIT_VIEW_ERROR.into());
        };
        let id = validate_commit_id(id)?;
        let parents = parents
            .split_whitespace()
            .map(validate_commit_id)
            .collect::<Result<Vec<_>, _>>()?;
        let pushed = devbox_git::run_bounded_target_with_cancel(
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
            &target,
            Duration::from_secs(5),
            MAX_SAFETY_METADATA_OUTPUT_BYTES,
            &cancel,
        )
        .is_ok()
            && devbox_git::run_bounded_target_with_cancel(
                &["merge-base", "--is-ancestor", "HEAD", "@{upstream}"],
                &target,
                Duration::from_secs(5),
                MAX_SAFETY_METADATA_OUTPUT_BYTES,
                &cancel,
            )
            .is_ok();
        revalidate_repository_context(&context, GIT_VIEW_ERROR)?;
        Ok(LastCommit {
            id,
            message: (*message).into(),
            pushed,
            merge: parents.len() > 1,
        })
    })
    .await
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amend_keeps_the_native_head_witness_and_old_requests_default_to_false() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::init_repo(tmp.path());
        let root = host_path_spelling(&tmp.path().canonicalize().unwrap(), "fixture").unwrap();
        let old: CommitRequest=serde_json::from_value(serde_json::json!({"path":root,"message":"message","operationId":"old","indexRevision":"x"})).unwrap();
        assert!(!old.amend);
        let review =
            crate::runtime::block_on(repo_commit_preview(PathRequest { path: root.clone() }))
                .unwrap();
        crate::test_support::git(
            tmp.path(),
            &["commit", "--quiet", "--amend", "-m", "external"],
        );
        assert_eq!(
            crate::runtime::block_on(repo_commit(CommitRequest {
                path: root,
                message: "stale amend".into(),
                operation_id: "stale-amend".into(),
                index_revision: review.revision,
                amend: true
            }))
            .unwrap_err(),
            "commit_review_stale"
        );
    }
    #[test]
    fn upstream_ancestry_distinguishes_pushed_and_new_local_commits() {
        use crate::test_support::{git, init_repo};
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir(&main).unwrap();
        std::fs::create_dir(&remote).unwrap();
        init_repo(&main);
        git(&remote, &["init", "--bare", "--quiet"]);
        git(
            &main,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&main, &["push", "--quiet", "-u", "origin", "main"]);
        let path = host_path_spelling(&main.canonicalize().unwrap(), "fixture").unwrap();
        assert!(
            crate::runtime::block_on(repo_last_commit(PathRequest { path: path.clone() }))
                .unwrap()
                .pushed
        );
        git(
            &main,
            &["commit", "--quiet", "--allow-empty", "-m", "local"],
        );
        assert!(
            !crate::runtime::block_on(repo_last_commit(PathRequest { path }))
                .unwrap()
                .pushed
        );
    }

    #[test]
    fn amend_rewrites_the_last_commit_message_and_contents() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::init_repo(tmp.path());
        let root =
            host_path_spelling(&tmp.path().canonicalize().unwrap(), "amend_no_commit").unwrap();
        let before = crate::test_support::git(tmp.path(), &["rev-parse", "HEAD"]);
        std::fs::write(tmp.path().join("README.md"), "amended\n").unwrap();
        crate::test_support::git(tmp.path(), &["add", "README.md"]);
        let review = crate::runtime::block_on(repo_commit_preview(RepoChangesRequest {
            path: root.clone(),
        }))
        .unwrap();
        crate::runtime::block_on(repo_commit(CommitRequest {
            path: root.clone(),
            message: "base (amended)".into(),
            operation_id: "a1".into(),
            index_revision: review.revision,
            amend: true,
        }))
        .unwrap();
        assert_ne!(
            crate::test_support::git(tmp.path(), &["rev-parse", "HEAD"]),
            before
        );
        assert_eq!(
            crate::test_support::git(tmp.path(), &["rev-list", "--count", "HEAD"]).trim(),
            "1"
        );
        let last = crate::runtime::block_on(repo_last_commit(PathRequest { path: root })).unwrap();
        assert_eq!(last.message.trim(), "base (amended)");
        assert!(!last.pushed);
    }

    #[test]
    fn message_only_amend_needs_no_staged_changes() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::init_repo(tmp.path());
        let root =
            host_path_spelling(&tmp.path().canonicalize().unwrap(), "amend_no_commit").unwrap();
        let review = crate::runtime::block_on(repo_commit_preview(RepoChangesRequest {
            path: root.clone(),
        }))
        .unwrap();
        crate::runtime::block_on(repo_commit(CommitRequest {
            path: root,
            message: "better message".into(),
            operation_id: "a2".into(),
            index_revision: review.revision,
            amend: true,
        }))
        .unwrap();
        assert_eq!(
            crate::test_support::git(tmp.path(), &["log", "-1", "--format=%s"]).trim(),
            "better message"
        );
    }

    #[test]
    fn a_repository_without_commits_cannot_amend() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::git(tmp.path(), &["init", "--quiet", "-b", "main"]);
        let error = crate::runtime::block_on(repo_last_commit(PathRequest {
            path: host_path_spelling(&tmp.path().canonicalize().unwrap(), "amend_no_commit")
                .unwrap(),
        }))
        .unwrap_err();
        assert_eq!(error, "amend_no_commit");
    }
}
