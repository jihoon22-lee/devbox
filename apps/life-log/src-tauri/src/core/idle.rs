//! idle 감지 정책 (순수 로직). 시간 입력만 받아 WSL에서 테스트한다.

/// 기본 idle threshold (ms). 자리를 비운 5분 후부터 idle로 본다.
pub const DEFAULT_IDLE_THRESHOLD_MS: i64 = 5 * 60 * 1000;

/// threshold 최소값 (1분) — 0이나 음수로 설정해 항상 idle이 되는 것을 막는다.
pub const MIN_IDLE_THRESHOLD_MS: i64 = 60 * 1000;

/// 마지막 입력 이후 경과가 threshold 이상이면 idle.
pub fn is_idle(since_last_input_ms: i64, threshold_ms: i64) -> bool {
    since_last_input_ms >= threshold_ms
}

/// idle이면 현재 세션을 끝낼 시각(epoch ms)을 반환한다.
/// 세션은 idle 시작 시점(now - idle)에서 끝나야 한다 — 자리를 비운 시간이
/// 앱 사용 시간에 집계되지 않도록.
pub fn session_end_on_idle(now: i64, since_last_input_ms: i64, threshold_ms: i64) -> Option<i64> {
    if is_idle(since_last_input_ms, threshold_ms) {
        Some(now - since_last_input_ms)
    } else {
        None
    }
}

/// 사용자 설정 문자열을 threshold(ms)로 파싱한다. 형식이 아니거나 최소값 미만이면
/// 기본값을 쓴다.
pub fn parse_threshold_ms(value: &str) -> i64 {
    let parsed = value
        .trim()
        .parse::<i64>()
        .unwrap_or(DEFAULT_IDLE_THRESHOLD_MS);
    if parsed < MIN_IDLE_THRESHOLD_MS {
        DEFAULT_IDLE_THRESHOLD_MS
    } else {
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_boundary() {
        assert!(!is_idle(299_999, 300_000));
        assert!(is_idle(300_000, 300_000));
        assert!(is_idle(600_000, 300_000));
    }

    #[test]
    fn session_ends_at_idle_start() {
        // now=1000, idle 10분, threshold 5분 → 세션은 idle 시작(now-600000)에서 끝
        assert_eq!(
            session_end_on_idle(1_000_000, 600_000, 300_000),
            Some(400_000)
        );
        // idle 미만이면 세션 유지
        assert_eq!(session_end_on_idle(1_000_000, 200_000, 300_000), None);
    }

    #[test]
    fn parse_threshold_clamps() {
        assert_eq!(parse_threshold_ms("600000"), 600_000);
        assert_eq!(parse_threshold_ms("garbage"), DEFAULT_IDLE_THRESHOLD_MS);
        assert_eq!(parse_threshold_ms("1000"), DEFAULT_IDLE_THRESHOLD_MS); // 최소 미만
        assert_eq!(parse_threshold_ms(""), DEFAULT_IDLE_THRESHOLD_MS);
    }
}
