//! OSV querybatch adapter. Raw response bodies never cross the domain boundary.
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};
pub const ENDPOINT: &str = "https://api.osv.dev/v1/querybatch";
pub const CACHE_MS: u64 = 30 * 60 * 1000;
pub type HttpFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, AuditFailure>> + Send>>;
pub trait Http: Send + Sync {
    fn post(&self, endpoint: &str, body: Vec<u8>) -> HttpFuture;
}
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
pub struct NetworkHttp;
impl Http for NetworkHttp {
    fn post(&self, endpoint: &str, body: Vec<u8>) -> HttpFuture {
        let endpoint = endpoint.to_owned();
        Box::pin(async move {
            // reqwest rustls-no-provider needs an explicit process provider even
            // when OSV is the first TLS client used, before any API request.
            static PROVIDER: OnceLock<()> = OnceLock::new();
            PROVIDER.get_or_init(|| {
                let _ = rustls::crypto::ring::default_provider().install_default();
            });
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| AuditFailure::Network)?;
            let mut response = client
                .post(endpoint)
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .map_err(|_| AuditFailure::Network)?;
            if !response.status().is_success() {
                return Err(AuditFailure::Http);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| AuditFailure::Network)? {
                if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                    return Err(AuditFailure::LimitExceeded);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRequest {
    pub queries: Vec<Query>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub package: Package,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub ecosystem: String,
    pub name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchResponse {
    results: Vec<QueryResult>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryResult {
    #[serde(default)]
    vulns: Vec<Vulnerability>,
    next_page_token: Option<String>,
}
#[derive(Deserialize)]
struct Vulnerability {
    id: String,
    modified: String,
}
pub struct Auditor {
    http: Arc<dyn Http>,
    clock: Clock,
    endpoint: String,
    cache: tokio::sync::Mutex<std::collections::BTreeMap<String, AuditReport>>,
}
impl Auditor {
    pub fn new(http: Arc<dyn Http>, clock: Clock, endpoint: String) -> Self {
        Self {
            http,
            clock,
            endpoint,
            cache: Default::default(),
        }
    }
    pub async fn audit(
        &self,
        project: &str,
        inventory: &DependencyInventory,
        progress: Arc<dyn Fn(u32, u32) + Send + Sync>,
    ) -> Result<(AuditReport, bool), AuditFailure> {
        use sha2::{Digest, Sha256};
        let (mut packages, skipped) = packages(inventory)?;
        let mut hash = Sha256::new();
        // Length-prefixed structured serialization avoids delimiter/name collisions.
        hash.update(
            serde_json::to_vec(&(
                project,
                &self.endpoint,
                &inventory.fingerprint,
                &packages,
                skipped,
            ))
            .map_err(|_| AuditFailure::InvalidResponse)?,
        );
        let key = format!("{:x}", hash.finalize());
        let total = packages.len() as u32;
        let now = (self.clock)();
        if let Some(report) = self
            .cache
            .lock()
            .await
            .get(&key)
            .filter(|r| {
                now.checked_sub(r.completed_at_ms)
                    .is_some_and(|age| age < CACHE_MS)
            })
            .cloned()
        {
            progress(total, total);
            return Ok((report, true));
        }
        progress(0, total);
        let mut completed = 0;
        let mut advisory_count = 0;
        for chunk in packages.chunks_mut(100) {
            let mut pending: Vec<(usize, Query)> = chunk
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        i,
                        Query {
                            package: Package {
                                ecosystem: ecosystem(p.ecosystem).into(),
                                name: p.name.clone(),
                            },
                            version: p.version.clone(),
                            page_token: None,
                        },
                    )
                })
                .collect();
            let mut pages = 0;
            let mut seen_tokens = std::collections::BTreeSet::new();
            while !pending.is_empty() {
                pages += 1;
                if pages > 16 {
                    return Err(AuditFailure::Incomplete);
                }
                let request = BatchRequest {
                    queries: pending.iter().map(|(_, q)| q.clone()).collect(),
                };
                let body =
                    serde_json::to_vec(&request).map_err(|_| AuditFailure::InvalidResponse)?;
                let bytes = self.http.post(&self.endpoint, body).await?;
                if bytes.len() > 2 * 1024 * 1024 {
                    return Err(AuditFailure::LimitExceeded);
                }
                let response: BatchResponse =
                    serde_json::from_slice(&bytes).map_err(|_| AuditFailure::InvalidResponse)?;
                if response.results.len() != pending.len() {
                    return Err(AuditFailure::Incomplete);
                }
                let mut next = Vec::new();
                for ((index, mut query), result) in pending.into_iter().zip(response.results) {
                    for vulnerability in result.vulns {
                        let id = vulnerability.id;
                        if id.is_empty()
                            || id.len() > 128
                            || !id
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                            || vulnerability.modified.is_empty()
                            || vulnerability.modified.len() > 64
                        {
                            return Err(AuditFailure::InvalidResponse);
                        }
                        advisory_count += 1;
                        if advisory_count > 4096 {
                            return Err(AuditFailure::LimitExceeded);
                        }
                        chunk[index].advisories.push(Advisory {
                            url: format!("https://osv.dev/vulnerability/{id}"),
                            id,
                        });
                    }
                    if let Some(token) = result.next_page_token.filter(|s| !s.is_empty()) {
                        if token.len() > 4096 || !seen_tokens.insert((index, token.clone())) {
                            return Err(AuditFailure::Incomplete);
                        }
                        query.page_token = Some(token);
                        next.push((index, query));
                    } else {
                        completed += 1;
                        progress(completed, total);
                    }
                }
                pending = next;
            }
            for package in chunk {
                package.advisories.sort_by(|a, b| a.id.cmp(&b.id));
                package.advisories.dedup_by(|a, b| a.id == b.id);
            }
        }
        let report = AuditReport {
            fingerprint: inventory.fingerprint.clone(),
            completed_at_ms: (self.clock)(),
            packages,
            skipped,
        };
        let mut cache = self.cache.lock().await;
        if cache.len() >= 16 {
            cache.pop_first();
        }
        cache.insert(key, report.clone());
        Ok((report, false))
    }
}
fn ecosystem(value: Ecosystem) -> &'static str {
    match value {
        Ecosystem::Npm => "npm",
        Ecosystem::Cargo => "crates.io",
        Ecosystem::Go => "Go",
        Ecosystem::Python => "PyPI",
    }
}
fn packages(inventory: &DependencyInventory) -> Result<(Vec<AuditPackage>, u32), AuditFailure> {
    let mut keys = std::collections::BTreeSet::new();
    let mut skipped = 0;
    for d in &inventory.dependencies {
        for v in &d.versions {
            if v.source != DependencySource::Registry {
                skipped += 1;
                continue;
            }
            let name = match d.ecosystem {
                Ecosystem::Python => {
                    let mut n = String::new();
                    for c in d.name.chars() {
                        if matches!(c, '-' | '_' | '.') {
                            if !n.ends_with('-') {
                                n.push('-');
                            }
                        } else {
                            n.push(c.to_ascii_lowercase());
                        }
                    }
                    n
                }
                Ecosystem::Cargo => d.name.to_ascii_lowercase().replace('_', "-"),
                _ => d.name.clone(),
            };
            keys.insert((d.ecosystem, name, v.version.clone()));
            if keys.len() > crate::locks::MAX_ITEMS {
                return Err(AuditFailure::LimitExceeded);
            }
        }
    }
    Ok((
        keys.into_iter()
            .map(|(ecosystem, name, version)| AuditPackage {
                ecosystem,
                name,
                version,
                advisories: vec![],
            })
            .collect(),
        skipped,
    ))
}
