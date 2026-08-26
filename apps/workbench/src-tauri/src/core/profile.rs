//! ProjectProfile 저장 (순수 로직). Workbench가 단일 writer다.
//!
//! canonical identity는 `crates/wsl`(devbox_wsl)의 `canonical_project_key` 단일 규칙을
//! 쓴다 — Windows 표기와 `/mnt/` 표기가 같은 프로젝트로 식별된다 (§10.2, PR 28).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROFILE_VERSION: u32 = 1;

pub const MAX_PROFILE_NAME_CHARS: usize = 120;
pub const MAX_PROFILE_ID_CHARS: usize = 128;
pub const MAX_SERVICE_ID_CHARS: usize = 128;
pub const MAX_EXPECTED_PORTS: usize = 128;
pub const MAX_SERVICES: usize = 128;
pub const MAX_PROFILES: usize = 512;
pub const MAX_PROFILE_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_WSL_DISTRO_CHARS: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ProjectProfile {
    pub id: String,
    pub name: String,
    pub windows_path: Option<String>,
    pub wsl: Option<WslProfile>,
    pub git_root: Option<String>,
    #[serde(default)]
    pub expected_ports: Vec<u16>,
    #[serde(default)]
    pub run_manager_service_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct WslProfile {
    pub distro: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ProfileStore {
    pub version: u32,
    pub profiles: Vec<ProjectProfile>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self::empty()
    }
}

impl ProfileStore {
    pub fn empty() -> Self {
        Self {
            version: PROFILE_VERSION,
            profiles: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serialize only a store which has passed the same checks used on load.
    /// This prevents an invalid in-memory value from replacing a valid file.
    pub fn to_json_checked(&self) -> Result<String, String> {
        self.validate()?;
        let json = self
            .to_json()
            .map_err(|_| "프로필 저장소를 직렬화할 수 없습니다".to_string())?;
        if json.len() > MAX_PROFILE_FILE_BYTES {
            return Err("프로필 저장소 크기 제한을 초과했습니다".into());
        }
        Ok(json)
    }

    /// Parse a complete, bounded, validated store.
    ///
    /// Missing files are handled by the command layer. An input string is
    /// never silently converted to an empty store: callers must decide how to
    /// handle this error without overwriting the original bytes.
    pub fn load(input: &str) -> Result<Self, String> {
        if input.len() > MAX_PROFILE_FILE_BYTES {
            return Err("프로필 저장소 크기 제한을 초과했습니다".into());
        }
        let store = serde_json::from_str::<ProfileStore>(input)
            .map_err(|_| "프로필 저장소 형식이 올바르지 않습니다".to_string())?;
        store.validate()?;
        Ok(store)
    }

    /// Validate the storage envelope and every profile before it is read or
    /// written. All failures are stable user-facing messages and never echo
    /// paths, credentials, or arbitrary service metadata.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != PROFILE_VERSION {
            return Err("프로필 저장소 버전을 지원하지 않습니다".into());
        }
        if self.profiles.len() > MAX_PROFILES {
            return Err("프로필이 너무 많습니다".into());
        }

        let mut ids = std::collections::HashSet::new();
        let mut identities = std::collections::HashSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(profile.id.as_str()) {
                return Err("같은 프로필 ID를 두 번 사용할 수 없습니다".into());
            }
            let identity = profile
                .canonical_key()
                .map_err(|_| "프로젝트 경로가 올바르지 않습니다".to_string())?;
            if !identities.insert(identity) {
                return Err("같은 프로젝트를 두 번 등록할 수 없습니다".into());
            }
        }
        Ok(())
    }

    /// 등록 (canonical key로 중복 방지). 같은 key가 있으면 기존 프로필을 반환한다.
    pub fn upsert(&mut self, profile: ProjectProfile) -> Result<Option<ProjectProfile>, String> {
        self.validate()?;
        profile.validate()?;
        let key = profile
            .canonical_key()
            .map_err(|_| "프로젝트 경로가 올바르지 않습니다".to_string())?;
        if let Some(existing) = self
            .profiles
            .iter()
            .find(|candidate| candidate.id == profile.id)
        {
            if existing.canonical_key().ok().as_deref() == Some(key.as_str()) {
                return Ok(Some(existing.clone()));
            }
            return Err("같은 프로필 ID를 두 번 사용할 수 없습니다".into());
        }
        if let Some(existing) = self
            .profiles
            .iter()
            .find(|p| p.canonical_key().ok().as_deref() == Some(key.as_str()))
        {
            return Ok(Some(existing.clone()));
        }
        let mut next = self.clone();
        next.profiles.push(profile);
        next.validate()?;
        *self = next;
        Ok(None)
    }

    /// Replace one existing profile only after validating the complete next
    /// store. In particular, a canonical-key collision leaves the old profile
    /// in place instead of deleting it before the collision is reported.
    pub fn replace(&mut self, profile: ProjectProfile) -> Result<(), String> {
        self.validate()?;
        profile.validate()?;
        let index = self
            .profiles
            .iter()
            .position(|candidate| candidate.id == profile.id)
            .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;
        let key = profile
            .canonical_key()
            .map_err(|_| "프로젝트 경로가 올바르지 않습니다".to_string())?;
        if self
            .profiles
            .iter()
            .enumerate()
            .any(|(candidate_index, candidate)| {
                candidate_index != index
                    && candidate.canonical_key().ok().as_deref() == Some(key.as_str())
            })
        {
            return Err("같은 프로젝트를 두 번 등록할 수 없습니다".into());
        }

        let mut next = self.clone();
        next.profiles[index] = profile;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        self.profiles.len() != before
    }
}

impl ProjectProfile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            windows_path: None,
            wsl: None,
            git_root: None,
            expected_ports: Vec::new(),
            run_manager_service_ids: Vec::new(),
        }
    }

    /// canonical identity — crates/wsl 단일 규칙.
    pub fn canonical_key(&self) -> Result<String, String> {
        let wsl = self
            .wsl
            .as_ref()
            .map(|w| (w.distro.as_str(), w.path.as_str()));
        devbox_wsl::path::canonical_project_key(self.windows_path.as_deref(), wsl)
            .map_err(|_| "프로젝트 경로 identity를 계산할 수 없습니다".to_string())
    }

    /// Validate data at the IPC/storage boundary as well as in the editor.
    /// The returned messages are stable and never include user-provided paths
    /// or service identifiers.
    pub fn validate(&self) -> Result<(), String> {
        validate_profile_id(&self.id)?;
        let name = self.name.trim();
        if name.is_empty() {
            return Err("프로필 이름이 필요합니다".into());
        }
        if has_control_character(name) {
            return Err("프로필 이름이 올바르지 않습니다".into());
        }
        if name.chars().count() > MAX_PROFILE_NAME_CHARS {
            return Err("프로필 이름이 너무 깁니다".into());
        }
        if self.name != name {
            return Err("프로필 이름이 올바르지 않습니다".into());
        }

        let windows_path = self.windows_path.as_deref().unwrap_or_default().trim();
        let wsl = self.wsl.as_ref();
        if self
            .windows_path
            .as_deref()
            .is_some_and(|path| path.trim().is_empty() || path != path.trim())
        {
            return Err("프로젝트 경로가 올바르지 않습니다".into());
        }
        if self
            .git_root
            .as_deref()
            .is_some_and(|path| path.trim().is_empty() || path != path.trim())
        {
            return Err("Git root 경로가 올바르지 않습니다".into());
        }
        if windows_path.is_empty() && wsl.is_none() {
            return Err("프로젝트 경로가 필요합니다".into());
        }
        if [
            self.windows_path.as_deref().unwrap_or_default(),
            self.git_root.as_deref().unwrap_or_default(),
        ]
        .into_iter()
        .chain(
            wsl.into_iter()
                .flat_map(|profile| [profile.distro.as_str(), profile.path.as_str()]),
        )
        .any(has_control_character)
        {
            return Err("프로젝트 입력에 허용되지 않는 문자가 있습니다".into());
        }
        if let Some(wsl) = wsl {
            if wsl.distro.trim().is_empty()
                || wsl.path.trim().is_empty()
                || wsl.distro != wsl.distro.trim()
                || wsl.path != wsl.path.trim()
            {
                return Err("WSL distro와 경로가 모두 필요합니다".into());
            }
            if wsl.distro.chars().count() > MAX_WSL_DISTRO_CHARS {
                return Err("WSL distro 이름이 너무 깁니다".into());
            }
            if devbox_wsl::distro::validate_distro_name(&wsl.distro).is_err() {
                return Err("WSL distro 이름이 올바르지 않습니다".into());
            }
            if devbox_filesystem::parse_safe_project_path(wsl.path.trim()).is_none() {
                return Err("프로젝트 경로가 올바르지 않습니다".into());
            }
        }
        if !windows_path.is_empty()
            && devbox_filesystem::parse_safe_project_path(windows_path).is_none()
        {
            return Err("프로젝트 경로가 올바르지 않습니다".into());
        }
        if let Some(git_root) = self
            .git_root
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            if devbox_filesystem::parse_safe_project_path(git_root).is_none() {
                return Err("Git root 경로가 올바르지 않습니다".into());
            }
        }
        if self.expected_ports.len() > MAX_EXPECTED_PORTS {
            return Err("예상 포트가 너무 많습니다".into());
        }
        if self.expected_ports.contains(&0) {
            return Err("포트는 1부터 시작해야 합니다".into());
        }
        let mut ports = std::collections::HashSet::new();
        if self.expected_ports.iter().any(|port| !ports.insert(*port)) {
            return Err("같은 포트를 두 번 등록할 수 없습니다".into());
        }
        if self.run_manager_service_ids.len() > MAX_SERVICES {
            return Err("서비스가 너무 많습니다".into());
        }
        let mut service_ids = std::collections::HashSet::new();
        for id in &self.run_manager_service_ids {
            validate_service_id(id)?;
            if !service_ids.insert(id.as_str()) {
                return Err("같은 서비스 ID를 두 번 등록할 수 없습니다".into());
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_service_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty()
        || id != id.trim()
        || id.chars().count() > MAX_SERVICE_ID_CHARS
        || has_control_character(id)
    {
        return Err("서비스 ID가 올바르지 않습니다".into());
    }
    Ok(())
}

pub fn validate_profile_id(id: &str) -> Result<(), String> {
    if id.trim().is_empty()
        || id != id.trim()
        || id.chars().count() > MAX_PROFILE_ID_CHARS
        || has_control_character(id)
    {
        return Err("프로필 ID가 올바르지 않습니다".into());
    }
    Ok(())
}

fn has_control_character(value: &str) -> bool {
    value.chars().any(|character| character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, win: Option<&str>, wsl: Option<(&str, &str)>) -> ProjectProfile {
        let mut p = ProjectProfile::new(name);
        p.windows_path = win.map(|s| s.to_string());
        p.wsl = wsl.map(|(d, path)| WslProfile {
            distro: d.into(),
            path: path.into(),
        });
        p
    }

    #[test]
    fn upsert_dedups_by_canonical_key() {
        let mut store = ProfileStore::empty();
        let a = profile("A", Some("E:\\projects\\devbox"), None);
        let b = profile("B", None, Some(("Ubuntu", "/mnt/e/projects/devbox")));
        assert!(store.upsert(a.clone()).unwrap().is_none());
        // B는 A와 같은 canonical key → 중복으로 거부, 기존 반환
        let dup = store.upsert(b).unwrap().unwrap();
        assert_eq!(dup.id, a.id);
        assert_eq!(store.profiles.len(), 1);
    }

    #[test]
    fn remove_drops_profile() {
        let mut store = ProfileStore::empty();
        let a = profile("A", Some("C:/proj"), None);
        store.upsert(a).unwrap();
        let id = store.profiles[0].id.clone();
        assert!(store.remove(&id));
        assert!(store.profiles.is_empty());
    }

    #[test]
    fn load_corrupt_fails_closed_without_an_empty_replacement() {
        let error = ProfileStore::load("not json").unwrap_err();
        assert_eq!(error, "프로필 저장소 형식이 올바르지 않습니다");
    }

    #[test]
    fn load_rejects_unsupported_version_and_oversized_input() {
        let unsupported = r#"{"version":2,"profiles":[]}"#.to_string();
        assert_eq!(
            ProfileStore::load(&unsupported).unwrap_err(),
            "프로필 저장소 버전을 지원하지 않습니다"
        );
        let oversized = " ".repeat(MAX_PROFILE_FILE_BYTES + 1);
        assert_eq!(
            ProfileStore::load(&oversized).unwrap_err(),
            "프로필 저장소 크기 제한을 초과했습니다"
        );
    }

    #[test]
    fn load_rejects_unknown_sensitive_fields_without_echoing_them() {
        let raw = r#"{"version":1,"profiles":[],"credential":"TOP_SECRET"}"#;
        let error = ProfileStore::load(raw).unwrap_err();
        assert_eq!(error, "프로필 저장소 형식이 올바르지 않습니다");
        assert!(!error.contains("TOP_SECRET"));
    }

    #[test]
    fn replace_collision_preserves_the_original_profile() {
        let mut store = ProfileStore::empty();
        let first = profile("first", Some("C:/first"), None);
        let second = profile("second", Some("C:/second"), None);
        store.upsert(first.clone()).unwrap();
        store.upsert(second.clone()).unwrap();

        let mut conflicting = first.clone();
        conflicting.name = "renamed".into();
        conflicting.windows_path = second.windows_path.clone();

        assert_eq!(
            store.replace(conflicting).unwrap_err(),
            "같은 프로젝트를 두 번 등록할 수 없습니다"
        );
        assert_eq!(store.profiles, vec![first, second]);
    }

    #[test]
    fn store_validation_rejects_duplicate_ids_and_profile_limits() {
        let first = profile("first", Some("C:/first"), None);
        let mut duplicate = profile("second", Some("C:/second"), None);
        duplicate.id = first.id.clone();
        let store = ProfileStore {
            version: PROFILE_VERSION,
            profiles: vec![first, duplicate],
        };
        assert_eq!(
            store.validate().unwrap_err(),
            "같은 프로필 ID를 두 번 사용할 수 없습니다"
        );

        let oversized = ProfileStore {
            version: PROFILE_VERSION,
            profiles: (0..=MAX_PROFILES)
                .map(|index| {
                    profile(
                        &format!("profile-{index}"),
                        Some(&format!("C:/p-{index}")),
                        None,
                    )
                })
                .collect(),
        };
        assert_eq!(oversized.validate().unwrap_err(), "프로필이 너무 많습니다");
    }

    #[test]
    fn upsert_rejects_crossed_id_and_identity_collisions() {
        let mut store = ProfileStore::empty();
        let first = profile("first", Some("C:/first"), None);
        let second = profile("second", Some("C:/second"), None);
        store.upsert(first.clone()).unwrap();
        store.upsert(second.clone()).unwrap();

        let mut crossed = profile("crossed", Some("C:/second"), None);
        crossed.id = first.id.clone();
        assert_eq!(
            store.upsert(crossed).unwrap_err(),
            "같은 프로필 ID를 두 번 사용할 수 없습니다"
        );
        assert_eq!(store.profiles, vec![first, second]);
    }

    #[test]
    fn profile_validation_rejects_bounds_without_echoing_values() {
        let mut port_profile = profile("x", Some("C:/project"), None);
        port_profile.expected_ports = vec![3000; MAX_EXPECTED_PORTS + 1];
        assert_eq!(
            port_profile.validate().unwrap_err(),
            "예상 포트가 너무 많습니다"
        );

        let mut invalid_id_profile = profile("x", Some("C:/project"), None);
        invalid_id_profile.id = " bad-id".into();
        assert_eq!(
            invalid_id_profile.validate().unwrap_err(),
            "프로필 ID가 올바르지 않습니다"
        );

        let mut distro_profile = profile("x", Some("C:/project"), None);
        distro_profile.wsl = Some(WslProfile {
            distro: "d".repeat(MAX_WSL_DISTRO_CHARS + 1),
            path: "/mnt/c/project".into(),
        });
        let error = distro_profile.validate().unwrap_err();
        assert_eq!(error, "WSL distro 이름이 너무 깁니다");
        assert!(!error.contains("ddd"));
    }

    #[test]
    fn profile_validation_requires_normalized_fields_and_safe_distro() {
        let mut whitespace = profile(" x", Some("C:/project"), None);
        assert_eq!(
            whitespace.validate().unwrap_err(),
            "프로필 이름이 올바르지 않습니다"
        );

        whitespace.name = "x".into();
        whitespace.windows_path = Some(" C:/project".into());
        assert_eq!(
            whitespace.validate().unwrap_err(),
            "프로젝트 경로가 올바르지 않습니다"
        );

        let unsafe_distro = profile("x", None, Some(("Ubuntu;unexpected", "/mnt/c/project")));
        let error = unsafe_distro.validate().unwrap_err();
        assert_eq!(error, "WSL distro 이름이 올바르지 않습니다");
        assert!(!error.contains("unexpected"));
    }

    #[test]
    fn canonical_key_roundtrip() {
        let p = profile("x", Some("E:\\a\\b"), None);
        assert!(p.canonical_key().is_ok());
    }

    #[test]
    fn validation_rejects_invalid_ports_and_duplicate_services() {
        let mut p = profile("x", Some("E:\\a\\b"), None);
        p.expected_ports = vec![0];
        assert_eq!(p.validate().unwrap_err(), "포트는 1부터 시작해야 합니다");

        p.expected_ports = vec![3000];
        p.run_manager_service_ids = vec!["dev".into(), "dev".into()];
        assert_eq!(
            p.validate().unwrap_err(),
            "같은 서비스 ID를 두 번 등록할 수 없습니다"
        );
    }

    #[test]
    fn validation_rejects_partial_wsl_profile_without_echoing_path() {
        let p = profile("x", None, Some(("", "/secret/project")));
        assert_eq!(
            p.validate().unwrap_err(),
            "WSL distro와 경로가 모두 필요합니다"
        );
    }

    #[test]
    fn validation_rejects_device_path_without_echoing_it() {
        let p = profile("x", Some(r"\\?\C:\secret\project"), None);
        assert_eq!(
            p.validate().unwrap_err(),
            "프로젝트 경로가 올바르지 않습니다"
        );
    }

    #[test]
    fn validation_accepts_clean_service_and_port_references() {
        let mut p = profile("x", Some("E:\\a\\b"), None);
        p.expected_ports = vec![3000, 5173];
        p.run_manager_service_ids = vec!["devbox-dev".into()];
        assert!(p.validate().is_ok());
    }
}
