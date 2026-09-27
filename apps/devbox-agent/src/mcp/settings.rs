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

#[cfg(test)]
mod tests {
    use super::*;
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
