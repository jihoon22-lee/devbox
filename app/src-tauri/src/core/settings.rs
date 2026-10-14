use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path, sync::Mutex};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub distro: Option<String>,
    pub autostart_asked: bool,
    pub autostart: bool,
    pub tray_notice_shown: bool,
    pub global_shortcuts: GlobalShortcuts,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalShortcuts {
    pub palette: Option<String>,
    pub bottom_terminal: Option<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            distro: None,
            autostart_asked: false,
            autostart: true,
            tray_notice_shown: false,
            global_shortcuts: GlobalShortcuts::default(),
        }
    }
}
// Supervisor and UI preferences share one read-modify-write lock.
pub fn with_settings<T>(
    path: &Path,
    f: impl FnOnce(&mut Settings) -> std::io::Result<T>,
) -> std::io::Result<T> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    f(&mut load(path)?)
}
pub fn update(path: &Path, f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
    with_settings(path, |settings| {
        f(settings);
        save(path, settings)
    })
}
pub fn load(path: &Path) -> std::io::Result<Settings> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e),
    }
}
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("설정 경로 없음"))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut tmp, settings).map_err(std::io::Error::other)?;
    tmp.flush()?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_and_distro_updates_preserve_each_other_and_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        update(&path, |settings| {
            settings.autostart_asked = true;
            settings.autostart = false;
            settings.tray_notice_shown = true;
        })
        .unwrap();
        update(&path, |settings| settings.distro = Some("Ubuntu".into())).unwrap();
        let saved = load(&path).unwrap();
        assert!(saved.autostart_asked && saved.tray_notice_shown);
        assert!(!saved.autostart);
        assert_eq!(saved.distro.as_deref(), Some("Ubuntu"));
        std::fs::write(&path, b"malformed").unwrap();
        assert!(update(&path, |settings| settings.distro = None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"malformed");
    }
    #[test]
    fn atomic_save_reports_errors_and_preserves_previous_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save(
            &path,
            &Settings {
                distro: Some("Ubuntu".into()),
                ..Settings::default()
            },
        )
        .unwrap();
        assert_eq!(load(&path).unwrap().distro.as_deref(), Some("Ubuntu"));
        assert!(save(&path.join("impossible"), &Settings::default()).is_err());
        assert_eq!(load(&path).unwrap().distro.as_deref(), Some("Ubuntu"));
        std::fs::write(&path, b"invalid").unwrap();
        assert!(load(&path).is_err());
    }
}
