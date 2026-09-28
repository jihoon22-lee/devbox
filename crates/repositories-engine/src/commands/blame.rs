//! Read-only blame using the admitted native Git executable and repository.
use super::*;
use crate::core::blame::parse_porcelain;
pub use crate::core::blame::Blame;
const FAILED: &str = "blame_unavailable";
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlameRequest {
    pub path: String,
    pub file: String,
    pub commit_id: Option<String>,
}
pub async fn repo_blame(request: BlameRequest) -> Result<Blame, String> {
    validate_change_path(&request.file).map_err(|_| FAILED)?;
    if let Some(commit) = &request.commit_id {
        validate_commit_id(commit).map_err(|_| FAILED)?;
    }
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        source_root_before_io(&context.worktree).map_err(|_| FAILED)?;
        match devbox_filesystem::ensure_no_links(context.worktree.join(&request.file)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(FAILED.into()),
        }
        let target = git_target_for_path(&context.worktree, FAILED)?;
        let mut args = vec![
            "--no-pager",
            "--literal-pathspecs",
            "blame",
            "--porcelain",
            "--no-textconv",
        ];
        if let Some(commit) = &request.commit_id {
            args.push(commit);
        }
        args.extend(["--", &request.file]);
        let output =
            devbox_git::run_bounded_target(&args, &target, Duration::from_secs(5), 4 * 1024 * 1024)
                .map_err(|_| FAILED)?;
        let mut view = parse_porcelain(&output, 10_000)?;
        view.file = request.file;
        revalidate_repository_context(&context, FAILED)?;
        Ok(view)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    #[test]
    fn two_lines_show_two_commits_and_a_requested_revision() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        let first = git(tmp.path(), &["rev-parse", "HEAD"]).trim().to_owned();
        std::fs::write(tmp.path().join("README.md"), "base\nsecond\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "second"]);
        let root =
            host_path_spelling(&tmp.path().canonicalize().unwrap(), "blame_unavailable").unwrap();
        let view = crate::runtime::block_on(repo_blame(BlameRequest {
            path: root.clone(),
            file: "README.md".into(),
            commit_id: None,
        }))
        .unwrap();
        assert_eq!(view.lines.len(), 2);
        assert_eq!(view.commits.len(), 2);
        assert_eq!(view.file, "README.md");
        let old = crate::runtime::block_on(repo_blame(BlameRequest {
            path: root,
            file: "README.md".into(),
            commit_id: Some(first),
        }))
        .unwrap();
        assert_eq!(old.lines.len(), 1);
    }
    #[test]
    fn repositories_without_commits_and_escaping_paths_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "--quiet", "-b", "main"]);
        std::fs::write(tmp.path().join("README.md"), "new\n").unwrap();
        let root =
            host_path_spelling(&tmp.path().canonicalize().unwrap(), "blame_unavailable").unwrap();
        for file in ["README.md", "../outside"] {
            assert_eq!(
                crate::runtime::block_on(repo_blame(BlameRequest {
                    path: root.clone(),
                    file: file.into(),
                    commit_id: None
                }))
                .unwrap_err(),
                "blame_unavailable"
            );
        }
    }
}
