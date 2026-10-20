//! Execution definitions owned by a project file or by this PC.
mod container_model;
mod containers;
pub use container_model::*;
pub use containers::DockerContainers;
pub mod cron;
mod defs;
pub mod detect;
pub use detect::{DefinitionKind, DetectRuns, Suggestion, SuggestionSource};
mod agent_test;
mod execution;
pub use agent_test::AgentTestRequest;
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
pub mod windows;
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

method!(pub RuntimeContainers = "runtime.containers", query, ContainersQuery => ContainersResult, ContainerError);
method!(pub RuntimeContainerAction = "runtime.container_action", mutation(reversible), ContainerActionParams => ContainerActionResult, ContainerError);
method!(pub RuntimeDetect = "runtime.detect", query, DetectRuns => Vec<Suggestion>, RuntimeError);
method!(pub RuntimeList = "runtime.list", query, ListRuns => DefinitionList, RuntimeError);
method!(pub RuntimeDefine = "runtime.define", mutation, DefineRun => DefineResult, RuntimeError);
method!(pub RuntimeUndefine = "runtime.undefine", mutation(always), UndefineRun => devbox_protocol::Done, RuntimeError);
method!(pub RuntimeStart = "runtime.start", mutation, StartRun => Run, RuntimeError);
method!(pub RuntimeStop = "runtime.stop", mutation(reversible), RunId => Run, RuntimeError);
method!(pub RuntimeRestart = "runtime.restart", mutation, RunId => Run, RuntimeError);
method!(pub RuntimeExited = "runtime.exited", mutation(always), exit_record::Exited => devbox_protocol::Done, RuntimeError);
method!(pub RuntimeGet = "runtime.get", query, RunId => RunDetail, RuntimeError);
method!(pub RuntimeSummary = "runtime.summary", query, RunSummaryQuery => RunSummary, RuntimeError);
method!(pub RuntimeRuns = "runtime.runs", query, RunsQuery => RunsResult, RuntimeError);
method!(pub RuntimeSchedules = "runtime.schedules", query, SchedulesQuery => SchedulesResult, RuntimeError);
topic!(pub RuntimeChanged = "runtime.changed", RuntimeChange);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<RuntimeContainers>();
    x.method::<RuntimeContainerAction>();
    x.method::<RuntimeDetect>();
    x.method::<RuntimeSchedules>();
    x.method::<RuntimeStart>();
    x.method::<RuntimeStop>();
    x.method::<RuntimeRestart>();
    x.method::<RuntimeExited>();
    x.method::<RuntimeRuns>();
    x.method::<RuntimeSummary>();
    x.method::<RuntimeGet>();
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
#[cfg(test)]
mod unit_lookup_tests;

mod unit_lookup;
pub use unit_lookup::LiveUnitRun;

mod log_observation;
pub use log_observation::{RunLocationContext, RunLogMetadata, RunLogPage};

#[cfg(test)]
mod containers_tests;

pub mod secret_input;

#[cfg(test)]
mod windows_tests;

#[cfg(test)]
mod run_ui_tests;

#[cfg(test)]
mod agent_test_tests;
