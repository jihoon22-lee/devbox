mod support;
use devbox_git::{model::*, rpc::*};
use devbox_protocol::{Confirm, Method};
use support::{TestDaemon, bounded};
fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Owned")
        .env("GIT_AUTHOR_EMAIL", "owned@example.invalid")
        .env("GIT_COMMITTER_NAME", "Owned")
        .env("GIT_COMMITTER_EMAIL", "owned@example.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[tokio::test]
async fn source_git_rpc_resolves_targets_and_mutation_metadata() {
    assert_eq!(GitDiscard::CONFIRM, Confirm::Always);
    assert_eq!(GitBranchDelete::CONFIRM, Confirm::Always);
    assert_eq!(GitStashDrop::CONFIRM, Confirm::Always);
    assert_eq!(GitCleanup::CONFIRM, Confirm::Always);
    assert_eq!(GitPush::CONFIRM, Confirm::Always);
    assert_eq!(GitPull::CONFIRM, Confirm::Always);
    assert_eq!(GitStage::CONFIRM, Confirm::Reversible);
    let d = TestDaemon::start();
    let c = d.client().await;
    let dir = d.home.path().join("source-git");
    std::fs::create_dir(&dir).unwrap();
    git(&dir, &["init", "-qb", "main"]);
    std::fs::write(dir.join("a"), "one\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "initial"]);
    git(&dir, &["config", "filter.owned.clean", "SECRET-SENTINEL"]);
    let project = bounded(
        c.call::<devbox_projects::ProjectsAdd>(&devbox_projects::AddProject {
            path: dir.to_string_lossy().into_owned(),
        }),
    )
    .await
    .unwrap();
    assert!(
        project
            .git_warnings
            .unwrap()
            .iter()
            .any(|w| w.key == "filter.owned.clean")
    );
    let target = SourceTarget {
        project_id: project.id,
        worktree_id: None,
    };
    let query = GitTargetQuery {
        target: target.clone(),
    };
    let status = bounded(c.call::<GitStatus>(&query)).await.unwrap();
    assert_eq!(status.expect.state.branch.as_deref(), Some("main"));
    std::fs::write(dir.join("a"), "two\n").unwrap();
    let diff = bounded(c.call::<GitDiff>(&GitDiffQuery {
        target: target.clone(),
        path: "a".into(),
        staged: false,
    }))
    .await
    .unwrap();
    bounded(c.call::<GitStage>(&GitFileChange {
        target: target.clone(),
        path: "a".into(),
        hunk: None,
        expect: diff.expect,
    }))
    .await
    .unwrap();
    let status = bounded(c.call::<GitStatus>(&query)).await.unwrap();
    assert_eq!(status.summary.staged, 1);
    let invalid = GitTargetQuery {
        target: SourceTarget {
            project_id: dir.to_string_lossy().into_owned(),
            worktree_id: None,
        },
    };
    let error = bounded(c.call::<GitStatus>(&invalid)).await.unwrap_err();
    assert!(
        matches!(error,devbox_cli::client::ClientError::Rpc{code,..}if code=="git.invalid_target")
    );
    let invalid = GitTargetQuery {
        target: SourceTarget {
            project_id: target.project_id.clone(),
            worktree_id: Some(dir.to_string_lossy().into_owned()),
        },
    };
    assert!(bounded(c.call::<GitStatus>(&invalid)).await.is_err());
    let branches = bounded(c.call::<GitBranches>(&query)).await.unwrap();
    assert_eq!(branches[0].name, "main");
    let log = bounded(c.call::<GitLog>(&GitLogQuery {
        target: target.clone(),
        path: Some("a".into()),
        limit: 10,
        skip: 0,
    }))
    .await
    .unwrap();
    assert_eq!(log[0].subject, "initial");
    let trees = bounded(c.call::<GitWorktrees>(&query)).await.unwrap();
    assert!(trees[0].main);
    assert!(
        bounded(c.call::<GitCleanupCandidates>(&query))
            .await
            .unwrap()
            .is_empty()
    );
}
