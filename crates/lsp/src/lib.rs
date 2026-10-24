pub mod framing;

pub mod supervisor;
pub mod uri;

mod model;
pub use model::*;
pub mod servers;

pub mod adapters;
pub mod relay;

mod relay_documents;

pub mod diagnostics;

mod service;
pub use service::{DiagnosticEvent, DiagnosticSink, LspService, ProjectContext};

mod actor;
mod process;
mod usage;
mod wire;

use devbox_protocol::{Exporter, method, topic};
method!(pub LspOpen = "lsp.open", stream, OpenLsp => devbox_protocol::StreamOpened, LspError);
method!(pub LspServers = "lsp.servers", query, ServersQuery => ServersResult, LspError);
method!(pub LspRestart = "lsp.restart", mutation, OpenLsp => devbox_protocol::Done, LspError);
method!(pub LspInstall = "lsp.install", mutation, InstallLsp => Installed, LspError);
topic!(pub LspRestarted = "lsp.restarted", Restarted);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<LspOpen>();
    x.method::<LspServers>();
    x.method::<LspRestart>();
    x.method::<LspInstall>();
    x.topic::<LspRestarted>();
}
