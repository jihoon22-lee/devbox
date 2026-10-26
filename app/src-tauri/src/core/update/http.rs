//! Small public GitHub transport. No renderer-supplied URLs, credentials or TLS bypasses.
use super::{
    R, UpdateError,
    release::{Candidate, MAX_METADATA, REPOSITORY},
    storage::Pending,
};
use reqwest::{
    Client, Response, Url,
    header::{ACCEPT, LOCATION},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const REQUEST_TIME: Duration = Duration::from_secs(300);
const READ_TIME: Duration = Duration::from_secs(15);
const REDIRECTS: usize = 3;
pub struct Http {
    client: Client,
    deadline: Duration,
    redirect_allowed: fn(&Url) -> bool,
}
impl Http {
    pub fn new() -> R<Self> {
        // reqwest rustls-no-provider cannot build a client before a provider is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(READ_TIME)
            .timeout(REQUEST_TIME)
            .user_agent("devbox-updater/1")
            .build()
            .map_err(|_| UpdateError::Network)?;
        Ok(Self {
            client,
            deadline: REQUEST_TIME,
            redirect_allowed: allowed_redirect,
        })
    }
    pub async fn metadata(&self, cancel: &CancellationToken) -> R<Vec<u8>> {
        let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
        tokio::time::timeout(Duration::from_secs(30), self.metadata_at(&url, cancel))
            .await
            .map_err(|_| UpdateError::Timeout)?
    }
    async fn metadata_at(&self, url: &str, cancel: &CancellationToken) -> R<Vec<u8>> {
        let mut response = self
            .send(url, "application/vnd.github+json", cancel)
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(
                br#"{"tag_name":"v0.0.0","draft":false,"prerelease":false,"assets":[]}"#.to_vec(),
            );
        }
        if response.status().as_u16() != 200 {
            return Err(UpdateError::Network);
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_METADATA as u64)
        {
            return Err(UpdateError::Size);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = chunk(&mut response, cancel).await? {
            if bytes.len().saturating_add(chunk.len()) > MAX_METADATA {
                return Err(UpdateError::Size);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    async fn send(&self, url: &str, accept: &str, cancel: &CancellationToken) -> R<Response> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(UpdateError::Cancelled),
            response = self.client.get(url).header(ACCEPT, accept).header("X-GitHub-Api-Version", "2022-11-28").send() => response.map_err(network_error),
        }
    }
    pub async fn download(
        &self,
        candidate: &Candidate,
        pending: Pending,
        cancel: &CancellationToken,
        progress: impl Fn(u64),
    ) -> R<super::storage::ReadyProof> {
        let url = format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/assets/{}",
            candidate.id
        );
        self.download_at(&url, candidate, pending, cancel, progress)
            .await
    }
    async fn download_at(
        &self,
        initial: &str,
        candidate: &Candidate,
        pending: Pending,
        cancel: &CancellationToken,
        progress: impl Fn(u64),
    ) -> R<super::storage::ReadyProof> {
        tokio::time::timeout(
            self.deadline,
            self.download_inner(initial, candidate, pending, cancel, progress),
        )
        .await
        .map_err(|_| UpdateError::Timeout)?
    }
    async fn download_inner(
        &self,
        initial: &str,
        candidate: &Candidate,
        mut pending: Pending,
        cancel: &CancellationToken,
        progress: impl Fn(u64),
    ) -> R<super::storage::ReadyProof> {
        let mut url = Url::parse(initial).map_err(|_| UpdateError::InvalidRelease)?;
        for hop in 0..=REDIRECTS {
            let mut response = self
                .send(url.as_str(), "application/octet-stream", cancel)
                .await?;
            if response.status().is_redirection() {
                if hop == REDIRECTS {
                    return Err(UpdateError::Network);
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(UpdateError::InvalidRelease)?;
                let next = url
                    .join(location)
                    .map_err(|_| UpdateError::InvalidRelease)?;
                if !(self.redirect_allowed)(&next) {
                    return Err(UpdateError::InvalidRelease);
                }
                url = next;
                continue;
            }
            if response.status().as_u16() != 200 {
                return Err(UpdateError::Network);
            }
            if response
                .content_length()
                .is_some_and(|n| n != candidate.size)
            {
                return Err(UpdateError::Size);
            }
            while let Some(chunk) = chunk(&mut response, cancel).await? {
                if cancel.is_cancelled() {
                    return Err(UpdateError::Cancelled);
                }
                progress(pending.push(&chunk, candidate.size)?);
            }
            if cancel.is_cancelled() {
                return Err(UpdateError::Cancelled);
            }
            return pending.finish(candidate);
        }
        Err(UpdateError::Network)
    }
}
fn network_error(error: reqwest::Error) -> UpdateError {
    if error.is_timeout() {
        UpdateError::Timeout
    } else {
        UpdateError::Network
    }
}
async fn chunk(
    response: &mut Response,
    cancel: &CancellationToken,
) -> R<Option<impl AsRef<[u8]> + std::ops::Deref<Target = [u8]>>> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(UpdateError::Cancelled),
        value = tokio::time::timeout(READ_TIME, response.chunk()) => value.map_err(|_| UpdateError::Timeout)?.map_err(network_error),
    }
}
fn allowed_redirect(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && url.fragment().is_none()
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}
#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
