//! Explicit Recovery UI handoff to the retained helper after product shutdown.
use super::*;
use serde_json::{json, Value};
use std::{os::windows::process::CommandExt, sync::atomic::Ordering};
use windows::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{MessageBoxW, IDCANCEL, MB_ICONEXCLAMATION, MB_RETRYCANCEL},
};

static LEAVING: AtomicBool = AtomicBool::new(false);

/// Control Center's ordinary shortcut remains a recovery entrypoint even while
/// its WebView/data namespace is unavailable. No product store is opened here.
pub(crate) fn resume_before_shell() -> Result<bool> {
    let image = std::env::current_exe().map_err(|_| "bootstrap_identity_unavailable")?;
    let Some(root) = image
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .filter(|path| path.file_name().is_some_and(|name| name == "generations"))
        .and_then(|path| path.parent())
    else {
        return Ok(false);
    };
    match fs::symlink_metadata(root.join("suite-data-restore.block")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("restore_record_unavailable"),
        Ok(_) => {}
    }
    let (scope, root, payload, helper) = installation()?;
    let bytes = read(&root.join("suite-data-restore.json"), 4096)?;
    if read(&root.join("suite-data-restore.block"), 4096)? != bytes {
        return Err("restore_operation_conflict");
    }
    let (id, revision): (String, String) =
        serde_json::from_slice(&bytes).map_err(|_| "restore_record_invalid")?;
    let request = Request {
        action: "resume".into(),
        id: id.clone(),
    };
    if !request.validate() {
        return Err("restore_operation_invalid");
    }
    let operation = dirs::data_local_dir()
        .ok_or("bootstrap_data_unavailable")?
        .join(format!(
            "com.devbox.v08.suite-restore.i{}",
            scope.installation_key
        ))
        .join(&id);
    if hash(&read(
        &operation.join("restore-plan.json"),
        16 * 1024 * 1024,
    )?) != revision
    {
        return Err("restore_plan_changed");
    }
    let progress_path = operation.join("restore-progress.json");
    let progress: Value = match fs::symlink_metadata(&progress_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            json!({"phase":"applying","planRevision":revision})
        }
        _ => serde_json::from_slice(&read(&progress_path, 4096)?)
            .map_err(|_| "restore_record_invalid")?,
    };
    if progress["planRevision"].as_str() != Some(revision.as_str()) {
        return Err("restore_plan_changed");
    }
    let action = match progress["phase"].as_str() {
        Some("committing" | "committed") => "commit",
        Some("rollingBack" | "rolledBack") => "rollback",
        Some("applying" | "health") => "resume",
        _ => return Err("restore_record_invalid"),
    };
    scope.revalidate()?;
    std::process::Command::new(helper)
        .arg("--reviewed-data-action")
        .arg(root)
        .arg(payload)
        .arg(action)
        .arg(id)
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|_| "bootstrap_launch_failed")?;
    Ok(true)
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub action: String,
    pub id: String,
}
impl Request {
    pub(crate) fn validate(&self) -> bool {
        match self.action.as_str() {
            "snapshot" => self.id.is_empty(),
            "restore" | "resume" | "commit" | "rollback" => {
                uuid::Uuid::parse_str(&self.id).is_ok_and(|id| id.to_string() == self.id)
            }
            _ => false,
        }
    }
}
fn installation() -> Result<(
    crate::suite::platform::component_scope::CapturedScope,
    PathBuf,
    PathBuf,
    PathBuf,
)> {
    let scope = crate::suite::capture_own("control-center")?;
    let root = PathBuf::from(scope.review_root());
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.installation_id != scope.manifest.installation_id
        || owner.generation != scope.manifest.generation
        || owner.root_identity
            != filesystem_identity(&root, true)
                .map_err(|_| "bootstrap_root_changed")?
                .components()
        || owner.payload_revision.len() != 64
        || !owner
            .payload_revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("bootstrap_owner_changed");
    }
    let directory = root.join("setup").join(&owner.payload_revision);
    let payload_path = directory.join("suite-payload.json");
    let bytes = read(&payload_path, MAX_RELEASE_BYTES as u64)?;
    if hash(&bytes) != owner.payload_revision {
        return Err("bootstrap_payload_changed");
    }
    let payload = Payload::parse(&bytes)?;
    let helper = directory.join("devbox-suite-bootstrap.exe");
    verify_payload_owner(&payload, &helper)?;
    scope.revalidate()?;
    Ok((scope, root, payload_path, helper))
}
pub(crate) fn inventory() -> Result<Value> {
    let (scope, root, _, _) = installation()?;
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    let key = &scope.installation_key;
    let data = parent.join(format!("com.devbox.v08.controlcenter.i{key}"));
    let checkpoints = crate::core::delivery_store::Store::inspect(&data)?
        .filter(|(journal, _)| {
            journal.candidate == scope.manifest && journal.installation_key == *key
        })
        .map(|(journal, _)| journal.data_checkpoints)
        .ok_or("bootstrap_journal_missing")?;
    let recovery = parent.join(format!("com.devbox.v08.suite-restore.i{key}"));
    let mut operations = Vec::new();
    match fs::symlink_metadata(&recovery) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("restore_record_unavailable"),
        Ok(_) => {
            ensure_no_links(&recovery).map_err(|_| "restore_operation_unsafe")?;
            for entry in fs::read_dir(&recovery)
                .map_err(|_| "restore_record_unavailable")?
                .take(33)
            {
                let entry = entry.map_err(|_| "restore_record_unavailable")?;
                let id = entry
                    .file_name()
                    .to_str()
                    .ok_or("restore_operation_invalid")?
                    .to_owned();
                if !uuid::Uuid::parse_str(&id).is_ok_and(|uuid| uuid.to_string() == id) {
                    return Err("restore_operation_invalid");
                }
                let path = entry.path();
                let plan_path = path.join("restore-plan.json");
                match fs::symlink_metadata(&plan_path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        operations.push(json!({"id":id,"phase":"preparationInterrupted"}));
                        continue;
                    }
                    _ => {}
                }
                let bytes = read(&plan_path, 16 * 1024 * 1024)?;
                let plan: DataRestorePlan =
                    serde_json::from_slice(&bytes).map_err(|_| "restore_plan_invalid")?;
                if plan.operation_id != id
                    || plan.installation_key != *key
                    || plan.generation != scope.manifest.generation
                {
                    return Err("restore_plan_changed");
                }
                let progress_path = path.join("restore-progress.json");
                let phase = match fs::symlink_metadata(&progress_path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        "prepared".to_owned()
                    }
                    _ => {
                        let progress: Value = serde_json::from_slice(&read(&progress_path, 4096)?)
                            .map_err(|_| "restore_record_invalid")?;
                        if progress["planRevision"].as_str() != Some(hash(&bytes).as_str()) {
                            return Err("restore_plan_changed");
                        }
                        progress["phase"]
                            .as_str()
                            .ok_or("restore_record_invalid")?
                            .to_owned()
                    }
                };
                operations.push(json!({"id":id,"phase":phase,"checkpointId":plan.source.id,"preparedMs":plan.prepared_ms}));
            }
            if operations.len() > 32 {
                return Err("bootstrap_restore_retention_review_required");
            }
        }
    }
    operations
        .sort_by_key(|value| std::cmp::Reverse(value["preparedMs"].as_u64().unwrap_or_default()));
    let active = match fs::symlink_metadata(root.join("suite-data-restore.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        _ => Some(
            serde_json::from_slice::<(String, String)>(&read(
                &root.join("suite-data-restore.json"),
                4096,
            )?)
            .map_err(|_| "restore_record_invalid")?
            .0,
        ),
    };
    scope.revalidate()?;
    Ok(json!({"checkpoints":checkpoints,"operations":operations,"activeOperation":active}))
}
pub(crate) fn launch(app: &tauri::AppHandle, request: Request) -> Result<Value> {
    if !request.validate() {
        return Err("restore_action_invalid");
    }
    if LEAVING.swap(true, Ordering::SeqCst) {
        return Err("restore_action_already_started");
    }
    let result = (|| {
        let (scope, root, payload, helper) = installation()?;
        let available = inventory()?;
        if request.action == "restore"
            && !available["checkpoints"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == request.id))
        {
            return Err("checkpoint_missing");
        }
        if matches!(request.action.as_str(), "resume" | "commit" | "rollback")
            && !available["operations"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == request.id))
        {
            return Err("restore_operation_invalid");
        }
        scope.revalidate()?;
        let child = std::process::Command::new(helper)
            .arg("--reviewed-data-action")
            .arg(root)
            .arg(payload)
            .arg(&request.action)
            .arg(&request.id)
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|_| "bootstrap_launch_failed")?;
        drop(child);
        let app = app.clone();
        // Deliver the accepted response before closing this shell. The helper
        // waits for *all* leases; it never kills another product or user task.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(750));
            app.exit(0);
        });
        Ok(json!({"accepted":true}))
    })();
    if result.is_err() {
        LEAVING.store(false, Ordering::SeqCst);
    }
    result
}
pub(super) fn run(arguments: &[std::ffi::OsString]) -> Result<StageResult> {
    if arguments.len() != 5 {
        return Err("bootstrap_arguments_invalid");
    }
    let mut request = Request {
        action: arguments[3]
            .to_str()
            .ok_or("restore_action_invalid")?
            .into(),
        id: arguments[4]
            .to_str()
            .ok_or("restore_action_invalid")?
            .into(),
    };
    if !request.validate() {
        return Err("restore_action_invalid");
    }
    let root = PathBuf::from(&arguments[1]);
    let payload = PathBuf::from(&arguments[2]);
    let image = std::env::current_exe().map_err(|_| "bootstrap_identity_unavailable")?;
    let mut began = std::time::Instant::now();
    loop {
        let result = match request.action.as_str() {
            "snapshot" => snapshot_install(&root, &payload, &image, false),
            "restore" => prepare_data_restore(&root, &payload, &image, &request.id),
            "resume" => {
                data_restore::execute(&root, &payload, &image, &request.id, "--apply-data-restore")
            }
            "commit" => data_restore::execute(
                &root,
                &payload,
                &image,
                &request.id,
                "--commit-data-restore",
            ),
            "rollback" => data_restore::execute(
                &root,
                &payload,
                &image,
                &request.id,
                "--rollback-data-restore",
            ),
            _ => return Err("restore_action_invalid"),
        };
        match result {
            Ok(result) if request.action == "restore" => {
                request.id = result.operation_id.ok_or("restore_operation_invalid")?;
                request.action = "resume".into();
            }
            Ok(result) => {
                open_install(&root, &payload, &image, "control-center")?;
                return Ok(result);
            }
            Err("suite_writers_must_close")
                if began.elapsed() < std::time::Duration::from_secs(60) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            Err(issue) => {
                let title = "Devbox 데이터 복구\0".encode_utf16().collect::<Vec<_>>();
                let message = format!("작업을 완료하지 못했습니다 ({issue}).\n네 제품을 모두 닫은 뒤 다시 시도하세요. 실행 중인 사용자 작업은 자동 종료하지 않습니다.\n취소해도 원본과 복원 데이터는 보존되며, 부분 복원이 있으면 제품 실행이 계속 차단됩니다.\n복구 작업: {}\0", request.id).encode_utf16().collect::<Vec<_>>();
                let answer = unsafe {
                    MessageBoxW(
                        None,
                        PCWSTR(message.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        MB_RETRYCANCEL | MB_ICONEXCLAMATION,
                    )
                };
                if answer == IDCANCEL {
                    if matches!(fs::symlink_metadata(root.join("suite-data-restore.block")), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
                    {
                        let _ = open_install(&root, &payload, &image, "control-center");
                    }
                    return Err(issue);
                }
                began = std::time::Instant::now();
            }
        }
    }
}
