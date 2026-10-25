use crate::{
    exit_record::{ExitStore, Exited},
    *,
};
use devbox_core::{
    notify::{Notification, NotificationReason, raise},
    systemd::Systemd,
};
use devbox_protocol::{Done, Internal};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

impl RuntimeService {
    pub async fn exited(&self, p: Exited) -> R<Done> {
        exit_record::validate_identity(&p.run_id).internal("validate exit run identity")?;
        exit_record::validate_identity(&p.attempt_id).internal("validate exit attempt identity")?;
        let paths = &self.execution()?.paths;
        let store = ExitStore::new(&paths.data_dir.join("runs"), &p.run_id)
            .internal("resolve exit result")?;
        // The notification cannot supply or replace the exit code. Late invocations are ignored.
        if store
            .current_attempt()
            .internal("read current invocation")?
            .as_deref()
            == Some(&p.attempt_id)
        {
            let run = self.reconcile_one(&p.run_id).await?;
            if run.windows.is_some() {
                let value = self.stored(&p.run_id).await?;
                if let Some(reference) = value.windows.as_ref().and_then(|w| w.copy.as_ref()) {
                    self.reconcile_windows_target(&reference.key).await?;
                }
            }
        }
        Ok(Done {
            rev: self.db.rev().to_string(),
        })
    }

    pub(crate) async fn reconcile_one(&self, id: &str) -> R<Run> {
        let _guard = self.run_lock(id).await;
        let mut value = self.stored(id).await?;
        let before = value.clone();
        let paths = &self.execution()?.paths;
        let store = ExitStore::new(&paths.data_dir.join("runs"), id)
            .internal("resolve execution result")?;
        let unit = Systemd
            .show(&value.run.unit)
            .await
            .internal("inspect execution unit")?;
        let current = store.current_attempt().internal("read execution attempt")?;
        let record = store.read().internal("read execution exit")?.filter(|r| {
            Some(&r.attempt_id) == current.as_ref() && r.ended_ms >= value.run.started_ms
        });
        let active = matches!(
            unit.active_state.as_str(),
            "active" | "activating" | "deactivating"
        );
        let attempt = if active {
            unit.invocation_id.clone().or(current)
        } else {
            current
        };
        let record = record.filter(|r| Some(&r.attempt_id) == attempt.as_ref());
        let new_attempt = value.run.attempt_id != attempt;
        value.run.attempt_id = attempt;
        value.run.pid = unit.main_pid;
        if unit.load_state != "not-found" {
            value.run.restarts = unit.n_restarts;
        }
        if new_attempt {
            value.run.ready = false;
        }
        let hit_limit = start_limit_hit(
            &unit,
            value.restart,
            value.start_limit.as_ref(),
            record.as_ref(),
        );
        if value.run.state != RunState::Stopped {
            let restart_pending = value.run.kind == RunKind::Service
                && !hit_limit
                && (unit.sub_state.starts_with("auto-restart")
                    || active
                        && record.as_ref().is_some_and(|exit| match value.restart {
                            RestartPolicy::No => false,
                            RestartPolicy::Always => true,
                            RestartPolicy::OnFailure => {
                                exit.code != Some(0) || exit.signal.is_some()
                            }
                        }));
            if hit_limit || unit.active_state == "failed" {
                value.run.state = RunState::Failed;
                value.run.ended_ms = Some(
                    record
                        .as_ref()
                        .map_or_else(exit_record::now_ms, |r| r.ended_ms),
                );
            } else if restart_pending {
                value.run.state = RunState::Restarting;
                value.run.ended_ms = None;
                value.run.ready = false;
            } else if let Some(exit) = &record {
                value.run.state = if exit.code == Some(0) && exit.signal.is_none() {
                    RunState::Exited
                } else {
                    RunState::Failed
                };
                value.run.ended_ms = Some(exit.ended_ms);
            } else if active {
                value.run.state = RunState::Running;
                value.run.ended_ms = None;
            } else {
                value.run.state = RunState::Failed;
                value.run.ended_ms.get_or_insert_with(exit_record::now_ms);
            }
            value.run.exit = record;
        }
        if !value.run.state.is_live() {
            value.run.ready = false;
            value.run.pid = None;
        }
        let alert = hit_limit && !value.start_limit_notified;
        if alert {
            value.start_limit_notified = true;
        }
        if value != before {
            let rev = self.save_run(&value).await?;
            if alert {
                raise(
                    &self.hub,
                    rev,
                    &Notification {
                        id: format!("run-{}-start-limit-{}", value.run.id, value.run.started_ms),
                        project_id: value.run.project_id.clone(),
                        run_id: Some(value.run.id.clone()),
                        task: None,
                        reason: NotificationReason::StartLimit,
                    },
                );
            }
        }
        Ok(value.run)
    }
    async fn tracked_runs(&self) -> R<Vec<run_store::StoredRun>> {
        self.db
            .read(|tx| run_store::list(tx, None, u32::MAX))
            .await
            .internal("read tracked executions")
    }
    pub(crate) async fn initialize_runs(self: &Arc<Self>) -> R<()> {
        if self.execution.is_none() {
            return Ok(());
        }
        self.reconcile_windows_copies().await?;
        let runs = self.tracked_runs().await?;
        if !runs.is_empty() {
            let pattern = format!("devbox-{}-run-*", self.execution()?.paths.instance.as_str());
            let units = Systemd
                .list_units(&pattern)
                .await
                .internal("list owned execution units")?;
            for value in runs {
                if value.run.state.is_live() || units.contains(&value.run.unit) {
                    self.reconcile_one(&value.run.id).await?;
                }
            }
        }
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let service = self.clone();
        let mut changes = self.hub.subscribe("runtime.changed");
        let join = tokio::spawn(async move {
            let mut next_poll = None;
            loop {
                let has_services = match service.tracked_runs().await {
                    Ok(runs) => runs
                        .iter()
                        .any(|v| v.run.kind == RunKind::Service && v.run.state.is_live()),
                    Err(_) => false,
                };
                let has_windows = service
                    .db
                    .read(crate::windows_copy_store::needs_reconcile)
                    .await
                    .unwrap_or(true);
                let needs_poll = has_services || has_windows;
                let deadline =
                    polling_deadline(&mut next_poll, needs_poll, tokio::time::Instant::now())
                        .unwrap_or_else(|| {
                            tokio::time::Instant::now() + Duration::from_secs(86400)
                        });
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    event = changes.recv() => { if event.is_none() { break; } },
                    _ = tokio::time::sleep_until(deadline), if needs_poll => {
                        next_poll = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                        if service.reconcile_windows_copies().await.is_err() {tracing::warn!("Windows copy reconciliation unavailable");}
                        if let Ok(runs) = service.tracked_runs().await {
                            for value in runs {
                                if value.run.kind == RunKind::Service && value.run.state.is_live()
                                    && service.reconcile_one(&value.run.id).await.is_err()
                                { tracing::warn!("service state reconciliation failed"); }
                            }
                        }
                    }
                }
            }
        });
        *self.run_watcher.lock().await = Some(crate::watch::WatchHandle { cancel, join });
        Ok(())
    }
}

fn polling_deadline(
    next: &mut Option<tokio::time::Instant>,
    active: bool,
    now: tokio::time::Instant,
) -> Option<tokio::time::Instant> {
    if active {
        next.get_or_insert(now + Duration::from_secs(10));
    } else {
        *next = None;
    }
    *next
}

fn start_limit_hit(
    unit: &devbox_core::systemd::UnitState,
    policy: RestartPolicy,
    limit: Option<&StartLimit>,
    exit: Option<&exit_record::ExitRecord>,
) -> bool {
    if unit.result == "start-limit-hit" {
        return true;
    }
    let Some(limit) = limit else {
        return false;
    };
    if limit.burst == 0
        || limit.interval_sec == 0
        || unit.n_restarts < limit.burst
        || unit.active_state != "failed"
        || unit.sub_state != "failed"
    {
        return false;
    }
    // systemd can retain the previous invocation's Result when the automatic
    // restart job hits its start limit (systemd/systemd#32203).
    match unit.result.as_str() {
        "success" => {
            policy == RestartPolicy::Always
                && exit.is_some_and(|e| e.code == Some(0) && e.signal.is_none())
        }
        "exit-code" => {
            policy != RestartPolicy::No && exit.is_some_and(|e| e.code.is_some_and(|c| c != 0))
        }
        "signal" | "core-dump" => {
            policy != RestartPolicy::No && exit.is_some_and(|e| e.signal.is_some())
        }
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn start_limit_fallback_requires_a_restartable_failure_and_enabled_limit() {
        let mut unit = devbox_core::systemd::UnitState {
            load_state: "loaded".into(),
            active_state: "failed".into(),
            sub_state: "failed".into(),
            result: "exit-code".into(),
            n_restarts: 3,
            main_pid: None,
            invocation_id: Some("attempt".into()),
        };
        let mut limit = StartLimit {
            burst: 3,
            interval_sec: 60,
        };
        let exit = exit_record::ExitRecord {
            attempt_id: "attempt".into(),
            code: Some(1),
            signal: None,
            ended_ms: 1,
            by: exit_record::ExitSource::Wrapper,
            service_result: None,
        };
        assert!(start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            Some(&exit)
        ));
        assert!(!start_limit_hit(
            &unit,
            RestartPolicy::No,
            Some(&limit),
            Some(&exit)
        ));
        assert!(!start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            None
        ));
        limit.interval_sec = 0;
        assert!(!start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            Some(&exit)
        ));
        limit.interval_sec = 60;
        unit.n_restarts = 2;
        assert!(!start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            Some(&exit)
        ));
        unit.n_restarts = 3;
        unit.active_state = "activating".into();
        unit.sub_state = "auto-restart".into();
        assert!(!start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            Some(&exit)
        ));
        unit.active_state = "failed".into();
        unit.sub_state = "failed".into();
        unit.result = "resources".into();
        assert!(!start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            Some(&exit)
        ));
        unit.result = "start-limit-hit".into();
        assert!(start_limit_hit(
            &unit,
            RestartPolicy::OnFailure,
            Some(&limit),
            None
        ));
    }
    #[test]
    fn service_observation_deadline_survives_frequent_run_changes() {
        let now = tokio::time::Instant::now();
        let mut next = None;
        for seconds in 0..10 {
            assert_eq!(
                polling_deadline(&mut next, true, now + Duration::from_secs(seconds)),
                Some(now + Duration::from_secs(10))
            );
        }
        assert_eq!(
            polling_deadline(&mut next, false, now + Duration::from_secs(11)),
            None
        );
        assert_eq!(
            polling_deadline(&mut next, true, now + Duration::from_secs(100)),
            Some(now + Duration::from_secs(110))
        );
    }
}
