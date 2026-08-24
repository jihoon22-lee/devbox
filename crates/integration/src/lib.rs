//! devbox 공용 루트 integration snapshot 프리미티브.
//!
//! 추출 근거: §10.1 두 번째 producer(knowledge-base)가 첫 producer(run-manager)와
//! 같은 envelope을 쓰게 된다. envelope 직렬화·원자 기록·경로 계산만 여기로 옮긴다.
//! producer별 데이터 내용은 앱에 남는다.
//!
//! 경로: `%LOCALAPPDATA%\devbox\integration\<producer-id>\v1\summary.json`
//! consumer는 카탈로그 앱 ID만 알면 된다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub schema_version: u32,
    pub producer: String,
    pub producer_version: String,
    pub generated_at: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Envelope {
    pub fn new(
        producer: impl Into<String>,
        producer_version: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            schema_version: 1,
            producer: producer.into(),
            producer_version: producer_version.into(),
            generated_at: utc_now(),
            data,
        }
    }
}

/// 계약 root: `%LOCALAPPDATA%\devbox`
pub fn common_root() -> std::path::PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    std::path::PathBuf::from(base).join("devbox")
}

/// snapshot 디렉터리: `<common>/integration/<producer-id>/v<version>`
pub fn snapshot_dir(producer_id: &str, version: u32) -> std::path::PathBuf {
    common_root()
        .join("integration")
        .join(producer_id)
        .join(format!("v{version}"))
}

/// snapshot 파일 경로.
pub fn snapshot_path(producer_id: &str, version: u32) -> std::path::PathBuf {
    snapshot_dir(producer_id, version).join("summary.json")
}

/// 원자 기록: 고유한 임시 파일을 sync한 뒤 overwrite-capable rename으로
/// commit한다. 실패 시 해당 호출의 임시 파일만 정리한다.
pub fn write_atomic(envelope: &Envelope, dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?;
    let target = dir.join("summary.json");
    devbox_filesystem::atomic_write(target, json.as_bytes()).map_err(|error| error.to_string())
}

/// snapshot 읽기. 파일이 없으면 Ok(None). producer·schema version이 다르면 명확한 오류.
pub fn read_snapshot(producer_id: &str, version: u32) -> Result<Option<Envelope>, String> {
    let path = snapshot_path(producer_id, version);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    let envelope: Envelope =
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

/// ISO-8601 UTC (마이크로초 생략).
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    let (h, m, s) = (day_secs / 3600, (day_secs % 3600) / 60, day_secs % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_uses_common_root() {
        let p = snapshot_path("run-manager", 1)
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            p.ends_with("/devbox/integration/run-manager/v1/summary.json"),
            "{p}"
        );
    }

    #[test]
    fn roundtrips_envelope() {
        let dir = std::env::temp_dir().join(format!("integration-test-{}", std::process::id()));
        let env = Envelope::new("run-manager", "0.3.0", serde_json::json!({ "a": 1 }));
        write_atomic(&env, &dir).unwrap();
        let read = read_snapshot("run-manager", 1).unwrap_or(None);
        // 다른 디렉터리를 읽지 않도록 직접 경로로 재확인
        let text = std::fs::read_to_string(dir.join("summary.json")).unwrap();
        let parsed: Envelope = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.producer, "run-manager");
        assert_eq!(parsed.data["a"], 1);
        assert!(read.is_none(), "기본 경로에는 없다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_snapshot_is_none() {
        assert_eq!(read_snapshot("nonexistent-producer", 99).unwrap(), None);
    }

    #[test]
    fn rejects_wrong_version() {
        // read_snapshot은 공용 루트 경로를 읽는다 — 그 경로에 직접 써서 검증
        let path = snapshot_path("x", 1);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"schemaVersion":2,"producer":"x","producerVersion":"1","generatedAt":"t","data":{}}"#,
        )
        .unwrap();
        let err = read_snapshot("x", 1).unwrap_err();
        assert!(err.contains("schema version"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        let secs: i64 = 1_783_000_000;
        let (y, m, _d) = civil_from_days(secs.div_euclid(86_400));
        assert_eq!((y, m), (2026, 7));
    }
}
