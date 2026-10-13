use super::*;

#[test]
fn squash_messages_reject_before_quiesce_or_git_mutation() {
    cases(&[
        "squash-message-huge-fails",
        "squash-message-nul-fails",
        "squash-message-byte-over-fails",
        "squash-message-unicode-over-fails",
    ]);
}
#[test]
fn squash_messages_accept_boundary_default_and_raw_multiline_bytes() {
    cases(&[
        "squash-message-unicode",
        "squash-message-ascii",
        "squash-message-default",
        "squash-message-multiline",
        "squash-message-merge-ignored",
        "squash-message-rebase-ignored",
    ]);
}
pub(super) async fn case(mode: &str) {
    let (mut fixture, task, wt) = action_fixture().await;
    let service = Arc::get_mut(&mut fixture.svc).expect("owned service");
    service.cleanup_after_close = false;
    service.use_scope = true;
    fs::write(
        fixture.svc.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned configuration");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_TMUX_OWNER").expect("owned socket receipt"),
        fixture.svc.terminal.tmux().socket(),
    )
    .expect("socket receipt");
    let terminal = fixture
        .svc
        .terminal
        .create_internal(
            devbox_terminal::SessionKind::Agent,
            Some(fixture.project.clone()),
            wt.clone(),
            "Owned squash fixture".into(),
            "cat".into(),
            &[],
        )
        .await
        .expect("owned terminal");
    let terminal_id = terminal.id.clone();
    fixture
        .svc
        .update(&task.id, move |task| {
            task.terminal_id = Some(terminal_id);
            task.state = AgentState::Running;
            true
        })
        .await
        .expect("running agent");
    let before = fixture.svc.get(&task.id).await.expect("before task");
    let runtime = fixture.svc.launch_binding(&task.id).await.expect("runtime");
    let base = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("base");
    let head = devbox_git::run::resolve(&wt, "HEAD").await.expect("head");
    let worktree_file = fs::read(wt.join("tracked")).expect("owned content");
    let index = fs::read(fixture.repo.join(".git/index")).expect("shared index");
    let receipt = PathBuf::from(std::env::var_os("DEVBOX_SQUASH_RECEIPT").expect("argv receipt"));
    fs::remove_file(&receipt).expect("remove fixture commit receipt");
    let message = match mode {
        "squash-message-huge-fails" => Some("x".repeat(140 * 1024)),
        "squash-message-nul-fails" => Some("accepted\0bad".into()),
        "squash-message-byte-over-fails" => Some("a".repeat(64 * 1024 + 1)),
        "squash-message-unicode-over-fails" => Some(format!("{}a", "😀".repeat(16 * 1024))),
        "squash-message-unicode" => Some("😀".repeat(16 * 1024)),
        "squash-message-ascii" => Some("a".repeat(64 * 1024)),
        "squash-message-default" => None,
        "squash-message-multiline" => {
            Some("  한글 제목 😀  \n\n  body 'quotes' \"literal\" --flag  \n\n".into())
        }
        "squash-message-merge-ignored" | "squash-message-rebase-ignored" => {
            Some(format!("ignored\0{}", "x".repeat(140 * 1024)))
        }
        _ => panic!("unknown owned scenario"),
    };
    let confirmed = expect(&fixture, &task).await;
    let before_rev = fixture.svc.db.rev();
    let result = fixture
        .svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy: if mode == "squash-message-merge-ignored" {
                MergeKind::Merge
            } else if mode == "squash-message-rebase-ignored" {
                MergeKind::Rebase
            } else {
                MergeKind::Squash
            },
            message: message.clone(),
            expect: confirmed,
        })
        .await;
    if mode.ends_with("fails") {
        if mode == "squash-message-nul-fails" {
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::InvalidInput))),
                "NUL admission: {result:?}"
            );
        } else {
            assert!(
                matches!(
                    result,
                    Err(Fail::Domain(AgentsError::SquashMessageTooLong {
                        limit_bytes: 65536
                    }))
                ),
                "exact UTF8 limit: {result:?}"
            );
        }
        assert!(
            fixture
                .svc
                .terminal
                .tmux()
                .list()
                .await
                .expect("sessions")
                .iter()
                .any(|row| row.name == terminal.id),
            "invalid message must leave agent terminal live; error={result:?}"
        );
        assert_eq!(fixture.svc.db.rev(), before_rev);
        assert_eq!(
            serde_json::to_value(fixture.svc.get(&task.id).await.expect("task")).expect("after"),
            serde_json::to_value(before).expect("before")
        );
        assert_eq!(
            fixture.svc.launch_binding(&task.id).await.expect("runtime"),
            runtime
        );
        assert_eq!(
            devbox_git::run::resolve(&fixture.repo, "main")
                .await
                .expect("base"),
            base
        );
        assert_eq!(
            devbox_git::run::resolve(&wt, "HEAD").await.expect("head"),
            head
        );
        assert_eq!(
            fs::read(wt.join("tracked")).expect("content"),
            worktree_file
        );
        assert_eq!(
            fs::read(fixture.repo.join(".git/index")).expect("index"),
            index
        );
        assert!(!fixture.repo.join(".git/index.lock").exists());
        assert!(wt.exists());
        assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
        assert!(
            !receipt.exists(),
            "invalid message may not reach git commit"
        );
        assert!(
            !Path::new(&std::env::var_os("DEVBOX_CLOSE_SCOPE_MARKER").expect("marker")).exists()
        );
    } else {
        assert_eq!(
            result.expect("bounded real Git squash").state,
            AgentState::Closed
        );
        if mode.ends_with("ignored") {
            assert!(
                !receipt.exists(),
                "unused message is not passed to git commit"
            );
        } else {
            assert_eq!(
                fs::read(&receipt).expect("actual commit argv"),
                message
                    .unwrap_or_else(|| format!("{} (agent)", before.title))
                    .as_bytes(),
                "forwarded bytes are exact; native Git commit cleanup remains unchanged"
            );
        }
        assert_ne!(
            devbox_git::run::resolve(&fixture.repo, "main")
                .await
                .expect("new commit"),
            base
        );
        assert_eq!(
            devbox_git::run::git(&fixture.repo, &["rev-parse", "main^{tree}"])
                .await
                .expect("merged tree"),
            devbox_git::run::git(&wt, &["rev-parse", "HEAD^{tree}"])
                .await
                .expect("agent tree")
        );
        assert!(wt.exists());
    }
}
