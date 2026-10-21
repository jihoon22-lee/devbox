use crate::{detect::detect, *};
use devbox_protocol::Fail;
use std::{collections::BTreeSet, path::PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/detect")
        .join(name)
}

#[tokio::test]
async fn detect_package_managers_and_service_kinds() {
    for (manager, command) in [
        ("pnpm", "pnpm run build"),
        ("yarn", "yarn run build"),
        ("npm", "npm run build"),
        ("bun", "bun run build"),
    ] {
        let items = detect(&fixture(manager), &[])
            .await
            .expect("fixture result");
        assert_eq!(
            items
                .iter()
                .find(|s| s.id == "build")
                .expect("fixture value")
                .command,
            command
        );
        assert!(
            items
                .iter()
                .all(|s| s.from == SuggestionSource::PackageJson)
        );
    }
    let items = detect(&fixture("pnpm"), &[]).await.expect("fixture result");
    for id in ["dev", "start", "serve"] {
        assert_eq!(
            items
                .iter()
                .find(|s| s.id == id)
                .expect("fixture value")
                .kind,
            DefinitionKind::Service
        );
    }
    assert_eq!(
        items
            .iter()
            .find(|s| s.id == "dev")
            .expect("fixture value")
            .command,
        "pnpm dev"
    );
    assert_eq!(
        items
            .iter()
            .find(|s| s.id == "build")
            .expect("fixture value")
            .kind,
        DefinitionKind::Task
    );
}

#[tokio::test]
async fn detect_static_make_just_cargo_and_compose_fixtures() {
    for (name, expected) in [
        (
            "make",
            vec![
                ("all", "make all"),
                ("build", "make build"),
                ("clean", "make clean"),
            ],
        ),
        (
            "just",
            vec![
                ("_build", "just _build"),
                ("default", "just default"),
                ("serve", "just serve"),
            ],
        ),
        (
            "cargo",
            vec![("build", "cargo build"), ("test", "cargo test")],
        ),
        ("compose", vec![("compose-up", "docker compose up")]),
        ("compose-legacy", vec![("compose-up", "docker compose up")]),
    ] {
        let items = detect(&fixture(name), &[]).await.expect("fixture result");
        assert_eq!(
            items
                .iter()
                .map(|s| (s.id.as_str(), s.command.as_str()))
                .collect::<Vec<_>>(),
            expected,
            "{name}"
        );
        if name.starts_with("compose") {
            assert_eq!(items[0].kind, DefinitionKind::Service);
        }
        let source = match name {
            "make" => "Makefile",
            "just" => "justfile",
            "cargo" => "Cargo.toml",
            "compose" => "compose.yaml",
            _ => "docker-compose.yml",
        };
        assert_eq!(
            serde_json::to_string(&items[0].from).expect("fixture result"),
            format!("\"{source}\"")
        );
    }
}

#[tokio::test]
async fn detect_lock_priority_fallback_and_bun_binary_lock() {
    let root = tempfile::tempdir().expect("fixture result");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"build":"echo build"}}"#,
    )
    .expect("fixture result");
    let locks = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
        ("npm-shrinkwrap.json", "npm"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ];
    for (file, _) in locks {
        std::fs::write(root.path().join(file), b"fixture").expect("fixture result");
    }
    for (file, manager) in locks {
        assert_eq!(
            detect(root.path(), &[]).await.expect("fixture result")[0].command,
            format!("{manager} run build")
        );
        std::fs::remove_file(root.path().join(file)).expect("fixture result");
    }
    assert_eq!(
        detect(root.path(), &[]).await.expect("fixture result")[0].command,
        "npm run build"
    );
}

#[tokio::test]
async fn detect_duplicates_are_deterministic_and_existing_ids_stay_excluded() {
    let root = tempfile::tempdir().expect("fixture result");
    for (name, text) in [
        ("package.json", r#"{"scripts":{"build":"echo package"}}"#),
        ("Makefile", "build:\n\techo make\n"),
        ("justfile", "build:\n    echo just\n"),
        ("Cargo.toml", ""),
        ("compose.yaml", ""),
        ("docker-compose.yml", ""),
    ] {
        std::fs::write(root.path().join(name), text).expect("fixture result");
    }
    let items = detect(root.path(), &[]).await.expect("fixture result");
    assert_eq!(items.iter().filter(|s| s.id == "build").count(), 1);
    assert_eq!(
        items
            .iter()
            .find(|s| s.id == "build")
            .expect("fixture value")
            .from,
        SuggestionSource::PackageJson
    );
    assert_eq!(
        items
            .iter()
            .find(|s| s.id == "compose-up")
            .expect("fixture value")
            .from,
        SuggestionSource::ComposeYaml
    );
    assert!(
        detect(root.path(), &["build".into(), "compose-up".into()])
            .await
            .expect("fixture result")
            .iter()
            .all(|s| s.id == "test")
    );
    assert_eq!(
        items,
        detect(root.path(), &[]).await.expect("fixture result")
    );
}

#[tokio::test]
async fn detect_marker_files_are_read_without_execution() {
    let root = tempfile::tempdir().expect("fixture result");
    let marker = root.path().join("detect-spawn-marker");
    for (name, file) in [("make", "Makefile"), ("just", "justfile")] {
        let content = std::fs::read_to_string(fixture(name).join(file))
            .expect("fixture result")
            .replace(
                "detect-spawn-marker",
                marker.to_str().expect("fixture value"),
            );
        std::fs::write(root.path().join(file), content).expect("fixture result");
    }
    assert!(
        !detect(root.path(), &[])
            .await
            .expect("fixture result")
            .is_empty()
    );
    assert!(
        !marker.exists(),
        "detection must never evaluate make shell or just backticks/recipes"
    );
}

#[tokio::test]
async fn detect_parse_and_io_failures_are_typed_not_empty_success() {
    let root = tempfile::tempdir().expect("fixture result");
    for content in ["{", r#"{"scripts":{"build":17}}"#, r#"{"scripts":[]}"#] {
        std::fs::write(root.path().join("package.json"), content).expect("fixture result");
        assert!(matches!(
            detect(root.path(), &[]).await,
            Err(RuntimeError::DetectParse { .. })
        ));
    }
    std::fs::remove_file(root.path().join("package.json")).expect("fixture result");
    std::fs::create_dir(root.path().join("package.json")).expect("fixture result");
    assert!(matches!(
        detect(root.path(), &[]).await,
        Err(RuntimeError::DetectIo { .. })
    ));
}

#[tokio::test]
async fn detect_script_names_preserve_literal_shell_arguments() {
    let root = tempfile::tempdir().expect("fixture result");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"a'; touch marker; echo '":"ignored","with space":"ignored"}}"#,
    )
    .expect("fixture result");
    let items = detect(root.path(), &[]).await.expect("fixture result");
    for item in &items {
        assert_eq!(
            item.command,
            format!("npm run {}", devbox_core::sh_quote(&item.id))
        );
    }
    assert!(!root.path().join("marker").exists());
}

#[tokio::test]
async fn detect_service_filters_all_merged_definitions_beyond_first_page() {
    let f = crate::service_tests::Fixture::new().await;
    let mut config = String::new();
    for n in 0..270 {
        config.push_str(&format!("[[task]]\nid='a-{n:03}'\ncommand='true'\n"));
    }
    config.push_str("[[task]]\nid='z-existing'\ncommand='true'\n");
    f.write(&config);
    f.define("local", "true", SaveTo::ThisPc).await;
    let repo = f.root.path().join("repo");
    std::fs::write(
        repo.join("package.json"),
        r#"{"scripts":{"z-existing":"ignored","local":"ignored","new":"ignored"}}"#,
    )
    .expect("fixture result");
    let list = f
        .runs
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: None,
        })
        .await
        .expect("fixture result");
    assert!(list.next_cursor.is_some());
    let suggestions = f
        .runs
        .detect(DetectRuns {
            project_id: f.project.clone(),
        })
        .await
        .expect("fixture result");
    assert_eq!(
        suggestions
            .iter()
            .map(|s| s.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["new"])
    );
    assert!(matches!(
        f.runs
            .detect(DetectRuns {
                project_id: "missing".into()
            })
            .await,
        Err(Fail::Domain(RuntimeError::ProjectNotFound { .. }))
    ));
}

#[tokio::test]
async fn detect_script_builtin_names_colons_and_options_use_explicit_run() {
    let root = tempfile::tempdir().expect("fixture result");
    std::fs::write(root.path().join("package.json"), r#"{"scripts":{"install":"ignored","test":"ignored","build:web":"ignored","-custom":"ignored"}}"#).expect("fixture result");
    for (lock, manager) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
        ("bun.lock", "bun"),
    ] {
        std::fs::write(root.path().join(lock), "fixture").expect("fixture result");
        for item in detect(root.path(), &[]).await.expect("fixture result") {
            let expected = if item.id.starts_with('-') {
                format!("{manager} run -- {}", item.id)
            } else {
                format!("{manager} run {}", item.id)
            };
            assert_eq!(item.command, expected);
        }
        std::fs::remove_file(root.path().join(lock)).expect("fixture result");
    }
}

#[tokio::test]
async fn detect_input_validation_and_non_utf8_are_reported() {
    let root = tempfile::tempdir().expect("fixture result");
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"":"ignored"}}"#,
    )
    .expect("fixture result");
    assert!(matches!(
        detect(root.path(), &[]).await,
        Err(RuntimeError::DefInvalid { .. })
    ));
    std::fs::remove_file(root.path().join("package.json")).expect("fixture result");
    std::fs::write(root.path().join("Makefile"), [0xff]).expect("fixture result");
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectIo { file, .. }) if file == "Makefile")
    );
}

#[tokio::test]
async fn detect_file_and_response_budgets_are_typed() {
    let root = tempfile::tempdir().expect("fixture result");
    std::fs::write(
        root.path().join("package.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .expect("fixture result");
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectLimit { subject, .. }) if subject == "package.json")
    );
    let scripts: std::collections::BTreeMap<_, _> = (0..5000)
        .map(|n| (format!("script-{n:04}-{}", "a".repeat(60)), "ignored"))
        .collect();
    let json =
        serde_json::to_string(&serde_json::json!({ "scripts": scripts })).expect("fixture result");
    assert!(json.len() < 1024 * 1024);
    std::fs::write(root.path().join("package.json"), json).expect("fixture result");
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectLimit { subject, .. }) if subject == "suggestions")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn detect_non_regular_files_return_without_opening() {
    let root = tempfile::tempdir().expect("fixture result");
    let _socket = std::os::unix::net::UnixListener::bind(root.path().join("Makefile"))
        .expect("fixture result");
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectIo { file, .. }) if file == "Makefile")
    );
}
