//! Shell integration blocks owned by WSL Desktop.
//!
//! The command layer reads and writes a user's rc file only after explicit confirmation. This
//! module is deliberately filesystem-free: it recognizes one marker-owned block, prepares the
//! next complete file, and fails closed on ambiguous markers.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const MAX_RC_FILE_BYTES: usize = 1024 * 1024;
pub const BEGIN_MARKER: &str = "# >>> devbox WSL Desktop shell integration >>>";
pub const END_MARKER: &str = "# <<< devbox WSL Desktop shell integration <<<";
const LEGACY_BEGIN_MARKER: &str = "# >>> WSL Desktop OSC 7 cwd integration >>>";
const LEGACY_END_MARKER: &str = "# <<< WSL Desktop OSC 7 cwd integration <<<";

const BASH_BLOCK: &str = r#"# >>> devbox WSL Desktop shell integration >>>
# version: 1
__devbox_wsld_encode_path() {
  local value="$1" result="" char hex i
  local LC_ALL=C
  for ((i = 0; i < ${#value}; i++)); do
    char="${value:i:1}"
    case "$char" in
      [A-Za-z0-9._~/-]) result+="$char" ;;
      *) printf -v hex '%02X' "'$char"; result+="%$hex" ;;
    esac
  done
  printf '%s' "$result"
}

__devbox_wsld_report_cwd() {
  local exit_status=$?
  printf '\033]7;file://%s\033\\' "$(__devbox_wsld_encode_path "$PWD")"
  return "$exit_status"
}

if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
  if [[ " ${PROMPT_COMMAND[*]} " != *" __devbox_wsld_report_cwd "* ]]; then
    PROMPT_COMMAND=(__devbox_wsld_report_cwd "${PROMPT_COMMAND[@]}")
  fi
elif [[ ";${PROMPT_COMMAND:-};" != *";__devbox_wsld_report_cwd;"* ]]; then
  PROMPT_COMMAND="__devbox_wsld_report_cwd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi
# <<< devbox WSL Desktop shell integration <<<
"#;

const ZSH_BLOCK: &str = r#"# >>> devbox WSL Desktop shell integration >>>
# version: 1
__devbox_wsld_encode_path() {
  emulate -L zsh
  unsetopt multibyte
  local value="$1" result="" char hex i
  local LC_ALL=C
  for ((i = 1; i <= ${#value}; i++)); do
    char="${value[i]}"
    case "$char" in
      [A-Za-z0-9._~/-]) result+="$char" ;;
      *) printf -v hex '%02X' "'$char"; result+="%$hex" ;;
    esac
  done
  printf '%s' "$result"
}

__devbox_wsld_report_cwd() {
  local exit_status=$?
  printf '\033]7;file://%s\033\\' "$(__devbox_wsld_encode_path "$PWD")"
  return "$exit_status"
}

autoload -Uz add-zsh-hook
if (( ${precmd_functions[(Ie)__devbox_wsld_report_cwd]} == 0 )); then
  add-zsh-hook precmd __devbox_wsld_report_cwd
fi
# <<< devbox WSL Desktop shell integration <<<
"#;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShellKind {
    Bash,
    Zsh,
}

impl ShellKind {
    pub fn rc_file(self) -> &'static str {
        match self {
            Self::Bash => ".bashrc",
            Self::Zsh => ".zshrc",
        }
    }

    pub fn executable_name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShellIntegrationStatus {
    Missing,
    Current,
    Outdated,
    Conflict,
    Blocked,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShellIntegrationAction {
    Install,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentPlan {
    pub before: ShellIntegrationStatus,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct OwnedBlock {
    start: usize,
    end: usize,
}

pub fn canonical_block(shell: ShellKind) -> &'static str {
    match shell {
        ShellKind::Bash => BASH_BLOCK,
        ShellKind::Zsh => ZSH_BLOCK,
    }
}

fn owned_block(input: &str) -> Result<Option<OwnedBlock>, ()> {
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    let mut offset = 0;
    for line in input.split_inclusive('\n') {
        let body = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or_else(|| line.strip_suffix('\n').unwrap_or(line));
        if body == BEGIN_MARKER {
            begins.push((offset, false));
        } else if body == LEGACY_BEGIN_MARKER {
            begins.push((offset, true));
        } else if body == END_MARKER {
            ends.push((offset + line.len(), false));
        } else if body == LEGACY_END_MARKER {
            ends.push((offset + line.len(), true));
        }
        offset += line.len();
    }
    if offset < input.len() {
        return Err(());
    }
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start, legacy_begin)], [(end, legacy_end)])
            if start < end && legacy_begin == legacy_end =>
        {
            Ok(Some(OwnedBlock {
                start: *start,
                end: *end,
            }))
        }
        _ => Err(()),
    }
}

fn normalize_block(input: &str) -> String {
    let mut normalized = input.replace("\r\n", "\n");
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

pub fn inspect_content(input: &str, shell: ShellKind) -> ShellIntegrationStatus {
    if input.len() > MAX_RC_FILE_BYTES {
        return ShellIntegrationStatus::Conflict;
    }
    match owned_block(input) {
        Ok(None) => ShellIntegrationStatus::Missing,
        Ok(Some(range)) => {
            if normalize_block(&input[range.start..range.end]) == canonical_block(shell) {
                ShellIntegrationStatus::Current
            } else {
                ShellIntegrationStatus::Outdated
            }
        }
        Err(()) => ShellIntegrationStatus::Conflict,
    }
}

pub fn plan_content(
    input: &str,
    shell: ShellKind,
    action: ShellIntegrationAction,
) -> Result<ContentPlan, String> {
    let before = inspect_content(input, shell);
    if matches!(
        before,
        ShellIntegrationStatus::Conflict | ShellIntegrationStatus::Blocked
    ) {
        return Err("셸 설정에 중복되거나 완성되지 않은 Devbox marker가 있습니다".into());
    }
    let range = owned_block(input).map_err(|()| "셸 설정 marker를 해석할 수 없습니다")?;
    match action {
        ShellIntegrationAction::Install if before == ShellIntegrationStatus::Current => {
            Ok(ContentPlan { before, next: None })
        }
        ShellIntegrationAction::Install => {
            let next = if let Some(range) = range {
                format!(
                    "{}{}{}",
                    &input[..range.start],
                    canonical_block(shell),
                    &input[range.end..]
                )
            } else {
                let mut next = input.to_owned();
                if !next.is_empty() && !next.ends_with('\n') {
                    next.push('\n');
                }
                next.push_str(canonical_block(shell));
                next
            };
            Ok(ContentPlan {
                before,
                next: Some(next),
            })
        }
        ShellIntegrationAction::Remove if before == ShellIntegrationStatus::Missing => {
            Ok(ContentPlan { before, next: None })
        }
        ShellIntegrationAction::Remove => {
            let range = range.ok_or_else(|| "제거할 셸 연동 block이 없습니다".to_string())?;
            Ok(ContentPlan {
                before,
                next: Some(format!("{}{}", &input[..range.start], &input[range.end..])),
            })
        }
    }
}

pub fn content_revision(exists: bool, input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    exists.hash(&mut hasher);
    input.hash(&mut hasher);
    format!("{:016x}-{}", hasher.finish(), input.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    fn run_shell(shell: &str, script: &str, syntax_only: bool) -> std::process::Output {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut command = Command::new(shell);
        if syntax_only {
            command.arg("-n");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn installs_after_existing_content_without_rewriting_it() {
        let input = "export PATH=/custom/bin:$PATH";
        let plan = plan_content(input, ShellKind::Bash, ShellIntegrationAction::Install).unwrap();
        assert_eq!(plan.before, ShellIntegrationStatus::Missing);
        let next = plan.next.unwrap();
        assert!(next.starts_with("export PATH=/custom/bin:$PATH\n"));
        assert!(next.ends_with(BASH_BLOCK));
        assert_eq!(
            inspect_content(&next, ShellKind::Bash),
            ShellIntegrationStatus::Current
        );
    }

    #[test]
    fn current_install_and_missing_remove_are_noops() {
        assert_eq!(
            plan_content(BASH_BLOCK, ShellKind::Bash, ShellIntegrationAction::Install)
                .unwrap()
                .next,
            None
        );
        assert_eq!(
            plan_content(
                "export EDITOR=vim\n",
                ShellKind::Bash,
                ShellIntegrationAction::Remove
            )
            .unwrap()
            .next,
            None
        );
    }

    #[test]
    fn repairs_only_the_owned_block() {
        let input = format!("before\n{BEGIN_MARKER}\n# version: old\n{END_MARKER}\nafter\n");
        assert_eq!(
            inspect_content(&input, ShellKind::Bash),
            ShellIntegrationStatus::Outdated
        );
        let next = plan_content(&input, ShellKind::Bash, ShellIntegrationAction::Install)
            .unwrap()
            .next
            .unwrap();
        assert!(next.starts_with("before\n"));
        assert!(next.ends_with("after\n"));
        assert_eq!(
            inspect_content(&next, ShellKind::Bash),
            ShellIntegrationStatus::Current
        );
    }

    #[test]
    fn upgrades_the_prior_manually_installed_marker_block() {
        let legacy =
            format!("before\n{LEGACY_BEGIN_MARKER}\nold hook\n{LEGACY_END_MARKER}\nafter\n");
        assert_eq!(
            inspect_content(&legacy, ShellKind::Bash),
            ShellIntegrationStatus::Outdated
        );
        let next = plan_content(&legacy, ShellKind::Bash, ShellIntegrationAction::Install)
            .unwrap()
            .next
            .unwrap();
        assert!(!next.contains(LEGACY_BEGIN_MARKER));
        assert_eq!(
            inspect_content(&next, ShellKind::Bash),
            ShellIntegrationStatus::Current
        );
    }

    #[test]
    fn removes_only_the_owned_block() {
        let input = format!("before\n{BASH_BLOCK}after\n");
        let next = plan_content(&input, ShellKind::Bash, ShellIntegrationAction::Remove)
            .unwrap()
            .next
            .unwrap();
        assert_eq!(next, "before\nafter\n");
    }

    #[test]
    fn fails_closed_on_unbalanced_or_duplicate_markers() {
        let unbalanced = format!("{BEGIN_MARKER}\nno end\n");
        let duplicate = format!("{BASH_BLOCK}{BASH_BLOCK}");
        for input in [unbalanced, duplicate] {
            assert_eq!(
                inspect_content(&input, ShellKind::Bash),
                ShellIntegrationStatus::Conflict
            );
            assert!(
                plan_content(&input, ShellKind::Bash, ShellIntegrationAction::Install).is_err()
            );
            assert!(plan_content(&input, ShellKind::Bash, ShellIntegrationAction::Remove).is_err());
        }
    }

    #[test]
    fn recognizes_crlf_owned_block_without_rewriting_it() {
        let input = BASH_BLOCK.replace('\n', "\r\n");
        assert_eq!(
            inspect_content(&input, ShellKind::Bash),
            ShellIntegrationStatus::Current
        );
    }

    #[test]
    fn revision_covers_existence_and_content() {
        assert_ne!(content_revision(false, ""), content_revision(true, ""));
        assert_ne!(content_revision(true, "a"), content_revision(true, "b"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn canonical_blocks_are_valid_shell_syntax() {
        for (shell, block) in [("bash", BASH_BLOCK), ("zsh", ZSH_BLOCK)] {
            let output = run_shell(shell, block, true);
            assert!(
                output.status.success(),
                "{shell}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn canonical_encoders_percent_encode_special_and_utf8_bytes() {
        for (shell, block) in [("bash", BASH_BLOCK), ("zsh", ZSH_BLOCK)] {
            let script = format!("{block}\n__devbox_wsld_encode_path '/tmp/a b#한글'\n");
            let output = run_shell(shell, &script, false);
            assert!(
                output.status.success(),
                "{shell}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                "/tmp/a%20b%23%ED%95%9C%EA%B8%80"
            );
        }
    }
}
