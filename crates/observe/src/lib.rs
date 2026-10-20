//! Bounded log observation. Execution/container management remains in its own domain.
mod container;
mod docker_ports;
mod host;
mod host_model;
mod host_service;
mod logs;
mod model;
mod ports;
mod problem_ingest;
mod problem_model;
mod problem_store;
mod problems;
pub use problem_ingest::ProblemBatch;
pub use problem_model::*;
pub use problem_store::ProblemRun;
pub use problems::extract;
#[cfg(test)]
mod problems_tests;
mod processes;
mod search;
mod service;
mod sources;
mod system;
use devbox_protocol::{Exporter, method, topic};
pub use host::ProcFs;
pub use host_model::*;
pub use model::*;
pub use ports::{is_listening, is_listening_in, ports_listening_in};
pub use service::ObserveService;
pub use sources::{ResolvedSource, validate_sources};
method!(pub LogsOpen = "logs.open", stream, OpenLogs => devbox_protocol::StreamOpened, ObserveError);
method!(pub LogsSearch = "logs.search", query, SearchLogs => SearchResult, ObserveError);
method!(pub ObservePorts = "observe.ports", query, ObserveQuery => Vec<PortInfo>, ObserveError);
method!(pub ObserveProcesses = "observe.processes", query, ProcessesQuery => Vec<ProcessInfo>, ObserveError);
method!(pub ObserveKill = "observe.kill", mutation(always), KillProcess => KillResult, ObserveError);
method!(pub ObserveSystem = "observe.system", query, ObserveQuery => SystemInfo, ObserveError);
method!(pub ObserveProblems = "observe.problems", query, ProblemsQuery => ProblemsResult, ObserveError);
topic!(pub ObserveProblemsChanged = "observe.problems.changed", ProblemsChange);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ObserveProblems>();
    x.topic::<ObserveProblemsChanged>();
    x.method::<ObservePorts>();
    x.method::<ObserveProcesses>();
    x.method::<ObserveKill>();
    x.method::<ObserveSystem>();
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

#[cfg(test)]
mod ports_tests;
#[cfg(test)]
mod processes_tests;
#[cfg(test)]
mod system_tests;

#[cfg(test)]
mod problem_ingest_tests;
