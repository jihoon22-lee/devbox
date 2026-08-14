//! devbox 공용 루트의 versioned integration snapshot reader.
//!
//! consumer는 상대 앱의 `app_local_data_dir`을 직접 읽지 않고 이 계약만 읽는다.
//! producer가 `%LOCALAPPDATA%\devbox\integration\<producer-id>\v1\summary.json`에
//! 원자적으로 기록한 snapshot을 파싱한다 (§10.1).

use serde::{Deserialize, Serialize};

/// producer-id (카탈로그 앱 ID).
pub const PRODUCER_RUN_MANAGER: &str = "run-manager";

/// 계약 root: `%LOCALAPPDATA%\devbox`
fn common_root() -> std::path::PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    std::path::PathBuf::from(base).join("devbox")
}

/// snapshot 파일 경로.
pub fn snapshot_path(producer_id: &str, version: u32) -> std::path::PathBuf {
    common_root()
        .join("integration")
        .join(producer_id)
        .join(format!("v{version}"))
        .join("summary.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationEnvelope {
    pub schema_version: u32,
    pub producer: String,
    pub producer_version: String,
    pub generated_at: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

/// snapshot을 읽는다. 파일이 없으면 Ok(None). 스키마 버전이 다르면 명확한 오류.
pub fn read_snapshot(
    producer_id: &str,
    version: u32,
) -> Result<Option<IntegrationEnvelope>, String> {
    let path = snapshot_path(producer_id, version);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    let envelope: IntegrationEnvelope =
        serde_json::from_str(&text).map_err(|e| format!("snapshot 파싱 실패: {e}"))?;
    if envelope.producer != producer_id {
        return Err(format!(
            "snapshot producer 불일치: 기대 {producer_id}, 실제 {}",
            envelope.producer
        ));
    }
    if envelope.schema_version != version {
        return Err(format!(
            "snapshot schema version 불일치: 기대 v{version}, 실제 v{}",
            envelope.schema_version
        ));
    }
    Ok(Some(envelope))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_uses_common_root_and_app_id() {
        let p = snapshot_path(PRODUCER_RUN_MANAGER, 1);
        let p = p.to_string_lossy().replace('\\', "/");
        assert!(
            p.ends_with("/devbox/integration/run-manager/v1/summary.json"),
            "{p}"
        );
    }

    #[test]
    fn parses_valid_envelope() {
        let json = r#"{
            "schemaVersion": 1,
            "producer": "run-manager",
            "producerVersion": "0.3.0",
            "generatedAt": "2026-08-14T12:00:00Z",
            "data": { "runs": { "success": 1, "failed": 0 } }
        }"#;
        let env: IntegrationEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(env.schema_version, 1);
        assert_eq!(env.producer, "run-manager");
        assert_eq!(env.data["runs"]["success"], 1);
    }

    #[test]
    fn missing_snapshot_reads_none() {
        let producer = format!("nonexistent-producer-{}", std::process::id());
        assert_eq!(read_snapshot(&producer, 99).unwrap(), None);
    }
}
