mod support;
use devbox_notes::{
    CreateNote, DailyParams, InsertSummary, NotePath, NotesBacklinks, NotesCreate, NotesDaily,
    NotesInsertSummary, NotesRename, NotesRenameStatus, RenameNote, RenameOp, RenameState,
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

/// Invalid `[notes]`/`[search]` turn those features off with a reported problem; the
/// daemon still starts and never falls back to the default `~/notes` vault.
#[tokio::test]
async fn invalid_notes_and_search_settings_turn_those_features_off() {
    let mut d = with_config("[notes]\nvault = 'relative'\n[search]\nexclud = []\n");
    d.launch();
    let client = d.client().await;
    let refused = |result: Result<String, devbox_cli::client::ClientError>| match result {
        Err(devbox_cli::client::ClientError::Rpc { code, .. }) => code,
        other => panic!("expected a refusal, got {other:?}"),
    };
    let empty = serde_json::json!({});
    assert_eq!(
        refused(client.raw_call("notes.tags", &empty, None).await),
        "notes.disabled"
    );
    assert_eq!(
        refused(
            client
                .raw_call("notes.read", serde_json::json!({ "path": "a.md" }), None)
                .await
        ),
        "notes.disabled"
    );
    assert_eq!(
        refused(client.raw_call("search.saved", &empty, None).await),
        "search.disabled"
    );
    assert_eq!(
        refused(
            client
                .raw_call(
                    "documents.read",
                    serde_json::json!({ "key": { "root": "vault", "path": "a.md" } }),
                    None
                )
                .await
        ),
        "documents.out_of_scope"
    );
    let info: serde_json::Value = serde_json::from_str(
        &client
            .raw_call("system.info", &empty, None)
            .await
            .expect("system.info"),
    )
    .expect("system.info JSON");
    let file = d.paths.config_dir.join("config.toml").display().to_string();
    let sections: Vec<_> = info["startupProblems"]
        .as_array()
        .expect("startup problems")
        .iter()
        .map(|p| {
            assert_eq!(p["kind"], "configSection", "{p}");
            assert_eq!(p["file"], file.as_str(), "{p}");
            p["section"].as_str().expect("section").to_owned()
        })
        .collect();
    assert_eq!(sections, ["notes", "search"]);
    assert!(!d.home.path().join("notes").exists());
    // Nor is the default vault a log root while notes are off.
    let log = d.home.path().join("notes/x.log");
    std::fs::create_dir_all(log.parent().expect("fixture folder")).expect("fixture folder");
    std::fs::write(&log, "outside\n").expect("fixture log");
    assert_eq!(
        refused(
            client
                .raw_call(
                    "logs.search",
                    serde_json::json!({
                        "sources": [{"kind": "file", "path": log}],
                        "query": "", "regex": false, "limit": 1
                    }),
                    None,
                )
                .await
        ),
        "observe.file_outside_roots"
    );
}

/// A domain section is checked in each file: a valid `config.local.toml` copy does not
/// rescue an invalid `config.toml` one.
#[tokio::test]
async fn a_valid_local_notes_section_does_not_rescue_an_invalid_shared_one() {
    let mut d = with_config("[notes]\nvault = 'relative'\n");
    std::fs::write(
        d.paths.config_dir.join("config.local.toml"),
        "[notes]\nvault = '~/n'\n",
    )
    .expect("write owned local config");
    d.launch();
    let client = d.client().await;
    let empty = serde_json::json!({});
    match client.raw_call("notes.tags", &empty, None).await {
        Err(devbox_cli::client::ClientError::Rpc { code, .. }) => {
            assert_eq!(code, "notes.disabled")
        }
        other => panic!("expected notes to be off, got {other:?}"),
    }
    let info: serde_json::Value = serde_json::from_str(
        &client
            .raw_call("system.info", &empty, None)
            .await
            .expect("system.info"),
    )
    .expect("system.info JSON");
    let problems = info["startupProblems"]
        .as_array()
        .expect("startup problems");
    assert_eq!(problems.len(), 1, "{problems:?}");
    let file = d.paths.config_dir.join("config.toml").display().to_string();
    assert_eq!(problems[0]["kind"], "configSection");
    assert_eq!(problems[0]["file"], file.as_str());
    assert_eq!(problems[0]["section"], "notes");
    assert!(
        problems[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("vault")),
        "{problems:?}"
    );
    assert!(!d.home.path().join("n").exists());
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

/// Task 6 through the real daemon: `notes.daily` creates once, `devbox note add
/// --daily` appends at the end, and the summary lands under this PC's heading.
#[tokio::test]
async fn daily_and_append_round_trip() {
    let mut d = with_config("[pc]\nname = \"pc-a\"\n");
    d.launch();
    let client = d.client().await;
    let first = support::bounded(client.call::<NotesDaily>(&DailyParams { date: None }))
        .await
        .expect("notes.daily");
    let again = support::bounded(client.call::<NotesDaily>(&DailyParams { date: None }))
        .await
        .expect("notes.daily again");
    assert_eq!((first.created, again.created), (true, false));
    assert_eq!(first.path, again.path);
    assert!(first.path.starts_with("daily/") && first.path.ends_with(".md"));
    let file = d.home.path().join("notes").join(&first.path);

    let mut add = d.environment(env!("CARGO_BIN_EXE_devbox"));
    add.args(["note", "add", "--daily", "hi"]);
    let status = add.status().expect("run devbox note add");
    assert!(status.success(), "devbox note add --daily hi");
    let text = std::fs::read_to_string(&file).expect("daily note");
    assert!(text.ends_with("\nhi\n"), "{text}");

    let inserted =
        support::bounded(client.call::<NotesInsertSummary>(&InsertSummary { date: None }))
            .await
            .expect("notes.insert_summary");
    assert_eq!(inserted.path, first.path);
    let text = std::fs::read_to_string(&file).expect("daily note");
    assert!(
        text.contains("\nhi\n\n## pc-a\n<!-- devbox:summary:pc-a -->\n### 오늘 한 일\n"),
        "{text}"
    );
    assert!(text.ends_with("<!-- /devbox:summary -->\n"), "{text}");
    d.stop();
}
