use serde::Serialize;

/// 네트워크 포트 하나를 나타내는 구조체.
/// Rust 커맨드와 프론트(serde) 사이에서 그대로 JSON으로 직렬화된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortInfo {
    /// 프로토콜 (예: "TCP", "TCP6", "UDP")
    pub proto: String,
    /// 로컬 주소 문자열 (예: "0.0.0.0:3000", "[::1]:3000")
    pub local_addr: String,
    /// 파싱된 포트 번호 (추출 불가 시 0)
    pub port: u16,
    /// 연결 상태 (TCP: LISTENING/ESTABLISHED 등, UDP는 빈 문자열)
    pub state: String,
    /// 소유 프로세스 PID (없으면 None)
    pub pid: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PortInfo {
        PortInfo {
            proto: "TCP".into(),
            local_addr: "0.0.0.0:3000".into(),
            port: 3000,
            state: "LISTENING".into(),
            pid: Some(18231),
        }
    }

    #[test]
    fn serde_serializes_fields() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("\"port\":3000"));
        assert!(json.contains("\"pid\":18231"));
    }
}
