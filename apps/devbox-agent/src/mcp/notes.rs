use super::host::host_error;
use knowledge_vault_engine::component::ProductVault;
use mcp_server::HostError;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
const MAX_NOTE: usize = 256 * 1024;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteText {
    pub path: String,
    pub text: String,
    pub truncated: bool,
}
fn relative(value: &str) -> Result<(), HostError> {
    if value.is_empty()
        || value.chars().count() > 1024
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':'))
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
        || !Path::new(value)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return Err(host_error("note_path_invalid"));
    }
    Ok(())
}
pub fn read(root: &Path, relative_path: &str) -> Result<NoteText, HostError> {
    relative(relative_path)?;
    let vault = ProductVault::inspect(root).map_err(|_| host_error("vault_unavailable"))?;
    let path = vault
        .entry(relative_path)
        .map_err(|_| host_error("note_path_invalid"))?;
    let parts: Vec<_> = relative_path.split('/').collect();
    let mut parents = vec![vault];
    let mut directory = parents[0].path().to_owned();
    for part in &parts[..parts.len() - 1] {
        directory.push(part);
        parents
            .push(ProductVault::inspect(&directory).map_err(|_| host_error("note_path_invalid"))?);
    }
    let (file, identity) =
        devbox_filesystem::open_filesystem_object(&path, false).map_err(|error| {
            host_error(if error.kind() == std::io::ErrorKind::NotFound {
                "note_missing"
            } else {
                "note_path_invalid"
            })
        })?;
    let mut bytes = Vec::new();
    file.take(MAX_NOTE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| host_error("note_unavailable"))?;
    let truncated = bytes.len() > MAX_NOTE;
    bytes.truncate(MAX_NOTE);
    let valid = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if truncated && error.error_len().is_none() => error.valid_up_to(),
        Err(_) => return Err(host_error("note_unavailable")),
    };
    bytes.truncate(valid);
    for parent in &parents {
        parent
            .revalidate()
            .map_err(|_| host_error("note_path_invalid"))?;
    }
    parents[0]
        .entry(relative_path)
        .map_err(|_| host_error("note_path_invalid"))?;
    if devbox_filesystem::filesystem_identity(&path, false)
        .map_err(|_| host_error("note_path_invalid"))?
        != identity
    {
        return Err(host_error("note_path_invalid"));
    }
    Ok(NoteText {
        path: relative_path.into(),
        text: String::from_utf8(bytes).map_err(|_| host_error("note_unavailable"))?,
        truncated,
    })
}
fn timestamp(now_ms: u64) -> String {
    let day = product_contract::operation_log::day_file_name(now_ms);
    let day = day
        .trim_start_matches("operations-")
        .trim_end_matches(".jsonl");
    let minutes = (now_ms / 60_000) % (24 * 60);
    format!("{day} {:02}{:02}", minutes / 60, minutes % 60)
}
pub fn capture(root: &Path, title: &str, body: &str, now_ms: u64) -> Result<String, HostError> {
    if title.trim().is_empty() || title.chars().count() > 120 || body.len() > 65536 {
        return Err(host_error("note_path_invalid"));
    }
    let vault = ProductVault::inspect(root).map_err(|_| host_error("vault_unavailable"))?;
    let inbox = vault
        .entry("Inbox")
        .map_err(|_| host_error("note_path_invalid"))?;
    match fs::create_dir(&inbox) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(host_error("note_unavailable")),
    }
    let inbox = ProductVault::inspect(&inbox).map_err(|_| host_error("note_path_invalid"))?;
    let filename: String = format!("{} {title}", timestamp(now_ms))
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let filename: String = filename.trim_matches([' ', '.']).chars().take(80).collect();
    let filename = filename.trim_end_matches([' ', '.']);
    let content = format!("# {title}\n\n{body}\n");
    for index in 1..=99 {
        let name = if index == 1 {
            format!("{filename}.md")
        } else {
            format!("{filename} ({index}).md")
        };
        let path = inbox
            .entry(&name)
            .map_err(|_| host_error("note_path_invalid"))?;
        vault
            .revalidate()
            .map_err(|_| host_error("note_path_invalid"))?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(host_error("note_unavailable")),
        };
        inbox
            .revalidate()
            .map_err(|_| host_error("note_path_invalid"))?;
        file.write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| host_error("note_unavailable"))?;
        vault
            .revalidate()
            .map_err(|_| host_error("note_path_invalid"))?;
        inbox
            .revalidate()
            .map_err(|_| host_error("note_path_invalid"))?;
        return Ok(format!("Inbox/{name}"));
    }
    Err(host_error("note_capture_limit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_limits_and_inbox_links_preserve_existing_data() {
        let vault = tempfile::tempdir().unwrap();
        assert!(capture(vault.path(), "x", &"x".repeat(65537), 0).is_err());
        assert!(!vault.path().join("Inbox").exists());
        for path in [
            "file.md:stream",
            "C:/file.md",
            "a:b.md",
            "a\\b.md",
            "../x.md",
        ] {
            assert!(read(vault.path(), path).is_err());
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), vault.path().join("Inbox")).unwrap();
            assert!(capture(vault.path(), "safe", "body", 0).is_err());
            assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        }
    }

    #[test]
    fn reading_is_limited_to_markdown_inside_the_vault() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir(vault.path().join("Projects")).unwrap();
        std::fs::write(vault.path().join("Projects/devbox.md"), "# Devbox\n").unwrap();
        std::fs::write(vault.path().join("secret.txt"), "x").unwrap();
        assert_eq!(
            read(vault.path(), "Projects/devbox.md").unwrap().text,
            "# Devbox\n"
        );
        for bad in [
            "../outside.md",
            "/etc/passwd",
            "secret.txt",
            "Projects/../../x.md",
            "",
        ] {
            assert!(read(vault.path(), bad).is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", vault.path().join("link.md")).unwrap();
            assert!(read(vault.path(), "link.md").is_err());
        }
    }

    #[test]
    fn large_notes_are_truncated_at_a_char_boundary() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("big.md"), "가".repeat(100_000)).unwrap();
        let note = read(vault.path(), "big.md").unwrap();
        assert!(note.truncated && note.text.len() <= 256 * 1024);
    }

    #[test]
    fn capture_creates_new_inbox_files_and_never_overwrites() {
        let vault = tempfile::tempdir().unwrap();
        let at = 1_790_812_800_000; // 2026-10-01T00:00:00Z
        let first = capture(vault.path(), "Build: failing?", "body", at).unwrap();
        assert_eq!(first, "Inbox/2026-10-01 0000 Build- failing-.md");
        let second = capture(vault.path(), "Build: failing?", "other", at).unwrap();
        assert_eq!(second, "Inbox/2026-10-01 0000 Build- failing- (2).md");
        assert_eq!(
            std::fs::read_to_string(vault.path().join(&first)).unwrap(),
            "# Build: failing?\n\nbody\n"
        );
    }
}
