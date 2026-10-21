//! API 요청 실행: 검사한 snapshot을 보내고 응답을 스트림으로 돌려주며 기록을 남긴다.
mod auth;
mod cookies;
mod error;
mod events;
mod history;
mod http;
mod model;
mod oauth_loopback;
mod redact;
mod service;
mod template;
mod tls;
mod validate;

use devbox_protocol::{Exporter, method};
pub use error::ApiError;
pub use history::API_INDEX_VERSION;
pub use model::*;
pub use service::{ApiOptions, ApiService, Clock};

method!(pub ApiSend = "api.send", stream, SendRequest => devbox_protocol::StreamOpened, ApiError);
method!(pub ApiHistory = "api.history", query, HistoryQuery => HistoryPage, ApiError);
method!(pub ApiHistoryGet = "api.history_get", query, HistoryId => HistoryEntry, ApiError);
method!(pub ApiCookies = "api.cookies", query, CookieJarRef => CookieList, ApiError);
method!(pub ApiCookiesClear = "api.cookies_clear", mutation(always), CookieJarRef => devbox_protocol::Done, ApiError);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ApiSend>();
    x.method::<ApiHistory>();
    x.method::<ApiHistoryGet>();
    x.method::<ApiCookies>();
    x.method::<ApiCookiesClear>();
}

pub type R<T> = Result<T, devbox_protocol::Fail<ApiError>>;

/// S2와 같은 가림 문자열.
pub const REDACTED: &str = "[REDACTED]";

#[cfg(test)]
mod template_tests;
