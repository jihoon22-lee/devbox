use super::*;

#[test]
fn pr_titles_reject_before_quiesce_or_push() {
    cases(&[
        "pr-title-huge-fails",
        "pr-title-nul-fails",
        "pr-title-byte-over-fails",
        "pr-title-unicode-over-fails",
    ]);
}

#[test]
fn pr_titles_accept_unicode_boundary_and_default_without_changing_input() {
    cases(&[
        "pr-title-unicode",
        "pr-title-ascii",
        "pr-title-default",
        "pr-title-spaces",
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
            "Owned title fixture".into(),
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
        .expect("running owned agent");
    let before = fixture.svc.get(&task.id).await.expect("before task");
    let base = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("base");
    let head = devbox_git::run::resolve(&wt, "HEAD").await.expect("head");
    let remote = fixture.root.path().join("remote.git");
    git(
        &fixture.repo,
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    git(
        &fixture.repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    let title = match mode {
        "pr-title-huge-fails" => Some("x".repeat(140 * 1024)),
        "pr-title-nul-fails" => Some("accepted\0bad".into()),
        "pr-title-byte-over-fails" => Some(format!("{}a", " ".repeat(64 * 1024))),
        "pr-title-unicode-over-fails" => Some(format!("{}a", "😀".repeat(16 * 1024))),
        "pr-title-unicode" => Some("😀".repeat(16 * 1024)),
        "pr-title-ascii" => Some("a".repeat(64 * 1024)),
        "pr-title-spaces" => Some("  한글 제목 😀  ".into()),
        "pr-title-default" => None,
        _ => panic!("unknown owned scenario"),
    };
    let before_rev = fixture.svc.db.rev();
    let result = fixture
        .svc
        .pr(PrAgent {
            id: task.id.clone(),
            title: title.clone(),
            body: Some("Owned title test".into()),
            expect: expect(&fixture, &task).await,
        })
        .await;
    if mode.ends_with("fails") {
        if mode == "pr-title-nul-fails" {
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::InvalidInput))),
                "NUL title must fail argv admission: {result:?}"
            );
        } else {
            assert!(
                matches!(
                    result,
                    Err(Fail::Domain(AgentsError::PrTitleTooLong {
                        limit_bytes: 65536
                    }))
                ),
                "oversized title needs exact typed limit: {result:?}"
            );
        }
        assert_eq!(
            fixture.svc.db.rev(),
            before_rev,
            "invalid title may not commit a revision"
        );
        let after = fixture.svc.get(&task.id).await.expect("retained task");
        assert_eq!(
            serde_json::to_value(&after).expect("after"),
            serde_json::to_value(&before).expect("before"),
            "invalid input may not mutate task history/state"
        );
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
            "agent terminal must remain live"
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
        assert!(
            devbox_git::branches(&remote)
                .await
                .expect("remote refs")
                .is_empty(),
            "invalid title may not push"
        );
        assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
        for key in [
            "DEVBOX_CLOSE_SCOPE_MARKER",
            "DEVBOX_CLOSE_GH_MARKER",
            "DEVBOX_PR_TITLE_RECEIPT",
        ] {
            assert!(
                !Path::new(&std::env::var_os(key).expect("owned marker")).exists(),
                "invalid title must not call scope stop or gh: {key}"
            );
        }
    } else {
        assert_eq!(
            result.expect("accepted bounded title").state,
            AgentState::Closed
        );
        assert_eq!(
            fs::read_to_string(std::env::var_os("DEVBOX_PR_TITLE_RECEIPT").expect("receipt"))
                .expect("gh argv title"),
            title.unwrap_or(before.title),
            "validated title must reach gh unchanged"
        );
        assert_eq!(
            devbox_git::branches(&remote).await.expect("remote refs"),
            vec!["agent/review".to_owned()]
        );
        assert!(wt.exists());
    }
}
