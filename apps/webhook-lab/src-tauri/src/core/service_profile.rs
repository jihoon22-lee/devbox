//! Strict app-local profiles used by exported disabled Run Manager services.
//!
//! The exported JSON contains only an opaque profile id and a fixed command.
//! Rule bodies stay in Webhook Lab's own local-data directory and are loaded
//! only by an exact `--service-profile <uuid>` startup mode.

use super::rules::{validate_rule_collection, ResponseRule, MAX_RULES};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SERVICE_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const SERVICE_PROFILE_DIRECTORY: &str = "service-profiles";
pub const MAX_SERVICE_PROFILES: usize = 64;
pub const MAX_PROFILE_DIRECTORY_ENTRIES: usize = 256;
pub const MAX_SERVICE_PROFILE_BYTES: u64 = 8 * 1024 * 1024;
pub const SERVICE_PROFILE_ERROR: &str = "Webhook service profile을 만들 수 없습니다";
pub const SERVICE_PROFILE_LOAD_ERROR: &str = "Webhook service profile을 읽을 수 없습니다";
pub const SERVICE_PROFILE_SECRET_ERROR: &str =
    "credential 형태의 응답이 포함된 규칙은 service profile로 내보낼 수 없습니다";
pub const SERVICE_PROFILE_LIMIT_ERROR: &str = "Webhook service profile 개수 제한에 도달했습니다";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceProfile {
    pub schema_version: u32,
    pub id: String,
    pub bind: String,
    pub port: u16,
    pub rules: Vec<ResponseRule>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunDefinitionExport {
    pub schema_version: u32,
    pub exported_at: String,
    pub jobs: Vec<RunServiceDefinition>,
    pub services: Vec<RunServiceDefinition>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunServiceDefinition {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub target_kind: String,
    pub target_distro: Option<String>,
    pub env_configured: bool,
    pub cron_expr: Option<String>,
    pub enabled: bool,
    pub overlap_policy: String,
    pub catch_up: bool,
    pub last_evaluated_at: Option<i64>,
    pub next_queue_sequence: i64,
    pub restart_policy: Option<String>,
    pub auto_start: Option<bool>,
    pub health_tcp_address: Option<String>,
    pub health_tcp_port: Option<u16>,
    pub health_start_grace_ms: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn export_run_definition_in(
    data_root: &Path,
    executable: &Path,
    bind: &str,
    port: u16,
    rules: Vec<ResponseRule>,
    now_ms: u64,
) -> Result<RunDefinitionExport, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let profile = ServiceProfile {
        schema_version: SERVICE_PROFILE_SCHEMA_VERSION,
        id: id.clone(),
        bind: bind.to_owned(),
        port,
        rules,
        created_at_ms: now_ms,
    };
    validate_profile(&profile)?;
    let command = service_command(executable, &id)?;
    write_profile(data_root, &profile)?;
    let timestamp = i64::try_from(now_ms).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    let service = RunServiceDefinition {
        id,
        kind: "service".into(),
        name: format!("Webhook Lab · {bind}:{port}"),
        command,
        cwd: None,
        target_kind: "windows".into(),
        target_distro: None,
        env_configured: false,
        cron_expr: None,
        enabled: false,
        overlap_policy: "skip".into(),
        catch_up: false,
        last_evaluated_at: None,
        next_queue_sequence: 0,
        restart_policy: Some("never".into()),
        auto_start: Some(false),
        health_tcp_address: Some(bind.into()),
        health_tcp_port: Some(port),
        health_start_grace_ms: Some(10_000),
        created_at: timestamp,
        updated_at: timestamp,
    };
    Ok(RunDefinitionExport {
        schema_version: 1,
        exported_at: now_ms.to_string(),
        jobs: Vec::new(),
        services: vec![service],
    })
}

pub fn load_profile(data_root: &Path, id: &str) -> Result<ServiceProfile, String> {
    validate_profile_id(id).map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    let directory = data_root.join(SERVICE_PROFILE_DIRECTORY);
    let path = profile_path(data_root, id);
    devbox_filesystem::ensure_no_links(&directory)
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    devbox_filesystem::ensure_no_links(&path)
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(&path, false)
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    let metadata = file
        .metadata()
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    if metadata.len() > MAX_SERVICE_PROFILE_BYTES {
        return Err(SERVICE_PROFILE_LOAD_ERROR.into());
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(MAX_SERVICE_PROFILE_BYTES) as usize);
    file.by_ref()
        .take(MAX_SERVICE_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    if bytes.len() as u64 > MAX_SERVICE_PROFILE_BYTES {
        return Err(SERVICE_PROFILE_LOAD_ERROR.into());
    }
    devbox_filesystem::ensure_no_links(&path)
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    let current = devbox_filesystem::filesystem_identity(&path, false)
        .map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    if current != identity {
        return Err(SERVICE_PROFILE_LOAD_ERROR.into());
    }
    let profile: ServiceProfile =
        serde_json::from_slice(&bytes).map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    validate_profile(&profile).map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    if profile.id != id {
        return Err(SERVICE_PROFILE_LOAD_ERROR.into());
    }
    Ok(profile)
}

fn write_profile(data_root: &Path, profile: &ServiceProfile) -> Result<(), String> {
    if !data_root.is_absolute() {
        return Err(SERVICE_PROFILE_ERROR.into());
    }
    std::fs::create_dir_all(data_root).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    devbox_filesystem::ensure_no_links(data_root).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    let directory = data_root.join(SERVICE_PROFILE_DIRECTORY);
    std::fs::create_dir_all(&directory).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    devbox_filesystem::ensure_no_links(&directory)
        .map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    enforce_profile_count(&directory)?;
    let path = profile_path(data_root, &profile.id);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            return Err(SERVICE_PROFILE_ERROR.into())
        }
        Ok(_) => return Err(SERVICE_PROFILE_ERROR.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(SERVICE_PROFILE_ERROR.into()),
    }
    let bytes =
        serde_json::to_vec_pretty(profile).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    if bytes.len() as u64 > MAX_SERVICE_PROFILE_BYTES {
        return Err(SERVICE_PROFILE_ERROR.into());
    }
    devbox_filesystem::atomic_write(&path, &bytes)
        .map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    devbox_filesystem::ensure_no_links(&path).map_err(|_| SERVICE_PROFILE_ERROR.to_string())
}

fn enforce_profile_count(directory: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(directory).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    let mut visited = 0usize;
    let mut profiles = 0usize;
    for entry in entries {
        let entry = entry.map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
        visited = visited.saturating_add(1);
        if visited > MAX_PROFILE_DIRECTORY_ENTRIES {
            return Err(SERVICE_PROFILE_LIMIT_ERROR.into());
        }
        let kind = entry
            .file_type()
            .map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
        if kind.is_symlink() {
            return Err(SERVICE_PROFILE_ERROR.into());
        }
        if !kind.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name
            .strip_suffix(".json")
            .is_some_and(|id| validate_profile_id(id).is_ok())
        {
            profiles = profiles.saturating_add(1);
        }
    }
    if profiles >= MAX_SERVICE_PROFILES {
        return Err(SERVICE_PROFILE_LIMIT_ERROR.into());
    }
    Ok(())
}

fn validate_profile(profile: &ServiceProfile) -> Result<(), String> {
    validate_profile_id(&profile.id)?;
    if profile.schema_version != SERVICE_PROFILE_SCHEMA_VERSION
        || !matches!(profile.bind.as_str(), "127.0.0.1" | "::1")
        || profile.port == 0
        || profile.created_at_ms == 0
        || profile.created_at_ms > 9_007_199_254_740_991
        || profile.rules.len() > MAX_RULES
    {
        return Err(SERVICE_PROFILE_ERROR.into());
    }
    validate_rule_collection(profile.rules.iter())
        .map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    let mut ids = HashSet::new();
    for rule in &profile.rules {
        if rule_contains_secret(rule) {
            return Err(SERVICE_PROFILE_SECRET_ERROR.into());
        }
        if rule.id.is_empty() || !ids.insert(rule.id.clone()) {
            return Err(SERVICE_PROFILE_ERROR.into());
        }
    }
    Ok(())
}

fn rule_contains_secret(rule: &ResponseRule) -> bool {
    text_contains_secret(&rule.id)
        || text_contains_secret(&rule.path)
        || rule.method.as_deref().is_some_and(text_contains_secret)
        || response_contains_secret(&rule.headers, &rule.body)
        || rule
            .sequence
            .iter()
            .any(|step| response_contains_secret(&step.headers, &step.body))
}

fn text_contains_secret(value: &str) -> bool {
    devbox_applink::contains_sensitive_value(value)
        || devbox_applink::validate_handoff_text(value).is_err()
}

fn response_contains_secret(headers: &[(String, String)], body: &str) -> bool {
    text_contains_secret(body)
        || headers
            .iter()
            .any(|(name, value)| text_contains_secret(&format!("{name}: {value}")))
}

fn service_command(executable: &Path, id: &str) -> Result<String, String> {
    validate_profile_id(id)?;
    if !executable.is_absolute() {
        return Err(SERVICE_PROFILE_ERROR.into());
    }
    let value = executable
        .to_str()
        .ok_or_else(|| SERVICE_PROFILE_ERROR.to_string())?;
    if value.is_empty()
        || value.len() > 8 * 1024
        || value.chars().any(char::is_control)
        || value.bytes().any(|byte| {
            matches!(
                byte,
                b'"' | b'%' | b'!' | b'^' | b'&' | b'|' | b'<' | b'>' | b'(' | b')'
            )
        })
    {
        return Err(SERVICE_PROFILE_ERROR.into());
    }
    Ok(format!("call \"{value}\" --service-profile {id}"))
}

pub fn parse_service_profile_argv(args: &[String]) -> Result<Option<String>, String> {
    let rest = args.get(1..).unwrap_or_default();
    if !rest.iter().any(|value| value == "--service-profile") {
        return Ok(None);
    }
    if rest.len() != 2 || rest[0] != "--service-profile" {
        return Err(SERVICE_PROFILE_LOAD_ERROR.into());
    }
    validate_profile_id(&rest[1]).map_err(|_| SERVICE_PROFILE_LOAD_ERROR.to_string())?;
    Ok(Some(rest[1].clone()))
}

fn validate_profile_id(value: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| SERVICE_PROFILE_ERROR.to_string())?;
    if parsed.to_string() != value {
        return Err(SERVICE_PROFILE_ERROR.into());
    }
    Ok(())
}

fn profile_path(data_root: &Path, id: &str) -> PathBuf {
    data_root
        .join(SERVICE_PROFILE_DIRECTORY)
        .join(format!("{id}.json"))
}

pub fn rules_map(profile: &ServiceProfile) -> HashMap<String, ResponseRule> {
    profile
        .rules
        .iter()
        .cloned()
        .map(|rule| (rule.id.clone(), rule))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> ResponseRule {
        ResponseRule {
            id: "rule-1".into(),
            priority: 10,
            method: Some("POST".into()),
            path: "/hook".into(),
            status: 202,
            headers: vec![],
            body: "accepted".into(),
            delay_ms: 0,
            sequence: vec![],
        }
    }

    #[test]
    fn export_is_run_manager_schema_v1_and_profile_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("Webhook Lab/webhook-lab.exe");
        let export = export_run_definition_in(
            root.path(),
            &executable,
            "127.0.0.1",
            9000,
            vec![rule()],
            1_725_000_000_000,
        )
        .unwrap();
        assert!(export.jobs.is_empty());
        assert_eq!(export.services.len(), 1);
        let service = &export.services[0];
        assert_eq!(service.kind, "service");
        assert!(!service.enabled);
        assert_eq!(service.auto_start, Some(false));
        assert_eq!(service.restart_policy.as_deref(), Some("never"));
        assert_eq!(service.health_tcp_port, Some(9000));
        assert!(service.command.contains("--service-profile"));
        assert!(!serde_json::to_string(&export).unwrap().contains("accepted"));
        let profile = load_profile(root.path(), &service.id).unwrap();
        assert_eq!(profile.rules, vec![rule()]);
        assert_eq!(profile.bind, "127.0.0.1");
    }

    #[test]
    fn export_rejects_lan_secrets_and_shell_metacharacters() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("webhook-lab.exe");
        assert!(export_run_definition_in(
            root.path(),
            &executable,
            "0.0.0.0",
            9000,
            vec![rule()],
            1,
        )
        .is_err());
        let mut secret = rule();
        secret.headers = vec![("Authorization".into(), "Bearer private-token".into())];
        assert_eq!(
            export_run_definition_in(root.path(), &executable, "127.0.0.1", 9000, vec![secret], 1,)
                .unwrap_err(),
            SERVICE_PROFILE_SECRET_ERROR
        );
        let mut secret_path = rule();
        secret_path.path = "/hook?access_token=private-token".into();
        assert_eq!(
            export_run_definition_in(
                root.path(),
                &executable,
                "127.0.0.1",
                9000,
                vec![secret_path],
                1,
            )
            .unwrap_err(),
            SERVICE_PROFILE_SECRET_ERROR
        );
        let mut secret_json = rule();
        secret_json.body = r#"{"password":"private-token"}"#.into();
        assert_eq!(
            export_run_definition_in(
                root.path(),
                &executable,
                "127.0.0.1",
                9000,
                vec![secret_json],
                1,
            )
            .unwrap_err(),
            SERVICE_PROFILE_SECRET_ERROR
        );
        let id = uuid::Uuid::new_v4().to_string();
        assert!(service_command(Path::new("/tmp/bad%path/webhook-lab.exe"), &id).is_err());
        assert!(service_command(Path::new("/tmp/bad(path)/webhook-lab.exe"), &id).is_err());
    }

    #[test]
    fn loader_rejects_unknown_fields_links_and_identity_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("webhook-lab.exe");
        let export =
            export_run_definition_in(root.path(), &executable, "::1", 9001, vec![rule()], 2)
                .unwrap();
        let id = &export.services[0].id;
        let path = profile_path(root.path(), id);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("command".into(), serde_json::json!("private"));
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            load_profile(root.path(), id).unwrap_err(),
            SERVICE_PROFILE_LOAD_ERROR
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let other = root.path().join("other.json");
            std::fs::write(&other, b"{}").unwrap();
            std::fs::remove_file(&path).unwrap();
            symlink(&other, &path).unwrap();
            assert_eq!(
                load_profile(root.path(), id).unwrap_err(),
                SERVICE_PROFILE_LOAD_ERROR
            );
        }
    }

    #[test]
    fn startup_argument_is_an_exact_bounded_pair() {
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            parse_service_profile_argv(&[
                "webhook-lab.exe".into(),
                "--service-profile".into(),
                id.clone(),
            ])
            .unwrap(),
            Some(id.clone())
        );
        assert!(parse_service_profile_argv(&[
            "webhook-lab.exe".into(),
            "--service-profile".into(),
            id,
            "--extra".into(),
        ])
        .is_err());
        assert_eq!(
            parse_service_profile_argv(&["webhook-lab.exe".into()]).unwrap(),
            None
        );
    }
}
