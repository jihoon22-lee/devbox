use crate::*;
use std::{io::Read, path::Path};

pub const MAX_FILES: u32 = 50;
pub const MAX_BYTES: u32 = 64 * 1024;

fn limit() -> Fail<ProjectsError> {
    ProjectsError::PromptLimitExceeded {
        max_files: MAX_FILES,
        max_bytes: MAX_BYTES,
    }
    .into()
}

pub fn read(project: &Path) -> R<PromptList> {
    let directory = project.join(".devbox/prompts");
    safe(&directory)?;
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PromptList { items: Vec::new() });
        }
        Err(error) => {
            return Err(Err::<(), _>(error)
                .internal("list project prompts")
                .expect_err("known error"));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.internal("list project prompt file")?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }
        safe(&path)?;
        let metadata = std::fs::symlink_metadata(&path).internal("inspect project prompt")?;
        if !metadata.is_file() {
            return Err(ProjectsError::UnsafePromptPath {
                path: path.display().to_string(),
            }
            .into());
        }
        paths.push(path);
        if paths.len() > MAX_FILES as usize {
            return Err(limit());
        }
    }
    paths.sort();
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        safe(&path)?;
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ProjectsError::PromptInvalidEncoding {
                name: path.display().to_string(),
            })?
            .to_owned();
        let input = config::open_regular(&path).internal("open project prompt")?;
        if input
            .metadata()
            .internal("inspect project prompt size")?
            .len()
            > u64::from(MAX_BYTES)
        {
            return Err(limit());
        }
        let mut bytes = Vec::new();
        input
            .take(u64::from(MAX_BYTES) + 1)
            .read_to_end(&mut bytes)
            .internal("read project prompt")?;
        if bytes.len() > MAX_BYTES as usize {
            return Err(limit());
        }
        let body = String::from_utf8(bytes)
            .map_err(|_| ProjectsError::PromptInvalidEncoding { name: name.clone() })?;
        items.push(ProjectPrompt { name, body });
    }
    Ok(PromptList { items })
}

fn safe(path: &Path) -> R<()> {
    config::check_ancestors(path).map_err(|_| {
        ProjectsError::UnsafePromptPath {
            path: path.display().to_string(),
        }
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_prompts_obey_utf8_file_and_byte_limits() {
        let root = tempfile::tempdir().expect("fixture");
        assert!(read(root.path()).expect("missing").items.is_empty());
        let dir = root.path().join(".devbox/prompts");
        std::fs::create_dir_all(&dir).expect("fixture");
        std::fs::write(dir.join("z.md"), "last").expect("fixture");
        std::fs::write(dir.join("a.md"), "{{title}}").expect("fixture");
        std::fs::write(dir.join("ignored.txt"), "ignored").expect("fixture");
        let list = read(root.path()).expect("prompts");
        assert_eq!(
            list.items
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(list.items[0].body, "{{title}}");
        std::fs::write(dir.join("a.md"), vec![b'a'; MAX_BYTES as usize]).expect("fixture");
        assert!(read(root.path()).is_ok());
        std::fs::write(dir.join("a.md"), vec![b'a'; MAX_BYTES as usize + 1]).expect("fixture");
        assert!(matches!(
            read(root.path()),
            Err(Fail::Domain(ProjectsError::PromptLimitExceeded { .. }))
        ));
        std::fs::write(dir.join("a.md"), [0xff]).expect("fixture");
        assert!(matches!(
            read(root.path()),
            Err(Fail::Domain(ProjectsError::PromptInvalidEncoding { .. }))
        ));
        std::fs::write(dir.join("a.md"), "ok").expect("fixture");
        for n in 0..48 {
            std::fs::write(dir.join(format!("{n:02}.md")), "ok").expect("fixture");
        }
        assert_eq!(read(root.path()).expect("50 files").items.len(), 50);
        std::fs::write(dir.join("extra.md"), "ok").expect("fixture");
        assert!(matches!(
            read(root.path()),
            Err(Fail::Domain(ProjectsError::PromptLimitExceeded { .. }))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_files_directories_and_project_ancestors() {
        let root = tempfile::tempdir().expect("fixture");
        let project = root.path().join("project");
        let dir = project.join(".devbox/prompts");
        std::fs::create_dir_all(&dir).expect("fixture");
        std::fs::write(root.path().join("outside.md"), "outside").expect("fixture");
        std::os::unix::fs::symlink(root.path().join("outside.md"), dir.join("link.md"))
            .expect("fixture");
        assert!(matches!(
            read(&project),
            Err(Fail::Domain(ProjectsError::UnsafePromptPath { .. }))
        ));
        std::fs::remove_file(dir.join("link.md")).expect("fixture");
        std::fs::remove_dir(&dir).expect("fixture");
        std::os::unix::fs::symlink(root.path(), &dir).expect("fixture");
        assert!(read(&project).is_err());
        std::os::unix::fs::symlink(&project, root.path().join("alias")).expect("fixture");
        assert!(read(&root.path().join("alias")).is_err());
    }
}
