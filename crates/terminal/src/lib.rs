//! Devbox-owned tmux terminal sessions, metadata and typed RPC declarations.
mod attach;
mod attach_gate;
#[cfg(unix)]
mod attach_history;
pub mod model;
mod service;
mod store;
pub mod tmux;
use devbox_core::Migration;
use devbox_protocol::{Exporter, method, topic};
pub use model::*;
pub use service::{TerminalService, validate_title};
pub const TMUX_CONF: &str = include_str!("../assets/tmux.conf");
pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "terminal",
    version: 1,
    sql: store::MIGRATION_1,
}];
method!(pub TerminalAttach = "terminal.attach", stream, AttachSession => TerminalAttached, TerminalError);
method!(pub TerminalResize = "terminal.resize", mutation, ResizeStream => devbox_protocol::Done, TerminalError);
method!(pub TerminalList = "terminal.list", query, ListSessions => SessionList, TerminalError);
method!(pub TerminalCreate = "terminal.create", mutation, CreateSession => TerminalSession, TerminalError);
method!(pub TerminalRename = "terminal.rename", mutation, RenameSession => devbox_protocol::Done, TerminalError);
method!(pub TerminalClose = "terminal.close", mutation(always), CloseSession => devbox_protocol::Done, TerminalError);
method!(pub TerminalZellijList = "terminal.zellij_list", query, devbox_protocol::system::PingParams => ZellijSessions, TerminalError);
method!(pub Profiles = "terminal.profiles", query, devbox_protocol::system::PingParams => TerminalProfiles, TerminalError);
topic!(pub TerminalChangedTopic = "terminal.changed", TerminalChanged);
pub fn export<X: Exporter>(exporter: &mut X) {
    exporter.method::<TerminalAttach>();
    exporter.method::<TerminalResize>();
    exporter.method::<TerminalList>();
    exporter.method::<TerminalCreate>();
    exporter.method::<TerminalRename>();
    exporter.method::<TerminalClose>();
    exporter.method::<TerminalZellijList>();
    exporter.method::<Profiles>();
    exporter.topic::<TerminalChangedTopic>();
}
