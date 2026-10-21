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
    let output = tokio::process::Command::new("git")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(source)
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .kill_on_drop(true)
        .output()
        .await?;
    if !output.status.success() {
        return Err(WindowsError::Git);
    }
    reject_links(source)?;
    reject_links(target)?;
    let mut report = SyncReport::default();
    let mut admitted = Vec::new();
    let mut case_paths = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for bytes in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let name = std::str::from_utf8(bytes).map_err(|_| WindowsError::Path)?;
        let relative = Path::new(name);
        if !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
            || name.contains('\\')
        {
            return Err(WindowsError::Path);
        }
        if !seen.insert(name) {
            continue;
        }
        let mut prefix = String::new();
        for part in name.split('/') {
            if !valid_component(part) {
                return Err(WindowsError::Path);
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if case_paths
                .insert(prefix.to_lowercase(), prefix.clone())
                .is_some_and(|prior| prior != prefix)
            {
                return Err(WindowsError::Path);
            }
        }
        reject_links(&source.join(relative))?;
        reject_links(&target.join(relative))?;
        admitted.push(relative.to_path_buf());
    }
    tokio::fs::create_dir_all(target).await?;
    for relative in admitted {
        let from = source.join(&relative);
        let metadata = match tokio::fs::symlink_metadata(&from).await {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                report.skipped += 1;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        // Git symlinks and submodule directories are not regular copy inputs.
        if !metadata.is_file() {
            report.skipped += 1;
            continue;
        }
        let to = target.join(&relative);
        if tokio::fs::metadata(&to).await.is_ok_and(|m| {
            m.is_file()
                && m.len() == metadata.len()
                && m.modified().ok() == metadata.modified().ok()
        }) {
            report.skipped += 1;
            continue;
        }
        tokio::fs::create_dir_all(to.parent().ok_or(WindowsError::Path)?).await?;
        tokio::fs::copy(&from, &to).await?;
        let file = std::fs::OpenOptions::new().write(true).open(&to)?;
        file.set_times(std::fs::FileTimes::new().set_modified(metadata.modified()?))?;
        report.copied += 1;
    }
    Ok(report)
}

pub(crate) struct PreparedCopy {
    pub source: PathBuf,
    pub target: PathBuf,
    pub cwd: String,
    pub relative_cwd: PathBuf,
}
impl PreparedCopy {
    pub async fn sync(&self) -> Result<SyncReport, WindowsError> {
        let report = sync(&self.source, &self.target).await?;
        let cwd = self.target.join(&self.relative_cwd);
        reject_links(&cwd)?;
        tokio::fs::create_dir_all(cwd).await?;
        Ok(report)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WindowsDefinition {
    pub cwd: String,
    pub def: crate::RunDef,
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
fn reject_links(path: &Path) -> Result<(), WindowsError> {
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
    pub async fn windows_ready(&self, id: &devbox_protocol::windows::RunIdentity) -> crate::R<()> {
        self.reconcile_one(&id.run_id).await?;
        let _guard = self.run_lock(&id.run_id).await;
        self.prepare_windows(id).await?;
        let mut value = self.stored(&id.run_id).await?;
        if value.run.attempt_id.as_deref() != Some(&id.attempt_id) {
            return Err(devbox_protocol::Fail::Internal(
                "Windows readiness attempt changed".into(),
            ));
        }
        if !value.run.ready {
            value.run.ready = true;
            self.save_run(&value).await?;
        }
        Ok(())
    }
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
        name: &str,
        project: &str,
        definitions: &[crate::RunDef],
    ) -> Vec<String> {
        if !definitions.iter().any(crate::launch::is_windows) {
            return Vec::new();
        }
        let Some(root) = self
            .execution
            .as_ref()
            .and_then(|e| e.windows_root.as_deref())
        else {
            return definitions
                .iter()
                .filter(|d| crate::launch::is_windows(d))
                .map(|_| format!("{EXECUTION_NOTICE} · config.local.toml의 windows.root 설정 필요"))
                .take(1)
                .collect();
        };
        let Ok(directory) = target_directory(root, name) else {
            return vec![format!("{EXECUTION_NOTICE} · Windows 사본 경로 설정 오류")];
        };
        definitions
            .iter()
            .filter(|d| crate::launch::is_windows(d))
            .map(|d| {
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
