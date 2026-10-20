//! Shared system-record boundary for one run invocation (stdout cannot forge it).
const PREFIX: &str = "실행 시도 시작: ";
pub fn attempt_marker(attempt: &str) -> String {
    format!("{PREFIX}{attempt}")
}
pub fn parse_attempt_marker(text: &str) -> Option<&str> {
    let id = text.strip_prefix(PREFIX)?;
    (!id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
    .then_some(id)
}
