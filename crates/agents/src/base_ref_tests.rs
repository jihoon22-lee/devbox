use super::*;

#[test]
fn invalid_base_ref_rejects_inline_before_reservation() {
    child(
        "invalid_base_ref_rejects_inline_before_reservation",
        "invalid-inline",
    );
}
#[test]
fn invalid_base_ref_rejects_worktree_before_reservation() {
    child(
        "invalid_base_ref_rejects_worktree_before_reservation",
        "invalid-worktree",
    );
}
#[test]
fn detached_implicit_base_requires_explicit_input_before_reservation() {
    child(
        "detached_implicit_base_requires_explicit_input_before_reservation",
        "detached",
    );
}
#[test]
fn valid_explicit_base_on_detached_and_implicit_current_branch_are_accepted() {
    child(
        "valid_explicit_base_on_detached_and_implicit_current_branch_are_accepted",
        "valid",
    );
}

async fn case(mode: &str) {
    let cases = if mode == "valid" { 3 } else { 1 };
    for index in 0..cases {
        let fixture = Fixture::new().await;
        let marker = PathBuf::from(std::env::var_os("DEVBOX_BASE_REF_MARKER").expect("receipt"));
        let _ = fs::remove_file(&marker);
        let mut request = fixture.request();
        request.worktree = index == 1 || mode == "invalid-worktree";
        if mode == "valid" && index == 2 {
            git(&fixture.repo, &["checkout", "-b", "feature"]);
            request.base_branch = None;
        } else if mode.starts_with("invalid") {
            request.base_branch = Some("missing-owned-base".into());
        } else {
            git(&fixture.repo, &["checkout", "--detach", "HEAD"]);
            request.base_branch = if mode == "detached" {
                None
            } else {
                Some("main".into())
            };
        }
        let before = devbox_git::branches(&fixture.repo).await.expect("branches");
        let result = fixture.svc.create(request).await;
        fixture.drain().await;
        if mode != "valid" {
            assert!(
                matches!(
                    result,
                    Err(devbox_protocol::Fail::Domain(AgentsError::InvalidInput))
                ),
                "base validation must precede reservation: {result:?}"
            );
            let counts = fixture
                .svc
                .db
                .read(|tx| {
                    let mut counts = vec![];
                    for table in ["agents_tasks", "agents_runtime", "terminal_sessions"] {
                        counts.push(tx.query_row(
                            &format!("SELECT COUNT(*) FROM {table}"),
                            [],
                            |row| row.get::<_, u32>(0),
                        )?);
                    }
                    Ok(counts)
                })
                .await
                .expect("side effects");
            assert_eq!(counts, vec![0, 0, 0], "no task, reservation or terminal");
            assert!(!marker.exists(), "no process launch attempted");
            assert!(
                !fixture.svc.paths.data_dir.join("agents").exists(),
                "no task runtime files"
            );
            assert_eq!(
                devbox_git::branches(&fixture.repo)
                    .await
                    .expect("unchanged branches"),
                before
            );
        } else {
            let reserved = result.expect("valid base accepted");
            let task = fixture.svc.get(&reserved.id).await.expect("prepared task");
            assert_ne!(task.state, AgentState::Failed, "{task:?}");
            assert_eq!(
                task.base_branch,
                if index == 2 { "feature" } else { "main" }
            );
            assert!(
                task.terminal_id.is_some(),
                "preparation published owned terminal"
            );
            assert!(marker.exists(), "owned fake process launch receipt");
            fixture
                .svc
                .changes(&task.id)
                .await
                .expect("valid base supports review");
            fixture
                .svc
                .discard_check(&task.id)
                .await
                .expect("valid base supports discard confirmation");
            if index == 1 {
                assert!(Path::new(task.worktree_path.as_deref().expect("worktree")).exists());
            }
        }
    }
}
fn child(test: &str, mode: &str) {
    if let Ok(mode) = std::env::var("DEVBOX_BASE_REF_CASE") {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(case(&mode));
        return;
    }
    let root = tempfile::tempdir().expect("owned fake terminal");
    let wrapper = root.path().join("tmux");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$DEVBOX_BASE_REF_MARKER"
case "$*" in *has-session*) echo "can't find session" >&2; exit 1;; esac
exit 0
"#,
    )
    .expect("fake terminal receipt");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).expect("mode");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            &format!("service::resource_tests::base_ref_tests::{test}"),
            "--nocapture",
        ])
        .env("DEVBOX_BASE_REF_CASE", mode)
        .env("DEVBOX_BASE_REF_MARKER", root.path().join("marker"))
        .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
        .output()
        .expect("owned child");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}
