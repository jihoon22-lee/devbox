//! Notes configuration reaches support capture without running host doctor probes.
use super::{state::Services, support_bundle::vault_snapshot};
use devbox_core::{Env, Instance, Paths};
use std::{path::Path, sync::Arc};

fn services(home: &Path, vault: &Path) -> Arc<Services> {
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
    let value = serde_json::to_string(vault.to_str().expect("fixture vault"))
        .expect("quote fixture path for TOML");
    std::fs::write(
        paths.config_dir.join("config.toml"),
        format!("[notes]\nvault = {value}\n"),
    )
    .expect("fixture Notes setting");
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
    let captured = vault_snapshot(&services.notes).await.expect("capture");
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
    let captured = vault_snapshot(&services.notes).await.expect("capture");
    std::fs::remove_file(&alias).expect("remove owned alias");
    std::os::unix::fs::symlink(&second, &alias).expect("retarget owned alias");
    let later = vault_snapshot(&services.notes)
        .await
        .expect("later capture");
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
        vault_snapshot(&services.notes).await,
        Err(devbox_protocol::Fail::Domain(
            devbox_tools::ToolsError::SupportFailed
        ))
    ));
}
