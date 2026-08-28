//! Small bounded HTTP/1.x transport for the local Webhook Lab listener.
//!
//! A general-purpose third-party parser would parse a request on an internal
//! connection thread before the application receives it.  That makes
//! application-level limits too late for request-line/header allocation and
//! leaves a partial body without an idle deadline.  This module deliberately
//! supports only the request shape the
//! app needs: one HTTP/1.0 or HTTP/1.1 request per connection, an optional
//! fixed `Content-Length`, and a bounded UTF-8 body.  Unsupported transfer
//! encodings are rejected rather than guessed.

use super::fixtures::{
    MAX_FIXTURE_HEADER_NAME_BYTES, MAX_FIXTURE_HEADER_NAME_CHARS, MAX_FIXTURE_HEADER_VALUE_BYTES,
    MAX_FIXTURE_HEADER_VALUE_CHARS, MAX_FIXTURE_URL_BYTES, MAX_FIXTURE_URL_CHARS,
};
use super::history::{
    MAX_BODY_BYTES, MAX_HEADERS, MAX_HEADER_BYTES, MAX_HEADER_CHARS, MAX_METHOD_BYTES,
    MAX_METHOD_CHARS,
};
use std::fmt::Write as FmtWrite;
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The per-connection socket timeout is deliberately finite so an idle
/// header/body cannot hold a worker or block listener shutdown indefinitely.
pub const REQUEST_IO_TIMEOUT_MS: u64 = 5_000;
/// Bound the number of sockets that can retain a connection worker and an
/// input buffer at once. Excess clients get a fixed 503 and are closed.
pub const MAX_ACTIVE_CONNECTIONS: usize = 64;
/// Keep the line budget explicit even though the aggregate header budget is
/// larger.  A single hostile line must not allocate the whole request budget.
pub const MAX_REQUEST_LINE_BYTES: usize = MAX_METHOD_BYTES + MAX_FIXTURE_URL_BYTES + 32;
pub const MAX_HEADER_LINE_BYTES: usize =
    1 + MAX_FIXTURE_HEADER_NAME_BYTES + 1 + MAX_FIXTURE_HEADER_VALUE_BYTES + 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    Closed,
    Cancelled,
    Malformed,
    RequestLineTooLarge,
    HeaderTooLarge,
    BodyTooLarge,
    Timeout,
    Unsupported,
    RateLimited,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

type ParsedHead = (String, String, Vec<(String, String)>, Option<usize>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineError {
    Closed,
    Malformed,
    TooLarge,
    Timeout,
    Io,
}

fn read_crlf_line<R: Read>(
    reader: &mut R,
    max_bytes: usize,
    deadline: Instant,
) -> Result<Option<Vec<u8>>, LineError> {
    let mut line = Vec::with_capacity(max_bytes.min(256));
    let mut previous_cr = false;
    loop {
        if Instant::now() >= deadline {
            return Err(LineError::Timeout);
        }
        let mut byte = [0u8; 1];
        let read = match reader.read(&mut byte) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(LineError::Timeout)
            }
            Err(_) => return Err(LineError::Io),
        };
        if read == 0 {
            return if line.is_empty() {
                Err(LineError::Closed)
            } else {
                Err(LineError::Malformed)
            };
        }
        if line.len() >= max_bytes {
            return Err(LineError::TooLarge);
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            if !previous_cr {
                return Err(LineError::Malformed);
            }
            line.pop();
            line.pop();
            return Ok(Some(line));
        }
        previous_cr = byte[0] == b'\r';
    }
}

fn map_line_error(error: LineError, oversized: ParseError) -> ParseError {
    match error {
        LineError::Closed => ParseError::Closed,
        LineError::Malformed | LineError::Io => ParseError::Malformed,
        LineError::TooLarge => oversized,
        LineError::Timeout => ParseError::Timeout,
    }
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn header_value_is_wire_safe(value: &str) -> bool {
    value.is_ascii() && value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

fn parse_content_length(value: &str) -> Result<usize, ParseError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::Malformed);
    }
    let length = value
        .parse::<usize>()
        .map_err(|_| ParseError::BodyTooLarge)?;
    if length > MAX_BODY_BYTES {
        return Err(ParseError::BodyTooLarge);
    }
    Ok(length)
}

fn parse_head(
    line: &[u8],
    headers_reader: &mut impl Read,
    running: &AtomicBool,
    deadline: Instant,
) -> Result<ParsedHead, ParseError> {
    let request_line = std::str::from_utf8(line).map_err(|_| ParseError::Malformed)?;
    let mut fields = request_line.split(' ');
    let method = fields.next().ok_or(ParseError::Malformed)?;
    let target = fields.next().ok_or(ParseError::Malformed)?;
    let version = fields.next().ok_or(ParseError::Malformed)?;
    if fields.next().is_some() || method.is_empty() || target.is_empty() {
        return Err(ParseError::Malformed);
    }
    if !within(method, MAX_METHOD_CHARS, MAX_METHOD_BYTES)
        || !within(target, MAX_FIXTURE_URL_CHARS, MAX_FIXTURE_URL_BYTES)
    {
        return Err(ParseError::RequestLineTooLarge);
    }
    if !is_token(method)
        || target.bytes().any(|byte| byte <= 0x20 || byte == 0x7f)
        || !target.is_ascii()
    {
        return Err(ParseError::Malformed);
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(ParseError::Unsupported);
    }

    let mut headers = Vec::new();
    let mut total_chars = 0usize;
    let mut total_bytes = 0usize;
    let mut content_length = None;
    loop {
        if !running.load(Ordering::Acquire) {
            return Err(ParseError::Cancelled);
        }
        let line = read_crlf_line(headers_reader, MAX_HEADER_LINE_BYTES, deadline)
            .map_err(|error| map_line_error(error, ParseError::HeaderTooLarge))?
            .ok_or(ParseError::Closed)?;
        if line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(ParseError::HeaderTooLarge);
        }
        let line = std::str::from_utf8(&line).map_err(|_| ParseError::Malformed)?;
        let (name, raw_value) = line.split_once(':').ok_or(ParseError::Malformed)?;
        let value = raw_value.trim_matches([' ', '\t']);
        if !within(
            name,
            MAX_FIXTURE_HEADER_NAME_CHARS,
            MAX_FIXTURE_HEADER_NAME_BYTES,
        ) || !is_token(name)
            || !within(
                value,
                MAX_FIXTURE_HEADER_VALUE_CHARS,
                MAX_FIXTURE_HEADER_VALUE_BYTES,
            )
            || !header_value_is_wire_safe(value)
        {
            return Err(ParseError::Malformed);
        }
        total_chars = total_chars
            .checked_add(name.chars().count())
            .and_then(|total| total.checked_add(value.chars().count()))
            .ok_or(ParseError::HeaderTooLarge)?;
        total_bytes = total_bytes
            .checked_add(name.len())
            .and_then(|total| total.checked_add(value.len()))
            .ok_or(ParseError::HeaderTooLarge)?;
        if total_chars > MAX_HEADER_CHARS || total_bytes > MAX_HEADER_BYTES {
            return Err(ParseError::HeaderTooLarge);
        }

        if name.eq_ignore_ascii_case("content-length") {
            let parsed = parse_content_length(value)?;
            if content_length.is_some_and(|previous| previous != parsed) {
                return Err(ParseError::Malformed);
            }
            content_length = Some(parsed);
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("expect")
        {
            return Err(ParseError::Unsupported);
        }
        headers.push((name.to_string(), value.to_string()));
    }

    Ok((
        method.to_ascii_uppercase(),
        target.to_string(),
        headers,
        content_length,
    ))
}

fn within(value: &str, max_chars: usize, max_bytes: usize) -> bool {
    value.chars().count() <= max_chars && value.len() <= max_bytes
}

/// Read one bounded request from a single-use connection. The admission
/// callback runs after the complete header but before body allocation/read so
/// callers can reject rate-limited requests without consuming the body.
pub fn read_request<F>(
    stream: &mut TcpStream,
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
    let mut reader =
        BufReader::with_capacity(8 * 1024, stream.try_clone().map_err(|_| ParseError::Io)?);
    let request_line = read_crlf_line(&mut reader, MAX_REQUEST_LINE_BYTES, deadline)
        .map_err(|error| map_line_error(error, ParseError::RequestLineTooLarge))?
        .ok_or(ParseError::Closed)?;
    let (method, target, headers, content_length) =
        parse_head(&request_line, &mut reader, running, deadline)?;
    if !running.load(Ordering::Acquire) {
        return Err(ParseError::Cancelled);
    }
    if !admit() {
        return Err(ParseError::RateLimited);
    }
    let length = content_length.unwrap_or(0);
    let mut body = Vec::with_capacity(length.min(MAX_BODY_BYTES));
    let mut remaining = length;
    let mut chunk = [0u8; 16 * 1024];
    while remaining > 0 {
        if !running.load(Ordering::Acquire) {
            return Err(ParseError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ParseError::Timeout);
        }
        let read_len = remaining.min(chunk.len());
        let read = loop {
            match reader.read(&mut chunk[..read_len]) {
                Ok(read) => break read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    return Err(ParseError::Timeout)
                }
                Err(_) => return Err(ParseError::Timeout),
            }
        };
        if read == 0 {
            return Err(ParseError::Timeout);
        }
        body.extend_from_slice(&chunk[..read]);
        remaining -= read;
    }

    Ok(ParsedRequest {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        417 => "Expectation Failed",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ if (100..=199).contains(&status) => "Informational",
        _ if (200..=299).contains(&status) => "Success",
        _ if (300..=399).contains(&status) => "Redirection",
        _ if (400..=499).contains(&status) => "Client Error",
        _ => "Server Error",
    }
}

fn safe_response_header(name: &str, value: &str) -> bool {
    is_token(name) && header_value_is_wire_safe(value)
}

/// Write a bounded response and close the connection at the caller boundary.
/// Transport headers are owned here; rule headers cannot override framing.
pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(String, String)],
    body: &str,
) -> io::Result<()> {
    if body.len() > MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "response body exceeds its size bound",
        ));
    }
    let mut head = String::with_capacity(256 + body.len().min(MAX_BODY_BYTES));
    let _ = write!(&mut head, "HTTP/1.1 {status} {}\r\n", reason_phrase(status));
    let mut header_count = 0usize;
    let mut header_chars = 0usize;
    let mut header_bytes = 0usize;
    for (name, value) in headers {
        if safe_response_header(name, value)
            && within(
                name,
                MAX_FIXTURE_HEADER_NAME_CHARS,
                MAX_FIXTURE_HEADER_NAME_BYTES,
            )
            && within(
                value,
                MAX_FIXTURE_HEADER_VALUE_CHARS,
                MAX_FIXTURE_HEADER_VALUE_BYTES,
            )
            && !matches!(
                name.to_ascii_lowercase().as_str(),
                "connection"
                    | "content-length"
                    | "expect"
                    | "host"
                    | "proxy-connection"
                    | "te"
                    | "trailer"
                    | "transfer-encoding"
                    | "upgrade"
            )
        {
            let next_chars = header_chars
                .checked_add(name.chars().count())
                .and_then(|total| total.checked_add(value.chars().count()));
            let next_bytes = header_bytes
                .checked_add(name.len())
                .and_then(|total| total.checked_add(value.len()));
            let (Some(next_chars), Some(next_bytes)) = (next_chars, next_bytes) else {
                continue;
            };
            if header_count >= MAX_HEADERS
                || next_chars > MAX_HEADER_CHARS
                || next_bytes > MAX_HEADER_BYTES
            {
                continue;
            }
            header_count += 1;
            header_chars = next_chars;
            header_bytes = next_bytes;
            let _ = write!(&mut head, "{name}: {value}\r\n");
        }
    }
    let _ = write!(
        &mut head,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn parser_bounds_and_normalizes_a_fixed_length_request() {
        let (mut client, mut server) = pair();
        client
            .write_all(b"post /hook HTTP/1.1\r\nContent-Length: 7\r\nX-Trace: yes\r\n\r\nbody ok")
            .unwrap();
        let running = AtomicBool::new(true);
        let parsed = read_request(&mut server, &running, || true).unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.target, "/hook");
        assert_eq!(parsed.body, "body ok");
        assert_eq!(parsed.headers[1], ("X-Trace".into(), "yes".into()));
    }

    #[test]
    fn parser_rejects_chunked_and_oversized_headers_without_input_reflection() {
        let (mut client, mut server) = pair();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
            .unwrap();
        let running = AtomicBool::new(true);
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::Unsupported)
        );

        let (mut client, mut server) = pair();
        client
            .write_all(
                format!(
                    "POST /hook HTTP/1.1\r\nX-Huge: {}\r\n\r\n",
                    "x".repeat(MAX_HEADER_LINE_BYTES)
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::HeaderTooLarge)
        );
    }

    #[test]
    fn parser_cancels_before_body_read() {
        let (mut client, mut server) = pair();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 7\r\n\r\n")
            .unwrap();
        let running = AtomicBool::new(false);
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::Cancelled)
        );
    }

    #[test]
    fn parser_times_out_a_partial_body() {
        let (mut client, mut server) = pair();
        server
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        client
            .write_all(b"POST /hook HTTP/1.1\r\nContent-Length: 3\r\n\r\nx")
            .unwrap();
        let running = AtomicBool::new(true);
        assert_eq!(
            read_request(&mut server, &running, || true),
            Err(ParseError::Timeout)
        );
        drop(client);
    }

    #[test]
    fn response_writer_owns_framing_headers() {
        let (mut client, mut server) = pair();
        server
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let writer = thread::spawn(move || {
            write_response(
                &mut server,
                201,
                &[
                    ("Content-Length".into(), "999".into()),
                    ("Host".into(), "attacker.invalid".into()),
                    ("X-Test".into(), "ok".into()),
                ],
                "body",
            )
            .unwrap();
        });
        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).unwrap();
        writer.join().unwrap();
        let response = String::from_utf8(bytes).unwrap();
        assert!(response.contains("HTTP/1.1 201 Created"));
        assert!(response.contains("X-Test: ok"));
        assert!(response.contains("Content-Length: 4\r\n"));
        assert!(!response.contains("Content-Length: 999"));
        assert!(!response.contains("attacker.invalid"));
    }
}
