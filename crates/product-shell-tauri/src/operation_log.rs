//! Records one fixed-schema line per finished component call. Logging never
//! fails or delays a command; see `product_contract::operation_log`.
use product_contract::operation_log::{self, Entry, OperationLog, Outcome};
use product_contract::OperationState;
use std::sync::{Arc, Once};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{Manager, WebviewWindow};

pub(crate) struct Sink {
    log: Option<Arc<OperationLog>>,
    product: String,
    version: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

pub(crate) fn outcome_of(state: &OperationState) -> Outcome {
    match state {
        OperationState::Succeeded {} => Outcome::Succeeded,
        OperationState::Cancelled {} => Outcome::Cancelled,
        OperationState::Failed { .. } | OperationState::Stale {} | OperationState::Running {} => {
            Outcome::Failed
        }
    }
}

pub(crate) fn location_token(file: &str, line: u32) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("{name}:{line}")
}

/// Open `<app local data>/logs`, drop day files past retention and install
/// the panic hook. A missing or unwritable directory disables logging only.
pub(crate) fn initialize(app: &tauri::App, product: &str) {
    let version = app.package_info().version.to_string();
    let log = app
        .path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join("logs"))
        .and_then(|dir| {
            operation_log::prune(&dir, now_ms());
            OperationLog::open(dir).ok()
        })
        .map(Arc::new);
    if let Some(log) = &log {
        install_panic_hook(log.clone(), product.to_string(), version.clone());
    }
    app.manage(Sink {
        log,
        product: product.to_string(),
        version,
    });
}

fn install_panic_hook(log: Arc<OperationLog>, product: String, version: String) {
    static HOOK: Once = Once::new();
    HOOK.call_once(move || {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|location| location_token(location.file(), location.line()))
                .unwrap_or_else(|| "unknown".into());
            log.try_append(&Entry::new(
                now_ms(),
                &version,
                &product,
                "panic",
                &location,
                0,
                Outcome::Panicked,
                None,
            ));
            previous(info);
        }));
    });
}

#[must_use = "finish the guard with the operation outcome"]
pub struct OperationGuard {
    log: Option<Arc<OperationLog>>,
    product: String,
    version: String,
    component: String,
    method: String,
    started: Instant,
    finished: bool,
}

impl OperationGuard {
    pub(crate) fn start(sink: &Sink, component: &str, method: &str) -> Self {
        Self {
            log: sink.log.clone(),
            product: sink.product.clone(),
            version: sink.version.clone(),
            component: component.to_string(),
            method: method.to_string(),
            started: Instant::now(),
            finished: false,
        }
    }

    /// `failure` must be a host-projected issue code, never a raw error.
    pub fn finish(mut self, outcome: &OperationState, failure: Option<&str>) {
        self.finished = true;
        self.record(outcome_of(outcome), failure);
    }

    fn record(&self, outcome: Outcome, code: Option<&str>) {
        let Some(log) = &self.log else {
            return;
        };
        let duration_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if operation_log::should_record(outcome, duration_ms) {
            log.append(&Entry::new(
                now_ms(),
                &self.version,
                &self.product,
                &if self.finished { self.component.clone() } else { operation_log::opaque_token(&self.component) },
                &if self.finished { self.method.clone() } else { operation_log::opaque_token(&self.method) },
                duration_ms,
                outcome,
                code,
            ));
        }
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.record(Outcome::Rejected, None);
        }
    }
}

/// Start timing a component call. Early returns (validation, authorization,
/// replay, shutdown) drop the guard and are recorded as `rejected`.
pub fn begin_operation(window: &WebviewWindow, component: &str, method: &str) -> OperationGuard {
    match window.try_state::<Sink>() {
        Some(sink) => OperationGuard::start(&sink, component, method),
        None => OperationGuard::start(
            &Sink {
                log: None,
                product: String::new(),
                version: String::new(),
            },
            component,
            method,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::ProblemCode;

    #[test]
    fn operation_states_map_to_log_outcomes() {
        assert_eq!(outcome_of(&OperationState::Succeeded {}), Outcome::Succeeded);
        assert_eq!(outcome_of(&OperationState::Cancelled {}), Outcome::Cancelled);
        assert_eq!(
            outcome_of(&OperationState::Failed { code: ProblemCode::Unavailable }),
            Outcome::Failed
        );
        assert_eq!(outcome_of(&OperationState::Stale {}), Outcome::Failed);
    }

    #[test]
    fn panic_locations_keep_only_the_file_name() {
        assert_eq!(location_token("crates\\knowledge\\src\\component.rs", 212), "component.rs:212");
        assert_eq!(location_token("/home/runner/work/devbox/src/lib.rs", 9), "lib.rs:9");
    }

    #[test]
    fn a_dropped_guard_records_a_rejection_and_a_finished_one_does_not_double_count() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(OperationLog::open(dir.path().to_path_buf()).unwrap());
        let sink = Sink { log: Some(log), product: "knowledge".into(), version: "0.9.0".into() };
        drop(OperationGuard::start(&sink, "knowledge.notes", "write_file"));
        OperationGuard::start(&sink, "knowledge.notes", "read_file")
            .finish(&OperationState::Failed { code: product_contract::ProblemCode::Unavailable }, Some("vault_unavailable"));
        let summary = product_contract::operation_log::summarize(dir.path(), 10, 1 << 20);
        assert_eq!(summary.counts.rejected, 1);
        assert_eq!(summary.counts.failed, 1);
        assert_eq!(summary.recent[0].code.as_deref(), Some("vault_unavailable"));
    }
}
