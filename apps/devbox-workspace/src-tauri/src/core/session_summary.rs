//! Pure bounded summary metadata and immutable operation receipts.
use crate::core::development_sessions::{Phase, Session};
#[cfg(test)]
use crate::core::problems::Item;
use product_contract::{
    session_summary::{
        self as contract, Binding, Draft, Metadata, ProblemCategory, SelectedProblem,
    },
    ProjectContext,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, &'static str>;
const MAX_RECEIPTS: usize = 256;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Input {
    pub operation_id: String,
    pub session_id: String,
    pub revision: u64,
    #[serde(default)]
    include_problems: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub metadata: Metadata,
    include_problems: bool,
}
impl Receipt {
    pub(crate) fn draft(&self) -> Result<Draft> {
        contract::prepare(
            &serde_json::to_vec(&self.metadata).map_err(|_| "session_summary_invalid")?,
            &self.metadata.binding,
        )
    }
}
fn valid_operation(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
pub(crate) fn validate_receipts(receipts: &BTreeMap<String, Receipt>) -> Result<()> {
    if receipts.len() > MAX_RECEIPTS {
        return Err("session_summary_limit");
    }
    for (id, receipt) in receipts {
        if !valid_operation(id) {
            return Err("session_summary_invalid");
        }
        receipt.draft()?;
    }
    Ok(())
}
fn utc(ms: u64) -> Result<String> {
    let ms = i64::try_from(ms).map_err(|_| "session_summary_interval_unavailable")?;
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .ok_or("session_summary_interval_unavailable")
}
pub(crate) fn prepare(
    context: &ProjectContext,
    session: &Session,
    input: &Input,
    receipts: &BTreeMap<String, Receipt>,
    observation: Option<&crate::core::problems::SessionSnapshot>,
    now: u64,
) -> Result<Receipt> {
    if !valid_operation(&input.operation_id)
        || session.id != input.session_id
        || session.context != *context
        || session.revision != input.revision
    {
        return Err("session_summary_stale");
    }
    if let Some(receipt) = receipts.get(&input.operation_id) {
        if receipt.metadata.binding.session_id != session.id
            || receipt.metadata.binding.context != *context
            || receipt.metadata.binding.revision != session.revision
            || receipt.include_problems != input.include_problems
        {
            return Err("session_summary_conflict");
        }
        return Ok(receipt.clone());
    }
    if receipts.len() >= MAX_RECEIPTS {
        return Err("session_summary_limit");
    }
    let start = session
        .started_at_ms
        .ok_or("session_summary_interval_unavailable")?;
    let through = if session.phase == Phase::Stopped {
        session
            .stopped_at_ms
            .ok_or("session_summary_interval_unavailable")?
    } else {
        now
    };
    if through < start {
        return Err("session_summary_interval_unavailable");
    }
    let mut selected_problems = Vec::new();
    if input.include_problems {
        let observation = observation
            .filter(|row| !row.3)
            .ok_or("session_summary_problems_unavailable")?;
        let mut count = observation.2.len() as u32;
        if session.phase == Phase::Degraded
            && matches!(
                session.issue.as_deref(),
                Some("session_readiness_expired" | "session_readiness_failed")
            )
        {
            selected_problems.push(SelectedProblem {
                category: ProblemCategory::ReadinessFailed,
                count: 1,
            });
            count = count.saturating_sub(1);
        }
        if count > 0 {
            selected_problems.push(SelectedProblem {
                category: ProblemCategory::DiagnosticsError,
                count,
            });
        }
    }
    let receipt = Receipt {
        metadata: Metadata {
            schema_version: 1,
            binding: Binding {
                context: context.clone(),
                session_id: session.id.clone(),
                revision: session.revision,
            },
            started_at: utc(start)?,
            through_at: utc(through)?,
            // Borrowed services and retired native leases cannot establish complete interval counts.
            failed_runs: None,
            git_commits: None,
            selected_problems,
        },
        include_problems: input.include_problems,
    };
    receipt.draft()?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::development_sessions::Store;

    fn session() -> Session {
        Store::default()
            .create(
                "10000000-0000-4000-8000-000000000001".into(),
                ProjectContext {
                    project_id: "project".into(),
                    worktree_id: "tree".into(),
                    target: product_contract::ExecutionTarget::Windows,
                    revision: 1,
                },
                "a".repeat(64),
                1_783_296_000_000,
            )
            .unwrap()
    }
    fn input(session: &Session) -> Input {
        Input {
            operation_id: "20000000-0000-4000-8000-000000000001".into(),
            session_id: session.id.clone(),
            revision: session.revision,
            include_problems: false,
        }
    }
    #[test]
    fn retry_keeps_the_original_interval_and_unknown_counts() {
        let session = session();
        let input = input(&session);
        let mut receipts = BTreeMap::new();
        let prepared = prepare(
            &session.context,
            &session,
            &input,
            &receipts,
            None,
            1_783_296_010_000,
        )
        .unwrap();
        assert!(prepared.metadata.failed_runs.is_none());
        assert!(prepared.metadata.git_commits.is_none());
        receipts.insert(input.operation_id.clone(), prepared.clone());
        let retry = prepare(
            &session.context,
            &session,
            &input,
            &receipts,
            None,
            1_783_296_100_000,
        )
        .unwrap();
        assert_eq!(prepared.metadata, retry.metadata);
        assert!(retry.draft().unwrap().body.contains("확인 불가"));
        let mut changed = session.clone();
        changed.revision += 1;
        assert_eq!(
            prepare(
                &session.context,
                &changed,
                &input,
                &receipts,
                None,
                1_783_296_100_000
            )
            .err(),
            Some("session_summary_stale")
        );
    }
    #[test]
    fn old_history_and_unrecorded_stop_never_get_invented_intervals() {
        let mut session = session();
        let input = input(&session);
        session.started_at_ms = None;
        assert_eq!(
            prepare(
                &session.context,
                &session,
                &input,
                &BTreeMap::new(),
                None,
                1_783_296_010_000
            )
            .err(),
            Some("session_summary_interval_unavailable")
        );
        session.started_at_ms = Some(1_783_296_000_000);
        session.phase = Phase::Stopped;
        assert!(prepare(
            &session.context,
            &session,
            &input,
            &BTreeMap::new(),
            None,
            1_783_296_010_000
        )
        .is_err());
        session.stopped_at_ms = Some(1_783_296_005_000);
        let summary = prepare(
            &session.context,
            &session,
            &input,
            &BTreeMap::new(),
            None,
            1_783_296_100_000,
        )
        .unwrap();
        assert_eq!(summary.metadata.through_at, utc(1_783_296_005_000).unwrap());
    }
    #[test]
    fn selected_problem_metadata_contains_no_source_messages_or_paths() {
        let mut session = session();
        session.phase = Phase::Degraded;
        session.issue = Some("session_readiness_failed".into());
        let mut input = input(&session);
        input.include_problems = true;
        let observation = (
            session.id.clone(),
            "revision".into(),
            vec![Item {
                severity: crate::core::problems::Severity::Error,
                message: "synthetic-private-path-and-command".into(),
                target: crate::core::problems::Target::Route {
                    route: "terminal".into(),
                },
                log: None,
            }],
            false,
        );
        let summary = prepare(
            &session.context,
            &session,
            &input,
            &BTreeMap::new(),
            Some(&observation),
            1_783_296_010_000,
        )
        .unwrap();
        assert_eq!(summary.metadata.selected_problems.len(), 1);
        assert_eq!(
            summary.metadata.selected_problems[0].category,
            ProblemCategory::ReadinessFailed
        );
        assert!(!serde_json::to_string(&summary)
            .unwrap()
            .contains("synthetic-private"));
    }
}
