//! 송신 기록: 파일이 원본이고 `index.db`의 `api_history`는 다시 만들 수 있는 색인이다.
//! 시작 때와 24시간마다 30일 넘은 날짜 폴더·행을 지운다.
mod files;
mod index;
mod proof;
pub(crate) use files::day;
pub(crate) use proof::Receipt;
pub(crate) mod sanitize;

use crate::service::Clock;
use crate::validate::too_large;
use crate::{ApiError, HistoryEntry, HistoryId, HistoryPage, HistoryQuery, R};
use devbox_core::IndexDb;
use devbox_protocol::Internal;
use std::path::PathBuf;

pub const API_INDEX_VERSION: u32 = 1;
pub(crate) const RETENTION_DAYS: i64 = 30;
const MAX_PAGE_BYTES: usize = 768 * 1024;

pub(crate) struct History {
    root: PathBuf,
    index: IndexDb,
    clock: Clock,
}

fn cursor(text: &str) -> Result<(i64, String), ApiError> {
    let (sent, id) = text
        .split_once(':')
        .ok_or_else(|| ApiError::input("before", "cursor"))?;
    let sent = sent
        .parse()
        .map_err(|_| ApiError::input("before", "cursor"))?;
    if !files::valid_id(id) {
        return Err(ApiError::input("before", "cursor"));
    }
    Ok((sent, id.to_owned()))
}

impl History {
    /// 색인 도메인 버전이 바뀌었으면 `true`(파일로 다시 만들어야 함).
    pub(crate) fn open(
        root: PathBuf,
        index: IndexDb,
        clock: Clock,
    ) -> Result<(Self, bool), devbox_core::CoreError> {
        let rebuild = index.ensure_domain("api", API_INDEX_VERSION, index::SQL)?;
        Ok((Self { root, index, clock }, rebuild))
    }

    pub(crate) fn now(&self) -> i64 {
        (self.clock)()
    }

    pub(crate) async fn record(
        &self,
        entry: HistoryEntry,
        body: Vec<u8>,
    ) -> std::io::Result<Receipt> {
        let root = self.root.clone();
        let stored = entry.clone();
        let receipt = tokio::task::spawn_blocking(move || files::write(&root, stored, &body))
            .await
            .map_err(std::io::Error::other)??;
        let day = files::day(entry.sent_at_ms);
        self.index
            .write(move |tx| index::insert(tx, &entry, &day))
            .await
            .map_err(std::io::Error::other)?;
        Ok(receipt)
    }

    pub(crate) async fn page(&self, q: HistoryQuery) -> R<HistoryPage> {
        if !(1..=256).contains(&q.limit) {
            return Err(ApiError::input("limit", "range").into());
        }
        let before = q.before.as_deref().map(cursor).transpose()?;
        let origin = index::origin_key(&q.origin);
        let limit = q.limit;
        let mut rows = self
            .index
            .read(move |tx| index::page(tx, before, origin, limit))
            .await
            .internal("read api history")?;
        let mut more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let mut size = 64;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            size += serde_json::to_vec(&row)
                .map(|b| b.len() + 1)
                .unwrap_or(MAX_PAGE_BYTES);
            if size > MAX_PAGE_BYTES {
                more = true;
                break;
            }
            items.push(row);
        }
        if items.is_empty() && more {
            return Err(too_large(MAX_PAGE_BYTES).into());
        }
        let next = more
            .then(|| items.last().map(|i| format!("{}:{}", i.sent_at_ms, i.id)))
            .flatten();
        Ok(HistoryPage { items, next })
    }

    pub(crate) async fn get(&self, q: HistoryId) -> R<HistoryEntry> {
        if !files::valid_id(&q.id) {
            return Err(ApiError::HistoryNotFound.into());
        }
        let id = q.id.clone();
        let day = self
            .index
            .read(move |tx| index::day_of(tx, &id))
            .await
            .internal("read api history")?
            .ok_or(ApiError::HistoryNotFound)?;
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || files::read(&root, &day, &q.id))
            .await
            .internal("read api history")?
            .map_err(|_| ApiError::HistoryNotFound.into())
    }

    /// Only the receipt created after this daemon's successful durable write admits export.
    pub(crate) async fn export_get(&self, receipt: Receipt) -> std::io::Result<HistoryEntry> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || proof::read(&root, &receipt))
            .await
            .map_err(std::io::Error::other)?
    }

    /// 30일 넘은 행과 날짜 폴더를 지운다.
    pub(crate) async fn prune(&self) -> Result<(), devbox_core::CoreError> {
        let cutoff = self.now() - RETENTION_DAYS * files::DAY_MS;
        self.index.write(move |tx| index::prune(tx, cutoff)).await?;
        let root = self.root.clone();
        let day = files::day(cutoff);
        let _ = tokio::task::spawn_blocking(move || files::prune(&root, &day)).await;
        Ok(())
    }

    /// 기록 파일을 다시 읽어 색인을 채운다(완료 전 `api.history`는 있는 행만 보여 준다).
    pub(crate) async fn rebuild(&self) -> Result<(), devbox_core::CoreError> {
        let root = self.root.clone();
        let entries = tokio::task::spawn_blocking(move || files::scan(&root))
            .await
            .unwrap_or_default();
        for chunk in entries.chunks(256) {
            let chunk = chunk.to_vec();
            self.index
                .write(move |tx| {
                    for entry in &chunk {
                        index::insert(tx, entry, &files::day(entry.sent_at_ms))?;
                    }
                    Ok(())
                })
                .await?;
        }
        Ok(())
    }
}
