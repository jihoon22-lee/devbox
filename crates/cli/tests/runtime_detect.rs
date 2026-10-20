mod support;
use devbox_cli::client::ClientError;
use devbox_runtime::*;
use support::TestDaemon;

#[tokio::test]
async fn runtime_detect_query_suggestions_define_round_trip_and_typed_error() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let repo = daemon.git_repo("projects/detect");
    let project = daemon.add_project(&client, &repo).await;
    let repo = std::path::Path::new(&repo);
    std::fs::write(
        repo.join("package.json"),
        r#"{"scripts":{"dev":"touch marker"}}"#,
    )
    .expect("fixture result");
    std::fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").expect("fixture result");
    let request = DetectRuns {
        project_id: project.clone(),
    };
    let items = client
        .call::<RuntimeDetect>(&request)
        .await
        .expect("fixture result");
    assert_eq!(items.len(), 1);
    let suggestion = &items[0];
    assert_eq!(suggestion.id, "dev");
    assert_eq!(suggestion.command, "pnpm dev");
    assert_eq!(suggestion.kind, DefinitionKind::Service);
    assert_eq!(suggestion.from, SuggestionSource::PackageJson);
    // UI/MCP can display the typed command/source and explicitly save through runtime.define.
    client
        .call::<RuntimeDefine>(&DefineRun {
            project_id: project,
            def: RunDef::Service(ServiceDef {
                id: suggestion.id.clone(),
                command: suggestion.command.clone(),
                ..Default::default()
            }),
            save_to: SaveTo::ThisPc,
        })
        .await
        .expect("fixture result");
    assert!(
        client
            .call::<RuntimeDetect>(&request)
            .await
            .expect("fixture result")
            .is_empty()
    );
    assert!(!repo.join("marker").exists());
    std::fs::write(repo.join("package.json"), "{broken").expect("fixture result");
    assert!(
        matches!(client.call::<RuntimeDetect>(&request).await.expect_err("expected failure"),
        ClientError::Rpc { code, .. } if code == "runtime.detect_parse")
    );
}
