use crate::LspError;
use serde::Deserialize;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: u32,
    pub column_utf16: u32,
    pub severity: Severity,
    pub message: String,
}
#[derive(Debug, Clone)]
pub struct Diagnostics {
    pub uri: String,
    pub version: Option<i64>,
    pub items: Vec<Diagnostic>,
    pub overflow: bool,
}
#[derive(Deserialize)]
struct Publish {
    uri: String,
    version: Option<i64>,
    diagnostics: Vec<WireDiagnostic>,
}
#[derive(Deserialize)]
struct WireDiagnostic {
    range: Range,
    severity: Option<u32>,
    message: String,
}
#[derive(Deserialize)]
struct Range {
    start: Position,
    end: Position,
}
#[derive(Deserialize)]
struct Position {
    line: u32,
    character: u32,
}
pub fn parse(json: &str) -> Result<Diagnostics, LspError> {
    let p: Publish = serde_json::from_str(json).map_err(|_| LspError::Protocol)?;
    if p.uri.len() > 16384 {
        return Err(LspError::LimitExceeded);
    }
    let mut overflow = p.diagnostics.len() > 2048;
    let mut items = Vec::new();
    for d in p.diagnostics.into_iter().take(2048) {
        if (d.range.end.line, d.range.end.character) < (d.range.start.line, d.range.start.character)
        {
            return Err(LspError::Protocol);
        }
        let mut message = d.message;
        if message.len() > 4096 {
            let mut end = 4096;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            overflow = true;
        }
        items.push(Diagnostic {
            line: d
                .range
                .start
                .line
                .checked_add(1)
                .ok_or(LspError::Protocol)?,
            column_utf16: d
                .range
                .start
                .character
                .checked_add(1)
                .ok_or(LspError::Protocol)?,
            severity: match d.severity {
                Some(1) => Severity::Error,
                Some(2) => Severity::Warning,
                _ => Severity::Info,
            },
            message,
        });
    }
    Ok(Diagnostics {
        uri: p.uri,
        version: p.version,
        items,
        overflow,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_use_utf16_one_based_positions_and_bound_messages() {
        let d=parse(r#"{"uri":"file:///owned/가%20😀.rs","version":9,"diagnostics":[{"range":{"start":{"line":2,"character":4},"end":{"line":2,"character":8}},"severity":2,"message":"가😀 warning"}]}"#).expect("diagnostics");
        assert_eq!(
            d.items[0],
            Diagnostic {
                line: 3,
                column_utf16: 5,
                severity: Severity::Warning,
                message: "가😀 warning".into()
            }
        );
        assert_eq!(d.version, Some(9));
        assert!(parse(r#"{"uri":"file:///a","diagnostics":[{"range":{"start":{"line":4294967295,"character":0},"end":{"line":0,"character":0}},"message":"x"}]}"#).is_err());
        assert!(
            parse(r#"{"uri":"file:///a","diagnostics":[]}"#)
                .expect("empty")
                .items
                .is_empty()
        );
    }
}
