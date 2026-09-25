//! Pure scheduling vocabulary shared by Workspace and its engine call enums.
//! The host owns quotas, counters and workers; engines never depend on the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Lane {
    Terminal,
    TerminalIo,
    TerminalStop,
    Engine,
    EngineStop,
    EngineBackground,
    Source,
    Files,
    FilesWatch,
    Lsp,
    LspStop,
    Metadata,
    Probes,
    Dialogs,
    Context,
}
pub const DEFAULT_BUDGET_MS: u64 = 5_000;
pub const LONG_BUDGET_MS: u64 = 29_000;
