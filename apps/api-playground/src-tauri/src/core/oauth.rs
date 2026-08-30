//! Pure OAuth discovery, PKCE, callback, and token-response validation.
//!
//! Network I/O, browser launch, DPAPI, and persistence stay in the command
//! layer. This module treats every decoded field as hostile and returns only
//! stable error codes.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Url;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;
use zeroize::{Zeroize, Zeroizing};

pub const REQUEST_INVALID: &str = "mcp_oauth_request_invalid";
pub const DISCOVERY_FAILED: &str = "mcp_oauth_discovery_failed";
pub const RESOURCE_MISMATCH: &str = "mcp_oauth_resource_mismatch";
pub const ISSUER_MISMATCH: &str = "mcp_oauth_issuer_mismatch";
pub const PKCE_REQUIRED: &str = "mcp_oauth_pkce_required";
pub const CLIENT_UNSUPPORTED: &str = "mcp_oauth_client_unsupported";
pub const CALLBACK_FAILED: &str = "mcp_oauth_callback_failed";
pub const TOKEN_FAILED: &str = "mcp_oauth_token_failed";

pub const MAX_URL_BYTES: usize = 8 * 1024;
pub const MAX_METADATA_BYTES: usize = 128 * 1024;
pub const MAX_TOKEN_RESPONSE_BYTES: usize = 128 * 1024;
pub const MAX_TOKEN_BYTES: usize = 64 * 1024;
pub const MAX_SCOPES: usize = 32;
pub const MAX_SCOPE_BYTES: usize = 256;
pub const CALLBACK_PATH: &str = "/oauth/callback";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub authorization_response_iss_parameter_supported: bool,
}

pub struct TokenResponse {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_in: Option<u64>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerChallenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<Vec<String>>,
}

pub struct CallbackAuthorization {
    pub code: Zeroizing<String>,
}

pub fn normalize_resource(input: &str) -> Result<(Url, String), &'static str> {
    let url = validate_secure_url(input, true).map_err(|_| REQUEST_INVALID)?;
    let mut canonical = url.origin().ascii_serialization();
    if url.path() != "/" {
        canonical.push_str(url.path());
    }
    if let Some(query) = url.query() {
        canonical.push('?');
        canonical.push_str(query);
    }
    Ok((url, canonical))
}

pub fn normalize_issuer(input: &str) -> Result<(Url, String), &'static str> {
    let url = validate_secure_url(input, true).map_err(|_| REQUEST_INVALID)?;
    if url.query().is_some() {
        return Err(REQUEST_INVALID);
    }
    let canonical = if url.path() == "/" {
        url.origin().ascii_serialization()
    } else {
        format!("{}{}", url.origin().ascii_serialization(), url.path())
    };
    Ok((url, canonical))
}

pub fn validate_secure_url(input: &str, allow_loopback_http: bool) -> Result<Url, &'static str> {
    if input.is_empty()
        || input.len() > MAX_URL_BYTES
        || input.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(REQUEST_INVALID);
    }
    let url = Url::parse(input).map_err(|_| REQUEST_INVALID)?;
    if url.host_str().is_none()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url
            .query_pairs()
            .any(|(name, _)| is_sensitive_query_name(&name))
    {
        return Err(REQUEST_INVALID);
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if allow_loopback_http && url_host_is_loopback(&url) => Ok(url),
        _ => Err(REQUEST_INVALID),
    }
}

fn is_sensitive_query_name(name: &str) -> bool {
    let compact = name.to_ascii_lowercase().replace(['_', '-'], "");
    compact.contains("authorization")
        || compact.contains("cookie")
        || compact.contains("apikey")
        || compact.contains("apivalue")
        || compact.contains("token")
        || compact.contains("secret")
        || compact.contains("password")
        || compact.contains("passwd")
        || compact.contains("privatekey")
        || compact.contains("username")
}

pub fn url_host_is_loopback(url: &Url) -> bool {
    url.host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|address| address.is_loopback())
}

pub fn protected_resource_candidates(resource: &Url) -> Result<Vec<Url>, &'static str> {
    let mut path_specific = resource.clone();
    path_specific.set_query(None);
    path_specific.set_fragment(None);
    let suffix = if resource.path() == "/" {
        String::new()
    } else {
        resource.path().to_string()
    };
    path_specific.set_path(&format!("/.well-known/oauth-protected-resource{suffix}"));
    let mut root = resource.clone();
    root.set_query(None);
    root.set_fragment(None);
    root.set_path("/.well-known/oauth-protected-resource");
    if path_specific == root {
        Ok(vec![root])
    } else {
        Ok(vec![path_specific, root])
    }
}

pub fn authorization_server_candidates(issuer: &Url) -> Result<Vec<Url>, &'static str> {
    let suffix = if issuer.path() == "/" {
        String::new()
    } else {
        issuer.path().to_string()
    };
    let mut oauth = issuer.clone();
    oauth.set_path(&format!("/.well-known/oauth-authorization-server{suffix}"));
    let mut oidc_inserted = issuer.clone();
    oidc_inserted.set_path(&format!("/.well-known/openid-configuration{suffix}"));
    if suffix.is_empty() {
        return Ok(vec![oauth, oidc_inserted]);
    }
    let mut oidc_appended = issuer.clone();
    oidc_appended.set_path(&format!(
        "{}/.well-known/openid-configuration",
        issuer.path().trim_end_matches('/')
    ));
    Ok(vec![oauth, oidc_inserted, oidc_appended])
}

pub fn parse_bearer_challenge(values: &[String]) -> Result<BearerChallenge, &'static str> {
    let total = values.iter().try_fold(0usize, |total, value| {
        total.checked_add(value.len()).ok_or(DISCOVERY_FAILED)
    })?;
    if total > 32 * 1024 {
        return Err(DISCOVERY_FAILED);
    }
    let mut resource_metadata = None;
    let mut scope = None;
    let mut bearer_seen = false;
    for value in values {
        let Some(index) = find_bearer_start(value)? else {
            continue;
        };
        if std::mem::replace(&mut bearer_seen, true) {
            return Err(DISCOVERY_FAILED);
        }
        for (name, parameter) in parse_challenge_parameters(&value[index + 6..])? {
            match name.to_ascii_lowercase().as_str() {
                "resource_metadata" => {
                    if resource_metadata.replace(parameter).is_some() {
                        return Err(DISCOVERY_FAILED);
                    }
                }
                "scope" => {
                    if scope.is_some() {
                        return Err(DISCOVERY_FAILED);
                    }
                    scope = Some(
                        validate_scopes(
                            &parameter
                                .split_ascii_whitespace()
                                .map(ToOwned::to_owned)
                                .collect::<Vec<_>>(),
                        )
                        .map_err(|_| DISCOVERY_FAILED)?,
                    );
                }
                _ => {}
            }
        }
    }
    Ok(BearerChallenge {
        resource_metadata,
        scope,
    })
}

fn find_bearer_start(value: &str) -> Result<Option<usize>, &'static str> {
    let bytes = value.as_bytes();
    let mut quoted = false;
    let mut escaped = false;
    let mut found = None;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            quoted = true;
            index += 1;
            continue;
        }
        let before_ok =
            index == 0 || bytes[index - 1].is_ascii_whitespace() || bytes[index - 1] == b',';
        let after = bytes.get(index + 6).copied();
        let after_ok = after.is_none_or(|next| next.is_ascii_whitespace());
        if before_ok
            && after_ok
            && bytes
                .get(index..index + 6)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(b"bearer"))
        {
            if found.replace(index).is_some() {
                return Err(DISCOVERY_FAILED);
            }
            index += 6;
        } else {
            index += 1;
        }
    }
    Ok(found)
}

fn parse_challenge_parameters(input: &str) -> Result<Vec<(String, String)>, &'static str> {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    let mut output = Vec::new();
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b',') {
            index += 1;
        }
        let name_start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'-'))
        {
            index += 1;
        }
        if name_start == index {
            break;
        }
        let name = &input[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            break;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let value = if bytes.get(index) == Some(&b'"') {
            index += 1;
            let mut decoded = String::new();
            let mut closed = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    b'\\' => {
                        index += 1;
                        let escaped = *bytes.get(index).ok_or(DISCOVERY_FAILED)?;
                        if escaped.is_ascii_control() {
                            return Err(DISCOVERY_FAILED);
                        }
                        decoded.push(char::from(escaped));
                        index += 1;
                    }
                    byte if byte.is_ascii_control() || !byte.is_ascii() => {
                        return Err(DISCOVERY_FAILED)
                    }
                    byte => {
                        decoded.push(char::from(byte));
                        index += 1;
                    }
                }
            }
            if !closed {
                return Err(DISCOVERY_FAILED);
            }
            decoded
        } else {
            let start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b','
            {
                if bytes[index].is_ascii_control() || !bytes[index].is_ascii() {
                    return Err(DISCOVERY_FAILED);
                }
                index += 1;
            }
            input[start..index].to_string()
        };
        if value.is_empty() || value.len() > MAX_URL_BYTES {
            return Err(DISCOVERY_FAILED);
        }
        output.push((name.to_string(), value));
    }
    Ok(output)
}

pub fn parse_protected_resource_metadata(
    bytes: &[u8],
    expected_resource: &str,
) -> Result<ProtectedResourceMetadata, &'static str> {
    let value = parse_unique_json(bytes, MAX_METADATA_BYTES, DISCOVERY_FAILED)?;
    let object = value.as_object().ok_or(DISCOVERY_FAILED)?;
    let resource = required_string(object, "resource", MAX_URL_BYTES, DISCOVERY_FAILED)?;
    let (_, normalized_resource) = normalize_resource(resource).map_err(|_| DISCOVERY_FAILED)?;
    if normalized_resource != expected_resource {
        return Err(RESOURCE_MISMATCH);
    }
    let servers = required_string_array(
        object,
        "authorization_servers",
        8,
        MAX_URL_BYTES,
        DISCOVERY_FAILED,
    )?;
    if servers.is_empty() {
        return Err(DISCOVERY_FAILED);
    }
    let mut seen = BTreeSet::new();
    let mut authorization_servers = Vec::with_capacity(servers.len());
    for server in servers {
        let (_, normalized) = normalize_issuer(&server).map_err(|_| DISCOVERY_FAILED)?;
        if !seen.insert(normalized.clone()) {
            return Err(DISCOVERY_FAILED);
        }
        authorization_servers.push(normalized);
    }
    let scopes_supported = optional_string_array(
        object,
        "scopes_supported",
        MAX_SCOPES,
        MAX_SCOPE_BYTES,
        DISCOVERY_FAILED,
    )?
    .map(|scopes| validate_scopes(&scopes).map_err(|_| DISCOVERY_FAILED))
    .transpose()?
    .unwrap_or_default();
    Ok(ProtectedResourceMetadata {
        resource: normalized_resource,
        authorization_servers,
        scopes_supported,
    })
}

pub fn select_issuer(
    advertised: &[String],
    requested: Option<&str>,
) -> Result<String, &'static str> {
    if advertised.is_empty() {
        return Err(DISCOVERY_FAILED);
    }
    match requested {
        Some(requested) => {
            let (_, normalized) = normalize_issuer(requested).map_err(|_| REQUEST_INVALID)?;
            advertised
                .iter()
                .any(|issuer| issuer == &normalized)
                .then_some(normalized)
                .ok_or(ISSUER_MISMATCH)
        }
        None if advertised.len() == 1 => Ok(advertised[0].clone()),
        None => Err(ISSUER_MISMATCH),
    }
}

pub fn parse_authorization_server_metadata(
    bytes: &[u8],
    expected_issuer: &str,
) -> Result<AuthorizationServerMetadata, &'static str> {
    let value = parse_unique_json(bytes, MAX_METADATA_BYTES, DISCOVERY_FAILED)?;
    let object = value.as_object().ok_or(DISCOVERY_FAILED)?;
    let issuer = required_string(object, "issuer", MAX_URL_BYTES, DISCOVERY_FAILED)?;
    let (_, normalized_issuer) = normalize_issuer(issuer).map_err(|_| DISCOVERY_FAILED)?;
    if normalized_issuer != expected_issuer {
        return Err(ISSUER_MISMATCH);
    }
    let authorization_endpoint = validate_endpoint_field(object, "authorization_endpoint")?;
    let token_endpoint = validate_endpoint_field(object, "token_endpoint")?;
    let revocation_endpoint = object
        .get("revocation_endpoint")
        .map(|_| validate_endpoint_field(object, "revocation_endpoint"))
        .transpose()?;

    let pkce = required_string_array(
        object,
        "code_challenge_methods_supported",
        16,
        64,
        DISCOVERY_FAILED,
    )?;
    if !pkce.iter().any(|method| method == "S256") {
        return Err(PKCE_REQUIRED);
    }
    let token_auth = optional_string_array(
        object,
        "token_endpoint_auth_methods_supported",
        16,
        128,
        DISCOVERY_FAILED,
    )?
    .unwrap_or_else(|| vec!["client_secret_basic".into()]);
    if !token_auth.iter().any(|method| method == "none") {
        return Err(CLIENT_UNSUPPORTED);
    }
    let response_types = required_string_array(
        object,
        "response_types_supported",
        16,
        128,
        DISCOVERY_FAILED,
    )?;
    if !response_types.iter().any(|value| value == "code") {
        return Err(CLIENT_UNSUPPORTED);
    }
    if let Some(grants) =
        optional_string_array(object, "grant_types_supported", 16, 128, DISCOVERY_FAILED)?
    {
        if !grants.iter().any(|value| value == "authorization_code") {
            return Err(CLIENT_UNSUPPORTED);
        }
    }
    let authorization_response_iss_parameter_supported =
        match object.get("authorization_response_iss_parameter_supported") {
            Some(value) => value.as_bool().ok_or(DISCOVERY_FAILED)?,
            None => false,
        };
    Ok(AuthorizationServerMetadata {
        issuer: normalized_issuer,
        authorization_endpoint,
        token_endpoint,
        revocation_endpoint,
        authorization_response_iss_parameter_supported,
    })
}

fn validate_endpoint_field(
    object: &Map<String, Value>,
    name: &str,
) -> Result<String, &'static str> {
    let value = required_string(object, name, MAX_URL_BYTES, DISCOVERY_FAILED)?;
    validate_secure_url(value, true)
        .map(|url| url.to_string())
        .map_err(|_| DISCOVERY_FAILED)
}

pub fn validate_client_id(client_id: &str) -> Result<(), &'static str> {
    if client_id.is_empty()
        || client_id.len() > MAX_URL_BYTES
        || client_id.chars().any(char::is_control)
    {
        Err(REQUEST_INVALID)
    } else {
        Ok(())
    }
}

pub fn validate_scopes(scopes: &[String]) -> Result<Vec<String>, &'static str> {
    if scopes.len() > MAX_SCOPES {
        return Err(REQUEST_INVALID);
    }
    let mut seen = BTreeSet::new();
    let mut validated = Vec::with_capacity(scopes.len());
    for scope in scopes {
        if scope.is_empty()
            || scope.len() > MAX_SCOPE_BYTES
            || !scope
                .bytes()
                .all(|byte| byte == b'!' || matches!(byte, b'#'..=b'[' | b']'..=b'~'))
            || !seen.insert(scope.clone())
        {
            return Err(REQUEST_INVALID);
        }
        validated.push(scope.clone());
    }
    Ok(validated)
}

pub fn generate_state_and_pkce() -> Result<(Zeroizing<String>, Zeroizing<String>, String), ()> {
    let mut state_bytes = Zeroizing::new([0_u8; 32]);
    let mut verifier_bytes = Zeroizing::new([0_u8; 32]);
    getrandom::fill(&mut state_bytes[..]).map_err(|_| ())?;
    getrandom::fill(&mut verifier_bytes[..]).map_err(|_| ())?;
    let state = Zeroizing::new(URL_SAFE_NO_PAD.encode(&state_bytes[..]));
    let verifier = Zeroizing::new(URL_SAFE_NO_PAD.encode(&verifier_bytes[..]));
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Ok((state, verifier, challenge))
}

pub struct AuthorizationUrlInput<'a> {
    pub endpoint: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub challenge: &'a str,
    pub resource: &'a str,
    pub scopes: &'a [String],
}

pub fn build_authorization_url(input: AuthorizationUrlInput<'_>) -> Result<Url, &'static str> {
    validate_client_id(input.client_id)?;
    let mut endpoint = validate_secure_url(input.endpoint, true).map_err(|_| DISCOVERY_FAILED)?;
    let reserved = [
        "response_type",
        "client_id",
        "redirect_uri",
        "state",
        "code_challenge",
        "code_challenge_method",
        "resource",
        "scope",
    ];
    if endpoint
        .query_pairs()
        .any(|(name, _)| reserved.contains(&name.as_ref()))
    {
        return Err(DISCOVERY_FAILED);
    }
    let scopes = validate_scopes(input.scopes)?;
    let mut query = endpoint.query_pairs_mut();
    query
        .append_pair("response_type", "code")
        .append_pair("client_id", input.client_id)
        .append_pair("redirect_uri", input.redirect_uri)
        .append_pair("state", input.state)
        .append_pair("code_challenge", input.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("resource", input.resource);
    if !scopes.is_empty() {
        query.append_pair("scope", &scopes.join(" "));
    }
    drop(query);
    if endpoint.as_str().len() > MAX_URL_BYTES * 2 {
        return Err(REQUEST_INVALID);
    }
    Ok(endpoint)
}

pub fn parse_callback_request(
    bytes: &[u8],
    expected_state: &str,
    expected_issuer: &str,
    require_issuer: bool,
) -> Result<CallbackAuthorization, &'static str> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 || bytes.contains(&0) {
        return Err(CALLBACK_FAILED);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CALLBACK_FAILED)?;
    let header_end = text.find("\r\n\r\n").ok_or(CALLBACK_FAILED)?;
    if header_end + 4 != text.len() {
        return Err(CALLBACK_FAILED);
    }
    let request_line = text[..header_end]
        .split("\r\n")
        .next()
        .ok_or(CALLBACK_FAILED)?;
    let mut parts = request_line.split(' ');
    let method = parts.next().ok_or(CALLBACK_FAILED)?;
    let target = parts.next().ok_or(CALLBACK_FAILED)?;
    let version = parts.next().ok_or(CALLBACK_FAILED)?;
    if parts.next().is_some() || method != "GET" || version != "HTTP/1.1" {
        return Err(CALLBACK_FAILED);
    }
    if target.contains('#') {
        return Err(CALLBACK_FAILED);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return Err(CALLBACK_FAILED);
    }
    let mut code: Option<Zeroizing<String>> = None;
    let mut state: Option<Zeroizing<String>> = None;
    let mut issuer = None;
    let mut error_seen = false;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "code" => {
                if code.replace(Zeroizing::new(value.into_owned())).is_some() {
                    return Err(CALLBACK_FAILED);
                }
            }
            "state" => {
                if state.replace(Zeroizing::new(value.into_owned())).is_some() {
                    return Err(CALLBACK_FAILED);
                }
            }
            "iss" => {
                if issuer.replace(value.into_owned()).is_some() {
                    return Err(CALLBACK_FAILED);
                }
            }
            "error" => {
                if std::mem::replace(&mut error_seen, true) {
                    return Err(CALLBACK_FAILED);
                }
                let mut ignored = value.into_owned();
                ignored.zeroize();
            }
            _ => {
                if let std::borrow::Cow::Owned(mut ignored) = value {
                    ignored.zeroize();
                }
            }
        }
    }
    if error_seen || state.as_deref().map(String::as_str) != Some(expected_state) {
        return Err(CALLBACK_FAILED);
    }
    match issuer.as_deref() {
        Some(value) if value != expected_issuer => return Err(ISSUER_MISMATCH),
        None if require_issuer => return Err(ISSUER_MISMATCH),
        _ => {}
    }
    let code = code.ok_or(CALLBACK_FAILED)?;
    if code.is_empty() || code.len() > MAX_URL_BYTES || code.chars().any(char::is_control) {
        return Err(CALLBACK_FAILED);
    }
    Ok(CallbackAuthorization { code })
}

pub fn parse_token_response(bytes: &[u8]) -> Result<TokenResponse, &'static str> {
    let value = parse_unique_json(bytes, MAX_TOKEN_RESPONSE_BYTES, TOKEN_FAILED)?;
    let Value::Object(mut object) = value else {
        return Err(TOKEN_FAILED);
    };
    let token_type = take_string(&mut object, "token_type", 64, TOKEN_FAILED)?;
    if !token_type.eq_ignore_ascii_case("bearer") {
        return Err(TOKEN_FAILED);
    }
    let access = Zeroizing::new(take_string(
        &mut object,
        "access_token",
        MAX_TOKEN_BYTES,
        TOKEN_FAILED,
    )?);
    validate_token(&access)?;
    let refresh =
        take_optional_string(&mut object, "refresh_token", MAX_TOKEN_BYTES, TOKEN_FAILED)?
            .map(Zeroizing::new);
    if let Some(value) = &refresh {
        validate_token(value)?;
    }
    let expires_in = object
        .remove("expires_in")
        .map(|value| value.as_u64().ok_or(TOKEN_FAILED))
        .transpose()?;
    if expires_in.is_some_and(|seconds| seconds == 0 || seconds > 366 * 24 * 60 * 60) {
        return Err(TOKEN_FAILED);
    }
    let scopes = object
        .remove("scope")
        .map(|value| match value {
            Value::String(value) => Ok(value),
            _ => Err(TOKEN_FAILED),
        })
        .transpose()?
        .map(|value| {
            validate_scopes(
                &value
                    .split_ascii_whitespace()
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| TOKEN_FAILED)
        })
        .transpose()?;
    let mut ignored_extensions = Value::Object(object);
    zeroize_json_value(&mut ignored_extensions);
    Ok(TokenResponse {
        access_token: access,
        refresh_token: refresh,
        expires_in,
        scopes,
    })
}

fn zeroize_json_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => {
            for value in values {
                zeroize_json_value(value);
            }
        }
        Value::Object(object) => {
            for (mut key, mut value) in std::mem::take(object) {
                key.zeroize();
                zeroize_json_value(&mut value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    *value = Value::Null;
}

fn take_string(
    object: &mut Map<String, Value>,
    name: &str,
    limit: usize,
    error: &'static str,
) -> Result<String, &'static str> {
    let value = match object.remove(name).ok_or(error)? {
        Value::String(value) => value,
        _ => return Err(error),
    };
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(error);
    }
    Ok(value)
}

fn take_optional_string(
    object: &mut Map<String, Value>,
    name: &str,
    limit: usize,
    error: &'static str,
) -> Result<Option<String>, &'static str> {
    let Some(value) = object.remove(name) else {
        return Ok(None);
    };
    let value = match value {
        Value::String(value) => value,
        _ => return Err(error),
    };
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(error);
    }
    Ok(Some(value))
}

fn validate_token(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > MAX_TOKEN_BYTES
        || value.chars().any(|character| character.is_control())
    {
        Err(TOKEN_FAILED)
    } else {
        Ok(())
    }
}

pub fn parse_unique_json(
    bytes: &[u8],
    limit: usize,
    error: &'static str,
) -> Result<Value, &'static str> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(error);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = UniqueValue::deserialize(&mut deserializer)
        .map_err(|_| error)?
        .0;
    deserializer.end().map_err(|_| error)?;
    Ok(value)
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    limit: usize,
    error: &'static str,
) -> Result<&'a str, &'static str> {
    let value = object.get(name).and_then(Value::as_str).ok_or(error)?;
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(error);
    }
    Ok(value)
}

fn required_string_array(
    object: &Map<String, Value>,
    name: &str,
    count: usize,
    bytes: usize,
    error: &'static str,
) -> Result<Vec<String>, &'static str> {
    optional_string_array(object, name, count, bytes, error)?.ok_or(error)
}

fn optional_string_array(
    object: &Map<String, Value>,
    name: &str,
    count: usize,
    bytes: usize,
    error: &'static str,
) -> Result<Option<Vec<String>>, &'static str> {
    let Some(value) = object.get(name) else {
        return Ok(None);
    };
    let values = value.as_array().ok_or(error)?;
    if values.len() > count {
        return Err(error);
    }
    let mut output = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for value in values {
        let value = value.as_str().ok_or(error)?;
        if value.is_empty()
            || value.len() > bytes
            || value.chars().any(char::is_control)
            || !seen.insert(value.to_string())
        {
            return Err(error);
        }
        output.push(value.to_string());
    }
    Ok(Some(output))
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        UniqueValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
            if values.insert(key, value.0).is_some() {
                return Err(de::Error::custom("duplicate JSON object key"));
            }
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_candidates_follow_mcp_path_order() {
        let resource = Url::parse("https://example.com/public/mcp").unwrap();
        assert_eq!(
            protected_resource_candidates(&resource)
                .unwrap()
                .into_iter()
                .map(|url| url.to_string())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/.well-known/oauth-protected-resource/public/mcp",
                "https://example.com/.well-known/oauth-protected-resource",
            ]
        );
        let issuer = Url::parse("https://auth.example.com/tenant1").unwrap();
        assert_eq!(
            authorization_server_candidates(&issuer)
                .unwrap()
                .into_iter()
                .map(|url| url.to_string())
                .collect::<Vec<_>>(),
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example.com/.well-known/openid-configuration/tenant1",
                "https://auth.example.com/tenant1/.well-known/openid-configuration",
            ]
        );
    }

    #[test]
    fn metadata_rejects_duplicate_fields_and_binding_mismatch() {
        assert_eq!(
            parse_protected_resource_metadata(
                br#"{"resource":"https://mcp.example","resource":"https://evil.example","authorization_servers":["https://auth.example"]}"#,
                "https://mcp.example",
            ),
            Err(DISCOVERY_FAILED)
        );
        assert_eq!(
            parse_protected_resource_metadata(
                br#"{"resource":"https://evil.example","authorization_servers":["https://auth.example"]}"#,
                "https://mcp.example",
            ),
            Err(RESOURCE_MISMATCH)
        );
        assert_eq!(
            parse_protected_resource_metadata(
                br#"{"resource":"https://mcp.example","authorization_servers":["https://auth.example"],"scopes_supported":["bad scope"]}"#,
                "https://mcp.example",
            ),
            Err(DISCOVERY_FAILED)
        );
    }

    #[test]
    fn bearer_challenge_parses_quoted_binding_and_rejects_duplicates() {
        let challenge = parse_bearer_challenge(&[String::from(
            r#"Basic realm="legacy", Bearer resource_metadata="https://mcp.example/.well-known/oauth-protected-resource", scope="read write""#,
        )])
        .unwrap();
        assert_eq!(
            challenge.resource_metadata.as_deref(),
            Some("https://mcp.example/.well-known/oauth-protected-resource")
        );
        assert_eq!(challenge.scope.unwrap(), vec!["read", "write"]);

        assert_eq!(
            parse_bearer_challenge(&[
                String::from("Bearer scope=read"),
                String::from("Bearer scope=write"),
            ]),
            Err(DISCOVERY_FAILED)
        );
        assert_eq!(
            parse_bearer_challenge(&[String::from("Bearer scope=read, Bearer scope=write",)]),
            Err(DISCOVERY_FAILED)
        );
        assert_eq!(
            parse_bearer_challenge(&[String::from("Basic realm=\"not a bearer challenge\"",)])
                .unwrap(),
            BearerChallenge {
                resource_metadata: None,
                scope: None,
            }
        );
        assert_eq!(
            parse_bearer_challenge(&[String::from(r#"Bearer scope="bad\"scope""#)]),
            Err(DISCOVERY_FAILED)
        );
    }

    #[test]
    fn authorization_metadata_requires_exact_issuer_pkce_and_public_client() {
        let valid = br#"{
          "issuer":"https://auth.example/tenant",
          "authorization_endpoint":"https://auth.example/authorize",
          "token_endpoint":"https://auth.example/token",
          "response_types_supported":["code"],
          "grant_types_supported":["authorization_code","refresh_token"],
          "code_challenge_methods_supported":["S256"],
          "token_endpoint_auth_methods_supported":["none"],
          "authorization_response_iss_parameter_supported":true
        }"#;
        let metadata =
            parse_authorization_server_metadata(valid, "https://auth.example/tenant").unwrap();
        assert!(metadata.authorization_response_iss_parameter_supported);
        assert_eq!(
            parse_authorization_server_metadata(valid, "https://other.example"),
            Err(ISSUER_MISMATCH)
        );
        let trailing_slash = String::from_utf8(valid.to_vec()).unwrap().replace(
            "https://auth.example/tenant",
            "https://auth.example/tenant/",
        );
        assert_eq!(
            parse_authorization_server_metadata(
                trailing_slash.as_bytes(),
                "https://auth.example/tenant",
            ),
            Err(ISSUER_MISMATCH)
        );
        let no_pkce = String::from_utf8(valid.to_vec())
            .unwrap()
            .replace("[\"S256\"]", "[\"plain\"]");
        assert_eq!(
            parse_authorization_server_metadata(no_pkce.as_bytes(), "https://auth.example/tenant"),
            Err(PKCE_REQUIRED)
        );
    }

    #[test]
    fn callback_enforces_state_issuer_and_duplicate_parameters() {
        let request = b"GET /oauth/callback?code=abc&state=state&iss=https%3A%2F%2Fauth.example HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_callback_request(request, "state", "https://auth.example", true)
                .unwrap()
                .code
                .as_str(),
            "abc"
        );
        let duplicate = b"GET /oauth/callback?code=abc&code=def&state=state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_callback_request(duplicate, "state", "https://auth.example", false).map(|_| ()),
            Err(CALLBACK_FAILED)
        );
    }

    #[test]
    fn token_response_accepts_only_bearer_and_ignores_extensions() {
        let token = parse_token_response(
            br#"{"access_token":"access","token_type":"Bearer","expires_in":3600,"refresh_token":"refresh","scope":"read write"}"#,
        )
        .unwrap();
        assert_eq!(token.access_token.as_str(), "access");
        assert_eq!(token.scopes.unwrap(), vec!["read", "write"]);
        assert_eq!(
            parse_token_response(
                br#"{"access_token":"one","access_token":"two","token_type":"Bearer"}"#
            )
            .map(|_| ()),
            Err(TOKEN_FAILED)
        );
        let extended = parse_token_response(
            br#"{"access_token":"access","token_type":"Bearer","id_token":"ignored","extension":{"nested":"ignored-secret"}}"#,
        )
        .unwrap();
        assert_eq!(extended.access_token.as_str(), "access");
        assert_eq!(
            parse_token_response(
                br#"{"access_token":"access","token_type":"Bearer","scope":"bad\\scope"}"#,
            )
            .map(|_| ()),
            Err(TOKEN_FAILED)
        );
    }

    #[test]
    fn pkce_is_s256_and_authorization_url_binds_resource() {
        let (state, verifier, challenge) = generate_state_and_pkce().unwrap();
        assert_eq!(state.len(), 43);
        assert_eq!(verifier.len(), 43);
        assert_eq!(
            challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
        let url = build_authorization_url(AuthorizationUrlInput {
            endpoint: "https://auth.example/authorize",
            client_id: "public-client",
            redirect_uri: "http://127.0.0.1:49152/oauth/callback",
            state: &state,
            challenge: &challenge,
            resource: "https://mcp.example/mcp",
            scopes: &["read".into()],
        })
        .unwrap();
        assert!(url
            .query_pairs()
            .any(|(name, value)| name == "resource" && value == "https://mcp.example/mcp"));
    }

    #[test]
    fn non_loopback_http_is_rejected() {
        assert!(validate_secure_url("http://127.0.0.1:9000/mcp", true).is_ok());
        assert_eq!(
            validate_secure_url("http://192.0.2.1/mcp", true),
            Err(REQUEST_INVALID)
        );
        assert_eq!(
            validate_secure_url("https://mcp.example/mcp?access_token=secret", true),
            Err(REQUEST_INVALID)
        );
    }
}
