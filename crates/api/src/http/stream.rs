//! 응답 머리·본문을 가려서 스트림으로 보내고 기록용 사본(상한 16MiB)을 모은다.
use crate::events::{Closed, Events};
use crate::redact::Redactor;
use crate::{ApiError, Pair, Redirect, SendEvent};

pub(crate) const HISTORY_BODY_MAX: usize = 16 * 1024 * 1024;

fn is_textual_media_type(media_type: &str) -> bool {
    if media_type.starts_with("text/") {
        return true;
    }
    media_type.split_once('/').is_some_and(|(_, subtype)| {
        matches!(
            subtype,
            "json"
                | "xml"
                | "javascript"
                | "x-javascript"
                | "yaml"
                | "x-yaml"
                | "graphql"
                | "csv"
                | "x-www-form-urlencoded"
        ) || subtype.ends_with("+json")
            || subtype.ends_with("+xml")
    })
}

/// 미디어 유형과 내용(UTF-8·NUL·제어 문자 비율)으로 이진 응답을 판정한다(조각 단위).
pub(crate) struct Classifier {
    media: Option<bool>,
    pending: Vec<u8>,
    utf8: bool,
    nul: bool,
    controls: usize,
    total: usize,
}

impl Classifier {
    pub(crate) fn new(media_type: &str) -> Self {
        let media_type = media_type.trim().to_ascii_lowercase();
        Self {
            media: (!media_type.is_empty()).then(|| is_textual_media_type(&media_type)),
            pending: vec![],
            utf8: true,
            nul: false,
            controls: 0,
            total: 0,
        }
    }
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len();
        self.nul |= chunk.contains(&0);
        self.controls += chunk
            .iter()
            .filter(|b| **b < 9 || (**b > 13 && **b < 32))
            .count();
        if !self.utf8 {
            return;
        }
        self.pending.extend_from_slice(chunk);
        match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.clear(),
            Err(e) if e.error_len().is_none() => {
                let valid = e.valid_up_to();
                self.pending.drain(..valid);
            }
            Err(_) => self.utf8 = false,
        }
    }
    pub(crate) fn binary(&self) -> bool {
        if self.media == Some(false) {
            return true;
        }
        !self.utf8 || !self.pending.is_empty() || self.nul || self.controls > self.total.max(2) / 20
    }
}

#[cfg(test)]
pub(crate) fn is_binary_response(media_type: &str, bytes: &[u8]) -> bool {
    let mut c = Classifier::new(media_type);
    c.push(bytes);
    c.binary()
}

#[derive(Default)]
pub(crate) struct Captured {
    pub(crate) status: u16,
    pub(crate) reason: String,
    pub(crate) http_version: String,
    pub(crate) headers: Vec<Pair>,
    pub(crate) redirects: Vec<Redirect>,
    pub(crate) head_ms: i64,
    pub(crate) elapsed_ms: i64,
    pub(crate) size_bytes: i64,
    pub(crate) binary: bool,
    pub(crate) body: Vec<u8>,
    pub(crate) truncated: bool,
}

pub(crate) enum Stop {
    Closed,
    Fail(ApiError),
}
impl From<Closed> for Stop {
    fn from(_: Closed) -> Self {
        Self::Closed
    }
}

fn version(v: reqwest::Version) -> &'static str {
    match v {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_2 => "HTTP/2",
        reqwest::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
}

pub(crate) fn headers(map: &reqwest::header::HeaderMap, redactor: &Redactor) -> Vec<Pair> {
    map.iter()
        .map(|(n, v)| Pair {
            name: n.as_str().to_owned(),
            value: redactor.header(n.as_str(), v.as_bytes()),
        })
        .collect()
}

/// 머리를 보내고 본문을 조각마다 가려서 보낸다. 모든 단계가 요청 시한 하나를 공유한다.
pub(crate) async fn relay(
    mut response: reqwest::Response,
    deadline: tokio::time::Instant,
    started: std::time::Instant,
    redactor: &Redactor,
    events: &Events<'_>,
    out: &mut Captured,
) -> Result<(), Stop> {
    let status = response.status();
    out.status = status.as_u16();
    out.reason = status.canonical_reason().unwrap_or("").to_owned();
    out.http_version = version(response.version()).to_owned();
    out.headers = headers(response.headers(), redactor);
    out.head_ms = started.elapsed().as_millis() as i64;
    // A later redirect response may introduce a credential already present in
    // an earlier Location. Refresh the complete published redirect list.
    for redirect in &mut out.redirects {
        redirect.location = redactor.text(&redirect.location);
    }
    let media = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").to_owned())
        .unwrap_or_default();
    events
        .send(&SendEvent::Head {
            status: out.status,
            reason: out.reason.clone(),
            http_version: out.http_version.clone(),
            headers: out.headers.clone(),
            elapsed_ms: out.head_ms,
            redirects: out.redirects.clone(),
        })
        .await?;
    let mut classifier = Classifier::new(&media);
    let mut masker = redactor.stream();
    loop {
        let chunk = tokio::time::timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| Stop::Fail(ApiError::network("timeout")))?
            .map_err(|e| Stop::Fail(super::kind(&e)))?;
        let Some(chunk) = chunk else { break };
        out.size_bytes += chunk.len() as i64;
        classifier.push(&chunk);
        // 가림기가 값 경계만큼만 보류하므로 받은 만큼 바로 보낸다(이벤트 하나는 최대 48KiB).
        let masked = masker.push(&chunk);
        keep(out, &masked);
        events.body(&masked).await?;
    }
    let tail = masker.finish();
    keep(out, &tail);
    events.body(&tail).await?;
    out.binary = classifier.binary();
    out.elapsed_ms = started.elapsed().as_millis() as i64;
    Ok(())
}

/// 기록 사본: 이미 가린 바이트만 담고 16MiB를 넘으면 잘림 표시만 남긴다.
fn keep(out: &mut Captured, masked: &[u8]) {
    let room = HISTORY_BODY_MAX.saturating_sub(out.body.len());
    if masked.len() > room {
        out.body.extend_from_slice(&masked[..room]);
        out.truncated = true;
    } else {
        out.body.extend_from_slice(masked);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn binary_response_classification_uses_media_type_and_utf8_safely() {
        use super::is_binary_response as b;
        assert!(b("application/octet-stream", b"plain"));
        assert!(!b("text/plain", b"plain"));
        assert!(!b("application/problem+json", b"{}"));
        assert!(b("application/notjsonpayload", b"plain"));
        assert!(b("text/plain", &[0xff, 0xfe]));
        assert!(b("text/plain", &[0, 1, 2]));
        assert!(b("", &[0, 1, 2]));
        let mut split = super::Classifier::new("text/plain");
        let text = "한글".as_bytes();
        split.push(&text[..1]);
        split.push(&text[1..]);
        assert!(!split.binary(), "UTF-8 split across chunks stays text");
    }
}
