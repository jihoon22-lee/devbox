use super::*;

fn log(files: &BTreeMap<String, String>) -> &str {
    files
        .iter()
        .find(|(name, _)| name.starts_with("logs/"))
        .expect("log")
        .1
}

#[tokio::test]
async fn configured_path_forms_and_prefix_boundaries_survive_archive_passes() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let vault = paths.home.join("custom\"vault\nwith\\escapes\t\u{7}");
    std::fs::create_dir(&vault).expect("owned escaped vault");
    let raw = vault.to_str().expect("path");
    let json = serde_json::to_string(raw).expect("JSON");
    let debug = format!("{raw:?}");
    let mut data = input(&paths);
    data.notes_vault = VaultSnapshot::capture(&vault).expect("snapshot");
    data.doctor.checks[0].message = format!("vault={raw}/private.md");
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!("raw={raw}/a\njson={json}\ndebug={debug}\nprefix={raw}-sibling\n"),
    )
    .expect("log");
    let files = entries(&create_bundle(data, &secrets).await.expect("bundle").path);
    let text = log(&files);
    assert!(text.contains("raw=<notes>/a"));
    assert!(text.contains("json=\"<notes>\""));
    assert!(text.contains("debug=\"<notes>\""));
    assert!(text.contains("prefix=~/custom\"vault\nwith\\escapes\t\u{7}-sibling"));
    assert!(files["doctor.json"].contains("vault=<notes>/private.md"));
}

#[tokio::test]
async fn path_mask_wins_over_secret_components_and_remains_stable() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let vault = paths.home.join("custom-notes-vault");
    std::fs::create_dir(&vault).expect("vault");
    for (name, value) in [
        ("COMPONENT", "notes"),
        ("HOME", paths.home.to_str().expect("home")),
    ] {
        secrets
            .set(SetSecret {
                name: name.into(),
                value: value.into(),
            })
            .await
            .expect("secret");
    }
    let mut data = input(&paths);
    data.notes_vault = VaultSnapshot::capture(&vault).expect("snapshot");
    data.doctor.checks[0].message = format!("vault={}/private.md notes", vault.display());
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!("vault={}/private.md notes", vault.display()),
    )
    .expect("log");
    let files = entries(&create_bundle(data, &secrets).await.expect("bundle").path);
    assert_eq!(log(&files), "vault=<notes>/private.md [REDACTED]");
    assert!(files["doctor.json"].contains("vault=<notes>/private.md [REDACTED]"));
}

#[tokio::test]
async fn locked_bundle_masks_configured_doctor_paths_while_omitting_logs() {
    let (_temp, db, paths, _secrets) = fixture().await;
    let outside = tempfile::tempdir().expect("external vault");
    let mut data = input(&paths);
    data.notes_vault = VaultSnapshot::capture(outside.path()).expect("snapshot");
    data.doctor.checks[0].message = format!("vault={}/private.md", outside.path().display());
    std::fs::write(paths.log_dir.join("daemon.today.log"), "private log").expect("log");
    let locked = SecretsService::new(db, paths);
    let bundle = create_bundle(data, &locked).await.expect("bundle");
    assert_eq!(bundle.logs_omitted.as_deref(), Some("secrets_locked"));
    let files = entries(&bundle.path);
    assert!(!files.keys().any(|name| name.starts_with("logs/")));
    assert!(files["doctor.json"].contains("vault=<notes>/private.md"));
    assert!(
        files
            .values()
            .all(|text| !text.contains(outside.path().to_str().expect("path")))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn captured_alias_and_canonical_roots_are_masked_after_alias_retarget() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let outside = tempfile::tempdir().expect("owned external fixture");
    let first = outside.path().join("first");
    let second = outside.path().join("second");
    let alias = paths.home.join("alias");
    std::fs::create_dir(&first).expect("first vault");
    std::fs::create_dir(&second).expect("second vault");
    std::os::unix::fs::symlink(&first, &alias).expect("alias");
    let mut data = input(&paths);
    data.notes_vault = VaultSnapshot::capture(&alias).expect("snapshot");
    std::fs::remove_file(&alias).expect("owned alias");
    std::os::unix::fs::symlink(&second, &alias).expect("retarget alias");
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!(
            "alias={}/a canonical={}/b sibling={}-copy/c",
            alias.display(),
            first.display(),
            first.display()
        ),
    )
    .expect("log");
    let files = entries(&create_bundle(data, &secrets).await.expect("bundle").path);
    assert_eq!(
        log(&files),
        format!(
            "alias=<notes>/a canonical=<notes>/b sibling={}-copy/c",
            first.display()
        )
    );
}
