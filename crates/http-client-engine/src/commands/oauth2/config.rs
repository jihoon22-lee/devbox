use crate::core::oauth;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ts_rs::TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GrantType {
    #[default]
    AuthorizationCode,
    ClientCredentials,
}
#[derive(Clone, Default, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct OAuth2Config {
    pub grant_type: GrantType,
    pub authorization_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: String,
}
impl std::fmt::Debug for OAuth2Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OAuth2Config { redacted }")
    }
}
pub struct ValidatedConfig {
    pub grant_type: GrantType,
    pub authorization_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: Zeroizing<String>,
    pub scopes: Vec<String>,
}
impl std::fmt::Debug for ValidatedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ValidatedConfig { redacted }")
    }
}
pub fn validate(input: &OAuth2Config) -> Result<ValidatedConfig, &'static str> {
    const INVALID: &str = "oauth2_config_invalid";
    oauth::validate_client_id(&input.client_id).map_err(|_| INVALID)?;
    if input.client_secret.len() > 64 * 1024
        || input.scopes.len() > 32 * 1024
        || (input.grant_type == GrantType::ClientCredentials && input.client_secret.is_empty())
    {
        return Err(INVALID);
    }
    let token_url = oauth::validate_secure_url(&input.token_url, true)
        .map_err(|_| INVALID)?
        .to_string();
    let authorization_url = if input.grant_type == GrantType::AuthorizationCode {
        oauth::validate_secure_url(&input.authorization_url, true)
            .map_err(|_| INVALID)?
            .to_string()
    } else {
        String::new()
    };
    let mut scopes = oauth::validate_scopes(
        &input
            .scopes
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
    )
    .map_err(|_| INVALID)?;
    scopes.sort();
    Ok(ValidatedConfig {
        grant_type: input.grant_type,
        authorization_url,
        token_url,
        client_id: input.client_id.clone(),
        client_secret: Zeroizing::new(input.client_secret.clone()),
        scopes,
    })
}
pub fn profile_key(config: &ValidatedConfig) -> String {
    oauth::token_profile_key(
        match config.grant_type {
            GrantType::AuthorizationCode => "authorizationCode",
            GrantType::ClientCredentials => "clientCredentials",
        },
        &config.token_url,
        &config.client_id,
        &config.scopes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(grant: GrantType) -> OAuth2Config {
        OAuth2Config {
            grant_type: grant,
            authorization_url: "https://auth.x.test/authorize".into(),
            token_url: "https://auth.x.test/token".into(),
            client_id: "devbox".into(),
            client_secret: "s3cret".into(),
            scopes: "write read".into(),
        }
    }
    #[test]
    fn validation_requires_secure_urls_and_the_right_fields() {
        assert!(validate(&config(GrantType::AuthorizationCode)).is_ok());
        let mut insecure = config(GrantType::ClientCredentials);
        insecure.token_url = "http://auth.x.test/token".into();
        assert_eq!(validate(&insecure).unwrap_err(), "oauth2_config_invalid");
        insecure.token_url = "http://127.0.0.1:8080/token".into();
        assert!(validate(&insecure).is_ok());
        let mut missing = config(GrantType::AuthorizationCode);
        missing.authorization_url.clear();
        assert!(validate(&missing).is_err());
        let mut missing = config(GrantType::ClientCredentials);
        missing.client_secret.clear();
        assert!(validate(&missing).is_err());
    }
    #[test]
    fn profile_keys_ignore_secret_and_scope_order() {
        let a = validate(&config(GrantType::ClientCredentials)).unwrap();
        let mut other = config(GrantType::ClientCredentials);
        other.client_secret = "different".into();
        other.scopes = "read write".into();
        assert_eq!(profile_key(&a), profile_key(&validate(&other).unwrap()));
        assert_eq!(profile_key(&a).len(), 64);
        assert_ne!(
            profile_key(&a),
            profile_key(&validate(&config(GrantType::AuthorizationCode)).unwrap())
        );
        assert!(!format!("{a:?}").contains("s3cret"));
    }
}
