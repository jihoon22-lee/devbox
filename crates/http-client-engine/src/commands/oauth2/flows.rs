use super::config::ValidatedConfig;
use crate::{
    commands::oauth_common as common,
    core::oauth::{self, TokenResponse},
};
use std::{net::Ipv4Addr, time::Duration};
use tokio::{net::TcpListener, sync::watch};
use zeroize::Zeroizing;
const FAILED: &str = "oauth2_token_failed";
const CANCELLED: &str = "oauth2_cancelled";
fn mapped(error: String) -> &'static str {
    if error == common::OAUTH_CANCELLED {
        CANCELLED
    } else {
        FAILED
    }
}
pub async fn client_credentials(
    client: &reqwest::Client,
    config: &ValidatedConfig,
    cancel: &mut watch::Receiver<bool>,
) -> Result<TokenResponse, &'static str> {
    let scopes = config.scopes.join(" ");
    let mut form = vec![
        ("grant_type", "client_credentials"),
        ("client_id", config.client_id.as_str()),
        ("client_secret", config.client_secret.as_str()),
    ];
    if !scopes.is_empty() {
        form.push(("scope", &scopes));
    }
    let bytes = common::send_form(client, &config.token_url, &form, cancel, FAILED)
        .await
        .map_err(mapped)?;
    oauth::parse_token_response(&bytes).map_err(|_| FAILED)
}
pub enum RefreshOutcome {
    Token(TokenResponse),
    Rejected,
}
pub async fn refresh(
    client: &reqwest::Client,
    config: &ValidatedConfig,
    refresh_token: &str,
    cancel: &mut watch::Receiver<bool>,
) -> Result<RefreshOutcome, &'static str> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", config.client_id.as_str()),
    ];
    if !config.client_secret.is_empty() {
        form.push(("client_secret", config.client_secret.as_str()));
    }
    let response = common::send_request(client.post(&config.token_url).form(&form), cancel, FAILED)
        .await
        .map_err(mapped)?;
    let status = response.status();
    let bytes =
        common::read_json_response(response, oauth::MAX_TOKEN_RESPONSE_BYTES, cancel, FAILED)
            .await
            .map_err(mapped)?;
    if matches!(status.as_u16(), 400 | 401)
        && serde_json::from_slice::<serde_json::Value>(&bytes)
            .is_ok_and(|value| value["error"] == "invalid_grant")
    {
        return Ok(RefreshOutcome::Rejected);
    }
    if !status.is_success() {
        return Err(FAILED);
    }
    let mut token = oauth::parse_token_response(&bytes).map_err(|_| FAILED)?;
    if token.refresh_token.is_none() {
        token.refresh_token = Some(Zeroizing::new(refresh_token.to_owned()));
    }
    Ok(RefreshOutcome::Token(token))
}
pub async fn authorization_code(
    client: &reqwest::Client,
    config: &ValidatedConfig,
    open: &(dyn Fn(&str) -> Result<(), ()> + Send + Sync),
    cancel: &mut watch::Receiver<bool>,
    timeout: Duration,
) -> Result<TokenResponse, &'static str> {
    if *cancel.borrow() {
        return Err(CANCELLED);
    }
    let flow = async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| FAILED)?;
        let redirect = format!(
            "http://127.0.0.1:{}{}",
            listener.local_addr().map_err(|_| FAILED)?.port(),
            oauth::CALLBACK_PATH
        );
        let (state, verifier, challenge) = oauth::generate_state_and_pkce().map_err(|_| FAILED)?;
        let url = oauth::build_authorization_url(oauth::AuthorizationUrlInput {
            endpoint: &config.authorization_url,
            client_id: &config.client_id,
            redirect_uri: &redirect,
            state: &state,
            challenge: &challenge,
            resource: None,
            scopes: &config.scopes,
        })
        .map_err(|_| FAILED)?;
        open(url.as_str()).map_err(|_| FAILED)?;
        let (mut stream, peer) = tokio::select! {biased; _=cancel.changed()=>return Err(CANCELLED),accepted=listener.accept()=>accepted.map_err(|_|FAILED)?};
        if !peer.ip().is_loopback() {
            return Err(FAILED);
        }
        let request = common::read_callback(&mut stream, cancel)
            .await
            .map_err(mapped)?;
        let callback = match oauth::parse_callback_request(&request, &state, "", false) {
            Ok(value) => value,
            Err(_) => {
                let _ = common::write_callback_page(&mut stream, false).await;
                return Err(FAILED);
            }
        };
        drop(request);
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", callback.code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", config.client_id.as_str()),
            ("code_verifier", verifier.as_str()),
        ];
        if !config.client_secret.is_empty() {
            form.push(("client_secret", config.client_secret.as_str()));
        }
        let result = common::send_form(client, &config.token_url, &form, cancel, FAILED)
            .await
            .map_err(mapped)
            .and_then(|bytes| oauth::parse_token_response(&bytes).map_err(|_| FAILED));
        let _ = common::write_callback_page(&mut stream, result.is_ok()).await;
        result
    };
    tokio::time::timeout(timeout.min(Duration::from_secs(300)), flow)
        .await
        .map_err(|_| CANCELLED)?
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::config::{validate, GrantType, OAuth2Config};
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    pub(crate) async fn serve_once(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let mut buffer = [0; 2048];
            loop {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (url, task)
    }
    pub(crate) fn config(grant_type: GrantType, token_url: &str) -> ValidatedConfig {
        validate(&OAuth2Config {
            grant_type,
            authorization_url: "https://auth.x.test/authorize".into(),
            token_url: token_url.into(),
            client_id: "devbox".into(),
            client_secret: "s".into(),
            scopes: "read".into(),
        })
        .unwrap()
    }
    fn client() -> reqwest::Client {
        crate::commands::oauth_common::oauth_client("oauth2_token_failed").unwrap()
    }
    #[tokio::test]
    async fn client_credentials_posts_a_form_and_parses_the_token() {
        let (url, request) = serve_once(
            200,
            r#"{"access_token":"tok","token_type":"Bearer","expires_in":60}"#,
        )
        .await;
        let (_sender, mut receiver) = watch::channel(false);
        let token = client_credentials(
            &client(),
            &config(GrantType::ClientCredentials, &url),
            &mut receiver,
        )
        .await
        .unwrap();
        assert_eq!(token.access_token.as_str(), "tok");
        let raw = request.await.unwrap();
        assert!(
            raw.contains("grant_type=client_credentials")
                && raw.contains("client_id=devbox")
                && raw.contains("scope=read")
        );
    }
    #[tokio::test]
    async fn refresh_rejection_is_distinct_and_success_keeps_the_old_refresh_token_if_omitted() {
        let (url, request) = serve_once(400, r#"{"error":"invalid_grant"}"#).await;
        let (_sender, mut receiver) = watch::channel(false);
        assert!(matches!(
            refresh(
                &client(),
                &config(GrantType::AuthorizationCode, &url),
                "refresh-old",
                &mut receiver
            )
            .await
            .unwrap(),
            RefreshOutcome::Rejected
        ));
        request.await.unwrap();
        let (url, request) =
            serve_once(200, r#"{"access_token":"new","token_type":"Bearer"}"#).await;
        let RefreshOutcome::Token(token) = refresh(
            &client(),
            &config(GrantType::AuthorizationCode, &url),
            "refresh-old",
            &mut receiver,
        )
        .await
        .unwrap() else {
            panic!("expected refreshed token")
        };
        assert_eq!(token.refresh_token.unwrap().as_str(), "refresh-old");
        request.await.unwrap();
    }
    #[tokio::test]
    async fn authorization_code_accepts_a_state_bound_callback_and_sends_pkce() {
        let (url, request) =
            serve_once(200, r#"{"access_token":"code-tok","token_type":"bearer"}"#).await;
        let (_sender, mut receiver) = watch::channel(false);
        let open = |authorization_url: &str| -> Result<(), ()> {
            let url = reqwest::Url::parse(authorization_url).unwrap();
            let pairs = url
                .query_pairs()
                .into_owned()
                .collect::<std::collections::HashMap<_, _>>();
            assert!(!pairs.contains_key("resource"));
            let callback = format!(
                "{}?code=abc&state={}",
                pairs["redirect_uri"], pairs["state"]
            );
            tokio::spawn(async move {
                let _ = reqwest::get(callback).await;
            });
            Ok(())
        };
        let token = authorization_code(
            &client(),
            &config(GrantType::AuthorizationCode, &url),
            &open,
            &mut receiver,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(token.access_token.as_str(), "code-tok");
        let raw = request.await.unwrap();
        assert!(
            raw.contains("grant_type=authorization_code")
                && raw.contains("code=abc")
                && raw.contains("code_verifier=")
        );
    }
    #[tokio::test]
    async fn timeout_and_cancel_release_the_callback_port() {
        let config = config(GrantType::AuthorizationCode, "https://auth.x.test/token");
        let (_sender, mut receiver) = watch::channel(false);
        let port = std::sync::Mutex::new(None);
        let open = |url: &str| {
            let url = reqwest::Url::parse(url).unwrap();
            let redirect = url
                .query_pairs()
                .find(|(key, _)| key == "redirect_uri")
                .unwrap()
                .1
                .into_owned();
            *port.lock().unwrap() = reqwest::Url::parse(&redirect).unwrap().port();
            Ok(())
        };
        assert_eq!(
            authorization_code(
                &client(),
                &config,
                &open,
                &mut receiver,
                Duration::from_millis(50)
            )
            .await
            .err(),
            Some("oauth2_cancelled")
        );
        let callback_port = port.lock().unwrap().unwrap();
        assert!(
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, callback_port))
                .await
                .is_err()
        );
        let (sender, mut receiver) = watch::channel(false);
        let client = client();
        let (result, _) = tokio::join!(
            authorization_code(
                &client,
                &config,
                &open,
                &mut receiver,
                Duration::from_secs(60)
            ),
            async {
                sender.send(true).unwrap();
            }
        );
        assert_eq!(result.err(), Some("oauth2_cancelled"));
    }
}
