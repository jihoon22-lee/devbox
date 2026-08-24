//! Knowledge inbound `Path` boundary.
//!
//! An applink path is untrusted input. Resolve it against the configured
//! Knowledge root, reject traversal/reparse escapes, and return a canonical
//! target plus a portable root-relative UI path.

use std::io::Read;
use std::path::{Path, PathBuf};

const OPEN_NOTE_ERROR: &str = "요청한 노트를 열 수 없습니다";
const MAX_INBOUND_PATH_BYTES: usize = 32 * 1024;
const MAX_INBOUND_NOTE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedInboundNote {
    pub relative_path: String,
    pub canonical_path: PathBuf,
}

fn has_unsafe_raw_segment(value: &str) -> bool {
    value
        .split(['/', '\\'])
        .any(|segment| matches!(segment, "." | ".."))
}

pub fn resolve_note(root: &Path, requested: &str) -> Result<ResolvedInboundNote, &'static str> {
    if requested.is_empty()
        || requested.len() > MAX_INBOUND_PATH_BYTES
        || requested.contains('\0')
        || has_unsafe_raw_segment(requested)
    {
        return Err(OPEN_NOTE_ERROR);
    }

    let canonical_root = root.canonicalize().map_err(|_| OPEN_NOTE_ERROR)?;
    let requested_path = Path::new(requested);
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        canonical_root.join(requested_path)
    };
    let canonical_path = candidate.canonicalize().map_err(|_| OPEN_NOTE_ERROR)?;

    if !canonical_path.starts_with(&canonical_root)
        || !canonical_path.is_file()
        || !canonical_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        return Err(OPEN_NOTE_ERROR);
    }

    let size = canonical_path
        .metadata()
        .map_err(|_| OPEN_NOTE_ERROR)?
        .len();
    if size > MAX_INBOUND_NOTE_BYTES {
        return Err(OPEN_NOTE_ERROR);
    }

    let relative = canonical_path
        .strip_prefix(&canonical_root)
        .map_err(|_| OPEN_NOTE_ERROR)?
        .to_str()
        .ok_or(OPEN_NOTE_ERROR)?
        .replace('\\', "/");
    if relative.is_empty() {
        return Err(OPEN_NOTE_ERROR);
    }

    Ok(ResolvedInboundNote {
        relative_path: relative,
        canonical_path,
    })
}

pub fn read_note(root: &Path, requested: &str) -> Result<(String, String), &'static str> {
    let resolved = resolve_note(root, requested)?;
    let file = std::fs::File::open(&resolved.canonical_path).map_err(|_| OPEN_NOTE_ERROR)?;
    let mut content = String::new();
    file.take(MAX_INBOUND_NOTE_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|_| OPEN_NOTE_ERROR)?;
    if content.len() as u64 > MAX_INBOUND_NOTE_BYTES {
        return Err(OPEN_NOTE_ERROR);
    }
    Ok((resolved.relative_path, content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Notes")).unwrap();
        fs::write(dir.path().join("Notes/inside.md"), "# inside").unwrap();
        fs::write(dir.path().join("Notes/not-markdown.txt"), "text").unwrap();
        dir
    }

    #[test]
    fn accepts_absolute_and_relative_markdown_paths_inside_root() {
        let root = fixture();
        let absolute = root.path().join("Notes/inside.md");

        let resolved = resolve_note(root.path(), absolute.to_str().unwrap()).unwrap();
        assert_eq!(resolved.relative_path, "Notes/inside.md");
        assert_eq!(resolved.canonical_path, absolute.canonicalize().unwrap());

        let resolved = resolve_note(root.path(), "Notes/inside.md").unwrap();
        assert_eq!(resolved.relative_path, "Notes/inside.md");

        assert_eq!(
            read_note(root.path(), "Notes/inside.md").unwrap(),
            ("Notes/inside.md".to_string(), "# inside".to_string())
        );
    }

    #[test]
    fn rejects_traversal_missing_directories_and_non_markdown_without_echoing_input() {
        let root = fixture();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let secret = "path-secret-must-not-appear";

        for requested in [
            "Notes/../inside.md".to_string(),
            "Notes/missing.md".to_string(),
            "Notes".to_string(),
            "Notes/not-markdown.txt".to_string(),
            outside.path().to_string_lossy().into_owned(),
            format!("Notes/{secret}.md"),
        ] {
            let error = resolve_note(root.path(), &requested).unwrap_err();
            assert_eq!(error, OPEN_NOTE_ERROR);
            assert!(!error.contains(secret));
            assert!(!error.contains(&requested));
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_outside_root() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let outside = tempfile::tempdir().unwrap();
        let outside_note = outside.path().join("outside.md");
        fs::write(&outside_note, "# outside").unwrap();
        symlink(&outside_note, root.path().join("Notes/link.md")).unwrap();

        assert_eq!(
            resolve_note(root.path(), "Notes/link.md"),
            Err(OPEN_NOTE_ERROR)
        );
    }

    #[test]
    fn rejects_notes_above_the_inbound_size_limit() {
        let root = fixture();
        let large = root.path().join("Notes/large.md");
        let file = fs::File::create(&large).unwrap();
        file.set_len(MAX_INBOUND_NOTE_BYTES + 1).unwrap();

        assert_eq!(
            resolve_note(root.path(), large.to_str().unwrap()),
            Err(OPEN_NOTE_ERROR)
        );
    }

    #[test]
    fn rejects_oversized_path_before_filesystem_resolution() {
        let root = fixture();
        let oversized = "x".repeat(MAX_INBOUND_PATH_BYTES + 1);

        assert_eq!(resolve_note(root.path(), &oversized), Err(OPEN_NOTE_ERROR));
    }
}
