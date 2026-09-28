pub mod cache;
pub mod config;
pub mod flows;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::watch;
#[derive(Default)]
pub struct OAuth2State {
    active: Mutex<Option<(String, watch::Sender<bool>)>>,
    cache: OnceLock<cache::TokenCache>,
    pending: Mutex<std::collections::VecDeque<String>>,
}
struct FlowLease {
    state: Arc<OAuth2State>,
    id: String,
}
impl Drop for FlowLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            if active.as_ref().is_some_and(|(id, _)| id == &self.id) {
                *active = None;
            }
        }
    }
}
impl OAuth2State {
    fn begin(
        self: &Arc<Self>,
        id: &str,
    ) -> Result<(FlowLease, watch::Receiver<bool>), &'static str> {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err("oauth2_config_invalid");
        }
        let mut active = self.active.lock().map_err(|_| "oauth2_busy")?;
        if active.is_some() {
            return Err("oauth2_busy");
        }
        let (sender, receiver) = watch::channel(false);
        let mut pending = self.pending.lock().map_err(|_| "oauth2_busy")?;
        if let Some(index) = pending.iter().position(|value| value == id) {
            pending.remove(index);
            let _ = sender.send(true);
        }
        *active = Some((id.into(), sender));
        Ok((
            FlowLease {
                state: self.clone(),
                id: id.into(),
            },
            receiver,
        ))
    }
    pub fn cancel(&self, id: &str) -> Result<(), &'static str> {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err("oauth2_config_invalid");
        }
        let active = self.active.lock().map_err(|_| "oauth2_busy")?;
        if let Some((current, sender)) = active.as_ref().filter(|(current, _)| current == id) {
            let _ = current;
            let _ = sender.send(true);
        } else {
            let mut pending = self.pending.lock().map_err(|_| "oauth2_busy")?;
            if !pending.iter().any(|value| value == id) {
                if pending.len() == 32 {
                    pending.pop_front();
                }
                pending.push_back(id.into());
            }
        }
        Ok(())
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HeaderPlan {
    UseToken,
    Refresh,
    Fetch,
    RequireLogin,
}
pub(crate) fn header_plan(cached: &cache::CachedToken, grant: config::GrantType) -> HeaderPlan {
    match cached {
        cache::CachedToken::Valid { .. } => HeaderPlan::UseToken,
        cache::CachedToken::Expired { refresh: Some(_) } => HeaderPlan::Refresh,
        _ if grant == config::GrantType::ClientCredentials => HeaderPlan::Fetch,
        _ => HeaderPlan::RequireLogin,
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
impl OAuth2State {
    pub(crate) fn cache(&self, app: &tauri::AppHandle) -> Result<&cache::TokenCache, &'static str> {
        let root = crate::component::data_root(app).map_err(|_| "oauth2_storage_failed")?;
        Ok(self.cache.get_or_init(|| {
            cache::TokenCache::open(
                root.join("oauth2-tokens.json"),
                Arc::from(crate::platform::platform_oauth2_sealer()),
            )
        }))
    }
    fn save_current(
        &self,
        cache: &cache::TokenCache,
        id: &str,
        key: &str,
        token: &crate::core::oauth::TokenResponse,
    ) -> Result<(), &'static str> {
        let active = self.active.lock().map_err(|_| "oauth2_busy")?;
        if !active
            .as_ref()
            .is_some_and(|(current, sender)| current == id && !*sender.borrow())
        {
            return Err("oauth2_cancelled");
        }
        cache.put(key, token, now())
    }
    fn clear_profile(&self, cache: &cache::TokenCache, key: &str) -> Result<(), &'static str> {
        let active = self.active.lock().map_err(|_| "oauth2_busy")?;
        if let Some((_, sender)) = active.as_ref() {
            let _ = sender.send(true);
        }
        cache.remove(key)
    }
    pub(crate) async fn resolve_token(
        self: &Arc<Self>,
        cache: &cache::TokenCache,
        request_id: &str,
        config: &config::ValidatedConfig,
    ) -> Result<zeroize::Zeroizing<String>, &'static str> {
        let (_lease, mut cancel) = self.begin(request_id)?;
        if *cancel.borrow() {
            return Err("oauth2_cancelled");
        }
        let key = config::profile_key(config);
        let cached = cache.get(&key, now())?;
        let plan = header_plan(&cached, config.grant_type);
        if let cache::CachedToken::Valid {
            access,
            expires_at_ms: _expires_at_ms,
        } = cached
        {
            return Ok(access);
        }
        if plan == HeaderPlan::RequireLogin {
            return Err("oauth2_authorization_required");
        }
        let client = super::oauth_common::oauth_client("oauth2_token_failed")
            .map_err(|_| "oauth2_token_failed")?;
        let token = if let cache::CachedToken::Expired {
            refresh: Some(refresh),
        } = cached
        {
            match flows::refresh(&client, config, &refresh, &mut cancel).await? {
                flows::RefreshOutcome::Token(token) => token,
                flows::RefreshOutcome::Rejected => {
                    cache.remove(&key)?;
                    if config.grant_type == config::GrantType::AuthorizationCode {
                        return Err("oauth2_authorization_required");
                    }
                    flows::client_credentials(&client, config, &mut cancel).await?
                }
            }
        } else {
            flows::client_credentials(&client, config, &mut cancel).await?
        };
        self.save_current(cache, request_id, &key, &token)?;
        Ok(token.access_token)
    }
}

pub(crate) fn status(
    app: &tauri::AppHandle,
    auth: &super::request::AuthConfig,
    environment: &[super::request::EnvironmentVariable],
) -> Result<cache::TokenStatus, &'static str> {
    use tauri::Manager;
    let config = super::request::resolve_oauth2_auth(auth, environment)?;
    let state = app.state::<Arc<OAuth2State>>();
    Ok(state
        .cache(app)?
        .status(&config::profile_key(&config), now()))
}
pub(crate) async fn authorize(
    app: &tauri::AppHandle,
    id: &str,
    auth: &super::request::AuthConfig,
    environment: &[super::request::EnvironmentVariable],
) -> Result<cache::TokenStatus, &'static str> {
    use tauri::Manager;
    use tauri_plugin_opener::OpenerExt;
    let state = app.state::<Arc<OAuth2State>>();
    let (_lease, mut cancel) = state.inner().begin(id)?;
    let config = super::request::resolve_oauth2_auth(auth, environment)?;
    if config.grant_type != config::GrantType::AuthorizationCode {
        return Err("oauth2_config_invalid");
    }
    let client = super::oauth_common::oauth_client("oauth2_token_failed")
        .map_err(|_| "oauth2_token_failed")?;
    let open = |url: &str| app.opener().open_url(url, None::<&str>).map_err(|_| ());
    let token = flows::authorization_code(
        &client,
        &config,
        &open,
        &mut cancel,
        std::time::Duration::from_secs(300),
    )
    .await?;
    let cache = state.cache(app)?;
    let key = config::profile_key(&config);
    state.save_current(cache, id, &key, &token)?;
    Ok(cache.status(&key, now()))
}
pub(crate) async fn fetch(
    app: &tauri::AppHandle,
    auth: &super::request::AuthConfig,
    environment: &[super::request::EnvironmentVariable],
) -> Result<cache::TokenStatus, &'static str> {
    use tauri::Manager;
    let config = super::request::resolve_oauth2_auth(auth, environment)?;
    if config.grant_type != config::GrantType::ClientCredentials {
        return Err("oauth2_config_invalid");
    }
    let state = app.state::<Arc<OAuth2State>>();
    let cache = state.cache(app)?;
    let id = super::grpc_selection::random_hex_128().map_err(|_| "oauth2_token_failed")?;
    let _token = state.inner().resolve_token(cache, &id, &config).await?;
    Ok(cache.status(&config::profile_key(&config), now()))
}
pub(crate) fn clear(
    app: &tauri::AppHandle,
    auth: &super::request::AuthConfig,
    environment: &[super::request::EnvironmentVariable],
) -> Result<(), &'static str> {
    use tauri::Manager;
    let config = super::request::resolve_oauth2_auth(auth, environment)?;
    let state = app.state::<Arc<OAuth2State>>();
    state.clear_profile(state.cache(app)?, &config::profile_key(&config))
}
pub(crate) fn cancel(app: &tauri::AppHandle, id: &str) -> Result<(), &'static str> {
    use tauri::Manager;
    app.state::<Arc<OAuth2State>>().cancel(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock;
    impl devbox_secrets::Sealer for Mock {
        fn seal(&self, value: &str) -> Result<Vec<u8>, devbox_secrets::SealError> {
            Ok(value.bytes().rev().collect())
        }
        fn unseal(
            &self,
            value: &[u8],
        ) -> Result<zeroize::Zeroizing<String>, devbox_secrets::SealError> {
            Ok(zeroize::Zeroizing::new(
                String::from_utf8(value.iter().rev().copied().collect()).unwrap(),
            ))
        }
    }
    #[tokio::test]
    async fn coordinator_caches_client_credentials_and_does_not_repeat_the_token_call() {
        let (url, request) = flows::tests::serve_once(
            200,
            r#"{"access_token":"cached-token","token_type":"Bearer","expires_in":3600}"#,
        )
        .await;
        let config = flows::tests::config(config::GrantType::ClientCredentials, &url);
        let dir = tempfile::tempdir().unwrap();
        let cache = cache::TokenCache::open(dir.path().join("tokens.json"), Arc::new(Mock));
        let state = Arc::new(OAuth2State::default());
        assert_eq!(
            state
                .resolve_token(&cache, "first", &config)
                .await
                .unwrap()
                .as_str(),
            "cached-token"
        );
        assert!(request.await.unwrap().contains("client_credentials"));
        assert_eq!(
            state
                .resolve_token(&cache, "second", &config)
                .await
                .unwrap()
                .as_str(),
            "cached-token"
        );
    }
    #[tokio::test]
    async fn rejected_refresh_clears_the_cache_and_requires_login_without_replay() {
        let (url, request) = flows::tests::serve_once(400, r#"{"error":"invalid_grant"}"#).await;
        let config = flows::tests::config(config::GrantType::AuthorizationCode, &url);
        let dir = tempfile::tempdir().unwrap();
        let cache = cache::TokenCache::open(dir.path().join("tokens.json"), Arc::new(Mock));
        let token = crate::core::oauth::parse_token_response(br#"{"access_token":"old","refresh_token":"refresh","expires_in":1,"token_type":"bearer"}"#).unwrap();
        let key = config::profile_key(&config);
        cache.put(&key, &token, 1).unwrap();
        let state = Arc::new(OAuth2State::default());
        assert_eq!(
            state
                .resolve_token(&cache, "refresh", &config)
                .await
                .unwrap_err(),
            "oauth2_authorization_required"
        );
        assert!(request.await.unwrap().contains("refresh_token"));
        assert_eq!(cache.status(&key, now()).state, "missing");
    }
    #[test]
    fn clearing_a_profile_cancels_a_pending_flow_and_prevents_late_cache_writes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache::TokenCache::open(dir.path().join("tokens.json"), Arc::new(Mock));
        let state = Arc::new(OAuth2State::default());
        let (_lease, receiver) = state.begin("pending").unwrap();
        state.clear_profile(&cache, "key").unwrap();
        assert!(*receiver.borrow());
        let token = crate::core::oauth::parse_token_response(
            br#"{"access_token":"late","token_type":"bearer"}"#,
        )
        .unwrap();
        assert_eq!(
            state.save_current(&cache, "pending", "key", &token),
            Err("oauth2_cancelled")
        );
        assert_eq!(cache.status("key", now()).state, "missing");
    }

    #[test]
    fn cancellation_before_flow_registration_is_not_lost() {
        let state = Arc::new(OAuth2State::default());
        state.cancel("early").unwrap();
        let (_lease, receiver) = state.begin("early").unwrap();
        assert!(*receiver.borrow());
    }

    #[test]
    fn header_plans_cover_cached_expired_and_missing_tokens() {
        use cache::CachedToken;
        use config::GrantType::{AuthorizationCode, ClientCredentials};
        use zeroize::Zeroizing;
        assert_eq!(
            header_plan(
                &CachedToken::Valid {
                    access: Zeroizing::new("token".into()),
                    expires_at_ms: None
                },
                AuthorizationCode
            ),
            HeaderPlan::UseToken
        );
        assert_eq!(
            header_plan(
                &CachedToken::Expired {
                    refresh: Some(Zeroizing::new("refresh".into()))
                },
                AuthorizationCode
            ),
            HeaderPlan::Refresh
        );
        for cached in [CachedToken::Expired { refresh: None }, CachedToken::Missing] {
            assert_eq!(
                header_plan(&cached, AuthorizationCode),
                HeaderPlan::RequireLogin
            );
            assert_eq!(header_plan(&cached, ClientCredentials), HeaderPlan::Fetch);
        }
    }

    #[test]
    fn one_flow_owns_cancellation_until_its_lease_is_released() {
        let state = Arc::new(OAuth2State::default());
        let (lease, receiver) = state.begin("a").unwrap();
        assert!(matches!(state.begin("b"), Err("oauth2_busy")));
        state.cancel("other").unwrap();
        assert!(!*receiver.borrow());
        state.cancel("a").unwrap();
        assert!(*receiver.borrow());
        drop(lease);
        assert!(state.begin("b").is_ok());
    }
}
