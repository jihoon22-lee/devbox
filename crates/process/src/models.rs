use crate::ProcessId;
use serde::Serialize;

/// A single network port entry returned by a netstat-style data source.
///
/// This value contains only parsed data. It intentionally does not contain
/// UI-specific process details or any platform command state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortInfo {
    /// Protocol (for example, `TCP`, `TCP6`, or `UDP`).
    pub proto: String,
    /// Local address string (for example, `0.0.0.0:3000` or `[::1]:3000`).
    pub local_addr: String,
    /// Parsed port number; `0` means the address did not contain a port.
    pub port: u16,
    /// Connection state (`LISTENING`, `ESTABLISHED`, or empty for UDP).
    pub state: String,
    /// Owning process ID, if the source provided one.
    pub pid: Option<ProcessId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PortInfo {
        PortInfo {
            proto: "TCP".into(),
            local_addr: "0.0.0.0:3000".into(),
            port: 3000,
            state: "LISTENING".into(),
            pid: Some(18231),
        }
    }

    #[test]
    fn serde_serializes_fields() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("\"port\":3000"));
        assert!(json.contains("\"pid\":18231"));
    }
}
