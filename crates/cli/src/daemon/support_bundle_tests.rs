//! Notes configuration reaches support capture without running host doctor probes.
use super::{state::Services, support_bundle::vault_snapshot};
use devbox_core::{Env, Instance, Paths};
use std::{path::Path, sync::Arc};

fn services(home: &Path, vault: &Path) -> Arc<Services> {
    let value = serde_json::to_string(vault.to_str().expect("fixture vault"))
        .expect("quote fixture path for TOML");
    services_with(home, &format!("[notes]\nvault = {value}\n"))
}

fn services_with(home: &Path, config: &str) -> Arc<Services> {
    struct FixtureEnv(String);
    impl Env for FixtureEnv {
        fn var(&self, key: &str) -> Option<String> {
            (key == "HOME").then(|| self.0.clone())
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let paths = Paths::resolve(
        Instance::parse("test-support").expect("fixture instance"),
        &FixtureEnv(home.to_str().expect("fixture home").into()),
    )
    .expect("fixture paths");
    std::fs::create_dir_all(&paths.config_dir).expect("fixture config directory");
    std::fs::write(paths.config_dir.join("config.toml"), config).expect("fixture Notes setting");
    super::build_services(paths).expect("fixture services")
}

#[tokio::test]
async fn configured_custom_vault_is_captured_instead_of_default_notes() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let home = fixture.path().join("home");
    let vault = fixture.path().join("custom-vault");
    std::fs::create_dir_all(home.join("notes")).expect("unused default vault");
    std::fs::create_dir(&vault).expect("configured vault");
    let services = services(&home, &vault);
    let captured = vault_snapshot(&services)
        .await
        .expect("capture")
        .expect("notes are on");
    assert_eq!(captured.configured, vault);
    assert_eq!(captured.canonical, vault.canonicalize().expect("canonical"));
    assert_ne!(captured.canonical, home.join("notes"));
}

#[tokio::test]
async fn alias_retarget_does_not_change_an_already_captured_vault() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let first = fixture.path().join("first");
    let second = fixture.path().join("second");
    let alias = fixture.path().join("configured-alias");
    std::fs::create_dir(&first).expect("first vault");
    std::fs::create_dir(&second).expect("second vault");
    std::os::unix::fs::symlink(&first, &alias).expect("owned alias");
    let services = services(&fixture.path().join("home"), &alias);
    let captured = vault_snapshot(&services)
        .await
        .expect("capture")
        .expect("notes are on");
    std::fs::remove_file(&alias).expect("remove owned alias");
    std::os::unix::fs::symlink(&second, &alias).expect("retarget owned alias");
    let later = vault_snapshot(&services)
        .await
        .expect("later capture")
        .expect("notes are on");
    assert_eq!(captured.configured, alias);
    assert_eq!(captured.canonical, first.canonicalize().expect("first"));
    assert_eq!(later.configured, captured.configured);
    assert_eq!(later.canonical, second.canonicalize().expect("second"));
}

#[tokio::test]
async fn unavailable_configured_vault_fails_without_a_default_fallback() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let home = fixture.path().join("home");
    let vault = fixture.path().join("removed-vault");
    std::fs::create_dir_all(home.join("notes")).expect("unused default vault");
    std::fs::create_dir(&vault).expect("configured vault");
    let services = services(&home, &vault);
    std::fs::remove_dir(&vault).expect("remove owned vault");
    assert!(matches!(
        vault_snapshot(&services).await,
        Err(devbox_protocol::Fail::Domain(
            devbox_tools::ToolsError::SupportFailed
        ))
    ));
}

/// Notes are off (`[notes]` did not load) and the default vault does not exist: the
/// bundle is still made, without a vault and without logs, rather than failing on it.
#[tokio::test]
async fn notes_off_bundles_without_a_vault_instead_of_failing_on_the_default() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let home = fixture.path().join("home");
    let services = services_with(&home, "[notes]\nvault = 'relative'\n");
    let notes_vault = vault_snapshot(&services)
        .await
        .expect("no capture while notes are off");
    assert_eq!(notes_vault, None);
    std::fs::write(
        services.paths.log_dir.join("daemon.today.log"),
        "notes-off-log\n",
    )
    .expect("owned log");
    let bundle = devbox_tools::support::create_bundle(
        devbox_tools::support::BundleInput {
            paths: services.paths.clone(),
            notes_vault,
            doctor: devbox_protocol::system::DoctorReport { checks: Vec::new() },
            version: "fixture".into(),
        },
        &services.secrets,
    )
    .await
    .expect("support bundle while notes are off");
    assert_eq!(bundle.logs_omitted.as_deref(), Some("notes_disabled"));
    assert!(Path::new(&bundle.path).is_file());
    assert!(!home.join("notes").exists());
}
