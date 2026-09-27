use mcp_server::McpSettings;
use std::{
    fs,
    io::{self, Read},
    path::Path,
};
const FILE: &str = "mcp-settings.json";
pub fn load(dir: &Path) -> McpSettings {
    let read = || -> io::Result<McpSettings> {
        let path = dir.join(FILE);
        devbox_filesystem::ensure_no_links(&path)?;
        let mut bytes = Vec::new();
        let (file, identity) = devbox_filesystem::open_filesystem_object(&path, false)?;
        file.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 || devbox_filesystem::filesystem_identity(&path, false)? != identity {
            return Err(io::Error::other("mcp_settings_invalid"));
        }
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    };
    read().unwrap_or_default()
}
pub fn save(dir: &Path, settings: &McpSettings) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    devbox_filesystem::ensure_no_links(dir)?;
    devbox_filesystem::atomic_write(
        dir.join(FILE),
        &serde_json::to_vec(settings).map_err(io::Error::other)?,
    )
}

/// Enabling succeeds only after the owned launcher is ready. A damaged foreign
/// copy leaves the previous settings unchanged instead of advertising a broken grant.
pub fn save_ready(
    dir: &Path,
    installation: &Path,
    image: &Path,
    settings: &McpSettings,
    deadline: u64,
) -> io::Result<()> {
    if settings.enabled && !load(dir).enabled {
        super::launcher::refresh(installation, image)?;
    }
    workspace_core::current_deadline(deadline).map_err(io::Error::other)?;
    save(dir, settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enabling_requires_a_ready_owned_launcher_before_saving_the_grant() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data");
        let image = root.path().join("agent.exe");
        std::fs::write(&image, b"synthetic verified agent").unwrap();
        let enabled = mcp_server::McpSettings {
            enabled: true,
            ..Default::default()
        };
        save_ready(&dir, root.path(), &image, &enabled, u64::MAX).unwrap();
        assert!(super::super::launcher::stable_path(root.path()).exists());
        assert_eq!(load(&dir), enabled);
        save(&dir, &mcp_server::McpSettings::default()).unwrap();
        std::fs::write(
            super::super::launcher::stable_path(root.path()),
            b"foreign replacement",
        )
        .unwrap();
        assert!(save_ready(&dir, root.path(), &image, &enabled, u64::MAX).is_err());
        assert!(!load(&dir).enabled);
    }

    #[test]
    fn missing_or_corrupt_settings_disable_every_permission() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), mcp_server::McpSettings::default());
        let enabled = mcp_server::McpSettings {
            enabled: true,
            allow_note_capture: true,
            allow_task_run: false,
        };
        save(dir.path(), &enabled).unwrap();
        assert_eq!(load(dir.path()), enabled);
        std::fs::write(
            dir.path().join("mcp-settings.json"),
            br#"{"enabled":true,"allowNoteCapture":true,"allowTaskRun":true,"unknown":true}"#,
        )
        .unwrap();
        assert_eq!(load(dir.path()), mcp_server::McpSettings::default());
        std::fs::write(dir.path().join("mcp-settings.json"), b"{broken").unwrap();
        assert_eq!(load(dir.path()), mcp_server::McpSettings::default());
    }
}
