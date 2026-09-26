use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
type Result<T> = std::result::Result<T, &'static str>;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
// The first byte may wait while idle. Once a frame starts it must finish within
// one deadline; a slow peer cannot reset that deadline one byte at a time.
pub async fn read<S: AsyncRead + Unpin, T: DeserializeOwned>(stream: &mut S) -> Result<T> {
    let mut header = [0; 4];
    stream
        .read_exact(&mut header[..1])
        .await
        .map_err(|_| "connection_closed")?;
    tokio::time::timeout(IO_TIMEOUT, async {
        stream
            .read_exact(&mut header[1..])
            .await
            .map_err(|_| "connection_closed")?;
        let size = u32::from_le_bytes(header) as usize;
        if size == 0 {
            return Err("frame_empty");
        }
        if size > agent_protocol::MAX_FRAME_BYTES {
            return Err("frame_too_large");
        }
        let mut body = vec![0; size];
        stream
            .read_exact(&mut body)
            .await
            .map_err(|_| "connection_closed")?;
        agent_protocol::decode(&body).map_err(|e| e.code())
    })
    .await
    .map_err(|_| "frame_timeout")?
}
pub async fn write<S: AsyncWrite + Unpin, T: Serialize>(stream: &mut S, message: &T) -> Result<()> {
    let bytes = agent_protocol::encode(message).map_err(|e| e.code())?;
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&bytes))
        .await
        .map_err(|_| "write_timeout")?
        .map_err(|_| "connection_closed")
}
