//! Execution definitions owned by a project file or by this PC.
pub mod cron;
mod defs;
pub mod detect;
pub use detect::{DefinitionKind, DetectRuns, Suggestion, SuggestionSource};
mod execution;
pub mod exit_record;
mod graph;
pub mod launch;
#[cfg(test)]
mod launch_tests;
pub mod logfile;
mod model;
mod reconcile;
mod run_model;
mod run_store;
mod schedule_model;
mod schedule_service;
mod schedule_store;
pub mod scheduler;
mod service;
mod store;
mod watch;
pub use defs::{MergedDefinitions, merge, validate_definition};
use devbox_core::Migration;
pub use devbox_projects::run_config::*;
use devbox_protocol::{Exporter, method, topic};
pub use execution::{ExecutionConfig, Readiness, ReadyCheck, StartedRun, WorktreeLocation};
pub use graph::PreparedGraph;
pub use model::*;
pub use run_model::*;
pub use schedule_model::*;
pub use service::RuntimeService;

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        domain: "runtime",
        version: 1,
        sql: store::MIGRATION_1,
    },
    Migration {
        domain: "runtime",
        version: 2,
        sql: run_store::MIGRATION_2,
    },
    Migration {
        domain: "runtime",
        version: 3,
        sql: schedule_store::MIGRATION_3,
    },
];

method!(pub RuntimeDetect = "runtime.detect", query, DetectRuns => Vec<Suggestion>, RuntimeError);
method!(pub RuntimeList = "runtime.list", query, ListRuns => DefinitionList, RuntimeError);
method!(pub RuntimeDefine = "runtime.define", mutation, DefineRun => DefineResult, RuntimeError);
method!(pub RuntimeUndefine = "runtime.undefine", mutation(always), UndefineRun => devbox_protocol::Done, RuntimeError);
method!(pub RuntimeStart = "runtime.start", mutation, StartRun => Run, RuntimeError);
method!(pub RuntimeStop = "runtime.stop", mutation(always), RunId => Run, RuntimeError);
method!(pub RuntimeRestart = "runtime.restart", mutation, RunId => Run, RuntimeError);
method!(pub RuntimeExited = "runtime.exited", mutation(always), exit_record::Exited => devbox_protocol::Done, RuntimeError);
method!(pub RuntimeRuns = "runtime.runs", query, RunsQuery => RunsResult, RuntimeError);
method!(pub RuntimeSchedules = "runtime.schedules", query, SchedulesQuery => SchedulesResult, RuntimeError);
topic!(pub RuntimeChanged = "runtime.changed", RuntimeChange);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<RuntimeDetect>();
    x.method::<RuntimeSchedules>();
    x.method::<RuntimeStart>();
    x.method::<RuntimeStop>();
    x.method::<RuntimeRestart>();
    x.method::<RuntimeExited>();
    x.method::<RuntimeRuns>();
    x.method::<RuntimeList>();
    x.method::<RuntimeDefine>();
    x.method::<RuntimeUndefine>();
    x.topic::<RuntimeChanged>();
}

type R<T> = Result<T, devbox_protocol::Fail<RuntimeError>>;

#[cfg(test)]
mod defs_tests;
#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod scheduler_tests;

#[cfg(test)]
mod detect_tests;
