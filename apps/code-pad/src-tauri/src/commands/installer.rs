//! Thin commands for the reviewed managed-server catalog and installer.
//!
//! The UI receives catalog metadata and exact install status, but it never
//! supplies a manifest, artifact URL, or destination path back to the
//! installer. Installs resolve exact keys against the process-owned catalog;
//! removals resolve exact keys against the process-owned installed index.

use crate::lsp::{
    initial_catalog, InstallError, LspManager, ManagedInstallStatus, ManagedInstaller,
    ServerManifest,
};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn lsp_catalog() -> Result<Vec<ServerManifest>, String> {
    Ok(initial_catalog())
}

#[tauri::command]
pub fn lsp_installed(
    installer: State<'_, Arc<ManagedInstaller>>,
) -> Result<Vec<ManagedInstallStatus>, String> {
    installer
        .installed_status()
        .map(|statuses| statuses.into_iter().map(public_install_status).collect())
        .map_err(public_install_error)
}

#[tauri::command]
pub fn lsp_recover_installed(installer: State<'_, Arc<ManagedInstaller>>) -> Result<(), String> {
    installer
        .recover_installed_index()
        .map_err(|_| "관리형 서버 설치 목록을 복구하지 못했습니다".into())
}

#[tauri::command]
pub async fn lsp_install(
    installer: State<'_, Arc<ManagedInstaller>>,
    manifest_id: String,
    version: String,
    platform: String,
) -> Result<(), String> {
    installer
        .install_catalog(&manifest_id, &version, &platform)
        .await
        .map(|_| ())
        .map_err(public_install_error)
}

#[tauri::command]
pub async fn lsp_uninstall(
    manager: State<'_, Arc<LspManager>>,
    installer: State<'_, Arc<ManagedInstaller>>,
    manifest_id: String,
    version: String,
    platform: String,
) -> Result<(), String> {
    // A managed directory is never removed while a language session could be
    // using it.  stop_all is intentionally completed before any filesystem
    // mutation and an error leaves both the index and install untouched.
    manager
        .stop_all()
        .await
        .map_err(|_| "관리형 서버 작업을 완료하지 못했습니다".to_owned())?;
    installer
        .uninstall_indexed(&manifest_id, &version, &platform)
        .map_err(public_install_error)
}

fn public_install_status(mut status: ManagedInstallStatus) -> ManagedInstallStatus {
    if status.reason.is_some() {
        status.reason = Some("관리형 서버 설치 검증에 실패했습니다".into());
    }
    status
}

fn public_install_error(error: InstallError) -> String {
    match error {
        InstallError::IndexCorrupt => "관리형 서버 설치 목록 복구가 필요합니다".into(),
        _ => "관리형 서버 작업을 완료하지 못했습니다".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{public_install_error, public_install_status};
    use crate::lsp::{InstallError, ManagedInstallState, ManagedInstallStatus};

    #[test]
    fn installer_error_detail_is_replaced_before_ipc() {
        let public = public_install_error(InstallError::Network(
            "https://user:secret@example.test/private".into(),
        ));
        assert_eq!(public, "관리형 서버 작업을 완료하지 못했습니다");
        assert!(!public.contains("secret"));
    }

    #[test]
    fn corrupt_index_keeps_only_a_safe_recovery_signal() {
        assert_eq!(
            public_install_error(InstallError::IndexCorrupt),
            "관리형 서버 설치 목록 복구가 필요합니다"
        );
    }

    #[test]
    fn install_status_reason_is_replaced_before_ipc() {
        let status = public_install_status(ManagedInstallStatus {
            manifest_id: "fixture".into(),
            version: "1.0.0".into(),
            platform: "windows-x86_64".into(),
            state: ManagedInstallState::NeedsReinstall,
            reason: Some(r#"metadata at C:\private token=secret"#.into()),
            installed: None,
        });
        assert_eq!(
            status.reason.as_deref(),
            Some("관리형 서버 설치 검증에 실패했습니다")
        );
    }
}
