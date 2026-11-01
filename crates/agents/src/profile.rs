// crates/agents/src/profile.rs
use crate::{family_patterns::family_patterns, watch::WaitingPatterns};
use devbox_protocol::system::StartupProblem;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Hooks {
    Claude,
    Codex,
    Opencode,
    Pi,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolProfile {
    pub id: String,
    pub label: String,
    pub command: String,
    pub hooks: Hooks,
    // `waiting_patterns` is accepted too, the way the rest of `config.toml` is spelled.
    #[serde(default, alias = "waiting_patterns")]
    pub waiting_patterns: Vec<String>,
}

/// 기본 프로필. 입력 대기 화면 패턴은 프로필이 아니라 `hooks`를 따라 붙는다(`family_patterns`).
pub fn builtin() -> Vec<ToolProfile> {
    let profile = |id: &str, label: &str, hooks| ToolProfile {
        id: id.into(),
        label: label.into(),
        command: id.into(),
        hooks,
        waiting_patterns: Vec::new(),
    };
    vec![
        profile("claude", "Claude Code", Hooks::Claude),
        profile("codex", "Codex", Hooks::Codex),
        profile("opencode", "opencode", Hooks::Opencode),
        profile("pi", "pi", Hooks::Pi),
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

/// Like [`configured`], but an invalid entry, or a later entry repeating an id, is skipped
/// and reported instead of failing the whole list. Builtin profiles stay available.
pub fn configured_lenient(
    config: &devbox_core::config::AgentsConfig,
) -> (Vec<ToolProfile>, Vec<StartupProblem>) {
    let file = config
        .profiles_file
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let (mut profiles, mut problems) = (Vec::new(), Vec::new());
    let mut ids = std::collections::HashSet::new();
    for (index, value) in config.profiles.iter().enumerate() {
        let id = value.get("id").and_then(toml::Value::as_str);
        // The first entry with an id owns it, even when that entry is invalid.
        let checked = match id {
            Some(id) if !ids.insert(id) => Err("duplicate id".to_owned()),
            _ => entry(value),
        };
        match checked {
            Ok(profile) => profiles.push(profile),
            Err(message) => {
                let label = match id {
                    Some(id) => format!("entry {} (id {id:?})", index + 1),
                    None => format!("entry {}", index + 1),
                };
                // Regex errors draw a caret diagram over several lines.
                let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
                problems.push(StartupProblem::ConfigSection {
                    file: file.clone(),
                    section: "agents.profiles".into(),
                    message: format!("{label}: {message}"),
                });
            }
        }
    }
    (profiles, problems)
}

/// One `[[agents.profiles]]` entry as [`configured`] would accept it.
fn entry(value: &toml::Value) -> Result<ToolProfile, String> {
    // Serde also reads a positional array as a struct; entries must be tables.
    if !value.is_table() {
        return Err("must be a table".into());
    }
    let profile: ToolProfile = value
        .clone()
        .try_into()
        .map_err(|error: toml::de::Error| error.message().to_owned())?;
    compiled(&crate::model::AgentProfiles {
        items: vec![profile.clone()],
    })
    .map_err(|error| error.to_string())?;
    Ok(profile)
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
    fn config(text: &str) -> devbox_core::config::AgentsConfig {
        let mut config = toml::from_str::<devbox_core::Config>(text)
            .expect("owned config")
            .agents;
        config.profiles_file = Some("/owned/config.toml".into());
        config
    }
    /// `(section, message)` of each problem, which must all name the profiles' file.
    fn reported(problems: &[StartupProblem]) -> Vec<(String, String)> {
        problems
            .iter()
            .map(|problem| match problem {
                StartupProblem::ConfigSection {
                    file,
                    section,
                    message,
                } => {
                    assert_eq!(file, "/owned/config.toml");
                    (section.clone(), message.clone())
                }
                other => panic!("unexpected problem {other:?}"),
            })
            .collect()
    }
    #[test]
    fn snake_case_waiting_patterns_alias_is_accepted() {
        let config = config(
            "[[agents.profiles]]\nid='x'\nlabel='X'\ncommand='x'\nhooks='none'\nwaiting_patterns=['ready']\n",
        );
        let (profiles, problems) = configured_lenient(&config);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(profiles[0].waiting_patterns, ["ready"]);
        assert_eq!(
            configured(&config).expect("strict")[0].waiting_patterns,
            ["ready"]
        );
    }
    #[test]
    fn invalid_profile_is_skipped_and_reported_while_builtins_remain() {
        let mut config = config(concat!(
            "[[agents.profiles]]\nid='mine'\nlabel='Mine'\ncommand='mine'\nhooks='none'\n",
            "[[agents.profiles]]\nid='claude'\nlabel='Broken'\ncommand='claude'\nhooks='invalid'\n",
            "[[agents.profiles]]\nid='regex'\nlabel='R'\ncommand='r'\nhooks='none'\nwaitingPatterns=['(']\n",
        ));
        // Serde would read a positional array as a profile; only tables are entries.
        config.profiles.push(toml::Value::Array(vec![
            "x".into(),
            "X".into(),
            "x".into(),
            "none".into(),
        ]));
        let (profiles, problems) = configured_lenient(&config);
        assert_eq!(
            profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["mine"]
        );
        let problems = reported(&problems);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems
                .iter()
                .all(|(section, _)| section == "agents.profiles")
        );
        assert!(
            problems[0].1.starts_with("entry 2 (id \"claude\"): "),
            "{problems:?}"
        );
        assert!(
            problems[1].1.starts_with("entry 3 (id \"regex\"): "),
            "{problems:?}"
        );
        assert!(problems[2].1.starts_with("entry 4: "), "{problems:?}");
        assert_eq!(
            resolve(&profiles, "claude").expect("builtin").label,
            "Claude Code"
        );
        assert!(resolve(&profiles, "codex").is_some());
        assert!(compiled(&crate::model::AgentProfiles { items: profiles }).is_ok());
    }
    #[test]
    fn duplicate_custom_id_keeps_the_first_and_reports_the_second() {
        let both = config(concat!(
            "[[agents.profiles]]\nid='custom'\nlabel='First'\ncommand='first'\nhooks='none'\n",
            "[[agents.profiles]]\nid='custom'\nlabel='Second'\ncommand='second'\nhooks='none'\n",
        ));
        let (profiles, problems) = configured_lenient(&both);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].label, "First");
        let problems = reported(&problems);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].1, "entry 2 (id \"custom\"): duplicate id");
        // The first entry owns the id even when it is invalid.
        let first_invalid = config(concat!(
            "[[agents.profiles]]\nid='custom'\nlabel='First'\ncommand='first'\nhooks='bad'\n",
            "[[agents.profiles]]\nid='custom'\nlabel='Second'\ncommand='second'\nhooks='none'\n",
        ));
        let (profiles, problems) = configured_lenient(&first_invalid);
        assert!(profiles.is_empty());
        let problems = reported(&problems);
        assert_eq!(problems[1].1, "entry 2 (id \"custom\"): duplicate id");
    }
    #[test]
    fn builtin_profiles_are_claude_codex_opencode_pi_in_order() {
        let builtin = builtin();
        let order: Vec<_> = builtin
            .iter()
            .map(|p| (p.id.as_str(), p.label.as_str(), p.command.as_str(), p.hooks))
            .collect();
        assert_eq!(
            order,
            [
                ("claude", "Claude Code", "claude", Hooks::Claude),
                ("codex", "Codex", "codex", Hooks::Codex),
                ("opencode", "opencode", "opencode", Hooks::Opencode),
                ("pi", "pi", "pi", Hooks::Pi),
            ]
        );
        // Their waiting patterns come with their family, the same for any profile of it.
        assert!(builtin.iter().all(|p| p.waiting_patterns.is_empty()));
        // opencode's plugin reports its prompts, so its pane is matched only for a draft in shell
        // mode (watch_tests: an_opencode_draft_in_shell_mode_is_waiting). pi's trust question is
        // matched on screen too (pi_trust_screen_is_waiting), and so is Claude's, which comes
        // before any of its hooks (claude_folder_trust_dialog_is_waiting), and a Claude draft in
        // shell mode (a_claude_draft_in_shell_mode_is_waiting).
        let family = crate::family_patterns::family_patterns;
        assert!(!family(Hooks::Claude).tail.is_empty());
        assert!(!family(Hooks::Codex).tail.is_empty());
        assert!(!family(Hooks::Codex).screen.is_empty());
        assert!(!family(Hooks::Pi).tail.is_empty());
        assert!(!family(Hooks::Opencode).tail.is_empty());
        let none = family(Hooks::None);
        assert!(none.tail.is_empty() && none.screen.is_empty());
    }
    #[test]
    fn old_hook_values_still_parse() {
        for (value, hooks) in [
            ("claude", Hooks::Claude),
            ("codex", Hooks::Codex),
            ("none", Hooks::None),
            ("opencode", Hooks::Opencode),
            ("pi", Hooks::Pi),
        ] {
            let config = config(&format!(
                "[[agents.profiles]]\nid='x'\nlabel='X'\ncommand='x'\nhooks='{value}'\n"
            ));
            let (profiles, problems) = configured_lenient(&config);
            assert!(problems.is_empty(), "{value}: {problems:?}");
            assert_eq!(profiles[0].hooks, hooks, "{value}");
            assert_eq!(configured(&config).expect("strict")[0].hooks, hooks);
            assert_eq!(
                serde_json::to_string(&hooks).expect("wire name"),
                format!("\"{value}\"")
            );
        }
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
/// The waiting patterns of each profile, by id: those of its tool family, then its own
/// `waiting_patterns`. A profile's own patterns add to its family's and never replace them.
pub(crate) fn compiled(
    profiles: &crate::model::AgentProfiles,
) -> Result<std::collections::HashMap<String, WaitingPatterns>, ProfileError> {
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
            let regex = |text: &str| {
                regex::Regex::new(text).map_err(|source| ProfileError::WaitingPattern {
                    profile: p.id.clone(),
                    source,
                })
            };
            let family = family_patterns(p.hooks);
            let own = p.waiting_patterns.iter().map(String::as_str);
            let patterns = WaitingPatterns {
                tail: family
                    .tail
                    .iter()
                    .copied()
                    .chain(own)
                    .map(regex)
                    .collect::<Result<_, _>>()?,
                screen: family
                    .screen
                    .iter()
                    .map(|text| regex(text))
                    .collect::<Result<_, _>>()?,
            };
            Ok((p.id.clone(), patterns))
        })
        .collect()
}
