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

impl RuntimeService {
    pub async fn summary(&self, _: crate::RunSummaryQuery) -> R<crate::RunSummary> {
        let (active, rev) = self.db.read(|tx| {
            let active = tx.query_row("SELECT count(*) FROM runtime_runs WHERE json_extract(body,'$.run.state') IN ('starting','running','restarting')", [], |row| row.get(0))?;
            Ok((active, devbox_core::current_rev(tx)?))
        }).await.internal("read live execution summary")?;
        Ok(crate::RunSummary {
            rev: rev.to_string(),
            active,
        })
    }
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn summary_counts_all_live_runs_beyond_recent_history_at_one_revision() {
        let f = crate::service_tests::Fixture::new().await;
        let other_path = f.root.path().join("other");
        std::fs::create_dir(&other_path).expect("fixture result");
        let other = f
            .projects
            .add(devbox_projects::AddProject {
                path: other_path.display().to_string(),
            })
            .await
            .expect("fixture result")
            .id;
        let project = f.project.clone();
        let (_, rev) = f.db.write(move |tx| {
            for n in 0..303 {
                let state = match n { 0 => "starting", 1 => "running", 2 => "restarting", _ => "exited" };
                let body = serde_json::json!({"run":{"state":state,"defId":"same-definition"}}).to_string();
                tx.execute("INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,?3,?4)", (format!("summary-{n}"), if n == 2 {&other} else {&project}, n, body))?;
            }
            Ok(())
        }).await.expect("fixture result");
        let summary = f
            .runs
            .summary(crate::RunSummaryQuery {})
            .await
            .expect("fixture result");
        assert_eq!(summary.active, 3);
        assert_eq!(summary.rev, rev.to_string());
    }
}
