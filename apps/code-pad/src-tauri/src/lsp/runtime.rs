//! Local/custom runtime resolution and the process-launch security boundary.
//!
//! This module resolves only already-installed local files.  It never downloads
//! or installs a server and never invokes a shell to discover a runtime.  The
//! resulting `ResolvedProcess` contains canonical paths, a canonical workspace
//! cwd, exact argv values, and an explicitly allowlisted environment suitable
//! for `LspProcess::spawn`.

use super::catalog::{CustomServer, RuntimeKind, RuntimeSpec, ServerManifest, ServerRef};
use super::process::ProcessSpec;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const RUNTIME_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const RUNTIME_PROBE_OUTPUT_LIMIT: usize = 8 * 1024;
const RUNTIME_PROBE_REAP_TIMEOUT: Duration = Duration::from_secs(1);

/// Environment values that may cross the child-process boundary.
///
/// The allowlist starts empty.  `system()` copies only `PATH`; all other
/// values require an explicit `allow` call.  In particular, this type never
/// snapshots the parent environment wholesale, so secrets such as tokens and
/// credentials cannot be forwarded accidentally.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentAllowlist {
    values: BTreeMap<OsString, OsString>,
}

impl EnvironmentAllowlist {
    pub fn new() -> Self {
        Self::default()
    }

    /// Copy the system `PATH` only.  Missing PATH is valid; a resolver will
    /// report a program-not-found error if it needs PATH lookup.
    pub fn system() -> Self {
        let mut allowlist = Self::new();
        if let Some(path) = std::env::var_os("PATH") {
            // PATH is a well-formed environment key and cannot fail here.
            let _ = allowlist.insert("PATH", path);
        }
        allowlist
    }

    pub fn with_path(path: impl Into<OsString>) -> Self {
        let mut allowlist = Self::new();
        let _ = allowlist.insert("PATH", path.into());
        allowlist
    }

    /// Add one explicitly approved environment variable.
    pub fn allow(
        mut self,
        key: impl Into<OsString>,
        value: impl Into<OsString>,
    ) -> Result<Self, RuntimeError> {
        self.insert(key, value)?;
        Ok(self)
    }

    pub fn insert(
        &mut self,
        key: impl Into<OsString>,
        value: impl Into<OsString>,
    ) -> Result<(), RuntimeError> {
        let key = key.into();
        let value = value.into();
        validate_environment_component("environment key", &key)?;
        validate_environment_component("environment value", &value)?;
        self.values.insert(key, value);
        Ok(())
    }

    pub fn get(&self, key: &OsStr) -> Option<&OsString> {
        self.values.get(key)
    }

    pub fn contains_key(&self, key: &OsStr) -> bool {
        self.values.contains_key(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&OsString, &OsString)> {
        self.values.iter()
    }

    pub fn as_map(&self) -> &BTreeMap<OsString, OsString> {
        &self.values
    }

    pub fn into_map(self) -> BTreeMap<OsString, OsString> {
        self.values
    }
}

fn validate_environment_component(field: &str, value: &OsStr) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.to_string_lossy().chars().any(|character| {
            character.is_control()
                || character == '\0'
                || (field == "environment key" && character == '=')
        })
    {
        return Err(RuntimeError::InvalidSpec(format!(
            "{field} must be a non-empty environment value without control characters"
        )));
    }
    Ok(())
}

/// A parsed runtime version consisting of numeric dot-separated components.
/// Node's leading `v` prefix is accepted, but no command is executed to obtain
/// the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeVersion {
    components: Vec<u64>,
}

impl RuntimeVersion {
    pub fn parse(value: &str) -> Result<Self, RuntimeError> {
        let value = value.trim();
        let value = value.strip_prefix('v').unwrap_or(value);
        if value.is_empty() || value.contains(char::is_whitespace) {
            return Err(RuntimeError::InvalidVersion {
                value: value.to_owned(),
                reason: "version must contain numeric dot-separated components".into(),
            });
        }
        let mut components = Vec::new();
        for component in value.split('.') {
            if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(RuntimeError::InvalidVersion {
                    value: value.to_owned(),
                    reason: "version must contain numeric dot-separated components".into(),
                });
            }
            components.push(component.parse::<u64>().map_err(|_| {
                RuntimeError::InvalidVersion {
                    value: value.to_owned(),
                    reason: "version component is too large".into(),
                }
            })?);
        }
        Ok(Self { components })
    }

    pub fn components(&self) -> &[u64] {
        &self.components
    }

    fn compare(&self, other: &Self) -> Ordering {
        let length = self.components.len().max(other.components.len());
        for index in 0..length {
            let left = self.components.get(index).copied().unwrap_or(0);
            let right = other.components.get(index).copied().unwrap_or(0);
            match left.cmp(&right) {
                Ordering::Equal => continue,
                ordering => return ordering,
            }
        }
        Ordering::Equal
    }
}

/// Operators accepted by a user/runtime version requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOperator {
    Exact,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Compatible,
    Tilde,
}

/// A parsed, non-shell version requirement such as `>=20` or `~20.11`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRequirement {
    operator: VersionOperator,
    version: RuntimeVersion,
}

impl VersionRequirement {
    pub fn parse(value: &str) -> Result<Self, RuntimeError> {
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(RuntimeError::InvalidVersionRequirement(value.to_owned()));
        }
        let (operator, version) = if let Some(version) = value.strip_prefix(">=") {
            (VersionOperator::GreaterOrEqual, version)
        } else if let Some(version) = value.strip_prefix("<=") {
            (VersionOperator::LessOrEqual, version)
        } else if let Some(version) = value.strip_prefix('>') {
            (VersionOperator::Greater, version)
        } else if let Some(version) = value.strip_prefix('<') {
            (VersionOperator::Less, version)
        } else if let Some(version) = value.strip_prefix('^') {
            (VersionOperator::Compatible, version)
        } else if let Some(version) = value.strip_prefix('~') {
            (VersionOperator::Tilde, version)
        } else if let Some(version) = value.strip_prefix('=') {
            (VersionOperator::Exact, version)
        } else {
            (VersionOperator::Exact, value)
        };
        if version.is_empty() || version.contains('*') {
            return Err(RuntimeError::InvalidVersionRequirement(value.to_owned()));
        }
        Ok(Self {
            operator,
            version: RuntimeVersion::parse(version)?,
        })
    }

    pub fn operator(&self) -> VersionOperator {
        self.operator
    }

    pub fn version(&self) -> &RuntimeVersion {
        &self.version
    }

    pub fn matches(&self, actual: &RuntimeVersion) -> bool {
        match self.operator {
            VersionOperator::Exact => self.version.compare(actual) == Ordering::Equal,
            VersionOperator::Greater => actual.compare(&self.version) == Ordering::Greater,
            VersionOperator::GreaterOrEqual => actual.compare(&self.version) != Ordering::Less,
            VersionOperator::Less => actual.compare(&self.version) == Ordering::Less,
            VersionOperator::LessOrEqual => actual.compare(&self.version) != Ordering::Greater,
            VersionOperator::Compatible => {
                actual.compare(&self.version) != Ordering::Less
                    && actual.compare(&self.compatible_upper_bound()) == Ordering::Less
            }
            VersionOperator::Tilde => {
                actual.compare(&self.version) != Ordering::Less
                    && actual.compare(&self.tilde_upper_bound()) == Ordering::Less
            }
        }
    }

    pub fn matches_str(&self, actual: &str) -> Result<bool, RuntimeError> {
        Ok(self.matches(&RuntimeVersion::parse(actual)?))
    }

    fn compatible_upper_bound(&self) -> RuntimeVersion {
        let major = self.version.components.first().copied().unwrap_or(0);
        let minor = self.version.components.get(1).copied().unwrap_or(0);
        let patch = self.version.components.get(2).copied().unwrap_or(0);
        let components = if major > 0 {
            vec![major + 1]
        } else if minor > 0 {
            vec![0, minor + 1]
        } else {
            vec![0, 0, patch + 1]
        };
        RuntimeVersion { components }
    }

    fn tilde_upper_bound(&self) -> RuntimeVersion {
        let major = self.version.components.first().copied().unwrap_or(0);
        if self.version.components.len() == 1 {
            RuntimeVersion {
                components: vec![major + 1],
            }
        } else {
            let minor = self.version.components.get(1).copied().unwrap_or(0);
            RuntimeVersion {
                components: vec![major, minor + 1],
            }
        }
    }
}

/// A runtime path resolved to an existing regular file. Version checks for
/// user-defined runtimes remain explicit and consume a caller-provided
/// version string. Managed Node servers use the bounded direct probe below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntime {
    pub kind: RuntimeKind,
    pub executable: PathBuf,
    pub version_requirement: Option<VersionRequirement>,
}

impl ResolvedRuntime {
    pub fn validate_reported_version(&self, actual: &str) -> Result<(), RuntimeError> {
        let Some(requirement) = &self.version_requirement else {
            return Ok(());
        };
        if requirement.matches_str(actual)? {
            Ok(())
        } else {
            Err(RuntimeError::RuntimeVersionMismatch {
                requirement: format_requirement(requirement),
                actual: actual.trim().to_owned(),
            })
        }
    }
}

fn format_requirement(requirement: &VersionRequirement) -> String {
    let operator = match requirement.operator {
        VersionOperator::Exact => "=",
        VersionOperator::Greater => ">",
        VersionOperator::GreaterOrEqual => ">=",
        VersionOperator::Less => "<",
        VersionOperator::LessOrEqual => "<=",
        VersionOperator::Compatible => "^",
        VersionOperator::Tilde => "~",
    };
    format!(
        "{operator}{}",
        requirement
            .version
            .components
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(".")
    )
}

/// A process command that is safe to pass directly to `Command::new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProcess {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub runtime: Option<ResolvedRuntime>,
}

impl ResolvedProcess {
    pub fn process_spec(&self) -> ProcessSpec {
        ProcessSpec {
            executable: self.executable.clone(),
            args: self.args.clone(),
            current_dir: self.current_dir.clone(),
            env: self.env.clone(),
        }
    }
}

/// Resolves local paths and system PATH entries without executing anything.
#[derive(Debug, Clone)]
pub struct RuntimeResolver {
    search_path: Option<OsString>,
    environment: EnvironmentAllowlist,
}

impl Default for RuntimeResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeResolver {
    pub fn new() -> Self {
        let environment = EnvironmentAllowlist::system();
        let search_path = environment.get(OsStr::new("PATH")).cloned();
        Self {
            search_path,
            environment,
        }
    }

    /// Construct a deterministic resolver with exactly one PATH value.  This
    /// is useful for tests and for a caller that has already selected a PATH.
    pub fn with_path(path: impl Into<OsString>) -> Self {
        let environment = EnvironmentAllowlist::with_path(path);
        let search_path = environment.get(OsStr::new("PATH")).cloned();
        Self {
            search_path,
            environment,
        }
    }

    pub fn with_environment(mut self, environment: EnvironmentAllowlist) -> Self {
        self.search_path = environment.get(OsStr::new("PATH")).cloned();
        self.environment = environment;
        self
    }

    pub fn environment(&self) -> &EnvironmentAllowlist {
        &self.environment
    }

    pub fn canonical_workspace_root(path: impl AsRef<Path>) -> Result<PathBuf, RuntimeError> {
        canonical_workspace_root(path.as_ref())
    }

    /// Canonicalize a path and require that its real path remains under the
    /// workspace root.  Symlinks and `..` are resolved before comparison.
    pub fn resolve_workspace_path(
        workspace: impl AsRef<Path>,
        path: impl AsRef<Path>,
    ) -> Result<PathBuf, RuntimeError> {
        let workspace = canonical_workspace_root(workspace.as_ref())?;
        let raw = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            workspace.join(path.as_ref())
        };
        let canonical = canonical_file_or_directory(&raw, "workspace path")?;
        if !path_is_within(&workspace, &canonical) {
            return Err(RuntimeError::PathOutsideWorkspace {
                root: workspace,
                path: canonical,
            });
        }
        Ok(canonical)
    }

    pub fn resolve_runtime(&self, spec: &RuntimeSpec) -> Result<ResolvedRuntime, RuntimeError> {
        self.resolve_runtime_with_base(spec, None)
    }

    pub fn resolve_runtime_in_workspace(
        &self,
        spec: &RuntimeSpec,
        workspace: impl AsRef<Path>,
    ) -> Result<ResolvedRuntime, RuntimeError> {
        let workspace = canonical_workspace_root(workspace.as_ref())?;
        self.resolve_runtime_with_base(spec, Some(&workspace))
    }

    fn resolve_runtime_with_base(
        &self,
        spec: &RuntimeSpec,
        base: Option<&Path>,
    ) -> Result<ResolvedRuntime, RuntimeError> {
        spec.validate()
            .map_err(|error| RuntimeError::InvalidSpec(error.to_string()))?;
        let version_requirement = spec
            .min_version
            .as_deref()
            .map(VersionRequirement::parse)
            .transpose()?;
        let executable = self.resolve_executable(&spec.executable, base, "runtime.executable")?;
        Ok(ResolvedRuntime {
            kind: spec.kind,
            executable,
            version_requirement,
        })
    }

    /// Resolve a `local` selection.  `installed_path` may be a file, or a
    /// directory paired with a relative `executable`; relative children must
    /// remain below that directory after canonicalization.
    pub fn resolve_local(
        &self,
        server: &ServerRef,
        workspace: impl AsRef<Path>,
    ) -> Result<ResolvedProcess, RuntimeError> {
        let ServerRef::Local {
            installed_path,
            executable,
            args,
        } = server
        else {
            return Err(RuntimeError::InvalidSpec(
                "resolve_local requires a local server reference".into(),
            ));
        };
        let workspace = canonical_workspace_root(workspace.as_ref())?;
        let installed = canonical_file_or_directory(Path::new(installed_path), "installed_path")?;
        let args = validate_args("local.args", args)?;
        let executable_path = if let Some(executable) = executable {
            validate_argv_value("local.executable", executable)?;
            let relative = Path::new(executable);
            if relative.is_absolute() {
                self.canonical_executable(relative, "local.executable")?
            } else {
                let base = if installed.is_dir() {
                    installed.as_path()
                } else {
                    installed.parent().unwrap_or(installed.as_path())
                };
                let candidate = base.join(relative);
                let canonical = self.canonical_executable(&candidate, "local.executable")?;
                if !path_is_within(base, &canonical) {
                    return Err(RuntimeError::PathEscape {
                        base: base.to_path_buf(),
                        path: canonical,
                    });
                }
                canonical
            }
        } else {
            self.canonical_executable(&installed, "local.installed_path")?
        };
        Ok(ResolvedProcess {
            executable: executable_path,
            args,
            current_dir: workspace,
            env: self.environment.clone().into_map(),
            runtime: None,
        })
    }

    /// Resolve a custom server.  Native servers execute directly; Node
    /// servers execute through the resolved Node binary with the server path
    /// as argv[1].  Both paths must already exist as regular files.
    pub fn resolve_custom(
        &self,
        server: &CustomServer,
        workspace: impl AsRef<Path>,
    ) -> Result<ResolvedProcess, RuntimeError> {
        let workspace = canonical_workspace_root(workspace.as_ref())?;
        validate_argv_value("custom.executable", &server.executable)?;
        let custom_args = validate_args("custom.args", &server.args)?;
        let server_executable =
            self.resolve_custom_executable(&server.executable, &workspace, "custom.executable")?;
        let runtime = self.resolve_runtime_with_base(&server.runtime, Some(&workspace))?;
        let (executable, args) = match runtime.kind {
            RuntimeKind::Native => (server_executable, custom_args),
            RuntimeKind::Node => {
                let mut args = Vec::with_capacity(custom_args.len() + 1);
                args.push(server_executable.into_os_string());
                args.extend(custom_args);
                (runtime.executable.clone(), args)
            }
        };
        Ok(ResolvedProcess {
            executable,
            args,
            current_dir: workspace,
            env: self.environment.clone().into_map(),
            runtime: Some(runtime),
        })
    }

    /// Resolve one already-validated managed catalog entry. The installer
    /// supplies the canonical, process-owned install root; all command and
    /// server arguments come from the reviewed manifest. Node is invoked
    /// directly for a bounded `--version` probe before the process spec is
    /// returned, never through a shell.
    pub async fn resolve_managed(
        &self,
        manifest: &ServerManifest,
        language_id: &str,
        installed_root: impl AsRef<Path>,
        node_path: Option<&str>,
        workspace: impl AsRef<Path>,
    ) -> Result<ResolvedProcess, RuntimeError> {
        manifest
            .validate_for_install()
            .map_err(|error| RuntimeError::InvalidSpec(error.to_string()))?;
        let workspace = canonical_workspace_root(workspace.as_ref())?;
        // `ManagedInstaller::resolve_managed_install` returns this path after
        // checking the process-owned index and install tree. Treat that
        // canonical spelling as an identity: if the directory is replaced by
        // a junction/symlink between those checks and this launch boundary,
        // re-canonicalization must not silently accept the replacement.
        let expected_root = installed_root.as_ref().to_path_buf();
        let installed_root = canonical_file_or_directory(&expected_root, "managed root")?;
        if !canonical_paths_equal(&expected_root, &installed_root) {
            return Err(RuntimeError::PathEscape {
                base: expected_root,
                path: installed_root,
            });
        }
        let root_metadata =
            fs::symlink_metadata(&installed_root).map_err(|source| RuntimeError::PathIo {
                field: "managed root".into(),
                path: installed_root.clone(),
                source,
            })?;
        if metadata_has_reparse_point(&root_metadata) {
            return Err(RuntimeError::PathEscape {
                base: installed_root.clone(),
                path: installed_root.clone(),
            });
        }
        if !installed_root.is_dir() {
            return Err(RuntimeError::NotFileOrDirectory {
                field: "managed root".into(),
                path: installed_root,
            });
        }

        let language = manifest
            .languages
            .iter()
            .find(|language| language.language_id == language_id)
            .ok_or_else(|| {
                RuntimeError::InvalidSpec(format!(
                    "managed manifest does not declare language {language_id:?}"
                ))
            })?;
        let command = language.command.as_ref().unwrap_or(&manifest.command);
        let command_path = resolve_managed_file(
            &installed_root,
            &command.executable,
            "managed.command.executable",
        )?;
        let args = validate_args("managed.command.args", &command.args)?;
        let runtime = match manifest.runtime.kind {
            RuntimeKind::Native => {
                return Ok(ResolvedProcess {
                    executable: command_path,
                    args,
                    current_dir: workspace,
                    env: self.environment.clone().into_map(),
                    runtime: None,
                });
            }
            RuntimeKind::Node => {
                let version_requirement = manifest
                    .runtime
                    .min_version
                    .as_deref()
                    .map(VersionRequirement::parse)
                    .transpose()?;
                let executable = if let Some(node_path) = node_path {
                    validate_argv_value("managed.node_path", node_path)?;
                    let path = Path::new(node_path);
                    if !path.is_absolute() {
                        return Err(RuntimeError::InvalidSpec(
                            "managed.node_path must be an absolute path".into(),
                        ));
                    }
                    self.canonical_executable(path, "managed.node_path")?
                } else {
                    self.resolve_executable(
                        &manifest.runtime.executable,
                        None,
                        "managed.runtime.executable",
                    )?
                };
                ResolvedRuntime {
                    kind: RuntimeKind::Node,
                    executable,
                    version_requirement,
                }
            }
        };
        self.probe_runtime(&runtime).await?;
        let mut process_args = Vec::with_capacity(args.len() + 1);
        process_args.push(command_path.into_os_string());
        process_args.extend(args);
        Ok(ResolvedProcess {
            executable: runtime.executable.clone(),
            args: process_args,
            current_dir: workspace,
            env: self.environment.clone().into_map(),
            runtime: Some(runtime),
        })
    }

    async fn probe_runtime(&self, runtime: &ResolvedRuntime) -> Result<(), RuntimeError> {
        let mut command = Command::new(&runtime.executable);
        command
            .arg("--version")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in self.environment.iter() {
            command.env(key, value);
        }
        let mut child = command
            .spawn()
            .map_err(|_| RuntimeError::RuntimeProbeFailed)?;
        let Some(stdout) = child.stdout.take() else {
            kill_and_reap(&mut child).await;
            return Err(RuntimeError::RuntimeProbeFailed);
        };
        let Some(stderr) = child.stderr.take() else {
            kill_and_reap(&mut child).await;
            return Err(RuntimeError::RuntimeProbeFailed);
        };
        let probe = async {
            let stdout_future = read_probe_output(stdout);
            let stderr_future = read_probe_output(stderr);
            tokio::pin!(stdout_future);
            tokio::pin!(stderr_future);
            let mut stdout_bytes = None;
            let mut stderr_bytes = None;
            while stdout_bytes.is_none() || stderr_bytes.is_none() {
                tokio::select! {
                    result = &mut stdout_future, if stdout_bytes.is_none() => {
                        stdout_bytes = Some(result?);
                    }
                    result = &mut stderr_future, if stderr_bytes.is_none() => {
                        stderr_bytes = Some(result?);
                    }
                }
            }
            let stdout = stdout_bytes.ok_or(RuntimeError::RuntimeProbeFailed)?;
            let status = child
                .wait()
                .await
                .map_err(|_| RuntimeError::RuntimeProbeFailed)?;
            if !status.success() {
                return Err(RuntimeError::RuntimeProbeFailed);
            }
            let version = std::str::from_utf8(&stdout)
                .map_err(|_| RuntimeError::RuntimeProbeInvalidOutput)?
                .lines()
                .find(|line| !line.trim().is_empty())
                .ok_or(RuntimeError::RuntimeProbeInvalidOutput)?;
            let parsed = RuntimeVersion::parse(version)
                .map_err(|_| RuntimeError::RuntimeProbeInvalidOutput)?;
            if let Some(requirement) = &runtime.version_requirement {
                if !requirement.matches(&parsed) {
                    return Err(RuntimeError::RuntimeVersionMismatch {
                        requirement: format_requirement(requirement),
                        actual: version.trim().to_owned(),
                    });
                }
            }
            Ok(())
        };
        match timeout(RUNTIME_PROBE_TIMEOUT, probe).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                kill_and_reap(&mut child).await;
                Err(error)
            }
            Err(_) => {
                kill_and_reap(&mut child).await;
                Err(RuntimeError::RuntimeProbeTimeout)
            }
        }
    }

    /// Resolve a `custom` reference from the compact `server_by_language`
    /// map.  It has no runtime metadata, so it is intentionally direct/native.
    pub fn resolve_custom_ref(
        &self,
        server: &ServerRef,
        workspace: impl AsRef<Path>,
    ) -> Result<ResolvedProcess, RuntimeError> {
        let ServerRef::Custom { executable, args } = server else {
            return Err(RuntimeError::InvalidSpec(
                "resolve_custom_ref requires a custom server reference".into(),
            ));
        };
        let workspace = canonical_workspace_root(workspace.as_ref())?;
        validate_argv_value("custom.executable", executable)?;
        let args = validate_args("custom.args", args)?;
        let executable =
            self.resolve_custom_executable(executable, &workspace, "custom.executable")?;
        Ok(ResolvedProcess {
            executable,
            args,
            current_dir: workspace,
            env: self.environment.clone().into_map(),
            runtime: None,
        })
    }

    /// Managed entries are deliberately not resolved here.  This boundary
    /// accepts only existing local/custom files and has no download/install
    /// path.
    pub fn resolve_server_ref(
        &self,
        server: &ServerRef,
        workspace: impl AsRef<Path>,
    ) -> Result<ResolvedProcess, RuntimeError> {
        match server {
            ServerRef::Managed { .. } => Err(RuntimeError::ManagedServerUnsupported),
            ServerRef::Local { .. } => self.resolve_local(server, workspace),
            ServerRef::Custom { .. } => self.resolve_custom_ref(server, workspace),
        }
    }

    fn resolve_executable(
        &self,
        value: &str,
        base: Option<&Path>,
        field: &str,
    ) -> Result<PathBuf, RuntimeError> {
        validate_argv_value(field, value)?;
        let path = Path::new(value);
        if path.is_absolute() {
            return self.canonical_executable(path, field);
        }
        if contains_path_separator(value) {
            let candidate = base.map_or_else(|| path.to_path_buf(), |base| base.join(path));
            let canonical = self.canonical_executable(&candidate, field)?;
            if let Some(base) = base {
                if !path_is_within(base, &canonical) {
                    return Err(RuntimeError::PathEscape {
                        base: base.to_path_buf(),
                        path: canonical,
                    });
                }
            }
            return Ok(canonical);
        }
        self.search_program(path, field)
    }

    fn resolve_custom_executable(
        &self,
        value: &str,
        workspace: &Path,
        field: &str,
    ) -> Result<PathBuf, RuntimeError> {
        let path = Path::new(value);
        if !path.is_absolute() && !contains_path_separator(value) {
            // A bare custom entry may name a workspace-local file.  If there
            // is no such file, PATH lookup remains available for an already
            // installed system command.
            let candidate = workspace.join(path);
            if candidate.exists() {
                return self.canonical_executable(&candidate, field);
            }
        }
        self.resolve_executable(value, Some(workspace), field)
    }

    fn search_program(&self, program: &Path, field: &str) -> Result<PathBuf, RuntimeError> {
        let Some(search_path) = &self.search_path else {
            return Err(RuntimeError::ExecutableNotFound {
                field: field.to_owned(),
                path: program.to_path_buf(),
            });
        };
        for directory in std::env::split_paths(search_path) {
            let candidate = directory.join(program);
            if let Ok(path) = self.canonical_executable(&candidate, field) {
                return Ok(path);
            }
            #[cfg(windows)]
            if program.extension().is_none() {
                let candidate = directory.join(program).with_extension("exe");
                if let Ok(path) = self.canonical_executable(&candidate, field) {
                    return Ok(path);
                }
            }
        }
        Err(RuntimeError::ExecutableNotFound {
            field: field.to_owned(),
            path: program.to_path_buf(),
        })
    }

    fn canonical_executable(&self, path: &Path, field: &str) -> Result<PathBuf, RuntimeError> {
        let canonical = fs::canonicalize(path).map_err(|source| RuntimeError::PathIo {
            field: field.to_owned(),
            path: path.to_path_buf(),
            source,
        })?;
        if !canonical.is_file() {
            return Err(RuntimeError::ExecutableNotFile {
                field: field.to_owned(),
                path: canonical,
            });
        }
        Ok(canonical)
    }
}

/// Canonicalize and validate a workspace root independently from the resolver.
pub fn canonical_workspace_root(path: &Path) -> Result<PathBuf, RuntimeError> {
    let canonical = fs::canonicalize(path).map_err(|source| RuntimeError::PathIo {
        field: "workspace_root".into(),
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(RuntimeError::WorkspaceNotDirectory(canonical));
    }
    Ok(canonical)
}

fn canonical_file_or_directory(path: &Path, field: &str) -> Result<PathBuf, RuntimeError> {
    let canonical = fs::canonicalize(path).map_err(|source| RuntimeError::PathIo {
        field: field.to_owned(),
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|source| RuntimeError::PathIo {
        field: field.to_owned(),
        path: canonical.clone(),
        source,
    })?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(RuntimeError::NotFileOrDirectory {
            field: field.to_owned(),
            path: canonical,
        });
    }
    Ok(canonical)
}

fn resolve_managed_file(root: &Path, relative: &str, field: &str) -> Result<PathBuf, RuntimeError> {
    validate_argv_value(field, relative)?;
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(RuntimeError::PathEscape {
            base: root.to_path_buf(),
            path: path.to_path_buf(),
        });
    }
    let candidate = root.join(path);
    let metadata = fs::symlink_metadata(&candidate).map_err(|source| RuntimeError::PathIo {
        field: field.to_owned(),
        path: candidate.clone(),
        source,
    })?;
    if metadata_has_reparse_point(&metadata) || !metadata.is_file() {
        return Err(RuntimeError::ExecutableNotFile {
            field: field.to_owned(),
            path: candidate.clone(),
        });
    }
    reject_managed_hard_link(&candidate, root)?;
    let canonical = fs::canonicalize(&candidate).map_err(|source| RuntimeError::PathIo {
        field: field.to_owned(),
        path: candidate.clone(),
        source,
    })?;
    if !path_is_within(root, &canonical) {
        return Err(RuntimeError::PathEscape {
            base: root.to_path_buf(),
            path: canonical.clone(),
        });
    }
    // Recheck after canonicalization as a narrow race guard: a regular file
    // can be replaced by a reparse point or hardlink between the two checks.
    let canonical_metadata =
        fs::symlink_metadata(&canonical).map_err(|source| RuntimeError::PathIo {
            field: field.to_owned(),
            path: canonical.clone(),
            source,
        })?;
    if metadata_has_reparse_point(&canonical_metadata) || !canonical_metadata.is_file() {
        return Err(RuntimeError::ExecutableNotFile {
            field: field.to_owned(),
            path: canonical,
        });
    }
    reject_managed_hard_link(&canonical, root)?;
    Ok(canonical)
}

fn metadata_has_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn reject_managed_hard_link(path: &Path, root: &Path) -> Result<(), RuntimeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path).map_err(|source| RuntimeError::PathIo {
            field: "managed command".into(),
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.nlink() != 1 {
            return Err(RuntimeError::PathEscape {
                base: root.to_path_buf(),
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(|_| RuntimeError::PathEscape {
            base: root.to_path_buf(),
            path: path.to_path_buf(),
        })?;
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        let result = unsafe { GetFileInformationByHandle(handle, &mut information) };
        let close_result = unsafe { CloseHandle(handle) };
        if result.is_err()
            || close_result.is_err()
            || information.nNumberOfLinks != 1
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(RuntimeError::PathEscape {
                base: root.to_path_buf(),
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, root);
        Err(RuntimeError::PathEscape {
            base: root.to_path_buf(),
            path: path.to_path_buf(),
        })
    }
}

async fn read_probe_output<R>(mut reader: R) -> Result<Vec<u8>, RuntimeError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|_| RuntimeError::RuntimeProbeFailed)?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > RUNTIME_PROBE_OUTPUT_LIMIT {
            return Err(RuntimeError::RuntimeProbeOutputLimit);
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

async fn kill_and_reap(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = timeout(RUNTIME_PROBE_REAP_TIMEOUT, child.wait()).await;
}

fn contains_path_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn validate_argv_value(field: &str, value: &str) -> Result<(), RuntimeError> {
    if value.trim().is_empty()
        || value.chars().any(|character| {
            character.is_control() || matches!(character, '|' | ';' | '&' | '<' | '>' | '`')
        })
        || value.contains("$(")
        || (value.contains('$') && value.contains('{'))
    {
        return Err(RuntimeError::ShellSyntax {
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
    let lower = value.to_ascii_lowercase();
    if lower.contains("tcp://") || lower.contains("ws://") || lower.contains("wss://") {
        return Err(RuntimeError::RemoteEndpoint {
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
    if is_shell_program(value) {
        return Err(RuntimeError::ShellSyntax {
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_args(field: &str, args: &[String]) -> Result<Vec<OsString>, RuntimeError> {
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            let field = format!("{field}[{index}]");
            validate_argv_value(&field, arg)?;
            Ok(OsString::from(arg))
        })
        .collect()
}

fn is_shell_program(value: &str) -> bool {
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    let name = normalized.rsplit('/').next().unwrap_or_default();
    matches!(
        name,
        "cmd"
            | "cmd.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
    )
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    let mut root_components = root.components();
    let mut candidate_components = candidate.components();
    loop {
        match root_components.next() {
            None => return true,
            Some(root_component) => {
                let Some(candidate_component) = candidate_components.next() else {
                    return false;
                };
                if !components_equal(root_component, candidate_component) {
                    return false;
                }
            }
        }
    }
}

fn canonical_paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        windows_path_key(left) == windows_path_key(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(windows)]
fn windows_path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        value = format!("\\\\{rest}");
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        value = rest.to_owned();
    } else if let Some(rest) = value.strip_prefix("\\\\.\\") {
        value = rest.to_owned();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value.to_ascii_lowercase()
}

#[cfg(windows)]
fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    use std::os::windows::ffi::OsStrExt;

    left.as_os_str()
        .encode_wide()
        .map(fold_ascii_case)
        .eq(right.as_os_str().encode_wide().map(fold_ascii_case))
}

#[cfg(windows)]
const fn fold_ascii_case(unit: u16) -> u16 {
    if unit >= b'A' as u16 && unit <= b'Z' as u16 {
        unit + (b'a' - b'A') as u16
    } else {
        unit
    }
}

#[cfg(not(windows))]
fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    left == right
}

#[derive(Debug)]
pub enum RuntimeError {
    InvalidSpec(String),
    InvalidVersion {
        value: String,
        reason: String,
    },
    InvalidVersionRequirement(String),
    RuntimeVersionMismatch {
        requirement: String,
        actual: String,
    },
    ShellSyntax {
        field: String,
        value: String,
    },
    RemoteEndpoint {
        field: String,
        value: String,
    },
    PathIo {
        field: String,
        path: PathBuf,
        source: io::Error,
    },
    ExecutableNotFound {
        field: String,
        path: PathBuf,
    },
    ExecutableNotFile {
        field: String,
        path: PathBuf,
    },
    NotFileOrDirectory {
        field: String,
        path: PathBuf,
    },
    WorkspaceNotDirectory(PathBuf),
    PathOutsideWorkspace {
        root: PathBuf,
        path: PathBuf,
    },
    PathEscape {
        base: PathBuf,
        path: PathBuf,
    },
    RuntimeProbeFailed,
    RuntimeProbeTimeout,
    RuntimeProbeOutputLimit,
    RuntimeProbeInvalidOutput,
    ManagedServerUnsupported,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(message) => write!(f, "invalid LSP runtime spec: {message}"),
            Self::InvalidVersion { value, reason } => {
                write!(f, "invalid runtime version {value:?}: {reason}")
            }
            Self::InvalidVersionRequirement(value) => {
                write!(f, "invalid runtime version requirement {value:?}")
            }
            Self::RuntimeVersionMismatch {
                requirement,
                actual,
            } => write!(
                f,
                "runtime version {actual:?} does not satisfy {requirement}"
            ),
            Self::ShellSyntax { field, value } => {
                write!(
                    f,
                    "{field} contains shell syntax or a shell executable: {value:?}"
                )
            }
            Self::RemoteEndpoint { field, value } => {
                write!(f, "{field} contains a remote endpoint: {value:?}")
            }
            Self::PathIo {
                field,
                path,
                source,
            } => write!(f, "could not resolve {field} {}: {source}", path.display()),
            Self::ExecutableNotFound { field, path } => {
                write!(f, "{field} executable was not found: {}", path.display())
            }
            Self::ExecutableNotFile { field, path } => {
                write!(
                    f,
                    "{field} executable is not a regular file: {}",
                    path.display()
                )
            }
            Self::NotFileOrDirectory { field, path } => {
                write!(f, "{field} is not a file or directory: {}", path.display())
            }
            Self::WorkspaceNotDirectory(path) => {
                write!(f, "workspace root is not a directory: {}", path.display())
            }
            Self::PathOutsideWorkspace { root, path } => write!(
                f,
                "path {} is outside workspace root {}",
                path.display(),
                root.display()
            ),
            Self::PathEscape { base, path } => write!(
                f,
                "resolved path {} escapes base {}",
                path.display(),
                base.display()
            ),
            Self::RuntimeProbeFailed => f.write_str("managed runtime version probe failed"),
            Self::RuntimeProbeTimeout => f.write_str("managed runtime version probe timed out"),
            Self::RuntimeProbeOutputLimit => {
                f.write_str("managed runtime version probe output exceeded its limit")
            }
            Self::RuntimeProbeInvalidOutput => {
                f.write_str("managed runtime returned an invalid version")
            }
            Self::ManagedServerUnsupported => {
                f.write_str("managed LSP servers are not resolved by the local runtime boundary")
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PathIo { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{
        Artifact, ArtifactKind, CommandSpec, CustomServer, LanguageSupport, ManifestFiles,
        RuntimeKind, RuntimeSpec, ServerManifest, WINDOWS_X86_64_PLATFORM,
    };
    use std::fs;
    use tempfile::tempdir;

    fn managed_manifest(
        runtime: RuntimeKind,
        command: &str,
        languages: Vec<LanguageSupport>,
    ) -> ServerManifest {
        ServerManifest {
            id: "fixture-managed".into(),
            version: "1.0.0".into(),
            platform: WINDOWS_X86_64_PLATFORM.into(),
            languages,
            source_url: "https://example.com/source".into(),
            license: "MIT".into(),
            artifact: Artifact {
                kind: ArtifactKind::NpmTarball,
                url: "https://registry.npmjs.org/fixture-managed/-/fixture-managed-1.0.0.tgz"
                    .into(),
                sha256: "11".repeat(32),
                size_bytes: Some(1),
                allowed_redirect_hosts: Vec::new(),
                archive_root: "package".into(),
            },
            runtime: RuntimeSpec {
                kind: runtime,
                executable: if runtime == RuntimeKind::Node {
                    "node".into()
                } else {
                    command.into()
                },
                min_version: (runtime == RuntimeKind::Node).then(|| ">=20".into()),
            },
            command: CommandSpec {
                executable: command.into(),
                args: vec!["--stdio".into()],
            },
            files: ManifestFiles {
                entrypoint: command.into(),
                package_lock_sha256: (runtime == RuntimeKind::Node).then(|| "22".repeat(32)),
            },
            capabilities_hint: None,
            generated_at: "2026-08-13T00:00:00Z".into(),
        }
    }

    #[cfg(unix)]
    fn executable_fixture(path: &std::path::Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, contents).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn version_requirements_parse_and_match_without_process_execution() {
        let requirement = VersionRequirement::parse(">=20").unwrap();
        assert!(requirement.matches_str("v20.0.0").unwrap());
        assert!(requirement.matches_str("20.11.1").unwrap());
        assert!(!requirement.matches_str("19.99.0").unwrap());

        let compatible = VersionRequirement::parse("^1.2.3").unwrap();
        assert!(compatible.matches_str("1.9.0").unwrap());
        assert!(!compatible.matches_str("2.0.0").unwrap());
        let tilde = VersionRequirement::parse("~20.11").unwrap();
        assert!(tilde.matches_str("20.11.9").unwrap());
        assert!(!tilde.matches_str("20.12.0").unwrap());
        for invalid in ["", "latest", "1.*", ">= 20", "==20"] {
            assert!(VersionRequirement::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn environment_allowlist_does_not_copy_unlisted_values() {
        let allowlist = EnvironmentAllowlist::with_path("/safe/bin")
            .allow("NODE_PATH", "/safe/node_modules")
            .unwrap();
        assert_eq!(allowlist.as_map().len(), 2);
        assert!(allowlist.contains_key(OsStr::new("PATH")));
        assert!(allowlist.contains_key(OsStr::new("NODE_PATH")));
        assert!(!allowlist.contains_key(OsStr::new("SECRET_TOKEN")));
        assert!(allowlist.clone().allow("BAD=KEY", "value").is_err());
    }

    #[test]
    fn local_resolution_canonicalizes_files_and_rejects_relative_escape() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let installed = workspace.join("servers");
        let outside = directory.path().join("outside");
        fs::create_dir_all(&installed).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let executable = installed.join("server.exe");
        fs::write(&executable, b"fixture").unwrap();
        let escaped = outside.join("escape.exe");
        fs::write(&escaped, b"outside").unwrap();
        let workspace = RuntimeResolver::canonical_workspace_root(&workspace).unwrap();
        let resolver = RuntimeResolver::with_path("");
        let local = ServerRef::Local {
            installed_path: installed.to_string_lossy().into_owned(),
            executable: Some("server.exe".into()),
            args: vec!["--stdio".into()],
        };
        let resolved = resolver.resolve_local(&local, &workspace).unwrap();
        assert_eq!(resolved.executable, fs::canonicalize(executable).unwrap());
        assert_eq!(resolved.current_dir, workspace);
        assert_eq!(resolved.args, vec![OsString::from("--stdio")]);

        let escaping = ServerRef::Local {
            installed_path: installed.to_string_lossy().into_owned(),
            executable: Some("../../outside/escape.exe".into()),
            args: Vec::new(),
        };
        assert!(matches!(
            resolver.resolve_local(&escaping, &workspace),
            Err(RuntimeError::PathEscape { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_symlink_escape_is_rejected_after_canonicalization() {
        use std::os::unix::fs::symlink;
        let directory = tempdir().unwrap();
        let root = directory.path().join("root");
        let outside = directory.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("file.rs"), b"fn main() {}").unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        assert!(matches!(
            RuntimeResolver::resolve_workspace_path(&root, root.join("escape/file.rs")),
            Err(RuntimeError::PathOutsideWorkspace { .. })
        ));
    }

    #[test]
    fn custom_node_resolution_uses_node_argv_and_never_a_shell() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let bin = directory.path().join("bin");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let node = bin.join("node");
        let server = workspace.join("server.js");
        fs::write(&node, b"node fixture").unwrap();
        fs::write(&server, b"server fixture").unwrap();
        let resolver = RuntimeResolver::with_path(bin.as_os_str().to_os_string());
        let custom = CustomServer {
            language_ids: vec!["javascript".into()],
            executable: "server.js".into(),
            args: vec!["--stdio".into()],
            runtime: RuntimeSpec {
                kind: RuntimeKind::Node,
                executable: "node".into(),
                min_version: Some(">=20".into()),
            },
            source: "user-provided/unchecked".into(),
            license: "unknown".into(),
            version: "unknown".into(),
        };
        let resolved = resolver.resolve_custom(&custom, &workspace).unwrap();
        assert_eq!(resolved.executable, fs::canonicalize(node).unwrap());
        assert_eq!(
            resolved.args,
            vec![
                fs::canonicalize(server).unwrap().into_os_string(),
                OsString::from("--stdio")
            ]
        );
        assert_eq!(resolved.env.len(), 1);
        assert!(resolved.env.contains_key(OsStr::new("PATH")));
        assert!(resolved
            .runtime
            .unwrap()
            .validate_reported_version("v20.1.0")
            .is_ok());
    }

    #[tokio::test]
    async fn managed_native_resolution_uses_reviewed_command_and_workspace() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let installed = directory.path().join("installed");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("rust-analyzer.exe"), b"fixture").unwrap();
        let manifest = managed_manifest(
            RuntimeKind::Native,
            "rust-analyzer.exe",
            vec![LanguageSupport {
                language_id: "rust".into(),
                extensions: vec![".rs".into()],
                command: None,
            }],
        );
        let resolved = RuntimeResolver::with_path("")
            .resolve_managed(&manifest, "rust", &installed, None, &workspace)
            .await
            .unwrap();
        assert_eq!(
            resolved.executable,
            fs::canonicalize(installed.join("rust-analyzer.exe")).unwrap()
        );
        assert_eq!(resolved.args, vec![OsString::from("--stdio")]);
        assert_eq!(resolved.current_dir, fs::canonicalize(workspace).unwrap());
        assert!(resolved.env.is_empty() || resolved.env.contains_key(OsStr::new("PATH")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_root_replacement_is_rejected_before_command_resolution() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let installed = directory.path().join("installed");
        let outside = directory.path().join("outside");
        let moved = directory.path().join("moved-installed");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&installed).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(installed.join("rust-analyzer.exe"), b"fixture").unwrap();
        let expected_root = fs::canonicalize(&installed).unwrap();
        fs::rename(&installed, &moved).unwrap();
        symlink(&outside, &installed).unwrap();

        let manifest = managed_manifest(
            RuntimeKind::Native,
            "rust-analyzer.exe",
            vec![LanguageSupport {
                language_id: "rust".into(),
                extensions: vec![".rs".into()],
                command: None,
            }],
        );
        let result = RuntimeResolver::with_path("")
            .resolve_managed(&manifest, "rust", &expected_root, None, &workspace)
            .await;
        assert!(matches!(result, Err(RuntimeError::PathEscape { .. })));
        assert!(moved.join("rust-analyzer.exe").is_file());
        assert!(outside.is_dir());
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn managed_command_hardlink_is_rejected_at_launch_boundary() {
        use std::fs::hard_link;

        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let installed = directory.path().join("installed");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&installed).unwrap();
        let command = installed.join("rust-analyzer.exe");
        fs::write(&command, b"fixture").unwrap();
        hard_link(&command, installed.join("alias.exe")).unwrap();
        let manifest = managed_manifest(
            RuntimeKind::Native,
            "rust-analyzer.exe",
            vec![LanguageSupport {
                language_id: "rust".into(),
                extensions: vec![".rs".into()],
                command: None,
            }],
        );
        let result = RuntimeResolver::with_path("")
            .resolve_managed(&manifest, "rust", &installed, None, &workspace)
            .await;
        assert!(matches!(result, Err(RuntimeError::PathEscape { .. })));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_node_selects_each_declared_language_command() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let installed = directory.path().join("installed");
        let bin = directory.path().join("bin");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(installed.join("bin")).unwrap();
        fs::create_dir_all(&bin).unwrap();
        executable_fixture(
            &bin.join("node"),
            "#!/bin/sh\n[ \"$1\" = \"--version\" ] && echo v20.11.1\n",
        );
        for executable in [
            "vscode-json-language-server",
            "vscode-html-language-server",
            "vscode-css-language-server",
        ] {
            fs::write(installed.join("bin").join(executable), b"fixture").unwrap();
        }
        let manifest = managed_manifest(
            RuntimeKind::Node,
            "bin/vscode-json-language-server",
            vec![
                LanguageSupport {
                    language_id: "json".into(),
                    extensions: vec![".json".into()],
                    command: None,
                },
                LanguageSupport {
                    language_id: "html".into(),
                    extensions: vec![".html".into()],
                    command: Some(CommandSpec {
                        executable: "bin/vscode-html-language-server".into(),
                        args: vec!["--stdio".into()],
                    }),
                },
                LanguageSupport {
                    language_id: "css".into(),
                    extensions: vec![".css".into()],
                    command: Some(CommandSpec {
                        executable: "bin/vscode-css-language-server".into(),
                        args: vec!["--stdio".into()],
                    }),
                },
            ],
        );
        let resolver = RuntimeResolver::with_path(bin.as_os_str().to_os_string());
        for (language, executable) in [
            ("json", "vscode-json-language-server"),
            ("html", "vscode-html-language-server"),
            ("css", "vscode-css-language-server"),
        ] {
            let resolved = resolver
                .resolve_managed(&manifest, language, &installed, None, &workspace)
                .await
                .unwrap();
            assert_eq!(
                resolved.args[0],
                installed
                    .join("bin")
                    .join(executable)
                    .canonicalize()
                    .unwrap()
                    .into_os_string()
            );
            assert_eq!(resolved.args[1], OsString::from("--stdio"));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_node_rejects_old_missing_and_hanging_runtimes() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let installed = directory.path().join("installed");
        let bin = directory.path().join("bin");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&installed).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(installed.join("server.js"), b"fixture").unwrap();
        let manifest = managed_manifest(
            RuntimeKind::Node,
            "server.js",
            vec![LanguageSupport {
                language_id: "javascript".into(),
                extensions: vec![".js".into()],
                command: None,
            }],
        );

        executable_fixture(
            &bin.join("node"),
            "#!/bin/sh\n[ \"$1\" = \"--version\" ] && echo v19.0.0\n",
        );
        let resolver = RuntimeResolver::with_path(bin.as_os_str().to_os_string());
        assert!(matches!(
            resolver
                .resolve_managed(&manifest, "javascript", &installed, None, &workspace)
                .await,
            Err(RuntimeError::RuntimeVersionMismatch { .. })
        ));

        executable_fixture(
            &bin.join("node"),
            "#!/bin/sh\n[ \"$1\" = \"--version\" ] && echo v20.11.1\n",
        );
        let explicit_node = bin.join("node");
        let explicit_resolver = RuntimeResolver::with_path("");
        let resolved = explicit_resolver
            .resolve_managed(
                &manifest,
                "javascript",
                &installed,
                Some(explicit_node.to_str().unwrap()),
                &workspace,
            )
            .await
            .unwrap();
        assert_eq!(resolved.executable, explicit_node.canonicalize().unwrap());

        fs::remove_file(bin.join("node")).unwrap();
        assert!(matches!(
            resolver
                .resolve_managed(&manifest, "javascript", &installed, None, &workspace)
                .await,
            Err(RuntimeError::ExecutableNotFound { .. })
        ));

        executable_fixture(
            &bin.join("node"),
            "#!/bin/sh\ni=0\nwhile [ $i -lt 9000 ]; do printf x; i=$((i+1)); done\n",
        );
        assert!(matches!(
            resolver
                .resolve_managed(&manifest, "javascript", &installed, None, &workspace)
                .await,
            Err(RuntimeError::RuntimeProbeOutputLimit)
        ));

        executable_fixture(&bin.join("node"), "#!/bin/sh\nwhile true; do :; done\n");
        let started = std::time::Instant::now();
        let hanging = resolver
            .resolve_managed(&manifest, "javascript", &installed, None, &workspace)
            .await;
        assert!(matches!(hanging, Err(RuntimeError::RuntimeProbeTimeout)));
        assert!(started.elapsed() < Duration::from_secs(7));
    }

    #[test]
    fn shell_strings_remote_endpoints_missing_files_and_managed_entries_fail() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let resolver = RuntimeResolver::with_path("");
        let shell = ServerRef::Custom {
            executable: "server".into(),
            args: vec!["$(whoami)".into()],
        };
        assert!(matches!(
            resolver.resolve_custom_ref(&shell, &workspace),
            Err(RuntimeError::ShellSyntax { .. })
        ));
        let remote = ServerRef::Custom {
            executable: "server".into(),
            args: vec!["tcp://127.0.0.1:9000".into()],
        };
        assert!(matches!(
            resolver.resolve_custom_ref(&remote, &workspace),
            Err(RuntimeError::RemoteEndpoint { .. })
        ));
        let managed = ServerRef::managed("rust-analyzer", "1.0.0");
        assert!(matches!(
            resolver.resolve_server_ref(&managed, &workspace),
            Err(RuntimeError::ManagedServerUnsupported)
        ));
        let missing = RuntimeSpec {
            kind: RuntimeKind::Node,
            executable: "node".into(),
            min_version: Some(">=20".into()),
        };
        assert!(matches!(
            resolver.resolve_runtime_in_workspace(&missing, &workspace),
            Err(RuntimeError::ExecutableNotFound { .. })
        ));
    }

    #[test]
    fn resolved_process_converts_to_argv_only_process_spec() {
        let directory = tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let executable = workspace.join("server");
        fs::write(&executable, b"fixture").unwrap();
        let resolver = RuntimeResolver::with_path("");
        let server = ServerRef::Custom {
            executable: executable.to_string_lossy().into_owned(),
            args: vec!["--literal;not-shell".into()],
        };
        // Shell punctuation is rejected even though argv would otherwise keep
        // it literal; users should register an ordinary argument instead.
        assert!(resolver.resolve_custom_ref(&server, &workspace).is_err());

        let server = ServerRef::Custom {
            executable: executable.to_string_lossy().into_owned(),
            args: vec!["--literal".into()],
        };
        let resolved = resolver.resolve_custom_ref(&server, &workspace).unwrap();
        let process = resolved.process_spec();
        assert_eq!(process.executable, resolved.executable);
        assert_eq!(process.args, resolved.args);
        assert_eq!(process.current_dir, resolved.current_dir);
        assert_eq!(process.env, resolved.env);
    }
}
