//! Destination-owned preparation and one-pointer activation for three SQLite
//! stores. The native caller keeps engines stopped and vault writers quiesced.
//! Rollback never deletes or restores Markdown/assets outside these generations.
use super::{
    import_rows::{self, Report, Source},
    stores::{self, Manifest},
};
use data_migration::core::migration::{acquire_snapshot, verify_snapshot, Snapshot};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
const COMPONENTS: [&str; 3] = ["notes", "activity", "search"];
const MAX_DATABASE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_JOURNAL_BYTES: u64 = 64 * 1024;
const MAX_PLANS: usize = 8;
pub fn list(root: &Path) -> Result<Vec<Plan>, String> {
    let imports = root.join("imports");
    match fs::symlink_metadata(&imports) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("import_storage_unavailable".into()),
        Ok(metadata) if !metadata.is_dir() => return Err("import_path_invalid".into()),
        _ => {}
    }
    devbox_filesystem::ensure_no_links(&imports).map_err(|_| "import_path_invalid")?;
    let mut plans = Vec::new();
    for entry in fs::read_dir(imports)
        .map_err(|_| "import_storage_unavailable")?
        .take(MAX_PLANS + 1)
    {
        if plans.len() == MAX_PLANS {
            return Err("import_storage_limit".into());
        }
        let entry = entry.map_err(|_| "import_storage_unavailable")?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| "import_path_invalid")?;
        plans.push(read(root, &id)?);
    }
    plans.sort_by_key(|plan| std::cmp::Reverse(plan.prepared_at_ms));
    Ok(plans)
}

/// Conservative room for two backup copies and destination growth. A snapshot
/// still enforces its own byte limit while concurrent WAL writers are running.
pub fn required_space(root: &Path, legacy_base: &Path, sources: &[Source]) -> Result<u64, String> {
    let size = |path: PathBuf| -> Result<u64, String> {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_source_unavailable")?;
        let mut bytes = fs::metadata(&path)
            .map_err(|_| "import_source_unavailable")?
            .len();
        let wal = path.with_extension("db-wal");
        if wal.exists() {
            devbox_filesystem::ensure_no_links(&wal).map_err(|_| "import_path_invalid")?;
            bytes = bytes.saturating_add(
                fs::metadata(wal)
                    .map_err(|_| "import_source_unavailable")?
                    .len(),
            );
        }
        if bytes > MAX_DATABASE_BYTES {
            return Err("import_limit_exceeded".into());
        }
        Ok(bytes)
    };
    let mut bytes = 64 * 1024 * 1024;
    if let Some(base) = stores::read(root)? {
        for component in COMPONENTS {
            bytes += 2 * size(stores::directory(root, &base, component)?.join("data.db"))?;
        }
    }
    for source in sources {
        bytes += 2 * size(source_path(legacy_base, *source)?)?;
    }
    Ok(bytes)
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Building,
    Prepared,
    Activating,
    Activated,
    Cancelling,
    Cancelled,
    RollingBack,
    RolledBack,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceReport {
    pub source: Source,
    pub snapshot: Snapshot,
    pub report: Report,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub id: String,
    pub phase: Phase,
    pub base: Option<Manifest>,
    pub next: Manifest,
    pub baseline: Vec<Snapshot>,
    pub staged: Vec<String>,
    pub authoritative: Vec<String>,
    pub sources: Vec<SourceReport>,
    pub prepared_at_ms: u64,
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn sql<T>(value: rusqlite::Result<T>) -> Result<T, String> {
    value.map_err(|_| "import_database_invalid".into())
}
fn cancelled(value: &AtomicBool) -> Result<(), String> {
    if value.load(Ordering::Acquire) {
        Err("import_cancelled".into())
    } else {
        Ok(())
    }
}
fn directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir(path).map_err(|_| "import_storage_unavailable")?;
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| "import_path_invalid")?;
    if !path.is_dir() {
        return Err("import_path_invalid".into());
    }
    Ok(())
}
fn lock(root: &Path) -> Result<File, String> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "import_path_invalid")?;
    let path = root.join("store-activation.lock");
    if path.exists() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| "import_storage_unavailable")?;
    file.try_lock().map_err(|_| "store_busy")?;
    Ok(file)
}
fn plan_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !uuid(id) {
        return Err("import_plan_invalid".into());
    }
    let path = root.join("imports").join(id);
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
    Ok(path)
}
fn validate(plan: &Plan) -> Result<(), String> {
    if plan.schema_version != 1 {
        return Err("import_schema_unsupported".into());
    }
    if !uuid(&plan.id)
        || !uuid(&plan.next.generation)
        || plan.next.schema_version != 1
        || plan
            .base
            .as_ref()
            .is_some_and(|m| m.schema_version != 1 || !uuid(&m.generation) || m == &plan.next)
        || plan.baseline.len() > 3
        || plan.staged.len() > 3
        || plan.staged.iter().any(|h| !hash(h))
        || plan.authoritative.len() > 3
        || plan.authoritative.iter().any(|h| !hash(h))
        || plan.sources.len() > 3
    {
        return Err("import_plan_invalid".into());
    }
    let mut seen = std::collections::HashSet::new();
    for source in &plan.sources {
        if !seen.insert(source.source.key())
            || source.snapshot.schema_version != 0
            || source.snapshot.bytes > MAX_DATABASE_BYTES
            || !hash(&source.snapshot.sha256)
            || source.snapshot.acquisition != "sqlite-online-backup/v1"
        {
            return Err("import_plan_invalid".into());
        }
    }
    for snapshot in &plan.baseline {
        if snapshot.schema_version != 0
            || snapshot.bytes > MAX_DATABASE_BYTES
            || !hash(&snapshot.sha256)
            || snapshot.acquisition != "sqlite-online-backup/v1"
        {
            return Err("import_plan_invalid".into());
        }
    }
    if matches!(
        plan.phase,
        Phase::Prepared
            | Phase::Activating
            | Phase::Activated
            | Phase::RollingBack
            | Phase::RolledBack
    ) && (plan.staged.len() != 3
        || plan.authoritative.len() != 3
        || plan.sources.is_empty()
        || plan.baseline.len() != if plan.base.is_some() { 3 } else { 0 }
        || plan.prepared_at_ms == 0)
    {
        return Err("import_plan_invalid".into());
    }
    Ok(())
}
fn write(root: &Path, plan: &Plan) -> Result<(), String> {
    validate(plan)?;
    let path = plan_dir(root, &plan.id)?.join("journal.json");
    if path.exists() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
    }
    let bytes = serde_json::to_vec(plan).map_err(|_| "import_plan_invalid")?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err("import_plan_invalid".into());
    }
    devbox_filesystem::atomic_write(path, &bytes).map_err(|_| "import_storage_unavailable".into())
}
pub fn read(root: &Path, id: &str) -> Result<Plan, String> {
    let path = plan_dir(root, id)?.join("journal.json");
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(&path, false)
        .map_err(|_| "import_storage_unavailable")?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "import_storage_unavailable")?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES
        || devbox_filesystem::filesystem_identity(&path, false)
            .map_err(|_| "import_path_invalid")?
            != identity
    {
        return Err("import_plan_invalid".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|_| "import_plan_invalid")?;
    if plan.id != id {
        return Err("import_plan_invalid".into());
    }
    validate(&plan)?;
    Ok(plan)
}
fn digest(path: &Path) -> Result<String, String> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| "import_path_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "import_storage_unavailable")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0;
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|_| "import_storage_unavailable")?;
        if size == 0 {
            break;
        }
        total += size as u64;
        if total > MAX_DATABASE_BYTES {
            return Err("import_limit_exceeded".into());
        }
        hash.update(&buffer[..size]);
    }
    if devbox_filesystem::filesystem_identity(path, false).map_err(|_| "import_path_invalid")?
        != identity
    {
        return Err("import_path_invalid".into());
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
fn copy_new(source: &Path, target: &Path) -> Result<(), String> {
    devbox_filesystem::ensure_no_links(target.parent().ok_or("import_path_invalid")?)
        .map_err(|_| "import_path_invalid")?;
    let mut input = File::open(source).map_err(|_| "import_storage_unavailable")?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| "import_storage_unavailable")?;
    let copied = std::io::copy(
        &mut Read::by_ref(&mut input).take(MAX_DATABASE_BYTES + 1),
        &mut output,
    )
    .map_err(|_| "import_storage_unavailable")?;
    if copied > MAX_DATABASE_BYTES {
        return Err("import_limit_exceeded".into());
    }
    output.flush().map_err(|_| "import_storage_unavailable")?;
    output
        .sync_all()
        .map_err(|_| "import_storage_unavailable".into())
}
fn source_path(base: &Path, source: Source) -> Result<PathBuf, String> {
    let path = base.join(source.identifier()).join("data.db");
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_source_unavailable")?;
    Ok(path)
}
fn prepare_inner(
    root: &Path,
    legacy_base: &Path,
    sources: &[Source],
    token: Arc<AtomicBool>,
    plan: &mut Plan,
) -> Result<(), String> {
    let stage = plan_dir(root, &plan.id)?;
    let generations = root.join("stores");
    directory(&generations)?;
    let generation = generations.join(&plan.next.generation);
    fs::create_dir(&generation).map_err(|_| "import_storage_unavailable")?;
    devbox_filesystem::atomic_write(generation.join("import-owner.json"), plan.id.as_bytes())
        .map_err(|_| "import_storage_unavailable")?;
    for component in COMPONENTS {
        fs::create_dir(generation.join(component)).map_err(|_| "import_storage_unavailable")?;
    }
    for component in COMPONENTS {
        cancelled(&token)?;
        let target = generation.join(component).join("data.db");
        if let Some(base) = &plan.base {
            let origin = stores::directory(root, base, component)?.join("data.db");
            let snapshot_stage = stage.join(format!("base-{component}"));
            let snapshot = acquire_snapshot(&origin, &snapshot_stage, &[0], &token)?;
            copy_new(&verify_snapshot(&snapshot_stage, &snapshot)?, &target)?;
            plan.baseline.push(snapshot);
        } else {
            match component {
                "notes" => knowledge_base_lib::component::create_empty_store(
                    &target,
                    &root.join("notes-vault"),
                )?,
                "activity" => life_log_lib::component::create_empty_store(&target)?,
                "search" => everything_plus_lib::component::create_empty_store(&target)?,
                _ => unreachable!(),
            }
        }
    }
    for source in sources {
        cancelled(&token)?;
        let origin = source_path(legacy_base, *source)?;
        let snapshot_stage = stage.join(format!("source-{}", source.key()));
        let snapshot = acquire_snapshot(&origin, &snapshot_stage, &[0], &token)?;
        let snapshot_path = verify_snapshot(&snapshot_stage, &snapshot)?;
        let target = generation.join(source.key()).join("data.db");
        let mut destination = sql(Connection::open_with_flags(
            &target,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        ))?;
        sql(destination.execute_batch("PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;"))?;
        let report = import_rows::merge(
            &snapshot_path,
            &mut destination,
            *source,
            plan.base.is_none(),
            token.clone(),
        )?;
        drop(destination);
        plan.sources.push(SourceReport {
            source: *source,
            snapshot,
            report,
        });
    }
    for component in COMPONENTS {
        let path = stores::directory(root, &plan.next, component)?.join("data.db");
        let db = sql(Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ))?;
        let result: String = sql(db.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
        if result != "ok" {
            return Err("import_database_invalid".into());
        }
        let source = match component {
            "notes" => Source::Notes,
            "activity" => Source::Activity,
            "search" => Source::Search,
            _ => unreachable!(),
        };
        plan.authoritative
            .push(import_rows::authoritative_fingerprint(
                &db,
                source,
                token.clone(),
            )?);
        drop(db);
        plan.staged.push(digest(&path)?);
    }
    cancelled(&token)?;
    if stores::read(root)? != plan.base {
        return Err("import_preview_stale".into());
    }
    plan.prepared_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "import_plan_invalid")?
        .as_millis()
        .try_into()
        .map_err(|_| "import_plan_invalid")?;
    plan.phase = Phase::Prepared;
    write(root, plan)
}
/// No active pointer is touched by a preview or any preparation failure.
pub fn prepare(
    root: &Path,
    legacy_base: &Path,
    sources: &[Source],
    token: Arc<AtomicBool>,
) -> Result<Plan, String> {
    let _lock = lock(root)?;
    cancelled(&token)?;
    if sources.is_empty()
        || sources.len() > 3
        || sources
            .iter()
            .map(|s| s.key())
            .collect::<std::collections::HashSet<_>>()
            .len()
            != sources.len()
    {
        return Err("import_args_invalid".into());
    }
    let imports = root.join("imports");
    directory(&imports)?;
    // Cancelled runs contain only their validated journal; reclaim these small
    // records so repeated cancellation does not exhaust the preparation slots.
    for plan in list(root)? {
        if plan.phase == Phase::Cancelled
            && !root.join("stores").join(&plan.next.generation).exists()
        {
            let directory = plan_dir(root, &plan.id)?;
            if fs::read_dir(&directory)
                .map_err(|_| "import_storage_unavailable")?
                .count()
                == 1
            {
                fs::remove_file(directory.join("journal.json"))
                    .map_err(|_| "import_storage_unavailable")?;
                fs::remove_dir(directory).map_err(|_| "import_storage_unavailable")?;
            }
        }
    }
    if fs::read_dir(&imports)
        .map_err(|_| "import_storage_unavailable")?
        .take(MAX_PLANS)
        .count()
        >= MAX_PLANS
    {
        return Err("import_storage_limit".into());
    }
    let mut plan = Plan {
        schema_version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        phase: Phase::Building,
        base: stores::read(root)?,
        next: Manifest {
            schema_version: 1,
            generation: uuid::Uuid::new_v4().to_string(),
        },
        baseline: Vec::new(),
        staged: Vec::new(),
        authoritative: Vec::new(),
        sources: Vec::new(),
        prepared_at_ms: 0,
    };
    fs::create_dir(imports.join(&plan.id)).map_err(|_| "import_storage_unavailable")?;
    write(root, &plan)?;
    // Keep a discoverable building journal on failure. No active data was changed.
    prepare_inner(root, legacy_base, sources, token, &mut plan)?;
    Ok(plan)
}
fn verify_staged(root: &Path, plan: &Plan) -> Result<(), String> {
    for (component, expected) in COMPONENTS.iter().zip(&plan.staged) {
        let path = stores::directory(root, &plan.next, component)?.join("data.db");
        // Prepared generations have no open writers and use DELETE journaling.
        if path.with_extension("db-wal").exists()
            || path.with_extension("db-journal").exists()
            || digest(&path)? != *expected
        {
            return Err("import_preview_stale".into());
        }
    }
    Ok(())
}
/// The caller acquires source/vault ownership before this check and keeps it
/// through component initialization. Changed sources require a fresh preview.
pub fn activate(
    root: &Path,
    legacy_base: &Path,
    id: &str,
    token: &AtomicBool,
) -> Result<Plan, String> {
    let _lock = lock(root)?;
    let mut plan = read(root, id)?;
    cancelled(token)?;
    if matches!(plan.phase, Phase::Activating | Phase::Activated)
        && stores::read(root)? == Some(plan.next.clone())
    {
        // Continue the selected generation, preserving any newer edits. It is
        // rollback, not forward recovery, that requires the original hashes.
        return Ok(plan);
    }
    if plan.phase != Phase::Prepared || stores::read(root)? != plan.base {
        return Err("import_preview_stale".into());
    }
    verify_staged(root, &plan)?;
    let checks = plan_dir(root, id)?.join(format!("check-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&checks).map_err(|_| "import_storage_unavailable")?;
    struct Checks(PathBuf);
    impl Drop for Checks {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _checks = Checks(checks.clone());
    for report in &plan.sources {
        let current = acquire_snapshot(
            &source_path(legacy_base, report.source)?,
            &checks.join(report.source.key()),
            &[0],
            token,
        )?;
        if current.sha256 != report.snapshot.sha256 {
            return Err("import_source_changed".into());
        }
    }
    if let Some(base) = &plan.base {
        for (component, expected) in COMPONENTS.iter().zip(&plan.baseline) {
            let current = acquire_snapshot(
                &stores::directory(root, base, component)?.join("data.db"),
                &checks.join(format!("base-{component}")),
                &[0],
                token,
            )?;
            if current.sha256 != expected.sha256 {
                return Err("import_preview_stale".into());
            }
        }
    }
    cancelled(token)?;
    plan.phase = Phase::Activating;
    write(root, &plan)?;
    if stores::read(root)? != plan.base {
        return Err("import_preview_stale".into());
    }
    devbox_filesystem::atomic_write(
        root.join("active-stores.json"),
        &serde_json::to_vec(&plan.next).map_err(|_| "import_plan_invalid")?,
    )
    .map_err(|_| "import_storage_unavailable")?;
    Ok(plan)
}
/// Only after all three engines initialize successfully. A failed health check
/// leaves Activating visible for an explicit restart/recovery choice.
pub fn commit(root: &Path, id: &str) -> Result<Plan, String> {
    let _lock = lock(root)?;
    let mut plan = read(root, id)?;
    if !matches!(plan.phase, Phase::Activating | Phase::Activated)
        || stores::read(root)? != Some(plan.next.clone())
    {
        return Err("import_preview_stale".into());
    }
    plan.phase = Phase::Activated;
    write(root, &plan)?;
    Ok(plan)
}
/// Run only before opening engines. Changed user-owned rows block rollback;
/// derived-index changes do not. Both generations remain preserved.
pub fn rollback(root: &Path, id: &str) -> Result<Plan, String> {
    let _lock = lock(root)?;
    let mut plan = read(root, id)?;
    if plan.phase == Phase::RolledBack && stores::read(root)? == plan.base {
        return Ok(plan);
    }
    if plan.phase == Phase::RollingBack && stores::read(root)? == plan.base {
        plan.phase = Phase::RolledBack;
        write(root, &plan)?;
        return Ok(plan);
    }
    if !matches!(
        plan.phase,
        Phase::Activating | Phase::Activated | Phase::RollingBack
    ) || stores::read(root)? != Some(plan.next.clone())
    {
        return Err("import_preview_stale".into());
    }
    for (source, expected) in [Source::Notes, Source::Activity, Source::Search]
        .iter()
        .zip(&plan.authoritative)
    {
        let db = sql(Connection::open_with_flags(
            stores::directory(root, &plan.next, source.key())?.join("data.db"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ))?;
        if import_rows::authoritative_fingerprint(&db, *source, Arc::new(AtomicBool::new(false)))?
            != *expected
        {
            return Err("import_preview_stale".into());
        }
    }
    plan.phase = Phase::RollingBack;
    write(root, &plan)?;
    if let Some(base) = &plan.base {
        for component in COMPONENTS {
            stores::directory(root, base, component)?;
        }
        devbox_filesystem::atomic_write(
            root.join("active-stores.json"),
            &serde_json::to_vec(base).map_err(|_| "import_plan_invalid")?,
        )
        .map_err(|_| "import_storage_unavailable")?;
    } else {
        fs::remove_file(root.join("active-stores.json"))
            .map_err(|_| "import_storage_unavailable")?;
    }
    plan.phase = Phase::RolledBack;
    write(root, &plan)?;
    Ok(plan)
}

fn inspect_owned_tree(path: &Path, count: &mut usize) -> Result<(), String> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| "import_path_invalid")?;
    *count += 1;
    if *count > 96 {
        return Err("import_path_invalid".into());
    }
    for entry in fs::read_dir(path).map_err(|_| "import_storage_unavailable")? {
        let entry = entry.map_err(|_| "import_storage_unavailable")?;
        let path = entry.path();
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| "import_storage_unavailable")?;
        if metadata.is_dir() {
            inspect_owned_tree(&path, count)?;
        } else if !metadata.is_file() {
            return Err("import_path_invalid".into());
        }
    }
    Ok(())
}
/// Remove only an unselected generation whose native creation receipt matches.
/// The small cancelled journal remains available as a diagnostic.
pub fn cancel(root: &Path, id: &str) -> Result<Plan, String> {
    let _lock = lock(root)?;
    let mut plan = read(root, id)?;
    if plan.phase == Phase::Cancelled {
        return Ok(plan);
    }
    if !matches!(
        plan.phase,
        Phase::Building | Phase::Prepared | Phase::Cancelling
    ) || stores::read(root)? == Some(plan.next.clone())
    {
        return Err("import_preview_stale".into());
    }
    let generation = root.join("stores").join(&plan.next.generation);
    if generation.exists() {
        inspect_owned_tree(&generation, &mut 0)?;
        let marker = generation.join("import-owner.json");
        devbox_filesystem::ensure_no_links(&marker).map_err(|_| "import_path_invalid")?;
        let mut owner = String::new();
        File::open(marker)
            .map_err(|_| "import_storage_unavailable")?
            .take(37)
            .read_to_string(&mut owner)
            .map_err(|_| "import_path_invalid")?;
        if owner != id {
            return Err("import_path_invalid".into());
        }
    }
    let stage = plan_dir(root, id)?;
    inspect_owned_tree(&stage, &mut 0)?;
    plan.phase = Phase::Cancelling;
    write(root, &plan)?;
    if generation.exists() {
        fs::remove_dir_all(&generation).map_err(|_| "import_storage_unavailable")?;
    }
    for entry in fs::read_dir(&stage).map_err(|_| "import_storage_unavailable")? {
        let entry = entry.map_err(|_| "import_storage_unavailable")?;
        if entry.file_name() == "journal.json" {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|_| "import_storage_unavailable")?
            .is_dir()
        {
            return Err("import_path_invalid".into());
        }
        fs::remove_dir_all(entry.path()).map_err(|_| "import_storage_unavailable")?;
    }
    plan.phase = Phase::Cancelled;
    write(root, &plan)?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy(base: &Path, source: Source) -> PathBuf {
        let directory = base.join(source.identifier());
        fs::create_dir(&directory).unwrap();
        let path = directory.join("data.db");
        match source {
            Source::Notes => knowledge_base_lib::component::create_empty_store(
                &path,
                &base.join("original-vault"),
            )
            .unwrap(),
            Source::Activity => life_log_lib::component::create_empty_store(&path).unwrap(),
            Source::Search => everything_plus_lib::component::create_empty_store(&path).unwrap(),
        }
        path
    }
    fn token() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }
    #[test]
    fn wal_snapshot_preparation_cancel_resume_and_atomic_pointer_preserve_originals() {
        let root = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        let path = legacy(base.path(), Source::Notes);
        legacy(base.path(), Source::Activity);
        legacy(base.path(), Source::Search);
        let source = Connection::open(&path).unwrap();
        source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO note_templates VALUES(1,'WAL','committed WAL template',1,2); BEGIN IMMEDIATE; INSERT INTO note_templates VALUES(2,'uncommitted','not imported',1,2);").unwrap();
        let old = stores::create_empty(root.path()).unwrap();
        let plan = prepare(
            root.path(),
            base.path(),
            &[Source::Notes, Source::Activity, Source::Search],
            token(),
        )
        .unwrap();
        assert_eq!(stores::read(root.path()).unwrap(), Some(old.clone()));
        let imported = Connection::open(
            stores::directory(root.path(), &plan.next, "notes")
                .unwrap()
                .join("data.db"),
        )
        .unwrap();
        assert_eq!(
            imported
                .query_row("SELECT content FROM note_templates", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "committed WAL template"
        );
        assert_eq!(
            imported
                .query_row("SELECT count(*) FROM note_templates", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(imported);
        assert_eq!(read(root.path(), &plan.id).unwrap().phase, Phase::Prepared);
        cancel(root.path(), &plan.id).unwrap();
        assert_eq!(stores::read(root.path()).unwrap(), Some(old));
        assert!(!root
            .path()
            .join("stores")
            .join(plan.next.generation)
            .exists());
        assert_eq!(
            source
                .query_row("SELECT count(*) FROM note_templates", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        source.execute_batch("ROLLBACK;").unwrap();
    }
    #[test]
    fn activation_recovers_after_pointer_write_and_rollback_refuses_newer_edits() {
        let root = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        legacy(base.path(), Source::Notes);
        let old = stores::create_empty(root.path()).unwrap();
        let plan = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        let activated =
            activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false)).unwrap();
        assert_eq!(activated.phase, Phase::Activating);
        assert_eq!(stores::read(root.path()).unwrap(), Some(plan.next.clone()));
        assert_eq!(
            activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false))
                .unwrap()
                .phase,
            Phase::Activating
        );
        commit(root.path(), &plan.id).unwrap();
        rollback(root.path(), &plan.id).unwrap();
        assert_eq!(stores::read(root.path()).unwrap(), Some(old.clone()));
        let next = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        activate(root.path(), base.path(), &next.id, &AtomicBool::new(false)).unwrap();
        commit(root.path(), &next.id).unwrap();
        let target = stores::directory(root.path(), &next.next, "notes")
            .unwrap()
            .join("data.db");
        Connection::open(&target)
            .unwrap()
            .execute("INSERT INTO settings VALUES('newer','keep')", [])
            .unwrap();
        assert!(rollback(root.path(), &next.id).is_err());
        assert_eq!(stores::read(root.path()).unwrap(), Some(next.next));
        assert!(stores::directory(root.path(), &old, "notes")
            .unwrap()
            .join("data.db")
            .exists());
        assert_eq!(
            Connection::open(target)
                .unwrap()
                .query_row("SELECT value FROM settings WHERE key='newer'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "keep"
        );
    }
    #[test]
    fn changed_source_or_destination_and_future_journal_fail_without_switching() {
        let root = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        let source = legacy(base.path(), Source::Notes);
        let old = stores::create_empty(root.path()).unwrap();
        let plan = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        Connection::open(source)
            .unwrap()
            .execute("INSERT INTO settings VALUES('later','source change')", [])
            .unwrap();
        assert!(activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false)).is_err());
        assert_eq!(stores::read(root.path()).unwrap(), Some(old.clone()));
        cancel(root.path(), &plan.id).unwrap();
        let plan = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        Connection::open(
            stores::directory(root.path(), &old, "notes")
                .unwrap()
                .join("data.db"),
        )
        .unwrap()
        .execute("INSERT INTO settings VALUES('later','product change')", [])
        .unwrap();
        assert!(activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false)).is_err());
        assert_eq!(stores::read(root.path()).unwrap(), Some(old.clone()));
        let journal = plan_dir(root.path(), &plan.id)
            .unwrap()
            .join("journal.json");
        let mut value = serde_json::to_value(&plan).unwrap();
        value["schemaVersion"] = serde_json::json!(9);
        fs::write(&journal, serde_json::to_vec(&value).unwrap()).unwrap();
        let bytes = fs::read(&journal).unwrap();
        assert!(read(root.path(), &plan.id).is_err());
        assert_eq!(fs::read(journal).unwrap(), bytes);
        assert_eq!(stores::read(root.path()).unwrap(), Some(old));
    }
    #[test]
    fn first_import_rollback_never_removes_authoritative_vault_bytes() {
        let root = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        legacy(base.path(), Source::Notes);
        let vault = base.path().join("original-vault");
        fs::create_dir(&vault).unwrap();
        fs::write(vault.join("note.md"), "original").unwrap();
        let plan = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        assert!(stores::read(root.path()).unwrap().is_none());
        activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false)).unwrap();
        fs::write(vault.join("new-note.md"), "new product note").unwrap();
        rollback(root.path(), &plan.id).unwrap();
        assert!(stores::read(root.path()).unwrap().is_none());
        assert_eq!(
            fs::read_to_string(vault.join("note.md")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_to_string(vault.join("new-note.md")).unwrap(),
            "new product note"
        );
    }
    #[test]
    fn derived_rebuilds_allow_rollback_but_forward_recovery_preserves_new_user_rows() {
        let root = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        legacy(base.path(), Source::Notes);
        let old = stores::create_empty(root.path()).unwrap();
        let plan = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false)).unwrap();
        let path = stores::directory(root.path(), &plan.next, "notes")
            .unwrap()
            .join("data.db");
        Connection::open(path).unwrap().execute_batch("INSERT INTO docs(path,title,body,modified_ts) VALUES('existing.md','Rebuilt index','derived content',1); INSERT INTO settings VALUES('wikilink-schema','1');").unwrap();
        rollback(root.path(), &plan.id).unwrap();
        assert_eq!(stores::read(root.path()).unwrap(), Some(old));
        let plan = prepare(root.path(), base.path(), &[Source::Notes], token()).unwrap();
        activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false)).unwrap();
        let path = stores::directory(root.path(), &plan.next, "notes")
            .unwrap()
            .join("data.db");
        Connection::open(&path)
            .unwrap()
            .execute(
                "INSERT INTO note_templates VALUES(1,'new edit','keep it',1,2)",
                [],
            )
            .unwrap();
        assert_eq!(
            activate(root.path(), base.path(), &plan.id, &AtomicBool::new(false))
                .unwrap()
                .phase,
            Phase::Activating
        );
        assert!(rollback(root.path(), &plan.id).is_err());
        assert_eq!(
            Connection::open(path)
                .unwrap()
                .query_row("SELECT content FROM note_templates", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "keep it"
        );
    }
}
