//! Bounded OAuth HTTP and loopback plumbing shared by MCP and HTTP requests.
use crate::core::oauth;
use futures_util::StreamExt;
use reqwest::header::CONTENT_TYPE;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;
use zeroize::Zeroizing;
// Preserve the existing MCP public code; HTTP OAuth maps this closed code.
pub(crate) const OAUTH_CANCELLED: &str = "mcp_oauth_cancelled";
const NETWORK_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn oauth_client(error: &'static str) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(NETWORK_TIMEOUT)
        .timeout(NETWORK_TIMEOUT)
        .build()
        .map_err(|_| error.to_string())
}

pub(crate) async fn send_request(
    request: reqwest::RequestBuilder,
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<reqwest::Response, String> {
    if *cancellation.borrow() {
        return Err(OAUTH_CANCELLED.into());
    }
    tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            Err(OAUTH_CANCELLED.into())
        }
        response = request.send() => response.map_err(|_| error.to_string()),
    }
}

pub(crate) async fn read_json_response(
    response: reqwest::Response,
    limit: usize,
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value.to_ascii_lowercase().ends_with("+json")
        });
    if !content_type {
        return Err(error.into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            biased;
            changed = cancellation.changed() => {
                let _ = changed;
                return Err(OAUTH_CANCELLED.into());
            }
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|_| error.to_string())?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(error.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(error.into());
    }
    Ok(bytes)
}

pub(crate) async fn send_form(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let url = oauth::validate_secure_url(endpoint, true).map_err(|_| error.to_string())?;
    let response = send_request(client.post(url).form(form), cancellation, error).await?;
    if response.status().is_redirection() || !response.status().is_success() {
        return Err(error.into());
    }
    read_json_response(
        response,
        oauth::MAX_TOKEN_RESPONSE_BYTES,
        cancellation,
        error,
    )
    .await
}

pub(crate) async fn send_form_allow_empty(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<(), String> {
    let url = oauth::validate_secure_url(endpoint, true).map_err(|_| error.to_string())?;
    let response = send_request(client.post(url).form(form), cancellation, error).await?;
    if response.status().is_redirection() || !response.status().is_success() {
        return Err(error.into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = 0usize;
    while let Some(chunk) = tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            return Err(OAUTH_CANCELLED.into());
        }
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk.map_err(|_| error.to_string())?;
        bytes = bytes.saturating_add(chunk.len());
        if bytes > oauth::MAX_TOKEN_RESPONSE_BYTES {
            return Err(error.into());
        }
    }
    Ok(())
}

pub(crate) async fn read_callback(
    stream: &mut TcpStream,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut request = Zeroizing::new(Vec::new());
    let mut buffer = Zeroizing::new([0_u8; 2048]);
    loop {
        let count = tokio::select! {
            biased;
            changed = cancellation.changed() => {
                let _ = changed;
                return Err(OAUTH_CANCELLED.into());
            }
            read = tokio::time::timeout(NETWORK_TIMEOUT, stream.read(&mut buffer[..])) => {
                read.map_err(|_| oauth::CALLBACK_FAILED.to_string())?
                    .map_err(|_| oauth::CALLBACK_FAILED.to_string())?
            }
        };
        if count == 0 {
            return Err(oauth::CALLBACK_FAILED.into());
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > 16 * 1024 {
            return Err(oauth::CALLBACK_FAILED.into());
        }
        if request.ends_with(b"\r\n\r\n") {
            return Ok(request);
        }
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Err(oauth::CALLBACK_FAILED.into());
        }
    }
}

pub(crate) async fn write_callback_page(
    stream: &mut TcpStream,
    success: bool,
) -> Result<(), String> {
    let body = if success {
        "Authorization completed. You may close this window."
    } else {
        "Authorization failed. You may close this window."
    };
    let status = if success { "200 OK" } else { "400 Bad Request" };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())?;
    stream
        .shutdown()
        .await
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())
}
