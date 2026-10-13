pub mod hooks;
pub mod model;
pub mod plan;
pub mod profile;
pub mod service;
pub mod start;
pub mod state;
pub mod store;
pub mod tracker;
use devbox_core::Migration;
use devbox_protocol::{Exporter, method, topic};
pub use model::*;
pub use service::AgentsService;
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        domain: "agents",
        version: 1,
        sql: store::MIGRATION_1,
    },
    Migration {
        domain: "agents",
        version: 2,
        sql: store::MIGRATION_2,
    },
];
method!(pub AgentsCreate = "agents.create", mutation, CreateAgentTask => AgentTask, AgentsError);
method!(pub AgentsListMethod = "agents.list", query, ListAgents => AgentsList, AgentsError);
method!(pub AgentsGet = "agents.get", query, GetAgent => AgentTask, AgentsError);
method!(pub AgentsProfiles = "agents.profiles", query, devbox_protocol::system::PingParams => AgentProfiles, AgentsError);
method!(pub AgentsEvent = "agents.event", mutation, AgentEventParams => devbox_protocol::Done, AgentsError);
topic!(pub AgentsChangedTopic = "agents.changed", AgentsChanged);
topic!(pub AgentsAttentionTopic = "agents.attention", AgentsAttention);
pub fn export<X: Exporter>(e: &mut X) {
    e.method::<AgentsCreate>();
    e.method::<AgentsListMethod>();
    e.method::<AgentsGet>();
    e.method::<AgentsProfiles>();
    e.method::<AgentsEvent>();
    e.topic::<AgentsChangedTopic>();
    e.topic::<AgentsAttentionTopic>();
}

mod prepare;

mod lifecycle;
