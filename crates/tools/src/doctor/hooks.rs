use super::{AgentTool, ProbeIssue, Probed};
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Deserialize)]
struct ClaudeSettings {
    #[serde(default)]
    hooks: BTreeMap<String, Vec<ClaudeGroup>>,
}
#[derive(Deserialize)]
struct ClaudeGroup {
    #[serde(default)]
    hooks: Vec<ClaudeHook>,
}
#[derive(Deserialize)]
struct ClaudeHook {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    command: String,
}
#[derive(Deserialize)]
struct CodexSettings {
    notify: Option<Vec<String>>,
}
fn is_devbox_binary(binary: &str) -> bool {
    std::path::Path::new(binary.trim().trim_matches(['\'', '"']))
        .file_name()
        .is_some_and(|name| name == "devbox")
}
/// Configuration evidence only; never execute hooks while diagnosing them.
pub fn hook_present(agent: AgentTool, text: &str) -> Probed<bool> {
    match agent {
        AgentTool::Claude => {
            let settings: ClaudeSettings =
                serde_json::from_str(text).map_err(|_| ProbeIssue::InvalidData)?;
            Ok(settings
                .hooks
                .values()
                .flatten()
                .flat_map(|group| &group.hooks)
                .any(|hook| {
                    hook.kind == "command"
                        && hook
                            .command
                            .split_once(" agent event --from claude")
                            .is_some_and(|(binary, _)| is_devbox_binary(binary))
                }))
        }
        AgentTool::Codex => {
            let settings: CodexSettings =
                toml::from_str(text).map_err(|_| ProbeIssue::InvalidData)?;
            Ok(settings.notify.is_some_and(|args| {
                args.len() >= 5
                    && is_devbox_binary(&args[0])
                    && args[1..5] == ["agent", "event", "--from", "codex"]
            }))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_hooks_use_actual_fields_not_comments_or_unrelated_values() {
        assert_eq!(
            hook_present(
                AgentTool::Claude,
                r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"'/owned/devbox' agent event --from claude"}]}]}}"#
            ),
            Ok(true)
        );
        assert_eq!(
            hook_present(
                AgentTool::Codex,
                "notify=['/owned/devbox','agent','event','--from','codex']"
            ),
            Ok(true)
        );
        assert_eq!(
            hook_present(
                AgentTool::Claude,
                r#"{"note":"devbox agent event --from claude"}"#
            ),
            Ok(false)
        );
        assert_eq!(
            hook_present(
                AgentTool::Codex,
                "# notify=['/owned/devbox','agent','event','--from','codex']"
            ),
            Ok(false)
        );
        assert_eq!(
            hook_present(
                AgentTool::Codex,
                "notify=['/owned/devbox','agent','event','--from','claude']"
            ),
            Ok(false)
        );
        assert_eq!(
            hook_present(
                AgentTool::Claude,
                r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"'/owned/not-devbox' agent event --from claude"}]}]}}"#
            ),
            Ok(false)
        );
        assert_eq!(
            hook_present(
                AgentTool::Codex,
                "notify=['/owned/not-devbox','agent','event','--from','codex']"
            ),
            Ok(false)
        );
    }
}
