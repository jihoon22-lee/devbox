mod checks;
mod probe;

pub use checks::{checks, tmux_at_least};
pub use probe::Probe;

#[cfg(test)]
mod checks_tests;
