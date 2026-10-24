use crate::{error::storage, *};
use devbox_core::{Db, Hub};
use devbox_protocol::Done;
use rusqlite::{OptionalExtension, params};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub(crate) struct Active {
    pub cancel: CancellationToken,
    pub task: tokio::task::JoinHandle<()>,
}
pub(crate) struct Inner {
    pub db: Db,
    pub hub: Arc<Hub>,
    pub dir: PathBuf,
    pub sequences: Mutex<HashMap<String, rules::ResponseSequenceState>>,
    pub rule_lock: Mutex<()>,
}
pub struct WebhooksService {
    pub(crate) inner: Arc<Inner>,
    active: Mutex<HashMap<String, Active>>,
}
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
impl WebhooksService {
    pub fn new(db: Db, hub: impl Into<Arc<Hub>>, dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                hub: hub.into(),
                dir,
                sequences: Mutex::new(HashMap::new()),
                rule_lock: Mutex::new(()),
            }),
            active: Mutex::new(HashMap::new()),
        }
    }
    pub async fn listeners(&self) -> R<ListenerList> {
        let mut items = self
            .inner
            .db
            .read(|tx| {
                tx.prepare("SELECT id,port,lan FROM webhooks_listeners ORDER BY created_ms,id")?
                    .query_map([], |r| {
                        let port = r.get(1)?;
                        let lan = r.get(2)?;
                        Ok(Listener {
                            id: r.get(0)?,
                            port,
                            lan,
                            addresses: address::addresses(port, lan),
                            running: false,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .map_err(storage)?;
        let active = self.active.lock().await;
        for item in &mut items {
            item.running = active.get(&item.id).is_some_and(|a| !a.task.is_finished());
        }
        Ok(ListenerList { items })
    }
    pub async fn start(&self, p: StartListener) -> R<Listener> {
        let mut active = self.active.lock().await;
        if active.len() >= 16 {
            return Err(WebhooksError::LimitReached.into());
        }
        let socket = tokio::net::TcpListener::bind((address::bind_address(p.lan), p.port))
            .await
            .map_err(|_| WebhooksError::BindFailed)?;
        let port = socket.local_addr().map_err(storage)?.port();
        // Reuse a stopped listener's identity so its rules/history remain available.
        let existing = self.inner.db.read(move |tx| tx.query_row("SELECT id FROM webhooks_listeners WHERE port=?1 AND lan=?2 ORDER BY created_ms LIMIT 1", params![port,p.lan], |r| r.get::<_,String>(0)).optional()).await.map_err(storage)?;
        let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let owned = id.clone();
        self.inner
            .db
            .write(move |tx| {
                tx.execute(
                    "INSERT OR IGNORE INTO webhooks_listeners VALUES(?1,?2,?3,?4)",
                    params![owned, port, p.lan, now_ms()],
                )?;
                Ok(())
            })
            .await
            .map_err(storage)?;
        self.inner.sequences.lock().await.remove(&id);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(listener::serve(
            socket,
            self.inner.clone(),
            id.clone(),
            cancel.clone(),
        ));
        active.insert(id.clone(), Active { cancel, task });
        Ok(Listener {
            id,
            port,
            lan: p.lan,
            addresses: address::addresses(port, p.lan),
            running: true,
        })
    }
    pub async fn stop(&self, p: ListenerId) -> R<Done> {
        if let Some(active) = self.active.lock().await.remove(&p.id) {
            active.cancel.cancel();
            let _ = active.task.await;
        }
        Ok(Done {
            rev: self.inner.db.rev().to_string(),
        })
    }
    pub async fn rules(&self, p: ListenerId) -> R<RuleList> {
        self.inner.rules(p.id).await
    }
    pub async fn rule_upsert(&self, p: UpsertRule) -> R<ResponseRule> {
        let _guard = self.inner.rule_lock.lock().await;
        let mut rules: HashMap<_, _> = self
            .inner
            .rules(p.listener_id.clone())
            .await?
            .items
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        let id = rules::upsert(&mut rules, p.rule).map_err(|_| WebhooksError::InvalidInput)?;
        let rule = rules.remove(&id).ok_or(WebhooksError::InvalidInput)?;
        let text = serde_json::to_string(&rule).map_err(storage)?;
        let result = rule.clone();
        let listener_id = p.listener_id.clone();
        self.inner.db.write(move |tx| { tx.execute("INSERT INTO webhooks_rules VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(listener_id,id) DO UPDATE SET priority=excluded.priority,method=excluded.method,path=excluded.path,response_json=excluded.response_json",params![rule.id,listener_id,rule.priority,rule.method,rule.path,text])?; Ok(()) }).await.map_err(storage)?;
        self.inner
            .sequences
            .lock()
            .await
            .entry(p.listener_id)
            .or_default()
            .reset(&id);
        Ok(result)
    }
    pub async fn rule_delete(&self, p: RuleRef) -> R<Done> {
        let _guard = self.inner.rule_lock.lock().await;
        let ((), rev) = self
            .inner
            .db
            .write(move |tx| {
                tx.execute(
                    "DELETE FROM webhooks_rules WHERE listener_id=?1 AND id=?2",
                    params![p.listener_id, p.id],
                )?;
                Ok(())
            })
            .await
            .map_err(storage)?;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
}
impl Drop for WebhooksService {
    fn drop(&mut self) {
        for active in self.active.get_mut().values() {
            active.cancel.cancel();
        }
    }
}
impl Inner {
    pub async fn rules(&self, listener: String) -> R<RuleList> {
        let values = self
            .db
            .read(move |tx| {
                tx.prepare(
                    "SELECT response_json FROM webhooks_rules WHERE listener_id=?1 ORDER BY id",
                )?
                .query_map([listener], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .map_err(storage)?;
        let items = values
            .into_iter()
            .map(|s| serde_json::from_str(&s))
            .collect::<Result<Vec<ResponseRule>, _>>()
            .map_err(storage)?;
        Ok(RuleList { items })
    }
}
