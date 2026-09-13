//! The single suite shortcut owner's bounded configuration; no arbitrary key
//! or command binding can cross the product connection boundary.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub accelerator: String,
    pub enabled: bool,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub capture: bool,
    #[serde(default)]
    pub project: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            accelerator: "Ctrl+Alt+Space".into(),
            enabled: false,
            terminal: false,
            capture: false,
            project: false,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !["Ctrl+Alt+Space", "Ctrl+Alt+L", "Ctrl+Alt+J"].contains(&self.accelerator.as_str()) {
            return Err("shortcut_invalid".into());
        }
        Ok(())
    }
    pub fn bindings(&self) -> Vec<(String, String)> {
        if !self.enabled {
            return Vec::new();
        }
        let mut bindings = vec![("control-center.launcher".into(), self.accelerator.clone())];
        for (enabled, command, key) in [
            (self.terminal, "workspace.summon-terminal", "Ctrl+Alt+T"),
            (self.capture, "knowledge.quick-capture", "Ctrl+Alt+N"),
            (self.project, "workspace.open-current-project", "Ctrl+Alt+P"),
        ] {
            if enabled {
                bindings.push((command.into(), key.into()));
            }
        }
        bindings
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_bindings_are_unique_and_never_reserve_sigint() {
        let config = Config {
            enabled: true,
            terminal: true,
            capture: true,
            project: true,
            ..Default::default()
        };
        config.validate().unwrap();
        let bindings = config.bindings();
        assert_eq!(bindings.len(), 4);
        assert_eq!(
            bindings
                .iter()
                .map(|(_, key)| key)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
        assert!(!bindings.iter().any(|(_, key)| key == "Ctrl+C"));
        assert!(Config {
            accelerator: "Ctrl+C".into(),
            ..config
        }
        .validate()
        .is_err());
    }
}
