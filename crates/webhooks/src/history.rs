use crate::{
    error::storage,
    service::{Inner, now_ms},
    *,
};
use rusqlite::{OptionalExtension, params};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const PREVIEW: usize = 1024 * 1024;
fn safe_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok() && id.len() == 36
}
async fn directory(path: &Path) -> R<()> {
    tokio::fs::create_dir_all(path).await.map_err(storage)?;
    if !tokio::fs::symlink_metadata(path)
        .await
        .map_err(storage)?
        .is_dir()
    {
        return Err(WebhooksError::Storage.into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(storage)?;
    }
    Ok(())
}
impl Inner {
    async fn body_path(&self, listener: &str, id: &str) -> R<std::path::PathBuf> {
        if !safe_id(listener) || !safe_id(id) {
            return Err(WebhooksError::Storage.into());
        }
        let dir = self.dir.join(listener);
        for directory in [&self.dir, &dir] {
            match tokio::fs::symlink_metadata(directory).await {
                Ok(metadata) if metadata.is_dir() => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(WebhooksError::Storage.into()),
            }
        }
        Ok(dir.join(format!("{id}.bin")))
    }
    pub async fn prune_expired(&self) -> R<()> {
        let old = self
            .db
            .read(|tx| {
                tx.prepare("SELECT listener_id,id FROM webhooks_received WHERE at_ms < ?1")?
                    .query_map([now_ms() - 30 * 86400000i64], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .map_err(storage)?;
        for (listener, id) in old {
            if !safe_id(&listener) || !safe_id(&id) {
                return Err(WebhooksError::Storage.into());
            }
            let path = self.body_path(&listener, &id).await?;
            match tokio::fs::remove_file(path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(storage(e)),
            }
            self.db
                .write(move |tx| {
                    tx.execute("DELETE FROM webhooks_received WHERE id=?1", [id])?;
                    Ok(())
                })
                .await
                .map_err(storage)?;
        }
        Ok(())
    }
    pub async fn record(&self, mut detail: ReceivedDetail, body: &[u8]) -> R<()> {
        directory(&self.dir).await?;
        let dir = self.dir.join(&detail.received.listener_id);
        directory(&dir).await?;
        let path = dir.join(format!("{}.bin", detail.received.id));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(&path).await.map_err(storage)?;
        if let Err(e) = async {
            file.write_all(body).await?;
            file.sync_all().await
        }
        .await
        {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(storage(e));
        }
        detail.body_base64.clear();
        let text = serde_json::to_string(&detail).map_err(storage)?;
        let event = detail.received.clone();
        let id = event.id.clone();
        let listener = event.listener_id.clone();
        let write = self.db.write(move |tx| {
            tx.execute("INSERT INTO webhooks_received VALUES(?1,?2,?3,?4)",params![id,listener,event.at_ms as i64,text])?;
            let old = tx.prepare("SELECT listener_id,id FROM webhooks_received WHERE at_ms < ?1 OR (listener_id=?2 AND id NOT IN (SELECT id FROM webhooks_received WHERE listener_id=?2 ORDER BY at_ms DESC,rowid DESC LIMIT 200))")?.query_map(params![now_ms()-30*86400000i64,listener],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for (_,id) in &old { tx.execute("DELETE FROM webhooks_received WHERE id=?1",[id])?; }
            Ok(old)
        }).await;
        let (old, rev) = match write {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(storage(e));
            }
        };
        for (listener, id) in old {
            if let Ok(path) = self.body_path(&listener, &id).await {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        self.hub.publish::<WebhooksReceived>(rev, &detail.received);
        Ok(())
    }
}
impl WebhooksService {
    pub async fn history(&self, p: HistoryQuery) -> R<ReceivedPage> {
        self.inner.prune_expired().await?;
        let texts = self.inner.db.read(move |tx| tx.prepare("SELECT detail_json FROM webhooks_received WHERE listener_id=?1 AND (?2 IS NULL OR rowid < (SELECT rowid FROM webhooks_received WHERE id=?2 AND listener_id=?1)) ORDER BY at_ms DESC,rowid DESC LIMIT ?3")?.query_map(params![p.listener_id,p.before,p.limit.clamp(1,200)],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()).await.map_err(storage)?;
        let items = texts
            .into_iter()
            .map(|s| serde_json::from_str::<ReceivedDetail>(&s).map(|d| d.received))
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(ReceivedPage { items })
    }
    pub async fn history_get(&self, p: ReceivedId) -> R<ReceivedDetail> {
        self.inner.prune_expired().await?;
        let text = self
            .inner
            .db
            .read(move |tx| {
                tx.query_row(
                    "SELECT detail_json FROM webhooks_received WHERE id=?1",
                    [p.id],
                    |r| r.get::<_, String>(0),
                )
                .optional()
            })
            .await
            .map_err(storage)?
            .ok_or(WebhooksError::NotFound)?;
        let mut detail: ReceivedDetail = serde_json::from_str(&text).map_err(storage)?;
        if !safe_id(&detail.received.id) || !safe_id(&detail.received.listener_id) {
            return Err(WebhooksError::Storage.into());
        }
        let path = self
            .inner
            .body_path(&detail.received.listener_id, &detail.received.id)
            .await?;
        if !tokio::fs::symlink_metadata(&path)
            .await
            .map_err(storage)?
            .is_file()
        {
            return Err(WebhooksError::Storage.into());
        }
        let mut bytes = Vec::new();
        tokio::fs::File::open(path)
            .await
            .map_err(storage)?
            .take((PREVIEW + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(storage)?;
        detail.preview_truncated = bytes.len() > PREVIEW;
        bytes.truncate(PREVIEW);
        detail.body_base64 = data_encoding::BASE64.encode(&bytes);
        Ok(detail)
    }
    pub async fn fixture_save(&self, p: SaveFixture) -> R<Fixture> {
        if p.name.trim().is_empty() || p.name.len() > 256 {
            return Err(WebhooksError::InvalidInput.into());
        }
        let request = self.history_get(ReceivedId { id: p.received_id }).await?;
        if request.preview_truncated || request.received.truncated {
            return Err(WebhooksError::InvalidInput.into());
        }
        let fixture = Fixture {
            id: uuid::Uuid::new_v4().to_string(),
            name: p.name,
            request,
        };
        let copy = fixture.clone();
        let text = serde_json::to_string(&copy.request).map_err(storage)?;
        self.inner
            .db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO webhooks_fixtures VALUES(?1,?2,?3)",
                    params![copy.id, copy.name, text],
                )?;
                Ok(())
            })
            .await
            .map_err(storage)?;
        Ok(fixture)
    }
}
