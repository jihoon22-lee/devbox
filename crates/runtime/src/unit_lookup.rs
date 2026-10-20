use crate::{R, RuntimeService};
use devbox_protocol::Internal;

/// Live execution identity for observers, independent of paginated history queries.
#[derive(Debug, Clone)]
pub struct LiveUnitRun {
    pub unit: String,
    pub run_id: String,
    pub project_id: String,
}
impl RuntimeService {
    pub async fn live_unit_runs(&self) -> R<Vec<LiveUnitRun>> {
        self.db.read(|tx| {
            tx.prepare("SELECT json_extract(body,'$.run.unit'),id,project_id FROM runtime_runs WHERE json_extract(body,'$.run.state') IN ('starting','running','restarting') ORDER BY id")?
                .query_map([], |row| Ok(LiveUnitRun { unit: row.get(0)?, run_id: row.get(1)?, project_id: row.get(2)? }))?.collect()
        }).await.internal("read live execution units")
    }
}
