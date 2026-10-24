use devbox_core::Migration;
pub const MIGRATIONS: &[Migration] = &[Migration { domain: "webhooks", version: 1, sql: "
CREATE TABLE webhooks_listeners(id TEXT PRIMARY KEY, port INTEGER NOT NULL, lan INTEGER NOT NULL, created_ms INTEGER NOT NULL);
CREATE TABLE webhooks_rules(id TEXT NOT NULL, listener_id TEXT NOT NULL REFERENCES webhooks_listeners(id), priority INTEGER NOT NULL, method TEXT, path TEXT NOT NULL, response_json TEXT NOT NULL, PRIMARY KEY(listener_id,id));
CREATE TABLE webhooks_received(id TEXT PRIMARY KEY, listener_id TEXT NOT NULL REFERENCES webhooks_listeners(id), at_ms INTEGER NOT NULL, detail_json TEXT NOT NULL);
CREATE INDEX webhooks_received_time ON webhooks_received(listener_id,at_ms,id);
CREATE TABLE webhooks_fixtures(id TEXT PRIMARY KEY, name TEXT NOT NULL, request_json TEXT NOT NULL);
" }];
