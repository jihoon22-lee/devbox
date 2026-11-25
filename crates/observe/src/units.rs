//! The user's own systemd user units, read-only: two bounded `systemctl --user` queries per
//! listing and no verb that starts, stops or changes a unit.
use crate::*;
use devbox_core::{Instance, project_root::owning_root};
use devbox_protocol::channel::AppChannel;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

pub(crate) const MAX_OUTPUT: usize = 1024 * 1024;
/// Names handed to the one `show` call; more loaded units set `truncated`.
pub(crate) const MAX_UNITS: usize = 256;
pub(crate) const MAX_DESCRIPTION_CHARS: usize = 200;
pub(crate) const MAX_STATE_CHARS: usize = 32;
/// systemd's own limits for a unit name and a path.
const MAX_NAME_BYTES: usize = 255;
const MAX_DIR_BYTES: usize = 4096;

// systemd 249 (Ubuntu 22.04) already prints `list-units` as JSON. Only names are read from it,
// so every state comes from one source.
const LIST: [&str; 6] = [
    "--user",
    "list-units",
    "--type=service,timer",
    "--all",
    "--no-pager",
    "--output=json",
];
const SHOW: [&str; 5] = [
    "--user",
    "show",
    "--no-pager",
    "--property=Id,Description,LoadState,ActiveState,SubState,FragmentPath,WorkingDirectory,NextElapseUSecRealtime,Triggers",
    "--",
];

fn unit_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":-_.\\@".contains(&b))
}
/// A valid service or timer name. Whatever else the manager prints is neither shown nor
/// passed back to it as an argument.
pub(crate) fn unit_kind(name: &str) -> Option<UserUnitKind> {
    let (stem, kind) = match name.strip_suffix(".service") {
        Some(stem) => (stem, UserUnitKind::Service),
        None => (name.strip_suffix(".timer")?, UserUnitKind::Timer),
    };
    (!stem.is_empty() && unit_name(name)).then_some(kind)
}
/// Devbox's own units, by the names their creators give them and nothing looser, so that
/// `mydevbox-tool.service` or `devbox-notes.service` stays somebody's own unit:
/// - an instance of a channel's daemon or tmux template, `<base>@<instance>.<type>` with
///   `<base>` from [`AppChannel::config`] (`devbox`, `devbox-tmux`, `devbox-accept`, …);
/// - a run `devbox-<instance>-run-<id>.service` and an agent scope
///   `devbox-<instance>-agent-a<id>.scope`, `<id>` being 32 lowercase hex digits;
/// - a verification scope `devbox-verify-<id>` of `scripts/verify-resources.py`.
pub(crate) fn devbox_unit(name: &str) -> bool {
    let Some((stem, _)) = name.rsplit_once('.') else {
        return false;
    };
    if let Some((base, _)) = stem.split_once('@') {
        return [AppChannel::Prod, AppChannel::Accept, AppChannel::Dev]
            .iter()
            .map(|channel| channel.config())
            .any(|config| base == config.daemon_unit || base == config.tmux_unit);
    }
    let Some(rest) = stem.strip_prefix("devbox-") else {
        return false;
    };
    let id =
        |id: &str| id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let of_instance = |mark: &str, owned: &dyn Fn(&str) -> bool| {
        rest.rsplit_once(mark)
            .is_some_and(|(instance, tail)| Instance::parse(instance).is_ok() && owned(tail))
    };
    rest.strip_prefix("verify-").is_some_and(id)
        || of_instance("-run-", &id)
        || of_instance("-agent-", &|tail| tail.strip_prefix('a').is_some_and(id))
}
/// Names to show, in the manager's order, and whether the cap cut the list.
pub(crate) fn parse_list(bytes: &[u8]) -> Option<(Vec<String>, bool)> {
    #[derive(serde::Deserialize)]
    struct Row {
        #[serde(default)]
        unit: Option<String>,
    }
    let rows: Vec<Row> = serde_json::from_slice(bytes).ok()?;
    let mut names: Vec<_> = rows
        .into_iter()
        .filter_map(|row| row.unit)
        .filter(|name| unit_kind(name).is_some() && !devbox_unit(name))
        .collect();
    let truncated = names.len() > MAX_UNITS;
    names.truncate(MAX_UNITS);
    Some((names, truncated))
}
/// Unit names and descriptions are untrusted text: no control character, bounded length.
fn text(value: &str, max: usize) -> String {
    let clean: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    clean.trim().chars().take(max).collect()
}
/// `Thu 2026-01-01 03:00:00 UTC`, which is how `show` prints a timestamp for a client in UTC.
/// `--timestamp=` does not reach `show` properties on systemd 249 and has no epoch style
/// before 251, so the zone comes from the child's `TZ`. Anything else is not known.
fn utc_ms(value: &str) -> Option<u64> {
    let mut parts = value.split(' ');
    let (_day, date, time, zone) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if zone != "UTC" || parts.next().is_some() {
        return None;
    }
    let at =
        chrono::NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M:%S%.f")
            .ok()?;
    u64::try_from(at.and_utc().timestamp_millis()).ok()
}
/// Shown are the units the user wrote: the unit file is directly in the user's own unit
/// directory (`~/.config/systemd/user`; a linked file keeps the link's path there). Packaged
/// (`/usr/lib`), generated and transient (`/run`) units and names without a file are plumbing.
fn unit(
    block: &HashMap<&str, &str>,
    unit_dir: &Path,
    projects: &[ProjectRoot],
) -> Option<UserUnitInfo> {
    let field = |key| block.get(key).copied().unwrap_or_default();
    let name = field("Id");
    let kind = unit_kind(name)?;
    if devbox_unit(name) || Path::new(field("FragmentPath")).parent() != Some(unit_dir) {
        return None;
    }
    let timer = kind == UserUnitKind::Timer;
    // `!` marks `WorkingDirectory=-…`; `~` and a relative value name no project folder.
    let directory = field("WorkingDirectory");
    let directory = directory.strip_prefix('!').unwrap_or(directory);
    Some(UserUnitInfo {
        name: name.to_owned(),
        description: text(field("Description"), MAX_DESCRIPTION_CHARS),
        load_state: text(field("LoadState"), MAX_STATE_CHARS),
        active_state: text(field("ActiveState"), MAX_STATE_CHARS),
        sub_state: text(field("SubState"), MAX_STATE_CHARS),
        kind,
        next_elapse_ms: timer
            .then(|| utc_ms(field("NextElapseUSecRealtime")))
            .flatten(),
        triggers: field("Triggers")
            .split(' ')
            .next()
            .filter(|unit| timer && unit_name(unit))
            .map(str::to_owned),
        project_id: (!timer && directory.len() <= MAX_DIR_BYTES)
            .then(|| owning_root(Path::new(directory), projects))
            .flatten()
            .map(str::to_owned),
    })
}
/// `show` prints one `Key=Value` block per unit, separated by an empty line, in the manager's
/// property order. Blocks are keyed by `Id`, never by position. A description is one line in a
/// unit file; a line break in one could at worst add or shadow a row of this read-only list.
pub(crate) fn parse_show(
    text: &str,
    unit_dir: &Path,
    projects: &[ProjectRoot],
) -> Vec<UserUnitInfo> {
    let mut units = BTreeMap::new();
    let mut block = HashMap::new();
    for line in text.lines().chain([""]) {
        if let Some((key, value)) = line.split_once('=') {
            block.entry(key).or_insert(value);
        } else if line.is_empty() {
            if let Some(unit) = unit(&block, unit_dir, projects) {
                units.entry(unit.name.clone()).or_insert(unit);
            }
            block.clear();
        }
    }
    units.into_values().collect()
}
pub(crate) fn unavailable(reason: UnitsUnavailableReason) -> UnitsResult {
    UnitsResult {
        available: false,
        reason: Some(reason),
        items: vec![],
        truncated: false,
    }
}
enum Cli {
    Missing,
    Finished { success: bool, stdout: Vec<u8> },
}
/// One bounded query of the login session's user manager (the bus is reached the way
/// `devbox_core::systemd` reaches it). The child is killed and reaped on every path.
async fn run(program: &Path, args: &[&str], timeout: Duration) -> R<Cli> {
    let mut child = match tokio::process::Command::new(program)
        .args(args)
        .env("XDG_RUNTIME_DIR", devbox_core::systemd::login_runtime_dir())
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Cli::Missing),
        Err(error) => {
            tracing::warn!(%error, "systemctl spawn failed");
            return Err(ObserveError::SourceUnavailable.into());
        }
    };
    let stdout = child.stdout.take();
    let result = tokio::time::timeout(timeout, async {
        let mut bytes = vec![];
        stdout
            .ok_or(ObserveError::SourceUnavailable)?
            .take(MAX_OUTPUT as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| ObserveError::SourceUnavailable)?;
        if bytes.len() > MAX_OUTPUT {
            return Err(ObserveError::LimitExceeded.into());
        }
        let status = child
            .wait()
            .await
            .map_err(|_| ObserveError::SourceUnavailable)?;
        Ok(Cli::Finished {
            success: status.success(),
            stdout: bytes,
        })
    })
    .await
    .unwrap_or_else(|_| Err(ObserveError::SourceUnavailable.into()));
    if result.is_err() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    result
}
pub(crate) async fn snapshot(
    program: &Path,
    timeout: Duration,
    unit_dir: &Path,
    projects: &[ProjectRoot],
) -> R<UnitsResult> {
    let listed = match run(program, &LIST, timeout).await? {
        Cli::Missing => return Ok(unavailable(UnitsUnavailableReason::CliMissing)),
        Cli::Finished { success: false, .. } => {
            return Ok(unavailable(UnitsUnavailableReason::ManagerUnavailable));
        }
        Cli::Finished { stdout, .. } => stdout,
    };
    let (names, truncated) = parse_list(&listed).ok_or(ObserveError::SourceUnavailable)?;
    let mut result = UnitsResult {
        available: true,
        reason: None,
        items: vec![],
        truncated,
    };
    if names.is_empty() {
        return Ok(result);
    }
    let args: Vec<_> = SHOW
        .into_iter()
        .chain(names.iter().map(String::as_str))
        .collect();
    let Cli::Finished {
        success: true,
        stdout,
    } = run(program, &args, timeout).await?
    else {
        return Err(ObserveError::SourceUnavailable.into());
    };
    result.items = parse_show(&String::from_utf8_lossy(&stdout), unit_dir, projects);
    Ok(result)
}
