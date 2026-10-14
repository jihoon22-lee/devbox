use devbox_protocol::debug::{DebugError, EchoParams};
use devbox_protocol::{Ctx, Fail, StreamOpened};
pub(super) async fn echo(ctx: Ctx, params: EchoParams) -> Result<StreamOpened, Fail<DebugError>> {
    let mut ends = ctx.open_stream().map_err(|_| DebugError::Unsupported)?;
    let id = ends.id;
    tokio::spawn(async move {
        let chunk = vec![b'x'; devbox_protocol::frame::MAX_STREAM_CHUNK];
        let mut left = params.bytes as usize;
        while left > 0 {
            let n = left.min(chunk.len());
            if ends.tx.write(&chunk[..n]).await.is_err() {
                return;
            }
            left -= n;
        }
        while let Some(data) = ends.input.recv().await {
            if ends.tx.write(&data).await.is_err() {
                return;
            }
        }
        ends.tx.finish().await;
    });
    Ok(StreamOpened { stream_id: id })
}
