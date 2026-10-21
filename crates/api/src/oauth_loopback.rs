//! OAuth 2.1 auth code + PKCE: 인증 중에만 `127.0.0.1:0`의 `/callback`을 연다
//! (01 §5.1 TCP 예외). state는 무작위·5분 시한·1회용이다.
use crate::auth::{OauthConfig, exchange};
use crate::events::Events;
use crate::{ApiError, SendEvent};
use http_body_util::Full;
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

const WAIT: Duration = Duration::from_secs(5 * 60);
const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Devbox</title><p>인증이 끝났습니다. 이 창을 닫아도 됩니다.</p>";
const BAD_PAGE: &str =
    "<!doctype html><meta charset=utf-8><title>Devbox</title><p>인증 응답이 올바르지 않습니다.</p>";

fn random() -> String {
    let bytes: Vec<u8> = [uuid::Uuid::new_v4(), uuid::Uuid::new_v4()]
        .iter()
        .flat_map(|u| *u.as_bytes())
        .collect();
    data_encoding::BASE64URL_NOPAD.encode(&bytes)
}

enum Callback {
    Code(Zeroizing<String>),
    Denied,
}

fn page(status: u16, body: &'static str) -> hyper::Response<Full<bytes::Bytes>> {
    let mut response = hyper::Response::new(Full::new(bytes::Bytes::from_static(body.as_bytes())));
    *response.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::BAD_REQUEST);
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

/// state가 맞는 첫 콜백만 받는다. 다른 요청은 400으로 돌려보내고 계속 기다린다.
fn handle(
    uri: &hyper::Uri,
    state: &str,
    used: &Mutex<bool>,
    tx: &mpsc::Sender<Callback>,
) -> hyper::Response<Full<bytes::Bytes>> {
    if uri.path() != "/callback" {
        return page(404, BAD_PAGE);
    }
    let pairs: Vec<(String, String)> =
        url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    if get("state") != Some(state) {
        return page(400, BAD_PAGE);
    }
    let Ok(mut used) = used.lock() else {
        return page(400, BAD_PAGE);
    };
    if *used {
        return page(400, BAD_PAGE);
    }
    *used = true;
    let callback = match get("code") {
        Some(code) if !code.is_empty() && get("error").is_none() => {
            Callback::Code(Zeroizing::new(code.to_owned()))
        }
        _ => Callback::Denied,
    };
    let denied = matches!(callback, Callback::Denied);
    let _ = tx.try_send(callback);
    if denied {
        page(400, BAD_PAGE)
    } else {
        page(200, DONE_PAGE)
    }
}

pub(crate) async fn authorize(
    client: &reqwest::Client,
    config: &OauthConfig,
    events: &Events<'_>,
) -> Result<(Zeroizing<String>, Option<u64>), ApiError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| ApiError::oauth("callback"))?;
    let port = listener
        .local_addr()
        .map_err(|_| ApiError::oauth("callback"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = random();
    let verifier = Zeroizing::new(random());
    let challenge = data_encoding::BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()));
    let mut url = config
        .authorization_url
        .clone()
        .ok_or_else(|| ApiError::oauth("authorize"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(scope) = &config.scope {
            q.append_pair("scope", scope);
        }
    }
    events
        .send(&SendEvent::Authorize {
            url: url.to_string(),
        })
        .await
        .map_err(|_| ApiError::network("cancelled"))?;
    let (tx, mut rx) = mpsc::channel(1);
    let used = Arc::new(Mutex::new(false));
    let accept = async {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                // fd 고갈 같은 accept 오류가 바쁜 루프가 되지 않게 잠깐 쉰다(전체 대기는 WAIT가 끊는다).
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            };
            let (state, used, tx) = (state.clone(), used.clone(), tx.clone());
            tokio::spawn(async move {
                let service = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let response = handle(req.uri(), &state, &used, &tx);
                        async move { Ok::<_, Infallible>(response) }
                    },
                );
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    };
    let callback = tokio::time::timeout(WAIT, async {
        tokio::select! {
            _ = accept => None,
            callback = rx.recv() => callback,
        }
    })
    .await
    .map_err(|_| ApiError::oauth("authorize_timeout"))?;
    drop(listener);
    let code = match callback {
        Some(Callback::Code(code)) => code,
        _ => return Err(ApiError::oauth("authorize")),
    };
    let form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier.to_string()),
    ];
    exchange(
        client,
        config,
        form,
        tokio::time::Instant::now() + Duration::from_secs(60),
    )
    .await
}
