//! Reusable Workbench profile templates.
//!
//! Templates are intentionally smaller than `ProjectProfile`: they contain
//! project-independent defaults only and can never carry project environment
//! metadata or secret references.  A wizard supplies the concrete project
//! identity before the template is applied.

use super::profile::{validate_profile_id, validate_service_id, ProjectProfile, WslProfile};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const PROFILE_TEMPLATE_VERSION: u32 = 1;
pub const MAX_PROFILE_TEMPLATES: usize = 128;
pub const MAX_PROFILE_TEMPLATE_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_TEMPLATE_NAME_CHARS: usize = 120;
const MAX_WSL_DISTRO_CHARS: usize = 128;

/// A profile template contains defaults that are safe to copy into a new
/// profile.  It has no `environment` field by design: `.env` metadata and
/// secret references are always selected and reviewed for the concrete
/// project in the wizard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub windows_path: Option<String>,
    #[serde(default)]
    pub wsl: Option<WslProfile>,
    #[serde(default)]
    pub git_root: Option<String>,
    #[serde(default)]
    pub expected_ports: Vec<u16>,
    #[serde(default)]
    pub run_manager_service_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileTemplateStore {
    pub version: u32,
    pub templates: Vec<ProfileTemplate>,
}

impl Default for ProfileTemplateStore {
    fn default() -> Self {
        Self::empty()
    }
}

impl ProfileTemplateStore {
    pub fn empty() -> Self {
        Self {
            version: PROFILE_TEMPLATE_VERSION,
            templates: Vec::new(),
        }
    }

    pub fn to_json_checked(&self) -> Result<String, String> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|_| "프로필 템플릿을 직렬화할 수 없습니다".to_string())?;
        if json.len() > MAX_PROFILE_TEMPLATE_FILE_BYTES {
            return Err("프로필 템플릿 저장소 크기 제한을 초과했습니다".into());
        }
        Ok(json)
    }

    pub fn load(input: &str) -> Result<Self, String> {
        if input.len() > MAX_PROFILE_TEMPLATE_FILE_BYTES {
            return Err("프로필 템플릿 저장소 크기 제한을 초과했습니다".into());
        }
        let store = serde_json::from_str::<Self>(input)
            .map_err(|_| "프로필 템플릿 저장소 형식이 올바르지 않습니다".to_string())?;
        store.validate()?;
        Ok(store)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != PROFILE_TEMPLATE_VERSION {
            return Err("프로필 템플릿 저장소 버전을 지원하지 않습니다".into());
        }
        if self.templates.len() > MAX_PROFILE_TEMPLATES {
            return Err("프로필 템플릿이 너무 많습니다".into());
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for template in &self.templates {
            template.validate()?;
            if !ids.insert(template.id.as_str()) {
                return Err("같은 프로필 템플릿 ID를 두 번 사용할 수 없습니다".into());
            }
            if !names.insert(template.name.to_lowercase()) {
                return Err("같은 프로필 템플릿 이름을 두 번 사용할 수 없습니다".into());
            }
        }
        Ok(())
    }

    /// Insert a template, returning an existing same-ID/name entry as a
    /// harmless idempotent result.  The candidate store is validated before
    /// replacing the current value.
    pub fn upsert(&mut self, template: ProfileTemplate) -> Result<Option<ProfileTemplate>, String> {
        self.validate()?;
        template.validate()?;
        if let Some(existing) = self
            .templates
            .iter()
            .find(|candidate| candidate.id == template.id)
        {
            if existing == &template {
                return Ok(Some(existing.clone()));
            }
            return Err("같은 프로필 템플릿 ID를 두 번 사용할 수 없습니다".into());
        }
        if let Some(existing) = self
            .templates
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(&template.name))
        {
            return Ok(Some(existing.clone()));
        }
        let mut next = self.clone();
        next.templates.push(template);
        next.validate()?;
        *self = next;
        Ok(None)
    }

    pub fn replace(&mut self, template: ProfileTemplate) -> Result<(), String> {
        self.validate()?;
        template.validate()?;
        let index = self
            .templates
            .iter()
            .position(|candidate| candidate.id == template.id)
            .ok_or_else(|| "프로필 템플릿을 찾을 수 없습니다".to_string())?;
        if self
            .templates
            .iter()
            .enumerate()
            .any(|(candidate_index, candidate)| {
                candidate_index != index && candidate.name.eq_ignore_ascii_case(&template.name)
            })
        {
            return Err("같은 프로필 템플릿 이름을 두 번 사용할 수 없습니다".into());
        }
        let mut next = self.clone();
        next.templates[index] = template;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.templates.len();
        self.templates.retain(|template| template.id != id);
        before != self.templates.len()
    }
}

impl ProfileTemplate {
    #[allow(dead_code)]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            windows_path: None,
            wsl: None,
            git_root: None,
            expected_ports: Vec::new(),
            run_manager_service_ids: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_profile_id(&self.id)?;
        if self.name.trim().is_empty() || self.name != self.name.trim() {
            return Err("프로필 템플릿 이름이 올바르지 않습니다".into());
        }
        if self.name.chars().count() > MAX_TEMPLATE_NAME_CHARS
            || self.name.chars().any(char::is_control)
        {
            return Err("프로필 템플릿 이름이 올바르지 않습니다".into());
        }
        validate_optional_path(
            self.windows_path.as_deref(),
            "프로젝트 경로가 올바르지 않습니다",
        )?;
        validate_optional_path(
            self.git_root.as_deref(),
            "Git root 경로가 올바르지 않습니다",
        )?;
        if let Some(wsl) = &self.wsl {
            if wsl.distro.trim().is_empty()
                || wsl.distro != wsl.distro.trim()
                || wsl.path.trim().is_empty()
                || wsl.path != wsl.path.trim()
                || wsl.distro.chars().count() > MAX_WSL_DISTRO_CHARS
                || wsl.distro.chars().any(char::is_control)
                || wsl.path.chars().any(char::is_control)
            {
                return Err("WSL distro와 경로가 올바르지 않습니다".into());
            }
            if devbox_wsl::distro::validate_distro_name(&wsl.distro).is_err()
                || devbox_filesystem::parse_safe_project_path(&wsl.path).is_none()
            {
                return Err("WSL distro와 경로가 올바르지 않습니다".into());
            }
        }
        if self.expected_ports.len() > super::profile::MAX_EXPECTED_PORTS
            || self.expected_ports.contains(&0)
        {
            return Err("예상 포트가 올바르지 않습니다".into());
        }
        let mut ports = HashSet::new();
        if self.expected_ports.iter().any(|port| !ports.insert(*port)) {
            return Err("같은 포트를 두 번 등록할 수 없습니다".into());
        }
        if self.run_manager_service_ids.len() > super::profile::MAX_SERVICES {
            return Err("서비스가 너무 많습니다".into());
        }
        let mut services = HashSet::new();
        for service in &self.run_manager_service_ids {
            validate_service_id(service)?;
            if !services.insert(service.as_str()) {
                return Err("같은 서비스 ID를 두 번 등록할 수 없습니다".into());
            }
        }
        Ok(())
    }

    /// Apply only empty wizard fields from this template.  Existing user
    /// input always wins, while the resulting profile is validated as a
    /// complete profile.  Environment data is explicitly cleared.
    pub fn apply_to_profile(&self, mut profile: ProjectProfile) -> Result<ProjectProfile, String> {
        self.validate()?;
        if profile.windows_path.is_none() {
            profile.windows_path = self.windows_path.clone();
        }
        if profile.wsl.is_none() {
            profile.wsl = self.wsl.clone();
        }
        if profile.git_root.is_none() {
            profile.git_root = self.git_root.clone();
        }
        if profile.expected_ports.is_empty() {
            profile.expected_ports = self.expected_ports.clone();
        }
        if profile.run_manager_service_ids.is_empty() {
            profile.run_manager_service_ids = self.run_manager_service_ids.clone();
        }
        // A template is never an authority for project environment values.
        // The wizard can perform a separate explicit native preview later.
        profile.environment = None;
        profile.validate()?;
        Ok(profile)
    }
}

fn validate_optional_path(path: Option<&str>, message: &str) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    if path.trim().is_empty()
        || path != path.trim()
        || path.chars().any(char::is_control)
        || devbox_filesystem::parse_safe_project_path(path).is_none()
    {
        return Err(message.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str, name: &str) -> ProfileTemplate {
        ProfileTemplate {
            id: id.into(),
            name: name.into(),
            windows_path: Some("C:/projects/template".into()),
            wsl: Some(WslProfile {
                distro: "Ubuntu".into(),
                path: "/mnt/c/projects/template".into(),
            }),
            git_root: Some("C:/projects/template".into()),
            expected_ports: vec![3000, 5173],
            run_manager_service_ids: vec!["web".into()],
        }
    }

    #[test]
    fn template_round_trips_and_rejects_unknown_fields() {
        let store = ProfileTemplateStore {
            version: PROFILE_TEMPLATE_VERSION,
            templates: vec![template("template-1", "Node")],
        };
        let json = store.to_json_checked().unwrap();
        assert_eq!(ProfileTemplateStore::load(&json).unwrap(), store);
        assert!(ProfileTemplateStore::load(
            r#"{"version":1,"templates":[],"credential":"TOP_SECRET"}"#
        )
        .is_err());
    }

    #[test]
    fn template_store_rejects_duplicate_names_and_oversized_lists() {
        let first = template("template-1", "Node");
        let mut second = template("template-2", "node");
        second.windows_path = Some("C:/projects/other".into());
        let store = ProfileTemplateStore {
            version: PROFILE_TEMPLATE_VERSION,
            templates: vec![first, second],
        };
        assert!(store.validate().is_err());

        let oversized = ProfileTemplateStore {
            version: PROFILE_TEMPLATE_VERSION,
            templates: (0..=MAX_PROFILE_TEMPLATES)
                .map(|index| template(&format!("template-{index}"), &format!("Template {index}")))
                .collect(),
        };
        assert_eq!(
            oversized.validate().unwrap_err(),
            "프로필 템플릿이 너무 많습니다"
        );
    }

    #[test]
    fn template_store_crud_is_idempotent_and_failed_replace_is_atomic() {
        let mut store = ProfileTemplateStore::empty();
        let original = template("template-1", "Node");
        assert!(store.upsert(original.clone()).unwrap().is_none());
        assert_eq!(
            store.upsert(original.clone()).unwrap(),
            Some(original.clone())
        );

        let mut replacement = original.clone();
        replacement.name = "Node updated".into();
        store.replace(replacement.clone()).unwrap();
        assert_eq!(store.templates, vec![replacement.clone()]);

        let mut colliding = template("template-2", "Node updated");
        colliding.expected_ports = vec![9000];
        assert!(store.replace(colliding).is_err());
        assert_eq!(store.templates, vec![replacement]);
        assert!(store.remove("template-1"));
        assert!(!store.remove("template-1"));
        assert!(store.templates.is_empty());
    }

    #[test]
    fn applying_template_fills_only_empty_fields_and_drops_environment() {
        let source = template("template-1", "Node");
        let mut profile = ProjectProfile::new("new project");
        profile.windows_path = Some("C:/projects/concrete".into());
        profile.environment = Some(super::super::environment::ProjectEnvironmentConfig {
            enabled: false,
            source: ".env".into(),
            revision: "a".repeat(64),
            variables: Vec::new(),
        });
        let applied = source.apply_to_profile(profile).unwrap();
        assert_eq!(
            applied.windows_path.as_deref(),
            Some("C:/projects/concrete")
        );
        assert_eq!(applied.expected_ports, vec![3000, 5173]);
        assert_eq!(
            applied.wsl.as_ref().map(|wsl| wsl.distro.as_str()),
            Some("Ubuntu")
        );
        assert!(applied.environment.is_none());
    }

    #[test]
    fn unsafe_template_defaults_fail_closed_without_echoing_input() {
        let mut unsafe_template = template("template-1", "Node");
        unsafe_template.windows_path = Some("C:/private/../escape".into());
        let error = unsafe_template.validate().unwrap_err();
        assert_eq!(error, "프로젝트 경로가 올바르지 않습니다");
        assert!(!error.contains("private"));
    }
}
