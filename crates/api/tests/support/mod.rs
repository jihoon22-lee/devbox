//! 소유 fixture: 127.0.0.1:0 hyper 1 서버(받은 요청·연결 수·지연·리디렉션·에코)와 스트림을 읽는 가짜 화면.
#![allow(dead_code)]
pub mod api;
pub mod grpc;
pub mod lines;
pub mod realtime;
pub mod tls;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::body::Frame;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// 서버가 받은 요청 한 건. 헤더 이름은 hyper가 소문자로 준다.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub client_cert: bool,
}
impl Seen {
    pub fn header(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
    pub fn query_pairs(&self) -> Vec<(String, String)> {
        let query = self.target.split_once('?').map(|(_, q)| q).unwrap_or("");
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect()
    }
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    /// v0.9.0 시험의 원문 요청 문자열과 같은 꼴(소문자 헤더 이름).
    pub fn raw(&self) -> String {
        let mut out = format!("{} {} HTTP/1.1\r\n", self.method, self.target);
        for (name, value) in &self.headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        out.push_str("\r\n");
        out.push_str(&self.body_text());
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub head_delay: Duration,
    pub body_delay: Duration,
}
impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::status(200, body)
    }
    pub fn status(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
            ..Self::default()
        }
    }
    pub fn redirect(status: u16, location: &str) -> Self {
        Self::status(status, Vec::new()).header("location", location)
    }
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn delayed(mut self, head: Duration) -> Self {
        self.head_delay = head;
        self
    }
    pub fn slow_body(mut self, delay: Duration) -> Self {
        self.body_delay = delay;
        self
    }
}

pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub type Handler = Arc<dyn Fn(Seen) -> BoxFut<Reply> + Send + Sync>;

pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Seen) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Reply> + Send + 'static,
{
    Arc::new(move |seen| Box::pin(f(seen)))
}

#[derive(Default)]
struct State {
    accepted: AtomicUsize,
    closed: AtomicUsize,
    requests: Mutex<Vec<Seen>>,
    changed: Notify,
}

pub struct Server {
    pub addr: SocketAddr,
    pub scheme: &'static str,
    state: Arc<State>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    pub async fn start(handler: Handler) -> Self {
        Self::spawn(handler, None).await
    }
    pub async fn ok() -> Self {
        Self::start(self::handler(|_| async { Reply::ok("ok") })).await
    }
    /// 받은 요청의 Authorization·Cookie와 본문을 그대로 되돌려 준다.
    pub async fn echo() -> Self {
        Self::start(self::handler(|seen: Seen| async move {
            let mut reply = Reply::ok(seen.body.clone());
            for (name, value) in &seen.headers {
                if matches!(name.as_str(), "authorization" | "cookie" | "x-api-key") {
                    reply = reply.header(&format!("x-echo-{name}"), value);
                }
            }
            reply.header("content-type", "text/plain")
        }))
        .await
    }
    pub async fn spawn(handler: Handler, tls: Option<tls::ServerTls>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned fixture");
        let addr = listener.local_addr().expect("fixture address");
        let state = Arc::new(State::default());
        let scheme = if tls.is_some() { "https" } else { "http" };
        let acceptor = tls.map(tls::acceptor);
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                task_state.accepted.fetch_add(1, Ordering::SeqCst);
                task_state.changed.notify_waiters();
                let state = task_state.clone();
                let handler = handler.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor {
                        None => serve(stream, false, handler, state.clone()).await,
                        Some(acceptor) => {
                            if let Ok(tls) = acceptor.accept(stream).await {
                                let cert = tls.get_ref().1.peer_certificates().is_some();
                                serve(tls, cert, handler, state.clone()).await;
                            }
                        }
                    }
                    state.closed.fetch_add(1, Ordering::SeqCst);
                    state.changed.notify_waiters();
                });
            }
        });
        Self {
            addr,
            scheme,
            state,
            task,
        }
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}://{}{}", self.scheme, self.addr, path)
    }
    pub fn accepted_connections(&self) -> usize {
        self.state.accepted.load(Ordering::SeqCst)
    }
    pub fn closed_connections(&self) -> usize {
        self.state.closed.load(Ordering::SeqCst)
    }
    pub fn requests(&self) -> Vec<Seen> {
        self.state.requests.lock().expect("requests").clone()
    }
    pub fn only(&self) -> Seen {
        let requests = self.requests();
        assert_eq!(requests.len(), 1, "exactly one request reached the fixture");
        requests[0].clone()
    }
    pub async fn wait(&self, done: impl Fn(&Self) -> bool, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                let changed = self.state.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if done(self) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .is_ok()
    }
}

async fn serve<I>(io: I, client_cert: bool, handler: Handler, state: Arc<State>)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service =
        hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
            let handler = handler.clone();
            let state = state.clone();
            async move {
                let (head, body) = request.into_parts();
                let body = body
                    .collect()
                    .await
                    .map(|b| b.to_bytes().to_vec())
                    .unwrap_or_default();
                let seen = Seen {
                    method: head.method.to_string(),
                    target: head
                        .uri
                        .path_and_query()
                        .map(|p| p.as_str().to_owned())
                        .unwrap_or_default(),
                    headers: head
                        .headers
                        .iter()
                        .map(|(n, v)| {
                            (
                                n.as_str().to_owned(),
                                String::from_utf8_lossy(v.as_bytes()).into_owned(),
                            )
                        })
                        .collect(),
                    body,
                    client_cert,
                };
                state.requests.lock().expect("requests").push(seen.clone());
                state.changed.notify_waiters();
                let reply = handler(seen).await;
                tokio::time::sleep(reply.head_delay).await;
                let mut response = hyper::Response::builder().status(reply.status);
                for (name, value) in &reply.headers {
                    response = response.header(name, value);
                }
                let body: BoxBody<Bytes, Infallible> = if reply.body_delay.is_zero() {
                    Full::new(Bytes::from(reply.body)).boxed()
                } else {
                    let (delay, data) = (reply.body_delay, Bytes::from(reply.body));
                    let len = data.len();
                    response = response.header("content-length", len.to_string());
                    StreamBody::new(futures::stream::once(async move {
                        tokio::time::sleep(delay).await;
                        Ok::<_, Infallible>(Frame::data(data))
                    }))
                    .boxed()
                };
                Ok::<_, Infallible>(response.body(body).expect("fixture response"))
            }
        });
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(hyper_util::rt::TokioIo::new(io), service)
        .await;
}

/// 사용하지 않는 포트(닫힌 포트) 하나.
pub fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind owned fixture");
    listener.local_addr().expect("fixture address").port()
}
