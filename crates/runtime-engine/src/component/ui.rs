//! UI-only facilities remain in Workspace when execution is hosted by the agent.
//! None of these functions initialize or acquire the runtime database.
pub use crate::applink::{is_supported_request, PendingOpen};
pub use crate::commands::{hide_main_window, show_main_window};
