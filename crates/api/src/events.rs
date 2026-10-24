//! `api.send` 스트림에 이벤트를 NDJSON 한 줄씩 쓴다.
use crate::{ApiError, SendEvent};
use devbox_protocol::stream::StreamTx;

/// 화면이 스트림을 닫았다.
#[derive(Debug)]
pub(crate) struct Closed;

/// 응답 본문 이벤트 하나에 담는 원본 바이트 상한.
pub(crate) const BODY_CHUNK: usize = 48 * 1024;

pub(crate) struct Events<'a> {
    tx: &'a StreamTx,
}

impl<'a> Events<'a> {
    pub(crate) fn new(tx: &'a StreamTx) -> Self {
        Self { tx }
    }

    pub(crate) async fn send(&self, event: &SendEvent) -> Result<(), Closed> {
        let mut line = serde_json::to_vec(event).map_err(|_| Closed)?;
        line.push(b'\n');
        self.tx.write(&line).await.map_err(|_| Closed)
    }

    pub(crate) async fn body(&self, data: &[u8]) -> Result<(), Closed> {
        for chunk in data.chunks(BODY_CHUNK) {
            self.send(&SendEvent::Body {
                data_base64: data_encoding::BASE64.encode(chunk),
            })
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn failed(&self, error: &ApiError) -> Result<(), Closed> {
        self.send(&SendEvent::Failed {
            error: devbox_protocol::to_wire(error),
        })
        .await
    }
}
