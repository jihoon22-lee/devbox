//! Local/custom runtime resolution and the process-launch security boundary.
//!
//! This module resolves only already-installed local files.  It never downloads
//! or installs a server and never invokes a shell to discover a runtime.  The
//! resulting `ResolvedProcess` contains canonical paths, a canonical workspace
//! cwd, exact argv values, and an explicitly allowlisted environment suitable
//! for `LspProcess::spawn`.

use super::catalog::{CustomServer, RuntimeKind, RuntimeSpec, ServerRef};
use super::process::ProcessSpec;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

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

/// A runtime path resolved to an existing regular file.  Version checks are
/// explicit and consume a caller-provided version string; they never execute
/// `node`, `cmd.exe`, PowerShell, or any other shell/runtime discovery command.
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
    use crate::lsp::{CustomServer, RuntimeKind, RuntimeSpec};
    use std::fs;
    use tempfile::tempdir;

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
