//! Bounded log observation. Execution/container management remains in its own domain.
mod container;
mod logs;
mod model;
mod search;
mod service;
mod sources;
use devbox_protocol::{Exporter, method};
pub use model::*;
pub use service::ObserveService;
pub use sources::{ResolvedSource, validate_sources};
method!(pub LogsOpen = "logs.open", stream, OpenLogs => devbox_protocol::StreamOpened, ObserveError);
method!(pub LogsSearch = "logs.search", query, SearchLogs => SearchResult, ObserveError);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<LogsOpen>();
    x.method::<LogsSearch>();
}
pub(crate) type R<T> = Result<T, devbox_protocol::Fail<ObserveError>>;
pub(crate) const MAX_LINE: usize = 3 * 1024 * 1024 + 128;
pub(crate) const MAX_SOURCES: usize = 8;
#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod logs_tests;
#[cfg(test)]
mod stream_tests;
