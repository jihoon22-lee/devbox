// crates/agents/src/profile.rs
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Hooks {
    Claude,
    Codex,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolProfile {
    pub id: String,
    pub label: String,
    pub command: String,
    pub hooks: Hooks,
    #[serde(default)]
    pub waiting_patterns: Vec<String>,
}

/// 기본 프로필. Codex 0.162.1 S0a에서 관찰한 승인 화면. 다른 UI 유형·버전은 실제 fixture로 보강한다.
pub fn builtin() -> Vec<ToolProfile> {
    vec![
        ToolProfile {
            id: "claude".into(),
            label: "Claude Code".into(),
            command: "claude".into(),
            hooks: Hooks::Claude,
            waiting_patterns: vec![],
        },
        ToolProfile {
            id: "codex".into(),
            label: "Codex".into(),
            command: "codex".into(),
            hooks: Hooks::Codex,
            waiting_patterns: vec![
                r"(?im)^\s*Would you like to run the following command\?\s*$".into(),
            ],
        },
    ]
}

pub fn resolve(custom: &[ToolProfile], id: &str) -> Option<ToolProfile> {
    custom
        .iter()
        .chain(builtin().iter())
        .find(|p| p.id == id)
        .cloned()
}

/// Parse domain-owned profile types without making core depend on agents.
pub fn configured(
    config: &devbox_core::config::AgentsConfig,
) -> Result<Vec<ToolProfile>, toml::de::Error> {
    config
        .profiles
        .iter()
        .cloned()
        .map(toml::Value::try_into)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_profiles_override_defaults_and_parse_strictly() {
        let config: devbox_core::Config = toml::from_str("[[agents.profiles]]\nid='claude'\nlabel='Custom'\ncommand='claude --verbose'\nhooks='none'\nwaitingPatterns=['ready']").expect("config");
        let custom = configured(&config.agents).expect("typed profiles");
        assert_eq!(resolve(&custom, "claude").expect("custom").label, "Custom");
        assert_eq!(
            resolve(&custom, "codex").expect("builtin").hooks,
            Hooks::Codex
        );
        assert!(resolve(&custom, "missing").is_none());
        for field in ["unexpected=true", "hooks='invalid'", "waitingPatterns=[1]"] {
            let text = format!("[[agents.profiles]]\nid='x'\nlabel='X'\ncommand='x'\n{field}");
            let config: devbox_core::Config = toml::from_str(&text).expect("neutral config");
            assert!(configured(&config.agents).is_err());
        }
    }
}
