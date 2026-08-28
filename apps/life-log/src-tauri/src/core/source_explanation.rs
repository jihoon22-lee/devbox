//! Stable, user-facing source scope and freshness explanations.
//!
//! These labels describe how a statistic was obtained; they never contain a
//! filesystem path, activity title, or provider payload. Freshness is a
//! display signal only: a stale snapshot is shown with its error/scope and is
//! not silently presented as live Life Log activity.

pub const FRESH_SOURCE_MAX_MS: u64 = 2 * 60 * 1_000;
pub const STALE_SOURCE_MAX_MS: u64 = 15 * 60 * 1_000;

pub fn freshness_state(
    available: bool,
    freshness_ms: Option<u64>,
    has_error: bool,
) -> &'static str {
    if has_error {
        return "error";
    }
    if !available {
        return "unknown";
    }
    match freshness_ms {
        Some(age) if age <= FRESH_SOURCE_MAX_MS => "fresh",
        Some(age) if age <= STALE_SOURCE_MAX_MS => "stale",
        Some(_) => "expired",
        None => "unknown",
    }
}

pub fn scope_for_source(producer: &str) -> &'static str {
    match producer {
        "life-log" => "live-local",
        "git" => "requested-range",
        "run-manager" | "knowledge-base" => "latest-snapshot-out-of-range",
        _ => "unavailable",
    }
}

pub fn explanation_for_source(producer: &str, scope: &str) -> &'static str {
    // Scope is more authoritative than a producer label for browser and
    // unavailable rows. A producer can be represented in a browser fixture or
    // fail before its normal source-specific explanation is meaningful.
    if scope == "browser-preview-only" {
        return "브라우저 미리보기에서는 native DB와 local snapshot을 읽지 않습니다.";
    }
    if scope == "unavailable" {
        if producer == "integration-root" {
            return "공용 integration root를 읽지 못해 source를 확인할 수 없습니다.";
        }
        return "이 source는 현재 사용할 수 없으며 통계에 조용히 합치지 않습니다.";
    }
    match producer {
        "life-log" => "Life Log의 로컬 DB를 선택한 날짜 범위와 필터로 집계합니다.",
        "git" => "설정된 프로젝트의 read-only Git count를 요청 범위로 제한합니다.",
        "run-manager" => "Run Manager의 최신 local snapshot을 provenance로만 표시하며 PC 통계에는 합치지 않습니다.",
        "knowledge-base" => "Knowledge의 최신 local snapshot을 provenance로만 표시하며 활동 원문은 읽지 않습니다.",
        "integration-root" => "공용 integration root를 읽지 못해 해당 source를 확인할 수 없습니다.",
        _ => "이 source는 현재 사용할 수 없으며 통계에 조용히 합치지 않습니다.",
    }
}

pub fn error_code(error: Option<&str>) -> Option<String> {
    let message = error?;
    let lower = message.to_ascii_lowercase();
    let code = if lower.contains("stale") {
        "snapshot_stale"
    } else if lower.contains("schema") || message.contains("지원하지 않습니다") {
        "snapshot_schema_unsupported"
    } else if lower.contains("snapshot") && (lower.contains("read") || message.contains("읽을")) {
        "snapshot_unavailable"
    } else if lower.contains("payload") {
        "snapshot_payload_invalid"
    } else if lower.contains("snapshot") {
        "snapshot_invalid"
    } else {
        "source_unavailable"
    };
    Some(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_boundaries_are_explicit() {
        assert_eq!(freshness_state(true, Some(120_000), false), "fresh");
        assert_eq!(freshness_state(true, Some(120_001), false), "stale");
        assert_eq!(freshness_state(true, Some(900_001), false), "expired");
        assert_eq!(freshness_state(false, None, true), "error");
    }

    #[test]
    fn explanations_are_path_free() {
        let text = explanation_for_source("knowledge-base", "latest-snapshot-out-of-range");
        assert!(!text.contains('/') && !text.contains('\\'));
        assert_eq!(
            explanation_for_source("life-log", "browser-preview-only"),
            "브라우저 미리보기에서는 native DB와 local snapshot을 읽지 않습니다."
        );
        assert_eq!(
            explanation_for_source("integration-root", "unavailable"),
            "공용 integration root를 읽지 못해 source를 확인할 수 없습니다."
        );
    }

    #[test]
    fn scopes_and_error_codes_are_stable() {
        assert_eq!(scope_for_source("life-log"), "live-local");
        assert_eq!(
            scope_for_source("run-manager"),
            "latest-snapshot-out-of-range"
        );
        assert_eq!(scope_for_source("git"), "requested-range");
        assert_eq!(
            error_code(Some("snapshot JSON 형식이 올바르지 않습니다")),
            Some("snapshot_invalid".into())
        );
        assert_eq!(
            error_code(Some("payload가 올바르지 않습니다")),
            Some("snapshot_payload_invalid".into())
        );
    }
}
