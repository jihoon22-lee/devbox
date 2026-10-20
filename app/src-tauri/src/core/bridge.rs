#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    #[tokio::test]
    async fn magic_is_fragmented_and_error_is_bounded() {
        let (mut writer, mut reader) = tokio::io::duplex(64);
        tokio::spawn(async move {
            for b in MAGIC {
                writer.write_all(&[*b]).await.unwrap();
            }
        });
        handshake(&mut reader).await.unwrap();
        let (mut writer, mut reader) = tokio::io::duplex(128);
        tokio::spawn(async move {
            writer.write_all(b"Wsl/Service/WSL_E_FAIL").await.unwrap();
        });
        assert!(
            handshake(&mut reader)
                .await
                .unwrap_err()
                .to_string()
                .contains("WSL_E_FAIL")
        );
    }
    #[tokio::test]
    async fn short_error_without_eof_retains_partial_diagnostic() {
        let (mut writer, mut reader) = tokio::io::duplex(32);
        writer.write_all(&[255, 254, 65, 0, 66, 0]).await.unwrap();
        let error = handshake_deadline(&mut reader, Duration::from_millis(10))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("AB"));
    }
    #[tokio::test]
    async fn partial_frame_eof_is_an_error() {
        let (mut writer, reader) = tokio::io::duplex(32);
        tokio::spawn(async move {
            writer
                .write_all(&[5, 0, 0, 0, 1, 0, 0, 0, 0, 7, 0, 0])
                .await
                .unwrap();
        });
        let (events, mut incoming) = tokio::sync::mpsc::channel(4);
        assert!(
            pump_frames(reader, events)
                .await
                .unwrap_err()
                .to_string()
                .contains("중간")
        );
        let Event::Frame(frame) = incoming.recv().await.unwrap() else {
            panic!("expected frame")
        };
        assert_eq!(frame, [5, 0, 0, 0, 1, 0, 0, 0, 0]);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn broken_stdin_and_stderr_failure_are_visible_and_reaped() {
        for script in [
            "exec 0<&-; printf 'DEVBOX-BRIDGE/1\\n'; exec sleep 30",
            "printf WSL_E_ACCESS_DENIED >&2; exit 1",
        ] {
            let mut command = tokio::process::Command::new("sh");
            command.args(["-c", script]);
            let mut child = command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let pid = child.id().unwrap();
            let (tx, rx) = mpsc::channel(2);
            let (events, _incoming) = mpsc::channel(2);
            tx.send(vec![0; 65536]).await.unwrap();
            let error = tokio::time::timeout(
                Duration::from_secs(2),
                drive(&mut child, rx, events, CancellationToken::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
            if script.contains("ACCESS_DENIED") {
                assert!(error.to_string().contains("WSL_E_ACCESS_DENIED"));
            }
            assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_bridge_is_cancelled_and_reaped() {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", "exec sleep 30"]);
        let mut child = c
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let (_tx, rx) = tokio::sync::mpsc::channel(2);
        let (events, _) = tokio::sync::mpsc::channel(2);
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            drive(&mut child, rx, events, token).await,
            Err(ShellError::Cancelled)
        ));
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
}

use super::process::ShellError;
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Child,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;
pub const MAGIC: &[u8; 16] = b"DEVBOX-BRIDGE/1\n";
#[derive(Debug)]
pub enum Event {
    Ready,
    Frame(Vec<u8>),
}
pub async fn handshake(reader: &mut (impl AsyncRead + Unpin)) -> Result<(), ShellError> {
    handshake_deadline(reader, Duration::from_secs(10)).await
}
async fn handshake_deadline(
    reader: &mut (impl AsyncRead + Unpin),
    deadline: Duration,
) -> Result<(), ShellError> {
    let mut bytes = Vec::with_capacity(8192);
    let prefix = async {
        while bytes.len() < MAGIC.len() {
            let mut b = [0u8; 16];
            let want = MAGIC.len() - bytes.len();
            let n = reader.read(&mut b[..want]).await?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&b[..n]);
        }
        Ok::<_, ShellError>(())
    };
    match tokio::time::timeout(deadline, prefix).await {
        Ok(result) => result?,
        Err(_) if bytes.is_empty() => return Err(ShellError::Timeout),
        Err(_) => {}
    }
    if bytes == MAGIC {
        return Ok(());
    }
    let remainder = async {
        let mut buf = [0u8; 512];
        while bytes.len() < 8192 {
            let want = buf.len().min(8192 - bytes.len());
            let n = reader.read(&mut buf[..want]).await?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
        }
        Ok::<_, std::io::Error>(())
    };
    // WSL can leave its pipe open after writing an error; never wait for EOF indefinitely.
    let _ = tokio::time::timeout(Duration::from_millis(250), remainder).await;
    let text = super::wsl::decode_output(&bytes);
    let code = super::wsl::error_code(&text).unwrap_or_default();
    Err(ShellError::Invalid(format!(
        "브리지 시작 실패 {code}: {text}"
    )))
}
async fn pump_frames(
    mut reader: impl AsyncRead + Unpin,
    events: mpsc::Sender<Event>,
) -> Result<(), ShellError> {
    let mut split = devbox_mux::FrameSplitter::default();
    let mut buffered = 0usize;
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            return Err(ShellError::Invalid(
                if buffered > 0 {
                    "브리지 프레임이 중간에 끊겼습니다"
                } else {
                    "브리지 연결이 종료되었습니다"
                }
                .into(),
            ));
        }
        buffered += n;
        for frame in split
            .push(&buf[..n])
            .map_err(|e| ShellError::Invalid(e.to_string()))?
        {
            buffered -= frame.len();
            events
                .send(Event::Frame(frame))
                .await
                .map_err(|_| ShellError::Invalid("화면 전송 큐가 가득 찼거나 닫혔습니다".into()))?;
        }
    }
}
/// Caller retains the Job handle until this function has killed and reaped the owned child.
pub async fn drive(
    child: &mut Child,
    mut writes: mpsc::Receiver<Vec<u8>>,
    events: mpsc::Sender<Event>,
    cancel: CancellationToken,
) -> Result<(), ShellError> {
    let operation = async {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ShellError::Invalid("stdout 없음".into()))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ShellError::Invalid("stdin 없음".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ShellError::Invalid("stderr 없음".into()))?;
        let stderr_task = super::process::read_bounded(stderr);
        tokio::pin!(stderr_task);
        let transfer = async {
            handshake(&mut stdout).await?;
            events
                .send(Event::Ready)
                .await
                .map_err(|_| ShellError::Invalid("화면 연결이 닫혔습니다".into()))?;
            let writer = async {
                while let Some(frame) = writes.recv().await {
                    let frame = zeroize::Zeroizing::new(frame);
                    stdin.write_all(&frame).await?;
                    stdin.flush().await?;
                }
                Err::<(), ShellError>(ShellError::Invalid("브리지 전송이 닫혔습니다".into()))
            };
            tokio::try_join!(pump_frames(stdout, events), writer)?;
            Ok::<_, ShellError>(())
        };
        tokio::pin!(transfer);
        tokio::select! {
            result=&mut transfer=> {
                let diagnostic=tokio::time::timeout(Duration::from_millis(250),&mut stderr_task).await;
                match diagnostic {Ok(Ok(bytes))=>with_diagnostic(result,&bytes),Ok(Err(e))=>Err(e),Err(_)=>result}
            }
            diagnostic=&mut stderr_task=>{let bytes=diagnostic?;with_diagnostic(transfer.await,&bytes)}
        }
    };
    let result =
        tokio::select! {biased;_=cancel.cancelled()=>Err(ShellError::Cancelled),r=operation=>r};
    // Close before draining so cancelled transports also wipe queued unlocks.
    writes.close();
    while let Ok(mut frame) = writes.try_recv() {
        use zeroize::Zeroize;
        frame.zeroize();
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

fn with_diagnostic(result: Result<(), ShellError>, bytes: &[u8]) -> Result<(), ShellError> {
    if bytes.is_empty() {
        return result;
    }
    result.map_err(|e| ShellError::Invalid(format!("{e}: {}", super::wsl::decode_output(bytes))))
}
