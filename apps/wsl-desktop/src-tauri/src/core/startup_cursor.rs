//! The native owner answers ConPTY's initial cursor-position query before any
//! renderer attaches. Do not publish that query again to a terminal emulator:
//! its second reply would become unsolicited input in the now-running shell.
use std::borrow::Cow;
const QUERY: &[u8] = b"\x1b[6n";
pub(crate) struct StartupCursor {
    matched: usize,
    decided: bool,
}
impl StartupCursor {
    pub(crate) fn new(native_answered: bool) -> Self {
        Self {
            matched: 0,
            decided: !native_answered,
        }
    }
    pub(crate) fn filter<'a>(&mut self, bytes: &'a [u8]) -> Cow<'a, [u8]> {
        if self.decided {
            return Cow::Borrowed(bytes);
        }
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != QUERY[self.matched] {
                self.decided = true;
                let mut output = QUERY[..self.matched].to_vec();
                output.extend_from_slice(&bytes[index..]);
                self.matched = 0;
                return Cow::Owned(output);
            }
            self.matched += 1;
            if self.matched == QUERY.len() {
                self.decided = true;
                self.matched = 0;
                return Cow::Borrowed(&bytes[index + 1..]);
            }
        }
        Cow::Borrowed(&[])
    }
    pub(crate) fn finish(&mut self) -> Vec<u8> {
        let rest = QUERY[..self.matched].to_vec();
        self.matched = 0;
        self.decided = true;
        rest
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suppresses_only_the_answered_startup_query_at_every_pipe_split() {
        let stream = b"\x1b[6n\x1b[?9001hprompt# \x1b[6n";
        for split in 0..=stream.len() {
            let mut filter = StartupCursor::new(true);
            let mut output = filter.filter(&stream[..split]).into_owned();
            output.extend_from_slice(&filter.filter(&stream[split..]));
            output.extend(filter.finish());
            assert_eq!(output, &stream[4..]);
            // Replay contains only the later live application's query, never
            // the startup query whose native CPR was already consumed.
            assert!(!output.starts_with(QUERY));
        }
    }
    #[test]
    fn preserves_unanswered_queries_other_controls_and_partial_eof() {
        assert_eq!(StartupCursor::new(false).filter(QUERY).as_ref(), QUERY);
        for stream in [
            b"prompt\x1b[6n".as_slice(),
            b"\x1b[?6n",
            b"\x1b[31mtext",
            b"\x1b[6",
        ] {
            let mut filter = StartupCursor::new(true);
            let mut output = Vec::new();
            for byte in stream {
                output.extend_from_slice(&filter.filter(&[*byte]));
            }
            output.extend(filter.finish());
            assert_eq!(output, stream);
        }
    }
}
