//! Read-only Git history shared by calendar and export/digest aggregation.
//! Deduplication uses Git's canonical common directory plus commit object ID.
//! No message, author, remote, environment or raw stderr enters the result.
use devbox_filesystem::{parse_safe_project_path, MAX_PROJECT_PATH_BYTES};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const PROJECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_PROJECTS: usize = 64;

#[derive(Debug)]
pub struct ProjectHistory {
    pub path: String,
    pub timestamps: Vec<i64>,
    pub error_code: Option<String>,
}

fn cancelled(signal: &AtomicBool) -> Result<(), String> {
    if signal.load(Ordering::Acquire) {
        Err("digest_cancelled".into())
    } else {
        Ok(())
    }
}
fn seconds_ceiling(milliseconds: i64) -> i64 {
    milliseconds.div_euclid(1000) + i64::from(milliseconds.rem_euclid(1000) != 0)
}
fn parse_commits(output: &str, start: i64, end: i64) -> Result<Vec<(String, i64)>, String> {
    let mut commits = BTreeMap::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let (hash, seconds) = line.split_once('\t').ok_or("git_output_invalid")?;
        if !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("git_output_invalid".into());
        }
        let timestamp = seconds
            .parse::<i64>()
            .ok()
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or("git_output_invalid")?;
        let hash = hash.to_ascii_lowercase();
        if commits
            .insert(hash, timestamp)
            .is_some_and(|previous| previous != timestamp)
        {
            return Err("git_output_invalid".into());
        }
    }
    Ok(commits
        .into_iter()
        .filter(|(_, timestamp)| *timestamp >= start && *timestamp < end)
        .collect())
}
fn common_identity(target: &devbox_git::GitTarget, output: &str) -> Result<String, String> {
    let value = output
        .strip_suffix("\r\n")
        .or_else(|| output.strip_suffix('\n'))
        .ok_or("git_output_invalid")?;
    if value.is_empty()
        || value.len() > MAX_PROJECT_PATH_BYTES
        || value.chars().any(char::is_control)
    {
        return Err("git_output_invalid".into());
    }
    let path = target
        .host_path_from_git(value)
        .map_err(|_| "git_output_invalid")?;
    parse_safe_project_path(&path)
        .map(|path| path.identity().to_owned())
        .ok_or_else(|| "git_output_invalid".into())
}

pub fn collect(
    projects: &[String],
    start: i64,
    end: i64,
    signal: &AtomicBool,
) -> Result<Vec<ProjectHistory>, String> {
    collect_with(
        projects,
        start,
        end,
        signal,
        |args, target, timeout, max, signal| {
            devbox_git::run_bounded_target_with_cancel(args, target, timeout, max, signal)
        },
    )
}
fn collect_with<F>(
    projects: &[String],
    start: i64,
    end: i64,
    signal: &AtomicBool,
    mut run: F,
) -> Result<Vec<ProjectHistory>, String>
where
    F: FnMut(
        &[&str],
        &devbox_git::GitTarget,
        Duration,
        usize,
        &AtomicBool,
    ) -> Result<String, String>,
{
    if projects.len() > MAX_PROJECTS || end <= start {
        return Err("git_invalid_arguments".into());
    }
    let mut paths = projects
        .iter()
        .map(|path| parse_safe_project_path(path).ok_or("git_invalid_target"))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort_by(|left, right| {
        left.identity()
            .as_bytes()
            .cmp(right.identity().as_bytes())
            .then_with(|| left.as_str().as_bytes().cmp(right.as_str().as_bytes()))
    });
    paths.dedup_by(|left, right| left.identity() == right.identity());
    let since = format!("--since=@{}", start.div_euclid(1000));
    let before = format!("--before=@{}", seconds_ceiling(end));
    let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut results = Vec::with_capacity(paths.len());
    for path in paths {
        cancelled(signal)?;
        let deadline = Instant::now() + PROJECT_TIMEOUT;
        let result: Result<Vec<i64>, String> = (|| {
            let target = devbox_git::GitTarget::from_project_path(path.as_str())?;
            let common = run(
                &[
                    "--no-pager",
                    "--no-optional-locks",
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-common-dir",
                ],
                &target,
                deadline.saturating_duration_since(Instant::now()),
                MAX_PROJECT_PATH_BYTES + 2,
                signal,
            )?;
            let identity = common_identity(&target, &common)?;
            cancelled(signal)?;
            if Instant::now() >= deadline {
                return Err("git_timeout".into());
            }
            let output = run(
                &[
                    "--no-pager",
                    "--no-optional-locks",
                    "log",
                    "--no-show-signature",
                    "--no-decorate",
                    "--no-notes",
                    "--no-patch",
                    "--color=never",
                    &since,
                    &before,
                    "--format=%H%x09%ct",
                    "--",
                ],
                &target,
                deadline.saturating_duration_since(Instant::now()),
                MAX_OUTPUT_BYTES,
                signal,
            )?;
            cancelled(signal)?;
            let commits = parse_commits(&output, start, end)?;
            // Commit only a fully parsed project into the shared-repository set.
            // A corrupt later line must not hide valid counts from another worktree.
            let repository = seen.entry(identity).or_default();
            Ok(commits
                .into_iter()
                .filter_map(|(hash, timestamp)| repository.insert(hash).then_some(timestamp))
                .collect())
        })();
        let (timestamps, error_code) = match result {
            Ok(timestamps) => (timestamps, None),
            Err(error) if error == "git_cancelled" || error == "digest_cancelled" => {
                return Err("digest_cancelled".into())
            }
            Err(error) => (
                Vec::new(),
                Some(if crate::core::error_codes::is_git(&error) {
                    error
                } else {
                    "git_failed".into()
                }),
            ),
        };
        results.push(ProjectHistory {
            path: path.into_string(),
            timestamps,
            error_code,
        });
    }
    cancelled(signal)?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(hash: char, seconds: i64) -> String {
        format!("{}\t{seconds}\n", hash.to_string().repeat(40))
    }
    #[test]
    fn commits_are_validated_atomically_and_use_exact_half_open_milliseconds() {
        let output = format!("{}{}{}", record('a', 0), record('b', 1), record('c', 2));
        assert_eq!(
            parse_commits(&output, 1, 2000).unwrap(),
            vec![("b".repeat(40), 1000)]
        );
        for bad in [
            format!("{}bad\n", record('a', 1)),
            format!("{}{}", record('a', 1), record('a', 2)),
            format!("{}\t{}\n", "a".repeat(64), i64::MAX),
        ] {
            assert!(parse_commits(&bad, 0, 2000).is_err());
        }
        assert!(parse_commits(&format!("{}\t1\n", "a".repeat(64)), 0, 2000).is_ok());
        assert_eq!(seconds_ceiling(-1), 0);
    }
    #[test]
    fn common_worktrees_deduplicate_but_an_independent_repository_keeps_same_hash() {
        let root = tempfile::tempdir().unwrap();
        let paths = ["a-main", "b-worktree", "c-independent"]
            .map(|name| root.path().join(name).to_string_lossy().into_owned());
        let common = root
            .path()
            .join("common.git")
            .to_string_lossy()
            .into_owned();
        let separate = root
            .path()
            .join("separate.git")
            .to_string_lossy()
            .into_owned();
        let mut calls = 0;
        let rows = collect_with(
            &paths,
            0,
            2000,
            &AtomicBool::new(false),
            |args, _, timeout, _, _| {
                assert!(timeout <= PROJECT_TIMEOUT);
                calls += 1;
                if args.contains(&"rev-parse") {
                    Ok(format!(
                        "{}\n",
                        if calls <= 4 { &common } else { &separate }
                    ))
                } else {
                    assert!(args.contains(&"--format=%H%x09%ct"));
                    Ok(record('a', 1))
                }
            },
        )
        .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.timestamps.len())
                .collect::<Vec<_>>(),
            vec![1, 0, 1]
        );
        assert_eq!(calls, 6);
    }
    #[test]
    fn malformed_or_unavailable_project_never_poison_other_project_counts() {
        let root = tempfile::tempdir().unwrap();
        let paths =
            ["a-bad", "b-good"].map(|name| root.path().join(name).to_string_lossy().into_owned());
        let common = root
            .path()
            .join("common.git")
            .to_string_lossy()
            .into_owned();
        let mut calls = 0;
        let rows = collect_with(
            &paths,
            0,
            2000,
            &AtomicBool::new(false),
            |args, _, _, _, _| {
                calls += 1;
                if args.contains(&"rev-parse") {
                    Ok(format!("{common}\n"))
                } else if calls == 2 {
                    Ok(format!("{}invalid\n", record('a', 1)))
                } else {
                    Ok(record('a', 1))
                }
            },
        )
        .unwrap();
        assert_eq!(rows[0].error_code.as_deref(), Some("git_output_invalid"));
        assert!(rows[0].timestamps.is_empty());
        assert_eq!(rows[1].timestamps, vec![1000]);
        let result = collect_with(&paths, 0, 2000, &AtomicBool::new(true), |_, _, _, _, _| {
            panic!("cancelled query must not spawn")
        });
        assert_eq!(result.unwrap_err(), "digest_cancelled");
    }
    fn fixture_git(
        root: &std::path::Path,
        cwd: &std::path::Path,
        args: &[&str],
        date: &str,
    ) -> String {
        let mut command = std::process::Command::new(devbox_git::resolve_git());
        command
            .current_dir(cwd)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
                "-c",
            ])
            .arg(format!("core.hooksPath={}", root.join("hooks").display()))
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.join("empty-config"))
            .env("GIT_CONFIG_COUNT", "0")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env("GIT_AUTHOR_NAME", "Synthetic Fixture")
            .env("GIT_COMMITTER_NAME", "Synthetic Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
        for name in [
            "GIT_DIR",
            "GIT_COMMON_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ] {
            command.env_remove(name);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "synthetic Git fixture command failed"
        );
        String::from_utf8(output.stdout).unwrap()
    }
    #[test]
    fn real_linked_worktrees_share_history_while_independent_repository_does_not() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("empty-config"), "").unwrap();
        std::fs::create_dir(root.path().join("hooks")).unwrap();
        let main = root.path().join("a-main");
        let worktree = root.path().join("b-worktree");
        let independent = root.path().join("c-independent");
        for path in [&main, &independent] {
            std::fs::create_dir(path).unwrap();
            fixture_git(
                root.path(),
                path,
                &["init", "-q", "--initial-branch=main"],
                "2000-01-01T00:00:01Z",
            );
            fixture_git(
                root.path(),
                path,
                &["commit", "--allow-empty", "-qm", "base"],
                "2000-01-01T00:00:01Z",
            );
        }
        let base = fixture_git(
            root.path(),
            &main,
            &["rev-parse", "HEAD"],
            "2000-01-01T00:00:01Z",
        );
        assert_eq!(
            base,
            fixture_git(
                root.path(),
                &independent,
                &["rev-parse", "HEAD"],
                "2000-01-01T00:00:01Z"
            )
        );
        fixture_git(
            root.path(),
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                worktree.to_str().unwrap(),
            ],
            "2000-01-01T00:00:01Z",
        );
        fixture_git(
            root.path(),
            &main,
            &["commit", "--allow-empty", "-qm", "main"],
            "2000-01-01T00:00:02Z",
        );
        fixture_git(
            root.path(),
            &worktree,
            &["commit", "--allow-empty", "-qm", "feature"],
            "2000-01-01T00:00:03Z",
        );
        let paths =
            [&worktree, &main, &independent, &main].map(|path| path.to_string_lossy().into_owned());
        let rows = collect(
            &paths,
            946_684_800_000,
            946_684_804_000,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(rows.iter().all(|row| row.error_code.is_none()), "{rows:?}");
        assert_eq!(
            rows.iter()
                .map(|row| row.timestamps.len())
                .collect::<Vec<_>>(),
            vec![2, 1, 1]
        );
        #[cfg(unix)]
        {
            let alias = root.path().join("d-alias");
            std::os::unix::fs::symlink(&main, &alias).unwrap();
            let mut with_alias = paths.to_vec();
            with_alias.push(alias.to_string_lossy().into_owned());
            let rows = collect(
                &with_alias,
                946_684_800_000,
                946_684_804_000,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(
                rows.iter()
                    .map(|row| row.timestamps.len())
                    .collect::<Vec<_>>(),
                vec![2, 1, 1, 0]
            );
        }
    }
}
