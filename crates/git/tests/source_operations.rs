mod source_support;
use devbox_git::{
    model::*,
    remote,
    service::{GitService, RemoteAction},
};
use source_support::*;
use std::sync::Arc;
fn target() -> SourceTarget {
    SourceTarget {
        project_id: "owned".into(),
        worktree_id: None,
    }
}
#[tokio::test]
async fn stale_remote_starts_no_operation_or_event() {
    let r = repo();
    let d = r.path();
    let expected = remote::state(d).await.expect("read owned remote state");
    git(d, &["checkout", "-qb", "other"]);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let service = Arc::new(GitService::new(
        Arc::new(|_| {}),
        Arc::new(move |event| {
            let _ = tx.send(event);
        }),
    ));
    let result = service
        .begin(
            d.into(),
            target(),
            RemoteAction::Push {
                expect: expected,
                force: false,
            },
        )
        .await;
    assert!(matches!(result, Err(SourceGitError::StateChanged { .. })));
    assert!(rx.try_recv().is_err());
}
#[tokio::test]
async fn operation_status_is_target_bound_and_reconciles_completion() {
    let r = repo();
    let d = r.path();
    let service = Arc::new(GitService::new(Arc::new(|_| {}), Arc::new(|_| {})));
    let started = service
        .begin(d.into(), target(), RemoteAction::Fetch)
        .await
        .expect("start owned remote operation");
    let query = GitOperationQuery {
        target: target(),
        operation_id: started.id,
    };
    let bad = GitOperationQuery {
        target: SourceTarget {
            project_id: "other".into(),
            worktree_id: None,
        },
        operation_id: query.operation_id.clone(),
    };
    assert!(service.operation(&bad).await.is_err());
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let result = service
                .operation(&query)
                .await
                .expect("read owned operation status");
            if result.state != GitOperationState::Running {
                break result;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned remote operation result");
    assert_eq!(result.state, GitOperationState::Succeeded);
    assert_eq!(
        result.current.expect("owned remote operation result"),
        remote::state(d).await.expect("read owned remote state")
    );
    service.shutdown().await;
}
#[cfg(unix)]
async fn cancelled_transfer(pull: bool) {
    use std::os::unix::fs::PermissionsExt;
    let r = repo();
    let d = r.path();
    let script = d.join("owned-upload-pack");
    let pid = d.join("helper.pid");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho $$ > '{}'\necho SECRET-SENTINEL >&2\nexec sleep 60\n",
            pid.display()
        ),
    )
    .expect("write owned fixture");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
        .expect("set owned fixture permissions");
    let bare = tempfile::tempdir().expect("create owned temporary directory");
    git(bare.path(), &["init", "--bare", "-q"]);
    git(
        d,
        &[
            "remote",
            "add",
            "origin",
            bare.path().to_str().expect("owned UTF-8 path"),
        ],
    );
    git(d, &["push", "-qu", "origin", "main"]);
    git(
        d,
        &[
            "config",
            "remote.origin.uploadpack",
            script.to_str().expect("owned UTF-8 path"),
        ],
    );
    let service = Arc::new(GitService::new(Arc::new(|_| {}), Arc::new(|_| {})));
    let action = if pull {
        RemoteAction::Pull {
            expect: remote::state(d).await.expect("read owned remote state"),
            rebase: false,
        }
    } else {
        RemoteAction::Fetch
    };
    let started = service
        .begin(d.into(), target(), action)
        .await
        .expect("start owned remote operation");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !pid.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned remote operation result");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.cancel(&GitOperationQuery {
            target: target(),
            operation_id: started.id,
        }),
    )
    .await
    .expect("cancel owned operation")
    .expect("capture owned branch expectation");
    assert_eq!(result.state, GitOperationState::Cancelled);
    assert!(result.error.is_none());
    assert_eq!(
        result.current.expect("owned remote operation result"),
        remote::state(d).await.expect("read owned remote state")
    );
    let pid = std::fs::read_to_string(pid)
        .expect("read owned fixture text")
        .trim()
        .parse::<i32>()
        .expect("read owned fixture text");
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let alive = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok();
            let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|s| s.split_whitespace().nth(2) == Some("Z"));
            if !alive || zombie {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned remote operation result");
    service.shutdown().await;
    assert!(
        result.completion_unknown,
        "readable local state cannot prove transfer outcome"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn cancelling_push_after_remote_ref_update_reports_confirmed_completion() {
    use std::os::unix::fs::PermissionsExt;
    let r = repo();
    let d = r.path();
    let bare = tempfile::tempdir().expect("create owned temporary directory");
    git(bare.path(), &["init", "--bare", "-q"]);
    git(
        d,
        &[
            "remote",
            "add",
            "origin",
            bare.path().to_str().expect("owned UTF-8 path"),
        ],
    );
    git(d, &["push", "-qu", "origin", "main"]);
    git(d, &["commit", "--allow-empty", "-qm", "approved"]);
    let marker = bare.path().join("receive-completed");
    let hook = bare.path().join("hooks/post-receive");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\necho done > '{}'\nexec sleep 60\n",
            marker.display()
        ),
    )
    .expect("write owned fixture");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
        .expect("set owned fixture permissions");
    let expected = remote::state(d).await.expect("read owned remote state");
    let service = Arc::new(GitService::new(Arc::new(|_| {}), Arc::new(|_| {})));
    let started = service
        .begin(
            d.into(),
            target(),
            RemoteAction::Push {
                expect: expected.clone(),
                force: false,
            },
        )
        .await
        .expect("start owned remote operation");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned remote operation result");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.cancel(&GitOperationQuery {
            target: target(),
            operation_id: started.id,
        }),
    )
    .await
    .expect("cancel owned operation")
    .expect("capture owned branch expectation");
    assert_eq!(result.state, GitOperationState::Succeeded);
    assert_eq!(result.remote_oid, expected.head_oid);
    assert!(!result.completion_unknown);
    assert_eq!(
        git(bare.path(), &["rev-parse", "refs/heads/main"]).trim(),
        result.remote_oid.expect("owned remote operation result")
    );
    service.shutdown().await;
}
#[cfg(unix)]
async fn push_destination_case(multiple: bool) {
    use std::os::unix::fs::PermissionsExt;
    let r = repo();
    let d = r.path();
    let first = tempfile::tempdir().expect("create owned temporary directory");
    let second = tempfile::tempdir().expect("create owned temporary directory");
    for bare in [&first, &second] {
        git(bare.path(), &["init", "--bare", "-q"]);
        git(
            d,
            &[
                "push",
                "-q",
                bare.path().to_str().expect("owned UTF-8 path"),
                "main",
            ],
        );
    }
    git(
        d,
        &[
            "remote",
            "add",
            "origin",
            first.path().to_str().expect("owned UTF-8 path"),
        ],
    );
    git(d, &["fetch", "-q", "origin"]);
    git(d, &["branch", "--set-upstream-to=origin/main", "main"]);
    let receiving = if multiple {
        first.path()
    } else {
        second.path()
    };
    git(
        d,
        &[
            "config",
            "--add",
            "remote.origin.pushurl",
            receiving.to_str().expect("owned UTF-8 path"),
        ],
    );
    if multiple {
        git(
            d,
            &[
                "config",
                "--add",
                "remote.origin.pushurl",
                second.path().to_str().expect("owned UTF-8 path"),
            ],
        );
    }
    let previous = git(second.path(), &["rev-parse", "refs/heads/main"]);
    git(
        d,
        &["commit", "--allow-empty", "-qm", "confirmed destination"],
    );
    let marker = receiving.join("receive-completed");
    let hook = receiving.join("hooks/post-receive");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\necho done > '{}'\nexec sleep 60\n",
            marker.display()
        ),
    )
    .expect("write owned fixture");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
        .expect("set owned fixture permissions");
    let expected = remote::state(d).await.expect("read owned remote state");
    let service = Arc::new(GitService::new(Arc::new(|_| {}), Arc::new(|_| {})));
    let started = service
        .begin(
            d.into(),
            target(),
            RemoteAction::Push {
                expect: expected.clone(),
                force: false,
            },
        )
        .await
        .expect("start owned remote operation");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned remote operation result");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.cancel(&GitOperationQuery {
            target: target(),
            operation_id: started.id,
        }),
    )
    .await
    .expect("cancel owned operation")
    .expect("capture owned branch expectation");
    if multiple {
        assert_eq!(result.state, GitOperationState::Cancelled);
        assert!(result.completion_unknown);
        assert_eq!(
            git(second.path(), &["rev-parse", "refs/heads/main"]),
            previous
        );
    } else {
        assert_eq!(result.state, GitOperationState::Succeeded);
        assert_eq!(result.remote_oid, expected.head_oid);
        assert!(!result.completion_unknown);
        assert_eq!(
            git(first.path(), &["rev-parse", "refs/heads/main"]),
            previous
        );
    }
    service.shutdown().await;
}
#[cfg(unix)]
#[tokio::test]
async fn cancellation_reconciles_the_push_url_instead_of_fetch_url() {
    push_destination_case(false).await;
}
#[cfg(unix)]
#[tokio::test]
async fn cancellation_never_claims_success_for_ambiguous_multiple_push_urls() {
    push_destination_case(true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancel_kills_owned_helper_and_reconciles_actual_state_without_output() {
    cancelled_transfer(false).await;
}
#[cfg(unix)]
#[tokio::test]
async fn cancelled_pull_before_transfer_preserves_state_but_reports_unknown() {
    cancelled_transfer(true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_pull_after_merge_preserves_applied_head_and_reports_unknown() {
    use std::os::unix::fs::PermissionsExt;
    let r = repo();
    let d = r.path();
    let bare = tempfile::tempdir().expect("create owned temporary directory");
    git(bare.path(), &["init", "--bare", "-q"]);
    git(
        d,
        &[
            "remote",
            "add",
            "origin",
            bare.path().to_str().expect("owned UTF-8 path"),
        ],
    );
    git(d, &["push", "-qu", "origin", "main"]);
    let before = git(d, &["rev-parse", "HEAD"]);
    std::fs::write(d.join("pulled.txt"), "remote content").expect("write owned fixture");
    git(d, &["add", "pulled.txt"]);
    git(d, &["commit", "-qm", "remote update"]);
    git(d, &["push", "-q", "origin", "main"]);
    let after = git(d, &["rev-parse", "HEAD"]);
    git(d, &["reset", "--hard", before.trim()]);
    let expected = remote::state(d).await.expect("read owned remote state");
    let hook = d.join(".git/hooks/post-merge");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho ready > .git/owned-merge-done\nexec sleep 60\n",
    )
    .expect("write owned fixture");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
        .expect("set owned fixture permissions");
    let svc = Arc::new(GitService::new(Arc::new(|_| {}), Arc::new(|_| {})));
    let op = svc
        .begin(
            d.into(),
            target(),
            RemoteAction::Pull {
                expect: expected,
                rebase: false,
            },
        )
        .await
        .expect("start owned remote operation");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !d.join(".git/owned-merge-done").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned remote operation result");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        svc.cancel(&GitOperationQuery {
            target: target(),
            operation_id: op.id,
        }),
    )
    .await
    .expect("cancel owned operation")
    .expect("capture owned branch expectation");
    assert_eq!(git(d, &["rev-parse", "HEAD"]), after);
    assert_eq!(
        std::fs::read_to_string(d.join("pulled.txt")).expect("read owned fixture text"),
        "remote content"
    );
    svc.shutdown().await;
    assert_eq!(
        result
            .current
            .as_ref()
            .expect("owned optional value")
            .head_oid
            .as_deref(),
        Some(after.trim())
    );
    assert_eq!(result.state, GitOperationState::Cancelled);
    assert!(
        result.completion_unknown,
        "completed merge cannot be reported as known noncompletion"
    );
    assert!(result.error.is_none());
}
