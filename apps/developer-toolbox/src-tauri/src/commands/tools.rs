use serde::Serialize;

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

/// UUID v4를 생성한다.
#[tauri::command]
pub fn generate_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
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
        let u = generate_uuid();
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(&parts[2][..1], "4");
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
