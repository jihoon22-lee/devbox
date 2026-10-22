mod support;
use devbox_notes::{
    CreateNote, NotePath, NotesBacklinks, NotesCreate, NotesRename, NotesRenameStatus, RenameNote,
    RenameOp, RenameState,
};
use std::time::{Duration, Instant};
use support::TestDaemon;

fn with_config(text: &str) -> TestDaemon {
    let d = TestDaemon::fixture();
    std::fs::create_dir_all(&d.paths.config_dir).expect("create owned config");
    std::fs::write(d.paths.config_dir.join("config.toml"), text).expect("write owned config");
    d
}

#[tokio::test]
async fn configured_vault_is_used_for_documents_and_logs() {
    let mut d = with_config("[notes]\nvault = \"~/n2\"\n");
    d.launch();
    let home = d.home.path().to_path_buf();
    let client = d.client().await;
    let created = support::bounded(client.call::<NotesCreate>(&CreateNote {
        folder: "".into(),
        title: "hello".into(),
        template: None,
    }))
    .await
    .expect("notes.create");
    assert_eq!(created.path, "hello.md");
    assert_eq!(
        std::fs::read_to_string(home.join("n2/hello.md")).expect("note in the configured vault"),
        "# hello\n"
    );
    assert!(
        !home.join("notes").exists(),
        "the default vault is not used"
    );
    // The configured vault did not exist, so the daemon created it with `.trash/` ignored.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !home.join("n2/.gitignore").exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        std::fs::read(home.join("n2/.gitignore")).expect("new vault .gitignore"),
        b".trash/\n"
    );
    let read = client
        .raw_call(
            "documents.read",
            serde_json::json!({"key": {"root": "vault", "path": "hello.md"}}),
            None,
        )
        .await
        .expect("documents use the configured vault");
    assert!(read.contains("# hello"), "{read}");

    std::fs::write(home.join("n2/x.log"), "inside\n").expect("fixture log");
    std::fs::create_dir_all(home.join("notes")).expect("fixture default folder");
    std::fs::write(home.join("notes/x.log"), "outside\n").expect("fixture log");
    let search = |path: std::path::PathBuf| {
        let client = &client;
        async move {
            client
                .raw_call(
                    "logs.search",
                    serde_json::json!({
                        "sources": [{"kind": "file", "path": path}],
                        "query": "", "regex": false, "limit": 1
                    }),
                    None,
                )
                .await
        }
    };
    search(home.join("n2/x.log"))
        .await
        .expect("logs inside the configured vault are allowed");
    let error = search(home.join("notes/x.log"))
        .await
        .expect_err("the default vault is no longer a log root");
    assert!(
        format!("{error:?}").contains("observe.file_outside_roots"),
        "{error:?}"
    );
    d.stop();
}

#[test]
fn unknown_notes_settings_stop_service_assembly() {
    for text in ["[notes]\nvalt = '~/x'\n", "[notes]\nvault = 'relative'\n"] {
        let d = with_config(text);
        let built = devbox_cli::daemon::build_services(d.paths.clone());
        assert!(built.is_err(), "{text}");
    }
}

/// AR-D01 through the real daemon: it stops right after moving the note (test hook,
/// debug test instances only), is stopped, and the next start finishes the rewrites.
#[tokio::test]
async fn rename_resumes_after_daemon_restart() {
    let mut d = TestDaemon::fixture();
    let vault = d.home.path().join("notes");
    std::fs::create_dir_all(&vault).expect("fixture vault");
    for (rel, text) in [
        ("a.md", "x\n"),
        ("b.md", "[[a]]\n"),
        ("c.md", "[a](a.md)\n"),
    ] {
        std::fs::write(vault.join(rel), text).expect("fixture note");
    }
    d.extra_env
        .push(("DEVBOX_TEST_NOTES_STOP".into(), "after_move".into()));
    d.launch();
    let client = d.client().await;
    // The first scan runs in the background; rename plans from the index.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let back = support::bounded(client.call::<NotesBacklinks>(&NotePath {
            path: "a.md".into(),
        }))
        .await
        .expect("notes.backlinks");
        if back.items.len() == 2 {
            break;
        }
        assert!(Instant::now() < deadline, "first scan indexes both links");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let started = support::bounded(client.call::<NotesRename>(&RenameNote {
        path: "a.md".into(),
        new_path: "z/a2.md".into(),
        rewrite_links: true,
    }))
    .await
    .expect("notes.rename");
    assert!(vault.join("z/a2.md").exists() && !vault.join("a.md").exists());
    assert_eq!(
        std::fs::read_to_string(vault.join("b.md")).expect("b"),
        "[[a]]\n"
    );
    drop(client);
    d.stop();
    let journals = d.paths.data_dir.join("notes-rename");
    assert_eq!(
        std::fs::read_dir(&journals)
            .expect("journal folder")
            .count(),
        1
    );

    d.extra_env.retain(|(k, _)| k != "DEVBOX_TEST_NOTES_STOP");
    d.launch();
    let client = d.client().await;
    let op = RenameOp {
        op_id: started.op_id,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = support::bounded(client.call::<NotesRenameStatus>(&op)).await;
        if matches!(&status, Ok(s) if s.state != RenameState::Running) {
            assert_eq!(status.expect("status").state, RenameState::Done);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "resumed rename finishes: {status:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        std::fs::read_to_string(vault.join("b.md")).expect("b"),
        "[[a2]]\n"
    );
    assert_eq!(
        std::fs::read_to_string(vault.join("c.md")).expect("c"),
        "[a](z/a2.md)\n"
    );
    assert_eq!(
        std::fs::read_dir(&journals)
            .expect("journal folder")
            .count(),
        0
    );
    d.stop();
}
