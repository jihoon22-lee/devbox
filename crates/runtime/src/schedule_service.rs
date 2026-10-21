use crate::{schedule_model::ScheduleClaim, service::project_error, *};
use chrono::{DateTime, Local, Utc};
use devbox_core::notify::{Notification, NotificationReason, raise};
use devbox_protocol::Internal;

pub(crate) fn next(def: &ScheduleDef, after: i64) -> R<Option<i64>> {
    let after =
        DateTime::<Utc>::from_timestamp_millis(after).ok_or_else(|| RuntimeError::DefInvalid {
            id: def.id.clone(),
            reason: "invalid schedule time".into(),
        })?;
    let result = if let Some(tz) = &def.tz {
        let tz: chrono_tz::Tz = tz.parse().map_err(|_| RuntimeError::DefInvalid {
            id: def.id.clone(),
            reason: "invalid timezone".into(),
        })?;
        cron::next_after(&def.cron, &tz, after)
    } else {
        cron::next_after(&def.cron, &Local, after)
    };
    result
        .map(|date| date.map(|date| date.timestamp_millis()))
        .map_err(|_| {
            RuntimeError::DefInvalid {
                id: def.id.clone(),
                reason: "invalid cron occurrence".into(),
            }
            .into()
        })
}
pub(crate) fn timestamp(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .expect("validated schedule time")
        .to_rfc3339()
}
impl RuntimeService {
    pub(crate) async fn remove_schedule_state(&self, project: &str, id: &str) -> R<()> {
        let project = project.to_owned();
        let id = id.to_owned();
        self.db
            .write(move |tx| schedule_store::remove(tx, &project, &id))
            .await
            .internal("remove schedule cursor")?;
        Ok(())
    }

    pub async fn schedules(&self, p: SchedulesQuery) -> R<SchedulesResult> {
        let state = self
            .projects
            .run_config_status(&p.project_id)
            .await
            .map_err(project_error)?;
        let (local, _) = self.local(&p.project_id).await?;
        let merged = merge(state.config.definitions(), local)?;
        let mut items = Vec::new();
        for definition in merged.defs {
            let encoded = serde_json::to_string(&definition).internal("encode current schedule")?;
            if let RunDef::Schedule(def) = definition.def {
                let project = p.project_id.clone();
                let id = def.id.clone();
                let (state, last) = self
                    .db
                    .read(move |tx| {
                        Ok((
                            schedule_store::state(tx, &project, &id)?,
                            schedule_store::last(tx, &project, &id)?,
                        ))
                    })
                    .await
                    .internal("read schedule status")?;
                let now = Utc::now().timestamp_millis();
                let cursor = state
                    .filter(|s| s.definition == encoded)
                    .map_or(now, |s| s.cursor.max(now));
                items.push(ScheduleStatus {
                    id: def.id.clone(),
                    cron: def.cron.clone(),
                    next: next(&def, cursor)?.map(timestamp),
                    last: last.map(|(at, result)| ScheduleLast {
                        at: timestamp(at),
                        result,
                    }),
                });
            }
        }
        Ok(SchedulesResult { items })
    }
    pub(crate) async fn finish_schedule(
        &self,
        claim: ScheduleClaim,
        outcome: ScheduleOutcome,
        run: Option<String>,
    ) -> R<()> {
        let event_claim = claim.clone();
        let event_run = run.clone();
        let hub = self.hub.clone();
        self.db
            .write_checked_with_commit(
                move |tx| {
                    Ok(Ok::<_, RuntimeError>(schedule_store::finish(
                        tx,
                        &claim,
                        outcome,
                        run.as_deref(),
                    )?))
                },
                move |(changed, alert), rev| {
                    if *alert {
                        schedule_alert(&hub, rev, &event_claim, outcome, event_run);
                    }
                    if *changed {
                        hub.publish::<RuntimeChanged>(
                            rev,
                            &RuntimeChange {
                                rev: rev.to_string(),
                                project_id: event_claim.project_id,
                            },
                        );
                    }
                },
            )
            .await
            .internal("record schedule result")??;
        Ok(())
    }
}
pub(crate) fn schedule_alert(
    hub: &devbox_core::Hub,
    rev: u64,
    claim: &ScheduleClaim,
    outcome: ScheduleOutcome,
    run_id: Option<String>,
) {
    raise(
        hub,
        rev,
        &Notification {
            id: format!(
                "schedule-{}-{}-{}",
                claim.project_id, claim.schedule_id, claim.at
            ),
            project_id: claim.project_id.clone(),
            task: Some(claim.task.clone()),
            run_id,
            reason: if outcome == ScheduleOutcome::SkippedTrust {
                NotificationReason::ScheduleTrustRequired
            } else {
                NotificationReason::ScheduleFailed
            },
        },
    );
}
