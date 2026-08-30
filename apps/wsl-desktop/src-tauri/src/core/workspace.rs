//! WSL Desktop terminal workspace/profile schema and validation.
//!
//! Runtime PTY session ids never cross this boundary. A profile stores stable pane keys,
//! tab ownership, distro/cwd/layout and an optional one-line start command. The command is
//! intentionally shell input, but raw credentials and control characters are rejected before
//! the definition reaches disk.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const PROFILE_STORE_VERSION: u32 = 1;
pub const MAX_PROFILES: usize = 100;
pub const MAX_TABS: usize = 16;
pub const MAX_PANES: usize = 32;
pub const MAX_NAME_BYTES: usize = 120;
pub const MAX_START_COMMAND_CHARACTERS: usize = 4_096;
const MAX_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Grid,
    Cols,
    Rows,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MultiplexerKind {
    #[default]
    Native,
    Tmux,
    Zellij,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePane {
    pub key: String,
    pub distro: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub start_command: Option<String>,
    #[serde(default)]
    pub multiplexer: MultiplexerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTab {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub custom_title: bool,
    pub layout: Layout,
    pub pane_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProfile {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub tabs: Vec<WorkspaceTab>,
    pub panes: Vec<WorkspacePane>,
    pub active_tab_id: String,
    #[serde(default)]
    pub active_pane_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStore {
    pub version: u32,
    pub profiles: Vec<WorkspaceProfile>,
}

impl Default for ProfileStore {
    fn default() -> Self {
        Self::empty()
    }
}

impl ProfileStore {
    pub fn empty() -> Self {
        Self {
            version: PROFILE_STORE_VERSION,
            profiles: Vec::new(),
        }
    }

    /// Parse a complete store without turning corrupt input into a writable
    /// empty value. Callers may show an empty read view, but mutations must
    /// preserve invalid source bytes until the user explicitly repairs them.
    pub fn load(input: &str) -> Result<Self, String> {
        let store = serde_json::from_str::<Self>(input)
            .map_err(|_| "터미널 프로필 저장소 형식이 올바르지 않습니다".to_string())?;
        store.validate()?;
        Ok(store)
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "터미널 프로필을 직렬화할 수 없습니다".to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != PROFILE_STORE_VERSION || self.profiles.len() > MAX_PROFILES {
            return Err("터미널 프로필 저장소 형식이 올바르지 않습니다".into());
        }
        let mut ids = HashSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(profile.id.as_str()) {
                return Err("중복된 터미널 프로필 식별자가 있습니다".into());
            }
        }
        Ok(())
    }

    pub fn upsert(&mut self, profile: WorkspaceProfile) -> Result<(), String> {
        profile.validate()?;
        if let Some(existing) = self.profiles.iter_mut().find(|item| item.id == profile.id) {
            *existing = profile;
            return self.validate();
        }
        if self.profiles.len() >= MAX_PROFILES {
            return Err("터미널 프로필은 최대 100개까지 저장할 수 있습니다".into());
        }
        self.profiles.push(profile);
        self.validate()
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.id != id);
        before != self.profiles.len()
    }
}

impl WorkspaceProfile {
    pub fn validate(&self) -> Result<(), String> {
        validate_id(&self.id, "프로필")?;
        validate_name(&self.name, "프로필 이름")?;
        if self.tabs.is_empty()
            || self.tabs.len() > MAX_TABS
            || self.panes.is_empty()
            || self.panes.len() > MAX_PANES
        {
            return Err("터미널 프로필의 탭 또는 팬 수가 올바르지 않습니다".into());
        }

        let mut pane_keys = HashSet::new();
        for pane in &self.panes {
            pane.validate()?;
            if !pane_keys.insert(pane.key.as_str()) {
                return Err("중복된 터미널 팬 식별자가 있습니다".into());
            }
        }

        let mut tab_ids = HashSet::new();
        let mut referenced_panes = HashSet::new();
        for tab in &self.tabs {
            tab.validate()?;
            if !tab_ids.insert(tab.id.as_str()) {
                return Err("중복된 터미널 탭 식별자가 있습니다".into());
            }
            for key in &tab.pane_keys {
                if !pane_keys.contains(key.as_str()) || !referenced_panes.insert(key.as_str()) {
                    return Err("터미널 탭의 팬 참조가 올바르지 않습니다".into());
                }
            }
        }
        if referenced_panes.len() != pane_keys.len()
            || !tab_ids.contains(self.active_tab_id.as_str())
        {
            return Err("터미널 프로필의 활성 탭 또는 팬이 올바르지 않습니다".into());
        }
        let active_tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == self.active_tab_id)
            .expect("active tab identity was checked");
        if self.active_pane_key.as_deref().is_some_and(|key| {
            !active_tab
                .pane_keys
                .iter()
                .any(|candidate| candidate == key)
        }) {
            return Err("터미널 프로필의 활성 팬이 활성 탭에 속하지 않습니다".into());
        }
        Ok(())
    }
}

impl WorkspacePane {
    fn validate(&self) -> Result<(), String> {
        validate_id(&self.key, "팬")?;
        devbox_wsl::argv::build_exec_argv(&self.distro, None, "")
            .map_err(|_| "터미널 프로필의 배포판 이름이 올바르지 않습니다".to_string())?;
        if let Some(cwd) = self.cwd.as_deref() {
            if devbox_filesystem::parse_safe_project_path(cwd).is_none() {
                return Err("터미널 프로필의 시작 경로가 안전하지 않습니다".into());
            }
        }
        if let Some(command) = self.start_command.as_deref() {
            validate_start_command(command)?;
        }
        Ok(())
    }
}

impl WorkspaceTab {
    fn validate(&self) -> Result<(), String> {
        validate_id(&self.id, "탭")?;
        validate_name(&self.title, "탭 이름")?;
        if self.pane_keys.is_empty() || self.pane_keys.len() > MAX_PANES {
            return Err("터미널 탭의 팬 구성이 올바르지 않습니다".into());
        }
        let mut keys = HashSet::new();
        for key in &self.pane_keys {
            validate_id(key, "팬")?;
            if !keys.insert(key.as_str()) {
                return Err("터미널 탭에 중복된 팬이 있습니다".into());
            }
        }
        Ok(())
    }
}

fn validate_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("터미널 {label} 식별자가 올바르지 않습니다"));
    }
    Ok(())
}

fn validate_name(value: &str, label: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_NAME_BYTES || trimmed.chars().any(char::is_control)
    {
        return Err(format!("{label}이 올바르지 않습니다"));
    }
    Ok(())
}

pub fn validate_start_command(command: &str) -> Result<(), String> {
    let trimmed = command.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_START_COMMAND_CHARACTERS
        || trimmed.chars().any(char::is_control)
    {
        return Err("시작 명령은 4,096자 이하의 한 줄이어야 합니다".into());
    }
    if looks_like_raw_credential(trimmed) {
        return Err("시작 명령에 평문 자격증명을 저장할 수 없습니다".into());
    }
    Ok(())
}

fn looks_like_raw_credential(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if contains_private_key_header(&lower)
        || ["sk-", "ghp_", "github_pat_", "xoxb-", "xoxp-"]
            .iter()
            .any(|prefix| contains_prefixed_secret(&lower, prefix))
    {
        return true;
    }

    if lower.match_indices("authorization:").any(|(index, _)| {
        let remainder = lower[index + "authorization:".len()..].trim_start();
        remainder.strip_prefix("bearer").is_some_and(|candidate| {
            candidate.chars().next().is_some_and(char::is_whitespace)
                && is_literal_credential(candidate)
        })
    }) {
        return true;
    }

    [
        "--password=",
        "--password ",
        "--token=",
        "--token ",
        "api_key=",
        "apikey=",
        "client_secret=",
        "access_token=",
    ]
    .iter()
    .any(|marker| {
        lower
            .match_indices(marker)
            .any(|(index, _)| is_literal_credential(&command[index + marker.len()..]))
    })
}

fn contains_private_key_header(value: &str) -> bool {
    value.match_indices("-----begin ").any(|(index, _)| {
        value[index + "-----begin ".len()..]
            .find("private key-----")
            .is_some_and(|offset| offset <= 40)
    })
}

fn is_literal_credential(remainder: &str) -> bool {
    let candidate = remainder
        .trim_start()
        .split_ascii_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['\'', '"']);
    !candidate.is_empty()
        && !candidate.starts_with('$')
        && !(candidate.starts_with('%') && candidate.ends_with('%'))
        && !(candidate.starts_with("{{") && candidate.ends_with("}}"))
}

fn contains_prefixed_secret(value: &str, prefix: &str) -> bool {
    value.match_indices(prefix).any(|(index, _)| {
        let boundary = index == 0
            || value.as_bytes()[index - 1].is_ascii_whitespace()
            || matches!(value.as_bytes()[index - 1], b'\'' | b'"' | b'=' | b':');
        if !boundary {
            return false;
        }
        value[index + prefix.len()..]
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            .take(12)
            .count()
            >= 12
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> WorkspaceProfile {
        WorkspaceProfile {
            id: "profile-1".into(),
            name: "개발 환경".into(),
            tabs: vec![WorkspaceTab {
                id: "tab-1".into(),
                title: "server".into(),
                custom_title: true,
                layout: Layout::Cols,
                pane_keys: vec!["pane-1".into(), "pane-2".into()],
            }],
            panes: vec![
                WorkspacePane {
                    key: "pane-1".into(),
                    distro: "Ubuntu".into(),
                    cwd: Some("/mnt/e/projects/devbox".into()),
                    start_command: Some("pnpm dev".into()),
                    multiplexer: MultiplexerKind::Native,
                },
                WorkspacePane {
                    key: "pane-2".into(),
                    distro: "Ubuntu".into(),
                    cwd: Some(r"E:\projects\devbox".into()),
                    start_command: None,
                    multiplexer: MultiplexerKind::Tmux,
                },
            ],
            active_tab_id: "tab-1".into(),
            active_pane_key: Some("pane-2".into()),
        }
    }

    #[test]
    fn profile_store_roundtrips() {
        let mut store = ProfileStore::empty();
        store.upsert(profile()).unwrap();
        let json = store.to_json().unwrap();
        assert_eq!(ProfileStore::load(&json).unwrap(), store);
    }

    #[test]
    fn corrupt_or_unsupported_store_fails_closed() {
        assert!(ProfileStore::load("not json").is_err());
        assert!(ProfileStore::load(r#"{"version":99,"profiles":[]}"#).is_err());
    }

    #[test]
    fn rejects_duplicate_or_missing_pane_references() {
        let mut duplicated = profile();
        duplicated.tabs[0].pane_keys.push("pane-1".into());
        assert!(duplicated.validate().is_err());

        let mut orphan = profile();
        orphan.tabs[0].pane_keys.pop();
        assert!(orphan.validate().is_err());

        let mut wrong_active_tab = profile();
        wrong_active_tab.tabs = vec![
            WorkspaceTab {
                id: "tab-1".into(),
                title: "one".into(),
                custom_title: false,
                layout: Layout::Grid,
                pane_keys: vec!["pane-1".into()],
            },
            WorkspaceTab {
                id: "tab-2".into(),
                title: "two".into(),
                custom_title: false,
                layout: Layout::Grid,
                pane_keys: vec!["pane-2".into()],
            },
        ];
        wrong_active_tab.active_tab_id = "tab-1".into();
        wrong_active_tab.active_pane_key = Some("pane-2".into());
        assert!(wrong_active_tab.validate().is_err());
    }

    #[test]
    fn rejects_unsafe_cwd_and_multiline_or_secret_commands() {
        let mut unsafe_path = profile();
        unsafe_path.panes[0].cwd = Some("../../escape".into());
        assert!(unsafe_path.validate().is_err());

        assert!(validate_start_command("echo one\necho two").is_err());
        assert!(validate_start_command("curl -H 'Authorization: Bearer raw-value'").is_err());
        assert!(validate_start_command("curl -H 'Authorization:   Bearer raw-value'").is_err());
        assert!(validate_start_command("echo '-----BEGIN OPENSSH PRIVATE KEY-----'").is_err());
        assert!(validate_start_command("tool --token=literal-value").is_err());
        assert!(validate_start_command("tool --token=$TOKEN next --token=literal-value").is_err());
        assert!(validate_start_command("curl -H 'Authorization: Bearer $TOKEN'").is_ok());
        assert!(validate_start_command("tool --token={{token_ref}}").is_ok());
        assert!(validate_start_command("task-runner --mode dev").is_ok());
    }

    #[test]
    fn upsert_replaces_by_id_without_reordering() {
        let mut store = ProfileStore::empty();
        store.upsert(profile()).unwrap();
        let mut changed = profile();
        changed.name = "변경됨".into();
        store.upsert(changed).unwrap();
        assert_eq!(store.profiles.len(), 1);
        assert_eq!(store.profiles[0].name, "변경됨");
    }
}
