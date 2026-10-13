mod support;
use devbox_protocol::Fail;
use devbox_terminal::*;
use support::Harness;

#[tokio::test]
async fn oversized_commands_are_rejected_before_tmux_or_metadata() {
    let h = Harness::new();
    let command = format!("sleep 60 #{}", "x".repeat(65_527));
    assert_eq!(command.len(), 65_537);
    let result = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: Some("owned oversized command".into()),
            command: Some(command),
            attach_zellij: None,
        })
        .await;
    let Err(Fail::Domain(error)) = result else {
        panic!("oversized command must fail admission before a session exists")
    };
    assert_eq!(
        serde_json::to_value(error).expect("typed failure")["code"],
        "terminal.command_too_long"
    );
    assert_eq!(h.db.rev(), 0);
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("owned tmux")
            .is_empty()
    );
}

async fn legacy_commands(h: &Harness) {
    let mut directory = h.home.path().to_path_buf();
    for _ in 0..30 {
        directory.push("d".repeat(100));
    }
    std::fs::create_dir_all(&directory).expect("owned valid long working directory");
    let cwd = directory.display().to_string();
    h.db.write(move |tx| {
        let command = format!("sleep 60 #{}", "\\".repeat(32_768));
        let title = "😀".repeat(256);
        for i in 0..600 {
            tx.execute("INSERT INTO terminal_sessions(id,project_id,title,cwd,kind,command,created_ms) VALUES(?1,NULL,?2,?3,'shell',?4,?5)",
                rusqlite::params![format!("t{:032x}", 600-i), title, cwd, command, i/2])?;
        }
        Ok(())
    }).await.expect("owned realistic ended rows");
}

#[tokio::test]
async fn ended_commands_cannot_make_a_terminal_list_exceed_its_response_budget() {
    let h = Harness::new();
    legacy_commands(&h).await;
    let list = h
        .terminal
        .list(ListSessions {
            project_id: None,
            after_id: None,
        })
        .await
        .expect("owned terminal list");
    let bytes = serde_json::to_vec(&list).expect("wire list").len();
    assert!(
        bytes <= 768 * 1024,
        "ended command rows produced {bytes} response bytes"
    );
}

#[tokio::test]
async fn command_admission_covers_internal_and_zellij_paths_and_utf8_boundary() {
    let h = Harness::new();
    for command in ["가".repeat(21_846), "echo\0bad".into()] {
        let error = h
            .terminal
            .create_internal(
                SessionKind::Agent,
                None,
                h.home.path().into(),
                "owned".into(),
                command,
                &[],
            )
            .await
            .expect_err("invalid internal command");
        assert!(matches!(
            error,
            Fail::Domain(TerminalError::CommandTooLong { .. } | TerminalError::CommandInvalid)
        ));
    }
    let error = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: Some("short title".into()),
            command: None,
            attach_zellij: Some("z".repeat(65_536)),
        })
        .await
        .expect_err("generated command limit");
    assert!(matches!(
        error,
        Fail::Domain(TerminalError::CommandTooLong { .. })
    ));
    assert_eq!(h.db.rev(), 0);
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("no owned processes")
            .is_empty()
    );
    let command = format!("sleep 60 #{}", "가".repeat(21_842));
    assert_eq!(command.len(), 65_536);
    validate_command(&command).expect("exact Unicode admission boundary");
    // tmux imposes its own encoded-command limit after admission.
    let command = format!("sleep 60 #{}", "가".repeat(1_024));
    let created = h
        .terminal
        .create_internal(
            SessionKind::Shell,
            None,
            h.home.path().into(),
            "boundary".into(),
            command.clone(),
            &[],
        )
        .await
        .expect("owned Unicode command");
    assert_eq!(created.command, command);
    let id = created.id.clone();
    let stored: String =
        h.db.read(move |tx| {
            tx.query_row(
                "SELECT command FROM terminal_sessions WHERE id=?1",
                [id],
                |r| r.get(0),
            )
        })
        .await
        .expect("full command persisted");
    assert_eq!(stored, command);
}

#[tokio::test]
async fn bounded_pages_preserve_all_legacy_rows_in_stable_order_and_allow_late_removal() {
    let h = Harness::new();
    legacy_commands(&h).await;
    let mut after_id = None;
    let mut seen = vec![];
    let mut pages = 0;
    loop {
        let page = h
            .terminal
            .list(ListSessions {
                project_id: None,
                after_id,
            })
            .await
            .expect("page");
        assert!(serde_json::to_vec(&page).expect("page bytes").len() <= 768 * 1024);
        assert!(page.items.len() <= 256);
        assert_eq!(page.rev, "1");
        assert!(page.items.iter().all(|s| !s.alive
            && s.command.ends_with("… (생략)")
            && s.command.chars().count() == 262));
        seen.extend(page.items.into_iter().map(|s| (s.created_ms, s.id)));
        pages += 1;
        after_id = page.next_after_id;
        if after_id.is_none() {
            break;
        }
        assert!(pages < 10);
    }
    assert!(pages >= 3);
    assert_eq!(seen.len(), 600);
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        h.terminal
            .all_sessions(None)
            .await
            .expect("internal complete summary")
            .len(),
        600
    );
    let last = seen.last().expect("last page").1.clone();
    let id = last.clone();
    let raw: String =
        h.db.read(move |tx| {
            tx.query_row(
                "SELECT command FROM terminal_sessions WHERE id=?1",
                [id],
                |r| r.get(0),
            )
        })
        .await
        .expect("legacy raw command");
    assert_eq!(raw.len(), 32_778);
    h.terminal
        .close(CloseSession { id: last })
        .await
        .expect("remove ended later-page row");
    assert_eq!(
        h.terminal
            .all_sessions(None)
            .await
            .expect("all remaining")
            .len(),
        599
    );
}

#[tokio::test]
async fn cursor_validation_project_filter_and_unicode_preview_are_bounded() {
    let h = Harness::new();
    for after_id in [
        "bad".into(),
        "1:".into(),
        "1:../x".into(),
        "x:id".into(),
        format!("1:{}", "a".repeat(129)),
    ] {
        assert!(matches!(
            h.terminal
                .list(ListSessions {
                    project_id: None,
                    after_id: Some(after_id)
                })
                .await,
            Err(Fail::Domain(TerminalError::InvalidCursor))
        ));
    }
    h.db.write(|tx| {
        for (id, project, command) in [
            ("a", "p", "😀".repeat(300)),
            ("b", "q", "other".into()),
            ("c", "p", "short".into()),
        ] {
            tx.execute(
                "INSERT INTO terminal_sessions VALUES(?1,?2,'owned','/tmp','shell',?3,1)",
                rusqlite::params![id, project, command],
            )?;
        }
        Ok(())
    })
    .await
    .expect("owned rows");
    let page = h
        .terminal
        .list(ListSessions {
            project_id: Some("p".into()),
            after_id: None,
        })
        .await
        .expect("filter");
    assert_eq!(
        page.items.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["a", "c"]
    );
    assert_eq!(
        page.items[0].command,
        format!("{}… (생략)", "😀".repeat(256))
    );
    assert_eq!(page.items[1].command, "short");
    assert!(page.next_after_id.is_none());
}
