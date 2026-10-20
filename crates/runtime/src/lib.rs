//! Execution definitions owned by a project file or by this PC.
pub mod cron;
mod defs;
mod model;
mod service;
mod store;
mod watch;
pub use defs::{MergedDefinitions, merge, validate_definition};
use devbox_core::Migration;
pub use devbox_projects::run_config::*;
use devbox_protocol::{Exporter, method, topic};
pub use model::*;
pub use service::RuntimeService;

pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "runtime",
    version: 1,
    sql: store::MIGRATION_1,
}];

method!(pub RuntimeList = "runtime.list", query, ListRuns => DefinitionList, RuntimeError);
method!(pub RuntimeDefine = "runtime.define", mutation, DefineRun => DefineResult, RuntimeError);
method!(pub RuntimeUndefine = "runtime.undefine", mutation(always), UndefineRun => devbox_protocol::Done, RuntimeError);
topic!(pub RuntimeChanged = "runtime.changed", RuntimeChange);

pub fn export<X: Exporter>(x: &mut X) {
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
