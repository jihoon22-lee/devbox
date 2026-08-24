//! `wsl.exe` 출력 디코딩.
//!
//! 파이프로 실행된 `wsl.exe`(`-l -v`, `--version`)는 UTF-16LE(BOM 또는 다량의
//! NUL)로 출력한다. 이 경우 UTF-16LE로, 그 외에는 UTF-8로 해석한다. (출력에
//! NUL이 남아 다음 명령의 인자가 되면 "null byte found in provided data"
//! 오류가 발생한다)
//!
//! 추출 근거: wsl-desktop에 있던 구현을 devbox-manager(환경 진단)가 두 번째로
//! 필요하게 되어 `crates/wsl`로 옮겼다. 로직은 원본과 동일하다.

/// `wsl.exe` 출력을 BOM/NUL 비율로 판별해 UTF-16LE 또는 UTF-8로 디코딩한다.
pub fn decode_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let null_ratio = if bytes.is_empty() {
        0
    } else {
        bytes.iter().filter(|b| **b == 0).count() * 4 / bytes.len()
    };
    if bom || (bytes.len() >= 2 && null_ratio >= 1) {
        let start = if bom { 2 } else { 0 };
        let units: Vec<u16> = bytes[start..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8() {
        assert_eq!(decode_output(b"Ubuntu\n"), "Ubuntu\n");
    }

    #[test]
    fn decodes_utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for c in "Ubuntu\n".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let s = decode_output(&bytes);
        assert_eq!(s, "Ubuntu\n");
    }

    #[test]
    fn decodes_utf16le_null_heavy() {
        let mut bytes = Vec::new();
        for c in "Ubuntu".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        // BOM은 없지만 NUL 비율이 높아 UTF-16LE로 판별돼야 한다.
        let s = decode_output(&bytes);
        assert_eq!(s, "Ubuntu");
    }

    #[test]
    fn ignores_incomplete_utf16le_trailing_byte() {
        let bytes = [0xFF, 0xFE, b'A', 0x00, 0xFF];
        assert_eq!(decode_output(&bytes), "A");
    }

    #[test]
    fn keeps_non_wsl_utf8_unchanged() {
        let s = decode_output("프로토콜".as_bytes());
        assert_eq!(s, "프로토콜");
    }
}
