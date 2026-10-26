#[cfg(any(windows, test))]
pub mod activity;
#[cfg(any(windows, test))]
pub mod bridge;
pub mod channel;
#[cfg(any(windows, test))]
pub mod connection;
#[cfg(any(windows, test))]
pub mod deploy;
#[cfg(any(windows, test))]
pub mod geometry;
#[cfg(any(windows, test))]
pub mod process;
pub mod self_check;
#[cfg(any(windows, test))]
pub mod settings;
#[cfg(any(windows, test))]
pub mod supervision;
#[cfg(any(windows, test))]
pub mod wsl;

#[cfg(any(windows, test))]
pub mod shutdown;

#[cfg(any(windows, test))]
pub mod diagnostics;
#[cfg(any(windows, test))]
pub mod external;
#[cfg(any(windows, test))]
pub mod source_external;

#[cfg(any(windows, test))]
pub mod intents;
#[cfg(any(windows, test))]
pub mod notify;
#[cfg(any(windows, test))]
pub mod preferences;
#[cfg(any(windows, test))]
pub mod shortcuts;

#[cfg(any(windows, test))]
pub mod secrets;

#[cfg(any(windows, test))]
pub mod windows;

#[cfg(any(windows, test))]
pub mod install;
#[cfg(any(windows, test))]
pub mod maintenance;
#[cfg(any(windows, test))]
pub mod quit;

#[cfg(any(windows, test))]
pub mod update;
