//! 이 요청에서 쓴 비밀 값을 응답 헤더·본문·오류·기록에서 `[REDACTED]`로 바꾼다.
use aho_corasick::{AhoCorasick, MatchKind};
use std::sync::Arc;

const MASK: &[u8] = crate::REDACTED.as_bytes();

#[derive(Clone, Default)]
pub(crate) struct Redactor {
    ac: Option<Arc<AhoCorasick>>,
    longest: usize,
}

/// 값 하나와 응답에 되돌아올 수 있는 인코딩 꼴(form·percent·JSON 문자열).
fn variants(value: &str) -> Vec<String> {
    let form: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
    let percent = form.replace('+', "%20");
    let json = serde_json::to_string(value)
        .map(|q| q[1..q.len() - 1].to_owned())
        .unwrap_or_default();
    vec![value.to_owned(), form, percent, json]
}

impl Redactor {
    pub(crate) fn new<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut patterns: Vec<String> = values
            .into_iter()
            .filter(|v| !v.as_ref().is_empty())
            .flat_map(|v| variants(v.as_ref()))
            .filter(|v| !v.is_empty())
            .collect();
        patterns.sort();
        patterns.dedup();
        if patterns.is_empty() {
            return Self::default();
        }
        let longest = patterns.iter().map(String::len).max().unwrap_or(0);
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&patterns)
            .ok()
            .map(Arc::new);
        Self { ac, longest }
    }

    pub(crate) fn bytes(&self, data: &[u8]) -> Vec<u8> {
        let mut stream = self.stream();
        let mut out = stream.push(data);
        out.extend(stream.finish());
        out
    }

    pub(crate) fn text(&self, text: &str) -> String {
        String::from_utf8_lossy(&self.bytes(text.as_bytes())).into_owned()
    }

    pub(crate) fn contains(&self, text: &str) -> bool {
        self.ac
            .as_ref()
            .is_some_and(|ac| ac.find(text.as_bytes()).is_some())
    }

    pub(crate) fn stream(&self) -> StreamRedactor {
        StreamRedactor {
            ac: self.ac.clone(),
            hold: self.longest.saturating_sub(1),
            buf: Vec::new(),
        }
    }
}

/// 조각 경계에 걸친 값을 놓치지 않도록 가장 긴 값−1바이트를 다음 조각까지 보류한다.
pub(crate) struct StreamRedactor {
    ac: Option<Arc<AhoCorasick>>,
    hold: usize,
    buf: Vec<u8>,
}

impl StreamRedactor {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let Some(ac) = self.ac.clone() else {
            return chunk.to_vec();
        };
        self.buf.extend_from_slice(chunk);
        let cut = self.buf.len().saturating_sub(self.hold);
        self.drain(&ac, cut)
    }

    pub(crate) fn finish(&mut self) -> Vec<u8> {
        match self.ac.clone() {
            Some(ac) => {
                let end = self.buf.len();
                self.drain(&ac, end)
            }
            None => std::mem::take(&mut self.buf),
        }
    }

    /// `cut` 앞에서 시작한 일치는 모두 버퍼 안에 있다(그보다 긴 값은 hold 안에서 시작).
    fn drain(&mut self, ac: &AhoCorasick, cut: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(cut + MASK.len());
        let mut pos = 0;
        for found in ac.find_iter(&self.buf[..]) {
            if found.start() >= cut {
                break;
            }
            out.extend_from_slice(&self.buf[pos..found.start()]);
            out.extend_from_slice(MASK);
            pos = found.end();
        }
        let keep = pos.max(cut);
        out.extend_from_slice(&self.buf[pos..keep]);
        self.buf.drain(..keep);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Redactor;

    #[test]
    fn redacts_response_body_error_and_url_values() {
        let r = Redactor::new(["top-secret"]);
        assert_eq!(
            r.text(r#"{"echo":"top-secret","access_token":"other"}"#),
            r#"{"echo":"[REDACTED]","access_token":"other"}"#
        );
        assert!(!r.text("failed top-secret").contains("top-secret"));
        assert!(
            !r.text("https://other.test/cb?token=top-secret")
                .contains("top-secret")
        );
        let spaced = Redactor::new(["a b+c"]);
        assert_eq!(
            spaced.text("?q=a+b%2Bc&r=a%20b%2Bc"),
            "?q=[REDACTED]&r=[REDACTED]"
        );
        let quoted = Redactor::new(["a\"b\nc"]);
        assert_eq!(quoted.text(r#"{"v":"a\"b\nc"}"#), r#"{"v":"[REDACTED]"}"#);
        assert_eq!(Redactor::new(Vec::<String>::new()).text("plain"), "plain");
    }

    #[test]
    fn streaming_redaction_spans_chunk_boundaries_and_prefers_longest() {
        let r = Redactor::new(["abc", "abcdefgh", "zz"]);
        let input = b"..abcdefgh..abc..zz..";
        for size in 1..input.len() {
            let mut stream = r.stream();
            let mut out = vec![];
            for chunk in input.chunks(size) {
                out.extend(stream.push(chunk));
            }
            out.extend(stream.finish());
            assert_eq!(
                String::from_utf8(out).expect("utf8"),
                "..[REDACTED]..[REDACTED]..[REDACTED]..",
                "chunk size {size}"
            );
        }
    }
}
