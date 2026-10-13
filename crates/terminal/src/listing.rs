use super::{Result, TerminalService, tmux_error};
use crate::{ListSessions, SessionList, TerminalError, TerminalSession, limits, store};
use devbox_core::current_rev;
use devbox_protocol::{Fail, Internal};
use std::collections::HashMap;

fn cursor(session: &TerminalSession) -> String {
    format!("{}:{}", session.created_ms, session.id)
}
fn parse_cursor(value: Option<String>) -> Result<(i64, String)> {
    let Some(value) = value else {
        return Ok((i64::MIN, String::new()));
    };
    let invalid = || Fail::Domain(TerminalError::InvalidCursor);
    if value.len() > limits::CURSOR_BYTES {
        return Err(invalid());
    }
    let (created, id) = value.split_once(':').ok_or_else(invalid)?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(invalid());
    }
    Ok((created.parse().map_err(|_| invalid())?, id.to_owned()))
}
fn observe(items: &mut [TerminalSession], live: &HashMap<String, String>) {
    for item in items {
        if let Some(command) = live.get(&item.id) {
            item.alive = true;
            item.command = limits::command_preview(command);
        } else {
            item.command = limits::command_preview(&item.command);
        }
    }
}
fn bytes<T: serde::Serialize>(value: &T) -> Result<usize> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| Fail::Internal(format!("measure terminal page: {error}")))
}

impl TerminalService {
    async fn live_commands(&self) -> Result<HashMap<String, String>> {
        Ok(self
            .tmux
            .list()
            .await
            .map_err(tmux_error)?
            .into_iter()
            .map(|session| (session.name, session.command))
            .collect())
    }

    /// Internal summaries inspect every session, independently of bounded RPC pages.
    pub async fn all_sessions(&self, project_id: Option<&str>) -> Result<Vec<TerminalSession>> {
        let project_id = project_id.map(str::to_owned);
        let mut items = self
            .db
            .read(move |tx| store::all(tx, project_id.as_deref()))
            .await
            .internal("all terminal metadata")?;
        observe(&mut items, &self.live_commands().await?);
        Ok(items)
    }

    pub async fn list(&self, params: ListSessions) -> Result<SessionList> {
        let after = parse_cursor(params.after_id)?;
        let live = self.live_commands().await?;
        let (mut items, rev) = self
            .db
            .read(move |tx| {
                Ok((
                    store::page(tx, params.project_id.as_deref(), after.0, &after.1)?,
                    current_rev(tx)?,
                ))
            })
            .await
            .internal("list bounded terminal page")?;
        // The revision describes DB metadata. tmux liveness/command is a separate observation.
        observe(&mut items, &live);
        let mut page = SessionList {
            rev: rev.to_string(),
            items: vec![],
            next_after_id: None,
        };
        let mut used = bytes(&page)?;
        // Conservative JSON-escaped cursor reserve also covers negative legacy timestamps.
        let reserve = ",\"nextAfterId\":".len() + 2 + limits::CURSOR_BYTES * 6;
        for item in items {
            let size = bytes(&item)? + usize::from(!page.items.is_empty());
            if page.items.len() == limits::PAGE_ROWS || used + size + reserve > limits::PAGE_BYTES {
                page.next_after_id = page.items.last().map(cursor);
                if page.items.is_empty() {
                    return Err(Fail::Internal(
                        "terminal metadata row exceeds page budget".into(),
                    ));
                }
                break;
            }
            used += size;
            page.items.push(item);
        }
        Ok(page)
    }
}
