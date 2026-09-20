//! Privacy-reviewed remote metadata planning and bounded response parsing.
//!
//! This module contains no network or repository I/O. The command adapter owns
//! those effects and can only execute an immutable plan produced here from a
//! freshly validated offline report.

use super::dependency_lens::{
    validated_package_name, validated_version_text, DependencyEcosystem, DependencyReport,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const DEPENDENCY_ENRICHMENT_ERROR: &str = "Dependency Lens 원격 정보를 불러오지 못했습니다.";
pub const DEPENDENCY_ENRICHMENT_BUSY: &str =
    "다른 Dependency Lens 분석 또는 원격 조회가 진행 중입니다.";
pub const DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED: &str = "전송 내용을 다시 검토해 주세요.";

pub const OSV_HOST: &str = "api.osv.dev";
pub const DEPS_DEV_HOST: &str = "api.deps.dev";
pub const MAX_OSV_TARGETS: usize = 256;
pub const MAX_DEPS_DEV_TARGETS: usize = 48;
pub const MAX_CACHE_ENTRIES: usize = 2_048;
pub const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_OSV_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DEPS_VERSION_RESPONSE_BYTES: usize = 512 * 1024;
pub const MAX_DEPS_PACKAGE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const PREVIEW_TTL_MS: u64 = 5 * 60 * 1_000;
pub const FRESH_CACHE_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const STALE_CACHE_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;
const MAX_PACKAGE_IDS_PER_COORDINATE: usize = 128;
const MAX_ADVISORIES_PER_COORDINATE: usize = 128;
const MAX_LICENSES_PER_COORDINATE: usize = 16;
const MAX_LICENSE_BYTES: usize = 128;
const MAX_ADVISORY_ID_BYTES: usize = 128;
const MAX_REMOTE_PACKAGE_VERSIONS: usize = 20_000;
const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnrichmentService {
    Osv,
    DepsDev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnrichmentSelection {
    pub osv: bool,
    pub deps_dev: bool,
}

impl EnrichmentSelection {
    pub fn any(self) -> bool {
        self.osv || self.deps_dev
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentCoordinatePreview {
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    pub direct: bool,
    pub local_package_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentServicePreview {
    pub service: EnrichmentService,
    pub host: String,
    pub transmitted: Vec<EnrichmentCoordinatePreview>,
    pub cached_count: usize,
    pub stale_fallback_count: usize,
    pub omitted_count: usize,
    pub request_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEnrichmentPreview {
    pub token: String,
    pub revision: String,
    pub expires_at_ms: u64,
    pub services: Vec<EnrichmentServicePreview>,
    pub local_package_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnrichmentValueState {
    Fresh,
    Cached,
    Stale,
    Failed,
    NotRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsvEnrichmentValue {
    pub state: EnrichmentValueState,
    pub fetched_at_ms: Option<u64>,
    pub age_ms: Option<u64>,
    pub advisory_ids: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepsDevEnrichmentValue {
    pub state: EnrichmentValueState,
    pub fetched_at_ms: Option<u64>,
    pub age_ms: Option<u64>,
    pub licenses: Vec<String>,
    pub default_version: Option<String>,
    pub deprecated: bool,
    pub advisory_ids: Vec<String>,
    pub version_found: bool,
    pub package_found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEnrichmentEntry {
    pub package_ids: Vec<String>,
    pub osv: OsvEnrichmentValue,
    pub deps_dev: DepsDevEnrichmentValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentServiceSummary {
    pub service: EnrichmentService,
    pub target_count: usize,
    pub transmitted_count: usize,
    pub cached_count: usize,
    pub stale_count: usize,
    pub failed_count: usize,
    pub omitted_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEnrichmentReport {
    pub revision: String,
    pub completed_at_ms: u64,
    pub local_authoritative: bool,
    pub cache_persisted: bool,
    pub entries: Vec<DependencyEnrichmentEntry>,
    pub services: Vec<EnrichmentServiceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RemoteCoordinate {
    pub cache_key: String,
    pub system: &'static str,
    pub osv_ecosystem: &'static str,
    pub name: String,
    pub version: String,
    pub direct: bool,
    pub package_ids: Vec<String>,
}

impl RemoteCoordinate {
    fn preview(&self, service: EnrichmentService) -> EnrichmentCoordinatePreview {
        EnrichmentCoordinatePreview {
            ecosystem: match service {
                EnrichmentService::Osv => self.osv_ecosystem,
                EnrichmentService::DepsDev => self.system,
            }
            .to_string(),
            name: self.name.clone(),
            version: self.version.clone(),
            direct: self.direct,
            local_package_count: self.package_ids.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedOsvValue {
    pub fetched_at_ms: u64,
    pub advisory_ids: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachedDepsDevValue {
    pub fetched_at_ms: u64,
    pub licenses: Vec<String>,
    pub default_version: Option<String>,
    pub deprecated: bool,
    pub advisory_ids: Vec<String>,
    pub version_found: bool,
    pub package_found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnrichmentCacheEntry {
    pub key: String,
    pub osv: Option<CachedOsvValue>,
    pub deps_dev: Option<CachedDepsDevValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnrichmentCache {
    pub schema_version: u32,
    pub entries: Vec<EnrichmentCacheEntry>,
}

impl Default for EnrichmentCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOsvTarget {
    pub coordinate: RemoteCoordinate,
    pub transmit: bool,
    pub cached: Option<CachedOsvValue>,
    pub stale_fallback: Option<CachedOsvValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDepsDevTarget {
    pub coordinate: RemoteCoordinate,
    pub transmit: bool,
    pub cached: Option<CachedDepsDevValue>,
    pub stale_fallback: Option<CachedDepsDevValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichmentPlan {
    pub revision: String,
    pub local_package_count: usize,
    pub osv: Option<Vec<PlannedOsvTarget>>,
    pub deps_dev: Option<Vec<PlannedDepsDevTarget>>,
    pub osv_omitted_count: usize,
    pub deps_dev_omitted_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOsvValue {
    pub advisory_ids: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDepsVersion {
    pub licenses: Vec<String>,
    pub deprecated: bool,
    pub advisory_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDepsPackage {
    pub default_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDepsDevValue {
    pub licenses: Vec<String>,
    pub default_version: Option<String>,
    pub deprecated: bool,
    pub advisory_ids: Vec<String>,
    pub version_found: bool,
    pub package_found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichmentCacheUpdate {
    pub key: String,
    pub osv: Option<CachedOsvValue>,
    pub deps_dev: Option<CachedDepsDevValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEnrichment {
    pub report: DependencyEnrichmentReport,
    pub updates: Vec<EnrichmentCacheUpdate>,
}

pub fn build_enrichment_plan(
    report: &DependencyReport,
    selection: EnrichmentSelection,
    force_refresh: bool,
    cache: &EnrichmentCache,
    now_ms: u64,
) -> Result<EnrichmentPlan, String> {
    if !selection.any() {
        return Err(DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED.into());
    }
    validate_cache(cache, now_ms)?;
    let coordinates = collect_remote_coordinates(report);
    let cache_by_key = cache
        .entries
        .iter()
        .map(|entry| (entry.key.as_str(), entry))
        .collect::<BTreeMap<_, _>>();

    let (osv, osv_omitted_count) = if selection.osv {
        let selected = coordinates
            .iter()
            .take(MAX_OSV_TARGETS)
            .cloned()
            .collect::<Vec<_>>();
        let omitted = coordinates.len().saturating_sub(selected.len());
        let targets = selected
            .into_iter()
            .map(|coordinate| {
                let cached_value = cache_by_key
                    .get(coordinate.cache_key.as_str())
                    .and_then(|entry| entry.osv.clone())
                    .filter(|value| cache_value_is_usable(value.fetched_at_ms, now_ms));
                let safely_fresh = cached_value.as_ref().is_some_and(|value| {
                    cache_value_is_fresh_through_preview(value.fetched_at_ms, now_ms)
                });
                PlannedOsvTarget {
                    coordinate,
                    transmit: force_refresh || !safely_fresh,
                    cached: (!force_refresh && safely_fresh)
                        .then(|| cached_value.clone())
                        .flatten(),
                    stale_fallback: cached_value,
                }
            })
            .collect();
        (Some(targets), omitted)
    } else {
        (None, 0)
    };

    let direct_coordinates = coordinates
        .iter()
        .filter(|coordinate| coordinate.direct)
        .cloned()
        .collect::<Vec<_>>();
    let (deps_dev, deps_dev_omitted_count) = if selection.deps_dev {
        let selected = direct_coordinates
            .iter()
            .take(MAX_DEPS_DEV_TARGETS)
            .cloned()
            .collect::<Vec<_>>();
        let omitted = direct_coordinates.len().saturating_sub(selected.len());
        let targets = selected
            .into_iter()
            .map(|coordinate| {
                let cached_value = cache_by_key
                    .get(coordinate.cache_key.as_str())
                    .and_then(|entry| entry.deps_dev.clone())
                    .filter(|value| cache_value_is_usable(value.fetched_at_ms, now_ms));
                let safely_fresh = cached_value.as_ref().is_some_and(|value| {
                    cache_value_is_fresh_through_preview(value.fetched_at_ms, now_ms)
                });
                PlannedDepsDevTarget {
                    coordinate,
                    transmit: force_refresh || !safely_fresh,
                    cached: (!force_refresh && safely_fresh)
                        .then(|| cached_value.clone())
                        .flatten(),
                    stale_fallback: cached_value,
                }
            })
            .collect();
        (Some(targets), omitted)
    } else {
        (None, 0)
    };

    Ok(EnrichmentPlan {
        revision: report.revision.clone(),
        local_package_count: report.package_count,
        osv,
        deps_dev,
        osv_omitted_count,
        deps_dev_omitted_count,
    })
}

impl EnrichmentPlan {
    pub fn preview(&self, token: String, expires_at_ms: u64) -> DependencyEnrichmentPreview {
        let mut services = Vec::new();
        if let Some(targets) = &self.osv {
            let transmitted = targets
                .iter()
                .filter(|target| target.transmit)
                .map(|target| target.coordinate.preview(EnrichmentService::Osv))
                .collect::<Vec<_>>();
            services.push(EnrichmentServicePreview {
                service: EnrichmentService::Osv,
                host: OSV_HOST.to_string(),
                request_count: usize::from(!transmitted.is_empty()),
                cached_count: targets.iter().filter(|target| !target.transmit).count(),
                stale_fallback_count: targets
                    .iter()
                    .filter(|target| target.transmit && target.stale_fallback.is_some())
                    .count(),
                omitted_count: self.osv_omitted_count,
                transmitted,
            });
        }
        if let Some(targets) = &self.deps_dev {
            let transmitted = targets
                .iter()
                .filter(|target| target.transmit)
                .map(|target| target.coordinate.preview(EnrichmentService::DepsDev))
                .collect::<Vec<_>>();
            services.push(EnrichmentServicePreview {
                service: EnrichmentService::DepsDev,
                host: DEPS_DEV_HOST.to_string(),
                request_count: transmitted.len().saturating_mul(2),
                cached_count: targets.iter().filter(|target| !target.transmit).count(),
                stale_fallback_count: targets
                    .iter()
                    .filter(|target| target.transmit && target.stale_fallback.is_some())
                    .count(),
                omitted_count: self.deps_dev_omitted_count,
                transmitted,
            });
        }
        DependencyEnrichmentPreview {
            token,
            revision: self.revision.clone(),
            expires_at_ms,
            services,
            local_package_count: self.local_package_count,
        }
    }

    pub fn transmitted_osv_coordinates(&self) -> Vec<RemoteCoordinate> {
        self.osv
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|target| target.transmit)
            .map(|target| target.coordinate.clone())
            .collect()
    }

    pub fn transmitted_deps_dev_coordinates(&self) -> Vec<RemoteCoordinate> {
        self.deps_dev
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|target| target.transmit)
            .map(|target| target.coordinate.clone())
            .collect()
    }
}

fn collect_remote_coordinates(report: &DependencyReport) -> Vec<RemoteCoordinate> {
    #[derive(Debug, Clone)]
    struct CoordinateBuilder {
        system: &'static str,
        osv_ecosystem: &'static str,
        name: String,
        version: String,
        direct: bool,
        package_ids: BTreeSet<String>,
    }

    let mut coordinates = BTreeMap::<(String, String, String), CoordinateBuilder>::new();
    for package in &report.packages {
        let Some((system, osv_ecosystem)) = ecosystem_mapping(package.ecosystem) else {
            continue;
        };
        let Some(name) = validated_package_name(&package.name) else {
            continue;
        };
        let Some(version) = validated_version_text(&package.version) else {
            continue;
        };
        let key = (system.to_string(), name.clone(), version.clone());
        let entry = coordinates.entry(key).or_insert_with(|| CoordinateBuilder {
            system,
            osv_ecosystem,
            name,
            version,
            direct: false,
            package_ids: BTreeSet::new(),
        });
        entry.direct |= package.direct;
        if entry.package_ids.len() < MAX_PACKAGE_IDS_PER_COORDINATE {
            entry.package_ids.insert(package.id.clone());
        }
    }

    let mut coordinates = coordinates
        .into_values()
        .map(|builder| RemoteCoordinate {
            cache_key: coordinate_cache_key(builder.system, &builder.name, &builder.version),
            system: builder.system,
            osv_ecosystem: builder.osv_ecosystem,
            name: builder.name,
            version: builder.version,
            direct: builder.direct,
            package_ids: builder.package_ids.into_iter().collect(),
        })
        .collect::<Vec<_>>();
    coordinates.sort_by(|left, right| {
        right
            .direct
            .cmp(&left.direct)
            .then_with(|| left.system.cmp(right.system))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.version.cmp(&right.version))
    });
    coordinates
}

fn ecosystem_mapping(ecosystem: DependencyEcosystem) -> Option<(&'static str, &'static str)> {
    match ecosystem {
        DependencyEcosystem::Cargo => Some(("CARGO", "crates.io")),
        DependencyEcosystem::Pnpm | DependencyEcosystem::Npm => Some(("NPM", "npm")),
        DependencyEcosystem::Python => Some(("PYPI", "PyPI")),
        DependencyEcosystem::Gradle => None,
    }
}

fn coordinate_cache_key(system: &str, name: &str, version: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"dependency-enrichment-coordinate/v1");
    digest.update([0]);
    digest.update(system.as_bytes());
    digest.update([0]);
    digest.update(name.as_bytes());
    digest.update([0]);
    digest.update(version.as_bytes());
    lower_hex(&digest.finalize())
}

pub fn preview_token(
    canonical_repository: &str,
    revision: &str,
    sequence: u64,
    now_ms: u64,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"dependency-enrichment-preview/v1");
    digest.update([0]);
    digest.update(canonical_repository.as_bytes());
    digest.update([0]);
    digest.update(revision.as_bytes());
    digest.update([0]);
    digest.update(sequence.to_le_bytes());
    digest.update(now_ms.to_le_bytes());
    lower_hex(&digest.finalize())
}

pub fn valid_preview_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn cache_value_is_usable(fetched_at_ms: u64, now_ms: u64) -> bool {
    fetched_at_ms <= now_ms.saturating_add(MAX_CLOCK_SKEW_MS)
        && now_ms.saturating_sub(fetched_at_ms) <= STALE_CACHE_TTL_MS
}

fn cache_value_is_fresh_through_preview(fetched_at_ms: u64, now_ms: u64) -> bool {
    cache_value_is_usable(fetched_at_ms, now_ms)
        && now_ms
            .saturating_sub(fetched_at_ms)
            .saturating_add(PREVIEW_TTL_MS)
            <= FRESH_CACHE_TTL_MS
}

pub fn parse_cache(bytes: &[u8], now_ms: u64) -> Result<EnrichmentCache, String> {
    if bytes.len() > MAX_CACHE_BYTES {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let mut cache: EnrichmentCache =
        serde_json::from_slice(bytes).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    validate_cache(&cache, now_ms)?;
    prune_cache(&mut cache, now_ms);
    Ok(cache)
}

pub fn serialize_cache(cache: &EnrichmentCache, now_ms: u64) -> Result<Vec<u8>, String> {
    let mut cache = cache.clone();
    prune_cache(&mut cache, now_ms);
    validate_cache(&cache, now_ms)?;
    let bytes =
        serde_json::to_vec_pretty(&cache).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    if bytes.len() > MAX_CACHE_BYTES {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    Ok(bytes)
}

pub fn apply_cache_updates(
    cache: &mut EnrichmentCache,
    updates: &[EnrichmentCacheUpdate],
    now_ms: u64,
) {
    let mut entries = cache
        .entries
        .drain(..)
        .map(|entry| (entry.key.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    for update in updates {
        let entry = entries
            .entry(update.key.clone())
            .or_insert_with(|| EnrichmentCacheEntry {
                key: update.key.clone(),
                osv: None,
                deps_dev: None,
            });
        if let Some(value) = &update.osv {
            entry.osv = Some(value.clone());
        }
        if let Some(value) = &update.deps_dev {
            entry.deps_dev = Some(value.clone());
        }
    }
    cache.schema_version = CACHE_SCHEMA_VERSION;
    cache.entries = entries.into_values().collect();
    prune_cache(cache, now_ms);
}

fn prune_cache(cache: &mut EnrichmentCache, now_ms: u64) {
    for entry in &mut cache.entries {
        if entry
            .osv
            .as_ref()
            .is_some_and(|value| !cache_value_is_usable(value.fetched_at_ms, now_ms))
        {
            entry.osv = None;
        }
        if entry
            .deps_dev
            .as_ref()
            .is_some_and(|value| !cache_value_is_usable(value.fetched_at_ms, now_ms))
        {
            entry.deps_dev = None;
        }
    }
    cache
        .entries
        .retain(|entry| entry.osv.is_some() || entry.deps_dev.is_some());
    cache.entries.sort_by(|left, right| {
        cache_entry_timestamp(right)
            .cmp(&cache_entry_timestamp(left))
            .then_with(|| left.key.cmp(&right.key))
    });
    cache.entries.truncate(MAX_CACHE_ENTRIES);
    cache
        .entries
        .sort_by(|left, right| left.key.cmp(&right.key));
}

fn cache_entry_timestamp(entry: &EnrichmentCacheEntry) -> u64 {
    entry
        .osv
        .as_ref()
        .map(|value| value.fetched_at_ms)
        .into_iter()
        .chain(entry.deps_dev.as_ref().map(|value| value.fetched_at_ms))
        .max()
        .unwrap_or(0)
}

fn validate_cache(cache: &EnrichmentCache, now_ms: u64) -> Result<(), String> {
    if cache.schema_version != CACHE_SCHEMA_VERSION || cache.entries.len() > MAX_CACHE_ENTRIES {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let mut previous = None::<&str>;
    for entry in &cache.entries {
        if !valid_cache_key(&entry.key)
            || previous.is_some_and(|value| value >= entry.key.as_str())
            || (entry.osv.is_none() && entry.deps_dev.is_none())
        {
            return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
        }
        previous = Some(&entry.key);
        if let Some(value) = &entry.osv {
            if !cache_value_is_usable(value.fetched_at_ms, now_ms)
                || !valid_advisory_ids(&value.advisory_ids)
            {
                return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
            }
        }
        if let Some(value) = &entry.deps_dev {
            if !cache_value_is_usable(value.fetched_at_ms, now_ms)
                || !valid_licenses(&value.licenses)
                || !valid_advisory_ids(&value.advisory_ids)
                || value
                    .default_version
                    .as_deref()
                    .is_some_and(|version| validated_version_text(version).is_none())
                || (!value.package_found && value.default_version.is_some())
            {
                return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
            }
        }
    }
    Ok(())
}

fn valid_cache_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_advisory_ids(values: &[String]) -> bool {
    values.len() <= MAX_ADVISORIES_PER_COORDINATE
        && strictly_sorted(values)
        && values.iter().all(|value| {
            !value.is_empty()
                && value.len() <= MAX_ADVISORY_ID_BYTES
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
                })
        })
}

fn valid_licenses(values: &[String]) -> bool {
    values.len() <= MAX_LICENSES_PER_COORDINATE
        && strictly_sorted(values)
        && values.iter().all(|value| {
            !value.is_empty()
                && value.len() <= MAX_LICENSE_BYTES
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b' ' | b'-' | b'_' | b'.' | b'+' | b'(' | b')' | b':' | b'/'
                        )
                })
        })
}

fn strictly_sorted(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[derive(Deserialize)]
struct RawOsvBatch {
    results: Vec<RawOsvResult>,
}

#[derive(Deserialize)]
struct RawOsvResult {
    #[serde(default)]
    vulns: Vec<RawAdvisory>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct RawAdvisory {
    id: String,
}

pub fn parse_osv_batch(bytes: &[u8], expected: usize) -> Result<Vec<ParsedOsvValue>, String> {
    if bytes.len() > MAX_OSV_RESPONSE_BYTES || expected > MAX_OSV_TARGETS {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let raw: RawOsvBatch =
        serde_json::from_slice(bytes).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    if raw.results.len() != expected {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    raw.results
        .into_iter()
        .map(|result| {
            if result.vulns.len() > MAX_ADVISORIES_PER_COORDINATE {
                return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
            }
            let mut advisory_ids = result
                .vulns
                .into_iter()
                .map(|advisory| advisory.id)
                .collect::<Vec<_>>();
            advisory_ids.sort();
            advisory_ids.dedup();
            if !valid_advisory_ids(&advisory_ids) {
                return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
            }
            let truncated = result
                .next_page_token
                .is_some_and(|token| !token.is_empty());
            Ok(ParsedOsvValue {
                advisory_ids,
                truncated,
            })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDepsVersion {
    #[serde(default)]
    licenses: Vec<String>,
    #[serde(default)]
    is_deprecated: bool,
    #[serde(default)]
    advisory_keys: Vec<RawAdvisory>,
}

pub fn parse_deps_version(bytes: &[u8]) -> Result<ParsedDepsVersion, String> {
    if bytes.len() > MAX_DEPS_VERSION_RESPONSE_BYTES {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let raw: RawDepsVersion =
        serde_json::from_slice(bytes).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    if raw.licenses.len() > MAX_LICENSES_PER_COORDINATE
        || raw.advisory_keys.len() > MAX_ADVISORIES_PER_COORDINATE
    {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let mut licenses = raw.licenses;
    licenses.sort();
    licenses.dedup();
    let mut advisory_ids = raw
        .advisory_keys
        .into_iter()
        .map(|advisory| advisory.id)
        .collect::<Vec<_>>();
    advisory_ids.sort();
    advisory_ids.dedup();
    if !valid_licenses(&licenses) || !valid_advisory_ids(&advisory_ids) {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    Ok(ParsedDepsVersion {
        licenses,
        deprecated: raw.is_deprecated,
        advisory_ids,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDepsPackage {
    #[serde(default)]
    versions: Vec<RawDepsPackageVersion>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDepsPackageVersion {
    version_key: RawVersionKey,
    #[serde(default)]
    is_default: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVersionKey {
    version: String,
}

pub fn parse_deps_package(bytes: &[u8]) -> Result<ParsedDepsPackage, String> {
    if bytes.len() > MAX_DEPS_PACKAGE_RESPONSE_BYTES {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let raw: RawDepsPackage =
        serde_json::from_slice(bytes).map_err(|_| DEPENDENCY_ENRICHMENT_ERROR.to_string())?;
    if raw.versions.len() > MAX_REMOTE_PACKAGE_VERSIONS {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    let defaults = raw
        .versions
        .into_iter()
        .filter(|version| version.is_default)
        .map(|version| {
            validated_version_text(&version.version_key.version)
                .ok_or_else(|| DEPENDENCY_ENRICHMENT_ERROR.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if defaults.len() > 1 {
        return Err(DEPENDENCY_ENRICHMENT_ERROR.into());
    }
    Ok(ParsedDepsPackage {
        default_version: defaults.into_iter().next(),
    })
}

pub fn combine_deps_dev(
    version: Option<ParsedDepsVersion>,
    package: Option<ParsedDepsPackage>,
) -> ParsedDepsDevValue {
    let version_found = version.is_some();
    let package_found = package.is_some();
    let version = version.unwrap_or(ParsedDepsVersion {
        licenses: Vec::new(),
        deprecated: false,
        advisory_ids: Vec::new(),
    });
    ParsedDepsDevValue {
        licenses: version.licenses,
        default_version: package.and_then(|value| value.default_version),
        deprecated: version.deprecated,
        advisory_ids: version.advisory_ids,
        version_found,
        package_found,
    }
}

pub fn resolve_enrichment(
    plan: &EnrichmentPlan,
    osv_network: Result<Vec<ParsedOsvValue>, ()>,
    deps_dev_network: &BTreeMap<String, Result<ParsedDepsDevValue, ()>>,
    completed_at_ms: u64,
) -> ResolvedEnrichment {
    let mut entries = BTreeMap::<String, DependencyEnrichmentEntry>::new();
    let mut updates = BTreeMap::<String, EnrichmentCacheUpdate>::new();
    let mut summaries = Vec::new();

    if let Some(targets) = &plan.osv {
        let mut network_values = osv_network.ok().unwrap_or_default().into_iter();
        let mut stale_count = 0usize;
        let mut failed_count = 0usize;
        for target in targets {
            let (value, update) = if !target.transmit {
                if let Some(cached) = &target.cached {
                    (
                        osv_from_cache(cached, completed_at_ms, EnrichmentValueState::Cached),
                        None,
                    )
                } else {
                    failed_count += 1;
                    (empty_osv(EnrichmentValueState::Failed), None)
                }
            } else if let Some(network) = network_values.next() {
                let cached = CachedOsvValue {
                    fetched_at_ms: completed_at_ms,
                    advisory_ids: network.advisory_ids,
                    truncated: network.truncated,
                };
                (
                    osv_from_cache(&cached, completed_at_ms, EnrichmentValueState::Fresh),
                    Some(cached),
                )
            } else if let Some(stale) = &target.stale_fallback {
                stale_count += 1;
                (
                    osv_from_cache(stale, completed_at_ms, EnrichmentValueState::Stale),
                    None,
                )
            } else {
                failed_count += 1;
                (empty_osv(EnrichmentValueState::Failed), None)
            };
            let entry = entries
                .entry(target.coordinate.cache_key.clone())
                .or_insert_with(|| empty_entry(&target.coordinate.package_ids));
            entry.osv = value;
            if let Some(update) = update {
                updates
                    .entry(target.coordinate.cache_key.clone())
                    .or_insert_with(|| empty_update(&target.coordinate.cache_key))
                    .osv = Some(update);
            }
        }
        summaries.push(EnrichmentServiceSummary {
            service: EnrichmentService::Osv,
            target_count: targets.len(),
            transmitted_count: targets.iter().filter(|target| target.transmit).count(),
            cached_count: targets.iter().filter(|target| !target.transmit).count(),
            stale_count,
            failed_count,
            omitted_count: plan.osv_omitted_count,
        });
    }

    if let Some(targets) = &plan.deps_dev {
        let mut stale_count = 0usize;
        let mut failed_count = 0usize;
        for target in targets {
            let (value, update) = if !target.transmit {
                if let Some(cached) = &target.cached {
                    (
                        deps_from_cache(cached, completed_at_ms, EnrichmentValueState::Cached),
                        None,
                    )
                } else {
                    failed_count += 1;
                    (empty_deps(EnrichmentValueState::Failed), None)
                }
            } else if let Some(Ok(network)) = deps_dev_network.get(&target.coordinate.cache_key) {
                let cached = CachedDepsDevValue {
                    fetched_at_ms: completed_at_ms,
                    licenses: network.licenses.clone(),
                    default_version: network.default_version.clone(),
                    deprecated: network.deprecated,
                    advisory_ids: network.advisory_ids.clone(),
                    version_found: network.version_found,
                    package_found: network.package_found,
                };
                (
                    deps_from_cache(&cached, completed_at_ms, EnrichmentValueState::Fresh),
                    Some(cached),
                )
            } else if let Some(stale) = &target.stale_fallback {
                stale_count += 1;
                (
                    deps_from_cache(stale, completed_at_ms, EnrichmentValueState::Stale),
                    None,
                )
            } else {
                failed_count += 1;
                (empty_deps(EnrichmentValueState::Failed), None)
            };
            let entry = entries
                .entry(target.coordinate.cache_key.clone())
                .or_insert_with(|| empty_entry(&target.coordinate.package_ids));
            entry.deps_dev = value;
            if let Some(update) = update {
                updates
                    .entry(target.coordinate.cache_key.clone())
                    .or_insert_with(|| empty_update(&target.coordinate.cache_key))
                    .deps_dev = Some(update);
            }
        }
        summaries.push(EnrichmentServiceSummary {
            service: EnrichmentService::DepsDev,
            target_count: targets.len(),
            transmitted_count: targets.iter().filter(|target| target.transmit).count(),
            cached_count: targets.iter().filter(|target| !target.transmit).count(),
            stale_count,
            failed_count,
            omitted_count: plan.deps_dev_omitted_count,
        });
    }

    ResolvedEnrichment {
        report: DependencyEnrichmentReport {
            revision: plan.revision.clone(),
            completed_at_ms,
            local_authoritative: true,
            cache_persisted: false,
            entries: entries.into_values().collect(),
            services: summaries,
        },
        updates: updates.into_values().collect(),
    }
}

fn empty_entry(package_ids: &[String]) -> DependencyEnrichmentEntry {
    DependencyEnrichmentEntry {
        package_ids: package_ids.to_vec(),
        osv: empty_osv(EnrichmentValueState::NotRequested),
        deps_dev: empty_deps(EnrichmentValueState::NotRequested),
    }
}

fn empty_update(key: &str) -> EnrichmentCacheUpdate {
    EnrichmentCacheUpdate {
        key: key.to_string(),
        osv: None,
        deps_dev: None,
    }
}

fn empty_osv(state: EnrichmentValueState) -> OsvEnrichmentValue {
    OsvEnrichmentValue {
        state,
        fetched_at_ms: None,
        age_ms: None,
        advisory_ids: Vec::new(),
        truncated: false,
    }
}

fn empty_deps(state: EnrichmentValueState) -> DepsDevEnrichmentValue {
    DepsDevEnrichmentValue {
        state,
        fetched_at_ms: None,
        age_ms: None,
        licenses: Vec::new(),
        default_version: None,
        deprecated: false,
        advisory_ids: Vec::new(),
        version_found: false,
        package_found: false,
    }
}

fn osv_from_cache(
    cached: &CachedOsvValue,
    now_ms: u64,
    state: EnrichmentValueState,
) -> OsvEnrichmentValue {
    OsvEnrichmentValue {
        state,
        fetched_at_ms: Some(cached.fetched_at_ms),
        age_ms: Some(now_ms.saturating_sub(cached.fetched_at_ms)),
        advisory_ids: cached.advisory_ids.clone(),
        truncated: cached.truncated,
    }
}

fn deps_from_cache(
    cached: &CachedDepsDevValue,
    now_ms: u64,
    state: EnrichmentValueState,
) -> DepsDevEnrichmentValue {
    DepsDevEnrichmentValue {
        state,
        fetched_at_ms: Some(cached.fetched_at_ms),
        age_ms: Some(now_ms.saturating_sub(cached.fetched_at_ms)),
        licenses: cached.licenses.clone(),
        default_version: cached.default_version.clone(),
        deprecated: cached.deprecated,
        advisory_ids: cached.advisory_ids.clone(),
        version_found: cached.version_found,
        package_found: cached.package_found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dependency_lens::{DependencyPackage, DependencyReport};

    fn report(packages: Vec<DependencyPackage>) -> DependencyReport {
        let direct_count = packages.iter().filter(|package| package.direct).count();
        DependencyReport {
            revision: "a".repeat(64),
            sources: Vec::new(),
            package_count: packages.len(),
            direct_count,
            transitive_count: packages.len().saturating_sub(direct_count),
            packages,
            duplicates: Vec::new(),
            unresolved_dependency_count: 0,
            missing_lockfile_count: 0,
            stale_lockfile_count: 0,
            unsupported_count: 0,
            invalid_count: 0,
            truncated: false,
            summary_published: false,
        }
    }

    fn package(
        id: &str,
        ecosystem: DependencyEcosystem,
        name: &str,
        version: &str,
        direct: bool,
    ) -> DependencyPackage {
        DependencyPackage {
            id: id.into(),
            ecosystem,
            name: name.into(),
            version: version.into(),
            direct,
            dependencies: Vec::new(),
        }
    }

    #[test]
    fn plan_deduplicates_node_ecosystems_and_limits_deps_dev_to_direct() {
        let report = report(vec![
            package(
                "pnpm:a@1.0.0",
                DependencyEcosystem::Pnpm,
                "a",
                "1.0.0",
                true,
            ),
            package("npm:a@1.0.0", DependencyEcosystem::Npm, "a", "1.0.0", false),
            package(
                "cargo:b@2.0.0",
                DependencyEcosystem::Cargo,
                "b",
                "2.0.0",
                false,
            ),
            package("gradle:c@3", DependencyEcosystem::Gradle, "c", "3", true),
        ]);
        let plan = build_enrichment_plan(
            &report,
            EnrichmentSelection {
                osv: true,
                deps_dev: true,
            },
            false,
            &EnrichmentCache::default(),
            1_000,
        )
        .unwrap();

        let osv = plan.osv.unwrap();
        assert_eq!(osv.len(), 2);
        assert_eq!(osv[0].coordinate.name, "a");
        assert_eq!(osv[0].coordinate.package_ids.len(), 2);
        let deps = plan.deps_dev.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].coordinate.system, "NPM");
    }

    #[test]
    fn plan_enforces_service_caps_and_discloses_every_omission_and_request() {
        let packages = (0..300)
            .map(|index| {
                package(
                    &format!("cargo:package-{index:03}@1.0.0"),
                    DependencyEcosystem::Cargo,
                    &format!("package-{index:03}"),
                    "1.0.0",
                    true,
                )
            })
            .collect();
        let plan = build_enrichment_plan(
            &report(packages),
            EnrichmentSelection {
                osv: true,
                deps_dev: true,
            },
            false,
            &EnrichmentCache::default(),
            1_000,
        )
        .unwrap();
        assert_eq!(plan.osv.as_ref().unwrap().len(), MAX_OSV_TARGETS);
        assert_eq!(plan.osv_omitted_count, 300 - MAX_OSV_TARGETS);
        assert_eq!(plan.deps_dev.as_ref().unwrap().len(), MAX_DEPS_DEV_TARGETS);
        assert_eq!(plan.deps_dev_omitted_count, 300 - MAX_DEPS_DEV_TARGETS);

        let preview = plan.preview("b".repeat(64), 2_000);
        assert_eq!(preview.services[0].transmitted.len(), MAX_OSV_TARGETS);
        assert_eq!(preview.services[0].request_count, 1);
        assert_eq!(preview.services[1].transmitted.len(), MAX_DEPS_DEV_TARGETS);
        assert_eq!(preview.services[1].request_count, MAX_DEPS_DEV_TARGETS * 2);
    }

    #[test]
    fn fresh_cache_avoids_network_but_force_refresh_keeps_stale_fallback() {
        let now = FRESH_CACHE_TTL_MS;
        let report = report(vec![package(
            "cargo:a@1.0.0",
            DependencyEcosystem::Cargo,
            "a",
            "1.0.0",
            true,
        )]);
        let initial = build_enrichment_plan(
            &report,
            EnrichmentSelection {
                osv: true,
                deps_dev: false,
            },
            false,
            &EnrichmentCache::default(),
            now,
        )
        .unwrap();
        let key = initial.osv.as_ref().unwrap()[0]
            .coordinate
            .cache_key
            .clone();
        let cache = EnrichmentCache {
            schema_version: 1,
            entries: vec![EnrichmentCacheEntry {
                key,
                osv: Some(CachedOsvValue {
                    fetched_at_ms: now - 1_000,
                    advisory_ids: vec!["GHSA-abcd".into()],
                    truncated: false,
                }),
                deps_dev: None,
            }],
        };
        let cached = build_enrichment_plan(
            &report,
            EnrichmentSelection {
                osv: true,
                deps_dev: false,
            },
            false,
            &cache,
            now,
        )
        .unwrap();
        assert!(!cached.osv.as_ref().unwrap()[0].transmit);
        let forced = build_enrichment_plan(
            &report,
            EnrichmentSelection {
                osv: true,
                deps_dev: false,
            },
            true,
            &cache,
            now,
        )
        .unwrap();
        assert!(forced.osv.as_ref().unwrap()[0].transmit);
        assert!(forced.osv.as_ref().unwrap()[0].stale_fallback.is_some());
    }

    #[test]
    fn preview_discloses_only_actual_network_coordinates() {
        let plan = build_enrichment_plan(
            &report(vec![package(
                "python:demo@1.0.0",
                DependencyEcosystem::Python,
                "demo",
                "1.0.0",
                true,
            )]),
            EnrichmentSelection {
                osv: true,
                deps_dev: true,
            },
            false,
            &EnrichmentCache::default(),
            1_000,
        )
        .unwrap();
        let preview = plan.preview("b".repeat(64), 2_000);
        assert_eq!(preview.services[0].host, OSV_HOST);
        assert_eq!(preview.services[0].transmitted[0].ecosystem, "PyPI");
        assert_eq!(preview.services[1].host, DEPS_DEV_HOST);
        assert_eq!(preview.services[1].transmitted[0].ecosystem, "PYPI");
        let json = serde_json::to_string(&preview).unwrap();
        assert!(!json.contains("repository"));
        assert!(!json.contains("lockfile"));
    }

    #[test]
    fn parses_bounded_remote_responses_without_reflecting_extra_fields() {
        let osv = parse_osv_batch(
            br#"{"results":[{"vulns":[{"id":"GHSA-bbbb"},{"id":"CVE-2026-0001"}],"next_page_token":"more","ignored":"secret"}]}"#,
            1,
        )
        .unwrap();
        assert_eq!(osv[0].advisory_ids, ["CVE-2026-0001", "GHSA-bbbb"]);
        assert!(osv[0].truncated);

        let version = parse_deps_version(
            br#"{"licenses":["MIT","Apache-2.0"],"isDeprecated":true,"advisoryKeys":[{"id":"GHSA-abcd"}],"links":[{"url":"https://ignored.invalid"}]}"#,
        )
        .unwrap();
        assert_eq!(version.licenses, ["Apache-2.0", "MIT"]);
        assert!(version.deprecated);
        let package = parse_deps_package(
            br#"{"versions":[{"versionKey":{"version":"1.0.0"},"isDefault":false},{"versionKey":{"version":"2.0.0"},"isDefault":true}]}"#,
        )
        .unwrap();
        assert_eq!(package.default_version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn remote_parsers_reject_oversized_bodies_before_json_decoding() {
        assert!(parse_osv_batch(&vec![b' '; MAX_OSV_RESPONSE_BYTES + 1], 0).is_err());
        assert!(parse_deps_version(&vec![b' '; MAX_DEPS_VERSION_RESPONSE_BYTES + 1]).is_err());
        assert!(parse_deps_package(&vec![b' '; MAX_DEPS_PACKAGE_RESPONSE_BYTES + 1]).is_err());
    }

    #[test]
    fn cache_is_strict_opaque_and_prunes_expired_values() {
        let now = STALE_CACHE_TTL_MS + 10;
        let mut cache = EnrichmentCache {
            schema_version: 1,
            entries: vec![EnrichmentCacheEntry {
                key: "a".repeat(64),
                osv: Some(CachedOsvValue {
                    fetched_at_ms: now - STALE_CACHE_TTL_MS,
                    advisory_ids: vec!["CVE-2026-0001".into()],
                    truncated: false,
                }),
                deps_dev: None,
            }],
        };
        apply_cache_updates(&mut cache, &[], now);
        let bytes = serialize_cache(&cache, now).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("package"));
        assert!(parse_cache(&bytes, now).is_ok());

        let malformed =
            br#"{"schemaVersion":1,"entries":[{"key":"bad","osv":null,"depsDev":null}]}"#;
        assert!(parse_cache(malformed, now).is_err());
    }

    #[test]
    fn failed_refresh_uses_explicit_stale_state_and_success_creates_update() {
        let now = FRESH_CACHE_TTL_MS + PREVIEW_TTL_MS + 100;
        let report = report(vec![package(
            "cargo:a@1.0.0",
            DependencyEcosystem::Cargo,
            "a",
            "1.0.0",
            true,
        )]);
        let initial = build_enrichment_plan(
            &report,
            EnrichmentSelection {
                osv: true,
                deps_dev: false,
            },
            false,
            &EnrichmentCache::default(),
            now,
        )
        .unwrap();
        let key = initial.osv.as_ref().unwrap()[0]
            .coordinate
            .cache_key
            .clone();
        let cache = EnrichmentCache {
            schema_version: 1,
            entries: vec![EnrichmentCacheEntry {
                key,
                osv: Some(CachedOsvValue {
                    fetched_at_ms: 100,
                    advisory_ids: vec!["CVE-2026-0001".into()],
                    truncated: false,
                }),
                deps_dev: None,
            }],
        };
        let plan = build_enrichment_plan(
            &report,
            EnrichmentSelection {
                osv: true,
                deps_dev: false,
            },
            false,
            &cache,
            now,
        )
        .unwrap();
        let stale = resolve_enrichment(&plan, Err(()), &BTreeMap::new(), now);
        assert_eq!(
            stale.report.entries[0].osv.state,
            EnrichmentValueState::Stale
        );
        assert!(stale.updates.is_empty());

        let fresh = resolve_enrichment(
            &plan,
            Ok(vec![ParsedOsvValue {
                advisory_ids: vec!["GHSA-fresh".into()],
                truncated: false,
            }]),
            &BTreeMap::new(),
            now,
        );
        assert_eq!(
            fresh.report.entries[0].osv.state,
            EnrichmentValueState::Fresh
        );
        assert_eq!(fresh.updates.len(), 1);
    }

    #[test]
    fn preview_tokens_are_fixed_lower_hex() {
        let first = preview_token("win:c:/repo", &"a".repeat(64), 1, 2);
        let second = preview_token("win:c:/repo", &"a".repeat(64), 2, 2);
        assert!(valid_preview_token(&first));
        assert_ne!(first, second);
        assert!(!valid_preview_token(&"A".repeat(64)));
    }

    #[test]
    fn malformed_internal_cache_plan_fails_without_panicking() {
        let coordinate = RemoteCoordinate {
            cache_key: "a".repeat(64),
            system: "CARGO",
            osv_ecosystem: "crates.io",
            name: "demo".into(),
            version: "1.0.0".into(),
            direct: true,
            package_ids: vec!["cargo:demo@1.0.0".into()],
        };
        let plan = EnrichmentPlan {
            revision: "b".repeat(64),
            local_package_count: 1,
            osv: Some(vec![PlannedOsvTarget {
                coordinate: coordinate.clone(),
                transmit: false,
                cached: None,
                stale_fallback: None,
            }]),
            deps_dev: Some(vec![PlannedDepsDevTarget {
                coordinate,
                transmit: false,
                cached: None,
                stale_fallback: None,
            }]),
            osv_omitted_count: 0,
            deps_dev_omitted_count: 0,
        };

        let resolved = resolve_enrichment(&plan, Ok(Vec::new()), &BTreeMap::new(), 100);
        assert_eq!(
            resolved.report.entries[0].osv.state,
            EnrichmentValueState::Failed
        );
        assert_eq!(
            resolved.report.entries[0].deps_dev.state,
            EnrichmentValueState::Failed
        );
        assert!(resolved
            .report
            .services
            .iter()
            .all(|service| service.failed_count == 1));
    }
}
