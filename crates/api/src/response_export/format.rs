//! UTF-8 and Markdown admission is separate from command-text limits.
use super::failed;
use crate::{ApiError, HistoryEntry, HistoryOutcome};
const BODY: usize = 1024 * 1024;
// Notes adds its durable operation marker and separator after this formatter.
const WRAPPER: usize = 16 * 1024 - 256;
fn fence(text: &str) -> Result<String, ApiError> {
    let longest = |needle: u8| {
        let mut run = 0;
        let mut longest = 0;
        for b in text.bytes() {
            if b == needle {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
        longest
    };
    let back = longest(b'`');
    let tilde = longest(b'~');
    let (character, length) = if back <= tilde {
        ('`', back + 1)
    } else {
        ('~', tilde + 1)
    };
    if length > WRAPPER / 2 {
        return Err(failed("size"));
    }
    Ok(std::iter::repeat_n(character, length.max(3)).collect())
}
fn timestamp(ms: i64) -> String {
    let remainder = ms.rem_euclid(86_400_000);
    format!(
        "{} {:02}:{:02}:{:02}.{:03} UTC",
        crate::history::day(ms),
        remainder / 3_600_000,
        remainder / 60_000 % 60,
        remainder / 1000 % 60,
        remainder % 1000
    )
}
pub(super) fn render(entry: &HistoryEntry) -> Result<String, ApiError> {
    if entry.outcome != HistoryOutcome::Ok || entry.error.is_some() {
        return Err(failed("incomplete"));
    }
    let response = entry
        .response
        .as_ref()
        .ok_or_else(|| failed("incomplete"))?;
    if entry.method.len() + entry.url.len() > WRAPPER {
        return Err(failed("size"));
    }
    let metadata = format!(
        "{} {}\n상태: {}\n전송 시각: {}",
        entry.method,
        entry.url,
        response.status,
        timestamp(entry.sent_at_ms)
    );
    if metadata.contains('\0') {
        return Err(failed("encoding"));
    }
    let meta_fence = fence(&metadata)?;
    let prefix = format!("### API 응답\n\n{meta_fence}text\n{metadata}\n{meta_fence}\n\n");
    let (body, truncated) = if response.binary {
        let media = response
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.as_str())
            .unwrap_or("application/octet-stream");
        if media.len() > 1024 || media.chars().any(char::is_control) {
            return Err(failed("size"));
        }
        (
            format!("이진 응답: {}바이트 · {}", response.size_bytes, media),
            false,
        )
    } else {
        // Verified history reader retains at most BODY bytes; reject malformed encoded input before allocation.
        if entry.body_base64.len() > BODY.div_ceil(3) * 4 {
            return Err(failed("size"));
        }
        let bytes = data_encoding::BASE64
            .decode(entry.body_base64.as_bytes())
            .map_err(|_| failed("encoding"))?;
        if bytes.len() > BODY {
            return Err(failed("size"));
        }
        let (text, cut) = match std::str::from_utf8(&bytes) {
            Ok(text) => (text, false),
            Err(error) if error.error_len().is_none() && entry.body_truncated => (
                std::str::from_utf8(&bytes[..error.valid_up_to()])
                    .map_err(|_| failed("encoding"))?,
                true,
            ),
            Err(_) => return Err(failed("encoding")),
        };
        if text.contains('\0') {
            return Err(failed("encoding"));
        }
        (text.to_owned(), cut || entry.body_truncated)
    };
    let body_fence = fence(&body)?;
    let notice = if truncated {
        "\n※ 본문이 잘렸습니다. UTF-8 기준 최대 1MiB까지만 붙였습니다.\n"
    } else {
        ""
    };
    let opening = format!("{body_fence}text\n");
    let ending = format!("\n{body_fence}\n{notice}");
    // Count every delimiter, metadata byte, binary description and notice separately from retained body.
    let wrapper =
        prefix.len() + opening.len() + ending.len() + if response.binary { body.len() } else { 0 };
    if wrapper > WRAPPER || body.len() > BODY {
        return Err(failed("size"));
    }
    Ok(format!("{prefix}{opening}{body}{ending}"))
}
