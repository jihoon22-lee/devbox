//! Explicit official-release discovery/download. Renderer inputs are opaque
//! reviewed IDs; no URL, output path or executable is accepted from a webview.
use crate::core::suite_package::{Release, MAX_RELEASE_BYTES};
use devbox_filesystem::{ensure_no_links, filesystem_identity};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
const API: &str = "https://api.github.com/repos/jihoon22-lee/devbox/releases/latest";
#[derive(Clone)]
struct Review {
    id: String,
    release: Release,
    root: PathBuf,
    key: String,
    received: u64,
    state: &'static str,
    issue: Option<&'static str>,
    cancel: Arc<Cancellation>,
}
#[derive(Default)]
struct Cancellation {
    cancelled: AtomicBool,
    wake: tokio::sync::Notify,
}
#[derive(Default)]
pub(crate) struct Updates {
    review: Arc<Mutex<Option<Review>>>,
    checking: AtomicBool,
    launching: AtomicBool,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn view(review: &Review) -> Value {
    json!({"available":true,"id":review.id,"version":review.release.suite_version,"sourceSha":review.release.source_sha,
    "bytes":review.release.setup.size,"received":review.received,"state":review.state,"issue":review.issue})
}
fn version(value: &str) -> Result<Vec<u32>> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || part.len() > 8
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err("update_release_invalid");
    }
    parts
        .into_iter()
        .map(|part| part.parse().map_err(|_| "update_release_invalid"))
        .collect()
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("Devbox-Suite-Updater")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 4
                || !installation_tools::core::url_policy::is_allowed(attempt.url().as_str())
            {
                attempt.error("update_redirect_denied")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| "update_network_unavailable")
}
async fn bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|bytes| bytes > limit as u64)
    {
        return Err("update_download_failed");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "update_download_failed")?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("update_download_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn installation(app: &tauri::AppHandle) -> Result<(PathBuf, String)> {
    product_shell_tauri::require_suite_writable(app)?;
    let scope = crate::suite::capture_own("control-center", env!("CARGO_PKG_VERSION"))?;
    let root = PathBuf::from(scope.review_root());
    let owner = root.join("suite-owner.json");
    ensure_no_links(&owner).map_err(|_| "update_installed_suite_required")?;
    let bytes = fs::read(&owner).map_err(|_| "update_installed_suite_required")?;
    if bytes.len() > 4096 {
        return Err("update_owner_invalid");
    }
    let owner: Value = serde_json::from_slice(&bytes).map_err(|_| "update_owner_invalid")?;
    if owner["installationId"] != scope.manifest.installation_id
        || owner["generation"] != scope.manifest.generation
    {
        return Err("update_owner_changed");
    }
    scope.revalidate()?;
    Ok((root, scope.installation_key.clone()))
}
async fn check(app: &tauri::AppHandle) -> Result<Value> {
    let (root, key) = installation(app)?;
    let client = client()?;
    let response = client
        .get(API)
        .send()
        .await
        .map_err(|_| "update_network_unavailable")?;
    let api: Value = serde_json::from_slice(&bounded(response, 1024 * 1024).await?)
        .map_err(|_| "update_release_invalid")?;
    let tag = api["tag_name"].as_str().ok_or("update_release_invalid")?;
    let published = tag.strip_prefix('v').ok_or("update_release_invalid")?;
    if api["draft"] != false
        || api["prerelease"] != false
        || api["html_url"] != format!("https://github.com/jihoon22-lee/devbox/releases/tag/{tag}")
    {
        return Err("update_release_invalid");
    }
    if version(published)? <= version(&app.package_info().version.to_string())? {
        return Ok(json!({"available":false,"version":published}));
    }
    let assets = api["assets"].as_array().ok_or("update_release_invalid")?;
    let manifest = assets
        .iter()
        .find(|asset| asset["name"] == "release-manifest.json")
        .ok_or("update_manifest_missing")?;
    let url = format!(
        "https://github.com/jihoon22-lee/devbox/releases/download/{tag}/release-manifest.json"
    );
    let expected = manifest["digest"]
        .as_str()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| product_contract::commands::revision(value))
        .ok_or("update_manifest_digest_missing")?;
    if manifest["browser_download_url"] != url
        || manifest["size"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > MAX_RELEASE_BYTES as u64)
    {
        return Err("update_release_invalid");
    }
    let bytes = bounded(
        client
            .get(url)
            .send()
            .await
            .map_err(|_| "update_network_unavailable")?,
        MAX_RELEASE_BYTES,
    )
    .await?;
    if digest(&bytes) != expected || manifest["size"].as_u64() != Some(bytes.len() as u64) {
        return Err("update_manifest_changed");
    }
    let release = Release::parse(&bytes)?;
    if release.release_tag != tag {
        return Err("update_release_invalid");
    }
    let mut names = BTreeSet::from(["release-manifest.json".to_owned()]);
    for asset in std::iter::once(&release.setup)
        .chain(release.products.iter().map(|product| &product.portable))
        .chain(std::iter::once(&release.notices))
    {
        let remote = assets
            .iter()
            .find(|remote| remote["name"] == asset.name)
            .ok_or("update_asset_missing")?;
        if remote["browser_download_url"] != release.asset_url(asset)?
            || remote["size"].as_u64() != Some(asset.size)
            || remote["digest"] != format!("sha256:{}", asset.sha256)
            || !names.insert(asset.name.clone())
        {
            return Err("update_asset_changed");
        }
    }
    if assets.len() != names.len()
        || assets.iter().any(|asset| {
            asset["name"]
                .as_str()
                .is_none_or(|name| !names.contains(name))
        })
    {
        return Err("update_asset_topology_invalid");
    }
    let review = Review {
        id: digest(&bytes),
        release,
        root,
        key,
        received: 0,
        state: "reviewed",
        issue: None,
        cancel: Arc::new(Cancellation::default()),
    };
    let value = view(&review);
    let state = app.state::<Updates>();
    let mut current = state.review.lock().map_err(|_| "update_busy")?;
    if current
        .as_ref()
        .is_some_and(|review| review.state == "downloading")
    {
        return Err("update_busy");
    }
    *current = Some(review);
    Ok(value)
}
fn directory(review: &Review) -> Result<PathBuf> {
    let parent = dirs::data_local_dir().ok_or("update_cache_unavailable")?;
    ensure_no_links(&parent).map_err(|_| "update_cache_unsafe")?;
    let cache = parent.join(format!("com.devbox.v08.suite-downloads.i{}", review.key));
    for path in [&cache, &cache.join(&review.id)] {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("update_cache_unavailable"),
        }
        ensure_no_links(path).map_err(|_| "update_cache_unsafe")?;
        if path == &cache {
            crate::core::update_cache::prune_releases(&cache, &review.id)
                .map_err(|_| "update_cache_unavailable")?;
        }
    }
    let release = cache.join(&review.id);
    crate::core::update_cache::prune_partials(&release).map_err(|_| "update_cache_unavailable")?;
    Ok(release)
}

fn verify_file(path: &Path, review: &Review) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    ensure_no_links(path).map_err(|_| "update_cache_unsafe")?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(|_| "update_cache_unavailable")?;
    let identity = devbox_filesystem::opened_filesystem_identity(&file, false)
        .map_err(|_| "update_cache_unsafe")?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "update_cache_unavailable")?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > review.release.setup.size {
            return Err("update_asset_changed");
        }
        hash.update(&buffer[..count]);
    }
    let actual: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if received != review.release.setup.size
        || actual != review.release.setup.sha256
        || filesystem_identity(path, false).map_err(|_| "update_asset_changed")? != identity
    {
        return Err("update_asset_changed");
    }
    Ok(file)
}
fn progress(
    state: &Arc<Mutex<Option<Review>>>,
    id: &str,
    received: u64,
    status: &'static str,
    issue: Option<&'static str>,
) {
    if let Ok(mut state) = state.lock() {
        if let Some(review) = state.as_mut().filter(|review| review.id == id) {
            review.received = received;
            review.state = status;
            review.issue = issue;
        }
    }
}
async fn download(review: Review, state: Arc<Mutex<Option<Review>>>) -> Result<()> {
    let directory = directory(&review)?;
    let _pins = crate::suite::platform::component_scope::pin_directories(&directory)?;
    let destination = directory.join(&review.release.setup.name);
    if destination.exists() {
        verify_file(&destination, &review)?;
        progress(&state, &review.id, review.release.setup.size, "ready", None);
        return Ok(());
    }
    let temporary = directory.join(format!(".partial-{}", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "update_cache_unavailable")?;
    let identity = devbox_filesystem::opened_filesystem_identity(&file, false)
        .map_err(|_| "update_cache_unavailable")?;
    let result=async {
        let request=client()?.get(review.release.asset_url(&review.release.setup)?);
        let mut response=tokio::select! {response=request.send()=>response.map_err(|_|"update_network_unavailable")?,_ =review.cancel.wake.notified()=>return Err("update_download_cancelled")};
        if !response.status().is_success()||response.content_length().is_some_and(|bytes|bytes!=review.release.setup.size) {return Err("update_download_failed");}
        let mut received=0u64;let mut hash=Sha256::new();
        loop {
            if review.cancel.cancelled.load(Ordering::Acquire) {return Err("update_download_cancelled");}
            let chunk=tokio::select! {chunk=response.chunk()=>chunk.map_err(|_|"update_download_failed")?,_ =review.cancel.wake.notified()=>return Err("update_download_cancelled")};
            let Some(chunk)=chunk else {break;};
            received+=chunk.len() as u64;if received>review.release.setup.size {return Err("update_download_large");}
            file.write_all(&chunk).map_err(|_|"update_cache_unavailable")?;hash.update(&chunk);
            progress(&state,&review.id,received,"downloading",None);
        }
        let actual:String=hash.finalize().iter().map(|byte|format!("{byte:02x}")).collect();
        if review.cancel.cancelled.load(Ordering::Acquire) {return Err("update_download_cancelled");}
        if received!=review.release.setup.size||actual!=review.release.setup.sha256 {return Err("update_asset_changed");}
        file.sync_all().map_err(|_|"update_cache_unavailable")?;
        if filesystem_identity(&temporary,false).map_err(|_|"update_cache_changed")?!=identity {return Err("update_cache_changed");}
        fs::hard_link(&temporary,&destination).map_err(|_|"update_cache_conflict")?;
        Ok(received)
    }.await;
    drop(file);
    if filesystem_identity(&temporary, false).ok() == Some(identity) {
        let _ = fs::remove_file(&temporary);
    }
    let received = result?;
    verify_file(&destination, &review)?;
    progress(&state, &review.id, received, "ready", None);
    Ok(())
}
pub(crate) async fn execute(
    app: tauri::AppHandle,
    method: &str,
    args: Value,
    deadline: u64,
) -> Result<Value> {
    let state = app.state::<Updates>();
    if method == "check_suite_update" {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "update_clock_invalid")?
            .as_millis() as u64;
        if state.checking.swap(true, Ordering::AcqRel) {
            return Err("update_busy");
        }
        let result = tokio::time::timeout(
            Duration::from_millis(deadline.saturating_sub(now).min(29000)),
            check(&app),
        )
        .await
        .map_err(|_| "update_network_timeout")
        .and_then(|value| value);
        state.checking.store(false, Ordering::Release);
        return result;
    }
    let id = args["id"]
        .as_str()
        .filter(|id| product_contract::commands::revision(id))
        .ok_or("update_review_invalid")?;
    let mut review = state
        .review
        .lock()
        .map_err(|_| "update_busy")?
        .as_ref()
        .filter(|review| review.id == id)
        .cloned()
        .ok_or("update_review_expired")?;
    match method {
        "suite_update_status" => Ok(view(&review)),
        "cancel_suite_update" => {
            review.cancel.cancelled.store(true, Ordering::Release);
            review.cancel.wake.notify_one();
            Ok(view(&review))
        }
        "download_suite_update" => {
            let (root, key) = installation(&app)?;
            if root != review.root || key != review.key {
                return Err("update_owner_changed");
            }
            {
                let mut current = state.review.lock().map_err(|_| "update_busy")?;
                let current = current
                    .as_mut()
                    .filter(|current| current.id == id)
                    .ok_or("update_review_expired")?;
                if current.state == "downloading" || current.state == "ready" {
                    return Ok(view(current));
                }
                current.cancel = Arc::new(Cancellation::default());
                current.state = "downloading";
                current.received = 0;
                current.issue = None;
                review = current.clone();
            }
            let value = view(&review);
            let retained = state.review.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(issue) = download(review.clone(), retained.clone()).await {
                    progress(
                        &retained,
                        &review.id,
                        0,
                        if issue == "update_download_cancelled" {
                            "cancelled"
                        } else {
                            "failed"
                        },
                        Some(issue),
                    );
                }
            });
            Ok(value)
        }
        "launch_suite_update" => {
            if review.state != "ready" {
                return Err("update_download_required");
            }
            let (root, key) = installation(&app)?;
            if root != review.root || key != review.key {
                return Err("update_owner_changed");
            }
            if state.launching.swap(true, Ordering::AcqRel) {
                return Err("update_already_launched");
            }
            let result = (|| {
                use std::os::windows::process::CommandExt;
                let directory = directory(&review)?;
                let _pins = crate::suite::platform::component_scope::pin_directories(&directory)?;
                let installer = directory.join(&review.release.setup.name);
                let _image = verify_file(&installer, &review)?;
                std::process::Command::new(installer)
                    .raw_arg(format!("/D={}", root.to_string_lossy()))
                    .spawn()
                    .map_err(|_| "update_launch_failed")?;
                let app = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(750));
                    app.exit(0);
                });
                Ok(json!({"accepted":true}))
            })();
            if result.is_err() {
                state.launching.store(false, Ordering::Release);
            }
            result
        }
        _ => Err("update_request_invalid"),
    }
}
