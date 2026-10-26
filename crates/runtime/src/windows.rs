//! Windows copy targets. This is a non-deleting copy of Git's admitted file list.
use std::path::{Component, Path, PathBuf};

pub const EXECUTION_NOTICE: &str = "Windows에서 실행 · 자식 프로세스 종료는 보장하지 않음";
#[derive(Debug, thiserror::Error)]
pub enum WindowsError {
    #[error("invalid Windows copy path")]
    Path,
    #[error("Windows copy failed")]
    Io(#[from] std::io::Error),
    #[error("Git file listing failed")]
    Git,
    #[error("Windows copy inputs changed or coordination cancelled")]
    Changed,
    #[error("Windows copy input listing exceeded its limit")]
    Limit,
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub copied: u64,
    pub deleted: u64,
    pub skipped: u64,
}
pub fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
pub fn windows_to_wsl(path: &str) -> Result<PathBuf, WindowsError> {
    let bytes = path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || &bytes[1..3] != b":\\"
        || path.chars().any(char::is_control)
    {
        return Err(WindowsError::Path);
    }
    let mut result = PathBuf::from(format!("/mnt/{}", (bytes[0] as char).to_ascii_lowercase()));
    for part in path[3..].split('\\').filter(|s| !s.is_empty()) {
        if !valid_component(part) {
            return Err(WindowsError::Path);
        }
        result.push(part);
    }
    Ok(result)
}
pub fn target_directory(root: &str, name: &str) -> Result<String, WindowsError> {
    windows_to_wsl(root)?;
    if !valid_component(name) {
        return Err(WindowsError::Path);
    }
    Ok(format!("{}\\{name}", root.trim_end_matches('\\')))
}
pub async fn sync(source: &Path, target: &Path) -> Result<SyncReport, WindowsError> {
    crate::windows_copy::copy_files(source, target).await
}

#[derive(Clone)]
pub(crate) struct PreparedCopy {
    pub identity: crate::windows_copy::CopyIdentity,
    pub source: PathBuf,
    pub target: PathBuf,
    pub cwd: String,
    pub relative_cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WindowsDefinition {
    pub cwd: String,
    pub def: crate::RunDef,
    #[serde(default)]
    pub copy: Option<crate::windows_copy_store::CopyRef>,
}
impl crate::RuntimeService {
    /// Only an active recorded wrapper attempt can obtain its immutable Windows command.
    pub async fn prepare_windows(
        &self,
        id: &devbox_protocol::windows::RunIdentity,
    ) -> crate::R<devbox_protocol::windows::PreparedWindows> {
        use devbox_protocol::{Fail, windows::*};
        let invalid = || Fail::Internal("Windows attempt is unavailable".into());
        let value = self.stored(&id.run_id).await?;
        let store = crate::exit_record::ExitStore::new(
            &self.execution()?.paths.data_dir.join("runs"),
            &id.run_id,
        )
        .map_err(|_| invalid())?;
        if !value.run.state.is_live()
            || store.current_attempt().map_err(|_| invalid())?.as_deref() != Some(&id.attempt_id)
        {
            return Err(invalid());
        }
        let windows = value.windows.ok_or_else(invalid)?;
        let reference = windows.copy.as_ref().ok_or_else(invalid)?;
        let observation = self.observation_intent(id, reference).await?;
        let (env, names) = match &windows.def {
            crate::RunDef::Task(d) => (&d.env, &d.secrets),
            crate::RunDef::Service(d) => (&d.env, &d.secrets),
            _ => return Err(invalid()),
        };
        let mut injected = names.clone();
        injected.extend(
            env.iter()
                .filter(|(k, v)| crate::secret_input::injected_env(&windows.def, k, v))
                .map(|(k, _)| k.clone()),
        );
        injected.sort();
        injected.dedup();
        Ok(PreparedWindows {
            interop: self
                .execution()?
                .windows_interop
                .to_string_lossy()
                .into_owned(),
            request: RunWindows {
                run_id: id.run_id.clone(),
                attempt_id: id.attempt_id.clone(),
                cwd: windows.cwd,
                command: windows.def.command().ok_or_else(invalid)?.into(),
                env: env.clone(),
                observation,
                ports: if let crate::RunDef::Service(d) = &windows.def {
                    d.ports_hint.clone()
                } else {
                    Vec::new()
                },
            },
            injected,
        })
    }
}

pub(crate) fn valid_component(name: &str) -> bool {
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
    {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}
pub(crate) fn reject_links(path: &Path) -> Result<(), WindowsError> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        match std::fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(WindowsError::Path),
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
impl crate::RuntimeService {
    pub async fn claim_windows_native(
        &self,
        id: &devbox_protocol::windows::RunIdentity,
    ) -> crate::R<()> {
        self.prepare_windows(id).await?;
        crate::exit_record::ExitStore::new(
            &self.execution()?.paths.data_dir.join("runs"),
            &id.run_id,
        )
        .and_then(|store| store.claim_windows_native(&id.attempt_id))
        .map_err(|_| {
            devbox_protocol::Fail::Internal(
                "Windows native attempt already dispatched or unavailable".into(),
            )
        })
    }
}

impl crate::RuntimeService {
    pub fn windows_preview(
        &self,
        project_id: &str,
        project: &str,
        selected_worktree: Option<&str>,
        definitions: &[crate::RunDef],
    ) -> Vec<String> {
        if !definitions.iter().any(crate::launch::is_windows) {
            return Vec::new();
        }
        let Some(execution) = self.execution.as_ref().filter(|e| e.windows_root.is_some()) else {
            return definitions
                .iter()
                .filter(|d| crate::launch::is_windows(d))
                .map(|_| format!("{EXECUTION_NOTICE} · config.local.toml의 windows.root 설정 필요"))
                .take(1)
                .collect();
        };
        definitions
            .iter()
            .filter(|d| crate::launch::is_windows(d))
            .map(|d| {
                let configured_worktree = match d {
                    crate::RunDef::Task(d) => d.worktree.as_deref(),
                    crate::RunDef::Service(d) => d.worktree.as_deref(),
                    _ => None,
                };
                let effective = selected_worktree.or(configured_worktree).unwrap_or("main");
                let Ok(identity) = crate::windows_copy::CopyIdentity::new(
                    execution,
                    project_id,
                    effective,
                    Path::new(project),
                ) else {
                    return format!("{EXECUTION_NOTICE} · Windows 사본 경로 설정 오류");
                };
                let directory = identity.directory();
                let cwd = match d {
                    crate::RunDef::Task(d) => d.cwd.as_deref(),
                    crate::RunDef::Service(d) => d.cwd.as_deref(),
                    _ => None,
                }
                .unwrap_or(".");
                let path = Path::new(cwd);
                let suffix = if path.is_absolute() {
                    path.strip_prefix(project).ok()
                } else {
                    Some(path)
                };
                let target = suffix
                    .filter(|p| {
                        p.components()
                            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
                    })
                    .map(|p| {
                        let parts = p
                            .components()
                            .filter_map(|c| {
                                if let Component::Normal(s) = c {
                                    Some(s.to_string_lossy())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>();
                        if parts.is_empty() {
                            directory.clone()
                        } else {
                            format!("{}\\{}", directory, parts.join("\\"))
                        }
                    })
                    .unwrap_or_else(|| "프로젝트 범위를 벗어난 시작 폴더".into());
                format!("{} · {target} · {EXECUTION_NOTICE}", d.id())
            })
            .collect()
    }
}
