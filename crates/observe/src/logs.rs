use crate::*;
use chrono::{DateTime, Utc};
use regex::Regex;

pub(crate) struct Filter {
    text: Option<String>,
    regex: Option<Regex>,
    streams: Option<Vec<LogStream>>,
}
impl Filter {
    pub(crate) fn new(filter: LogFilter) -> R<Self> {
        for value in [&filter.text, &filter.regex].into_iter().flatten() {
            devbox_core::input::validate_command_text(value)
                .map_err(|_| ObserveError::InvalidInput)?;
        }
        let regex = filter
            .regex
            .map(|r| Regex::new(&r))
            .transpose()
            .map_err(|_| ObserveError::InvalidRegex)?;
        Ok(Self {
            text: filter.text,
            regex,
            streams: filter.streams,
        })
    }
    pub(crate) fn accepts(&self, line: &LogLine) -> bool {
        self.streams
            .as_ref()
            .is_none_or(|s| s.contains(&line.stream))
            && self.text.as_ref().is_none_or(|s| line.text.contains(s))
            && self.regex.as_ref().is_none_or(|r| r.is_match(&line.text))
    }
}
pub(crate) fn timestamp(ts: &str) -> i128 {
    DateTime::parse_from_rfc3339(ts)
        .map(|t| i128::from(t.timestamp()) * 1_000_000_000 + i128::from(t.timestamp_subsec_nanos()))
        .unwrap_or(0)
}
/// Merge the available backlog frontier. Idle sources do not block live output;
/// a future arrival may have an earlier time. Equal times use source request order.
pub(crate) fn oldest_head(heads: &[Option<LogLine>]) -> Option<usize> {
    heads
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.as_ref().map(|l| (timestamp(&l.ts), i)))
        .min()
        .map(|(_, i)| i)
}
enum RecordFormat {
    File,
    Run,
    Docker,
}
pub(crate) fn parse(src: &LogSource, raw: &[u8], stream: LogStream) -> LogLine {
    let format = match src {
        LogSource::File { .. } => RecordFormat::File,
        LogSource::Run { .. } => RecordFormat::Run,
        LogSource::Container { .. } => RecordFormat::Docker,
    };
    parse_as(src, raw, stream, format)
}
/// The owned Docker history snapshot uses the run record encoding. Its format is
/// chosen by the reader, never inferred from tabs in a user's log text.
pub(crate) fn parse_snapshot(src: &LogSource, raw: &[u8], stream: LogStream) -> LogLine {
    parse_as(src, raw, stream, RecordFormat::Run)
}
fn parse_as(src: &LogSource, raw: &[u8], stream: LogStream, format: RecordFormat) -> LogLine {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim_end_matches(['\n', '\r']);
    let mut result = LogLine {
        src: src.clone(),
        ts: Utc::now().to_rfc3339(),
        stream,
        text: text.into(),
    };
    match format {
        RecordFormat::Run => {
            let mut parts = text.splitn(3, '\t');
            if let (Some(ts), Some(stream), Some(text)) = (parts.next(), parts.next(), parts.next())
                && DateTime::parse_from_rfc3339(ts).is_ok()
            {
                result.ts = ts.into();
                result.stream = match stream {
                    "e" => LogStream::Stderr,
                    "s" => LogStream::System,
                    _ => LogStream::Stdout,
                };
                result.text = text.into();
            }
        }
        RecordFormat::Docker => {
            if let Some((ts, text)) = text.split_once(' ')
                && DateTime::parse_from_rfc3339(ts).is_ok()
            {
                result.ts = ts.into();
                result.text = text.into();
            }
        }
        RecordFormat::File => (),
    }
    result
}
