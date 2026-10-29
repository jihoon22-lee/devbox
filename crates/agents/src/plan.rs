use crate::model::AgentsError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SLUG_BYTES: usize = 40;

pub fn checked_title(title: &str) -> Result<&str, AgentsError> {
    let t = title.trim();
    if t.is_empty() || t.chars().count() > MAX_TITLE_CHARS || t.chars().any(char::is_control) {
        Err(AgentsError::TitleInvalid)
    } else {
        Ok(t)
    }
}

pub fn slug(title: &str, now_ms: u64) -> String {
    let mut out = String::new();
    for b in title.bytes() {
        if out.len() == MAX_SLUG_BYTES {
            break;
        }
        if b.is_ascii_alphanumeric() {
            out.push(b.to_ascii_lowercase() as char);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        format!("task-{}", now_ms / 1000)
    } else {
        out.into()
    }
}

fn checked_slug(base: &str) -> Result<&str, AgentsError> {
    if base.is_empty()
        || base.len() > MAX_SLUG_BYTES
        || !base
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || base.starts_with('-')
        || base.ends_with('-')
        || base.contains("--")
    {
        return Err(AgentsError::TitleInvalid);
    }
    Ok(base)
}

/// The name a client gave for a task's branch: the part after `agent/`. It has to be a slug
/// already, because the daemon never rewrites a name it was given. The slug rule is a strict
/// subset of `git check-ref-format`, so a name that passes is always a valid ref component.
pub fn explicit_slug(name: &str) -> Result<&str, AgentsError> {
    checked_slug(name).map_err(|_| AgentsError::InvalidInput)
}

/// The slug and work folder of a new task. A name a client gave is used exactly or refused with
/// `BranchExists` (a branch, an earlier task or a folder already has it); only a name made from
/// the title is numbered. `taken` is left alone for a given name.
pub fn choose(
    repo: &Path,
    title: &str,
    now_ms: u64,
    explicit: Option<&str>,
    taken: &mut HashSet<String>,
) -> Result<(String, PathBuf), AgentsError> {
    if let Some(name) = explicit {
        let slug = explicit_slug(name)?;
        let dir = worktree_dir(repo, slug)?;
        if taken.contains(slug) || std::fs::symlink_metadata(&dir).is_ok() {
            return Err(AgentsError::BranchExists {
                branch: branch_for(slug)?,
            });
        }
        return Ok((slug.to_owned(), dir));
    }
    let base = slug(title, now_ms);
    loop {
        let candidate = unique_slug(&base, taken)?;
        let dir = worktree_dir(repo, &candidate)?;
        if std::fs::symlink_metadata(&dir).is_ok() {
            taken.insert(candidate);
            continue;
        }
        return Ok((candidate, dir));
    }
}

pub fn unique_slug(base: &str, taken: &HashSet<String>) -> Result<String, AgentsError> {
    let base = checked_slug(base)?;
    for n in 1..=99 {
        let candidate = if n == 1 {
            base.into()
        } else {
            let suffix = format!("-{n}");
            let keep = MAX_SLUG_BYTES - suffix.len();
            format!(
                "{}{suffix}",
                base[..base.len().min(keep)].trim_end_matches('-')
            )
        };
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(AgentsError::SlugExhausted)
}

pub fn branch_for(slug: &str) -> Result<String, AgentsError> {
    Ok(format!("agent/{}", checked_slug(slug)?))
}

pub fn worktree_dir(project: &Path, slug: &str) -> Result<PathBuf, AgentsError> {
    let slug = checked_slug(slug)?;
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    Ok(project
        .parent()
        .unwrap_or(Path::new("/"))
        .join(format!("{name}-agents"))
        .join(slug))
}

pub use devbox_core::sh_quote; // Task 3에서 core에 둔 함수
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn slugs_are_ascii_and_bounded() {
        assert_eq!(slug("Fix login bug!", 0), "fix-login-bug");
        assert_eq!(slug("한글만", 1_700_000_000_000), "task-1700000000");
        assert!(slug(&"x".repeat(100), 0).len() <= MAX_SLUG_BYTES);
    }

    #[test]
    fn unique_slugs_get_numbered_suffixes() {
        let taken: HashSet<String> = ["fix".into(), "fix-2".into()].into();
        assert_eq!(unique_slug("fix", &taken).expect("owned fixture"), "fix-3");
    }

    #[test]
    fn worktree_dir_sits_next_to_the_project() {
        assert_eq!(
            worktree_dir(std::path::Path::new("/home/u/projects/devbox"), "fix")
                .expect("valid slug"),
            std::path::PathBuf::from("/home/u/projects/devbox-agents/fix")
        );
    }

    #[test]
    fn shell_quoting_survives_quotes_and_newlines() {
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("x\ny"), "'x\ny'");
    }

    #[test]
    fn invalid_branch_name_is_rejected_by_the_daemon() {
        let too_long = "x".repeat(MAX_SLUG_BYTES + 1);
        for invalid in [
            "",
            "Fix",
            "fix login",
            "fix--login",
            "-fix",
            "fix-",
            "fix_login",
            "fix/login",
            "agent/fix",
            ".hidden",
            "fix.lock",
            "../escape",
            "가나다",
            "fix\n",
            too_long.as_str(),
        ] {
            assert_eq!(
                explicit_slug(invalid),
                Err(AgentsError::InvalidInput),
                "{invalid:?}"
            );
        }
        let longest = "x".repeat(MAX_SLUG_BYTES);
        for valid in ["fix", "fix-login", "v2", "0", "a-b-c", longest.as_str()] {
            assert_eq!(explicit_slug(valid), Ok(valid), "{valid:?}");
        }
    }

    #[test]
    fn a_typed_name_is_used_as_it_is_or_refused_never_numbered() {
        let root = tempfile::tempdir().expect("owned fixture");
        let repo = root.path().join("project");
        let taken: HashSet<String> = ["fix".into()].into();
        // Free: used as given, though the title would have given another name.
        let mut seen = taken.clone();
        let chosen = choose(&repo, "Some title", 0, Some("custom"), &mut seen).expect("free name");
        assert_eq!(
            chosen,
            ("custom".into(), root.path().join("project-agents/custom"))
        );
        // Taken by a branch or an earlier task: refused, and nothing is recorded as taken.
        let mut seen = taken.clone();
        assert_eq!(
            choose(&repo, "Some title", 0, Some("fix"), &mut seen),
            Err(AgentsError::BranchExists {
                branch: "agent/fix".into()
            })
        );
        assert_eq!(seen, taken);
        // A folder already at its place takes the name too.
        std::fs::create_dir_all(root.path().join("project-agents/occupied")).expect("owned folder");
        assert_eq!(
            choose(&repo, "t", 0, Some("occupied"), &mut seen),
            Err(AgentsError::BranchExists {
                branch: "agent/occupied".into()
            })
        );
        // An invalid name is refused as input, before the filesystem or the set matter.
        assert_eq!(
            choose(&repo, "t", 0, Some("Bad Name"), &mut seen),
            Err(AgentsError::InvalidInput)
        );
        assert_eq!(seen, taken);
    }

    #[test]
    fn a_name_made_from_the_title_is_still_numbered_when_taken() {
        let root = tempfile::tempdir().expect("owned fixture");
        let repo = root.path().join("project");
        // `fix-login` has a folder in the way and `fix-login-2` is taken.
        std::fs::create_dir_all(root.path().join("project-agents/fix-login"))
            .expect("owned folder");
        let mut taken: HashSet<String> = ["fix-login-2".into()].into();
        let (slug, dir) = choose(&repo, "Fix login", 0, None, &mut taken).expect("numbered");
        assert_eq!(slug, "fix-login-3");
        assert_eq!(dir, root.path().join("project-agents/fix-login-3"));
        // A title without ASCII keeps the timestamp name when no name is given.
        let (slug, _) = choose(
            &repo,
            "한글만",
            1_700_000_000_000,
            None,
            &mut HashSet::new(),
        )
        .expect("fallback");
        assert_eq!(slug, "task-1700000000");
    }

    #[test]
    fn titles_must_be_short_and_printable() {
        assert!(checked_title("  ok  ").is_ok());
        assert!(checked_title("").is_err());
        assert!(checked_title("a\u{7}b").is_err());
        assert!(checked_title(&"가".repeat(121)).is_err());
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    #[test]
    fn noncanonical_slug_is_rejected_before_resource_naming() {
        for invalid in ["fix--login", "../escape", "가나다"] {
            assert!(unique_slug(invalid, &HashSet::new()).is_err());
            assert!(branch_for(invalid).is_err());
            assert!(worktree_dir(Path::new("/tmp/project"), invalid).is_err());
        }
        let canonical = unique_slug("fix-login", &HashSet::new()).expect("canonical slug");
        assert_eq!(branch_for(&canonical).expect("branch"), "agent/fix-login");
        assert_eq!(
            worktree_dir(Path::new("/tmp/project"), &canonical).expect("path"),
            PathBuf::from("/tmp/project-agents/fix-login")
        );
    }
    #[test]
    fn public_unique_slug_rejects_unsafe_input_and_exhaustion() {
        for unsafe_base in [
            "../escape",
            "/absolute",
            "가나다",
            "",
            "-foo",
            "foo-",
            &"x".repeat(41),
        ] {
            assert!(unique_slug(unsafe_base, &HashSet::new()).is_err());
        }
        let taken: HashSet<_> = (1..=99)
            .map(|n| {
                if n == 1 {
                    "fix".into()
                } else {
                    format!("fix-{n}")
                }
            })
            .collect();
        assert!(matches!(
            unique_slug("fix", &taken),
            Err(AgentsError::SlugExhausted)
        ));
        let base = "x".repeat(MAX_SLUG_BYTES);
        let taken = HashSet::from([base.clone()]);
        assert_eq!(
            unique_slug(&base, &taken).expect("suffix").len(),
            MAX_SLUG_BYTES
        );
        assert!(worktree_dir(Path::new("/tmp/project"), "../../escape").is_err());
    }
}
