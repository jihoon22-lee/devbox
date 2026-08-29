//! Explicit two-step remote enrichment command boundary.

use super::{
    dependency_analysis_lock, repository_entry, revalidate_repository_context, spawn_git_task,
    validated_repository_context,
};
use crate::core::dependency_enrichment::{
    apply_cache_updates, build_enrichment_plan, combine_deps_dev, parse_cache, parse_deps_package,
    parse_deps_version, parse_osv_batch, preview_token, resolve_enrichment, serialize_cache,
    valid_preview_token, DependencyEnrichmentPreview, DependencyEnrichmentReport, EnrichmentCache,
    EnrichmentPlan, EnrichmentSelection, ParsedDepsDevValue, ParsedOsvValue, RemoteCoordinate,
    DEPENDENCY_ENRICHMENT_BUSY, DEPENDENCY_ENRICHMENT_ERROR, DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED,
    DEPS_DEV_HOST, MAX_CACHE_BYTES, MAX_DEPS_PACKAGE_RESPONSE_BYTES,
    MAX_DEPS_VERSION_RESPONSE_BYTES, MAX_OSV_RESPONSE_BYTES, OSV_HOST, PREVIEW_TTL_MS,
};
use crate::core::dependency_lens::{analyze_repository, now_epoch_ms};
use futures_util::future::{join, join_all};
use reqwest::{redirect::Policy, Client, ClientBuilder, Response, StatusCode, Url};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const MAX_STORED_PREVIEWS: usize = 8;
const ANALYSIS_BUDGET: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const DEPS_DEV_BATCH_SIZE: usize = 4;
const CACHE_DIRECTORY: &str = "repo-manager";
const CACHE_FILE: &str = "dependency-enrichment-v1.json";
const USER_AGENT: &str = "devbox-repo-manager/dependency-lens";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyEnrichmentPreviewRequest {
    pub path: String,
    pub services: EnrichmentSelection,
    #[serde(default)]
    pub force_refresh: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyEnrichmentExecuteRequest {
    pub path: String,
    pub preview_token: String,
}

#[derive(Debug, Clone)]
struct StoredPreview {
    token: String,
    canonical_repository: String,
    expires_at_ms: u64,
    plan: EnrichmentPlan,
}

#[derive(Default)]
struct PreviewStore {
    entries: Vec<StoredPreview>,
}

impl PreviewStore {
    fn insert(&mut self, preview: StoredPreview, now_ms: u64) {
        self.entries
            .retain(|entry| entry.expires_at_ms > now_ms && entry.token != preview.token);
        self.entries.push(preview);
        self.entries.sort_by_key(|entry| entry.expires_at_ms);
        if self.entries.len() > MAX_STORED_PREVIEWS {
            let excess = self.entries.len() - MAX_STORED_PREVIEWS;
            self.entries.drain(..excess);
        }
    }

    fn consume(&mut self, token: &str, now_ms: u64) -> Option<StoredPreview> {
        self.entries.retain(|entry| entry.expires_at_ms > now_ms);
        let index = self.entries.iter().position(|entry| entry.token == token)?;
        Some(self.entries.remove(index))
    }
}

fn preview_store() -> &'static Mutex<PreviewStore> {
    static STORE: OnceLock<Mutex<PreviewStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(PreviewStore::default()))
}

fn next_preview_sequence() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
struct EnrichmentEndpoints {
    osv_query_batch: Url,
    deps_dev_root: Url,
}

impl EnrichmentEndpoints {
    fn production() -> Result<Self, String> {
        let osv_query_batch = Url::parse(&format!("https://{OSV_HOST}/v1/querybatch"))
            .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
        let deps_dev_root = Url::parse(&format!("https://{DEPS_DEV_HOST}/v3/"))
            .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
        Ok(Self {
            osv_query_batch,
            deps_dev_root,
        })
    }

    fn deps_version(&self, coordinate: &RemoteCoordinate) -> Result<Url, ()> {
        let mut url = self.deps_dev_root.clone();
        url.path_segments_mut()
            .map_err(|_| ())?
            .pop_if_empty()
            .extend([
                "systems",
                coordinate.system,
                "packages",
                coordinate.name.as_str(),
                "versions",
                coordinate.version.as_str(),
            ]);
        Ok(url)
    }

    fn deps_package(&self, coordinate: &RemoteCoordinate) -> Result<Url, ()> {
        let mut url = self.deps_dev_root.clone();
        url.path_segments_mut()
            .map_err(|_| ())?
            .pop_if_empty()
            .extend([
                "systems",
                coordinate.system,
                "packages",
                coordinate.name.as_str(),
            ]);
        Ok(url)
    }
}

#[tauri::command]
pub async fn dependency_enrichment_preview(
    request: DependencyEnrichmentPreviewRequest,
) -> Result<DependencyEnrichmentPreview, String> {
    if !request.services.any() {
        return Err(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.into());
    }
    let now_ms = now_epoch_ms();
    let common_root = devbox_integration::common_root();
    let prepared = spawn_git_task(DEPENDENCY_ENRICHMENT_ERROR, move || {
        let _analysis = dependency_analysis_lock()
            .try_lock()
            .map_err(|_| DEPENDENCY_ENRICHMENT_BUSY.to_string())?;
        let context = validated_repository_context(&request.path, DEPENDENCY_ENRICHMENT_ERROR)?;
        let repository = repository_entry(&context.worktree)
            .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
        let report = analyze_repository(&context.worktree, ANALYSIS_BUDGET)
            .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
        revalidate_repository_context(&context, DEPENDENCY_ENRICHMENT_ERROR)?;
        let cache = load_cache_in(&common_root, now_ms);
        let plan = build_enrichment_plan(
            &report,
            request.services,
            request.force_refresh,
            &cache,
            now_ms,
        )?;
        Ok((repository.canonical_key, plan))
    })
    .await?;

    let expires_at_ms = now_ms.saturating_add(PREVIEW_TTL_MS);
    let token = preview_token(
        &prepared.0,
        &prepared.1.revision,
        next_preview_sequence(),
        now_ms,
    );
    let preview = prepared.1.preview(token.clone(), expires_at_ms);
    preview_store()
        .lock()
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?
        .insert(
            StoredPreview {
                token,
                canonical_repository: prepared.0,
                expires_at_ms,
                plan: prepared.1,
            },
            now_ms,
        );
    Ok(preview)
}

#[tauri::command]
pub async fn dependency_enrichment_execute(
    request: DependencyEnrichmentExecuteRequest,
) -> Result<DependencyEnrichmentReport, String> {
    if !valid_preview_token(&request.preview_token) {
        return Err(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.into());
    }
    let _execution = ExecutionGuard::acquire()?;
    let now_ms = now_epoch_ms();
    let stored = preview_store()
        .lock()
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?
        .consume(&request.preview_token, now_ms)
        .ok_or_else(|| DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.to_string())?;
    validate_stored_plan(&request.path, &stored).await?;

    let client = production_client()?;
    let endpoints = EnrichmentEndpoints::production()?;
    let osv_coordinates = stored.plan.transmitted_osv_coordinates();
    let deps_coordinates = stored.plan.transmitted_deps_dev_coordinates();
    let (osv_network, deps_network) = join(
        fetch_osv(&client, &endpoints, &osv_coordinates),
        fetch_deps_dev(&client, &endpoints, &deps_coordinates),
    )
    .await;

    // Do not attach the response to a repository whose reviewed lock inputs
    // changed while the external services were in flight.
    validate_stored_plan(&request.path, &stored).await?;
    let completed_at_ms = now_epoch_ms();
    let mut resolved =
        resolve_enrichment(&stored.plan, osv_network, &deps_network, completed_at_ms);

    let common_root = devbox_integration::common_root();
    let updates = resolved.updates.clone();
    let cache_persisted = spawn_git_task(DEPENDENCY_ENRICHMENT_ERROR, move || {
        if updates.is_empty() {
            return Ok(true);
        }
        let mut cache = load_cache_in(&common_root, completed_at_ms);
        apply_cache_updates(&mut cache, &updates, completed_at_ms);
        Ok(write_cache_in(&common_root, &cache, completed_at_ms).is_ok())
    })
    .await
    .unwrap_or(false);
    resolved.report.cache_persisted = cache_persisted;
    Ok(resolved.report)
}

fn execution_active() -> &'static AtomicBool {
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    &ACTIVE
}

struct ExecutionGuard;

impl ExecutionGuard {
    fn acquire() -> Result<Self, String> {
        execution_active()
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| DEPENDENCY_ENRICHMENT_BUSY.to_string())?;
        Ok(Self)
    }
}

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        execution_active().store(false, Ordering::Release);
    }
}

async fn validate_stored_plan(path: &str, stored: &StoredPreview) -> Result<(), String> {
    let path = path.to_string();
    let expected_repository = stored.canonical_repository.clone();
    let expected_revision = stored.plan.revision.clone();
    spawn_git_task(DEPENDENCY_ENRICHMENT_ERROR, move || {
        let _analysis = dependency_analysis_lock()
            .try_lock()
            .map_err(|_| DEPENDENCY_ENRICHMENT_BUSY.to_string())?;
        let context = validated_repository_context(&path, DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED)?;
        let repository = repository_entry(&context.worktree)
            .map_err(|_| DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.to_string())?;
        if repository.canonical_key != expected_repository {
            return Err(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.into());
        }
        let report = analyze_repository(&context.worktree, ANALYSIS_BUDGET)
            .map_err(|_| DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.to_string())?;
        revalidate_repository_context(&context, DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED)?;
        if report.revision != expected_revision {
            return Err(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.into());
        }
        Ok(())
    })
    .await
}

fn production_client() -> Result<Client, String> {
    enrichment_client_builder()
        .https_only(true)
        .build()
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())
}

fn enrichment_client_builder() -> ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
}

async fn fetch_osv(
    client: &Client,
    endpoints: &EnrichmentEndpoints,
    coordinates: &[RemoteCoordinate],
) -> Result<Vec<ParsedOsvValue>, ()> {
    if coordinates.is_empty() {
        return Ok(Vec::new());
    }
    let queries = coordinates
        .iter()
        .map(|coordinate| {
            json!({
                "package": {
                    "name": coordinate.name,
                    "ecosystem": coordinate.osv_ecosystem,
                },
                "version": coordinate.version,
            })
        })
        .collect::<Vec<_>>();
    let response = client
        .post(endpoints.osv_query_batch.clone())
        .json(&json!({ "queries": queries }))
        .send()
        .await
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let bytes = read_response_bounded(response, MAX_OSV_RESPONSE_BYTES).await?;
    parse_osv_batch(&bytes, coordinates.len()).map_err(|_| ())
}

async fn fetch_deps_dev(
    client: &Client,
    endpoints: &EnrichmentEndpoints,
    coordinates: &[RemoteCoordinate],
) -> BTreeMap<String, Result<ParsedDepsDevValue, ()>> {
    let mut results = BTreeMap::new();
    for batch in coordinates.chunks(DEPS_DEV_BATCH_SIZE) {
        let futures = batch.iter().cloned().map(|coordinate| {
            let client = client.clone();
            let endpoints = endpoints.clone();
            async move {
                let key = coordinate.cache_key.clone();
                let result = fetch_deps_coordinate(&client, &endpoints, &coordinate).await;
                (key, result)
            }
        });
        for (key, result) in join_all(futures).await {
            results.insert(key, result);
        }
    }
    results
}

async fn fetch_deps_coordinate(
    client: &Client,
    endpoints: &EnrichmentEndpoints,
    coordinate: &RemoteCoordinate,
) -> Result<ParsedDepsDevValue, ()> {
    let version_url = endpoints.deps_version(coordinate)?;
    let package_url = endpoints.deps_package(coordinate)?;
    let (version, package) = join(
        fetch_optional(client, version_url, MAX_DEPS_VERSION_RESPONSE_BYTES),
        fetch_optional(client, package_url, MAX_DEPS_PACKAGE_RESPONSE_BYTES),
    )
    .await;
    let version = match version? {
        Some(bytes) => Some(parse_deps_version(&bytes).map_err(|_| ())?),
        None => None,
    };
    let package = match package? {
        Some(bytes) => Some(parse_deps_package(&bytes).map_err(|_| ())?),
        None => None,
    };
    Ok(combine_deps_dev(version, package))
}

async fn fetch_optional(
    client: &Client,
    url: Url,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, ()> {
    let response = client.get(url).send().await.map_err(|_| ())?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(());
    }
    read_response_bounded(response, max_bytes).await.map(Some)
}

async fn read_response_bounded(mut response: Response, max_bytes: usize) -> Result<Vec<u8>, ()> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn cache_path(common_root: &Path) -> PathBuf {
    common_root.join(CACHE_DIRECTORY).join(CACHE_FILE)
}

fn load_cache_in(common_root: &Path, now_ms: u64) -> EnrichmentCache {
    read_cache_bytes(common_root)
        .and_then(|bytes| parse_cache(&bytes, now_ms).ok())
        .unwrap_or_default()
}

fn read_cache_bytes(common_root: &Path) -> Option<Vec<u8>> {
    let path = cache_path(common_root);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return None,
    };
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_CACHE_BYTES as u64
        || devbox_filesystem::ensure_no_links(&path).is_err()
    {
        return None;
    }
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(&path, false).ok()?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let count = file.read(&mut chunk).ok()?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_CACHE_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if devbox_filesystem::ensure_no_links(&path).is_err()
        || devbox_filesystem::filesystem_identity(&path, false).ok()? != identity
        || file.metadata().ok()?.len() != bytes.len() as u64
    {
        return None;
    }
    Some(bytes)
}

fn write_cache_in(common_root: &Path, cache: &EnrichmentCache, now_ms: u64) -> Result<(), String> {
    let directory = common_root.join(CACHE_DIRECTORY);
    prepare_cache_directory(common_root, &directory)?;
    let path = directory.join(CACHE_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_file() => {
            return Err(DEPENDENCY_ENRICHMENT_ERROR.into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(DEPENDENCY_ENRICHMENT_ERROR.into()),
    }
    devbox_filesystem::ensure_no_links(&directory)
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    let bytes = serialize_cache(cache, now_ms)?;
    devbox_filesystem::atomic_write(&path, &bytes)
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    devbox_filesystem::ensure_no_links(&path).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())
}

fn prepare_cache_directory(common_root: &Path, directory: &Path) -> Result<(), String> {
    match fs::symlink_metadata(common_root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
            return Err(DEPENDENCY_ENRICHMENT_ERROR.into())
        }
        Ok(_) => devbox_filesystem::ensure_no_links(common_root)
            .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(common_root).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
            devbox_filesystem::ensure_no_links(common_root)
                .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
        }
        Err(_) => return Err(DEPENDENCY_ENRICHMENT_ERROR.into()),
    }
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
            return Err(DEPENDENCY_ENRICHMENT_ERROR.into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(directory).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
        }
        Err(_) => return Err(DEPENDENCY_ENRICHMENT_ERROR.into()),
    }
    devbox_filesystem::ensure_no_links(directory)
        .map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dependency_enrichment::{
        CachedDepsDevValue, EnrichmentService, EnrichmentValueState, PlannedDepsDevTarget,
        PlannedOsvTarget,
    };
    use std::io::Write as _;
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::process::Command;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::Instant;
    use tempfile::tempdir;

    struct FixtureResponse {
        status: &'static str,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl FixtureResponse {
        fn json(body: &str) -> Self {
            Self {
                status: "200 OK",
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: body.into(),
            }
        }

        fn status(status: &'static str) -> Self {
            Self {
                status,
                headers: Vec::new(),
                body: String::new(),
            }
        }
    }

    fn read_fixture_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4 * 1024];
        let mut expected_length = None;
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() < 64 * 1024);
            if expected_length.is_none() {
                if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    expected_length = Some(header_end + 4 + content_length);
                }
            }
            if expected_length.is_some_and(|length| bytes.len() >= length) {
                break;
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    fn write_fixture_response(stream: &mut TcpStream, response: FixtureResponse) {
        let mut headers = response
            .headers
            .into_iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect::<String>();
        headers.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
        headers.push_str("Connection: close\r\n");
        write!(
            stream,
            "HTTP/1.1 {}\r\n{}\r\n{}",
            response.status, headers, response.body
        )
        .unwrap();
        stream.flush().unwrap();
    }

    fn spawn_fixture_server<F>(
        expected_requests: usize,
        responder: F,
    ) -> (String, Receiver<String>, JoinHandle<()>)
    where
        F: Fn(&str) -> FixtureResponse + Send + 'static,
    {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut handled = 0usize;
            while handled < expected_requests && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = read_fixture_request(&mut stream);
                        let response = responder(&request);
                        let _ = sender.send(request);
                        write_fixture_response(&mut stream, response);
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
            assert_eq!(handled, expected_requests);
        });
        (format!("http://{address}/"), receiver, handle)
    }

    fn fixture_endpoints(base: &str) -> EnrichmentEndpoints {
        let root = Url::parse(base).unwrap();
        EnrichmentEndpoints {
            osv_query_batch: root.join("v1/querybatch").unwrap(),
            deps_dev_root: root.join("v3/").unwrap(),
        }
    }

    fn fixture_client(timeout: Duration) -> Client {
        enrichment_client_builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .unwrap()
    }

    fn coordinate() -> RemoteCoordinate {
        RemoteCoordinate {
            cache_key: "a".repeat(64),
            system: "NPM",
            osv_ecosystem: "npm",
            name: "@scope/package".into(),
            version: "1.2.3".into(),
            direct: true,
            package_ids: vec!["pnpm:@scope/package@1.2.3".into()],
        }
    }

    fn stored(token: &str, expires_at_ms: u64) -> StoredPreview {
        StoredPreview {
            token: token.into(),
            canonical_repository: "win:c:/repo".into(),
            expires_at_ms,
            plan: EnrichmentPlan {
                revision: "b".repeat(64),
                local_package_count: 1,
                osv: Some(vec![PlannedOsvTarget {
                    coordinate: coordinate(),
                    transmit: true,
                    cached: None,
                    stale_fallback: None,
                }]),
                deps_dev: None,
                osv_omitted_count: 0,
                deps_dev_omitted_count: 0,
            },
        }
    }

    #[test]
    fn preview_store_is_bounded_expiring_and_one_time() {
        let mut store = PreviewStore::default();
        for index in 0..=MAX_STORED_PREVIEWS {
            store.insert(stored(&format!("token-{index}"), 100 + index as u64), 1);
        }
        assert_eq!(store.entries.len(), MAX_STORED_PREVIEWS);
        assert!(store.consume("token-0", 1).is_none());
        assert!(store.consume("token-1", 1).is_some());
        assert!(store.consume("token-1", 1).is_none());
        assert!(store.consume("token-2", 200).is_none());
    }

    #[test]
    fn dependency_enrichment_execution_guard_is_single_flight_and_releases() {
        let first = ExecutionGuard::acquire().unwrap();
        assert_eq!(
            ExecutionGuard::acquire().err(),
            Some(DEPENDENCY_ENRICHMENT_BUSY.to_string())
        );
        drop(first);
        assert!(ExecutionGuard::acquire().is_ok());
    }

    #[test]
    fn deps_urls_percent_encode_scoped_names_and_accept_no_user_host() {
        let endpoints = EnrichmentEndpoints::production().unwrap();
        let coordinate = coordinate();
        let version = endpoints.deps_version(&coordinate).unwrap();
        assert_eq!(version.host_str(), Some(DEPS_DEV_HOST));
        assert_eq!(version.scheme(), "https");
        assert_eq!(
            version.as_str(),
            "https://api.deps.dev/v3/systems/NPM/packages/@scope%2Fpackage/versions/1.2.3"
        );
    }

    #[test]
    fn dependency_enrichment_fixture_sends_only_exact_reviewed_coordinates() {
        let (base, requests, server) = spawn_fixture_server(3, |request| {
            let request_line = request.lines().next().unwrap_or_default();
            if request_line == "POST /v1/querybatch HTTP/1.1" {
                FixtureResponse::json(r#"{"results":[{"vulns":[{"id":"GHSA-abcd"}]}]}"#)
            } else if request_line
                == "GET /v3/systems/NPM/packages/@scope%2Fpackage/versions/1.2.3 HTTP/1.1"
            {
                FixtureResponse::json(
                    r#"{"licenses":["MIT"],"isDeprecated":true,"advisoryKeys":[{"id":"GHSA-abcd"}]}"#,
                )
            } else if request_line == "GET /v3/systems/NPM/packages/@scope%2Fpackage HTTP/1.1" {
                FixtureResponse::json(
                    r#"{"versions":[{"versionKey":{"version":"2.0.0"},"isDefault":true}]}"#,
                )
            } else {
                FixtureResponse::status("404 Not Found")
            }
        });
        let client = fixture_client(Duration::from_secs(1));
        let endpoints = fixture_endpoints(&base);
        let coordinate = coordinate();
        let (osv, deps) = tauri::async_runtime::block_on(async {
            join(
                fetch_osv(&client, &endpoints, std::slice::from_ref(&coordinate)),
                fetch_deps_dev(&client, &endpoints, std::slice::from_ref(&coordinate)),
            )
            .await
        });
        server.join().unwrap();

        let osv = osv.unwrap();
        assert_eq!(osv[0].advisory_ids, ["GHSA-abcd"]);
        let deps = deps.get(&coordinate.cache_key).unwrap().as_ref().unwrap();
        assert_eq!(deps.licenses, ["MIT"]);
        assert_eq!(deps.default_version.as_deref(), Some("2.0.0"));
        assert!(deps.deprecated);

        let requests = requests.try_iter().collect::<Vec<_>>();
        let lines = requests
            .iter()
            .map(|request| request.lines().next().unwrap_or_default())
            .collect::<Vec<_>>();
        assert!(lines.contains(&"POST /v1/querybatch HTTP/1.1"));
        assert!(lines
            .contains(&"GET /v3/systems/NPM/packages/@scope%2Fpackage/versions/1.2.3 HTTP/1.1"));
        assert!(lines.contains(&"GET /v3/systems/NPM/packages/@scope%2Fpackage HTTP/1.1"));
        let osv_request = requests
            .iter()
            .find(|request| request.starts_with("POST /v1/querybatch "))
            .unwrap();
        let body = osv_request.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap(),
            json!({
                "queries": [{
                    "package": { "name": "@scope/package", "ecosystem": "npm" },
                    "version": "1.2.3",
                }],
            })
        );
        assert!(requests.iter().all(|request| request.contains(USER_AGENT)));
        assert!(requests
            .iter()
            .all(|request| !request.contains("repository")));
        assert!(requests
            .iter()
            .all(|request| !request.contains("credential")));
    }

    #[test]
    fn dependency_enrichment_client_rejects_redirects_and_times_out() {
        let destination = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        destination.set_nonblocking(true).unwrap();
        let location = format!("http://{}/followed", destination.local_addr().unwrap());
        let (base, _, redirect_server) = spawn_fixture_server(1, move |_| FixtureResponse {
            status: "302 Found",
            headers: vec![("Location".into(), location.clone())],
            body: String::new(),
        });
        let redirect_result = tauri::async_runtime::block_on(fetch_optional(
            &fixture_client(Duration::from_millis(250)),
            Url::parse(&base).unwrap(),
            1_024,
        ));
        redirect_server.join().unwrap();
        assert!(redirect_result.is_err());
        assert!(matches!(
            destination.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = Url::parse(&format!("http://{}/slow", listener.local_addr().unwrap())).unwrap();
        let slow_server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_fixture_request(&mut stream);
            thread::sleep(Duration::from_millis(120));
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            );
        });
        let timeout_result = tauri::async_runtime::block_on(fetch_optional(
            &fixture_client(Duration::from_millis(30)),
            url,
            1_024,
        ));
        assert!(timeout_result.is_err());
        slow_server.join().unwrap();
    }

    #[test]
    fn dependency_enrichment_partial_deps_failure_uses_only_that_stale_fallback() {
        let (base, _, server) = spawn_fixture_server(4, |request| {
            let line = request.lines().next().unwrap_or_default();
            if line.contains("/packages/bad/versions/") {
                FixtureResponse::status("503 Service Unavailable")
            } else if line.contains("/versions/") {
                FixtureResponse::json(
                    r#"{"licenses":["Apache-2.0"],"isDeprecated":false,"advisoryKeys":[]}"#,
                )
            } else {
                FixtureResponse::json(
                    r#"{"versions":[{"versionKey":{"version":"3.0.0"},"isDefault":true}]}"#,
                )
            }
        });
        let mut good = coordinate();
        good.cache_key = "b".repeat(64);
        good.name = "good".into();
        good.package_ids = vec!["npm:good@1.2.3".into()];
        let mut bad = coordinate();
        bad.cache_key = "c".repeat(64);
        bad.name = "bad".into();
        bad.package_ids = vec!["npm:bad@1.2.3".into()];
        let endpoints = fixture_endpoints(&base);
        let client = fixture_client(Duration::from_secs(1));
        let network = tauri::async_runtime::block_on(fetch_deps_dev(
            &client,
            &endpoints,
            &[good.clone(), bad.clone()],
        ));
        server.join().unwrap();
        assert!(network.get(&good.cache_key).unwrap().is_ok());
        assert!(network.get(&bad.cache_key).unwrap().is_err());

        let plan = EnrichmentPlan {
            revision: "b".repeat(64),
            local_package_count: 2,
            osv: None,
            deps_dev: Some(vec![
                PlannedDepsDevTarget {
                    coordinate: good,
                    transmit: true,
                    cached: None,
                    stale_fallback: None,
                },
                PlannedDepsDevTarget {
                    coordinate: bad,
                    transmit: true,
                    cached: None,
                    stale_fallback: Some(CachedDepsDevValue {
                        fetched_at_ms: 900,
                        licenses: vec!["MIT".into()],
                        default_version: Some("1.2.3".into()),
                        deprecated: false,
                        advisory_ids: Vec::new(),
                        version_found: true,
                        package_found: true,
                    }),
                },
            ]),
            osv_omitted_count: 0,
            deps_dev_omitted_count: 0,
        };
        let resolved = resolve_enrichment(&plan, Ok(Vec::new()), &network, 1_000);
        let states = resolved
            .report
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .package_ids
                    .iter()
                    .map(move |id| (id.as_str(), entry.deps_dev.state))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(states["npm:good@1.2.3"], EnrichmentValueState::Fresh);
        assert_eq!(states["npm:bad@1.2.3"], EnrichmentValueState::Stale);
        assert_eq!(resolved.report.services[0].stale_count, 1);
        assert_eq!(resolved.report.services[0].failed_count, 0);
    }

    #[test]
    fn dependency_enrichment_revision_change_requires_a_new_preview() {
        let root = tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success());
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\ndependencies = [\"serde\"]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let report = analyze_repository(root.path(), Duration::from_secs(2)).unwrap();
        let repository = repository_entry(root.path()).unwrap();
        let stored = StoredPreview {
            token: "d".repeat(64),
            canonical_repository: repository.canonical_key,
            expires_at_ms: u64::MAX,
            plan: EnrichmentPlan {
                revision: report.revision,
                local_package_count: report.package_count,
                osv: None,
                deps_dev: None,
                osv_omitted_count: 0,
                deps_dev_omitted_count: 0,
            },
        };
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.2.0\"\n",
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(validate_stored_plan(
            &root.path().to_string_lossy(),
            &stored,
        ));
        assert_eq!(result, Err(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.into()));
    }

    #[test]
    fn cache_io_round_trips_without_coordinate_text() {
        let root = tempdir().unwrap();
        let cache = EnrichmentCache {
            schema_version: 1,
            entries: vec![crate::core::dependency_enrichment::EnrichmentCacheEntry {
                key: "a".repeat(64),
                osv: Some(crate::core::dependency_enrichment::CachedOsvValue {
                    fetched_at_ms: 100,
                    advisory_ids: vec!["GHSA-abcd".into()],
                    truncated: false,
                }),
                deps_dev: None,
            }],
        };
        write_cache_in(root.path(), &cache, 100).unwrap();
        let loaded = load_cache_in(root.path(), 100);
        assert_eq!(loaded, cache);
        let text = fs::read_to_string(cache_path(root.path())).unwrap();
        assert!(!text.contains("@scope/package"));
    }

    #[cfg(unix)]
    #[test]
    fn cache_writer_rejects_symlinked_app_directory() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), root.path().join(CACHE_DIRECTORY)).unwrap();
        assert!(write_cache_in(root.path(), &EnrichmentCache::default(), 1).is_err());
    }

    #[test]
    fn empty_network_plan_resolves_without_remote_failure() {
        let plan = EnrichmentPlan {
            revision: "b".repeat(64),
            local_package_count: 1,
            osv: Some(Vec::new()),
            deps_dev: None,
            osv_omitted_count: 0,
            deps_dev_omitted_count: 0,
        };
        let resolved = resolve_enrichment(&plan, Ok(Vec::new()), &BTreeMap::new(), 100);
        assert_eq!(resolved.report.services[0].service, EnrichmentService::Osv);
        assert_eq!(resolved.report.services[0].failed_count, 0);
        assert!(resolved.report.entries.is_empty());
        assert!(resolved
            .report
            .entries
            .iter()
            .all(|entry| entry.osv.state != EnrichmentValueState::Failed));
    }
}
