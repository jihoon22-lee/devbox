use crate::{ScheduleOutcome, schedule_model::ScheduleClaim};
use rusqlite::{OptionalExtension, Transaction, params};

pub const MIGRATION_3: &str = "
CREATE TABLE runtime_schedule_state (
 project_id TEXT NOT NULL REFERENCES projects_projects(id) ON DELETE CASCADE,
 id TEXT NOT NULL, definition TEXT NOT NULL, cursor_ms INTEGER NOT NULL,
 trust_notified INTEGER NOT NULL DEFAULT 0, approval_stamp TEXT NOT NULL, PRIMARY KEY(project_id,id)
);
CREATE TABLE runtime_schedule_claims (
 project_id TEXT NOT NULL REFERENCES projects_projects(id) ON DELETE CASCADE,
 schedule_id TEXT NOT NULL, at_ms INTEGER NOT NULL, task TEXT NOT NULL,
 result TEXT NOT NULL, run_id TEXT, notified INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(project_id,schedule_id,at_ms)
);";
pub(crate) struct State {
    pub definition: String,
    pub cursor: i64,
    pub trust_notified: bool,
    pub approval_stamp: String,
}
pub(crate) fn state(
    tx: &Transaction<'_>,
    project: &str,
    id: &str,
) -> rusqlite::Result<Option<State>> {
    tx.query_row("SELECT definition,cursor_ms,trust_notified,approval_stamp FROM runtime_schedule_state WHERE project_id=?1 AND id=?2", params![project,id], |r| Ok(State { definition:r.get(0)?, cursor:r.get(1)?, trust_notified:r.get(2)?, approval_stamp:r.get(3)? })).optional()
}
pub(crate) fn establish(
    tx: &Transaction<'_>,
    project: &str,
    id: &str,
    definition: &str,
    now: i64,
    approval_stamp: &str,
) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO runtime_schedule_state(project_id,id,definition,cursor_ms,approval_stamp) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id,id) DO UPDATE SET definition=excluded.definition,cursor_ms=excluded.cursor_ms,trust_notified=CASE WHEN runtime_schedule_state.approval_stamp=excluded.approval_stamp THEN runtime_schedule_state.trust_notified ELSE 0 END,approval_stamp=excluded.approval_stamp", params![project,id,definition,now,approval_stamp])?;
    Ok(())
}
pub(crate) fn claim(
    tx: &Transaction<'_>,
    claim: &ScheduleClaim,
    definition: &str,
    cursor: i64,
    through: i64,
    outcome: ScheduleOutcome,
) -> rusqlite::Result<bool> {
    let changed=tx.execute("UPDATE runtime_schedule_state SET cursor_ms=?1 WHERE project_id=?2 AND id=?3 AND definition=?4 AND cursor_ms=?5", params![through,claim.project_id,claim.schedule_id,definition,cursor])?;
    if changed == 0 {
        return Ok(false);
    }
    tx.execute("INSERT OR IGNORE INTO runtime_schedule_claims(project_id,schedule_id,at_ms,task,result) VALUES(?1,?2,?3,?4,?5)",params![claim.project_id,claim.schedule_id,claim.at,claim.task,encode(outcome)]) .map(|n| n != 0)
}
fn encode(outcome: ScheduleOutcome) -> String {
    serde_json::to_string(&outcome).expect("schedule result")
}
pub(crate) fn last(
    tx: &Transaction<'_>,
    project: &str,
    id: &str,
) -> rusqlite::Result<Option<(i64, ScheduleOutcome)>> {
    tx.query_row("SELECT at_ms,result FROM runtime_schedule_claims WHERE project_id=?1 AND schedule_id=?2 ORDER BY at_ms DESC LIMIT 1",params![project,id],|r| {
        let text:String=r.get(1)?;
        let outcome=serde_json::from_str(&text).map_err(|e| rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Text,Box::new(e)))?;
        Ok((r.get(0)?,outcome))
    }).optional()
}
/// Returns (changed, notify). The marker and result commit together.
/// Only claimed/running are mutable; success, failure, stopped and skips are terminal.
pub(crate) fn finish(
    tx: &Transaction<'_>,
    claim: &ScheduleClaim,
    outcome: ScheduleOutcome,
    run: Option<&str>,
) -> rusqlite::Result<(bool, bool)> {
    let changed=tx.execute("UPDATE runtime_schedule_claims SET result=?1,run_id=COALESCE(?2,run_id) WHERE project_id=?3 AND schedule_id=?4 AND at_ms=?5 AND result IN ('\"claimed\"','\"running\"')",params![encode(outcome),run,claim.project_id,claim.schedule_id,claim.at])?;
    if changed == 0 {
        return Ok((false, false));
    }
    if outcome == ScheduleOutcome::Failed {
        return tx.execute("UPDATE runtime_schedule_claims SET notified=1 WHERE project_id=?1 AND schedule_id=?2 AND at_ms=?3 AND notified=0",params![claim.project_id,claim.schedule_id,claim.at]).map(|n|(true,n!=0));
    }
    if outcome == ScheduleOutcome::SkippedTrust {
        return tx.execute("UPDATE runtime_schedule_state SET trust_notified=1 WHERE project_id=?1 AND id=?2 AND trust_notified=0",params![claim.project_id,claim.schedule_id]).map(|n|(true,n!=0));
    }
    Ok((true, false))
}
pub(crate) fn approved(
    tx: &Transaction<'_>,
    project: &str,
    id: &str,
    approval_stamp: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE runtime_schedule_state SET trust_notified=0,approval_stamp=?3 WHERE project_id=?1 AND id=?2",
        params![project, id, approval_stamp],
    )?;
    Ok(())
}

pub(crate) fn remove(tx: &Transaction<'_>, project: &str, id: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM runtime_schedule_state WHERE project_id=?1 AND id=?2",
        params![project, id],
    )?;
    Ok(())
}
pub(crate) fn ids(tx: &Transaction<'_>, project: &str) -> rusqlite::Result<Vec<String>> {
    tx.prepare("SELECT id FROM runtime_schedule_state WHERE project_id=?1")?
        .query_map([project], |r| r.get(0))?
        .collect()
}
