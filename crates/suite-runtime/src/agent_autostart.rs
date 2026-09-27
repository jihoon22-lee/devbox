//! One per-user Run entry, with installation ownership and resumable changes.
use serde::{Deserialize, Serialize};
pub const AGENT_VALUE: &str = "Devbox Agent";
type Result<T> = std::result::Result<T, &'static str>;
pub trait RunKey {
    fn read(&self, name: &str) -> Result<Option<String>>;
    fn write(&mut self, name: &str, value: Option<&str>) -> Result<()>;
}
#[derive(Clone, Debug)]
pub struct Owner {
    root: String,
    namespace: String,
    executable: String,
}
impl Owner {
    pub fn new(root: &str, namespace: &str, executable: &str) -> Result<Self> {
        let root = normalized(root).ok_or("agent_autostart_invalid")?;
        if root.len() < 3
            || !root.as_bytes()[0].is_ascii_alphabetic()
            || &root.as_bytes()[1..3] != b":/"
        {
            return Err("agent_autostart_invalid");
        }
        if namespace.len() != 64
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("agent_autostart_invalid");
        }
        let owner = Self {
            root,
            namespace: namespace.into(),
            executable: executable.into(),
        };
        if !owner.owns_path(executable, AGENT_SUFFIX) {
            return Err("agent_autostart_invalid");
        }
        Ok(owner)
    }
    pub fn legacy_name(&self) -> String {
        format!("DevboxKnowledge-{}", self.namespace)
    }
    pub fn command(&self) -> String {
        format!("\"{}\" --autostart", self.executable)
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Values {
    pub agent: Option<String>,
    pub legacy: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub before: Values,
    pub after: Values,
}
pub fn snapshot(key: &impl RunKey, owner: &Owner) -> Result<Values> {
    Ok(Values {
        agent: key.read(AGENT_VALUE)?,
        legacy: key.read(&owner.legacy_name())?,
    })
}
const AGENT_SUFFIX: &str = "/products/control-center/resources/suite/devbox-agent.exe";
const LEGACY_SUFFIX: &str = "/products/knowledge/devbox-knowledge.exe";
fn normalized(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().any(|c| c.is_control() || c == '"') {
        return None;
    }
    let value = value.replace('\\', "/").to_ascii_lowercase();
    let value = value
        .strip_prefix("//?/")
        .unwrap_or(&value)
        .trim_end_matches('/');
    if value.split('/').any(|part| part == "." || part == "..") {
        return None;
    }
    Some(value.into())
}
impl Owner {
    fn owns_path(&self, path: &str, suffix: &str) -> bool {
        let Some(path) = normalized(path) else {
            return false;
        };
        let prefix = format!("{}/generations/", self.root);
        let Some(generation) = path
            .strip_prefix(&prefix)
            .and_then(|p| p.strip_suffix(suffix))
        else {
            return false;
        };
        !generation.is_empty()
            && generation
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    }
    fn owns(&self, command: &str, legacy: bool) -> bool {
        let Some(command) = command.strip_prefix('"') else {
            return false;
        };
        let Some((path, args)) = command.split_once('"') else {
            return false;
        };
        self.owns_path(path, if legacy { LEGACY_SUFFIX } else { AGENT_SUFFIX })
            && if legacy {
                args.is_empty() || args == " --background"
            } else {
                args == " --autostart"
            }
    }
    pub fn owns_workspace_shortcut(&self, path: &str, args: &str, description: &str) -> bool {
        args == "--background"
            && description == "Run Manager background scheduler"
            && self.owns_path(path, "/products/workspace/devbox-workspace.exe")
    }
    pub fn enabled(&self, values: &Values) -> bool {
        values.agent.as_deref().is_some_and(|v| self.owns(v, false))
    }
}
pub fn reconcile(owner: &Owner, before: Values) -> Result<Option<Change>> {
    if before
        .agent
        .as_deref()
        .is_some_and(|v| !owner.owns(v, false))
    {
        return Ok(None);
    }
    if !owner.enabled(&before)
        && !before
            .legacy
            .as_deref()
            .is_some_and(|v| owner.owns(v, true))
    {
        return Ok(None);
    }
    let change = setting(owner, before, true)?;
    Ok((change.before != change.after).then_some(change))
}
pub fn setting(owner: &Owner, before: Values, enabled: bool) -> Result<Change> {
    let mut after = before.clone();
    if enabled {
        if before
            .agent
            .as_deref()
            .is_some_and(|v| !owner.owns(v, false))
        {
            return Err("agent_autostart_conflict");
        }
        after.agent = Some(owner.command());
    } else if owner.enabled(&before) {
        after.agent = None;
    }
    if before
        .legacy
        .as_deref()
        .is_some_and(|v| owner.owns(v, true))
    {
        after.legacy = None;
    }
    Ok(Change { before, after })
}
pub fn apply(key: &mut impl RunKey, owner: &Owner, change: &Change) -> Result<()> {
    // Validate persisted changes as well as freshly calculated changes. Unchanged
    // foreign entries can coexist, but may never be rewritten or removed.
    for (before, after, legacy) in [
        (&change.before.agent, &change.after.agent, false),
        (&change.before.legacy, &change.after.legacy, true),
    ] {
        if before != after
            && [before, after]
                .iter()
                .any(|v| v.as_deref().is_some_and(|v| !owner.owns(v, legacy)))
        {
            return Err("agent_autostart_conflict");
        }
    }
    let observed = snapshot(key, owner)?;
    for (observed, before, after) in [
        (&observed.agent, &change.before.agent, &change.after.agent),
        (
            &observed.legacy,
            &change.before.legacy,
            &change.after.legacy,
        ),
    ] {
        if observed != before && observed != after {
            return Err("agent_autostart_conflict");
        }
    }
    let legacy_name = owner.legacy_name();
    let agent = (AGENT_VALUE, &change.before.agent, &change.after.agent);
    let legacy = (
        legacy_name.as_str(),
        &change.before.legacy,
        &change.after.legacy,
    );
    // Install the replacement before removing the predecessor, including rollback.
    let slots = if change.after.agent.is_some() {
        [agent, legacy]
    } else {
        [legacy, agent]
    };
    for (name, before, after) in slots {
        if before == after {
            continue;
        }
        let current = key.read(name)?;
        if current == *after {
            continue;
        }
        if current != *before {
            return Err("agent_autostart_conflict");
        }
        key.write(name, after.as_deref())?;
    }
    if snapshot(key, owner)? != change.after {
        return Err("agent_autostart_conflict");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Fake {
        values: BTreeMap<String, String>,
        fail_legacy: bool,
    }
    impl RunKey for Fake {
        fn read(&self, name: &str) -> Result<Option<String>> {
            Ok(self.values.get(name).cloned())
        }
        fn write(&mut self, name: &str, value: Option<&str>) -> Result<()> {
            if self.fail_legacy && name.starts_with("DevboxKnowledge-") {
                return Err("agent_autostart_unavailable");
            }
            match value {
                Some(value) => {
                    self.values.insert(name.into(), value.into());
                }
                None => {
                    self.values.remove(name);
                }
            }
            Ok(())
        }
    }
    fn owner(generation: &str) -> Owner {
        Owner::new(r"C:\suite", &"a".repeat(64), &format!(r"C:\suite\generations\{generation}\products\control-center\resources\suite\devbox-agent.exe")).unwrap()
    }
    #[test]
    fn ownership_rejects_relative_roots_sibling_installations_and_extra_arguments() {
        assert!(Owner::new(
            "relative",
            &"a".repeat(64),
            "relative/generations/g/products/control-center/resources/suite/devbox-agent.exe"
        )
        .is_err());
        let owner = owner("next");
        let description = "Run Manager background scheduler";
        assert!(owner.owns_workspace_shortcut(
            r"C:\suite\generations\old\products\workspace\devbox-workspace.exe",
            "--background",
            description
        ));
        assert!(!owner.owns_workspace_shortcut(
            r"C:\suite-other\generations\old\products\workspace\devbox-workspace.exe",
            "--background",
            description
        ));
        assert!(!owner.owns_workspace_shortcut(
            r"C:\suite\generations\old\products\workspace\devbox-workspace.exe",
            "--background --open",
            description
        ));
        let before = Values {
            agent: Some(format!("{} --extra", owner.command())),
            legacy: None,
        };
        assert!(setting(&owner, before, true).is_err());
    }
    #[test]
    fn enabling_migrates_only_our_legacy_entry_and_disabling_removes_only_our_agent() {
        let owner = owner("next");
        let mut key = Fake::default();
        key.values.insert(
            owner.legacy_name(),
            r#""C:\suite\generations\old\products\knowledge\devbox-knowledge.exe""#.into(),
        );
        key.values.insert("Unrelated".into(), "keep".into());
        let change = setting(&owner, snapshot(&key, &owner).unwrap(), true).unwrap();
        apply(&mut key, &owner, &change).unwrap();
        assert_eq!(key.values.get(AGENT_VALUE), Some(&owner.command()));
        assert!(!key.values.contains_key(&owner.legacy_name()));
        let change = setting(&owner, snapshot(&key, &owner).unwrap(), false).unwrap();
        apply(&mut key, &owner, &change).unwrap();
        assert_eq!(
            key.values,
            BTreeMap::from([("Unrelated".into(), "keep".into())])
        );
    }
    #[test]
    fn startup_defaults_off_and_refreshes_an_enabled_generation_without_touching_foreign_values() {
        let owner = owner("next");
        assert!(reconcile(&owner, Values::default()).unwrap().is_none());
        let previous = Values {
            agent: Some(self::owner("old").command()),
            legacy: None,
        };
        let change = reconcile(&owner, previous.clone()).unwrap().unwrap();
        assert_eq!(change.after.agent, Some(owner.command()));
        assert_eq!(change.before, previous);
        let foreign = Values {
            agent: Some(r#""C:\other\devbox-agent.exe" --autostart"#.into()),
            legacy: None,
        };
        assert!(setting(&owner, foreign.clone(), true).is_err());
        assert!(reconcile(&owner, foreign.clone()).unwrap().is_none());
        assert_eq!(
            setting(&owner, foreign.clone(), false).unwrap().after,
            foreign
        );
    }
    #[test]
    fn partial_migration_resumes_and_reverse_change_restores_the_original_preference() {
        let owner = owner("next");
        let mut key = Fake::default();
        key.values.insert(
            owner.legacy_name(),
            r#""C:\suite\generations\old\products\knowledge\devbox-knowledge.exe""#.into(),
        );
        let original = snapshot(&key, &owner).unwrap();
        let change = reconcile(&owner, original.clone()).unwrap().unwrap();
        key.fail_legacy = true;
        assert!(apply(&mut key, &owner, &change).is_err());
        assert_eq!(key.values.get(AGENT_VALUE), Some(&owner.command()));
        assert!(key.values.contains_key(&owner.legacy_name()));
        key.fail_legacy = false;
        apply(&mut key, &owner, &change).unwrap();
        apply(
            &mut key,
            &owner,
            &Change {
                before: change.after,
                after: change.before,
            },
        )
        .unwrap();
        assert_eq!(snapshot(&key, &owner).unwrap(), original);
        let change = setting(&owner, original, true).unwrap();
        key.values
            .insert(AGENT_VALUE.into(), "foreign change".into());
        assert!(apply(&mut key, &owner, &change).is_err());
        assert_eq!(key.values.get(AGENT_VALUE).unwrap(), "foreign change");
    }
}
