//! The legacy session stores one last workspace, not a folder history.
//! Classification is metadata-only; registration must perform its own OS review.
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Windows,
    Wsl,
    Unsupported,
}
#[derive(Debug, Serialize)]
pub struct Proposal {
    pub path: String,
    pub target: Target,
}
pub fn proposal(path: Option<&str>) -> Result<Option<Proposal>, &'static str> {
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    if path.len() > 32768 {
        return Err("legacy_workspace_limit");
    }
    let target = match devbox_wsl::path::parse_wsl_unc_path(path) {
        Ok(Some(_)) => Target::Wsl,
        Ok(None) => match devbox_filesystem::parse_safe_project_path(path) {
            Some(parsed)
                if matches!(
                    parsed.kind(),
                    devbox_filesystem::ProjectPathKind::WindowsDrive
                        | devbox_filesystem::ProjectPathKind::WindowsUnc
                ) =>
            {
                Target::Windows
            }
            _ => Target::Unsupported,
        },
        Err(_) => Target::Unsupported,
    };
    Ok(Some(Proposal {
        path: path.into(),
        target,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_without_resolving_paths_or_starting_a_distro() {
        for (path, target) in [
            (r"C:\missing\한글 폴더", Target::Windows),
            (r"\\offline\share\folder", Target::Windows),
            (r"\\wsl.localhost\Missing-Distro\home\fixture", Target::Wsl),
            ("/home/fixture", Target::Unsupported),
            ("../relative", Target::Unsupported),
            (r"C:\fixture\..\escape", Target::Unsupported),
        ] {
            let result = proposal(Some(path)).unwrap().unwrap();
            assert_eq!(result.path, path);
            assert_eq!(result.target, target);
        }
        assert!(proposal(None).unwrap().is_none());
        assert!(proposal(Some("")).unwrap().is_none());
        assert_eq!(
            proposal(Some(&"x".repeat(32769))).unwrap_err(),
            "legacy_workspace_limit"
        );
    }
}
