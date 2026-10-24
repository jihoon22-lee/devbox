//! gRPC 채널: tonic TLS 기능 대신 직접 TCP·rustls(ALPN h2)를 연결한다. 그래서 요청 TLS 설정
//! (검증 끔·CA·클라이언트 인증서)이 `api.send`·WebSocket과 같은 규칙(`tls::rustls_config_alpn`)으로 적용된다.
use super::Prepared;
use crate::ApiError;
use hyper_util::rt::TokioIo;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

pub(crate) enum Io {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Io {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Io {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_flush(cx),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// 연결 실패를 고정 종류로(오류 문장에 호스트·경로가 있을 수 있어 쓰지 않는다).
#[derive(Debug)]
pub(crate) struct ConnectFailed(pub(crate) &'static str);
impl std::fmt::Display for ConnectFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ConnectFailed {}

/// 채널을 만든다. 실제 연결은 첫 호출 때(lazy) 하고, 실패는 상태로 돌아온다.
pub(crate) fn channel(p: &Prepared) -> Result<tonic::transport::Channel, ApiError> {
    let host = p
        .url
        .host_str()
        .ok_or_else(|| ApiError::url("host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = p
        .url
        .port_or_known_default()
        .ok_or_else(|| ApiError::url("port"))?;
    let tls = if p.url.scheme() == "https" {
        let config = crate::tls::rustls_config_alpn(&p.tls, &[b"h2"])?;
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|_| ApiError::url("host"))?;
        Some((tokio_rustls::TlsConnector::from(config), name))
    } else {
        None
    };
    let tls = Arc::new(tls);
    let endpoint = tonic::transport::Endpoint::from_shared(p.url.origin().ascii_serialization())
        .map_err(|_| ApiError::url("parse"))?
        .connect_timeout(super::DESCRIBE_TIMEOUT);
    let connector = tower::service_fn(move |_: tonic::transport::Uri| {
        let (host, tls) = (host.clone(), tls.clone());
        async move {
            let tcp = TcpStream::connect((host.as_str(), port))
                .await
                .map_err(|_| ConnectFailed("connect"))?;
            let _ = tcp.set_nodelay(true);
            let io = match tls.as_ref() {
                None => Io::Plain(tcp),
                Some((connector, name)) => Io::Tls(Box::new(
                    connector
                        .connect(name.clone(), tcp)
                        .await
                        .map_err(|_| ConnectFailed("tls"))?,
                )),
            };
            Ok::<_, ConnectFailed>(TokioIo::new(io))
        }
    });
    Ok(endpoint.connect_with_connector_lazy(connector))
}
