use sysinfo::{Pid, System};

/// Process identifier used by the cross-platform process utilities.
pub type ProcessId = u32;

/// Minimal process data shared by consumers.
///
/// UI-specific fields such as executable path, start time, and memory remain
/// in the consuming app's DTO instead of becoming part of this crate's API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSummary {
    pub pid: ProcessId,
    pub name: String,
}

/// A point-in-time process table backed by `sysinfo`.
///
/// Keeping the snapshot as a wrapper avoids exposing `sysinfo` types to apps
/// while allowing callers that inspect several PIDs (such as port-manager) to
/// refresh the operating-system process list only once.
pub struct ProcessSnapshot {
    system: System,
}

impl ProcessSnapshot {
    /// Creates a process snapshot using the current operating-system state.
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
        }
    }

    /// Looks up a process and returns only data that is generic across UI
    /// consumers.
    pub fn lookup(&self, pid: ProcessId) -> Option<ProcessSummary> {
        self.system
            .process(Pid::from_u32(pid))
            .map(|process| ProcessSummary {
                pid,
                name: process.name().to_string_lossy().into_owned(),
            })
    }

    /// Reports whether the PID exists in this snapshot.
    pub fn is_alive(&self, pid: ProcessId) -> bool {
        self.lookup(pid).is_some()
    }
}

impl Default for ProcessSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Looks up one process using a fresh snapshot.
pub fn lookup_process(pid: ProcessId) -> Option<ProcessSummary> {
    ProcessSnapshot::new().lookup(pid)
}

/// Checks one PID using a fresh snapshot.
pub fn is_process_alive(pid: ProcessId) -> bool {
    ProcessSnapshot::new().is_alive(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_finds_current_process() {
        let pid = std::process::id();
        let snapshot = ProcessSnapshot::new();
        let summary = snapshot.lookup(pid).expect("current process is visible");

        assert_eq!(summary.pid, pid);
        assert!(snapshot.is_alive(pid));
    }

    #[test]
    fn unknown_pid_is_not_alive() {
        let pid = u32::MAX;
        let snapshot = ProcessSnapshot::new();

        assert!(snapshot.lookup(pid).is_none());
        assert!(!snapshot.is_alive(pid));
        assert!(lookup_process(pid).is_none());
        assert!(!is_process_alive(pid));
    }
}
