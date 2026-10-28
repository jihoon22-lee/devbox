// crates/agents/src/profile.rs
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

/// 기본 프로필. Codex 승인 화면은 0.162.1 S0a 관찰, 폴더 신뢰 화면은 0.164.0 실제 녹화, hook 검토 화면은
/// 0.164.0 실행 파일 문자열(실제 화면 아님)이다. 다른 UI 유형·버전은 실제 fixture로 보강한다.
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
                // Folder trust. Evidence: crates/agents/tests/fixtures/codex-0.164.0/trust.txt (real
                // capture). Codex wraps the question to the terminal width, which can push it out of
                // the matched tail, so this anchors on the first choice row, always among the last rows.
                r"(?im)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?Trust and continue\s*$".into(),
                // Hooks review. Evidence: crates/agents/tests/fixtures/codex-0.164.0/hooks-review.txt
                // (문자열 기반: sentences from the 0.164.0 binary, not a real capture). Either choice row
                // marks the prompt; "N hooks need review before they can run." is only a notice.
                r"(?im)^\s*(?:[›>]\s*)?(?:\d+[.)]\s+)?(?:Trust all and continue|Continue without trusting \(hooks won['’]t run\))".into(),
            ],
        },
        // Its plugin reports permission prompts, so its pane is not matched.
        ToolProfile {
            id: "opencode".into(),
            label: "opencode".into(),
            command: "opencode".into(),
            hooks: Hooks::Opencode,
            waiting_patterns: vec![],
        },
        // Its extension reports the folder trust question and extension dialogs. The trust
        // question is matched on screen as well: a send taken without the extension's ready
        // report, 30 s after the launch, must not answer it.
        ToolProfile {
            id: "pi".into(),
            label: "pi".into(),
            command: "pi".into(),
            hooks: Hooks::Pi,
            waiting_patterns: vec![
                // Folder trust. Evidence: crates/agents/tests/fixtures/pi-1.3.0/trust.txt (real
                // capture). The question, the folder rows and the explanation can leave the
                // matched tail, and `→` marks the selected choice, so this anchors on the last two
                // choice rows, with or without it.
                r"(?im)^\s*(?:→\s*)?Do not trust\s*\n\s*(?:→\s*)?Do not trust \(this session only\)\s*$".into(),
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
        // opencode's plugin reports its prompts, so its pane is not matched. pi's trust question
        // is matched on screen too (watch_tests: pi_trust_screen_is_waiting).
        assert!(builtin[2].waiting_patterns.is_empty());
        assert!(!builtin[3].waiting_patterns.is_empty());
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
