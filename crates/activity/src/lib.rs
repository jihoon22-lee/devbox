//! Windows activity log: the v0.9.0 sessionizer, idle and privacy rules (`model`,
//! no daemon dependency) plus, with the `daemon` feature, rules in `config.toml
//! [activity]`, batched writes to `devbox.db` domain `activity` with a bounded
//! retry queue, day/range summaries and exports. Titles never reach logs.
mod dto;
pub mod model;

#[cfg(feature = "daemon")]
mod export;
#[cfg(feature = "daemon")]
pub mod rules;
#[cfg(feature = "daemon")]
mod service;
#[cfg(feature = "daemon")]
pub mod store;
#[cfg(feature = "daemon")]
pub mod writer;

use devbox_protocol::{Exporter, method, topic};
pub use dto::*;

#[cfg(feature = "daemon")]
pub use rules::ActivitySettings;
#[cfg(feature = "daemon")]
pub use service::{ActivityConfig, ActivityService, ProjectNames};
#[cfg(feature = "daemon")]
pub use store::{MIGRATIONS, Session, SessionSink, SinkError};

method!(pub ActivityIngest = "activity.ingest", mutation, Ingest => Ingested, ActivityError);
method!(pub ActivityGetStatus = "activity.status", query, NoParams => ActivityStatus, ActivityError);
method!(pub ActivityRules = "activity.rules", query, NoParams => RulesView, ActivityError);
method!(pub ActivitySetRules = "activity.set_rules", mutation, SetRules => RulesRev, ActivityError);
method!(pub ActivityDay = "activity.day", query, DayQuery => DayActivity, ActivityError);
method!(pub ActivityRange = "activity.range", query, RangeQuery => RangeActivity, ActivityError);
method!(pub ActivityExport = "activity.export", mutation, ExportQuery => Exported, ActivityError);
topic!(pub ActivityChangedTopic = "activity.changed", ActivityChanged);
topic!(pub ActivityStatusChangedTopic = "activity.status_changed", ActivityStatus);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<ActivityIngest>();
    x.method::<ActivityGetStatus>();
    x.method::<ActivityRules>();
    x.method::<ActivitySetRules>();
    x.method::<ActivityDay>();
    x.method::<ActivityRange>();
    x.method::<ActivityExport>();
    x.topic::<ActivityChangedTopic>();
    x.topic::<ActivityStatusChangedTopic>();
}

#[cfg(feature = "daemon")]
pub(crate) type R<T> = Result<T, devbox_protocol::Fail<ActivityError>>;

#[cfg(all(test, feature = "daemon"))]
mod fixture;
#[cfg(all(test, feature = "daemon"))]
mod ingest_tests;
#[cfg(all(test, feature = "daemon"))]
mod service_tests;
#[cfg(all(test, feature = "daemon"))]
mod store_tests;
