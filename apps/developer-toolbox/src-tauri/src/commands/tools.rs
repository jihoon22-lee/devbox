use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const MAX_IDENTIFIER_BATCH: usize = 100;
const MAX_IDENTIFIER_TIMESTAMP: u64 = (1u64 << 48) - 1;
const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SECURE_RANDOM_ERROR: &str = "암호학적으로 안전한 난수를 사용할 수 없습니다.";
const IDENTIFIER_SEQUENCE_ERROR: &str = "식별자 생성 순서를 유지할 수 없습니다.";

// UUID v7 ordering is process-local by contract. The mutex serializes calls
// from concurrent Tauri commands and keeps a later call ordered after an
// earlier call even when the wall clock repeats or moves backwards.
static UUID_V7_STATE: Mutex<Option<[u8; 16]>> = Mutex::new(None);

/// Identifier generation request shared by the Tauri command and the UI.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateIdsRequest {
    pub kind: String,
    pub count: usize,
    pub uppercase: bool,
    pub hyphens: bool,
}

/// 정규식 매치 하나.
#[derive(Debug, Clone, Serialize)]
pub struct RegexMatch {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// diff 변경 구간 하나.
#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    /// 0 = 같은 구간, 1 = 삽입(+), 2 = 삭제(-)
    pub kind: u8,
    pub old_start: usize,
    pub old_end: usize,
    pub new_start: usize,
    pub new_end: usize,
}

/// 입력 데이터를 지정한 알고리즘으로 해시한다.
/// `algorithm`: "md5" | "sha256" | "sha512"
#[tauri::command]
pub fn hash(data: String, algorithm: String) -> Result<String, String> {
    use md5::Digest;
    use sha2::Sha256;
    use sha2::Sha512;

    let bytes = data.as_bytes();
    match algorithm.to_lowercase().as_str() {
        "md5" => Ok(hex(&md5::Md5::digest(bytes))),
        "sha256" => Ok(hex(&Sha256::digest(bytes))),
        "sha512" => Ok(hex(&Sha512::digest(bytes))),
        other => Err(format!(
            "지원하지 않는 알고리즘: {other} (md5/sha256/sha512)"
        )),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 기존 UUID v4 호출과의 호환을 유지하면서 bounded generator와 같은 오류 경계를 사용한다.
#[tauri::command]
pub fn generate_uuid() -> Result<String, String> {
    generate_identifier_batch("uuid-v4", 1, false, true)
        .and_then(|mut values| values.pop().ok_or_else(|| SECURE_RANDOM_ERROR.to_string()))
}

/// UUID v4/v7 또는 ULID를 제한된 수량으로 생성한다.
#[tauri::command]
pub fn generate_ids(request: GenerateIdsRequest) -> Result<Vec<String>, String> {
    generate_identifier_batch(
        &request.kind,
        request.count,
        request.uppercase,
        request.hyphens,
    )
}

fn generate_identifier_batch(
    kind: &str,
    count: usize,
    uppercase: bool,
    hyphens: bool,
) -> Result<Vec<String>, String> {
    if !(1..=MAX_IDENTIFIER_BATCH).contains(&count) {
        return Err(format!(
            "생성 수량은 1에서 {MAX_IDENTIFIER_BATCH} 사이여야 합니다."
        ));
    }
    if !matches!(kind, "uuid-v4" | "uuid-v7" | "ulid") {
        return Err("지원하지 않는 식별자 종류입니다.".to_string());
    }

    let mut values = Vec::with_capacity(count);
    let mut previous_ulid = None;
    for _ in 0..count {
        let value = match kind {
            "uuid-v4" => format_uuid(generate_uuid_v4()?, uppercase, hyphens),
            // The local state is deliberately not persisted or shared with
            // another process/machine, so only process-local ordering is
            // promised.
            "uuid-v7" => format_uuid(generate_uuid_v7()?, uppercase, hyphens),
            "ulid" => {
                let raw = generate_ulid(previous_ulid)?;
                previous_ulid = Some(raw);
                format_ulid(raw, uppercase, hyphens)
            }
            _ => unreachable!("kind was validated above"),
        };
        values.push(value);
    }
    Ok(values)
}

fn secure_random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| SECURE_RANDOM_ERROR.to_string())?;
    Ok(bytes)
}

fn generate_uuid_v4() -> Result<uuid::Uuid, String> {
    let mut bytes = secure_random_bytes::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes))
}

fn generate_uuid_v7() -> Result<uuid::Uuid, String> {
    let milliseconds = current_timestamp_millis();
    let random = secure_random_bytes::<16>()?;
    let mut state = UUID_V7_STATE
        .lock()
        .map_err(|_| IDENTIFIER_SEQUENCE_ERROR.to_string())?;
    let value = generate_uuid_v7_from_parts(milliseconds, random, *state)?;
    *state = Some(value);
    Ok(uuid::Uuid::from_bytes(value))
}

/// Builds a UUID v7 from deterministic inputs while preserving RFC 9562's
/// version/variant fields. This pure boundary is shared by the native state
/// machine and fixtures for repeated clocks, rollback, and exhaustion.
fn generate_uuid_v7_from_parts(
    milliseconds: u64,
    random: [u8; 16],
    previous: Option<[u8; 16]>,
) -> Result<[u8; 16], String> {
    let milliseconds = milliseconds.min(MAX_IDENTIFIER_TIMESTAMP);
    let mut value = random;
    write_timestamp(&mut value, milliseconds);
    value[6] = (value[6] & 0x0f) | 0x70;
    value[8] = (value[8] & 0x3f) | 0x80;

    let Some(previous) = previous else {
        return Ok(value);
    };
    let previous_timestamp = timestamp_from(&previous);
    if milliseconds > previous_timestamp {
        return Ok(value);
    }

    let mut monotonic = previous;
    if increment_uuid_v7_suffix(&mut monotonic) {
        return Ok(monotonic);
    }
    if previous_timestamp >= MAX_IDENTIFIER_TIMESTAMP {
        return Err(IDENTIFIER_SEQUENCE_ERROR.to_string());
    }
    write_timestamp(&mut value, previous_timestamp + 1);
    Ok(value)
}

fn current_timestamp_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
        .min(u128::from(MAX_IDENTIFIER_TIMESTAMP)) as u64
}

fn increment_uuid_v7_suffix(value: &mut [u8; 16]) -> bool {
    let mut carry = 1u8;
    for index in (9..16).rev() {
        let (next, overflow) = value[index].overflowing_add(carry);
        value[index] = next;
        carry = u8::from(overflow);
        if carry == 0 {
            return true;
        }
    }

    let next = (value[8] & 0x3f) + carry;
    value[8] = (value[8] & 0xc0) | (next & 0x3f);
    carry = u8::from(next > 0x3f);
    if carry == 0 {
        return true;
    }

    let (next, overflow) = value[7].overflowing_add(carry);
    value[7] = next;
    carry = u8::from(overflow);
    if carry == 0 {
        return true;
    }

    let next = (value[6] & 0x0f) + carry;
    value[6] = (value[6] & 0xf0) | (next & 0x0f);
    next <= 0x0f
}

fn format_uuid(value: uuid::Uuid, uppercase: bool, hyphens: bool) -> String {
    let mut result = if hyphens {
        value.to_string()
    } else {
        value.simple().to_string()
    };
    if uppercase {
        result.make_ascii_uppercase();
    }
    result
}

fn generate_ulid(previous: Option<[u8; 16]>) -> Result<[u8; 16], String> {
    generate_ulid_from_parts(current_timestamp_millis(), secure_random_bytes()?, previous)
}

/// Builds a ULID from deterministic inputs. Keeping clock/random acquisition
/// outside this function makes rollback and exhaustion behavior testable
/// without weakening the production CSPRNG path.
fn generate_ulid_from_parts(
    milliseconds: u64,
    random: [u8; 10],
    previous: Option<[u8; 16]>,
) -> Result<[u8; 16], String> {
    let milliseconds = milliseconds.min(MAX_IDENTIFIER_TIMESTAMP);
    let mut value = [0u8; 16];
    write_timestamp(&mut value, milliseconds);
    value[6..].copy_from_slice(&random);

    let Some(previous) = previous else {
        return Ok(value);
    };
    let previous_timestamp = timestamp_from(&previous);
    if milliseconds > previous_timestamp {
        return Ok(value);
    }

    let mut monotonic = previous;
    if increment_ulid_suffix(&mut monotonic) {
        return Ok(monotonic);
    }
    if previous_timestamp >= MAX_IDENTIFIER_TIMESTAMP {
        return Err(IDENTIFIER_SEQUENCE_ERROR.to_string());
    }
    // The already generated random tail is retained when the timestamp must
    // advance after a suffix overflow. This path is unreachable at the public
    // batch limit in practice, but keeps the no-duplicate contract explicit.
    write_timestamp(&mut value, previous_timestamp + 1);
    Ok(value)
}

fn timestamp_from(value: &[u8; 16]) -> u64 {
    value[..6]
        .iter()
        .fold(0u64, |timestamp, byte| timestamp * 256 + u64::from(*byte))
}

fn write_timestamp(value: &mut [u8; 16], timestamp: u64) {
    value[..6].copy_from_slice(&timestamp.to_be_bytes()[2..]);
}

fn increment_ulid_suffix(value: &mut [u8; 16]) -> bool {
    for index in (6..16).rev() {
        let (next, overflow) = value[index].overflowing_add(1);
        value[index] = next;
        if !overflow {
            return true;
        }
    }
    false
}

fn encode_ulid(value: [u8; 16]) -> String {
    // A 128-bit ULID is represented by 26 groups of 5 bits. The first two
    // bits are zero, which also enforces the canonical Crockford upper bound.
    let mut buffer = 0u32;
    let mut bit_count = 2u8;
    let mut result = String::with_capacity(26);

    for byte in value {
        buffer = (buffer << 8) | u32::from(byte);
        bit_count += 8;
        while bit_count >= 5 {
            bit_count -= 5;
            result.push(CROCKFORD_ALPHABET[((buffer >> bit_count) & 0x1f) as usize] as char);
            buffer &= if bit_count == 0 {
                0
            } else {
                (1u32 << bit_count) - 1
            };
        }
    }
    result
}

fn format_ulid(value: [u8; 16], uppercase: bool, hyphens: bool) -> String {
    let raw = encode_ulid(value);
    let cased = if uppercase {
        raw
    } else {
        raw.to_ascii_lowercase()
    };
    if !hyphens {
        return cased;
    }
    format!(
        "{}-{}-{}-{}-{}",
        &cased[0..5],
        &cased[5..10],
        &cased[10..15],
        &cased[15..20],
        &cased[20..]
    )
}

/// 정규식을 텍스트에 적용해 전체 매치(위치·본문)를 반환한다.
/// 매치는 0부터 시작하는 바이트 오프셋 기준이다.
#[tauri::command]
pub fn regex_test(pattern: String, text: String) -> Result<Vec<RegexMatch>, String> {
    let re = regex::Regex::new(&pattern).map_err(|e| format!("정규식 오류: {e}"))?;
    Ok(re
        .find_iter(&text)
        .map(|m| RegexMatch {
            start: m.start(),
            end: m.end(),
            text: m.as_str().to_string(),
        })
        .collect())
}

/// 두 텍스트의 차이를 라인 단위 변경 구간으로 반환한다.
/// 오프셋은 0부터 시작하는 라인 번호 (kind: 0=equal, 1=insert, 2=delete).
#[tauri::command]
pub fn diff(a: String, b: String) -> Vec<DiffHunk> {
    use similar::TextDiff;

    let text_diff = TextDiff::from_lines(&a, &b);
    text_diff
        .ops()
        .iter()
        .map(|op| {
            use similar::DiffOp;
            let (kind, old_start, old_end, new_start, new_end) = match op {
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => (0, *old_index, old_index + len, *new_index, new_index + len),
                DiffOp::Insert {
                    new_index, new_len, ..
                } => (1, 0, 0, *new_index, new_index + new_len),
                DiffOp::Delete {
                    old_index,
                    old_len,
                    new_index,
                } => (2, *old_index, old_index + old_len, *new_index, *new_index),
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => (
                    2,
                    *old_index,
                    old_index + old_len,
                    *new_index,
                    new_index + new_len,
                ),
            };
            DiffHunk {
                kind,
                old_start,
                old_end,
                new_start,
                new_end,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_known_vectors() {
        assert_eq!(
            hash("abc".into(), "md5".into()).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hash("abc".into(), "sha256".into()).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hash_rejects_unknown_algorithm() {
        assert!(hash("x".into(), "sha1".into()).is_err());
    }

    #[test]
    fn uuid_is_v4_format() {
        let u = generate_uuid().unwrap();
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(&parts[2][..1], "4");
    }

    #[test]
    fn identifier_batch_supports_uuid_versions_and_formats() {
        let compact = generate_identifier_batch("uuid-v4", 2, false, false).unwrap();
        assert_eq!(compact.len(), 2);
        assert!(compact.iter().all(|value| {
            value.len() == 32
                && value.chars().all(|character| {
                    character.is_ascii_hexdigit()
                        && (character.is_ascii_lowercase() || character.is_ascii_digit())
                })
        }));
        assert!(compact
            .iter()
            .all(|value| uuid::Uuid::parse_str(value).is_ok()));

        let formatted = generate_identifier_batch("uuid-v7", 1, true, true).unwrap();
        let value = uuid::Uuid::parse_str(&formatted[0]).unwrap();
        assert_eq!(formatted[0].len(), 36);
        assert_eq!(value.as_bytes()[6] >> 4, 7);
        assert_eq!(value.as_bytes()[8] & 0xc0, 0x80);
        assert_eq!(formatted[0], formatted[0].to_ascii_uppercase());
    }

    #[test]
    fn identifier_batch_generates_canonical_crockford_ulids() {
        let canonical = generate_identifier_batch("ulid", 3, true, false).unwrap();
        assert_eq!(canonical.len(), 3);
        assert!(canonical.iter().all(|value| {
            value.len() == 26
                && value
                    .chars()
                    .next()
                    .is_some_and(|character| character <= '7')
                && value
                    .chars()
                    .all(|character| CROCKFORD_ALPHABET.contains(&(character as u8)))
        }));

        let grouped = generate_identifier_batch("ulid", 1, false, true).unwrap();
        assert_eq!(grouped[0].len(), 30);
        assert_eq!(grouped[0].matches('-').count(), 4);
        assert!(grouped[0].chars().all(|character| character == '-'
            || character.is_ascii_lowercase()
            || character.is_ascii_digit()));
    }

    #[test]
    fn identifier_batch_keeps_v7_and_ulid_order_within_a_request() {
        let uuid_v7 = generate_identifier_batch("uuid-v7", 32, false, false).unwrap();
        assert!(uuid_v7.windows(2).all(|pair| pair[0] < pair[1]));

        let first_call = generate_identifier_batch("uuid-v7", 1, false, false).unwrap();
        let second_call = generate_identifier_batch("uuid-v7", 1, false, false).unwrap();
        assert!(first_call[0] < second_call[0]);

        let ulid = generate_identifier_batch("ulid", 32, true, false).unwrap();
        assert!(ulid.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn uuid_v7_monotonic_state_handles_repeated_and_backward_clock() {
        let first = generate_uuid_v7_from_parts(1_700, [0x22; 16], None).unwrap();
        let repeated = generate_uuid_v7_from_parts(1_700, [0x00; 16], Some(first)).unwrap();
        let backward = generate_uuid_v7_from_parts(1_600, [0x00; 16], Some(repeated)).unwrap();

        assert_eq!(timestamp_from(&repeated), 1_700);
        assert_eq!(timestamp_from(&backward), 1_700);
        assert!(first < repeated);
        assert!(repeated < backward);
        assert_eq!(first[6] >> 4, 7);
        assert_eq!(first[8] & 0xc0, 0x80);
    }

    #[test]
    fn uuid_v7_suffix_overflow_advances_timestamp_or_fails_at_upper_bound() {
        let previous = generate_uuid_v7_from_parts(42, [u8::MAX; 16], None).unwrap();
        let advanced = generate_uuid_v7_from_parts(42, [0x11; 16], Some(previous)).unwrap();
        assert_eq!(timestamp_from(&advanced), 43);
        assert_eq!(advanced[6] >> 4, 7);
        assert_eq!(advanced[8] & 0xc0, 0x80);
        assert!(previous < advanced);

        let at_max =
            generate_uuid_v7_from_parts(MAX_IDENTIFIER_TIMESTAMP, [u8::MAX; 16], None).unwrap();
        assert_eq!(
            generate_uuid_v7_from_parts(MAX_IDENTIFIER_TIMESTAMP, [0; 16], Some(at_max)),
            Err(IDENTIFIER_SEQUENCE_ERROR.to_string())
        );
    }

    #[test]
    fn ulid_encoder_matches_canonical_boundary_vectors() {
        assert_eq!(encode_ulid([0u8; 16]), "00000000000000000000000000");
        assert_eq!(encode_ulid([u8::MAX; 16]), format!("7{}", "Z".repeat(25)));
    }

    #[test]
    fn ulid_encoder_matches_published_vector() {
        assert_eq!(
            encode_ulid([
                0x01, 0x56, 0x3e, 0x3a, 0xb5, 0xd3, 0xd6, 0x76, 0x4c, 0x61, 0xef, 0xb9, 0x93, 0x02,
                0xbd, 0x5b,
            ]),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[test]
    fn ulid_monotonic_suffix_handles_repeated_and_backward_clock() {
        let first = generate_ulid_from_parts(1_700, [0x22; 10], None).unwrap();
        let repeated = generate_ulid_from_parts(1_700, [0x00; 10], Some(first)).unwrap();
        let backward = generate_ulid_from_parts(1_600, [0x00; 10], Some(repeated)).unwrap();

        assert_eq!(timestamp_from(&repeated), 1_700);
        assert_eq!(timestamp_from(&backward), 1_700);
        assert!(first < repeated);
        assert!(repeated < backward);
    }

    #[test]
    fn ulid_suffix_overflow_advances_timestamp_or_fails_at_upper_bound() {
        let previous = generate_ulid_from_parts(42, [u8::MAX; 10], None).unwrap();
        let advanced = generate_ulid_from_parts(42, [0x11; 10], Some(previous)).unwrap();
        assert_eq!(timestamp_from(&advanced), 43);
        assert_eq!(&advanced[6..], &[0x11; 10]);
        assert!(previous < advanced);

        let at_max =
            generate_ulid_from_parts(MAX_IDENTIFIER_TIMESTAMP, [u8::MAX; 10], None).unwrap();
        assert_eq!(
            generate_ulid_from_parts(MAX_IDENTIFIER_TIMESTAMP, [0; 10], Some(at_max),),
            Err(IDENTIFIER_SEQUENCE_ERROR.to_string())
        );
    }

    #[test]
    fn identifier_batch_rejects_unknown_kind_and_out_of_range_count() {
        assert!(generate_identifier_batch("uuid-v4", 0, false, true).is_err());
        assert!(
            generate_identifier_batch("uuid-v4", MAX_IDENTIFIER_BATCH + 1, false, true).is_err()
        );
        assert!(generate_identifier_batch("uuid-v5", 1, false, true).is_err());
    }

    #[test]
    fn regex_test_finds_all_matches() {
        let matches = regex_test("a".into(), "banana".into()).unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].text, "a");
        assert_eq!(matches[0].start, 1);
        assert_eq!(matches[1].start, 3);
    }

    #[test]
    fn regex_test_reports_invalid_pattern() {
        assert!(regex_test("(".into(), "x".into()).is_err());
    }

    #[test]
    fn diff_detects_insertion() {
        let hunks = diff("a\nb\n".into(), "a\nb\nc\n".into());
        let insert: Vec<&DiffHunk> = hunks.iter().filter(|h| h.kind == 1).collect();
        assert_eq!(insert.len(), 1);
        assert_eq!(insert[0].new_start, 2);
        assert_eq!(insert[0].new_end, 3);
    }

    #[test]
    fn diff_detects_deletion() {
        let hunks = diff("a\nb\nc\n".into(), "a\nb\n".into());
        let delete: Vec<&DiffHunk> = hunks.iter().filter(|h| h.kind == 2).collect();
        assert_eq!(delete.len(), 1);
        assert_eq!(delete[0].old_start, 2);
    }

    #[test]
    fn diff_identical_text_has_no_changes() {
        let hunks = diff("same\n".into(), "same\n".into());
        assert!(hunks.iter().all(|h| h.kind == 0));
    }
}
