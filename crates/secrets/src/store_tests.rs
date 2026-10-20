use crate::*;
use devbox_core::{Db, Env, Instance, Paths};
struct Home(String);
impl Env for Home {
    fn var(&self, k: &str) -> Option<String> {
        (k == "HOME").then(|| self.0.clone())
    }
    fn uid(&self) -> u32 {
        1000
    }
}
pub(crate) fn fixture() -> (tempfile::TempDir, Db, Paths) {
    let t = tempfile::tempdir().expect("owned fixture");
    let p = Paths::resolve(
        Instance::parse("test-secrets").expect("instance"),
        &Home(t.path().display().to_string()),
    )
    .expect("paths");
    p.ensure_dirs().expect("dirs");
    let db = Db::open(&p.db_path, &p.backups_dir, "test", MIGRATIONS).expect("db");
    (t, db, p)
}
#[tokio::test]
async fn key_mismatch_is_reported_not_silently_reset() {
    let (_t, db, p) = fixture();
    let service = SecretsService::with_key(db, p, crypto::Key::from_bytes([8; 32]))
        .await
        .expect("key");
    assert!(matches!(
        service
            .unlock(devbox_protocol::secrets::Unlock { key: vec![9; 32] })
            .await,
        Err(devbox_protocol::Fail::Domain(SecretsError::KeyMismatch))
    ));
    assert_eq!(
        service.export_key().await.expect("original").recovery,
        crypto::Key::from_bytes([8; 32]).recovery().as_str()
    );
}
#[test]
fn recovery_string_restores_the_same_key_id() {
    let key = crypto::Key::random();
    let text = key.recovery();
    assert!(text.split('-').all(|s| s.len() <= 4));
    assert_eq!(crypto::Key::recover(&text).expect("restore").id(), key.id());
    assert!(crypto::Key::recover("invalid").is_err());
}
#[tokio::test]
async fn crud_list_has_names_and_presence_but_no_values() {
    let (_t, db, p) = fixture();
    let service = SecretsService::with_key(db, p.clone(), crypto::Key::from_bytes([3; 32]))
        .await
        .expect("key");
    service
        .set(SetSecret {
            name: "TOKEN".into(),
            value: "owned-token".into(),
        })
        .await
        .expect("set");
    let json = serde_json::to_string(&service.list().await.expect("list")).expect("json");
    assert!(!json.contains("owned-token"));
    assert!(json.contains("TOKEN"));
    assert!(
        !std::fs::read_to_string(p.config_dir.join("config.toml"))
            .expect("shared names")
            .contains("owned-token")
    );
    assert_eq!(
        service.resolve(&["TOKEN".into()]).await.expect("resolve")["TOKEN"].as_slice(),
        b"owned-token"
    );
    service
        .delete(Name {
            name: "TOKEN".into(),
        })
        .await
        .expect("delete");
    assert!(!service.list().await.expect("name remains")[0].has_value);
    assert!(matches!(
        service.resolve(&["TOKEN".into()]).await,
        Err(devbox_protocol::Fail::Domain(SecretsError::Missing { .. }))
    ));
}

#[tokio::test]
async fn invalid_secret_config_section_returns_error_and_preserves_file() {
    for text in [
        "secrets = 'oops'\n",
        "secrets = []\n",
        "[secrets]\nnames='oops'\n",
        "[secrets]\nnames=[1]\n",
    ] {
        let (_t, db, p) = fixture();
        let service = SecretsService::with_key(db, p.clone(), crypto::Key::from_bytes([3; 32]))
            .await
            .expect("key");
        let path = p.config_dir.join("config.toml");
        std::fs::write(&path, text).expect("owned invalid config");
        assert!(matches!(
            service
                .set(SetSecret {
                    name: "TOKEN".into(),
                    value: "owned-token".into(),
                })
                .await,
            Err(devbox_protocol::Fail::Domain(SecretsError::InvalidInput))
        ));
        assert_eq!(
            std::fs::read_to_string(path).expect("config preserved"),
            text
        );
        assert!(matches!(
            service.resolve(&["TOKEN".into()]).await,
            Err(devbox_protocol::Fail::Domain(SecretsError::Missing { .. }))
        ));
    }
}

#[tokio::test]
async fn secret_names_preserve_valid_inline_tables() {
    for text in ["secrets = {}\n", "secrets = { names = ['EXISTING'] }\n"] {
        let (_t, db, paths) = fixture();
        let path = paths.config_dir.join("config.toml");
        std::fs::write(&path, text).expect("inline shared config");
        let service = SecretsService::with_key(db, paths.clone(), crypto::Key::from_bytes([3; 32]))
            .await
            .expect("fixture key");
        service
            .set(SetSecret {
                name: "TOKEN".into(),
                value: "owned-token".into(),
            })
            .await
            .expect("set with inline table");
        let config = devbox_core::Config::load(&paths).expect("inline table remains valid");
        assert!(config.secrets.names.contains(&"TOKEN".into()));
        if text.contains("EXISTING") {
            assert!(config.secrets.names.contains(&"EXISTING".into()));
        }
        assert!(
            !std::fs::read_to_string(path)
                .expect("shared config")
                .contains("owned-token")
        );
        assert_eq!(
            service
                .resolve(&["TOKEN".into()])
                .await
                .expect("stored secret")["TOKEN"]
                .as_slice(),
            b"owned-token"
        );
    }
}
