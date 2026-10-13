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
                r"(?im)^\s*3\. No, and tell Codex what to do differently \(esc\)\s*\n\s*Press enter to confirm or esc to cancel\s*$".into(),
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
) -> Result<Vec<ToolProfile>, ProfileError> {
    let profiles: Vec<ToolProfile> = config
        .profiles
        .iter()
        .cloned()
        .map(toml::Value::try_into)
        .collect::<Result<_, toml::de::Error>>()?;
    compiled(&crate::model::AgentProfiles {
        items: profiles.clone(),
    })?;
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_configured_ids_are_rejected_instead_of_mixing_launch_and_waiting_rules() {
        for id in ["codex", "custom"] {
            let text = format!(
                "[[agents.profiles]]\nid='{id}'\nlabel='First'\ncommand='first'\nhooks='codex'\nwaitingPatterns=['first']\n[[agents.profiles]]\nid='{id}'\nlabel='Second'\ncommand='second'\nhooks='none'\nwaitingPatterns=['second']\n"
            );
            let config: devbox_core::Config = toml::from_str(&text).expect("owned config");
            assert!(
                configured(&config.agents).is_err(),
                "duplicate {id} must be rejected"
            );
        }
    }
    #[test]
    fn duplicate_programmatic_ids_are_rejected_by_the_waiting_profile_compiler() {
        let first = ToolProfile {
            id: "custom".into(),
            label: "First".into(),
            command: "first".into(),
            hooks: Hooks::Codex,
            waiting_patterns: vec!["first".into()],
        };
        let mut second = first.clone();
        second.command = "second".into();
        second.waiting_patterns = vec!["second".into()];
        assert!(
            compiled(&crate::model::AgentProfiles {
                items: vec![first, second]
            })
            .is_err()
        );
    }
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

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("invalid agent profiles: {0}")]
    Configuration(#[from] toml::de::Error),
    #[error("duplicate agent profile id: {id}")]
    DuplicateId { id: String },
    #[error("invalid waiting pattern for profile {profile}: {source}")]
    WaitingPattern {
        profile: String,
        #[source]
        source: regex::Error,
    },
}
pub(crate) fn compiled(
    profiles: &crate::model::AgentProfiles,
) -> Result<std::collections::HashMap<String, Vec<regex::Regex>>, ProfileError> {
    let mut ids = std::collections::HashSet::new();
    for profile in &profiles.items {
        if !ids.insert(&profile.id) {
            return Err(ProfileError::DuplicateId {
                id: profile.id.clone(),
            });
        }
    }
    profiles
        .items
        .iter()
        .map(|p| {
            let patterns = p
                .waiting_patterns
                .iter()
                .map(|text| {
                    regex::Regex::new(text).map_err(|source| ProfileError::WaitingPattern {
                        profile: p.id.clone(),
                        source,
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok((p.id.clone(), patterns))
        })
        .collect()
}
