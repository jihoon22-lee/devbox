# P0-06 Webhook Lab 전송 형식 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Webhook Lab 리스너가 흔한 전송 형식을 받게 한다. `Transfer-Encoding: chunked`, `Expect: 100-continue`, UTF-8이 아닌 본문(gzip·protobuf 등 바이너리)을 수신·기록·fixture 저장·재전송할 수 있게 한다(B8·D16).

**Architecture:** HTTP 파서가 요청 머리에서 본문 길이 방식(`BodyFraming::{None, Length, Chunked}`)과 `Expect` 여부를 판정한다. 요청을 받아들인(admit) 뒤 `100 Continue`를 먼저 보내고 본문을 읽는다. 본문 바이트는 UTF-8이면 그대로, 아니면 base64 문자열로 보관하고 `body_encoding`(`utf8`|`base64`) 필드를 붙인다. 필드 기본값이 `utf8`이고 `utf8`일 때는 직렬화하지 않으므로 기존 fixture 파일은 그대로 읽히고 텍스트 fixture는 저장 형식도 바뀌지 않는다. 비밀값 마스킹은 텍스트 본문에만 적용한다.

**Tech Stack:** Rust(std net), `base64` 0.22(lockfile에 이미 있음), React + TypeScript

**Spec:** `review.md` §3 B8 · `00-roadmap.md` D16

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 수신 본문 한도는 기존 `MAX_BODY_BYTES`(1,024,000바이트, **디코딩된 원본 기준**) 그대로다. chunked는 조각 합계로 같은 한도를 적용한다.
- 기록·fixture에 보관하는 본문은 기존과 같이 앞 256,000자(`MAX_BODY_CHARS`)까지다. base64는 4의 배수 경계에서 자르므로 **192,000바이트까지의 바이너리는 온전히**, 그보다 크면 앞 192,000바이트가 보관된다(텍스트가 앞부분만 보관되는 기존 동작과 같은 규칙).
- `Transfer-Encoding`은 정확히 `chunked` 하나만 지원한다(대소문자 무시). 그 밖의 값·중복은 501, `Content-Length`와 함께 오면 400.
- `Expect`는 `100-continue`만 지원한다. 그 밖의 값은 417.
- 바이너리 fixture는 API Studio 요청으로 넘기지 않는다(명확한 오류). Log Lens 전달은 본문 대신 `[binary body N bytes]` 설명을 보낸다.
- 의존성 추가는 `webhook-core`의 `base64 = "0.22"` 하나다. PR 본문에 목적(바이너리 본문 보관)과 "lockfile에 이미 있는 버전"임을 적는다.

## Review Focus

1. `curl --data-binary @file.bin` 바이너리 본문 → 기록에 "바이너리 본문 · N bytes"로 보이고, fixture 저장 후 재전송하면 같은 바이트가 간다. (Task 1·2·3 테스트)
2. chunked 본문에 조각 여러 개·chunk extension(`;ext=1`)·trailer가 섞임 → 조각을 이어 붙인 본문 하나로 기록된다. (Task 1)
3. chunked 합계가 한도를 넘음 → 413이고 기록에 남지 않는다. (Task 1)
4. `Expect: 100-continue` 클라이언트 → 먼저 `100 Continue`를 받고 본문을 보낸 뒤 규칙 응답을 받는다. (Task 1)
5. v0.8.x에서 저장한 fixture 파일(`bodyEncoding` 없음) → 그대로 읽히고 재전송되며, 다시 저장해도 파일에 `bodyEncoding`이 생기지 않는다. (Task 2)

## Branch · PR

- 묶음: **B2** — 브랜치 `fix/suite/files-webhooks-logs-describe`, PR 제목 `fix(suite): cloud files, webhook bodies, operation logs and one describe per session`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `fix(devbox-api-studio): accept chunked, expect-continue and binary webhook bodies`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/webhook-core/Cargo.toml` | 수정 | `base64` |
| `crates/webhook-core/src/core/body.rs` | 생성 | `BodyEncoding`, 인코딩·디코딩·길이 |
| `crates/webhook-core/src/core/mod.rs` | 수정 | `pub mod body;` |
| `crates/webhook-core/src/core/http.rs` | 수정 | framing·Expect·chunked·바이너리 |
| `crates/webhook-core/src/core/history.rs` | 수정 | `RequestRecord.body_encoding`, `push_encoded` |
| `crates/webhook-core/src/core/fixtures.rs` | 수정 | `CapturedFixture.body_encoding`, `sanitize_fixture_body` |
| `crates/webhook-core/src/core/replay.rs` | 수정 | 디코딩한 바이트 전송 |
| `crates/webhook-core/src/core/handoff.rs` | 수정 | 바이너리 API 전달 거부 |
| `crates/webhook-host/src/commands.rs` | 수정 | 417 매핑, `push_encoded`, 전달 본문 |
| `packages/api-studio-features/src/webhooks/lib/body.ts`, `body.test.ts` | 생성 | 표시 문자열 |
| `packages/api-studio-features/src/webhooks/api.ts` | 수정 | 타입 필드 |
| `packages/api-studio-features/src/webhooks/App.tsx:1405,1463` | 수정 | 바이너리 표시 |

---

### Task 1: 본문 인코딩과 HTTP 파서

**Files:** `crates/webhook-core/Cargo.toml`, `src/core/body.rs`(생성), `src/core/mod.rs`, `src/core/http.rs`

**Interfaces (Produces):**
- `webhook_core::core::body::BodyEncoding { Utf8 (default), Base64 }` + `fn is_utf8(&self) -> bool`
- `encode_body(Vec<u8>) -> (String, BodyEncoding)`, `decode_body(&str, BodyEncoding) -> Result<Vec<u8>, ()>`, `decoded_len(&str, BodyEncoding) -> Option<usize>`
- `ParsedRequest { method, target, headers, body: String, body_encoding: BodyEncoding }`
- `ParseError::ExpectationFailed`
- `read_request<S: Read + Write, F: FnOnce() -> bool>(stream: &mut S, running: &AtomicBool, admit: F)` (host의 호출부는 모두 Read+Write인 `ConnectionIo`를 넘기므로 호출부 변경 없음)

- [ ] **Step 1: 의존성과 `body.rs`**

`crates/webhook-core/Cargo.toml` `[dependencies]`에 `base64 = "0.22"`를 추가하고, `src/core/mod.rs`에 `pub mod body;`를 추가한다.

`crates/webhook-core/src/core/body.rs`:

```rust
//! Request bodies are kept as text. Bytes that are not UTF-8 are stored as
//! base64 with an explicit encoding tag so nothing is lost or guessed.
use base64::Engine as _;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyEncoding {
    #[default]
    Utf8,
    Base64,
}

impl BodyEncoding {
    pub fn is_utf8(&self) -> bool {
        *self == Self::Utf8
    }
}

pub fn encode_body(bytes: Vec<u8>) -> (String, BodyEncoding) {
    match String::from_utf8(bytes) {
        Ok(text) => (text, BodyEncoding::Utf8),
        Err(error) => (
            base64::engine::general_purpose::STANDARD.encode(error.as_bytes()),
            BodyEncoding::Base64,
        ),
    }
}

pub fn decode_body(body: &str, encoding: BodyEncoding) -> Result<Vec<u8>, ()> {
    match encoding {
        BodyEncoding::Utf8 => Ok(body.as_bytes().to_vec()),
        BodyEncoding::Base64 => base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| ()),
    }
}

/// Number of bytes the body occupies on the wire.
pub fn decoded_len(body: &str, encoding: BodyEncoding) -> Option<usize> {
    match encoding {
        BodyEncoding::Utf8 => Some(body.len()),
        BodyEncoding::Base64 => decode_body(body, encoding).ok().map(|bytes| bytes.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_stays_text_and_binary_round_trips() {
        assert_eq!(
            encode_body(b"{\"ok\":true}".to_vec()),
            ("{\"ok\":true}".to_string(), BodyEncoding::Utf8)
        );
        let binary = vec![0x1f, 0x8b, 0x08, 0x00, 0xff];
        let (stored, encoding) = encode_body(binary.clone());
        assert_eq!(encoding, BodyEncoding::Base64);
        assert_eq!(decode_body(&stored, encoding).unwrap(), binary);
        assert_eq!(decoded_len(&stored, encoding), Some(5));
        assert!(decode_body("not base64!", BodyEncoding::Base64).is_err());
    }

    #[test]
    fn missing_encoding_field_means_utf8() {
        #[derive(Deserialize)]
        struct Record {
            #[serde(default)]
            body_encoding: BodyEncoding,
        }
        let record: Record = serde_json::from_str("{}").unwrap();
        assert!(record.body_encoding.is_utf8());
        assert_eq!(serde_json::to_string(&BodyEncoding::Base64).unwrap(), "\"base64\"");
    }
}
```

- [ ] **Step 2: 실패하는 파서 테스트** — `http.rs` 테스트 모듈에서
  - `parser_rejects_chunked_and_oversized_headers_without_input_reflection`의 **첫 번째 요청 블록**(Transfer-Encoding: chunked를 보내고 `Unsupported`를 단언하는 부분)을 지우고 이름을 `parser_rejects_oversized_headers_without_input_reflection`으로 바꾼다. 첫 블록에 있던 `let running = AtomicBool::new(true);`는 남은 블록 앞으로 옮긴다.
  - `parser_rejects_non_utf8_body_without_lossy_expansion`을 삭제한다(아래 `binary_bodies_are_kept_as_base64`가 대체).
  - 아래 테스트를 추가한다.

```rust
    #[test]
    fn chunked_bodies_with_extensions_and_trailers_are_joined() {
        let (mut client, mut server) = pair();
        client
            .write_all(
                b"POST /hook HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n4;ext=1\r\nWiki\r\n5\r\npedia\r\n0\r\nX-Trailer: t\r\n\r\n",
            )
            .unwrap();
        let running = AtomicBool::new(true);
        let parsed = read_request(&mut server, &running, || true).unwrap();
        assert_eq!(parsed.body, "Wikipedia");
        assert!(parsed.body_encoding.is_utf8());
    }

    #[test]
    fn chunked_total_over_the_limit_is_rejected() {
        let (mut client, mut server) = pair();
        let size = MAX_BODY_BYTES + 1;
        let head =
            format!("POST /hook HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{size:x}\r\n");
        client.write_all(head.as_bytes()).unwrap();
        let running = AtomicBool::new(true);
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::BodyTooLarge)
        );
    }

    #[test]
    fn conflicting_or_unknown_transfer_encodings_are_rejected() {
        let running = AtomicBool::new(true);
        let (mut client, mut server) = pair();
        client
            .write_all(
                b"POST /hook HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 3\r\n\r\n",
            )
            .unwrap();
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::Malformed)
        );
        let (mut client, mut server) = pair();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n")
            .unwrap();
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::Unsupported)
        );
    }

    #[test]
    fn expect_continue_gets_an_interim_response_before_the_body() {
        let (mut client, mut server) = pair();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 2\r\nExpect: 100-continue\r\n\r\n")
            .unwrap();
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let worker_running = running.clone();
        let worker = thread::spawn(move || read_request(&mut server, &worker_running, || true));
        let mut interim = [0u8; 25];
        client.read_exact(&mut interim).unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        client.write_all(b"ok").unwrap();
        assert_eq!(worker.join().unwrap().unwrap().body, "ok");
    }

    #[test]
    fn unknown_expectations_fail() {
        let (mut client, mut server) = pair();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 2\r\nExpect: something\r\n\r\nok")
            .unwrap();
        let running = AtomicBool::new(true);
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::ExpectationFailed)
        );
    }

    #[test]
    fn binary_bodies_are_kept_as_base64() {
        let (mut client, mut server) = pair();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 3\r\n\r\n\xff\x00\x01")
            .unwrap();
        let running = AtomicBool::new(true);
        let parsed = read_request(&mut server, &running, || true).unwrap();
        assert_eq!(parsed.body_encoding, crate::core::body::BodyEncoding::Base64);
        assert_eq!(
            crate::core::body::decode_body(&parsed.body, parsed.body_encoding).unwrap(),
            vec![0xff, 0x00, 0x01]
        );
    }
```

(테스트 모듈은 `use super::*;`로 `Read`·`Write`를 이미 가져온다.)

- [ ] **Step 3: 실패 확인**

Run: `source ~/.cargo/env && cargo test -p webhook-core --lib core::http`
Expected: 컴파일 실패(`body_encoding`, `ExpectationFailed` 없음).

- [ ] **Step 4: 파서 구현** — `crates/webhook-core/src/core/http.rs`

  1. 파일 머리 주석 7-10행의 "an optional fixed `Content-Length`, and a bounded UTF-8 body.  Unsupported transfer encodings are rejected rather than guessed."를 "a fixed `Content-Length` or `Transfer-Encoding: chunked` body (with `Expect: 100-continue`), bounded by `MAX_BODY_BYTES`.  Bodies that are not UTF-8 are kept as base64.  Other transfer encodings are rejected rather than guessed."로 바꾼다.
  2. `ParseError`에 `ExpectationFailed,`를 추가한다.
  3. `ParsedRequest`에 `pub body_encoding: crate::core::body::BodyEncoding,`를 추가한다.
  4. `type ParsedHead = (…);`를 아래로 바꾼다.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyFraming {
    None,
    Length(usize),
    Chunked,
}

struct ParsedHead {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    framing: BodyFraming,
    expect_continue: bool,
}
```

  5. `parse_head`: `let mut content_length = None;` 아래에 `let mut chunked = false;`와 `let mut expect_continue = false;`를 추가한다. `transfer-encoding`/`expect`를 `Unsupported`로 거부하던 `else if` 분기를 아래로 바꾼다.

```rust
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !value.eq_ignore_ascii_case("chunked") {
                return Err(ParseError::Unsupported);
            }
            chunked = true;
        } else if name.eq_ignore_ascii_case("expect") {
            if !value.eq_ignore_ascii_case("100-continue") {
                return Err(ParseError::ExpectationFailed);
            }
            expect_continue = true;
        }
```

  함수 끝의 `Ok((…))`를 아래로 바꾼다.

```rust
    let framing = match (chunked, content_length) {
        (true, Some(_)) => return Err(ParseError::Malformed),
        (true, None) => BodyFraming::Chunked,
        (false, None) | (false, Some(0)) => BodyFraming::None,
        (false, Some(length)) => BodyFraming::Length(length),
    };
    Ok(ParsedHead {
        method: method.to_ascii_uppercase(),
        target: target.to_string(),
        headers,
        framing,
        expect_continue,
    })
```

  6. `within` 함수 아래에 본문 읽기 도우미를 추가한다.

```rust
const MAX_CHUNK_LINE_BYTES: usize = 128;
const MAX_TRAILER_BYTES: usize = 8 * 1024;

fn read_exact_until<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    deadline: Instant,
    running: &AtomicBool,
) -> Result<(), ParseError> {
    let mut filled = 0;
    while filled < buffer.len() {
        if !running.load(Ordering::Acquire) {
            return Err(ParseError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ParseError::Timeout);
        }
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => return Err(ParseError::Timeout),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ParseError::Timeout),
        }
    }
    Ok(())
}

fn read_chunked<R: Read>(
    reader: &mut R,
    deadline: Instant,
    running: &AtomicBool,
) -> Result<Vec<u8>, ParseError> {
    let mut body = Vec::new();
    loop {
        let line = read_crlf_line(reader, MAX_CHUNK_LINE_BYTES, deadline)
            .map_err(|error| map_line_error(error, ParseError::Malformed))?
            .ok_or(ParseError::Malformed)?;
        let line = std::str::from_utf8(&line).map_err(|_| ParseError::Malformed)?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        if size_text.is_empty() || !size_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ParseError::Malformed);
        }
        let size = usize::from_str_radix(size_text, 16).map_err(|_| ParseError::BodyTooLarge)?;
        if size == 0 {
            let mut trailer_bytes = 0usize;
            loop {
                let trailer = read_crlf_line(reader, MAX_HEADER_LINE_BYTES, deadline)
                    .map_err(|error| map_line_error(error, ParseError::HeaderTooLarge))?
                    .ok_or(ParseError::Malformed)?;
                if trailer.is_empty() {
                    return Ok(body);
                }
                trailer_bytes += trailer.len();
                if trailer_bytes > MAX_TRAILER_BYTES {
                    return Err(ParseError::HeaderTooLarge);
                }
            }
        }
        if body
            .len()
            .checked_add(size)
            .is_none_or(|total| total > MAX_BODY_BYTES)
        {
            return Err(ParseError::BodyTooLarge);
        }
        let start = body.len();
        body.resize(start + size, 0);
        read_exact_until(reader, &mut body[start..], deadline, running)?;
        let mut crlf = [0u8; 2];
        read_exact_until(reader, &mut crlf, deadline, running)?;
        if &crlf != b"\r\n" {
            return Err(ParseError::Malformed);
        }
    }
}
```

  7. `read_request` 전체를 아래로 바꾼다(고정 길이 읽기 루프와 UTF-8 거부 주석은 삭제된다).

```rust
/// Read one bounded request from a single-use connection. The admission
/// callback runs after the complete header but before body allocation/read so
/// callers can reject rate-limited requests without consuming the body. A
/// `100 Continue` interim response is written only after admission.
pub fn read_request<S: Read + Write, F>(
    stream: &mut S,
    running: &AtomicBool,
    admit: F,
) -> Result<ParsedRequest, ParseError>
where
    F: FnOnce() -> bool,
{
    if !running.load(Ordering::Acquire) {
        return Err(ParseError::Cancelled);
    }
    let deadline = Instant::now() + Duration::from_millis(REQUEST_IO_TIMEOUT_MS);
    let mut reader = BufReader::with_capacity(8 * 1024, stream);
    let request_line = read_crlf_line(&mut reader, MAX_REQUEST_LINE_BYTES, deadline)
        .map_err(|error| map_line_error(error, ParseError::RequestLineTooLarge))?
        .ok_or(ParseError::Closed)?;
    let head = parse_head(&request_line, &mut reader, running, deadline)?;
    if !running.load(Ordering::Acquire) {
        return Err(ParseError::Cancelled);
    }
    if !admit() {
        return Err(ParseError::RateLimited);
    }
    if head.expect_continue && head.framing != BodyFraming::None {
        let inner = reader.get_mut();
        inner
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .and_then(|()| inner.flush())
            .map_err(|_| ParseError::Io)?;
    }
    let bytes = match head.framing {
        BodyFraming::None => Vec::new(),
        BodyFraming::Length(length) => {
            let mut body = vec![0u8; length];
            read_exact_until(&mut reader, &mut body, deadline, running)?;
            body
        }
        BodyFraming::Chunked => read_chunked(&mut reader, deadline, running)?,
    };
    // base64 grows an already bounded body by 4/3; the lossy UTF-8
    // conversion this replaces could grow it three-fold.
    let (body, body_encoding) = crate::core::body::encode_body(bytes);
    Ok(ParsedRequest {
        method: head.method,
        target: head.target,
        headers: head.headers,
        body,
        body_encoding,
    })
}
```

- [ ] **Step 5: 통과 확인**

Run: `cargo test -p webhook-core --lib core::http core::body`
Expected: PASS.

- [ ] **Step 6: 커밋**

```bash
git add crates/webhook-core/Cargo.toml Cargo.lock crates/webhook-core/src/core/body.rs crates/webhook-core/src/core/mod.rs crates/webhook-core/src/core/http.rs
git commit -m "fix(devbox-api-studio): parse chunked, expect-continue and binary webhook bodies"
```

---

### Task 2: 기록·fixture·재전송·전달

**Files:** `crates/webhook-core/src/core/{history,fixtures,replay,handoff}.rs`, `crates/webhook-host/src/commands.rs`

**Interfaces:**
- Consumes: Task 1의 `BodyEncoding`, `decode_body`, `decoded_len`, `ParsedRequest.body_encoding`, `ParseError::ExpectationFailed`
- Produces: `History::push_encoded(method: String, url: String, headers: Vec<(String, String)>, body: String, body_encoding: BodyEncoding, received_at_ms: i64)`, `RequestRecord.body_encoding`, `CapturedFixture.body_encoding`, `fixtures::sanitize_fixture_body(&str, BodyEncoding) -> Result<String, FixtureError>`, `handoff::HANDOFF_BINARY_BODY_ERROR`, `ReplayRequest.body: Vec<u8>`

- [ ] **Step 1: 실패하는 테스트**

`history.rs` 테스트 모듈:

```rust
    #[test]
    fn binary_history_bodies_skip_text_masking_and_keep_whole_base64_groups() {
        use crate::core::body::{decode_body, BodyEncoding};
        let mut h = History::default();
        h.push_encoded("POST".into(), "/hook".into(), vec![], "/wAB".into(), BodyEncoding::Base64, 1);
        let record = &h.list_masked()[0];
        assert_eq!(record.body, "/wAB");
        assert_eq!(record.body_encoding, BodyEncoding::Base64);

        let long = "AAAA".repeat(MAX_BODY_CHARS / 4 + 10);
        h.push_encoded("POST".into(), "/hook".into(), vec![], long, BodyEncoding::Base64, 2);
        let record = &h.list_masked()[0];
        assert_eq!(record.body.len(), MAX_BODY_CHARS);
        assert!(decode_body(&record.body, record.body_encoding).is_ok());
    }
```

`fixtures.rs` 테스트 모듈(`request(url, body)`는 1357행의 기존 도우미):

```rust
    #[test]
    fn v08_fixtures_without_body_encoding_load_and_serialize_unchanged() {
        let json = r#"{"id":"fixture-1","method":"POST","url":"/hook","headers":[],"body":"{}","receivedAtMs":1}"#;
        let fixture: CapturedFixture = serde_json::from_str(json).unwrap();
        assert!(fixture.body_encoding.is_utf8());
        assert!(!serde_json::to_string(&fixture).unwrap().contains("bodyEncoding"));
    }

    #[test]
    fn binary_fixtures_keep_their_bytes_and_reject_invalid_base64() {
        use crate::core::body::BodyEncoding;
        assert_eq!(sanitize_fixture_body("/wAB", BodyEncoding::Base64).unwrap(), "/wAB");
        assert_eq!(
            sanitize_fixture_body("not base64!", BodyEncoding::Base64),
            Err(FixtureError::Invalid)
        );
        let mut record = request("/hook", "/wAB");
        record.body_encoding = BodyEncoding::Base64;
        let fixture = fixture_from_request("fixture-1".into(), &record).unwrap();
        assert_eq!(fixture.body, "/wAB");
        assert_eq!(fixture.body_encoding, BodyEncoding::Base64);
    }
```

`replay.rs` 테스트 모듈(`fixture()`는 381행의 기존 도우미):

```rust
    #[test]
    fn base64_fixtures_replay_their_original_bytes() {
        let mut binary = fixture();
        binary.body = "/wAB".into();
        binary.body_encoding = crate::core::body::BodyEncoding::Base64;
        let (_, request) = build_request(&binary, "127.0.0.1:9000").unwrap();
        let wire = request.wire_bytes();
        assert!(wire.ends_with(&[0xff, 0x00, 0x01]));
        assert!(String::from_utf8_lossy(&wire).contains("Content-Length: 3\r\n"));
    }
```

`handoff.rs` 테스트 모듈(`fixture()`는 208행의 기존 도우미):

```rust
    #[test]
    fn binary_fixtures_are_not_sent_as_api_requests() {
        let mut binary = fixture();
        binary.body = "/wAB".into();
        binary.body_encoding = crate::core::body::BodyEncoding::Base64;
        assert_eq!(build_api_request_payload(&binary), Err(HANDOFF_BINARY_BODY_ERROR));
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p webhook-core --lib`
Expected: 컴파일 실패(`push_encoded`, `body_encoding`, `sanitize_fixture_body`, `HANDOFF_BINARY_BODY_ERROR` 없음).

- [ ] **Step 3: 구현**

`history.rs`
- 파일 상단 `use` 목록에 `use crate::core::body::BodyEncoding;`를 추가하고, 상수 선언 아래에 추가한다.

```rust
const _: () = assert!(MAX_BODY_CHARS % 4 == 0, "base64 history prefixes must stay decodable");
```

- `RequestRecord`의 `body` 아래에 추가한다.

```rust
    #[serde(default, skip_serializing_if = "BodyEncoding::is_utf8")]
    pub body_encoding: BodyEncoding,
```

- 기존 `push`의 이름을 `push_encoded`로 바꾸고 `body: String,` 다음 인자로 `body_encoding: BodyEncoding,`를 추가한다. 그 위에 기존 이름의 래퍼를 둔다(기존 테스트의 `push` 호출은 그대로 둔다).

```rust
    /// 요청을 기록하고 마스킹을 적용한다. 상한 초과 시 가장 오래된 것을 버린다.
    pub fn push(
        &mut self,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: String,
        received_at_ms: i64,
    ) {
        self.push_encoded(method, url, headers, body, BodyEncoding::Utf8, received_at_ms);
    }
```

- `push_encoded` 안의 `let body = crate::core::fixtures::sanitize_body_for_history(&body);`를 아래로 바꾸고, `RequestRecord { … }` 생성부의 `body,` 다음에 `body_encoding,`을 추가한다.

```rust
        let body = match body_encoding {
            BodyEncoding::Utf8 => crate::core::fixtures::sanitize_body_for_history(&body),
            // Base64 is opaque ASCII with no readable secret to mask. Cutting
            // at the display cap (a multiple of 4) keeps a decodable prefix.
            BodyEncoding::Base64 => {
                let mut kept = body;
                kept.truncate(MAX_BODY_CHARS);
                kept
            }
        };
```

`fixtures.rs`
- `CapturedFixture`의 `body` 아래에 추가한다(`deny_unknown_fields`는 그대로 둔다).

```rust
    #[serde(default, skip_serializing_if = "crate::core::body::BodyEncoding::is_utf8")]
    pub body_encoding: crate::core::body::BodyEncoding,
```

- `sanitize_body` 아래에 추가한다.

```rust
/// Text bodies go through the credential sanitizer. Base64 bodies are opaque
/// bytes: they are bounded like text and must decode, but are kept verbatim.
pub fn sanitize_fixture_body(
    body: &str,
    encoding: crate::core::body::BodyEncoding,
) -> Result<String, FixtureError> {
    if encoding.is_utf8() {
        return sanitize_body(body);
    }
    if !within(body, MAX_FIXTURE_BODY_CHARS, MAX_FIXTURE_BODY_BYTES) {
        return Err(FixtureError::Size);
    }
    crate::core::body::decode_body(body, encoding).map_err(|_| FixtureError::Invalid)?;
    Ok(body.to_string())
}
```

- `fixture_from_request`: `body: sanitize_body(&request.body)?,`를 `body: sanitize_fixture_body(&request.body, request.body_encoding)?,`로 바꾸고 다음 줄에 `body_encoding: request.body_encoding,`을 추가한다.
- `validate_fixture`: `if sanitize_body(&fixture.body)? != fixture.body {`를 `if sanitize_fixture_body(&fixture.body, fixture.body_encoding)? != fixture.body {`로 바꾼다.
- 테스트 도우미 리터럴 세 곳에 `body_encoding: Default::default(),`를 추가한다: `fn request`(1357행), `fn fixture`(1371행), 1386행 `&RequestRecord {`.

`replay.rs`
- `ReplayRequest.body`를 `pub body: Vec<u8>,`로 바꾸고 `wire_bytes`의 `bytes.extend_from_slice(self.body.as_bytes());`를 `bytes.extend_from_slice(&self.body);`로 바꾼다.
- `build_request`에서 `if fixture.body.len() > MAX_REPLAY_BODY_BYTES { … }` 블록을 아래로 바꾼다.

```rust
    let body = crate::core::body::decode_body(&fixture.body, fixture.body_encoding)
        .map_err(|_| ReplayError::InvalidFixture)?;
    if body.len() > MAX_REPLAY_BODY_BYTES {
        return Err(ReplayError::TooLarge);
    }
```

  `Content-Length` 줄의 `fixture.body.len().to_string()`을 `body.len().to_string()`으로, 마지막 `body: fixture.body.clone(),`을 `body,`로 바꾼다.
- 테스트 도우미 `fn fixture()`(381행) 리터럴에 `body_encoding: Default::default(),`를 추가한다.

`handoff.rs`
- `HANDOFF_INPUT_ERROR`(17행) 아래에 추가한다.

```rust
pub const HANDOFF_BINARY_BODY_ERROR: &str =
    "바이너리 본문 fixture는 API 요청으로 보낼 수 없습니다";
```

- `build_api_request_payload`의 `REDACTED_PATH` 검사 다음에 추가한다.

```rust
    if !fixture.body_encoding.is_utf8() {
        return Err(HANDOFF_BINARY_BODY_ERROR);
    }
```

- 테스트 도우미 `fn fixture()`(208행) 리터럴에 `body_encoding: Default::default(),`를 추가한다.

`crates/webhook-host/src/commands.rs`
- `parse_error_response`(325행 부근)에 `ParseError::ExpectationFailed => Some((417, "지원하지 않는 Expect 요청입니다")),`를 추가한다(`reason_phrase`에 417이 이미 있다).
- `handle_request`의 `history.push(`를 `history.push_encoded(`로 바꾸고 `request.body.clone(),` 다음 줄에 `request.body_encoding,`을 넣는다.
- `prepare_api_handoff`와 `publish_api_handoff`의 `build_api_request_payload(&fixture).map_err(|_| HANDOFF_INPUT_ERROR.to_string())?`를 `build_api_request_payload(&fixture).map_err(str::to_string)?`로 바꾼다(그 밖의 실패는 이미 `HANDOFF_INPUT_ERROR`를 돌려준다).
- `prepare_log_handoff`와 `publish_log_lens_handoff`의 `&fixture.body,`를 `&log_body(&fixture),`로 바꾸고 도우미를 추가한다.

```rust
fn log_body(fixture: &CapturedFixture) -> String {
    if fixture.body_encoding.is_utf8() {
        return fixture.body.clone();
    }
    let bytes = crate::core::body::decoded_len(&fixture.body, fixture.body_encoding).unwrap_or(0);
    format!("[binary body {bytes} bytes]")
}
```

  (host는 `webhook_core::core`를 `crate::core`로 재수출해 쓴다. 파일 상단 `use crate::core::…` 관례와 같다.)
- `rg -n "RequestRecord \{|CapturedFixture \{" crates/webhook-host`로 남은 리터럴을 찾아 `body_encoding: Default::default(),`를 추가한다.

- [ ] **Step 4: 통과 확인**

Run: `cargo test -p webhook-core --lib && cargo test -p devbox-webhook-host --lib && cargo clippy -p webhook-core -p devbox-webhook-host --all-targets -- -D warnings`
Expected: PASS, 경고 없음.

- [ ] **Step 5: 커밋**

```bash
git add crates/webhook-core crates/webhook-host
git commit -m "fix(devbox-api-studio): keep binary webhook bodies through history, fixtures and replay"
```

---

### Task 3: UI 표시

**Files:** `packages/api-studio-features/src/webhooks/lib/body.ts`(생성), `lib/body.test.ts`(생성), `api.ts`, `App.tsx`

**Interfaces:** Consumes: `RequestRecord.bodyEncoding`, `CapturedFixture.bodyEncoding`(JSON에서 `utf8`이면 생략됨)

- [ ] **Step 1: 실패하는 테스트** — `lib/body.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { bodyPreview } from "./body";

describe("bodyPreview", () => {
  it("shows the first 200 characters of text bodies", () => {
    expect(bodyPreview("x".repeat(300), undefined)).toBe("x".repeat(200));
    expect(bodyPreview("{}", "utf8")).toBe("{}");
  });

  it("describes binary bodies by decoded size", () => {
    expect(bodyPreview("/wAB", "base64")).toBe("바이너리 본문 · 3 bytes");
    expect(bodyPreview("/w==", "base64")).toBe("바이너리 본문 · 1 bytes");
  });
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/webhooks/lib/body.test.ts` → FAIL(모듈 없음).

- [ ] **Step 3: 구현** — `lib/body.ts`

```ts
export type BodyEncoding = "utf8" | "base64";

function decodedLength(base64: string): number {
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return (base64.length / 4) * 3 - padding;
}

export function bodyPreview(body: string, encoding: BodyEncoding | undefined): string {
  if (encoding === "base64") return `바이너리 본문 · ${decodedLength(body)} bytes`;
  return body.slice(0, 200);
}
```

- `api.ts`: 파일 상단에 `import type { BodyEncoding } from "./lib/body";`를 추가하고 `RequestRecord`(10-17행)와 `CapturedFixture`(78-85행)에 `bodyEncoding?: BodyEncoding;`를 추가한다.
- `App.tsx`: `import { bodyPreview } from "./lib/body";`를 추가하고, 1405행 `{request.body && <pre className="body">{request.body.slice(0, 200)}</pre>}`를 `{request.body && <pre className="body">{bodyPreview(request.body, request.bodyEncoding)}</pre>}`로, 1463행 fixture 표시도 같은 형태(`bodyPreview(fixture.body, fixture.bodyEncoding)`)로 바꾼다.

- [ ] **Step 4: 통과 확인** — Run: `pnpm --filter @devbox/api-studio-features exec vitest run src/webhooks && pnpm --filter @devbox/api-studio-features exec tsc --noEmit` → PASS.

- [ ] **Step 5: 커밋**

```bash
git add packages/api-studio-features/src/webhooks
git commit -m "fix(devbox-api-studio): label binary webhook bodies in history and fixtures"
```

---

### Task 4: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9를 수행한다.
- [ ] PR 본문 "Windows 실기 확인" 절에 아래를 적고 사용자 확인 대기로 둔다.
  1. API Studio › 웹훅에서 리스너를 켠다(예: 포트 8787).
  2. PowerShell: `curl.exe -X POST http://127.0.0.1:8787/hook -H "Transfer-Encoding: chunked" --data-binary "@README.md"` → 기록에 본문이 보인다.
  3. `curl.exe -X POST http://127.0.0.1:8787/hook -H "Expect: 100-continue" --data-binary "@README.md"` → 정상 응답.
  4. 190KB 미만 zip 파일을 `--data-binary`로 보내면 "바이너리 본문 · N bytes"로 보이고, fixture 저장 → 재전송이 성공한다.
