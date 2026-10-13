mod support;
use devbox_protocol::Fail;
use devbox_terminal::*;
use support::Harness;

const TITLE_LIMIT: usize = 256;
fn create(h: &Harness, title: String) -> CreateSession {
    CreateSession {
        project_id: None,
        cwd: Some(h.home.path().display().to_string()),
        title: Some(title),
        command: Some("exec sleep 60".into()),
        attach_zellij: None,
    }
}
fn too_long<T>(result: Result<T, Fail<TerminalError>>) {
    let Err(Fail::Domain(error)) = result else {
        panic!("oversized terminal title must return a typed domain error");
    };
    assert_eq!(
        serde_json::to_value(error).expect("serialize bounded error"),
        serde_json::json!({"code":"terminal.title_too_long", "detail":{"limit":256}})
    );
}
#[tokio::test]
async fn oversized_create_titles_have_no_metadata_or_tmux_side_effects() {
    let h = Harness::new();
    let mut events = h.hub.subscribe("terminal.changed");
    too_long(
        h.terminal
            .create(create(&h, "x".repeat(9 * 1024 * 1024)))
            .await,
    );
    too_long(
        h.terminal
            .create_internal(
                SessionKind::Agent,
                None,
                h.home.path().into(),
                "가".repeat(TITLE_LIMIT + 1),
                "exec sleep 60".into(),
                &[],
            )
            .await,
    );
    // Generated zellij titles must cross the same daemon boundary.
    let mut generated = create(&h, String::new());
    generated.title = None;
    generated.attach_zellij = Some("z".repeat(TITLE_LIMIT));
    too_long(h.terminal.create(generated).await);
    assert_eq!(h.db.rev(), 0);
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("no owned sessions")
            .is_empty()
    );
    assert!(
        h.terminal
            .list(ListSessions { project_id: None })
            .await
            .expect("no metadata")
            .items
            .is_empty()
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn oversized_rename_preserves_title_revision_and_notifications() {
    let h = Harness::new();
    let session = h
        .terminal
        .create(create(&h, "original".into()))
        .await
        .expect("original terminal");
    let before = h.db.rev();
    let mut events = h.hub.subscribe("terminal.changed");
    for title in [
        "a".repeat(TITLE_LIMIT + 1),
        "가".repeat(TITLE_LIMIT + 1),
        "😀".repeat(TITLE_LIMIT + 1),
    ] {
        too_long(
            h.terminal
                .rename(RenameSession {
                    id: session.id.clone(),
                    title,
                })
                .await,
        );
        assert_eq!(h.db.rev(), before);
    }
    let list = h
        .terminal
        .list(ListSessions { project_id: None })
        .await
        .expect("manageable list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].title, "original");
    assert!(
        serde_json::to_vec(&list).expect("list JSON").len()
            < devbox_protocol::frame::MAX_JSON_BYTES
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn title_boundary_counts_unicode_characters_and_preserves_short_text() {
    let h = Harness::new();
    let title = "가".repeat(TITLE_LIMIT);
    let session = h
        .terminal
        .create(create(&h, title.clone()))
        .await
        .expect("Unicode boundary create");
    assert_eq!(session.title, title);
    for title in [
        "a".repeat(TITLE_LIMIT),
        "😀".repeat(TITLE_LIMIT),
        "  unchanged  ".into(),
        String::new(),
    ] {
        h.terminal
            .rename(RenameSession {
                id: session.id.clone(),
                title: title.clone(),
            })
            .await
            .expect("allowed rename");
        assert_eq!(
            h.terminal
                .list(ListSessions { project_id: None })
                .await
                .expect("list")
                .items[0]
                .title,
            title
        );
    }
}
