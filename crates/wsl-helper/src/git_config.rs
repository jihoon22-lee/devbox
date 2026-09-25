//! Pure inspection of native-read Git configuration bytes. Never follow includes
//! or discover files through the parser; the platform owner admits every path.
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, &'static str>;
pub const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_REFERENCES: usize = 128;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigReferences {
    pub includes: Vec<String>,
    pub hook_paths: Vec<String>,
    // Known execution-related keys only. Values and credential URL subsections
    // are never part of the review projection.
    pub execution_keys: BTreeSet<String>,
}

pub fn inspect(bytes: &[u8]) -> Result<ConfigReferences> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("git_config_limit");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "git_config_invalid")?;
    let config = gix_config::File::try_from(text).map_err(|_| "git_config_invalid")?;
    let mut result = ConfigReferences::default();
    for section in config.sections() {
        let name = section.header().name().to_ascii_lowercase();
        if name == b"include" || name == b"includeif" {
            // Capture conditional sources conservatively as well. The approval
            // cannot become broader merely because a branch/remote later changes.
            for value in section.values("path") {
                let path = std::str::from_utf8(value.as_ref()).map_err(|_| "git_config_invalid")?;
                if path.is_empty() || path.chars().any(char::is_control) {
                    return Err("git_config_invalid");
                }
                result.includes.push(path.to_owned());
            }
        }
        if name == b"core" && section.header().subsection_name().is_none() {
            for value in section.values("hooksPath") {
                let path = std::str::from_utf8(value.as_ref()).map_err(|_| "git_config_invalid")?;
                if path.chars().any(char::is_control) {
                    return Err("git_config_invalid");
                }
                result.hook_paths.push(path.to_owned());
            }
        }
        let name = std::str::from_utf8(&name).map_err(|_| "git_config_invalid")?;
        for key in section.value_names() {
            let key = key.to_ascii_lowercase();
            if matches!(
                (name, key.as_str()),
                (
                    "core",
                    "hookspath" | "fsmonitor" | "sshcommand" | "gitproxy" | "askpass" | "editor"
                ) | ("credential", "helper")
                    | ("filter", "clean" | "smudge" | "process")
                    | ("diff", "external" | "command" | "textconv")
                    | ("merge", "driver")
                    | ("gpg", "program")
                    | ("hook", "command" | "event" | "enabled")
                    | ("mergetool" | "difftool", "cmd")
            ) {
                result.execution_keys.insert(format!("{name}.{key}"));
            }
        }
        if result.includes.len() + result.hook_paths.len() > MAX_REFERENCES {
            return Err("git_config_limit");
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_git_quoting_multivars_and_conditional_references_without_io() {
        let config = inspect(
            r#"[Include]
  path = "한글 directory/first.conf"
  path = second\
.conf
[includeIf "onbranch:topic/**"]
  path = "~/conditional.conf"
[core]
  hooksPath = "C:\\hooks\\owned"
  hooksPath =
[credential "https://secret-user:secret-value@example.invalid"]
  helper = !secret-command
"#
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            config.includes,
            [
                "한글 directory/first.conf",
                "second.conf",
                "~/conditional.conf"
            ]
        );
        assert_eq!(config.hook_paths, [r"C:\hooks\owned", ""]);
        assert_eq!(
            config.execution_keys,
            BTreeSet::from(["core.hookspath".into(), "credential.helper".into()])
        );
        assert!(!format!("{config:?}").contains("secret"));
    }

    #[test]
    fn rejects_invalid_or_excessive_config_with_fixed_errors() {
        assert_eq!(inspect(b"[unterminated"), Err("git_config_invalid"));
        assert_eq!(
            inspect(&vec![b'x'; MAX_CONFIG_BYTES + 1]),
            Err("git_config_limit")
        );
        assert_eq!(
            inspect(
                format!("[include]\n{}", "path = item\n".repeat(MAX_REFERENCES + 1)).as_bytes()
            ),
            Err("git_config_limit")
        );
        assert_eq!(inspect(b"[include]\npath =\n"), Err("git_config_invalid"));
    }
}
