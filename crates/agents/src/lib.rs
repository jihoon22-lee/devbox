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
pub use limits::input as validate_input;
pub use model::*;
pub use service::{AgentRuntime, AgentsService};
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
    Migration {
        domain: "agents",
        version: 3,
        sql: store::MIGRATION_3,
    },
];
method!(pub AgentsCreate = "agents.create", mutation, CreateAgentTask => AgentTask, AgentsError);
method!(pub AgentsListMethod = "agents.list", query, ListAgents => AgentsList, AgentsError);
method!(pub AgentsGet = "agents.get", query, GetAgent => AgentTask, AgentsError);
method!(pub AgentsProfiles = "agents.profiles", query, devbox_protocol::system::PingParams => AgentProfiles, AgentsError);
method!(pub AgentsEvent = "agents.event", mutation, AgentEventParams => devbox_protocol::Done, AgentsError);
method!(pub AgentsSend = "agents.send", mutation, SendToAgent => AgentTask, AgentsError);
method!(pub AgentsResume = "agents.resume", mutation, GetAgent => AgentTask, AgentsError);
method!(pub AgentsReview = "agents.review", mutation, GetAgent => AgentTask, AgentsError);
method!(pub AgentsRebase = "agents.rebase", mutation, GetAgent => AgentRebase, AgentsError);
method!(pub AgentsRebaseAbort = "agents.rebase_abort", mutation, GetAgent => AgentTask, AgentsError);
method!(pub AgentsMerge = "agents.merge", mutation(always), MergeAgent => AgentTask, AgentsError);
method!(pub AgentsPr = "agents.pr", mutation(always), PrAgent => AgentTask, AgentsError);
method!(pub AgentsDiscardCheck = "agents.discard_check", query, GetAgent => DiscardCheck, AgentsError);
method!(pub AgentsDiscard = "agents.discard", mutation(always), DiscardAgent => AgentTask, AgentsError);
method!(pub AgentsTest = "agents.test", mutation, GetAgent => AgentTest, AgentsError);
method!(pub AgentsCleanupCandidates = "agents.cleanup_candidates", query, CleanupCandidates => CleanupOptions, AgentsError);
method!(pub AgentsCleanup = "agents.cleanup", mutation(always), CleanupAgents => devbox_protocol::Done, AgentsError);
method!(pub AgentsChanges = "agents.changes", query, GetAgent => AgentChanges, AgentsError);
method!(pub AgentsPatch = "agents.patch", query, GetAgentPatch => AgentPatch, AgentsError);
method!(pub AgentsCommits = "agents.commits", query, GetAgent => AgentCommits, AgentsError);
method!(pub AgentsUsage = "agents.usage", query, GetAgent => AgentUsage, AgentsError);
method!(pub AgentsGitStats = "agents.git_stats", query, devbox_protocol::system::PingParams => AgentGitStats, AgentsError);
method!(pub AgentsResources = "agents.resources", query, devbox_protocol::system::PingParams => AgentResources, AgentsError);
topic!(pub AgentsChangedTopic = "agents.changed", AgentsChanged);
topic!(pub AgentsAttentionTopic = "agents.attention", AgentsAttention);
pub fn export<X: Exporter>(e: &mut X) {
    e.method::<AgentsCreate>();
    e.method::<AgentsListMethod>();
    e.method::<AgentsGet>();
    e.method::<AgentsProfiles>();
    e.method::<AgentsEvent>();
    e.method::<AgentsSend>();
    e.method::<AgentsResume>();
    e.method::<AgentsReview>();
    e.method::<AgentsRebase>();
    e.method::<AgentsRebaseAbort>();
    e.method::<AgentsMerge>();
    e.method::<AgentsPr>();
    e.method::<AgentsDiscardCheck>();
    e.method::<AgentsDiscard>();
    e.method::<AgentsTest>();
    e.method::<AgentsCleanupCandidates>();
    e.method::<AgentsCleanup>();
    e.method::<AgentsChanges>();
    e.method::<AgentsPatch>();
    e.method::<AgentsCommits>();
    e.method::<AgentsUsage>();
    e.method::<AgentsGitStats>();
    e.method::<AgentsResources>();
    e.topic::<AgentsChangedTopic>();
    e.topic::<AgentsAttentionTopic>();
}

mod prepare;
mod tool_lifecycle;

mod lifecycle;

mod action_cleanup;
mod actions;
mod cleanup;
mod queries;
mod resume;

mod usage;

mod closed_review;
mod discard_snapshot;
mod limits;
mod listing;
mod metrics;
mod resources;
mod send;
mod watch;
