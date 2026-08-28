//! Explicit, bounded JSON file transfer for API Playground collections/environments.
//!
//! The renderer supplies only an already-sanitized export document. The native side owns the
//! file picker, applies a byte bound, and uses atomic replacement so an interrupted transfer
//! cannot leave a partial export in the selected location.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

const MAX_TRANSFER_BYTES: usize = 1024 * 1024;
const MAX_FIELD_BYTES: usize = 64 * 1024;
const MAX_NAME_CHARS: usize = 120;
const MAX_COLLECTIONS: usize = 256;
const MAX_ENVIRONMENTS: usize = 64;
const MAX_VARIABLES: usize = 256;
const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const TRANSFER_ERROR: &str = "JSON 파일을 안전하게 처리하지 못했습니다";
const REDACTED: &str = "[REDACTED]";

#[tauri::command]
pub async fn read_json_file(app: AppHandle) -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("JSON", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| TRANSFER_ERROR.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| TRANSFER_ERROR.to_string())?;
    validate_file_path(&path, true)?;
    let bytes = tauri::async_runtime::spawn_blocking(move || read_bounded(&path))
        .await
        .map_err(|_| TRANSFER_ERROR.to_string())??;
    let content = Zeroizing::new(String::from_utf8(bytes).map_err(|_| TRANSFER_ERROR.to_string())?);
    validate_export_document(&content)?;
    Ok(Some(content.to_string()))
}

#[tauri::command]
pub async fn save_json_file(
    app: AppHandle,
    content: String,
    default_name: String,
) -> Result<bool, String> {
    let content = Zeroizing::new(content);
    validate_export_document(&content)?;
    let default_name = safe_default_name(&default_name);
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(default_name)
            .add_filter("JSON", &["json"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| TRANSFER_ERROR.to_string())?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let path = selected
        .into_path()
        .map_err(|_| TRANSFER_ERROR.to_string())?;
    validate_file_path(&path, false)?;
    tauri::async_runtime::spawn_blocking(move || {
        devbox_filesystem::atomic_write(path, content.as_bytes())
    })
    .await
    .map_err(|_| TRANSFER_ERROR.to_string())?
    .map_err(|_| TRANSFER_ERROR.to_string())?;
    Ok(true)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let identity = devbox_filesystem::filesystem_identity(path, false)
        .map_err(|_| TRANSFER_ERROR.to_string())?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| TRANSFER_ERROR.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_TRANSFER_BYTES as u64 {
        return Err(TRANSFER_ERROR.to_string());
    }
    let mut file = File::open(path).map_err(|_| TRANSFER_ERROR.to_string())?;
    if devbox_filesystem::filesystem_identity(path, false)
        .map_err(|_| TRANSFER_ERROR.to_string())?
        != identity
    {
        return Err(TRANSFER_ERROR.to_string());
    }
    let mut bytes = Vec::new();
    let mut limited = file.by_ref().take((MAX_TRANSFER_BYTES + 1) as u64);
    limited
        .read_to_end(&mut bytes)
        .map_err(|_| TRANSFER_ERROR.to_string())?;
    if bytes.len() > MAX_TRANSFER_BYTES {
        return Err(TRANSFER_ERROR.to_string());
    }
    Ok(bytes)
}

fn validate_export_document(content: &str) -> Result<(), String> {
    if content.len() > MAX_TRANSFER_BYTES {
        return invalid();
    }
    let document = serde_json::from_str::<serde_json::Value>(content).map_err(|_| error())?;
    let object = document.as_object().ok_or_else(error)?;
    let schema = object.get("schema").and_then(serde_json::Value::as_str);
    match schema {
        Some("devbox.api-playground.collection-export") => {
            exact_object(&document, &["schema", "schema_version", "collections"], &[])?;
            require_version(object)?;
            let collections = object
                .get("collections")
                .and_then(serde_json::Value::as_array)
                .filter(|items| items.len() <= MAX_COLLECTIONS)
                .ok_or_else(error)?;
            for collection in collections {
                validate_collection(collection)?;
            }
        }
        Some("devbox.api-playground.environment-export") => {
            exact_object(
                &document,
                &["schema", "schema_version", "environments"],
                &[],
            )?;
            require_version(object)?;
            let environments = object
                .get("environments")
                .and_then(serde_json::Value::as_array)
                .filter(|items| items.len() <= MAX_ENVIRONMENTS)
                .ok_or_else(error)?;
            for environment in environments {
                validate_environment(environment)?;
            }
        }
        _ => return invalid(),
    }
    Ok(())
}

fn validate_collection(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(
        value,
        &[
            "id",
            "name",
            "folder",
            "saved_at",
            "request",
            "requiresSecretReview",
        ],
        &[],
    )?;
    require_id(object.get("id"))?;
    require_name(object.get("name"))?;
    require_name(object.get("folder"))?;
    require_safe_integer(object.get("saved_at"))?;
    require_bool(object.get("requiresSecretReview"))?;
    validate_request(object.get("request").ok_or_else(error)?)
}

fn validate_request(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(
        value,
        &[
            "method",
            "url",
            "headers",
            "cookies",
            "multipart",
            "params",
            "body_kind",
            "body",
            "auth",
            "timeout_ms",
            "requiresSecretReview",
        ],
        &["graphql"],
    )?;
    let method = require_metadata(object.get("method"), 32)?;
    if method.is_empty() {
        return invalid();
    }
    let url = require_string(object.get("url"), MAX_FIELD_BYTES)?;
    validate_url(url)?;
    validate_array(object.get("headers"), 100, validate_header)?;
    validate_array(object.get("cookies"), 100, validate_cookie)?;
    validate_array(object.get("multipart"), 50, validate_multipart)?;
    validate_array(object.get("params"), 100, validate_key_value)?;
    let body_kind = require_metadata(object.get("body_kind"), 32)?;
    if !matches!(
        body_kind,
        "none" | "json" | "form" | "multipart" | "raw" | "graphql"
    ) {
        return invalid();
    }
    let body = require_string(object.get("body"), MAX_FIELD_BYTES)?;
    validate_body(body_kind, body)?;
    validate_auth(object.get("auth").ok_or_else(error)?)?;
    require_safe_integer(object.get("timeout_ms"))?;
    require_bool(object.get("requiresSecretReview"))?;
    if let Some(graphql) = object.get("graphql") {
        validate_graphql(graphql)?;
    }
    Ok(())
}

fn validate_header(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(value, &["key", "value"], &["enabled"])?;
    let key = require_metadata(object.get("key"), 256)?;
    let value = require_string(object.get("value"), MAX_FIELD_BYTES)?;
    validate_named_value(key, value)?;
    require_optional_bool(object.get("enabled"))
}

fn validate_cookie(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(value, &["name", "value"], &["enabled"])?;
    require_metadata(object.get("name"), 256)?;
    let value = require_string(object.get("value"), MAX_FIELD_BYTES)?;
    if !is_protected_value(value) {
        return invalid();
    }
    require_optional_bool(object.get("enabled"))
}

fn validate_key_value(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(value, &["key", "value"], &[])?;
    let key = require_metadata(object.get("key"), MAX_FIELD_BYTES)?;
    let value = require_string(object.get("value"), MAX_FIELD_BYTES)?;
    validate_named_value(key, value)
}

fn validate_multipart(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(
        value,
        &[
            "kind",
            "name",
            "value",
            "file_path",
            "file_name",
            "content_type",
        ],
        &["enabled"],
    )?;
    let kind = require_metadata(object.get("kind"), 16)?;
    let name = require_metadata(object.get("name"), 256)?;
    let value = require_string(object.get("value"), MAX_FIELD_BYTES)?;
    let file_path = require_string(object.get("file_path"), MAX_FIELD_BYTES)?;
    let file_name = require_metadata(object.get("file_name"), MAX_FIELD_BYTES)?;
    require_metadata(object.get("content_type"), 256)?;
    match kind {
        "file"
            if value.is_empty()
                && file_path.is_empty()
                && !file_name.contains(['/', '\\'])
                && !contains_known_secret(file_name) => {}
        "text" if file_path.is_empty() && file_name.is_empty() => {
            validate_named_value(name, value)?;
        }
        _ => return invalid(),
    }
    require_optional_bool(object.get("enabled"))
}

fn validate_auth(value: &serde_json::Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let object = exact_object(
        value,
        &[
            "kind",
            "username",
            "password",
            "token",
            "api_key",
            "api_value",
        ],
        &[],
    )?;
    require_metadata(object.get("kind"), 64)?;
    for field in ["username", "password", "token", "api_value"] {
        let value = require_string(object.get(field), MAX_FIELD_BYTES)?;
        if !is_protected_value(value) {
            return invalid();
        }
    }
    let api_key = require_metadata(object.get("api_key"), 256)?;
    reject_known_secret(api_key)
}

fn validate_graphql(value: &serde_json::Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let object = exact_object(value, &["query", "variables", "operation_name"], &[])?;
    let query = require_string(object.get("query"), MAX_FIELD_BYTES)?;
    reject_known_secret(query)?;
    let variables = require_string(object.get("variables"), MAX_FIELD_BYTES)?;
    if !variables.trim().is_empty() && !is_protected_value(variables) {
        let parsed = serde_json::from_str::<serde_json::Value>(variables).map_err(|_| error())?;
        validate_sanitized_json(&parsed, "")?;
    }
    let operation = require_metadata(object.get("operation_name"), 256)?;
    reject_known_secret(operation)
}

fn validate_environment(value: &serde_json::Value) -> Result<(), String> {
    let object = exact_object(value, &["id", "name", "variables"], &[])?;
    require_id(object.get("id"))?;
    require_name(object.get("name"))?;
    let variables = object
        .get("variables")
        .and_then(serde_json::Value::as_array)
        .filter(|items| items.len() <= MAX_VARIABLES)
        .ok_or_else(error)?;
    let mut keys = HashSet::with_capacity(variables.len());
    for variable in variables {
        let object = exact_object(variable, &["key", "reference", "secret"], &["value"])?;
        let key = require_string(object.get("key"), 128)?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            || !keys.insert(key)
        {
            return invalid();
        }
        let reference = require_string(object.get("reference"), 131)?;
        if reference != format!("${{{key}}}") {
            return invalid();
        }
        let secret = require_bool(object.get("secret"))?;
        if secret {
            if object.contains_key("value") {
                return invalid();
            }
        } else {
            let plain = require_string(object.get("value"), MAX_FIELD_BYTES)?;
            if is_sensitive_name(key) || contains_known_secret(plain) {
                return invalid();
            }
        }
    }
    Ok(())
}

fn validate_body(kind: &str, body: &str) -> Result<(), String> {
    reject_known_secret(body)?;
    match kind {
        "multipart" | "graphql" if !body.is_empty() => return invalid(),
        "json" if !body.trim().is_empty() => {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
                validate_sanitized_json(&value, "")?;
            } else {
                validate_malformed_body(body)?;
            }
        }
        "form" => {
            for line in body.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    validate_named_value(key.trim(), value)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_sanitized_json(value: &serde_json::Value, key: &str) -> Result<(), String> {
    if is_sensitive_name(key) {
        return match value.as_str() {
            Some(value) if is_protected_value(value) => Ok(()),
            _ => invalid(),
        };
    }
    match value {
        serde_json::Value::String(value) => reject_known_secret(value),
        serde_json::Value::Array(items) => {
            for item in items {
                validate_sanitized_json(item, "")?;
            }
            Ok(())
        }
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                validate_sanitized_json(value, key)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_malformed_body(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    for marker in [
        "authorization",
        "api_key",
        "api-key",
        "token",
        "secret",
        "password",
        "passwd",
        "private_key",
    ] {
        for offset in lower.match_indices(marker).map(|(offset, _)| offset) {
            let tail = &value[offset + marker.len()..];
            let tail = tail.trim_start_matches(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '\'')
            });
            let Some(tail) = tail.strip_prefix(':') else {
                return invalid();
            };
            let tail = tail.trim_start_matches(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '\'')
            });
            if !tail.starts_with(REDACTED)
                && !tail.starts_with("REDACTED")
                && !tail.starts_with("{{")
                && !tail.starts_with("${")
            {
                return invalid();
            }
        }
    }
    Ok(())
}

fn validate_url(value: &str) -> Result<(), String> {
    reject_known_secret(value)?;
    if let Ok(url) = reqwest::Url::parse(value) {
        if (!url.username().is_empty() && !is_protected_value(url.username()))
            || url
                .password()
                .is_some_and(|password| !is_protected_value(password))
        {
            return invalid();
        }
        for (key, value) in url.query_pairs() {
            validate_named_value(&key, &value)?;
        }
    }
    Ok(())
}

fn validate_named_value(name: &str, value: &str) -> Result<(), String> {
    if is_sensitive_name(name) && !is_protected_value(value) {
        return invalid();
    }
    reject_known_secret(value)
}

fn exact_object<'a>(
    value: &'a serde_json::Value,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a serde_json::Map<String, serde_json::Value>, String> {
    let object = value.as_object().ok_or_else(error)?;
    if !required.iter().all(|key| object.contains_key(*key))
        || object
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return invalid();
    }
    Ok(object)
}

fn validate_array(
    value: Option<&serde_json::Value>,
    max_items: usize,
    validate: fn(&serde_json::Value) -> Result<(), String>,
) -> Result<(), String> {
    let items = value
        .and_then(serde_json::Value::as_array)
        .filter(|items| items.len() <= max_items)
        .ok_or_else(error)?;
    for item in items {
        validate(item)?;
    }
    Ok(())
}

fn require_version(object: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    if object
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(1)
    {
        Ok(())
    } else {
        invalid()
    }
}

fn require_string(value: Option<&serde_json::Value>, max_bytes: usize) -> Result<&str, String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() <= max_bytes)
        .ok_or_else(error)
}

fn require_metadata(value: Option<&serde_json::Value>, max_bytes: usize) -> Result<&str, String> {
    require_string(value, max_bytes).and_then(|value| {
        if value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
            || contains_known_secret(value)
        {
            invalid()
        } else {
            Ok(value)
        }
    })
}

fn require_name(value: Option<&serde_json::Value>) -> Result<(), String> {
    let value = require_metadata(value, MAX_FIELD_BYTES)?;
    if value.chars().count() <= MAX_NAME_CHARS {
        Ok(())
    } else {
        invalid()
    }
}

fn require_id(value: Option<&serde_json::Value>) -> Result<(), String> {
    let value = require_metadata(value, 256)?;
    if value.is_empty() {
        invalid()
    } else {
        Ok(())
    }
}

fn require_safe_integer(value: Option<&serde_json::Value>) -> Result<(), String> {
    if value
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|value| value <= MAX_JS_SAFE_INTEGER)
    {
        Ok(())
    } else {
        invalid()
    }
}

fn require_bool(value: Option<&serde_json::Value>) -> Result<bool, String> {
    value.and_then(serde_json::Value::as_bool).ok_or_else(error)
}

fn require_optional_bool(value: Option<&serde_json::Value>) -> Result<(), String> {
    if value.is_none() || value.is_some_and(serde_json::Value::is_boolean) {
        Ok(())
    } else {
        invalid()
    }
}

fn is_sensitive_name(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "api-key",
        "api_key",
        "access-token",
        "access_token",
        "refresh-token",
        "refresh_token",
        "token",
        "secret",
        "password",
        "passwd",
        "private-key",
        "private_key",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_protected_value(value: &str) -> bool {
    if contains_known_secret(value) {
        return false;
    }
    value.is_empty() || value == REDACTED || value == "REDACTED" || is_exact_reference(value)
}

fn is_exact_reference(value: &str) -> bool {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
        .or_else(|| {
            trimmed
                .strip_prefix("${")
                .and_then(|value| value.strip_suffix('}'))
        });
    inner.is_some_and(|inner| {
        let inner = inner.trim();
        !inner.is_empty()
            && inner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    })
}

fn contains_known_secret(value: &str) -> bool {
    if value.contains("-----BEGIN") && value.contains("PRIVATE KEY-----") {
        return true;
    }
    value
        .split(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '=' | ':' | ',' | '&')
        })
        .filter(|candidate| !candidate.is_empty())
        .any(looks_like_secret)
}

fn looks_like_secret(value: &str) -> bool {
    let prefixed = [
        "sk-",
        "sk_",
        "ghp_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxa-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
    ]
    .iter()
    .any(|prefix| {
        value
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.len() >= 12 && tail.bytes().all(is_token_byte))
    });
    if prefixed {
        return true;
    }
    if value.len() == 20
        && value.starts_with("AKIA")
        && value
            .bytes()
            .skip(4)
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return true;
    }
    let jwt = value.split('.').collect::<Vec<_>>();
    jwt.len() == 3
        && jwt
            .iter()
            .all(|part| part.len() >= 10 && part.bytes().all(is_token_byte))
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

fn reject_known_secret(value: &str) -> Result<(), String> {
    if contains_known_secret(value) {
        invalid()
    } else {
        Ok(())
    }
}

fn invalid<T>() -> Result<T, String> {
    Err(error())
}

fn error() -> String {
    TRANSFER_ERROR.to_string()
}

fn safe_default_name(value: &str) -> String {
    if value.len() <= 64
        && !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && value.ends_with(".json")
    {
        value.to_string()
    } else {
        "api-playground-export.json".to_string()
    }
}

fn validate_file_path(path: &Path, must_exist: bool) -> Result<(), String> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(TRANSFER_ERROR.to_string());
    };
    if file_name.is_empty()
        || file_name.len() > 255
        || file_name
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
        || path
            .parent()
            .is_none_or(|parent| !has_safe_directory_chain(parent))
    {
        return Err(TRANSFER_ERROR.to_string());
    }
    let target = devbox_filesystem::filesystem_identity(path, false);
    if (must_exist && target.is_err())
        || (!must_exist && std::fs::symlink_metadata(path).is_ok() && target.is_err())
    {
        return Err(TRANSFER_ERROR.to_string());
    }
    Ok(())
}

pub(crate) fn has_safe_directory_chain(path: &Path) -> bool {
    let mut current = Some(path);
    while let Some(directory) = current {
        if devbox_filesystem::filesystem_identity(directory, true).is_err() {
            return false;
        }
        let parent = directory.parent();
        if parent == Some(directory) {
            break;
        }
        current = parent;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_never_accepts_a_path_or_unsafe_extension() {
        assert_eq!(safe_default_name("collections.json"), "collections.json");
        assert_eq!(
            safe_default_name("../secrets.json"),
            "api-playground-export.json"
        );
        assert_eq!(
            safe_default_name("collection.txt"),
            "api-playground-export.json"
        );
        assert_eq!(safe_default_name(""), "api-playground-export.json");
    }

    #[test]
    fn output_path_requires_a_real_parent_and_filename() {
        let valid = std::env::temp_dir().join("api-playground-transfer.json");
        assert!(validate_file_path(&valid, false).is_ok());
        assert!(validate_file_path(Path::new("/definitely/missing/transfer.json"), false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn input_and_output_reject_linked_targets_and_parent_chains() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let real_parent = root.path().join("real");
        std::fs::create_dir(&real_parent).unwrap();
        let linked_parent = root.path().join("linked-parent");
        symlink(&real_parent, &linked_parent).unwrap();
        assert!(validate_file_path(&linked_parent.join("output.json"), false).is_err());

        let real_file = real_parent.join("real.json");
        std::fs::write(&real_file, b"{}").unwrap();
        let linked_file = real_parent.join("linked.json");
        symlink(&real_file, &linked_file).unwrap();
        assert!(validate_file_path(&linked_file, true).is_err());
    }

    #[test]
    fn native_validator_accepts_only_exact_sanitized_collection_shape() {
        let safe = collection_document(REDACTED);
        assert!(validate_export_document(&safe.to_string()).is_ok());

        let plaintext = collection_document("plain-password");
        assert!(validate_export_document(&plaintext.to_string()).is_err());
        let mixed_reference = collection_document("prefix-${TOKEN}");
        assert!(validate_export_document(&mixed_reference.to_string()).is_err());

        let mut malformed = collection_document(REDACTED);
        malformed["collections"][0]["request"]["body"] =
            serde_json::json!(r#""password":"plain","note":"[REDACTED]""#);
        assert!(validate_export_document(&malformed.to_string()).is_err());
        malformed["collections"][0]["request"]["body"] =
            serde_json::json!(r#""password":"[REDACTED]""#);
        assert!(validate_export_document(&malformed.to_string()).is_ok());

        let mut unknown = safe;
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), serde_json::json!(true));
        assert!(validate_export_document(&unknown.to_string()).is_err());

        let mut path = collection_document(REDACTED);
        path["collections"][0]["request"]["multipart"] = serde_json::json!([{
            "kind": "file",
            "name": "upload",
            "value": "",
            "file_path": "/tmp/secret.txt",
            "file_name": "secret.txt",
            "content_type": "text/plain"
        }]);
        assert!(validate_export_document(&path.to_string()).is_err());
    }

    #[test]
    fn native_validator_rejects_environment_secret_downgrade_and_known_tokens() {
        let safe = serde_json::json!({
            "schema": "devbox.api-playground.environment-export",
            "schema_version": 1,
            "environments": [{
                "id": "env-1",
                "name": "local",
                "variables": [
                    {"key": "TOKEN", "reference": "${TOKEN}", "secret": true},
                    {"key": "REGION", "reference": "${REGION}", "secret": false, "value": "kr"}
                ]
            }]
        });
        assert!(validate_export_document(&safe.to_string()).is_ok());

        let downgraded = serde_json::json!({
            "schema": "devbox.api-playground.environment-export",
            "schema_version": 1,
            "environments": [{
                "id": "env-1",
                "name": "local",
                "variables": [{
                    "key": "TOKEN",
                    "reference": "${TOKEN}",
                    "secret": false,
                    "value": "ghp_12345678901234567890"
                }]
            }]
        });
        assert!(validate_export_document(&downgraded.to_string()).is_err());
    }

    fn collection_document(password: &str) -> serde_json::Value {
        serde_json::json!({
            "schema": "devbox.api-playground.collection-export",
            "schema_version": 1,
            "collections": [{
                "id": "collection-1",
                "name": "request",
                "folder": "",
                "saved_at": 1,
                "requiresSecretReview": true,
                "request": {
                    "method": "POST",
                    "url": "https://example.com/api",
                    "headers": [{"key": "Authorization", "value": password, "enabled": true}],
                    "cookies": [],
                    "multipart": [],
                    "params": [],
                    "body_kind": "json",
                    "body": "{\"password\":\"[REDACTED]\"}",
                    "auth": null,
                    "timeout_ms": 30000,
                    "requiresSecretReview": true
                }
            }]
        })
    }
}
