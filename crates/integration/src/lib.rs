//! devbox 공용 integration snapshot 계약.
//!
//! producer는 `<common-root>/integration/<producer-id>/v<n>/summary.json` 또는
//! independently versioned named view snapshot을 원자적으로 교체하고 consumer는
//! 이 크레이트를 통해서만 발견·검증·읽기한다. 기존 discovery는 `summary.json`만
//! 열거하므로 named view를 추가해도 legacy consumer가 보던 목록은 바뀌지 않는다.
//! producer별 업무 데이터는 앱에 남고, 이 크레이트는 경로·envelope·multi-view·
//! freshness·보안 경계만 소유한다.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 하나의 snapshot 파일이 차지할 수 있는 최대 크기.
pub const MAX_SNAPSHOT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_DISCOVERY_ENTRIES: usize = 4_096;
const MAX_JSON_DEPTH: usize = 32;
const MAX_OPAQUE_IDENTITY_SOURCE_BYTES: usize = 32_767;

/// Produce a stable digest identifier for a canonical identity that must
/// cross an integration snapshot boundary without embedding its source.
///
/// The namespace is included in the digest so an identifier minted for one
/// domain cannot be confused with another domain that happens to use the same
/// canonical source string. Callers still own canonicalization; this helper
/// only owns the privacy-safe integration representation.
pub fn opaque_identity(namespace: &str, canonical_source: &str) -> Result<String, String> {
    validate_kebab_identifier(
        namespace,
        32,
        "opaque identity namespace가 올바르지 않습니다",
    )?;
    if canonical_source.is_empty()
        || canonical_source.len() > MAX_OPAQUE_IDENTITY_SOURCE_BYTES
        || canonical_source.chars().any(char::is_control)
    {
        return Err("opaque identity source가 올바르지 않습니다".into());
    }

    let mut digest = Sha256::new();
    digest.update(namespace.as_bytes());
    digest.update([0]);
    digest.update(canonical_source.as_bytes());
    Ok(format!(
        "{namespace}-{}",
        encode_lower_hex(&digest.finalize())
    ))
}

fn encode_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

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

/// `data.views.<kind>`의 versioned read-only summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    pub schema_version: u32,
    /// envelope 생성 시점에 이 view 데이터가 이미 경과한 시간.
    pub freshness_ms: u64,
    pub entries: Vec<serde_json::Value>,
}

pub type SnapshotViews = BTreeMap<String, SnapshotView>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotViewRef {
    pub kind: String,
    pub schema_version: u32,
    /// view 자체 경과 시간과 snapshot 파일 경과 시간을 합친 현재 freshness.
    pub freshness_ms: u64,
    pub entry_count: usize,
}

/// 검증을 통과한 snapshot의 발견 결과. payload는 필요할 때 `read_snapshot`으로 읽는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRef {
    pub producer: String,
    pub version: u32,
    pub producer_version: String,
    pub generated_at: String,
    pub path: PathBuf,
    pub freshness_ms: u64,
    pub views: Vec<SnapshotViewRef>,
}

/// 다른 producer 발견을 중단시키지 않는 안전한 개별 오류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotIssue {
    pub producer: String,
    pub version: Option<u32>,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub snapshots: Vec<SnapshotRef>,
    pub issues: Vec<SnapshotIssue>,
    pub root_error: Option<String>,
}

impl Envelope {
    /// 기존 flat `data` producer를 위한 호환 생성자.
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

    /// 여러 kind를 한 envelope에 모아 한 번에 교체하기 위한 생성자.
    pub fn with_views(
        producer: impl Into<String>,
        producer_version: impl Into<String>,
        views: SnapshotViews,
    ) -> Self {
        Self {
            schema_version: 1,
            producer: producer.into(),
            producer_version: producer_version.into(),
            generated_at: utc_now(),
            data: serde_json::json!({ "views": views }),
        }
    }

    /// 새 multi-view envelope이면 검증된 view를, 기존 flat envelope이면 빈 map을 반환한다.
    pub fn views(&self) -> Result<SnapshotViews, String> {
        let Some(data) = self.data.as_object() else {
            return Err("snapshot data 형식이 올바르지 않습니다".into());
        };
        let Some(views) = data.get("views") else {
            return Ok(BTreeMap::new());
        };
        serde_json::from_value(views.clone())
            .map_err(|_| "snapshot views 형식이 올바르지 않습니다".into())
    }
}

/// 계약 root: `%LOCALAPPDATA%\devbox`.
pub fn common_root() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    PathBuf::from(base).join("devbox")
}

/// 모든 producer를 담는 integration root.
pub fn integration_root() -> PathBuf {
    common_root().join("integration")
}

/// snapshot 디렉터리: `<common>/integration/<producer-id>/v<version>`.
pub fn snapshot_dir(producer_id: &str, version: u32) -> PathBuf {
    snapshot_dir_in(&integration_root(), producer_id, version)
}

/// 주입한 integration root 아래 snapshot 디렉터리. fixture와 locator consumer용.
pub fn snapshot_dir_in(root: &Path, producer_id: &str, version: u32) -> PathBuf {
    root.join(producer_id).join(format!("v{version}"))
}

pub fn snapshot_path(producer_id: &str, version: u32) -> PathBuf {
    snapshot_dir(producer_id, version).join("summary.json")
}

pub fn snapshot_path_in(root: &Path, producer_id: &str, version: u32) -> PathBuf {
    snapshot_dir_in(root, producer_id, version).join("summary.json")
}

/// A validated named snapshot path in the producer/version directory. Named
/// files let a producer keep an old `summary.json` payload byte-compatible
/// while publishing a separate independently versioned named capability.
pub fn named_view_snapshot_path(
    producer_id: &str,
    version: u32,
    name: &str,
) -> Result<PathBuf, String> {
    named_view_snapshot_path_in(&integration_root(), producer_id, version, name)
}

pub fn named_view_snapshot_path_in(
    root: &Path,
    producer_id: &str,
    version: u32,
    name: &str,
) -> Result<PathBuf, String> {
    validate_producer_id(producer_id)?;
    validate_version(version)?;
    validate_kind(name)?;
    if name == "summary" {
        return Err("named snapshot kind is reserved".into());
    }
    Ok(snapshot_dir_in(root, producer_id, version).join(format!("{name}.json")))
}

/// 완성된 envelope 하나를 고유 임시 파일에서 `summary.json`으로 원자 교체한다.
pub fn write_atomic(envelope: &Envelope, dir: &Path) -> Result<(), String> {
    validate_envelope(envelope)?;
    validate_target_dir(envelope, dir)?;
    reject_owned_directory_links(dir)?;
    std::fs::create_dir_all(dir).map_err(|_| "snapshot 디렉터리를 만들 수 없습니다")?;
    reject_owned_directory_links(dir)?;

    write_snapshot_file(envelope, &dir.join("summary.json"))
}

/// Atomically write a validated named-view snapshot beside `summary.json`.
/// `name` is a kebab-case kind and becomes `<name>.json` under the producer's
/// version directory; it cannot escape that directory.
pub fn write_named_view_snapshot_atomic(
    envelope: &Envelope,
    root: &Path,
    name: &str,
) -> Result<(), String> {
    validate_envelope(envelope)?;
    validate_kind(name)?;
    validate_named_view_envelope(envelope, name)?;
    let target =
        named_view_snapshot_path_in(root, &envelope.producer, envelope.schema_version, name)?;
    let dir = snapshot_dir_in(root, &envelope.producer, envelope.schema_version);
    reject_owned_directory_links(&dir)?;
    std::fs::create_dir_all(&dir).map_err(|_| "snapshot 디렉터리를 만들 수 없습니다")?;
    reject_owned_directory_links(&dir)?;
    write_snapshot_file(envelope, &target)
}

fn write_snapshot_file(envelope: &Envelope, target: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if is_link_metadata(&metadata) => {
            return Err(
                "snapshot 경로에 symbolic link 또는 reparse point를 사용할 수 없습니다".into(),
            )
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("snapshot 경로를 확인할 수 없습니다".into()),
    }
    let json =
        serde_json::to_vec_pretty(envelope).map_err(|_| "snapshot을 직렬화할 수 없습니다")?;
    if json.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err("snapshot 크기 제한을 초과했습니다".into());
    }
    devbox_filesystem::atomic_write(target, &json)
        .map_err(|_| "snapshot을 원자적으로 기록할 수 없습니다".into())
}

/// 기본 integration root에서 snapshot을 읽는다. 파일 없음만 `Ok(None)`이다.
pub fn read_snapshot(producer_id: &str, version: u32) -> Result<Option<Envelope>, String> {
    read_snapshot_in(&integration_root(), producer_id, version)
}

/// 지정한 integration root에서 snapshot을 읽는다.
pub fn read_snapshot_in(
    root: &Path,
    producer_id: &str,
    version: u32,
) -> Result<Option<Envelope>, String> {
    validate_producer_id(producer_id)?;
    validate_version(version)?;
    reject_read_path_links(root, producer_id, version)?;
    let path = snapshot_path_in(root, producer_id, version);
    read_snapshot_file(&path, producer_id, version)
}

/// Read one named snapshot from a producer/version directory. Missing files
/// are represented by `Ok(None)` just like `read_snapshot_in`.
pub fn read_named_view_snapshot_in(
    root: &Path,
    producer_id: &str,
    version: u32,
    name: &str,
) -> Result<Option<Envelope>, String> {
    let path = named_view_snapshot_path_in(root, producer_id, version, name)?;
    reject_read_path_links(root, producer_id, version)?;
    match read_snapshot_file(&path, producer_id, version)? {
        Some(envelope) => {
            validate_named_view_envelope(&envelope, name)?;
            Ok(Some(envelope))
        }
        None => Ok(None),
    }
}

/// 기본 integration root에서 검증 가능한 모든 snapshot을 발견한다.
/// 손상 producer는 제외하되 다른 producer 결과는 유지한다.
pub fn discover() -> Vec<SnapshotRef> {
    discover_report().snapshots
}

/// UI 진단을 위해 격리된 producer 오류도 함께 돌려준다.
pub fn discover_report() -> DiscoveryReport {
    discover_report_in(&integration_root())
}

/// 지정한 integration root를 스캔한다. 테스트 fixture에도 같은 코드 경로를 사용한다.
pub fn discover_report_in(root: &Path) -> DiscoveryReport {
    let mut report = DiscoveryReport::default();
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if !metadata.file_type().is_dir() || is_link_metadata(&metadata) => {
            report.root_error = Some("integration root를 안전하게 읽을 수 없습니다".into());
            return report;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return report,
        Err(_) => {
            report.root_error = Some("integration root를 읽을 수 없습니다".into());
            return report;
        }
    }

    let producers = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => {
            report.root_error = Some("integration root를 읽을 수 없습니다".into());
            return report;
        }
    };

    let mut visited = 0usize;
    for producer_entry in producers.flatten() {
        if visited >= MAX_DISCOVERY_ENTRIES {
            break;
        }
        visited += 1;
        let Some(producer) = producer_entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if validate_producer_id(&producer).is_err() || !entry_is_plain_directory(&producer_entry) {
            continue;
        }

        let versions = match std::fs::read_dir(producer_entry.path()) {
            Ok(entries) => entries,
            Err(_) => {
                report.issues.push(SnapshotIssue {
                    producer,
                    version: None,
                    error: "producer snapshot 디렉터리를 읽을 수 없습니다".into(),
                });
                continue;
            }
        };

        for version_entry in versions.flatten() {
            if visited >= MAX_DISCOVERY_ENTRIES {
                break;
            }
            visited += 1;
            if !entry_is_plain_directory(&version_entry) {
                continue;
            }
            let Some(version) = parse_version_directory(&version_entry.file_name()) else {
                continue;
            };
            let path = version_entry.path().join("summary.json");
            match read_snapshot_file(&path, &producer, version) {
                Ok(Some(envelope)) => match snapshot_reference(&path, envelope) {
                    Ok(reference) => report.snapshots.push(reference),
                    Err(error) => report.issues.push(SnapshotIssue {
                        producer: producer.clone(),
                        version: Some(version),
                        error,
                    }),
                },
                Ok(None) => {}
                Err(error) => report.issues.push(SnapshotIssue {
                    producer: producer.clone(),
                    version: Some(version),
                    error,
                }),
            }
        }
    }

    report
        .snapshots
        .sort_by(|a, b| (&a.producer, a.version).cmp(&(&b.producer, b.version)));
    report
        .issues
        .sort_by(|a, b| (&a.producer, a.version).cmp(&(&b.producer, b.version)));
    report
}

fn read_snapshot_file(
    path: &Path,
    expected_producer: &str,
    expected_version: u32,
) -> Result<Option<Envelope>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("snapshot 파일을 읽을 수 없습니다".into()),
    };
    if !metadata.file_type().is_file() || is_link_metadata(&metadata) {
        return Err("snapshot 파일 형식이 안전하지 않습니다".into());
    }
    if metadata.len() > MAX_SNAPSHOT_BYTES {
        return Err("snapshot 크기 제한을 초과했습니다".into());
    }

    // Read from the exact no-follow handle that was authorized, then ensure
    // the path still names that same object before accepting its contents.
    // This closes the final-component replacement gap between metadata and
    // `File::open`; ancestor link changes are rejected again after the read.
    let (mut file, identity) = match devbox_filesystem::open_filesystem_object(path, false) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("snapshot 파일을 읽을 수 없습니다".into()),
    };
    let handle_metadata = file
        .metadata()
        .map_err(|_| "snapshot 파일을 읽을 수 없습니다")?;
    if handle_metadata.len() > MAX_SNAPSHOT_BYTES {
        return Err("snapshot 크기 제한을 초과했습니다".into());
    }
    let mut bytes = Vec::with_capacity(handle_metadata.len().min(MAX_SNAPSHOT_BYTES) as usize);
    file.by_ref()
        .take(MAX_SNAPSHOT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "snapshot 파일을 읽을 수 없습니다")?;
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err("snapshot 크기 제한을 초과했습니다".into());
    }
    revalidate_snapshot_path(path, identity)?;
    let envelope: Envelope =
        serde_json::from_slice(&bytes).map_err(|_| "snapshot JSON 형식이 올바르지 않습니다")?;
    validate_envelope(&envelope)?;
    if envelope.producer != expected_producer {
        return Err("snapshot producer가 경로와 일치하지 않습니다".into());
    }
    if envelope.schema_version != expected_version {
        return Err("snapshot schema version이 경로와 일치하지 않습니다".into());
    }
    Ok(Some(envelope))
}

fn revalidate_snapshot_path(
    path: &Path,
    expected_identity: devbox_filesystem::FilesystemIdentity,
) -> Result<(), String> {
    devbox_filesystem::ensure_no_links(path)
        .map_err(|_| "snapshot 파일 형식이 안전하지 않습니다")?;
    let current_identity = devbox_filesystem::filesystem_identity(path, false)
        .map_err(|_| "snapshot 파일을 읽을 수 없습니다")?;
    if current_identity != expected_identity {
        return Err("snapshot 파일이 읽는 동안 변경되었습니다".into());
    }
    Ok(())
}

fn snapshot_reference(path: &Path, envelope: Envelope) -> Result<SnapshotRef, String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "snapshot freshness를 확인할 수 없습니다")?;
    if is_link_metadata(&metadata) {
        return Err("snapshot 파일 형식이 안전하지 않습니다".into());
    }
    let modified = metadata
        .modified()
        .map_err(|_| "snapshot freshness를 확인할 수 없습니다")?;
    let freshness_ms = elapsed_ms(modified, std::time::SystemTime::now());
    let views = envelope
        .views()?
        .into_iter()
        .map(|(kind, view)| SnapshotViewRef {
            kind,
            schema_version: view.schema_version,
            freshness_ms: freshness_ms.saturating_add(view.freshness_ms),
            entry_count: view.entries.len(),
        })
        .collect();

    Ok(SnapshotRef {
        producer: envelope.producer,
        version: envelope.schema_version,
        producer_version: envelope.producer_version,
        generated_at: envelope.generated_at,
        path: path.to_path_buf(),
        freshness_ms,
        views,
    })
}

fn validate_envelope(envelope: &Envelope) -> Result<(), String> {
    validate_producer_id(&envelope.producer)?;
    validate_version(envelope.schema_version)?;
    validate_producer_version(&envelope.producer_version)?;
    validate_generated_at(&envelope.generated_at)?;
    if !envelope.data.is_object() {
        return Err("snapshot data 형식이 올바르지 않습니다".into());
    }
    validate_json_value(&envelope.data, 0)?;
    for (kind, view) in envelope.views()? {
        validate_kind(&kind)?;
        validate_version(view.schema_version)?;
        for entry in &view.entries {
            if !entry.is_object() {
                return Err("snapshot view entry 형식이 올바르지 않습니다".into());
            }
        }
    }
    Ok(())
}

fn validate_named_view_envelope(envelope: &Envelope, name: &str) -> Result<(), String> {
    let views = envelope.views()?;
    if views.len() != 1 || !views.contains_key(name) {
        return Err("named snapshot view가 파일 이름과 일치하지 않습니다".into());
    }
    Ok(())
}

fn validate_target_dir(envelope: &Envelope, dir: &Path) -> Result<(), String> {
    let expected_version = format!("v{}", envelope.schema_version);
    let version_matches =
        dir.file_name().and_then(|name| name.to_str()) == Some(expected_version.as_str());
    let producer_matches = dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some(envelope.producer.as_str());
    if !version_matches || !producer_matches {
        return Err("snapshot 기록 경로가 envelope identity와 일치하지 않습니다".into());
    }
    Ok(())
}

fn validate_producer_id(value: &str) -> Result<(), String> {
    validate_kebab_identifier(value, 64, "snapshot producer id가 올바르지 않습니다")
}

fn validate_kind(value: &str) -> Result<(), String> {
    validate_kebab_identifier(value, 64, "snapshot view kind가 올바르지 않습니다")
}

fn validate_kebab_identifier(value: &str, max: usize, message: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(message.into());
    }
    Ok(())
}

fn validate_version(version: u32) -> Result<(), String> {
    if version == 0 {
        return Err("snapshot version이 올바르지 않습니다".into());
    }
    Ok(())
}

fn validate_producer_version(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value.is_ascii()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
    {
        return Err("snapshot producer version이 올바르지 않습니다".into());
    }
    let (without_build, build) = match value.split_once('+') {
        Some((prefix, build)) if !build.is_empty() && !build.contains('+') => (prefix, Some(build)),
        Some(_) => return Err("snapshot producer version이 올바르지 않습니다".into()),
        None => (value, None),
    };
    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, prerelease)) if !prerelease.is_empty() => (core, Some(prerelease)),
        Some(_) => return Err("snapshot producer version이 올바르지 않습니다".into()),
        None => (without_build, None),
    };
    let segments: Vec<_> = core.split('.').collect();
    if segments.len() != 3
        || segments.iter().any(|segment| {
            segment.is_empty()
                || !segment.bytes().all(|b| b.is_ascii_digit())
                || (segment.len() > 1 && segment.starts_with('0'))
        })
    {
        return Err("snapshot producer version이 올바르지 않습니다".into());
    }
    if prerelease.is_some_and(|suffix| !valid_semver_suffix(suffix, false))
        || build.is_some_and(|suffix| !valid_semver_suffix(suffix, true))
    {
        return Err("snapshot producer version이 올바르지 않습니다".into());
    }
    Ok(())
}

fn valid_semver_suffix(value: &str, numeric_leading_zero_allowed: bool) -> bool {
    value.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && (numeric_leading_zero_allowed
                || !segment.bytes().all(|byte| byte.is_ascii_digit())
                || segment.len() == 1
                || !segment.starts_with('0'))
    })
}

fn validate_generated_at(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit()
        })
    {
        return Err("snapshot generatedAt이 올바르지 않습니다".into());
    }
    let number =
        |start: usize, end: usize| -> u32 { value[start..end].parse::<u32>().unwrap_or(u32::MAX) };
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let (hour, minute, second) = (number(11, 13), number(14, 16), number(17, 19));
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err("snapshot generatedAt이 올바르지 않습니다".into());
    }
    Ok(())
}

fn validate_json_value(value: &serde_json::Value, depth: usize) -> Result<(), String> {
    if depth > MAX_JSON_DEPTH {
        return Err("snapshot data 중첩 제한을 초과했습니다".into());
    }
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if forbidden_snapshot_key(key) {
                    return Err("snapshot에 민감 정보 필드를 저장할 수 없습니다".into());
                }
                validate_json_value(value, depth + 1)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_json_value(value, depth + 1)?;
            }
        }
        serde_json::Value::String(value) if looks_like_raw_credential(value) => {
            return Err("snapshot에 민감 정보 값을 저장할 수 없습니다".into());
        }
        _ => {}
    }
    Ok(())
}

fn forbidden_snapshot_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "authorization"
            | "auth"
            | "authentication"
            | "authheader"
            | "proxyauthorization"
            | "cookie"
            | "setcookie"
            | "sessioncookie"
            | "secret"
            | "secrets"
            | "password"
            | "passwords"
            | "credential"
            | "credentials"
            | "apikey"
            | "xapikey"
            | "clientsecret"
            | "privatekey"
            | "accesstoken"
            | "refreshtoken"
            | "token"
            | "environment"
            | "environmentvalue"
            | "environmentvalues"
            | "rawenvironment"
            | "rawenvironmentvalue"
            | "rawenvironmentvalues"
            | "rawenv"
            | "rawenvvalue"
            | "rawenvvalues"
            | "environmentvariable"
            | "environmentvariables"
            | "envvar"
            | "envvars"
            | "envvalue"
            | "envvalues"
    )
}

fn looks_like_raw_credential(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || trimmed.starts_with("sk-")
        || trimmed.starts_with("ghp_")
        || trimmed.starts_with("github_pat_")
}

fn parse_version_directory(name: &std::ffi::OsStr) -> Option<u32> {
    let name = name.to_str()?;
    let digits = name.strip_prefix('v')?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    let version = digits.parse().ok()?;
    validate_version(version).ok()?;
    Some(version)
}

fn entry_is_plain_directory(entry: &std::fs::DirEntry) -> bool {
    entry
        .file_type()
        .map(|kind| kind.is_dir() && !kind.is_symlink())
        .unwrap_or(false)
        && std::fs::symlink_metadata(entry.path())
            .map(|metadata| !is_link_metadata(&metadata))
            .unwrap_or(false)
}

fn reject_owned_directory_links(dir: &Path) -> Result<(), String> {
    // version, producer, integration은 devbox가 소유하는 마지막 세 디렉터리다.
    // 그 위의 사용자 profile/volume junction은 플랫폼 구성일 수 있어 검사 범위에서 뺀다.
    for path in dir.ancestors().take(3) {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if is_link_metadata(&metadata) => {
                return Err(
                    "snapshot 경로에 symbolic link 또는 reparse point를 사용할 수 없습니다".into(),
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("snapshot 경로를 확인할 수 없습니다".into()),
        }
    }
    Ok(())
}

fn reject_read_path_links(root: &Path, producer_id: &str, version: u32) -> Result<(), String> {
    for path in [
        root.to_path_buf(),
        root.join(producer_id),
        snapshot_dir_in(root, producer_id, version),
    ] {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if is_link_metadata(&metadata) => {
                return Err(
                    "snapshot 경로에 symbolic link 또는 reparse point를 사용할 수 없습니다".into(),
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err("snapshot 경로를 확인할 수 없습니다".into()),
        }
    }
    Ok(())
}

fn is_link_metadata(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn elapsed_ms(then: std::time::SystemTime, now: std::time::SystemTime) -> u64 {
    now.duration_since(then)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// ISO-8601 UTC (fractional seconds omitted).
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_ROOT: AtomicUsize = AtomicUsize::new(0);

    fn test_root(label: &str) -> PathBuf {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "devbox-integration-{label}-{}-{id}",
            std::process::id()
        ))
    }

    fn envelope(producer: &str, value: u64) -> Envelope {
        Envelope::new(producer, "0.5.0", serde_json::json!({ "valueId": value }))
    }

    fn write_to_root(root: &Path, envelope: &Envelope) {
        let dir = snapshot_dir_in(root, &envelope.producer, envelope.schema_version);
        write_atomic(envelope, &dir).unwrap();
    }

    #[test]
    fn path_uses_common_root() {
        let path = snapshot_path("run-manager", 1)
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            path.ends_with("/devbox/integration/run-manager/v1/summary.json"),
            "{path}"
        );
    }

    #[test]
    fn named_view_snapshot_round_trips_with_identity_and_is_ignored_by_discovery() {
        let root = test_root("named");
        let views = SnapshotViews::from([(
            "jobs-services".to_owned(),
            SnapshotView {
                schema_version: 1,
                freshness_ms: 0,
                entries: vec![serde_json::json!({ "id": "job-1" })],
            },
        )]);
        let envelope = Envelope::with_views("run-manager", "0.5.0", views);
        write_named_view_snapshot_atomic(&envelope, &root, "jobs-services").unwrap();

        let path = named_view_snapshot_path_in(&root, "run-manager", 1, "jobs-services").unwrap();
        assert!(path.ends_with("run-manager/v1/jobs-services.json"));
        let read = read_named_view_snapshot_in(&root, "run-manager", 1, "jobs-services")
            .unwrap()
            .unwrap();
        assert_eq!(read, envelope);
        assert!(discover_report_in(&root).snapshots.is_empty());

        let legacy = Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({
                "activeServices": [],
                "runs": {"success": 0, "failed": 0},
                "lastRunAtMs": null,
            }),
        );
        write_atomic(&legacy, &snapshot_dir_in(&root, "run-manager", 1)).unwrap();
        let summary_before = std::fs::read(snapshot_path_in(&root, "run-manager", 1)).unwrap();
        assert!(write_named_view_snapshot_atomic(&envelope, &root, "summary").is_err());
        assert_eq!(
            std::fs::read(snapshot_path_in(&root, "run-manager", 1)).unwrap(),
            summary_before
        );

        let mismatched = Envelope::with_views(
            "run-manager",
            "0.5.0",
            SnapshotViews::from([(
                "status".to_owned(),
                SnapshotView {
                    schema_version: 1,
                    freshness_ms: 0,
                    entries: vec![serde_json::json!({ "id": "status" })],
                },
            )]),
        );
        assert!(write_named_view_snapshot_atomic(&mismatched, &root, "jobs-services").is_err());

        let extra = Envelope::with_views(
            "run-manager",
            "0.5.0",
            SnapshotViews::from([
                (
                    "jobs-services".to_owned(),
                    SnapshotView {
                        schema_version: 1,
                        freshness_ms: 0,
                        entries: vec![],
                    },
                ),
                (
                    "status".to_owned(),
                    SnapshotView {
                        schema_version: 1,
                        freshness_ms: 0,
                        entries: vec![],
                    },
                ),
            ]),
        );
        assert!(write_named_view_snapshot_atomic(&extra, &root, "jobs-services").is_err());

        for name in [
            "../escape",
            "jobs_services",
            "",
            "jobs-services.json",
            "summary",
        ] {
            assert!(named_view_snapshot_path_in(&root, "run-manager", 1, name).is_err());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn named_view_snapshot_read_rejects_identity_and_shape_mismatches() {
        let root = test_root("named-read-contract");
        let path = named_view_snapshot_path_in(&root, "run-manager", 1, "jobs-services").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let jobs_view = || {
            SnapshotViews::from([(
                "jobs-services".to_owned(),
                SnapshotView {
                    schema_version: 1,
                    freshness_ms: 0,
                    entries: vec![],
                },
            )])
        };

        let wrong_producer = Envelope::with_views("knowledge-base", "0.5.0", jobs_view());
        std::fs::write(&path, serde_json::to_vec(&wrong_producer).unwrap()).unwrap();
        assert_eq!(
            read_named_view_snapshot_in(&root, "run-manager", 1, "jobs-services").unwrap_err(),
            "snapshot producer가 경로와 일치하지 않습니다"
        );

        let wrong_view = Envelope::with_views(
            "run-manager",
            "0.5.0",
            SnapshotViews::from([(
                "status".to_owned(),
                SnapshotView {
                    schema_version: 1,
                    freshness_ms: 0,
                    entries: vec![],
                },
            )]),
        );
        std::fs::write(&path, serde_json::to_vec(&wrong_view).unwrap()).unwrap();
        assert!(
            read_named_view_snapshot_in(&root, "run-manager", 1, "jobs-services")
                .unwrap_err()
                .contains("파일 이름")
        );

        let extra_views = Envelope::with_views(
            "run-manager",
            "0.5.0",
            SnapshotViews::from([
                (
                    "jobs-services".to_owned(),
                    SnapshotView {
                        schema_version: 1,
                        freshness_ms: 0,
                        entries: vec![],
                    },
                ),
                (
                    "status".to_owned(),
                    SnapshotView {
                        schema_version: 1,
                        freshness_ms: 0,
                        entries: vec![],
                    },
                ),
            ]),
        );
        std::fs::write(&path, serde_json::to_vec(&extra_views).unwrap()).unwrap();
        assert!(
            read_named_view_snapshot_in(&root, "run-manager", 1, "jobs-services")
                .unwrap_err()
                .contains("파일 이름")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_read_identity_revalidation_rejects_atomic_replacement() {
        let root = test_root("read-replacement");
        let path = snapshot_path_in(&root, "run-manager", 1);
        let displaced = path.with_file_name("displaced.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        devbox_filesystem::atomic_write(&path, b"first").unwrap();
        let (_opened, identity) = devbox_filesystem::open_filesystem_object(&path, false).unwrap();

        // Windows does not replace an open destination in-place consistently,
        // even when the reader shares delete access. Move the authorized
        // object aside first, then create a different object at the same path;
        // this is the cross-platform replacement race the reader must reject.
        std::fs::rename(&path, &displaced).unwrap();
        devbox_filesystem::atomic_write(&path, b"replacement").unwrap();

        assert_eq!(
            revalidate_snapshot_path(&path, identity).unwrap_err(),
            "snapshot 파일이 읽는 동안 변경되었습니다"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn named_view_snapshot_rejects_symlink_targets() {
        use std::os::unix::fs::symlink;

        let root = test_root("named-link");
        let outside = test_root("named-link-outside");
        std::fs::create_dir_all(snapshot_dir_in(&root, "run-manager", 1)).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(
            outside.join("jobs-services.json"),
            named_view_snapshot_path_in(&root, "run-manager", 1, "jobs-services").unwrap(),
        )
        .unwrap();
        let envelope = Envelope::with_views(
            "run-manager",
            "0.5.0",
            SnapshotViews::from([(
                "jobs-services".to_owned(),
                SnapshotView {
                    schema_version: 1,
                    freshness_ms: 0,
                    entries: vec![],
                },
            )]),
        );
        assert!(
            write_named_view_snapshot_atomic(&envelope, &root, "jobs-services")
                .unwrap_err()
                .contains("symbolic link")
        );
        assert!(
            read_named_view_snapshot_in(&root, "run-manager", 1, "jobs-services")
                .unwrap_err()
                .contains("안전하지")
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn discovers_zero_one_and_many_snapshots_in_stable_order() {
        let root = test_root("discovery");
        assert!(discover_report_in(&root).snapshots.is_empty());

        write_to_root(&root, &envelope("run-manager", 1));
        let one = discover_report_in(&root);
        assert_eq!(one.snapshots.len(), 1);
        assert_eq!(one.snapshots[0].producer, "run-manager");

        write_to_root(&root, &envelope("knowledge-base", 2));
        let many = discover_report_in(&root);
        assert_eq!(many.snapshots.len(), 2);
        assert_eq!(many.snapshots[0].producer, "knowledge-base");
        assert_eq!(many.snapshots[1].producer, "run-manager");
        assert!(many.issues.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_producer_does_not_hide_valid_producer() {
        let root = test_root("corrupt");
        write_to_root(&root, &envelope("knowledge-base", 1));
        let bad_dir = snapshot_dir_in(&root, "run-manager", 1);
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(bad_dir.join("summary.json"), b"{credential: raw-secret}").unwrap();

        let report = discover_report_in(&root);
        assert_eq!(report.snapshots.len(), 1);
        assert_eq!(report.snapshots[0].producer, "knowledge-base");
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].producer, "run-manager");
        assert!(!report.issues[0].error.contains("raw-secret"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn multi_view_snapshot_replaces_the_complete_previous_set() {
        let root = test_root("views");
        let mut first = SnapshotViews::new();
        first.insert(
            "profiles".into(),
            SnapshotView {
                schema_version: 1,
                freshness_ms: 20,
                entries: vec![serde_json::json!({ "id": "profile-1" })],
            },
        );
        first.insert(
            "runtime".into(),
            SnapshotView {
                schema_version: 2,
                freshness_ms: 40,
                entries: vec![serde_json::json!({ "id": "runtime-1" })],
            },
        );
        write_to_root(&root, &Envelope::with_views("wsl-desktop", "0.5.0", first));

        let mut replacement = SnapshotViews::new();
        replacement.insert(
            "runtime".into(),
            SnapshotView {
                schema_version: 2,
                freshness_ms: 5,
                entries: vec![serde_json::json!({ "id": "runtime-2" })],
            },
        );
        write_to_root(
            &root,
            &Envelope::with_views("wsl-desktop", "0.5.0", replacement),
        );

        let read = read_snapshot_in(&root, "wsl-desktop", 1).unwrap().unwrap();
        let views = read.views().unwrap();
        assert_eq!(views.keys().collect::<Vec<_>>(), vec!["runtime"]);
        assert_eq!(views["runtime"].entries[0]["id"], "runtime-2");
        let files = std::fs::read_dir(snapshot_dir_in(&root, "wsl-desktop", 1))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(files, vec!["summary.json"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_complete_writers_never_expose_partial_json_or_temp_files() {
        let root = test_root("concurrent");
        let dir = snapshot_dir_in(&root, "run-manager", 1);
        std::fs::create_dir_all(&dir).unwrap();
        let mut writers = Vec::new();
        for writer in 0..4u64 {
            let dir = dir.clone();
            writers.push(std::thread::spawn(move || {
                for sequence in 0..8u64 {
                    let id = writer * 100 + sequence;
                    let mut views = SnapshotViews::new();
                    views.insert(
                        "jobs-services".into(),
                        SnapshotView {
                            schema_version: 1,
                            freshness_ms: 0,
                            entries: vec![serde_json::json!({ "id": format!("job-{id}") })],
                        },
                    );
                    views.insert(
                        "status".into(),
                        SnapshotView {
                            schema_version: 1,
                            freshness_ms: 0,
                            entries: vec![serde_json::json!({ "id": format!("status-{id}") })],
                        },
                    );
                    write_atomic(&Envelope::with_views("run-manager", "0.5.0", views), &dir)
                        .unwrap();
                }
            }));
        }
        for writer in writers {
            writer.join().unwrap();
        }

        let final_envelope = read_snapshot_in(&root, "run-manager", 1).unwrap().unwrap();
        let final_views = final_envelope.views().unwrap();
        assert_eq!(
            final_views.keys().collect::<Vec<_>>(),
            vec!["jobs-services", "status"]
        );
        assert_eq!(final_views["jobs-services"].entries.len(), 1);
        assert_eq!(final_views["status"].entries.len(), 1);
        let names = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["summary.json"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn freshness_combines_view_age_with_file_age_without_future_underflow() {
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10);
        let then = std::time::UNIX_EPOCH + std::time::Duration::from_millis(8_500);
        assert_eq!(elapsed_ms(then, now), 1_500);
        assert_eq!(elapsed_ms(now, then), 0);

        let root = test_root("freshness");
        let mut views = SnapshotViews::new();
        views.insert(
            "status".into(),
            SnapshotView {
                schema_version: 1,
                freshness_ms: 250,
                entries: vec![],
            },
        );
        write_to_root(&root, &Envelope::with_views("run-manager", "0.5.0", views));
        let discovered = discover_report_in(&root).snapshots.remove(0);
        assert_eq!(
            discovered.views[0].freshness_ms,
            discovered.freshness_ms + 250
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keeps_legacy_flat_data_compatible() {
        let root = test_root("legacy");
        write_to_root(&root, &envelope("knowledge-base", 7));
        let reference = discover_report_in(&root).snapshots.remove(0);
        assert!(reference.views.is_empty());
        let read = read_snapshot_in(&root, "knowledge-base", 1)
            .unwrap()
            .unwrap();
        assert_eq!(read.data["valueId"], 7);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepts_unknown_view_metadata_for_version_forward_compatibility() {
        let envelope: Envelope = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "producer": "run-manager",
            "producerVersion": "0.5.0",
            "generatedAt": "2026-08-25T12:00:00Z",
            "data": {
                "views": {
                    "status": {
                        "schemaVersion": 2,
                        "freshnessMs": 0,
                        "entries": [{ "id": "job-1" }],
                        "futureMetadata": { "label": "safe" }
                    }
                }
            }
        }))
        .unwrap();
        validate_envelope(&envelope).unwrap();
        let views = envelope.views().unwrap();
        assert_eq!(views["status"].schema_version, 2);
        assert_eq!(views["status"].entries.len(), 1);
    }

    #[test]
    fn rejects_sensitive_fields_and_does_not_echo_values() {
        let root = test_root("secret");
        let secret = "do-not-echo-raw-credential";
        let unsafe_envelope = Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({ "Authorization": secret }),
        );
        let error =
            write_atomic(&unsafe_envelope, &snapshot_dir_in(&root, "run-manager", 1)).unwrap_err();
        assert!(error.contains("민감 정보"));
        assert!(!error.contains(secret));
        assert!(!root.exists());

        let credential_value = Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({ "header": "Bearer raw-token-value" }),
        );
        let error =
            write_atomic(&credential_value, &snapshot_dir_in(&root, "run-manager", 1)).unwrap_err();
        assert!(!error.contains("raw-token-value"));

        let environment_value = Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({ "rawEnvValues": { "DATABASE_URL": "private" } }),
        );
        let error = write_atomic(
            &environment_value,
            &snapshot_dir_in(&root, "run-manager", 1),
        )
        .unwrap_err();
        assert!(error.contains("민감 정보"));
    }

    #[test]
    fn rejects_unsafe_identity_before_path_access_and_hides_mismatch_values() {
        let root = test_root("identity");
        let error = read_snapshot_in(&root, "../escape", 1).unwrap_err();
        assert!(error.contains("producer id"));
        assert!(!root.exists());

        let dir = snapshot_dir_in(&root, "run-manager", 1);
        std::fs::create_dir_all(&dir).unwrap();
        let mismatched = envelope("knowledge-base", 1);
        std::fs::write(
            dir.join("summary.json"),
            serde_json::to_vec(&mismatched).unwrap(),
        )
        .unwrap();
        let error = read_snapshot_in(&root, "run-manager", 1).unwrap_err();
        assert_eq!(error, "snapshot producer가 경로와 일치하지 않습니다");
        assert!(!error.contains("knowledge-base"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn writer_rejects_a_directory_owned_by_another_envelope() {
        let root = test_root("writer-identity");
        let error = write_atomic(
            &envelope("run-manager", 1),
            &snapshot_dir_in(&root, "knowledge-base", 1),
        )
        .unwrap_err();
        assert!(error.contains("identity"));
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn discovery_does_not_follow_symbolic_link_producers() {
        use std::os::unix::fs::symlink;

        let root = test_root("links");
        let outside = test_root("outside");
        write_to_root(&outside, &envelope("run-manager", 1));
        std::fs::create_dir_all(&root).unwrap();
        symlink(outside.join("run-manager"), root.join("run-manager")).unwrap();

        let report = discover_report_in(&root);
        assert!(report.snapshots.is_empty());
        assert!(report.issues.is_empty());
        let error = read_snapshot_in(&root, "run-manager", 1).unwrap_err();
        assert!(error.contains("symbolic link"));

        let write_error = write_atomic(
            &envelope("run-manager", 2),
            &snapshot_dir_in(&root, "run-manager", 1),
        )
        .unwrap_err();
        assert!(write_error.contains("symbolic link"));
        assert_eq!(
            read_snapshot_in(&outside, "run-manager", 1)
                .unwrap()
                .unwrap()
                .data["valueId"],
            1
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn validates_generated_time_and_semver_like_producer_version() {
        assert!(validate_generated_at("2024-02-29T23:59:59Z").is_ok());
        assert!(validate_generated_at("2026-02-29T00:00:00Z").is_err());
        assert!(validate_generated_at("not-a-timestamp").is_err());
        assert!(validate_producer_version("0.5.0").is_ok());
        assert!(validate_producer_version("1.2.3-beta.1+win").is_ok());
        assert!(validate_producer_version("01.2.3").is_err());
        assert!(validate_producer_version("1.2.3-").is_err());
        assert!(validate_producer_version("1.2.3-01").is_err());
        assert!(validate_producer_version("1.2.3+").is_err());
        assert!(validate_producer_version("1").is_err());
    }

    #[test]
    fn opaque_identity_is_stable_namespaced_and_hides_source() {
        let source = "win:c:/users/example/projects/private-repository";
        let first = opaque_identity("project", source).unwrap();
        let second = opaque_identity("project", source).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("project-"));
        assert_eq!(first.len(), "project-".len() + 64);
        assert!(!first.contains("private-repository"));
        assert_ne!(first, opaque_identity("repository", source).unwrap());
        assert!(opaque_identity("Project", source).is_err());
        assert!(opaque_identity("project", "bad\nsource").is_err());
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        let secs: i64 = 1_783_000_000;
        let (year, month, _day) = civil_from_days(secs.div_euclid(86_400));
        assert_eq!((year, month), (2026, 7));
    }
}
