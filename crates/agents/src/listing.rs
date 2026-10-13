use crate::{
    model::*,
    service::{AgentsService, Result, now_ms},
    store,
};
use devbox_core::current_rev;
use devbox_protocol::Internal;
impl AgentsService {
    /// Internal lifecycle/metrics callers must inspect all owned tasks, independently of RPC pages.
    pub async fn all_tasks(&self, include_closed_hours: Option<u32>) -> Result<Vec<AgentTask>> {
        let cutoff =
            now_ms().saturating_sub(i64::from(include_closed_hours.unwrap_or(24)) * 3600000);
        self.db
            .read(move |tx| store::list(tx, cutoff))
            .await
            .internal("read all agent tasks")
    }
    pub async fn list(&self, p: ListAgents) -> Result<AgentsList> {
        if p.after_id
            .as_ref()
            .is_some_and(|id| id.len() > 64 || id.is_empty())
        {
            return Err(AgentsError::InvalidInput.into());
        }
        let cutoff =
            now_ms().saturating_sub(i64::from(p.include_closed_hours.unwrap_or(24)) * 3600000);
        self.db.read(move |tx| {
            let mut page = AgentsList { rev: current_rev(tx)?.to_string(), items: vec![], next_after_id: None };
            let encode = |value: &AgentTask| serde_json::to_vec(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)));
            let mut used = serde_json::to_vec(&page).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?.len();
            let mut query = tx.prepare("SELECT body FROM agents_tasks WHERE (state != 'closed' OR closed_ms >= ?1) AND id > ?2 ORDER BY id")?;
            let tasks = query.query_map(rusqlite::params![cutoff, p.after_id.unwrap_or_default()], store::decode)?;
            for task in tasks {
                let task = task?;
                let size = encode(&task)?.len();
                if size > crate::limits::TASK_BYTES {
                    return Ok(Err(AgentsError::InputTooLarge { limit_bytes: crate::limits::TASK_BYTES as u32 }));
                }
                // Reserve the exact JSON representation of this possible cursor and its envelope field.
                let cursor = serde_json::to_vec(&task.id).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?.len() + ",\"nextAfterId\":".len();
                let comma = usize::from(!page.items.is_empty());
                if used + size + comma + cursor > crate::limits::PAGE_BYTES {
                    page.next_after_id = page.items.last().map(|task| task.id.clone());
                    if page.items.is_empty() { return Ok(Err(AgentsError::InputTooLarge { limit_bytes: crate::limits::PAGE_BYTES as u32 })); }
                    break;
                }
                used += size + comma;
                page.items.push(task);
            }
            Ok(Ok(page))
        }).await.internal("list bounded agent page")?.map_err(devbox_protocol::Fail::Domain)
    }
}
