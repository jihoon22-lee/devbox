mod support;
use devbox_notes::{CreateNote, NotesCreate};
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
