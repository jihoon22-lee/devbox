//! gRPC fixture: 시험 안의 proto 텍스트를 protox로 해석해 reflection 서버에 넣고(v0.9.0 방식),
//! `/demo.Echo/*`는 prost codec으로 직접 처리하는 소유 tonic 서버(127.0.0.1:0). 연결 수·취소를 센다.
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use tonic::codegen::{BoxFuture, Service, http};
use tonic::{Request, Response, Status};

pub const PROTO: &str = r#"syntax = "proto3";
package demo;
message Payload {
  string name = 1;
  int64 count = 2;
}
service Echo {
  rpc Unary(Payload) returns (Payload);
  rpc Server(Payload) returns (stream Payload);
  rpc Slow(Payload) returns (stream Payload);
  rpc Fail(Payload) returns (Payload);
  rpc Client(stream Payload) returns (Payload);
}
"#;

#[derive(Clone, PartialEq, prost::Message)]
pub struct Payload {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int64, tag = "2")]
    pub count: i64,
}

#[derive(Default)]
pub struct State {
    pub connections: AtomicUsize,
    pub calls: AtomicUsize,
    /// `Slow` 응답 스트림이 버려진 수(화면이 닫아 클라이언트가 취소함).
    pub cancelled: AtomicUsize,
    pub metadata: std::sync::Mutex<Vec<(String, String)>>,
    pub changed: tokio::sync::Notify,
}

/// proto 파일을 임시 폴더에 쓰고 protox로 만든 encoded FileDescriptorSet과 경로를 돌려준다.
pub fn proto_file(dir: &std::path::Path) -> (String, Vec<u8>) {
    let path = dir.join("echo.proto");
    std::fs::write(&path, PROTO).expect("write fixture proto");
    let set = protox::compile([&path], [dir]).expect("compile fixture proto");
    (
        path.display().to_string(),
        prost::Message::encode_to_vec(&set),
    )
}

type Messages = Pin<Box<dyn futures::Stream<Item = Result<Payload, Status>> + Send + 'static>>;

struct Guard(Arc<State>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.cancelled.fetch_add(1, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}

struct Unary;
impl tonic::server::UnaryService<Payload> for Unary {
    type Response = Payload;
    type Future = BoxFuture<Response<Payload>, Status>;
    fn call(&mut self, request: Request<Payload>) -> Self::Future {
        Box::pin(async move {
            let echo = request
                .metadata()
                .get("x-echo")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut payload = request.into_inner();
            // `{"name":"big","count":N}`: N바이트 이름으로 답한다(응답 디코딩 상한 시험).
            if payload.name == "big" {
                payload.name = "x".repeat(payload.count as usize);
            }
            let mut response = Response::new(payload);
            if let Some(echo) = echo
                && let Ok(value) = echo.parse()
            {
                response.metadata_mut().insert("x-echo", value);
            }
            Ok(response)
        })
    }
}

struct Failing;
impl tonic::server::UnaryService<Payload> for Failing {
    type Response = Payload;
    type Future = BoxFuture<Response<Payload>, Status>;
    fn call(&mut self, request: Request<Payload>) -> Self::Future {
        let name = request.into_inner().name;
        Box::pin(async move { Err(Status::permission_denied(format!("denied:{name}"))) })
    }
}

struct Streaming(Arc<State>, bool);
impl tonic::server::ServerStreamingService<Payload> for Streaming {
    type Response = Payload;
    type ResponseStream = Messages;
    type Future = BoxFuture<Response<Messages>, Status>;
    fn call(&mut self, request: Request<Payload>) -> Self::Future {
        let (state, slow) = (self.0.clone(), self.1);
        Box::pin(async move {
            let first = request.into_inner();
            let stream: Messages = if slow {
                let guard = Guard(state);
                Box::pin(futures::stream::unfold(
                    (0, guard),
                    move |(i, guard)| async move {
                        if i > 0 {
                            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        }
                        Some((
                            Ok(Payload {
                                name: format!("slow-{i}"),
                                count: i,
                            }),
                            (i + 1, guard),
                        ))
                    },
                ))
            } else {
                let items: Vec<_> = (0..first.count)
                    .map(|i| {
                        Ok(Payload {
                            name: format!("{}-{i}", first.name),
                            count: i,
                        })
                    })
                    .collect();
                Box::pin(futures::stream::iter(items))
            };
            Ok(Response::new(stream))
        })
    }
}

#[derive(Clone)]
struct Echo(Arc<State>);
impl tonic::server::NamedService for Echo {
    const NAME: &'static str = "demo.Echo";
}
impl Service<http::Request<tonic::body::Body>> for Echo {
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        let state = self.0.clone();
        state.calls.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut seen) = state.metadata.lock() {
            for (name, value) in request.headers() {
                seen.push((
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                ));
            }
        }
        let codec = || tonic_prost::ProstCodec::<Payload, Payload>::default();
        let path = request.uri().path().to_owned();
        Box::pin(async move {
            let mut grpc = tonic::server::Grpc::new(codec());
            Ok(match path.as_str() {
                "/demo.Echo/Unary" => grpc.unary(Unary, request).await,
                "/demo.Echo/Fail" => grpc.unary(Failing, request).await,
                "/demo.Echo/Server" => {
                    grpc.server_streaming(Streaming(state, false), request)
                        .await
                }
                "/demo.Echo/Slow" => grpc.server_streaming(Streaming(state, true), request).await,
                _ => Status::unimplemented("").into_http(),
            })
        })
    }
}

pub struct GrpcServer {
    pub state: Arc<State>,
    pub address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for GrpcServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl GrpcServer {
    /// `reflection`이 있으면 그 encoded FileDescriptorSet으로 reflection v1·v1alpha를 함께 띄운다.
    pub async fn start(reflection: Option<Vec<u8>>) -> Self {
        Self::start_with(reflection, false).await
    }
    /// `alpha`면 reflection v1alpha만 띄운다(v1은 Unimplemented → 클라이언트가 v1alpha로 넘어가야 함).
    pub async fn start_with(reflection: Option<Vec<u8>>, alpha: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned fixture");
        let address = listener.local_addr().expect("address");
        let state = Arc::new(State::default());
        let counted = state.clone();
        let incoming = futures::stream::unfold(listener, move |listener| {
            let counted = counted.clone();
            async move {
                let accepted = listener.accept().await.map(|(s, _)| s);
                counted.connections.fetch_add(1, Ordering::SeqCst);
                Some((accepted, listener))
            }
        });
        let mut router = tonic::transport::Server::builder().add_service(Echo(state.clone()));
        if let Some(set) = reflection {
            let builder = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(&set);
            router = if alpha {
                router.add_service(builder.build_v1alpha().expect("reflection v1alpha"))
            } else {
                router.add_service(builder.build_v1().expect("reflection v1"))
            };
        }
        let task = tokio::spawn(async move {
            let _ = router.serve_with_incoming(incoming).await;
        });
        Self {
            state,
            address,
            task,
        }
    }
    pub fn target(&self) -> String {
        format!("http://{}", self.address)
    }
    pub fn connections(&self) -> usize {
        self.state.connections.load(Ordering::SeqCst)
    }
    pub fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }
    pub fn seen(&self, name: &str) -> Vec<String> {
        self.state
            .metadata
            .lock()
            .expect("metadata")
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
    pub async fn wait_cancelled(&self, timeout: std::time::Duration) -> bool {
        let state = self.state.clone();
        tokio::time::timeout(timeout, async move {
            loop {
                let changed = state.changed.notified();
                if state.cancelled.load(Ordering::SeqCst) > 0 {
                    return;
                }
                changed.await;
            }
        })
        .await
        .is_ok()
    }
}
