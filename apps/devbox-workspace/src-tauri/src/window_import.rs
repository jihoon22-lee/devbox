//! Explicit primary-window mapping and recovery. Legacy bytes remain in the
//! verified snapshot; current native geometry is preserved before OS changes.
use crate::{definitions::digest, host::Host, private_metadata::MetadataRoot};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use window_state::WindowState;
use window_state_tauri::MainWindowReview;
type Result<T> = std::result::Result<T, &'static str>;

pub(crate) trait Geometry {
    fn review(&self, saved: &WindowState) -> Result<MainWindowReview>;
    fn apply(&self, review: &MainWindowReview) -> Result<()>;
}
struct Pending {
    review: MainWindowReview,
    data: MetadataRoot,
    history: MetadataRoot,
    created: Instant,
}
#[derive(Default)]
pub(crate) struct WindowImports {
    pending: HashMap<String, Pending>,
}
fn history_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn read_history(history: &MetadataRoot, id: &str) -> Result<WindowState> {
    if !history_id(id) {
        return Err("invalid_request");
    }
    let bytes = history
        .read(&format!("{id}.json"))?
        .ok_or("window_history_unavailable")?;
    if digest(&bytes) != id {
        return Err("files_store_changed");
    }
    window_state::decode_state(&bytes).map_err(|_| "window_state_invalid")
}
fn roots(host: &Host) -> Result<(MetadataRoot, MetadataRoot)> {
    let data = MetadataRoot::open(&host.component("overview")?)?;
    let history = data.child("window-history")?;
    Ok((data, history))
}
fn validate(host: &Host, data: &MetadataRoot, history: &MetadataRoot) -> Result<()> {
    if host.component("overview")? != data.path() {
        return Err("store_generation_changed");
    }
    data.revalidate()?;
    history.revalidate()
}
impl WindowImports {
    pub(crate) fn allowed(method: &str) -> bool {
        matches!(
            method,
            "preview_window_import"
                | "preview_window_restore"
                | "apply_window_import"
                | "cancel_window_import"
                | "list_window_history"
        )
    }
    pub(crate) fn dispatch(
        &mut self,
        host: &Host,
        geometry: &impl Geometry,
        method: &str,
        args: Value,
        deadline: u64,
    ) -> Result<Value> {
        crate::files_host::current_deadline(deadline)?;
        self.pending
            .retain(|_, pending| pending.created.elapsed() < Duration::from_secs(180));
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Job {
            job_id: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Backup {
            backup_id: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Token {
            preview_id: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Apply {
            preview_id: String,
            replace_existing: bool,
        }
        match method {
            "preview_window_import" | "preview_window_restore" => {
                if self.pending.len() >= 4 {
                    return Err("window_review_limit");
                }
                let (data, history) = roots(host)?;
                let (source_id, saved) = if method == "preview_window_import" {
                    let Job { job_id } =
                        serde_json::from_value(args).map_err(|_| "invalid_request")?;
                    host.legacy.window_source(&job_id)?
                } else {
                    let Backup { backup_id } =
                        serde_json::from_value(args).map_err(|_| "invalid_request")?;
                    let state = read_history(&history, &backup_id)?;
                    (backup_id, state)
                };
                let review = geometry.review(&saved)?;
                validate(host, &data, &history)?;
                crate::files_host::current_deadline(deadline)?;
                let id = uuid::Uuid::new_v4().to_string();
                let result = json!({"previewId":id,"before":review.before,"after":review.after,"source":saved,"sourceId":source_id,"restoring":method=="preview_window_restore"});
                self.pending.insert(
                    id,
                    Pending {
                        review,
                        data,
                        history,
                        created: Instant::now(),
                    },
                );
                Ok(result)
            }
            "cancel_window_import" => {
                let Token { preview_id } =
                    serde_json::from_value(args).map_err(|_| "invalid_request")?;
                self.pending
                    .remove(&preview_id)
                    .ok_or("window_review_stale")?;
                Ok(Value::Null)
            }
            "apply_window_import" => {
                let Apply {
                    preview_id,
                    replace_existing,
                } = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                let pending = self
                    .pending
                    .remove(&preview_id)
                    .ok_or("window_review_stale")?;
                if !replace_existing {
                    return Err("window_replace_required");
                }
                validate(host, &pending.data, &pending.history)?;
                let before = pending
                    .review
                    .before
                    .to_bytes()
                    .map_err(|_| "window_state_invalid")?;
                let id = digest(&before);
                let name = format!("{id}.json");
                if pending.history.read(&name)?.is_none()
                    && std::fs::read_dir(pending.history.path())
                        .map_err(|_| "window_history_unavailable")?
                        .take(32)
                        .count()
                        >= 32
                {
                    return Err("window_history_limit");
                }
                pending.history.preserve(&name, &before)?;
                validate(host, &pending.data, &pending.history)?;
                crate::files_host::current_deadline(deadline)?;
                geometry.apply(&pending.review)?;
                Ok(json!({"backupId":id}))
            }
            "list_window_history" => {
                if args != json!({}) {
                    return Err("invalid_request");
                }
                let (data, history) = roots(host)?;
                let mut items = vec![];
                let mut unrecognized = 0;
                for entry in std::fs::read_dir(history.path())
                    .map_err(|_| "window_history_unavailable")?
                    .take(33)
                {
                    let name = entry
                        .map_err(|_| "window_history_unavailable")?
                        .file_name()
                        .to_string_lossy()
                        .into_owned();
                    let Some(id) = name.strip_suffix(".json").filter(|id| history_id(id)) else {
                        unrecognized += 1;
                        continue;
                    };
                    match read_history(&history, id) {
                        Ok(state) => items.push(json!({"id":id,"state":state,"issue":null})),
                        Err(issue) => items.push(json!({"id":id,"state":null,"issue":issue})),
                    }
                }
                items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
                validate(host, &data, &history)?;
                Ok(json!({"items":items,"unrecognized":unrecognized}))
            }
            _ => Err("invalid_request"),
        }
    }
}

/// Main-thread callbacks contain only native window operations. The bounded
/// worker owns history IO and retains its admission until this bridge returns.
pub(crate) struct NativeGeometry {
    pub window: tauri::WebviewWindow,
    pub deadline: u64,
}
impl NativeGeometry {
    fn main<T: Send + 'static>(
        &self,
        action: impl FnOnce(&tauri::WebviewWindow, &dyn Fn() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    ) -> Result<T> {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let window = self.window.clone();
        let deadline = self.deadline;
        self.window
            .run_on_main_thread(move || {
                let check = || {
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Err("window_review_stale");
                    }
                    crate::files_host::current_deadline(deadline)
                };
                let result = check().and_then(|()| action(&window, &check));
                let _ = sender.send(result);
            })
            .map_err(|_| "window_state_unavailable")?;
        let result = receiver.recv_timeout(Duration::from_secs(10));
        cancelled.store(true, Ordering::Release);
        result.map_err(|_| "window_state_unavailable")?
    }
}
impl Geometry for NativeGeometry {
    fn review(&self, saved: &WindowState) -> Result<MainWindowReview> {
        let saved = saved.clone();
        self.main(move |window, _| window_state_tauri::review_main_window(window, &saved))
    }
    fn apply(&self, review: &MainWindowReview) -> Result<()> {
        let review = review.clone();
        self.main(move |window, check| {
            window_state_tauri::apply_main_window_review(window, &review, check)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use window_state::{MonitorId, MonitorInfo, RestoreConfig, WindowBounds};
    struct Desktop {
        current: RefCell<WindowState>,
        monitor: RefCell<MonitorInfo>,
        fail: bool,
    }
    impl Geometry for Desktop {
        fn review(&self, saved: &WindowState) -> Result<MainWindowReview> {
            MainWindowReview::new(
                saved,
                self.current.borrow().clone(),
                vec![self.monitor.borrow().clone()],
                RestoreConfig::default(),
            )
        }
        fn apply(&self, review: &MainWindowReview) -> Result<()> {
            review.revalidate(
                &self.current.borrow(),
                std::slice::from_ref(&*self.monitor.borrow()),
            )?;
            *self.current.borrow_mut() = review.after.clone();
            if self.fail {
                Err("window_apply_failed")
            } else {
                Ok(())
            }
        }
    }
    fn desktop() -> Desktop {
        let monitor = MonitorInfo::new(
            MonitorId::new("current").unwrap(),
            WindowBounds::new(0, 0, 1920, 1040),
            1.0,
            true,
        )
        .unwrap();
        let current =
            WindowState::capture(WindowBounds::new(20, 30, 1180, 780), &monitor, false).unwrap();
        Desktop {
            current: RefCell::new(current),
            monitor: RefCell::new(monitor),
            fail: false,
        }
    }
    fn fixture() -> (tempfile::TempDir, Host, String, Vec<u8>) {
        let base = tempfile::tempdir().unwrap();
        let data = base.path().join("workspace");
        std::fs::create_dir(&data).unwrap();
        let source = base
            .path()
            .join(crate::core::legacy_inventory::Source::RepoManager.identifier());
        std::fs::create_dir(&source).unwrap();
        let saved = WindowState::new(
            MonitorId::new("unavailable old display").unwrap(),
            WindowBounds::new(70, 80, 900, 650),
            WindowBounds::new(0, 0, 1920, 1040),
            1.0,
            false,
        )
        .unwrap()
        .to_bytes()
        .unwrap();
        std::fs::write(source.join("window-state-v1.json"), &saved).unwrap();
        let host = Host::open(&data).unwrap();
        host.start_empty().unwrap();
        let job = json!(host
            .legacy
            .start(crate::core::legacy_inventory::Source::RepoManager)
            .unwrap());
        let started = Instant::now();
        while json!(host.legacy.status().unwrap())["phase"] != "ready" {
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            std::fs::read(source.join("window-state-v1.json")).unwrap(),
            saved
        );
        std::fs::remove_file(source.join("window-state-v1.json")).unwrap();
        std::fs::remove_dir(source).unwrap();
        (base, host, job["id"].as_str().unwrap().into(), saved)
    }
    fn preview(owner: &mut WindowImports, host: &Host, desktop: &Desktop, job: &str) -> Value {
        owner
            .dispatch(
                host,
                desktop,
                "preview_window_import",
                json!({"jobId":job}),
                u64::MAX,
            )
            .unwrap()
    }
    fn apply(
        owner: &mut WindowImports,
        host: &Host,
        desktop: &Desktop,
        preview: &Value,
    ) -> Result<Value> {
        owner.dispatch(
            host,
            desktop,
            "apply_window_import",
            json!({"previewId":preview["previewId"],"replaceExisting":true}),
            u64::MAX,
        )
    }
    #[test]
    fn snapshot_without_source_replays_and_explicit_restore_preserve_before_state_and_registry() {
        let (_base, host, job, bytes) = fixture();
        let desktop = desktop();
        let mut owner = WindowImports::default();
        let registry = host.projects().unwrap().snapshot().unwrap();
        let before = desktop.current.borrow().clone();
        let review = preview(&mut owner, &host, &desktop, &job);
        assert_eq!(
            review["source"],
            json!(window_state::decode_state(&bytes).unwrap())
        );
        assert_eq!(*desktop.current.borrow(), before);
        let result = apply(&mut owner, &host, &desktop, &review).unwrap();
        assert_eq!(json!(*desktop.current.borrow()), review["after"]);
        assert_eq!(
            apply(&mut owner, &host, &desktop, &review),
            Err("window_review_stale")
        );
        let (_, history) = roots(&host).unwrap();
        assert_eq!(
            read_history(&history, result["backupId"].as_str().unwrap()).unwrap(),
            before
        );
        let repeated = preview(&mut owner, &host, &desktop, &job);
        owner
            .dispatch(
                &host,
                &desktop,
                "cancel_window_import",
                json!({"previewId":repeated["previewId"]}),
                u64::MAX,
            )
            .unwrap();
        assert_eq!(
            apply(&mut owner, &host, &desktop, &repeated),
            Err("window_review_stale")
        );
        let restore = owner
            .dispatch(
                &host,
                &desktop,
                "preview_window_restore",
                json!({"backupId":result["backupId"]}),
                u64::MAX,
            )
            .unwrap();
        apply(&mut owner, &host, &desktop, &restore).unwrap();
        assert_eq!(*desktop.current.borrow(), before);
        assert_eq!(host.projects().unwrap().snapshot().unwrap(), registry);
        assert_eq!(
            host.legacy
                .window_source(&job)
                .unwrap()
                .1
                .to_bytes()
                .unwrap(),
            bytes
        );
    }
    #[test]
    fn stale_geometry_deadlines_unconfirmed_and_expired_tokens_never_apply() {
        let (_base, host, job, _) = fixture();
        let desktop = desktop();
        let mut owner = WindowImports::default();
        let p = preview(&mut owner, &host, &desktop, &job);
        assert_eq!(
            owner.dispatch(
                &host,
                &desktop,
                "apply_window_import",
                json!({"previewId":p["previewId"],"replaceExisting":false}),
                u64::MAX
            ),
            Err("window_replace_required")
        );
        assert_eq!(
            apply(&mut owner, &host, &desktop, &p),
            Err("window_review_stale")
        );
        let p = preview(&mut owner, &host, &desktop, &job);
        desktop.current.borrow_mut().bounds.x += 10;
        let moved = desktop.current.borrow().clone();
        assert_eq!(
            apply(&mut owner, &host, &desktop, &p),
            Err("window_review_stale")
        );
        assert_eq!(*desktop.current.borrow(), moved);
        let p = preview(&mut owner, &host, &desktop, &job);
        owner
            .pending
            .get_mut(p["previewId"].as_str().unwrap())
            .unwrap()
            .created = Instant::now() - Duration::from_secs(181);
        assert_eq!(
            apply(&mut owner, &host, &desktop, &p),
            Err("window_review_stale")
        );
        assert!(owner
            .dispatch(
                &host,
                &desktop,
                "preview_window_import",
                json!({"jobId":job}),
                0
            )
            .is_err());
        assert!(owner
            .dispatch(
                &host,
                &desktop,
                "preview_window_import",
                json!({"jobId":job,"state":moved}),
                u64::MAX
            )
            .is_err());
        assert_eq!(
            owner.dispatch(
                &host,
                &desktop,
                "preview_window_import",
                json!({"jobId":"foreign"}),
                u64::MAX
            ),
            Err("legacy_import_stale")
        );
        assert_eq!(*desktop.current.borrow(), moved);
    }
    #[test]
    fn partial_native_failure_retains_recovery_and_tampered_history_is_never_applied() {
        let (_base, host, job, _) = fixture();
        let mut desktop = desktop();
        desktop.fail = true;
        let mut owner = WindowImports::default();
        let before = desktop.current.borrow().clone();
        let p = preview(&mut owner, &host, &desktop, &job);
        assert_eq!(
            apply(&mut owner, &host, &desktop, &p),
            Err("window_apply_failed")
        );
        let id = digest(&before.to_bytes().unwrap());
        let (_, history) = roots(&host).unwrap();
        assert_eq!(read_history(&history, &id).unwrap(), before);
        history
            .write(&format!("{id}.json"), b"corrupt evidence")
            .unwrap();
        assert_eq!(
            owner.dispatch(
                &host,
                &desktop,
                "preview_window_restore",
                json!({"backupId":id}),
                u64::MAX
            ),
            Err("files_store_changed")
        );
        let list = owner
            .dispatch(&host, &desktop, "list_window_history", json!({}), u64::MAX)
            .unwrap();
        assert_eq!(list["items"][0]["issue"], "files_store_changed");
        assert_eq!(
            history.read(&format!("{id}.json")).unwrap().unwrap(),
            b"corrupt evidence"
        );
    }
}
