//! ProjectProfile 저장 (순수 로직). Workbench가 단일 writer다.
//!
//! canonical identity는 `crates/wsl`(devbox_wsl)의 `canonical_project_key` 단일 규칙을
//! 쓴다 — Windows 표기와 `/mnt/` 표기가 같은 프로젝트로 식별된다 (§10.2, PR 28).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProfile {
    pub id: String,
    pub name: String,
    pub windows_path: Option<String>,
    pub wsl: Option<WslProfile>,
    pub git_root: Option<String>,
    pub expected_ports: Vec<u16>,
    pub run_manager_service_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WslProfile {
    pub distro: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

    pub fn load(input: &str) -> Self {
        serde_json::from_str::<ProfileStore>(input)
            .ok()
            .filter(|s| s.version == PROFILE_VERSION)
            .unwrap_or_else(Self::empty)
    }

    /// 등록 (canonical key로 중복 방지). 같은 key가 있으면 기존 프로필을 반환한다.
    pub fn upsert(&mut self, profile: ProjectProfile) -> Result<Option<ProjectProfile>, String> {
        let key = profile.canonical_key()?;
        if let Some(existing) = self
            .profiles
            .iter()
            .find(|p| p.canonical_key().ok().as_deref() == Some(key.as_str()))
        {
            return Ok(Some(existing.clone()));
        }
        self.profiles.push(profile);
        Ok(None)
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
            .map_err(|e| e.to_string())
    }
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
    fn load_corrupt_returns_empty() {
        assert_eq!(ProfileStore::load("not json"), ProfileStore::empty());
    }

    #[test]
    fn canonical_key_roundtrip() {
        let p = profile("x", Some("E:\\a\\b"), None);
        assert!(p.canonical_key().is_ok());
    }
}
