use devbox_core::{Db, Env, Instance, Paths};
use devbox_protocol::system::{Check, DoctorReport, Status};
use devbox_secrets::{SecretsService, SetSecret, crypto::Key};
use devbox_tools::support::{BundleInput, VaultSnapshot, create_bundle};
use std::{collections::BTreeMap, io::Read};
struct FixtureEnv(String);
impl Env for FixtureEnv {
    fn var(&self, key: &str) -> Option<String> {
        match key {
            "HOME" => Some(self.0.clone()),
            "XDG_RUNTIME_DIR" => Some(format!("{}/run", self.0)),
            _ => None,
        }
    }
    fn uid(&self) -> u32 {
        1000
    }
}
async fn fixture() -> (tempfile::TempDir, Db, Paths, SecretsService) {
    let temp = tempfile::tempdir().expect("owned fixture");
    let paths = Paths::resolve(
        Instance::parse("test-support").expect("instance"),
        &FixtureEnv(temp.path().display().to_string()),
    )
    .expect("paths");
    paths.ensure_dirs().expect("fixture directories");
    let db = Db::open(
        &paths.db_path,
        &paths.backups_dir,
        "fixture",
        devbox_secrets::MIGRATIONS,
    )
    .expect("owned DB");
    let secrets = SecretsService::with_key(db.clone(), paths.clone(), Key::from_bytes([23; 32]))
        .await
        .expect("fixture key");
    (temp, db, paths, secrets)
}
fn input(paths: &Paths) -> BundleInput {
    let vault = paths.home.join("notes");
    std::fs::create_dir_all(&vault).expect("explicit owned default vault");
    BundleInput {
        notes_vault: VaultSnapshot::capture(&vault).expect("vault snapshot"),
        paths: paths.clone(),
        doctor: DoctorReport {
            checks: vec![Check {
                id: "env".into(),
                status: Status::Ok,
                message: "fixture doctor".into(),
                fix: None,
            }],
        },
        version: "fixture-version".into(),
    }
}
fn entries(path: &str) -> BTreeMap<String, String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).expect("bundle")).expect("zip");
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).expect("entry");
            let mut text = String::new();
            entry.read_to_string(&mut text).expect("text entry");
            (entry.name().to_owned(), text)
        })
        .collect()
}
#[tokio::test]
async fn support_bundle_redacts_secret_values_when_unlocked() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let value = "fixture-sensitive-token-0123456789";
    secrets
        .set(SetSecret {
            name: "PRIVATE_TOKEN_NAME".into(),
            value: value.into(),
        })
        .await
        .expect("fixture secret");
    secrets
        .set(SetSecret {
            name: "EMPTY_VALUE".into(),
            value: "".into(),
        })
        .await
        .expect("empty fixture secret");
    secrets
        .set(SetSecret {
            name: "NAME_WITHOUT_VALUE".into(),
            value: "discarded-fixture".into(),
        })
        .await
        .expect("fixture secret");
    secrets
        .delete(devbox_secrets::Name {
            name: "NAME_WITHOUT_VALUE".into(),
        })
        .await
        .expect("leave name without value");
    let config = paths.config_dir.join("config.toml");
    let text = std::fs::read_to_string(&config).expect("names config");
    std::fs::write(
        &config,
        format!("{text}\n[projects]\nroots=[\"{value}\"]\n"),
    )
    .expect("fixture configuration");
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!("request token={value}\n"),
    )
    .expect("daemon log");
    std::fs::write(
        paths.log_dir.join("crash-1.log"),
        format!("crash token={value}\n"),
    )
    .expect("crash log");
    let mut data = input(&paths);
    data.doctor.checks[0].message = format!("check {value}");
    let bundle = create_bundle(data, &secrets).await.expect("bundle");
    assert!(bundle.logs_omitted.is_none());
    let files = entries(&bundle.path);
    assert!(files.values().all(|text| !text.contains(value)));
    assert!(
        files
            .iter()
            .any(|(name, text)| name.starts_with("logs/") && text.contains("[REDACTED]"))
    );
    let summary = &files["settings-summary.json"];
    for value in [
        value,
        "PRIVATE_TOKEN_NAME",
        "NAME_WITHOUT_VALUE",
        "EMPTY_VALUE",
    ] {
        assert!(!summary.contains(value));
    }
    assert!(files["doctor.json"].contains("[REDACTED]"));
    assert!(files["metadata.json"].contains("fixture-version"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&bundle.path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[tokio::test]
async fn support_bundle_omits_logs_when_secrets_are_locked() {
    let (_temp, db, paths, secrets) = fixture().await;
    secrets
        .set(SetSecret {
            name: "TOKEN".into(),
            value: "private-fixture".into(),
        })
        .await
        .expect("secret");
    std::fs::write(paths.log_dir.join("daemon.today.log"), "private-fixture").expect("log");
    std::fs::write(paths.log_dir.join("crash-1.log"), "private-fixture").expect("crash");
    let locked = SecretsService::new(db, paths.clone());
    let bundle = create_bundle(input(&paths), &locked)
        .await
        .expect("safe locked bundle");
    assert_eq!(bundle.logs_omitted.as_deref(), Some("secrets_locked"));
    let files = entries(&bundle.path);
    assert!(
        !files
            .keys()
            .any(|name| name.starts_with("logs/") || name.starts_with("crashes/"))
    );
    assert!(files["manifest.json"].contains("secrets_locked"));
    assert!(!files.values().any(|text| text.contains("private-fixture")));
}
#[tokio::test]
async fn support_bundle_scrubs_home_and_note_paths() {
    let (_temp, _db, paths, secrets) = fixture().await;
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!(
            "home={} note={}/notes/private.md",
            paths.home.display(),
            paths.home.display()
        ),
    )
    .expect("log");
    let bundle = create_bundle(input(&paths), &secrets)
        .await
        .expect("bundle");
    let files = entries(&bundle.path);
    assert!(
        files
            .values()
            .all(|text| !text.contains(paths.home.to_str().expect("home")))
    );
    assert!(
        files
            .values()
            .any(|text| text.contains("<notes>/private.md") && text.contains("home=~"))
    );
}
#[tokio::test]
async fn support_bundle_omits_unavailable_secrets_and_never_follows_log_symlinks() {
    let (_temp, _db, paths, secrets) = fixture().await;
    std::fs::write(
        paths.config_dir.join("config.toml"),
        "[secrets]\nnames='invalid'",
    )
    .expect("invalid fixture config");
    std::fs::write(paths.log_dir.join("daemon.today.log"), "private-fixture").expect("log");
    let bundle = create_bundle(input(&paths), &secrets)
        .await
        .expect("safe unavailable bundle");
    assert_eq!(bundle.logs_omitted.as_deref(), Some("secrets_locked"));
    #[cfg(unix)]
    {
        std::fs::write(paths.config_dir.join("config.toml"), "").expect("valid fixture config");
        let target = paths.home.join("private-outside.log");
        std::fs::write(&target, "outside-private").expect("private fixture");
        std::os::unix::fs::symlink(&target, paths.log_dir.join("crash-link.log"))
            .expect("owned symlink");
        let bundle = create_bundle(input(&paths), &secrets)
            .await
            .expect("bundle");
        assert!(
            !entries(&bundle.path)
                .values()
                .any(|text| text.contains("outside-private"))
        );
    }
}

#[tokio::test]
async fn support_bundle_keeps_the_seven_day_daemon_window_and_refuses_output_symlinks() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let old = paths.log_dir.join("daemon.old.log");
    std::fs::write(&old, "old fixture log").expect("old log");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&old)
        .expect("old file");
    file.set_times(std::fs::FileTimes::new().set_modified(
        std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 24 * 60 * 60),
    ))
    .expect("fixture timestamp");
    std::fs::write(
        paths.log_dir.join("daemon.current.log"),
        "current fixture log",
    )
    .expect("current log");
    let bundle = create_bundle(input(&paths), &secrets)
        .await
        .expect("bundle");
    let files = entries(&bundle.path);
    assert!(!files.values().any(|text| text.contains("old fixture log")));
    assert!(
        files
            .values()
            .any(|text| text.contains("current fixture log"))
    );
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().expect("owned outside fixture");
        std::fs::remove_file(bundle.path).expect("remove owned bundle");
        std::fs::remove_dir(paths.data_dir.join("support")).expect("empty support dir");
        std::os::unix::fs::symlink(outside.path(), paths.data_dir.join("support"))
            .expect("owned symlink");
        assert!(create_bundle(input(&paths), &secrets).await.is_err());
        assert_eq!(
            std::fs::read_dir(outside.path())
                .expect("outside unchanged")
                .count(),
            0
        );
    }
}
#[tokio::test]
async fn support_bundle_redacts_escaped_json_fields_before_serialization() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let value = "fixture\"secret\nline\\end";
    secrets
        .set(SetSecret {
            name: "ESCAPED".into(),
            value: value.into(),
        })
        .await
        .expect("fixture secret");
    let mut data = input(&paths);
    data.doctor.checks[0].message = format!("before {value} after");
    let bundle = create_bundle(data, &secrets).await.expect("bundle");
    let files = entries(&bundle.path);
    let report: serde_json::Value =
        serde_json::from_str(&files["doctor.json"]).expect("valid JSON");
    assert_eq!(report["checks"][0]["message"], "before [REDACTED] after");
}

#[tokio::test]
async fn support_bundle_omits_oversized_logs_whole_without_exporting_secret_prefixes() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let value = "fixture-sensitive-boundary-value";
    secrets
        .set(SetSecret {
            name: "TOKEN".into(),
            value: value.into(),
        })
        .await
        .expect("secret");
    let mut log = "x".repeat(4 * 1024 * 1024 - 5);
    log.push_str(value);
    std::fs::write(paths.log_dir.join("daemon.large.log"), log).expect("oversized owned log");
    let bundle = create_bundle(input(&paths), &secrets)
        .await
        .expect("bounded bundle");
    let files = entries(&bundle.path);
    assert!(!files.keys().any(|name| name.starts_with("logs/")));
    let manifest: serde_json::Value =
        serde_json::from_str(&files["manifest.json"]).expect("manifest");
    assert_eq!(manifest["omittedFiles"], 1);
}

#[tokio::test]
async fn review_support_bundle_does_not_export_json_escaped_secret_values() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let value = "fixture\"secret\nline\\end";
    secrets
        .set(SetSecret {
            name: "ESCAPED_LOG".into(),
            value: value.into(),
        })
        .await
        .expect("fixture secret");
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        serde_json::json!({"token":value}).to_string(),
    )
    .expect("fixture log");
    let bundle = create_bundle(input(&paths), &secrets)
        .await
        .expect("bundle");
    let files = entries(&bundle.path);
    let log = files
        .iter()
        .find(|(name, _)| name.starts_with("logs/"))
        .expect("bundled log")
        .1;
    let decoded: serde_json::Value = serde_json::from_str(log).expect("bundled JSON");
    assert_ne!(
        decoded["token"], value,
        "archive exposes the entire secret after normal JSON decoding"
    );
}
#[tokio::test]
async fn support_bundle_masks_known_raw_json_and_debug_representations() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let value = "fixture\"secret\nline\\end\t\u{7}";
    secrets
        .set(SetSecret {
            name: "KNOWN_ESCAPES".into(),
            value: value.into(),
        })
        .await
        .expect("secret");
    let json = serde_json::to_string(value).expect("JSON");
    let debug = format!("{value:?}");
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!("raw={value}\njson={json}\ndebug={debug}\n"),
    )
    .expect("log");
    let bundle = create_bundle(input(&paths), &secrets)
        .await
        .expect("bundle");
    let files = entries(&bundle.path);
    let log = files
        .iter()
        .find(|(name, _)| name.starts_with("logs/"))
        .expect("log")
        .1;
    for representation in [value, &json[1..json.len() - 1], &debug[1..debug.len() - 1]] {
        assert!(!log.contains(representation));
    }
    assert_eq!(log.matches("[REDACTED]").count(), 3);
}

#[tokio::test]
async fn support_bundle_masks_the_configured_vault_outside_home() {
    let (_temp, _db, paths, secrets) = fixture().await;
    let outside = tempfile::tempdir().expect("owned external vault");
    let vault = outside.path().join("private-vault");
    std::fs::create_dir(&vault).expect("owned vault");
    let configured = serde_json::to_string(vault.to_str().expect("path")).expect("quoted path");
    std::fs::write(
        paths.config_dir.join("config.local.toml"),
        format!("[notes]\nvault={configured}\n"),
    )
    .expect("configuration");
    std::fs::write(
        paths.log_dir.join("daemon.today.log"),
        format!("note={}/private.md", vault.display()),
    )
    .expect("owned log");
    let mut data = input(&paths);
    data.notes_vault = VaultSnapshot::capture(&vault).expect("configured vault snapshot");
    let bundle = create_bundle(data, &secrets).await.expect("bundle");
    let files = entries(&bundle.path);
    let log = files
        .iter()
        .find(|(name, _)| name.starts_with("logs/"))
        .expect("log")
        .1;
    assert_eq!(
        log, "note=<notes>/private.md",
        "configured external vault must not be disclosed"
    );
}

#[path = "support_bundle/paths.rs"]
mod path_tests;
