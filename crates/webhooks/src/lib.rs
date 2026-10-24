//! Local webhook listeners and durable request history.
pub mod address;
mod error;
mod history;
mod listener;
mod matcher;
mod model;
pub mod rules;
mod sequence;
mod service;
mod store;
use devbox_protocol::{Exporter, method, topic};
pub use error::WebhooksError;
pub use model::*;
pub use rules::{ResponseRule, ResponseSequenceStep};
pub use service::WebhooksService;
pub use store::MIGRATIONS;
pub type R<T> = Result<T, devbox_protocol::Fail<WebhooksError>>;
method!(pub WebhooksListeners = "webhooks.listeners", query, Empty => ListenerList, WebhooksError);
method!(pub WebhooksStart = "webhooks.start", mutation, StartListener => Listener, WebhooksError);
method!(pub WebhooksStop = "webhooks.stop", mutation, ListenerId => devbox_protocol::Done, WebhooksError);
method!(pub WebhooksRules = "webhooks.rules", query, ListenerId => RuleList, WebhooksError);
method!(pub WebhooksRuleUpsert = "webhooks.rule_upsert", mutation, UpsertRule => ResponseRule, WebhooksError);
method!(pub WebhooksRuleDelete = "webhooks.rule_delete", mutation(always), RuleRef => devbox_protocol::Done, WebhooksError);
method!(pub WebhooksHistory = "webhooks.history", query, HistoryQuery => ReceivedPage, WebhooksError);
method!(pub WebhooksHistoryGet = "webhooks.history_get", query, ReceivedId => ReceivedDetail, WebhooksError);
method!(pub WebhooksFixtureSave = "webhooks.fixture_save", mutation, SaveFixture => Fixture, WebhooksError);
topic!(pub WebhooksReceived = "webhooks.received", Received);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<WebhooksListeners>();
    x.method::<WebhooksStart>();
    x.method::<WebhooksStop>();
    x.method::<WebhooksRules>();
    x.method::<WebhooksRuleUpsert>();
    x.method::<WebhooksRuleDelete>();
    x.method::<WebhooksHistory>();
    x.method::<WebhooksHistoryGet>();
    x.method::<WebhooksFixtureSave>();
    x.topic::<WebhooksReceived>();
}
#[cfg(test)]
mod history_tests;
