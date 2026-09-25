//! Strict shareable project definitions and local overlays. Parsing and merging
//! do not run commands, inspect .env values, install tools or write files.
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskSourceKind {
    PackageScript,
    Taskfile,
    RunDefinition,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub kind: TaskSourceKind,
    pub source: String,
    pub selector: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Toolchain {
    pub tool: String,
    pub version: String,
    /// A relative checked-in configuration file, not an executable path.
    pub source: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub tasks: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    #[serde(default)]
    pub tasks: BTreeMap<String, Task>,
    #[serde(default)]
    pub toolchains: BTreeMap<String, Toolchain>,
    #[serde(default)]
    pub openapi_sources: BTreeMap<String, String>,
    #[serde(default)]
    pub log_sources: BTreeMap<String, String>,
    #[serde(default)]
    pub sessions: BTreeMap<String, Session>,
    #[serde(default)]
    pub expected_ports: Option<Vec<u16>>,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tasks: BTreeMap::new(),
            toolchains: BTreeMap::new(),
            openapi_sources: BTreeMap::new(),
            log_sources: BTreeMap::new(),
            sessions: BTreeMap::new(),
            expected_ports: None,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalOverlay {
    pub schema_version: u32,
    /// Resolves through the native registry; roots and distro names are not
    /// exported to the repository manifest.
    pub context: ProjectContext,
    #[serde(default)]
    pub tasks: BTreeMap<String, Option<Task>>,
    #[serde(default)]
    pub toolchains: BTreeMap<String, Option<Toolchain>>,
    #[serde(default)]
    pub openapi_sources: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub log_sources: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub sessions: BTreeMap<String, Option<Session>>,
    pub expected_ports: Option<Vec<u16>>,
    /// Owner-local references only; values are resolved by their existing owner
    /// at execution time, never exported with shareable definitions.
    #[serde(default)]
    pub secrets: Vec<product_contract::references::OwnedSecretReference>,
    pub api_environment_id: Option<String>,
    pub layout_id: Option<String>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionDiff {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub expected_ports_changed: bool,
}
fn key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}
pub fn relative_source(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value.contains(['\\', ':'])
        && !value.chars().any(char::is_control)
        && value.split('/').all(|part| {
            !part.is_empty()
                && !matches!(part, "." | "..")
                && part.trim() == part
                && !part.ends_with('.')
        })
}
fn ports(values: &[u16]) -> bool {
    values.len() <= MAX_ENTRIES
        && !values.contains(&0)
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
fn map_keys<T>(values: &BTreeMap<String, T>) -> bool {
    values.len() <= MAX_ENTRIES && values.keys().all(|s| key(s))
}
fn task(value: &Task) -> bool {
    relative_source(&value.source) && key(&value.selector)
}
fn toolchain(value: &Toolchain) -> bool {
    key(&value.tool)
        && !value.version.is_empty()
        && value.version.len() <= 120
        && value
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".*^~<>=|-+ ".contains(&b))
        && value.source.as_ref().is_none_or(|s| relative_source(s))
}
impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err("manifest_limit");
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| "invalid_manifest")?;
        result.validate()?;
        Ok(result)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| "invalid_manifest")?;
        if bytes.len() > MAX_BYTES {
            return Err("manifest_limit");
        }
        Ok(bytes)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err("unsupported_manifest_version");
        }
        if !map_keys(&self.tasks)
            || !map_keys(&self.toolchains)
            || !map_keys(&self.openapi_sources)
            || !map_keys(&self.log_sources)
            || !map_keys(&self.sessions)
            || self.expected_ports.as_ref().is_some_and(|v| !ports(v))
            || !self.tasks.values().all(task)
            || !self.toolchains.values().all(toolchain)
            || !self
                .openapi_sources
                .values()
                .chain(self.log_sources.values())
                .all(|s| relative_source(s))
        {
            return Err("invalid_manifest_definition");
        }
        for session in self.sessions.values() {
            if session.tasks.len() > MAX_ENTRIES
                || session.tasks.iter().collect::<BTreeSet<_>>().len() != session.tasks.len()
                || !session.tasks.iter().all(|id| self.tasks.contains_key(id))
            {
                return Err("session_task_conflict");
            }
        }
        Ok(())
    }
    /// Hash the exact effective execution definitions plus every relevant
    /// source digest. The native owner obtains those digests using bounded,
    /// identity-checked reads. Renderer digests are never approval evidence.
    pub fn execution_digest(&self, source_digests: &BTreeMap<String, String>) -> Result<String> {
        self.validate()?;
        let sources: BTreeSet<_> = self
            .tasks
            .values()
            .map(|t| &t.source)
            .chain(self.toolchains.values().filter_map(|t| t.source.as_ref()))
            .collect();
        if sources.len() != source_digests.len()
            || sources.iter().any(|s| !source_digests.contains_key(*s))
            || !source_digests.values().all(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
        {
            return Err("execution_source_snapshot_required");
        }
        let bytes = serde_json::to_vec(&(
            1_u32,
            &self.tasks,
            &self.toolchains,
            &self.sessions,
            source_digests,
        ))
        .map_err(|_| "invalid_manifest")?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    pub fn diff(&self, next: &Self) -> Result<DefinitionDiff> {
        self.validate()?;
        next.validate()?;
        let flatten = |manifest: &Self| -> Result<BTreeMap<String, serde_json::Value>> {
            let value = serde_json::to_value(manifest).map_err(|_| "invalid_manifest")?;
            let mut fields = BTreeMap::new();
            for group in [
                "tasks",
                "toolchains",
                "openapiSources",
                "logSources",
                "sessions",
            ] {
                for (id, value) in value[group].as_object().ok_or("invalid_manifest")? {
                    fields.insert(format!("{group}/{id}"), value.clone());
                }
            }
            Ok(fields)
        };
        let before = flatten(self)?;
        let after = flatten(next)?;
        Ok(DefinitionDiff {
            added: after
                .keys()
                .filter(|k| !before.contains_key(*k))
                .cloned()
                .collect(),
            changed: after
                .iter()
                .filter(|(k, v)| before.get(*k).is_some_and(|old| old != *v))
                .map(|(k, _)| k.clone())
                .collect(),
            removed: before
                .keys()
                .filter(|k| !after.contains_key(*k))
                .cloned()
                .collect(),
            expected_ports_changed: self.expected_ports != next.expected_ports,
        })
    }
}
impl LocalOverlay {
    pub fn empty(context: ProjectContext) -> Self {
        Self {
            schema_version: 1,
            context,
            tasks: BTreeMap::new(),
            toolchains: BTreeMap::new(),
            openapi_sources: BTreeMap::new(),
            log_sources: BTreeMap::new(),
            sessions: BTreeMap::new(),
            expected_ports: None,
            secrets: Vec::new(),
            api_environment_id: None,
            layout_id: None,
        }
    }
    pub fn parse(bytes: &[u8], expected: &ProjectContext) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err("overlay_limit");
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| "invalid_overlay")?;
        result.validate(expected)?;
        Ok(result)
    }
    pub fn validate(&self, expected: &ProjectContext) -> Result<()> {
        if self.schema_version != 1 {
            return Err("unsupported_overlay_version");
        }
        self.context.validate().map_err(|_| "invalid_context")?;
        if &self.context != expected {
            return Err("stale_overlay_binding");
        }
        if !map_keys(&self.tasks)
            || !map_keys(&self.toolchains)
            || !map_keys(&self.openapi_sources)
            || !map_keys(&self.log_sources)
            || !map_keys(&self.sessions)
            || !self.tasks.values().flatten().all(task)
            || !self.toolchains.values().flatten().all(toolchain)
            || !self
                .openapi_sources
                .values()
                .chain(self.log_sources.values())
                .flatten()
                .all(|s| relative_source(s))
            || self.expected_ports.as_ref().is_some_and(|v| !ports(v))
            || self.sessions.values().flatten().any(|s| {
                s.tasks.len() > MAX_ENTRIES
                    || s.tasks.iter().any(|t| !key(t))
                    || s.tasks.iter().collect::<BTreeSet<_>>().len() != s.tasks.len()
            })
            || self.api_environment_id.as_ref().is_some_and(|v| !key(v))
            || self.layout_id.as_ref().is_some_and(|v| !key(v))
            || self.secrets.len() > MAX_ENTRIES
        {
            return Err("invalid_overlay_definition");
        }
        let mut secret_names = BTreeSet::new();
        for secret in &self.secrets {
            secret
                .validate_for(product_contract::references::SecretOwner::ProjectEnvironment)
                .map_err(|_| "invalid_secret_reference")?;
            if !secret_names.insert(&secret.reference.name) {
                return Err("duplicate_secret_reference");
            }
        }
        Ok(())
    }
}
/// Whole named entries replace previous values. Local null removes an entry;
/// omitted maps leave it intact. Ports replace the complete list. Deleting a
/// task still referenced by a session is an explicit conflict, never truncation.
pub fn effective(
    defaults: &Manifest,
    project: &Manifest,
    local: &LocalOverlay,
    context: &ProjectContext,
) -> Result<Manifest> {
    defaults.validate()?;
    project.validate()?;
    local.validate(context)?;
    fn merge<T: Clone>(
        base: &mut BTreeMap<String, T>,
        project: &BTreeMap<String, T>,
        overlay: &BTreeMap<String, Option<T>>,
    ) {
        base.extend(project.clone());
        for (key, value) in overlay {
            if let Some(value) = value {
                base.insert(key.clone(), value.clone());
            } else {
                base.remove(key);
            }
        }
    }
    let mut result = defaults.clone();
    merge(&mut result.tasks, &project.tasks, &local.tasks);
    merge(
        &mut result.toolchains,
        &project.toolchains,
        &local.toolchains,
    );
    merge(
        &mut result.openapi_sources,
        &project.openapi_sources,
        &local.openapi_sources,
    );
    merge(
        &mut result.log_sources,
        &project.log_sources,
        &local.log_sources,
    );
    merge(&mut result.sessions, &project.sessions, &local.sessions);
    result.expected_ports = local
        .expected_ports
        .clone()
        .or_else(|| project.expected_ports.clone())
        .or_else(|| defaults.expected_ports.clone());
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> ProjectContext {
        ProjectContext {
            project_id: "project-a".into(),
            worktree_id: "tree-a".into(),
            target: product_contract::ExecutionTarget::Windows,
            revision: 2,
        }
    }
    fn overlay() -> LocalOverlay {
        LocalOverlay {
            schema_version: 1,
            context: context(),
            tasks: BTreeMap::new(),
            toolchains: BTreeMap::new(),
            openapi_sources: BTreeMap::new(),
            log_sources: BTreeMap::new(),
            sessions: BTreeMap::new(),
            expected_ports: None,
            secrets: vec![],
            api_environment_id: None,
            layout_id: None,
        }
    }
    fn task(selector: &str) -> Task {
        Task {
            kind: TaskSourceKind::PackageScript,
            source: "package.json".into(),
            selector: selector.into(),
        }
    }
    fn project() -> Manifest {
        let mut project = Manifest::default();
        project.tasks.insert("dev".into(), task("dev"));
        project
    }
    #[test]
    fn shareable_schema_rejects_private_values_unknown_fields_and_unsafe_sources() {
        let base = project();
        assert_eq!(Manifest::parse(&base.encode().unwrap()).unwrap(), base);
        for extra in [
            "secret",
            "environment",
            "absoluteRoot",
            "distro",
            "layout",
            "command",
        ] {
            let mut value = serde_json::to_value(&base).unwrap();
            value[extra] = "not allowed".into();
            assert!(
                Manifest::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{extra}"
            );
        }
        for source in [
            "../outside",
            "/absolute",
            "C:/local",
            r"\\wsl$\Ubuntu\root",
            "nested/../outside",
            "a//b",
            "a/./b",
            "file:stream",
            "a/ b",
        ] {
            let mut bad = base.clone();
            bad.tasks.get_mut("dev").unwrap().source = source.into();
            assert!(bad.validate().is_err(), "{source}");
        }
        assert!(Manifest::parse(br#"{"schemaVersion":2}"#).is_err());
        assert!(Manifest::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
    #[test]
    fn precedence_preserves_omitted_fields_and_explicit_removal() {
        let mut defaults = project();
        defaults.tasks.insert("test".into(), task("test"));
        defaults.expected_ports = Some(vec![3000]);
        let mut project = project();
        project.tasks.insert("dev".into(), task("start"));
        let mut local = overlay();
        local.tasks.insert("test".into(), None);
        let result = effective(&defaults, &project, &local, &context()).unwrap();
        assert_eq!(result.tasks["dev"].selector, "start");
        assert!(!result.tasks.contains_key("test"));
        assert_eq!(result.expected_ports, Some(vec![3000]));
        local.tasks.insert("dev".into(), Some(task("local")));
        local.expected_ports = Some(vec![]);
        let result = effective(&defaults, &project, &local, &context()).unwrap();
        assert_eq!(result.tasks["dev"].selector, "local");
        assert_eq!(result.expected_ports, Some(vec![]));
    }
    #[test]
    fn deleting_a_session_dependency_reports_conflict_and_keeps_sources() {
        let mut project = project();
        project.sessions.insert(
            "daily".into(),
            Session {
                tasks: vec!["dev".into()],
            },
        );
        let before = project.clone();
        let mut local = overlay();
        local.tasks.insert("dev".into(), None);
        assert_eq!(
            effective(&Manifest::default(), &project, &local, &context()),
            Err("session_task_conflict")
        );
        assert_eq!(project, before);
        local.sessions.insert("daily".into(), None);
        assert!(
            effective(&Manifest::default(), &project, &local, &context())
                .unwrap()
                .tasks
                .is_empty()
        );
    }
    #[test]
    fn trust_digest_requires_all_exact_sources_and_changes_with_execution_definitions() {
        let original = project();
        let mut hashes = BTreeMap::from([("package.json".into(), "a".repeat(64))]);
        let first = original.execution_digest(&hashes).unwrap();
        assert_eq!(
            first,
            Manifest::parse(&original.encode().unwrap())
                .unwrap()
                .execution_digest(&hashes)
                .unwrap()
        );
        let mut changed = original.clone();
        changed.tasks.get_mut("dev").unwrap().selector = "start".into();
        assert_ne!(first, changed.execution_digest(&hashes).unwrap());
        hashes.insert("package.json".into(), "b".repeat(64));
        assert_ne!(first, original.execution_digest(&hashes).unwrap());
        hashes.insert("unreviewed.json".into(), "c".repeat(64));
        assert!(original.execution_digest(&hashes).is_err());
        assert!(original.execution_digest(&BTreeMap::new()).is_err());
    }
    #[test]
    fn overlay_is_context_bound_and_rejects_plaintext_secret_fields() {
        let value = overlay();
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(LocalOverlay::parse(&bytes, &context()).unwrap(), value);
        let mut other = context();
        other.revision += 1;
        assert_eq!(
            LocalOverlay::parse(&bytes, &other),
            Err("stale_overlay_binding")
        );
        let mut bad = serde_json::to_value(&value).unwrap();
        bad["secretValues"] = serde_json::json!({"TOKEN":"synthetic only"});
        assert!(LocalOverlay::parse(&serde_json::to_vec(&bad).unwrap(), &context()).is_err());
    }
    #[test]
    fn diff_reports_add_change_remove_and_port_change_without_writing() {
        let original = project();
        let mut changed = original.clone();
        changed.tasks.insert("dev".into(), task("start"));
        changed
            .log_sources
            .insert("server".into(), "logs/server.log".into());
        changed.expected_ports = Some(vec![8080]);
        assert_eq!(
            original.diff(&changed).unwrap(),
            DefinitionDiff {
                added: vec!["logSources/server".into()],
                changed: vec!["tasks/dev".into()],
                removed: vec![],
                expected_ports_changed: true
            }
        );
        changed.tasks.clear();
        assert_eq!(original.diff(&changed).unwrap().removed, ["tasks/dev"]);
    }
}
