//! Only explicit listener intent is durable; shutdown preserves that intent.
use serde::{Deserialize, Serialize};
use std::path::Path;
pub const SETTINGS_ERROR: &str = "webhook_listener_settings_unavailable";
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListenerSettings {
    pub enabled: bool,
    pub port: u16,
    pub allow_lan: bool,
    #[serde(default = "default_bind")]
    pub bind: String,
}
fn default_bind() -> String {
    "127.0.0.1".into()
}
fn validate(settings: &ListenerSettings) -> Result<(), String> {
    if settings.port == 0
        || !matches!(
            settings.bind.as_str(),
            "127.0.0.1" | "::1" | "0.0.0.0" | "[::]"
        )
        || (matches!(settings.bind.as_str(), "0.0.0.0" | "[::]") && !settings.allow_lan)
    {
        return Err(SETTINGS_ERROR.into());
    }
    Ok(())
}
pub fn load(root: &Path) -> Result<Option<ListenerSettings>, String> {
    use std::io::Read;
    let path = root.join("listener.json");
    if let Err(error) = devbox_filesystem::ensure_no_links(&path) {
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(SETTINGS_ERROR.into())
        };
    }
    let (file, identity) =
        devbox_filesystem::open_filesystem_object(&path, false).map_err(|_| SETTINGS_ERROR)?;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| SETTINGS_ERROR)?;
    if bytes.len() > 4096 {
        return Err(SETTINGS_ERROR.into());
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| SETTINGS_ERROR)?;
    if devbox_filesystem::filesystem_identity(&path, false).map_err(|_| SETTINGS_ERROR)? != identity
    {
        return Err(SETTINGS_ERROR.into());
    }
    let settings = serde_json::from_slice(&bytes).map_err(|_| SETTINGS_ERROR)?;
    validate(&settings)?;
    Ok(Some(settings))
}
pub fn save(root: &Path, settings: &ListenerSettings) -> Result<(), String> {
    validate(settings)?;
    // Reject links in every existing ancestor before creating our component dir.
    let existing = root
        .ancestors()
        .find(|path| std::fs::symlink_metadata(path).is_ok())
        .ok_or(SETTINGS_ERROR)?;
    devbox_filesystem::ensure_no_links(existing).map_err(|_| SETTINGS_ERROR)?;
    std::fs::create_dir_all(root).map_err(|_| SETTINGS_ERROR)?;
    devbox_filesystem::ensure_no_links(root).map_err(|_| SETTINGS_ERROR)?;
    let path = root.join("listener.json");
    match devbox_filesystem::ensure_no_links(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(SETTINGS_ERROR.into()),
    }
    let bytes = serde_json::to_vec(settings).map_err(|_| SETTINGS_ERROR)?;
    devbox_filesystem::atomic_write(path, &bytes).map_err(|_| SETTINGS_ERROR.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listener_settings_round_trip_and_resume_only_when_enabled() {
        let root = tempfile::tempdir().unwrap();
        assert!(load(&root.path().join("missing")).unwrap().is_none());
        let mut settings = ListenerSettings {
            enabled: true,
            port: 8787,
            allow_lan: false,
            bind: "::1".into(),
        };
        save(root.path(), &settings).unwrap();
        assert_eq!(load(root.path()).unwrap(), Some(settings.clone()));
        settings.enabled = false;
        save(root.path(), &settings).unwrap();
        assert!(!load(root.path()).unwrap().unwrap().enabled);
    }
    #[test]
    fn corrupt_or_unconfirmed_lan_settings_are_not_repaired_or_started() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("listener.json");
        let bytes = br#"{"enabled":true,"port":8787,"allowLan":false,"bind":"0.0.0.0"}"#;
        std::fs::write(&path, bytes).unwrap();
        assert!(load(root.path()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, "{broken").unwrap();
        assert!(load(root.path()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    }
    #[test]
    #[cfg(unix)]
    fn settings_do_not_follow_links() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("source"), b"preserved").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("source"),
            root.path().join("listener.json"),
        )
        .unwrap();
        assert!(load(root.path()).is_err());
        assert!(save(
            root.path(),
            &ListenerSettings {
                enabled: false,
                port: 8787,
                allow_lan: false,
                bind: default_bind()
            }
        )
        .is_err());
        assert_eq!(
            std::fs::read(outside.path().join("source")).unwrap(),
            b"preserved"
        );
    }
}
