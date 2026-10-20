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
        let items = detect(&fixture(manager), &[]).await.unwrap();
        assert_eq!(
            items.iter().find(|s| s.id == "build").unwrap().command,
            command
        );
        assert!(
            items
                .iter()
                .all(|s| s.from == SuggestionSource::PackageJson)
        );
    }
    let items = detect(&fixture("pnpm"), &[]).await.unwrap();
    for id in ["dev", "start", "serve"] {
        assert_eq!(
            items.iter().find(|s| s.id == id).unwrap().kind,
            DefinitionKind::Service
        );
    }
    assert_eq!(
        items.iter().find(|s| s.id == "dev").unwrap().command,
        "pnpm dev"
    );
    assert_eq!(
        items.iter().find(|s| s.id == "build").unwrap().kind,
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
        let items = detect(&fixture(name), &[]).await.unwrap();
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
            serde_json::to_string(&items[0].from).unwrap(),
            format!("\"{source}\"")
        );
    }
}

#[tokio::test]
async fn detect_lock_priority_fallback_and_bun_binary_lock() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"build":"echo build"}}"#,
    )
    .unwrap();
    let locks = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
        ("npm-shrinkwrap.json", "npm"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
    ];
    for (file, _) in locks {
        std::fs::write(root.path().join(file), b"fixture").unwrap();
    }
    for (file, manager) in locks {
        assert_eq!(
            detect(root.path(), &[]).await.unwrap()[0].command,
            format!("{manager} run build")
        );
        std::fs::remove_file(root.path().join(file)).unwrap();
    }
    assert_eq!(
        detect(root.path(), &[]).await.unwrap()[0].command,
        "npm run build"
    );
}

#[tokio::test]
async fn detect_duplicates_are_deterministic_and_existing_ids_stay_excluded() {
    let root = tempfile::tempdir().unwrap();
    for (name, text) in [
        ("package.json", r#"{"scripts":{"build":"echo package"}}"#),
        ("Makefile", "build:\n\techo make\n"),
        ("justfile", "build:\n    echo just\n"),
        ("Cargo.toml", ""),
        ("compose.yaml", ""),
        ("docker-compose.yml", ""),
    ] {
        std::fs::write(root.path().join(name), text).unwrap();
    }
    let items = detect(root.path(), &[]).await.unwrap();
    assert_eq!(items.iter().filter(|s| s.id == "build").count(), 1);
    assert_eq!(
        items.iter().find(|s| s.id == "build").unwrap().from,
        SuggestionSource::PackageJson
    );
    assert_eq!(
        items.iter().find(|s| s.id == "compose-up").unwrap().from,
        SuggestionSource::ComposeYaml
    );
    assert!(
        detect(root.path(), &["build".into(), "compose-up".into()])
            .await
            .unwrap()
            .iter()
            .all(|s| s.id == "test")
    );
    assert_eq!(items, detect(root.path(), &[]).await.unwrap());
}

#[tokio::test]
async fn detect_marker_files_are_read_without_execution() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("detect-spawn-marker");
    for (name, file) in [("make", "Makefile"), ("just", "justfile")] {
        let content = std::fs::read_to_string(fixture(name).join(file))
            .unwrap()
            .replace("detect-spawn-marker", marker.to_str().unwrap());
        std::fs::write(root.path().join(file), content).unwrap();
    }
    assert!(!detect(root.path(), &[]).await.unwrap().is_empty());
    assert!(
        !marker.exists(),
        "detection must never evaluate make shell or just backticks/recipes"
    );
}

#[tokio::test]
async fn detect_parse_and_io_failures_are_typed_not_empty_success() {
    let root = tempfile::tempdir().unwrap();
    for content in ["{", r#"{"scripts":{"build":17}}"#, r#"{"scripts":[]}"#] {
        std::fs::write(root.path().join("package.json"), content).unwrap();
        assert!(matches!(
            detect(root.path(), &[]).await,
            Err(RuntimeError::DetectParse { .. })
        ));
    }
    std::fs::remove_file(root.path().join("package.json")).unwrap();
    std::fs::create_dir(root.path().join("package.json")).unwrap();
    assert!(matches!(
        detect(root.path(), &[]).await,
        Err(RuntimeError::DetectIo { .. })
    ));
}

#[tokio::test]
async fn detect_script_names_preserve_literal_shell_arguments() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"a'; touch marker; echo '":"ignored","with space":"ignored"}}"#,
    )
    .unwrap();
    let items = detect(root.path(), &[]).await.unwrap();
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
    .unwrap();
    let list = f
        .runs
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: None,
        })
        .await
        .unwrap();
    assert!(list.next_cursor.is_some());
    let suggestions = f
        .runs
        .detect(DetectRuns {
            project_id: f.project.clone(),
        })
        .await
        .unwrap();
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
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("package.json"), r#"{"scripts":{"install":"ignored","test":"ignored","build:web":"ignored","-custom":"ignored"}}"#).unwrap();
    for (lock, manager) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
        ("bun.lock", "bun"),
    ] {
        std::fs::write(root.path().join(lock), "fixture").unwrap();
        for item in detect(root.path(), &[]).await.unwrap() {
            let expected = if item.id.starts_with('-') {
                format!("{manager} run -- {}", item.id)
            } else {
                format!("{manager} run {}", item.id)
            };
            assert_eq!(item.command, expected);
        }
        std::fs::remove_file(root.path().join(lock)).unwrap();
    }
}

#[tokio::test]
async fn detect_input_validation_and_non_utf8_are_reported() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"":"ignored"}}"#,
    )
    .unwrap();
    assert!(matches!(
        detect(root.path(), &[]).await,
        Err(RuntimeError::DefInvalid { .. })
    ));
    std::fs::remove_file(root.path().join("package.json")).unwrap();
    std::fs::write(root.path().join("Makefile"), [0xff]).unwrap();
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectIo { file, .. }) if file == "Makefile")
    );
}

#[tokio::test]
async fn detect_file_and_response_budgets_are_typed() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectLimit { subject, .. }) if subject == "package.json")
    );
    let scripts: std::collections::BTreeMap<_, _> = (0..5000)
        .map(|n| (format!("script-{n:04}-{}", "a".repeat(60)), "ignored"))
        .collect();
    let json = serde_json::to_string(&serde_json::json!({ "scripts": scripts })).unwrap();
    assert!(json.len() < 1024 * 1024);
    std::fs::write(root.path().join("package.json"), json).unwrap();
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectLimit { subject, .. }) if subject == "suggestions")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn detect_non_regular_files_return_without_opening() {
    let root = tempfile::tempdir().unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(root.path().join("Makefile")).unwrap();
    assert!(
        matches!(detect(root.path(), &[]).await, Err(RuntimeError::DetectIo { file, .. }) if file == "Makefile")
    );
}
