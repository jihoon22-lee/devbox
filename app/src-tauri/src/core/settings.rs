use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};
#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub distro: Option<String>,
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
    fn atomic_save_reports_errors_and_preserves_previous_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save(
            &path,
            &Settings {
                distro: Some("Ubuntu".into()),
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
