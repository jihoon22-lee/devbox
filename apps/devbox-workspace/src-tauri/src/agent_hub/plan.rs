use super::{store::AgentTask, AgentIssue};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use terminal_engine::core::workspace::{
    Layout, MultiplexerKind, PaneSizing, WorkspacePane, WorkspaceProfile, WorkspaceTab,
};

pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SLUG_BYTES: usize = 40;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentTool {
    ClaudeCode,
    Codex,
    Custom,
}

pub fn checked_title(title: &str) -> Result<&str, AgentIssue> {
    let title = title.trim();
    if title.is_empty()
        || title.chars().count() > MAX_TITLE_CHARS
        || title.chars().any(char::is_control)
    {
        Err(AgentIssue::TitleInvalid)
    } else {
        Ok(title)
    }
}
pub fn slug(title: &str, now_ms: u64) -> String {
    let mut result = String::new();
    for byte in title.bytes() {
        if result.len() == MAX_SLUG_BYTES {
            break;
        }
        if byte.is_ascii_alphanumeric() {
            result.push(byte.to_ascii_lowercase() as char);
        } else if !result.is_empty() && !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_end_matches('-');
    if result.is_empty() {
        format!("task-{}", now_ms / 1000)
    } else {
        result.into()
    }
}
pub fn unique_slug(base: &str, taken: &HashSet<String>) -> Result<String, AgentIssue> {
    for n in 1..=99 {
        let candidate = if n == 1 {
            base.into()
        } else {
            let suffix = format!("-{n}");
            format!(
                "{}{}",
                truncate_bytes(base, MAX_SLUG_BYTES - suffix.len()).trim_end_matches('-'),
                suffix
            )
        };
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(AgentIssue::SlugExhausted)
}
pub fn branch(slug: &str) -> String {
    format!("agent/{slug}")
}
pub fn valid_target_dir(root: &str) -> bool {
    root.starts_with('/')
        && !root.starts_with("//")
        && root.len() <= 4096
        && devbox_filesystem::parse_safe_project_path(root)
            .is_some_and(|p| p.kind() == devbox_filesystem::ProjectPathKind::Posix)
}
pub fn default_target_dir(base_root: &str, slug: &str) -> Result<String, AgentIssue> {
    if !valid_target_dir(base_root)
        || slug.is_empty()
        || slug.len() > MAX_SLUG_BYTES
        || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AgentIssue::TargetInvalid);
    }
    let result = format!("{}-{slug}", base_root.trim_end_matches('/'));
    if !valid_target_dir(&result) {
        return Err(AgentIssue::TargetInvalid);
    }
    Ok(result)
}
pub fn tool_command(tool: AgentTool, custom: Option<&str>) -> Result<String, AgentIssue> {
    match (tool, custom) {
        (AgentTool::ClaudeCode, None) => Ok("claude".into()),
        (AgentTool::Codex, None) => Ok("codex".into()),
        (AgentTool::Custom, Some(value)) => {
            terminal_engine::core::workspace::validate_start_command(value)
                .map_err(|_| AgentIssue::CommandInvalid)?;
            Ok(value.trim().into())
        }
        _ => Err(AgentIssue::CommandInvalid),
    }
}
fn truncate_bytes(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
pub fn agent_layout(task: &AgentTask, distro: &str) -> WorkspaceProfile {
    WorkspaceProfile {
        id: task.id.clone(),
        name: format!("에이전트 · {}", truncate_bytes(&task.title, 104)),
        tabs: vec![WorkspaceTab {
            id: "agent".into(),
            title: truncate_bytes(&task.title, 120).into(),
            custom_title: true,
            layout: Layout::Grid,
            pane_keys: vec!["agent".into()],
            sizing: PaneSizing {
                columns: vec![1.0],
                rows: vec![1.0],
            },
        }],
        panes: vec![WorkspacePane {
            key: "agent".into(),
            distro: distro.into(),
            cwd: Some(task.target_dir.clone()),
            start_command: Some(task.command.clone()),
            multiplexer: MultiplexerKind::Native,
        }],
        active_tab_id: "agent".into(),
        active_pane_key: Some("agent".into()),
        auto_run: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn long_multibyte_titles_and_numbered_slugs_remain_valid() {
        let task = crate::agent_hub::store::tests::task("t1", &"가".repeat(120));
        agent_layout(&task, "Ubuntu").validate().unwrap();
        let base = "a".repeat(MAX_SLUG_BYTES);
        let next = unique_slug(&base, &[base.clone()].into()).unwrap();
        assert_eq!(next.len(), MAX_SLUG_BYTES);
        assert!(next.ends_with("-2"));
        for root in ["/", "//server/share", "/work/../other", "relative"] {
            assert_eq!(
                default_target_dir(root, "task"),
                Err(AgentIssue::TargetInvalid)
            );
        }
    }

    #[test]
    fn slugs_are_ascii_and_fall_back_to_time_for_non_ascii_titles() {
        assert_eq!(slug("Fix login: retry 3x!", 0), "fix-login-retry-3x");
        assert_eq!(slug("  --Hello__World--  ", 0), "hello-world");
        assert_eq!(slug("로그인 고치기", 1_790_000_000_123), "task-1790000000");
        assert!(slug(&"a".repeat(200), 0).len() <= MAX_SLUG_BYTES);
    }

    #[test]
    fn duplicate_slugs_get_numbered_suffixes() {
        let taken: HashSet<String> = ["fix".into(), "fix-2".into()].into();
        assert_eq!(unique_slug("fix", &taken).unwrap(), "fix-3");
        assert_eq!(unique_slug("new", &taken).unwrap(), "new");
        let full: HashSet<String> = (1..=99)
            .map(|n| if n == 1 { "x".into() } else { format!("x-{n}") })
            .collect();
        assert_eq!(unique_slug("x", &full), Err(AgentIssue::SlugExhausted));
    }

    #[test]
    fn default_target_is_a_sibling_folder_or_reuses_a_worktrees_parent() {
        assert_eq!(
            default_target_dir("/home/me/projects/devbox", "fix").unwrap(),
            "/home/me/projects/devbox-fix"
        );
        assert_eq!(
            default_target_dir("/home/me/projects/.worktrees/devbox-a", "fix").unwrap(),
            "/home/me/projects/.worktrees/devbox-a-fix"
        );
        assert_eq!(
            default_target_dir("/", "fix"),
            Err(AgentIssue::TargetInvalid)
        );
        assert_eq!(
            default_target_dir("relative/path", "fix"),
            Err(AgentIssue::TargetInvalid)
        );
    }

    #[test]
    fn tool_commands_are_fixed_or_validated() {
        assert_eq!(tool_command(AgentTool::ClaudeCode, None).unwrap(), "claude");
        assert_eq!(tool_command(AgentTool::Codex, None).unwrap(), "codex");
        assert_eq!(
            tool_command(AgentTool::Custom, Some("aider --model x")).unwrap(),
            "aider --model x"
        );
        assert_eq!(
            tool_command(AgentTool::Custom, None),
            Err(AgentIssue::CommandInvalid)
        );
        assert_eq!(
            tool_command(
                AgentTool::Custom,
                Some("tool --token=sk-abcdefghijklmnopqrstuvwx")
            ),
            Err(AgentIssue::CommandInvalid)
        );
        assert_eq!(
            tool_command(AgentTool::ClaudeCode, Some("rm -rf /")),
            Err(AgentIssue::CommandInvalid)
        );
    }

    #[test]
    fn agent_layout_is_one_auto_run_pane_in_the_worktree() {
        let task = crate::agent_hub::store::tests::task("t1", "Fix login");
        let layout = agent_layout(&task, "Ubuntu-24.04");
        layout.validate().unwrap();
        assert!(layout.auto_run);
        assert_eq!(layout.panes.len(), 1);
        assert_eq!(
            layout.panes[0].cwd.as_deref(),
            Some(task.target_dir.as_str())
        );
        assert_eq!(layout.panes[0].start_command.as_deref(), Some("claude"));
        assert_eq!(layout.panes[0].distro, "Ubuntu-24.04");
    }
}
