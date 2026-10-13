use crate::model::AgentTask;
use rusqlite::{OptionalExtension, Transaction, params, types::Type};

pub const MIGRATION_1: &str = "
CREATE TABLE agents_tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  state TEXT NOT NULL,
  updated_ms INTEGER NOT NULL,
  closed_ms INTEGER,
  body TEXT NOT NULL
);
CREATE INDEX agents_tasks_project ON agents_tasks(project_id);
CREATE INDEX agents_tasks_state ON agents_tasks(state);";

pub fn put(tx: &Transaction<'_>, task: &AgentTask) -> rusqlite::Result<()> {
    let body = serde_json::to_string(task)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let state = serde_json::to_string(&task.state)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    tx.execute("INSERT INTO agents_tasks(id,project_id,state,updated_ms,closed_ms,body) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,state=excluded.state,updated_ms=excluded.updated_ms,closed_ms=excluded.closed_ms,body=excluded.body", params![task.id,task.project_id,state.trim_matches('"'),task.updated_ms,task.closed_ms,body])?;
    Ok(())
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTask> {
    let body: String = row.get(0)?;
    serde_json::from_str(&body)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
}

pub fn get(tx: &Transaction<'_>, id: &str) -> rusqlite::Result<Option<AgentTask>> {
    tx.query_row("SELECT body FROM agents_tasks WHERE id=?1", [id], decode)
        .optional()
}

/// Include active tasks and closed tasks whose closure is at or after the cutoff.
pub fn list(
    tx: &Transaction<'_>,
    include_closed_since_ms: i64,
) -> rusqlite::Result<Vec<AgentTask>> {
    let mut stmt = tx.prepare("SELECT body FROM agents_tasks WHERE state != 'closed' OR closed_ms >= ?1 ORDER BY updated_ms DESC, id")?;
    stmt.query_map([include_closed_since_ms], decode)?.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Attention, FollowUp, Tokens},
        state::{AgentState, CloseHow},
    };
    fn task(id: &str) -> AgentTask {
        AgentTask {
            id: id.into(),
            project_id: "p".into(),
            title: "Title".into(),
            profile: "claude".into(),
            base_branch: "main".into(),
            branch: Some("agent/fix".into()),
            worktree_path: Some("/tmp/owned".into()),
            workdir: "/tmp/owned".into(),
            session_id: Some("session".into()),
            terminal_id: Some("terminal".into()),
            tool_exited: false,
            state: AgentState::Waiting,
            reason: Some("reason".into()),
            attention: Some(Attention {
                kind: "input".into(),
                message: "질문".into(),
                at_ms: 1,
            }),
            port: 3000,
            prompt: "prompt".into(),
            follow_ups: vec![FollowUp {
                at_ms: 2,
                text: "next".into(),
            }],
            tokens: Some(Tokens {
                input: 1,
                output: 2,
                cache_read: 3,
                cache_write: 4,
            }),
            pr_url: Some("https://example.test/pr".into()),
            closed_how: None,
            created_ms: 1,
            updated_ms: 2,
            closed_ms: None,
            closed_commits: None,
            closed_head_oid: None,
            cleanup_error: None,
            tokens_truncated: false,
            activity_seq: 0,
        }
    }
    #[test]
    fn migration_roundtrip_upsert_columns_and_closed_boundary() {
        let mut db = rusqlite::Connection::open_in_memory().expect("owned database");
        let tx = db.transaction().expect("transaction");
        tx.execute_batch(MIGRATION_1).expect("migration");
        let indexes: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('agents_tasks_project','agents_tasks_state')",[],|r|r.get(0)).expect("indexes");
        assert_eq!(indexes, 2);
        let mut original = task("live");
        put(&tx, &original).expect("put");
        assert_eq!(
            serde_json::to_string(&get(&tx, "live").expect("get").expect("exists")).expect("json"),
            serde_json::to_string(&original).expect("json")
        );
        original.project_id = "other".into();
        original.state = AgentState::Idle;
        original.updated_ms = 20;
        put(&tx, &original).expect("upsert");
        let columns: (String, String, i64) = tx
            .query_row(
                "SELECT project_id,state,updated_ms FROM agents_tasks WHERE id='live'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("columns");
        assert_eq!(columns, ("other".into(), "idle".into(), 20));
        for (id, time) in [("before", 9), ("boundary", 10), ("after", 11)] {
            let mut closed = task(id);
            closed.state = AgentState::Closed;
            closed.closed_ms = Some(time);
            closed.closed_how = Some(CloseHow::Merged);
            closed.updated_ms = 30;
            put(&tx, &closed).expect("closed");
        }
        let ids: Vec<_> = list(&tx, 10)
            .expect("list")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["after", "boundary", "live"]);
        assert!(get(&tx, "absent").expect("missing").is_none());
        tx.execute("UPDATE agents_tasks SET body='broken' WHERE id='live'", [])
            .expect("corrupt owned row");
        assert!(matches!(
            get(&tx, "live"),
            Err(rusqlite::Error::FromSqlConversionFailure(..))
        ));
    }
}

pub const MIGRATION_2: &str = "CREATE TABLE agents_runtime (id TEXT PRIMARY KEY REFERENCES agents_tasks(id), body TEXT NOT NULL);";
pub(crate) fn binding(
    tx: &Transaction<'_>,
    id: &str,
) -> rusqlite::Result<Option<crate::tracker::Binding>> {
    let text: Option<String> = tx
        .query_row("SELECT body FROM agents_runtime WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    text.map(|s| {
        serde_json::from_str(&s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
    })
    .transpose()
}
pub(crate) fn put_binding(
    tx: &Transaction<'_>,
    id: &str,
    binding: &crate::tracker::Binding,
) -> rusqlite::Result<()> {
    let body = serde_json::to_string(binding)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    tx.execute("INSERT INTO agents_runtime(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![id,body])?;
    Ok(())
}
