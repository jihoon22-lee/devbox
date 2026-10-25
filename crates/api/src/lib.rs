//! API 요청 실행: 검사한 snapshot을 보내고 응답을 스트림으로 돌려주며 기록을 남긴다.
mod auth;
mod collections;
mod cookies;
mod error;
mod events;
mod grpc;
mod history;
mod http;
mod mcp;
mod model;
mod oauth_loopback;
mod realtime;
mod redact;
mod response_export;
mod service;
mod store;
mod template;
mod tls;
mod validate;

pub use collections::CollectionsService;
pub use collections::model::{
    CollectionExpect, CollectionFile, CollectionList, CollectionsChange, CollectionsQuery,
    DeleteFile, FileRef, FileText, ReviewFile, WriteFile, Written,
};
use devbox_protocol::{Exporter, method, topic};
pub use error::ApiError;
pub use grpc::{
    CatalogSource, GrpcCall, GrpcCatalog, GrpcEvent, GrpcMethod, GrpcService, GrpcTarget,
};
pub use history::API_INDEX_VERSION;
pub use mcp::{McpConnect, McpEvent, McpInput, McpList, McpSource, McpTransport};
pub use model::*;
pub use realtime::{Direction, MAX_SESSIONS, MessageKind, RealtimeEvent, SseOpen, WsInput, WsOpen};
pub use response_export::ResponseNoteExport;
pub use service::{ApiOptions, ApiService, Clock};
pub use store::MIGRATIONS;

method!(pub ApiSend = "api.send", stream, SendRequest => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiWsOpen = "api.ws_open", stream, WsOpen => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiSseOpen = "api.sse_open", stream, SseOpen => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiGrpcDescribe = "api.grpc_describe", query, GrpcTarget => GrpcCatalog, ApiError);
method!(pub ApiGrpcCall = "api.grpc_call", stream, GrpcCall => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiMcpConnect = "api.mcp_connect", stream, McpConnect => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiHistory = "api.history", query, HistoryQuery => HistoryPage, ApiError);
method!(pub ApiHistoryGet = "api.history_get", query, HistoryId => HistoryEntry, ApiError);
method!(pub ApiCookies = "api.cookies", query, CookieJarRef => CookieList, ApiError);
method!(pub ApiCookiesClear = "api.cookies_clear", mutation(always), CookieJarRef => devbox_protocol::Done, ApiError);
method!(pub ApiCollections = "api.collections", query, CollectionsQuery => CollectionList, ApiError);
method!(pub ApiCollectionRead = "api.collection_read", query, FileRef => FileText, ApiError);
method!(pub ApiCollectionWrite = "api.collection_write", mutation, WriteFile => Written, ApiError);
method!(pub ApiCollectionDelete = "api.collection_delete", mutation(always), DeleteFile => devbox_protocol::Done, ApiError);
method!(pub ApiCollectionReview = "api.collection_review", mutation, ReviewFile => devbox_protocol::Done, ApiError);
topic!(pub ApiCollectionsChanged = "api.collections_changed", CollectionsChange);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ApiSend>();
    x.method::<ApiWsOpen>();
    x.method::<ApiSseOpen>();
    x.method::<ApiGrpcDescribe>();
    x.method::<ApiGrpcCall>();
    x.method::<ApiMcpConnect>();
    x.method::<ApiHistory>();
    x.method::<ApiHistoryGet>();
    x.method::<ApiCookies>();
    x.method::<ApiCookiesClear>();
    x.method::<ApiCollections>();
    x.method::<ApiCollectionRead>();
    x.method::<ApiCollectionWrite>();
    x.method::<ApiCollectionDelete>();
    x.method::<ApiCollectionReview>();
    x.topic::<ApiCollectionsChanged>();
}

pub type R<T> = Result<T, devbox_protocol::Fail<ApiError>>;

/// S2와 같은 가림 문자열.
pub const REDACTED: &str = "[REDACTED]";

#[cfg(test)]
mod template_tests;
