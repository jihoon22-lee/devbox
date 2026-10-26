//! 이 요청에서 쓴 비밀 값을 응답 헤더·본문·오류·기록에서 `[REDACTED]`로 바꾼다.
use aho_corasick::{AhoCorasick, MatchKind};
use std::sync::Arc;

const MASK: &[u8] = crate::REDACTED.as_bytes();

#[derive(Clone, Default)]
pub(crate) struct Redactor {
    ac: Option<Arc<AhoCorasick>>,
    longest: usize,
    blocked: bool,
}

mod credentials;
mod seeds;
use crate::ApiError;
use seeds::Seeds;
use zeroize::{Zeroize, Zeroizing};

/// All sessions own bounded seeds; construction/extension failure never falls back to raw output.
pub(crate) struct Redaction {
    seeds: Seeds,
    /// Every seed: display, stream and history redaction.
    pub(crate) redactor: Redactor,
    /// Credential/secret seeds only (no cookie values): the cross-origin redirect Location
    /// guard, so a cookie value echoed in a Location (OAuth `state`) does not block the hop.
    /// The cross-origin header drop keeps using `redactor` (all seeds).
    pub(crate) guard: Redactor,
    failed: bool,
}
impl Redaction {
    pub(crate) fn new<I, S>(values: I) -> Result<Self, ApiError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut seeds = Seeds::default();
        for value in values {
            seeds.add(value.as_ref().as_bytes())?;
        }
        let (redactor, guard) = Redactor::compile_both(&seeds)?;
        Ok(Self {
            seeds,
            redactor,
            guard,
            failed: false,
        })
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }
    fn update(
        &mut self,
        collect: impl FnOnce(&mut Seeds) -> Result<bool, ApiError>,
    ) -> Result<(), ApiError> {
        if self.failed {
            return Err(seeds::failed("capacity"));
        }
        let result = collect(&mut self.seeds).and_then(|changed| {
            if changed {
                Redactor::compile_both(&self.seeds).map(Some)
            } else {
                Ok(None)
            }
        });
        match result {
            Ok(Some((redactor, guard))) => {
                self.redactor = redactor;
                self.guard = guard;
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => {
                self.failed = true;
                let blocked = Redactor {
                    blocked: true,
                    ..Default::default()
                };
                self.redactor = blocked.clone();
                self.guard = blocked;
                Err(error)
            }
        }
    }
    pub(crate) fn add(&mut self, value: &str) -> Result<(), ApiError> {
        self.update(|seeds| seeds.add(value.as_bytes()))
    }
    pub(crate) fn request_headers<'a>(
        &mut self,
        headers: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Result<(), ApiError> {
        self.update(|seeds| {
            let mut changed = false;
            for (name, value) in headers {
                changed |= credentials::request(seeds, name, value)?;
            }
            Ok(changed)
        })
    }
    pub(crate) fn response_headers<'a>(
        &mut self,
        headers: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Result<(), ApiError> {
        self.update(|seeds| {
            let mut changed = false;
            for (name, value) in headers {
                changed |= credentials::response(seeds, name, value)?;
            }
            Ok(changed)
        })
    }
}
impl Redactor {
    #[cfg(test)]
    pub(crate) fn new<I, S>(values: I) -> Result<Self, ApiError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Ok(Redaction::new(values)?.redactor)
    }
    fn compile_both(seeds: &Seeds) -> Result<(Self, Self), ApiError> {
        Ok((Self::compile(seeds, false)?, Self::compile(seeds, true)?))
    }
    fn compile(seeds: &Seeds, guard_only: bool) -> Result<Self, ApiError> {
        let patterns = seeds.patterns(guard_only)?;
        if patterns.is_empty() {
            return Ok(Self::default());
        }
        let longest = patterns.iter().map(Vec::len).max().unwrap_or(0);
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(patterns.iter())
            .map_err(|_| seeds::failed("matcher"))?;
        Ok(Self {
            ac: Some(Arc::new(ac)),
            longest,
            blocked: false,
        })
    }
    pub(crate) fn header(&self, name: &str, value: &[u8]) -> String {
        if credentials::sensitive_header(name) {
            crate::REDACTED.into()
        } else {
            String::from_utf8_lossy(&self.bytes(value)).into_owned()
        }
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
        self.blocked
            || self
                .ac
                .as_ref()
                .is_some_and(|ac| ac.find(text.as_bytes()).is_some())
    }

    pub(crate) fn stream(&self) -> StreamRedactor {
        StreamRedactor {
            ac: self.ac.clone(),
            hold: self.longest.saturating_sub(1),
            buf: Zeroizing::new(Vec::new()),
            blocked: self.blocked,
        }
    }
}

/// 조각 경계에 걸친 값을 놓치지 않도록 가장 긴 값−1바이트를 다음 조각까지 보류한다.
pub(crate) struct StreamRedactor {
    ac: Option<Arc<AhoCorasick>>,
    hold: usize,
    buf: Zeroizing<Vec<u8>>,
    blocked: bool,
}

impl StreamRedactor {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        if self.blocked {
            return if chunk.is_empty() {
                vec![]
            } else {
                MASK.to_vec()
            };
        }
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
            None => std::mem::take(&mut *self.buf),
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
        self.buf[..keep].zeroize();
        self.buf.drain(..keep);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Redactor;

    #[test]
    fn redacts_response_body_error_and_url_values() {
        let r = Redactor::new(["top-secret"]).expect("bounded");
        assert_eq!(
            r.text(r#"{"echo":"top-secret","access_token":"other"}"#),
            r#"{"echo":"[REDACTED]","access_token":"other"}"#
        );
        assert!(!r.text("failed top-secret").contains("top-secret"));
        assert!(
            !r.text("https://other.test/cb?token=top-secret")
                .contains("top-secret")
        );
        let spaced = Redactor::new(["a b+c"]).expect("bounded");
        assert_eq!(
            spaced.text("?q=a+b%2Bc&r=a%20b%2Bc"),
            "?q=[REDACTED]&r=[REDACTED]"
        );
        let quoted = Redactor::new(["a\"b\nc"]).expect("bounded");
        assert_eq!(quoted.text(r#"{"v":"a\"b\nc"}"#), r#"{"v":"[REDACTED]"}"#);
        assert_eq!(
            Redactor::new(Vec::<String>::new())
                .expect("empty")
                .text("plain"),
            "plain"
        );
    }

    #[test]
    fn streaming_redaction_spans_chunk_boundaries_and_prefers_longest() {
        let r = Redactor::new(["abc", "abcdefgh", "zz"]).expect("bounded");
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

#[cfg(test)]
mod credential_tests {
    use super::{Redaction, seeds::MAX_SEED_BYTES};
    fn fresh() -> Redaction {
        Redaction::new(std::iter::empty::<&str>()).expect("empty policy")
    }
    #[test]
    fn recognized_headers_basic_and_cookie_parts_share_one_policy() {
        let mut r = fresh();
        let basic = format!(
            "Basic {}",
            data_encoding::BASE64.encode(b"fixture-user:fixture-password")
        );
        for (name, value, parts) in [
            (
                "AuThOrIzAtIoN",
                "Bearer fixture-bearer",
                vec!["fixture-bearer"],
            ),
            (
                "Proxy-Authorization",
                basic.as_str(),
                vec!["fixture-user:fixture-password", "fixture-password"],
            ),
            (
                "Authorization",
                "Custom fixture-unknown-scheme",
                vec!["fixture-unknown-scheme"],
            ),
            ("X-API-Key", "fixture-api", vec!["fixture-api"]),
            ("API-Key", "fixture-second-api", vec!["fixture-second-api"]),
            ("X-Auth-Token", "fixture-token", vec!["fixture-token"]),
            (
                "Cookie",
                "first=fixture-first; second=\"fixture-second\"",
                vec!["fixture-first", "fixture-second"],
            ),
        ] {
            r.request_headers([(name, value.as_bytes())])
                .expect("recognized");
            for part in parts {
                assert_eq!(r.redactor.text(part), crate::REDACTED);
            }
            assert_eq!(r.redactor.header(name, value.as_bytes()), crate::REDACTED);
        }
        assert_eq!(
            r.redactor.text("fixture-user"),
            "fixture-user",
            "Basic username alone is not inferred"
        );
    }
    #[test]
    fn empty_short_binary_and_noncredential_values_keep_precise_scope() {
        let mut r = fresh();
        r.request_headers([
            ("Cookie", b"empty=; quoted=\"\"".as_slice()),
            ("x-public", b"public-readable".as_slice()),
        ])
        .expect("empty");
        r.response_headers([
            ("Set-Cookie", b"empty=\"\"; Path=/".as_slice()),
            ("x-api-key", b"response-only".as_slice()),
        ])
        .expect("response");
        assert_eq!(
            r.redactor
                .text("empty=; quoted=\"\" public-readable response-only Path /"),
            "empty=; quoted=\"\" public-readable response-only Path /"
        );
        assert_eq!(r.redactor.header("Cookie", b""), crate::REDACTED);
        r.response_headers([
            ("Set-Cookie", b"sid=Q; Path=/; SameSite=Lax".as_slice()),
            ("SET-cookie", b"bin=\xff\xfe; Secure".as_slice()),
        ])
        .expect("short binary");
        assert_eq!(
            r.redactor.text("Q Path SameSite Lax sid"),
            "[REDACTED] Path SameSite Lax sid"
        );
        assert_eq!(r.redactor.bytes(b"..\xff\xfe.."), b"..[REDACTED]..");
        assert_eq!(
            r.redactor.header("x-echo", b"..\xff\xfe.."),
            "..[REDACTED].."
        );
    }
    #[test]
    fn construction_and_extension_errors_poison_instead_of_raw_fallback() {
        assert!(Redaction::new(["a".repeat(MAX_SEED_BYTES + 1)]).is_err());
        let mut r = fresh();
        assert!(r.add(&"a".repeat(MAX_SEED_BYTES + 1)).is_err());
        assert!(r.failed());
        assert_eq!(
            r.redactor.text("raw-output-must-not-escape"),
            crate::REDACTED
        );
        assert!(r.add("small").is_err(), "failure is sticky");
        assert!(r.guard.contains("anything"), "guard fails closed too");
    }
    #[test]
    fn cookie_values_redact_output_but_only_credentials_guard_hops() {
        let mut r = Redaction::new(["fixture-secret"]).expect("policy");
        r.request_headers([
            ("Cookie", b"sid=fixture-cookie; bare".as_slice()),
            ("X-API-Key", b"fixture-api".as_slice()),
        ])
        .expect("request");
        r.response_headers([("Set-Cookie", b"state=fixture-state; Path=/".as_slice())])
            .expect("response");
        for value in ["fixture-cookie", "bare", "fixture-state"] {
            assert_eq!(r.redactor.text(value), crate::REDACTED);
            assert!(!r.guard.contains(value), "{value} never guards");
        }
        for value in ["fixture-secret", "fixture-api"] {
            assert!(r.guard.contains(&format!("?q={value}")));
        }
        // The same bytes later seen as a credential start guarding.
        r.request_headers([("Authorization", b"Bearer fixture-state".as_slice())])
            .expect("upgrade");
        assert!(r.guard.contains("?state=fixture-state"));
    }
}
