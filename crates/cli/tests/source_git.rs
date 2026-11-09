mod support;
use devbox_git::{model::*, rpc::*};
use devbox_protocol::{Confirm, Method};
use std::time::Duration;
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
        .expect("run owned Git fixture command");
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
    std::fs::create_dir(&dir).expect("create owned fixture directory");
    git(&dir, &["init", "-qb", "main"]);
    std::fs::write(dir.join("a"), "one\n").expect("write owned fixture");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "initial"]);
    git(&dir, &["config", "filter.owned.clean", "SECRET-SENTINEL"]);
    let project = bounded(
        c.call::<devbox_projects::ProjectsAdd>(&devbox_projects::AddProject {
            path: dir.to_string_lossy().into_owned(),
        }),
    )
    .await
    .expect("owned RPC response");
    assert!(
        project
            .git_warnings
            .expect("owned RPC response")
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
    let status = bounded(c.call::<GitStatus>(&query))
        .await
        .expect("owned RPC response");
    assert_eq!(status.expect.state.branch.as_deref(), Some("main"));
    std::fs::write(dir.join("a"), "two\n").expect("write owned fixture");
    let diff = bounded(c.call::<GitDiff>(&GitDiffQuery {
        target: target.clone(),
        path: "a".into(),
        staged: false,
    }))
    .await
    .expect("owned RPC response");
    bounded(c.call::<GitStage>(&GitFileChange {
        target: target.clone(),
        path: "a".into(),
        hunk: None,
        expect: diff.expect,
    }))
    .await
    .expect("owned RPC response");
    let status = bounded(c.call::<GitStatus>(&query))
        .await
        .expect("owned RPC response");
    assert_eq!(status.summary.staged, 1);
    let invalid = GitTargetQuery {
        target: SourceTarget {
            project_id: dir.to_string_lossy().into_owned(),
            worktree_id: None,
        },
    };
    let error = bounded(c.call::<GitStatus>(&invalid))
        .await
        .expect_err("owned RPC must reject stale or invalid input");
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
    let branches = bounded(c.call::<GitBranches>(&query))
        .await
        .expect("owned RPC response");
    assert_eq!(branches[0].name, "main");
    let log = bounded(c.call::<GitLog>(&GitLogQuery {
        target: target.clone(),
        path: Some("a".into()),
        limit: 10,
        skip: 0,
    }))
    .await
    .expect("owned RPC response");
    assert_eq!(log[0].subject, "initial");
    let trees = bounded(c.call::<GitWorktrees>(&query))
        .await
        .expect("owned RPC response");
    assert!(trees[0].main);
    assert!(
        bounded(c.call::<GitCleanupCandidates>(&query))
            .await
            .expect("owned RPC response")
            .is_empty()
    );
}

#[tokio::test]
async fn conflict_rpc_preserves_stale_file_and_applies_fresh_resolution() {
    use devbox_git::conflict_model::ConflictApply;
    assert_eq!(GitConflictApply::CONFIRM, Confirm::Reversible);
    let d = TestDaemon::start();
    let c = d.client().await;
    let dir = d.home.path().join("conflicts");
    std::fs::create_dir(&dir).expect("create owned fixture directory");
    git(&dir, &["init", "-qb", "main"]);
    std::fs::write(dir.join("a"), "base").expect("write owned fixture");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "base"]);
    git(&dir, &["checkout", "-b", "side"]);
    std::fs::write(dir.join("a"), "side").expect("write owned fixture");
    git(&dir, &["commit", "-am", "side"]);
    git(&dir, &["checkout", "main"]);
    std::fs::write(dir.join("a"), "main").expect("write owned fixture");
    git(&dir, &["commit", "-am", "main"]);
    assert!(
        !std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args([
                "-c",
                "user.name=Owned",
                "-c",
                "user.email=owned@example.invalid",
                "merge",
                "side"
            ])
            .output()
            .expect("run owned Git fixture command")
            .status
            .success()
    );
    let project = bounded(
        c.call::<devbox_projects::ProjectsAdd>(&devbox_projects::AddProject {
            path: dir.to_string_lossy().into_owned(),
        }),
    )
    .await
    .expect("owned RPC response");
    let target = SourceTarget {
        project_id: project.id,
        worktree_id: None,
    };
    let files = bounded(c.call::<GitConflicts>(&GitTargetQuery {
        target: target.clone(),
    }))
    .await
    .expect("owned RPC response");
    assert_eq!(files.len(), 1);
    let query = GitPathQuery {
        target: target.clone(),
        path: "a".into(),
    };
    let preview = bounded(c.call::<GitConflictPreview>(&query))
        .await
        .expect("owned RPC response");
    let index = std::fs::read(dir.join(".git/index")).expect("read owned index");
    std::fs::write(dir.join("a"), "external").expect("write owned fixture");
    let mut request = ConflictApply {
        target,
        path: "a".into(),
        resolution: Some("draft".into()),
        token: preview.token,
    };
    let error = bounded(c.call::<GitConflictApply>(&request))
        .await
        .expect_err("owned RPC must reject stale or invalid input");
    assert!(
        matches!(error, devbox_cli::client::ClientError::Rpc { code, .. } if code == "git.conflict_stale")
    );
    assert_eq!(
        std::fs::read(dir.join(".git/index")).expect("read owned index"),
        index
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("a")).expect("read owned fixture text"),
        "external"
    );
    request.token = bounded(c.call::<GitConflictPreview>(&query))
        .await
        .expect("owned RPC response")
        .token;
    bounded(c.call::<GitConflictApply>(&request))
        .await
        .expect("owned RPC response");
    assert_eq!(
        std::fs::read_to_string(dir.join("a")).expect("read owned fixture text"),
        "draft"
    );
    let files = bounded(c.call::<GitConflicts>(&GitTargetQuery {
        target: request.target,
    }))
    .await
    .expect("owned RPC response");
    assert!(files.is_empty());
}
#[test]
fn conflict_deletion_requires_explicit_null_and_never_an_omitted_resolution() {
    use devbox_git::conflict_model::{ConflictApply, ConflictToken};
    let target = SourceTarget {
        project_id: "p".into(),
        worktree_id: None,
    };
    let request = ConflictApply {
        target: target.clone(),
        path: "a".into(),
        resolution: None,
        token: ConflictToken {
            target,
            path: "a".into(),
            worktree_hash: "hash".into(),
            stage_oids: [None, None, None],
            identity: "identity".into(),
        },
    };
    let mut json = serde_json::to_value(&request).expect("encode owned RPC request");
    assert!(
        serde_json::from_value::<ConflictApply>(json.clone())
            .expect("owned RPC response")
            .resolution
            .is_none()
    );
    json.as_object_mut()
        .expect("owned request JSON object")
        .remove("resolution");
    assert!(serde_json::from_value::<ConflictApply>(json).is_err());
}

/// A task folder (`worktree_id` is the task) whose tool committed once and now waits idle.
async fn idle_task_folder(
    d: &mut TestDaemon,
) -> (
    devbox_cli::client::Client,
    String,
    String,
    serde_json::Value,
) {
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        r#"#!/usr/bin/env bash
printf change > agent.txt
git add agent.txt && git commit -qm 'agent change'
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude
while read -r text; do :; done
"#,
    )
    .expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[agents]\n[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\ncommand={}\nhooks='none'\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fixture", true).await;
    let idle = d
        .wait_for(&client, &id, "idle", Duration::from_secs(10))
        .await;
    (client, project, id, idle)
}
/// The `worktreeId` of every `git.changed` for `project` from now until one matches `until`, and
/// for a second after it, so another watcher that saw the same change has time to report it too.
async fn changed_targets(
    events: &mut devbox_cli::client::Subscription,
    project: &str,
    until: impl Fn(Option<&str>) -> bool,
) -> Vec<Option<String>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut settle = None;
    let mut seen = Vec::new();
    while let Ok(event) =
        tokio::time::timeout_at(settle.unwrap_or(deadline), events.next_event()).await
    {
        let event = event.expect("open git.changed subscription");
        // An empty rev is a resync, not a change.
        if event.rev.is_empty() {
            continue;
        }
        let changed: GitChanged =
            serde_json::from_str(&event.payload).expect("git.changed payload");
        if changed.project_id != project {
            continue;
        }
        if settle.is_none() && until(changed.worktree_id.as_deref()) {
            settle = Some(tokio::time::Instant::now() + Duration::from_secs(1));
        }
        seen.push(changed.worktree_id);
    }
    assert!(
        settle.is_some(),
        "the expected git.changed did not come: {seen:?}"
    );
    seen
}
#[tokio::test]
async fn task_folder_status_adds_no_watcher_and_conflict_reads_still_watch() {
    let mut d = TestDaemon::fixture();
    let (c, project, id, idle) = idle_task_folder(&mut d).await;
    let worktree = std::path::PathBuf::from(idle["worktreePath"].as_str().expect("worktree path"));
    let mut events = bounded(c.subscribe("git.changed"))
        .await
        .expect("owned subscription");
    let folder = SourceTarget {
        project_id: project.clone(),
        worktree_id: Some(id.clone()),
    };
    // The project's own checkout keeps its watcher.
    bounded(c.call::<GitStatus>(&GitTargetQuery {
        target: SourceTarget {
            project_id: project.clone(),
            worktree_id: None,
        },
    }))
    .await
    .expect("owned RPC response");
    // The task folder's status is read from that folder ...
    std::fs::write(worktree.join("untracked.txt"), "new\n").expect("write owned fixture");
    let status = bounded(c.call::<GitStatus>(&GitTargetQuery {
        target: folder.clone(),
    }))
    .await
    .expect("owned RPC response");
    assert_eq!(status.summary.branch.as_deref(), idle["branch"].as_str());
    assert!(status.files.iter().any(|file| file.path == "untracked.txt"));
    // ... without watching it: a commit there reaches the project's watcher alone.
    d.git(&worktree, &["commit", "--allow-empty", "-qm", "outside"]);
    let seen = changed_targets(&mut events, &project, |target| target.is_none()).await;
    assert!(
        !seen
            .iter()
            .any(|target| target.as_deref() == Some(id.as_str())),
        "a task folder's status registered a watcher: {seen:?}"
    );
    // A conflict read still watches the task folder, as conflict resolution relies on.
    assert!(
        bounded(c.call::<GitConflicts>(&GitTargetQuery { target: folder }))
            .await
            .expect("owned RPC response")
            .is_empty()
    );
    d.git(
        &worktree,
        &["commit", "--allow-empty", "-qm", "outside again"],
    );
    changed_targets(&mut events, &project, |target| target == Some(id.as_str())).await;
}
