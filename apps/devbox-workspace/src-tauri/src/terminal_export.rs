//! Disposable export of seven fixed Terminal preferences from a closed profile copy.
use crate::private_metadata::MetadataRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewWindow};
const LABEL: &str = "legacy-terminal-export";
pub(crate) const KEYS: &[&str] = &[
    "wsl-desktop:cwd-pinned",
    "wsl-desktop:cwd-value",
    "wsl-desktop:recent-paths",
    "wsl-desktop:copy-on-select",
    "wsl-desktop:font-size",
    "wsl-desktop:settings",
    "wsl-desktop:last-layout",
];
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Export {
    pub schema_version: u32,
    pub values: BTreeMap<String, Option<String>>,
}
impl Export {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.values.len() != KEYS.len()
            || !KEYS.iter().all(|key| self.values.contains_key(*key))
            || self
                .values
                .values()
                .flatten()
                .any(|value| value.len() > 1024 * 1024)
            || self
                .values
                .values()
                .flatten()
                .map(String::len)
                .sum::<usize>()
                > 4 * 1024 * 1024
        {
            return Err("terminal_export_invalid");
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ticket {
    schema_version: u32,
    nonce: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Completed {
    schema_version: u32,
    nonce: String,
    data: Export,
}
fn nonce_valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
fn read<T: serde::de::DeserializeOwned>(
    root: &MetadataRoot,
    file: &str,
) -> Result<T, &'static str> {
    serde_json::from_slice(&root.read(file)?.ok_or("terminal_export_missing")?)
        .map_err(|_| "terminal_export_invalid")
}
#[cfg(windows)]
pub(crate) fn ticket(stage: &MetadataRoot, nonce: &str) -> Result<(), &'static str> {
    if !nonce_valid(nonce) {
        return Err("terminal_export_invalid");
    }
    stage.create_new(
        "worker-ticket.json",
        &serde_json::to_vec(&Ticket {
            schema_version: 1,
            nonce: nonce.into(),
        })
        .map_err(|_| "terminal_export_invalid")?,
    )
}
struct Worker {
    root: MetadataRoot,
    nonce: String,
    started: Instant,
    state: Mutex<(bool, bool)>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    nonce: String,
    action: Action,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Action {
    Open {},
    Complete { data: Export },
    Failed {},
}
fn local(url: &tauri::Url) -> bool {
    url.path() == "/terminal-export.html"
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
}
#[tauri::command]
pub(crate) async fn terminal_export_message(
    window: WebviewWindow,
    request: Request,
) -> Result<Value, String> {
    if window.label() != LABEL || !window.url().is_ok_and(|url| local(&url)) {
        return Err("terminal_export_caller_rejected".into());
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<Value, &'static str> {
        let worker = window
            .try_state::<Worker>()
            .ok_or("terminal_export_caller_rejected")?;
        if request.nonce != worker.nonce || worker.started.elapsed() > Duration::from_secs(60) {
            return Err("terminal_export_expired");
        }
        let mut state = worker.state.lock().map_err(|_| "terminal_export_busy")?;
        if state.1 {
            return Err("terminal_export_replayed");
        }
        if matches!(request.action, Action::Open {}) {
            return Ok(json!({"ready":state.0}));
        }
        if !state.0 {
            return Err("terminal_export_profile_unverified");
        }
        match request.action {
            Action::Complete { data } => {
                data.validate()?;
                worker.root.create_new(
                    "worker-result.json",
                    &serde_json::to_vec(&Completed {
                        schema_version: 1,
                        nonce: worker.nonce.clone(),
                        data,
                    })
                    .map_err(|_| "terminal_export_invalid")?,
                )?;
                state.1 = true;
                window.app_handle().exit(0);
                Ok(Value::Null)
            }
            Action::Failed {} => {
                state.1 = true;
                window.app_handle().exit(2);
                Ok(Value::Null)
            }
            Action::Open {} => unreachable!(),
        }
    })
    .await
    .map_err(|_| "terminal_export_failed".to_owned())?
    .map_err(str::to_owned)
}
pub(crate) fn argument(args: &[String]) -> Result<Option<String>, &'static str> {
    if !args
        .iter()
        .any(|arg| arg.starts_with("--terminal-export-worker"))
    {
        return Ok(None);
    }
    if args.len() != 2
        || args[0] != "--terminal-export-worker"
        || !uuid::Uuid::parse_str(&args[1]).is_ok_and(|id| id.to_string() == args[1])
    {
        return Err("terminal_export_arguments_invalid");
    }
    Ok(Some(args[1].clone()))
}
pub(crate) fn run_worker(
    stage_id: String,
    mut context: tauri::Context<tauri::Wry>,
) -> tauri::Result<()> {
    let _installation = product_shell_tauri::isolate_installation(&mut context)?;
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .plugin(tauri::plugin::Builder::<tauri::Wry, ()>::new("workspace").invoke_handler(tauri::generate_handler![terminal_export_message]).build())
        .setup(move |app| {
            let path = app.path().app_local_data_dir()?.join("terminal-imports").join(&stage_id);
            let root = MetadataRoot::open(&path).map_err(std::io::Error::other)?;
            let ticket: Ticket = read(&root, "worker-ticket.json").map_err(std::io::Error::other)?;
            if ticket.schema_version != 1 || !nonce_valid(&ticket.nonce) { return Err(std::io::Error::other("terminal_export_invalid").into()); }
            root.create_new("worker-started", b"started").map_err(std::io::Error::other)?;
            let copy = root.path().join("webview-copy");
            let script = format!("Object.defineProperty(window, '__DEVBOX_TERMINAL_EXPORT__', {{ value: {}, writable: false }});", serde_json::to_string(&ticket.nonce)?);
            app.manage(Worker { root, nonce: ticket.nonce, started: Instant::now(), state: Mutex::new((false, false)) });
            let window = tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("terminal-export.html".into()))
                .data_directory(copy.clone()).visible(false).skip_taskbar(true).focused(false)
                .initialization_script(script).on_navigation(local).build()?;
            let handle = app.handle().clone();
            crate::platform::browser_profile::verify_profile(&window, copy, move |result| {
                if result.is_err() { handle.exit(2); return; }
                if let Some(worker) = handle.try_state::<Worker>() { if let Ok(mut state) = worker.state.lock() { state.0 = true; } }
            })?;
            Ok(())
        }).run(context)
}
#[cfg(windows)]
pub(crate) async fn export(
    stage: &MetadataRoot,
    id: &str,
    nonce: &str,
    cancelled: &AtomicBool,
) -> Result<Export, &'static str> {
    use std::process::Stdio;
    stage.revalidate()?;
    let executable = std::env::current_exe().map_err(|_| "terminal_export_unavailable")?;
    let mut command = tokio::process::Command::new(executable);
    command.arg("--terminal-export-worker").arg(id).env_clear();
    for name in [
        "SystemRoot",
        "WINDIR",
        "LOCALAPPDATA",
        "APPDATA",
        "TEMP",
        "TMP",
        "USERPROFILE",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env(
            "WEBVIEW2_USER_DATA_FOLDER",
            stage.path().join("webview-copy"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(0x0000_0004 | 0x0800_0000);
    let mut child = command.spawn().map_err(|_| "terminal_export_unavailable")?;
    let mut tree = match crate::platform::owned_process::ProcessTree::assign(&child) {
        Ok(tree) => tree,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err("terminal_export_unavailable");
        }
    };
    let started = Instant::now();
    let result = loop {
        if cancelled.load(Ordering::Acquire) {
            break Err("terminal_import_cancelled");
        }
        if started.elapsed() > Duration::from_secs(60) {
            break Err("terminal_export_timeout");
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("terminal_export_failed")
                }
            }
            Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
            Err(_) => break Err("terminal_export_failed"),
        }
    };
    if !tree.terminate(&mut child).await {
        return Err("terminal_export_cleanup_pending");
    }
    stage.create_new("worker-retired", b"retired")?;
    result?;
    let completed: Completed = read(stage, "worker-result.json")?;
    if completed.schema_version != 1 || completed.nonce != nonce {
        return Err("terminal_export_invalid");
    }
    completed.data.validate()?;
    Ok(completed.data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_start_claim_is_not_an_idempotent_preservation_write() {
        let directory = tempfile::tempdir().unwrap();
        let root = MetadataRoot::open(directory.path()).unwrap();
        root.create_new("worker-started", b"started").unwrap();
        assert!(root.create_new("worker-started", b"started").is_err());
        assert_eq!(root.read("worker-started").unwrap().unwrap(), b"started");
    }
    #[test]
    fn worker_arguments_cannot_select_a_source_or_arbitrary_stage_path() {
        assert!(argument(&["--terminal-export-worker".into(), "../source".into()]).is_err());
        assert!(argument(&[
            "--terminal-export-worker".into(),
            uuid::Uuid::new_v4().to_string(),
            "extra".into()
        ])
        .is_err());
        assert_eq!(argument(&["--route=terminal".into()]).unwrap(), None);
    }
    #[test]
    fn export_rejects_unregistered_storage_keys_and_oversized_values() {
        let mut export = Export {
            schema_version: 1,
            values: KEYS.iter().map(|key| (key.to_string(), None)).collect(),
        };
        export.validate().unwrap();
        export
            .values
            .insert("foreign-secret-store".into(), Some("synthetic".into()));
        assert!(export.validate().is_err());
        export.values.remove("foreign-secret-store");
        export
            .values
            .insert(KEYS[0].into(), Some("x".repeat(1024 * 1024 + 1)));
        assert!(export.validate().is_err());
    }
}
