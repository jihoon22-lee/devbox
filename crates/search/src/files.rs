//! File-name search: each project's `git ls-files -z` (cached 30 s, fsmonitor
//! off, time and size bounded) plus the indexed vault paths, ranked by a fuzzy
//! score that favours substrings and word starts.
use crate::model::SearchHit;
use crate::snippet::{MAX_RANGES, find_all};
use grep_regex::RegexMatcher;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone)]
pub struct GitConfig {
    /// `git` (tests point it at a script).
    pub program: PathBuf,
    pub timeout: Duration,
    /// Names kept per project; more set `truncated`.
    pub max_files: usize,
    /// `ls-files` stdout bytes read at most.
    pub max_output: usize,
    pub ttl: Duration,
}
impl Default for GitConfig {
    fn default() -> Self {
        Self {
            program: "git".into(),
            timeout: Duration::from_secs(3),
            max_files: 100_000,
            max_output: 16 << 20,
            ttl: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listed {
    /// Paths as `ls-files` printed them, and whether a cap cut them.
    Files(Vec<String>, bool),
    /// Not a Git work tree (or `ls-files` failed).
    NotRepo,
    TimedOut,
}

/// Kills the command's own process group (helpers that inherited its pipes too).
struct Group(Option<rustix::process::Pid>);
impl Drop for Group {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}

/// Runs `git ls-files -z` in `root`. Repository configuration cannot run a
/// command here: `core.fsmonitor` is forced off (it is a hook path otherwise),
/// no optional index lock is taken, and the caller's `GIT_*` location variables
/// are removed. Output and time are bounded; the process group is killed on exit.
pub async fn ls_files(git: &GitConfig, root: &Path) -> Listed {
    let mut command = tokio::process::Command::new(&git.program);
    command
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotepath=false",
            "ls-files",
            "-z",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(key);
    }
    let Ok(mut child) = command.spawn() else {
        return Listed::NotRepo;
    };
    let _group = Group(
        child
            .id()
            .and_then(|id| rustix::process::Pid::from_raw(id as i32)),
    );
    let Some(stdout) = child.stdout.take() else {
        return Listed::NotRepo;
    };
    let mut out = Vec::new();
    let run = async {
        stdout
            .take(git.max_output as u64 + 1)
            .read_to_end(&mut out)
            .await?;
        if out.len() > git.max_output {
            return Ok(None);
        }
        child.wait().await.map(Some)
    };
    let status = match tokio::time::timeout(git.timeout, run).await {
        Err(_) => return Listed::TimedOut,
        Ok(Err(_)) => return Listed::NotRepo,
        Ok(Ok(status)) => status,
    };
    let mut truncated = status.is_none();
    if status.is_some_and(|s| !s.success()) {
        return Listed::NotRepo;
    }
    if truncated {
        // Drop the name cut by the byte cap.
        let keep = out[..git.max_output]
            .iter()
            .rposition(|b| *b == 0)
            .map_or(0, |i| i + 1);
        out.truncate(keep);
    }
    let mut names = Vec::new();
    for name in out.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        if names.len() == git.max_files {
            truncated = true;
            break;
        }
        if let Ok(name) = std::str::from_utf8(name) {
            names.push(name.to_owned());
        }
    }
    Listed::Files(names, truncated)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Listing {
    pub names: Arc<Vec<String>>,
    pub truncated: bool,
}

/// Names a search may show: no hidden or internal component, not excluded, not private.
fn visible(
    root: &Path,
    name: &str,
    exclude: &ignore::overrides::Override,
    private: &[PathBuf],
) -> bool {
    name.split('/').all(|c| !c.starts_with('.'))
        && !devbox_documents::is_internal(name)
        && !excluded(name, exclude)
        && !private.iter().any(|p| root.join(name).starts_with(p))
}

/// Like the code walk, which prunes an excluded folder: the name itself and
/// every parent folder are checked (`dist/`, `dist` and `**/dist/**` all exclude
/// `web/dist/a.js`).
fn excluded(name: &str, exclude: &ignore::overrides::Override) -> bool {
    exclude.matched(name, false).is_ignore()
        || name
            .match_indices('/')
            .any(|(i, _)| exclude.matched(&name[..i], true).is_ignore())
}

pub(crate) struct FileNames {
    pub git: GitConfig,
    cache: Mutex<HashMap<PathBuf, (Instant, Listing)>>,
}
impl FileNames {
    pub fn new(git: GitConfig) -> Self {
        Self {
            git,
            cache: Mutex::default(),
        }
    }
    /// Visible, non-excluded names below `root` (cached for `ttl`). Outside a Git
    /// work tree the folder is walked like a code search; a timeout lists nothing.
    pub async fn list(&self, root: &Path, exclude: &[String], private: &[PathBuf]) -> Listing {
        if let Some((at, listing)) = self.cache.lock().expect("file name cache").get(root)
            && at.elapsed() < self.git.ttl
        {
            return listing.clone();
        }
        let listing = match ls_files(&self.git, root).await {
            Listed::Files(names, truncated) => {
                let mut overrides = ignore::overrides::OverrideBuilder::new(root);
                for glob in exclude {
                    let _ = overrides.add(&format!("!{glob}"));
                }
                let overrides = overrides
                    .build()
                    .unwrap_or_else(|_| ignore::overrides::Override::empty());
                let names = names
                    .into_iter()
                    .filter(|n| visible(root, n, &overrides, private))
                    .collect();
                Listing {
                    names: Arc::new(names),
                    truncated,
                }
            }
            Listed::NotRepo => {
                let (root, exclude, private) =
                    (root.to_path_buf(), exclude.to_vec(), private.to_vec());
                let max = self.git.max_files;
                let walked = tokio::task::spawn_blocking(move || {
                    crate::code::list(&root, &exclude, &private, max)
                })
                .await;
                let (names, truncated) = walked.unwrap_or_default();
                Listing {
                    names: Arc::new(names),
                    truncated,
                }
            }
            Listed::TimedOut => Listing {
                names: Arc::default(),
                truncated: true,
            },
        };
        let mut cache = self.cache.lock().expect("file name cache");
        cache.retain(|_, (at, _)| at.elapsed() < self.git.ttl);
        cache.insert(root.to_path_buf(), (Instant::now(), listing.clone()));
        listing
    }
}

fn word_start(chars: &[char], i: usize) -> bool {
    i == 0
        || !chars[i - 1].is_alphanumeric()
        || (chars[i - 1].is_lowercase() && chars[i].is_uppercase())
}
fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive {
        c
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}
/// `[from, to)` character spans → UTF-16 ranges.
fn utf16_spans(chars: &[char], spans: &[(usize, usize)]) -> Vec<(u32, u32)> {
    let unit = |i: usize| chars[..i].iter().map(|c| c.len_utf16() as u32).sum::<u32>();
    spans
        .iter()
        .take(MAX_RANGES)
        .map(|&(a, b)| (unit(a), unit(b)))
        .collect()
}

/// Fuzzy score of `query` against `path` (higher is better) and the matched
/// characters as UTF-16 ranges; `None` when not every query character appears in order.
/// A substring beats a scattered match; the file name, word starts and short paths add.
pub fn score(query: &str, path: &str, case_sensitive: bool) -> Option<(i64, Vec<(u32, u32)>)> {
    let q: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| fold(c, case_sensitive))
        .collect();
    let chars: Vec<char> = path.chars().collect();
    let folded: Vec<char> = chars.iter().map(|c| fold(*c, case_sensitive)).collect();
    if q.is_empty() || q.len() > folded.len() {
        return None;
    }
    let name_at = chars.iter().rposition(|c| *c == '/').map_or(0, |i| i + 1);
    let penalty = chars.len() as i64;
    let best = (0..=folded.len() - q.len())
        .filter(|&i| folded[i..i + q.len()] == q[..])
        .map(|i| {
            let bonus = 100 * i64::from(word_start(&chars, i)) + 200 * i64::from(i >= name_at);
            (1000 + bonus - penalty, i)
        })
        .max_by_key(|&(s, i)| (s, std::cmp::Reverse(i)));
    if let Some((s, i)) = best {
        return Some((s, utf16_spans(&chars, &[(i, i + q.len())])));
    }
    // Scattered: each query character at the next word start, else the next occurrence.
    let (mut at, mut spans, mut s) = (0usize, Vec::<(usize, usize)>::new(), 0i64);
    for c in &q {
        let next = (at..folded.len())
            .find(|&i| folded[i] == *c && word_start(&chars, i))
            .or_else(|| (at..folded.len()).find(|&i| folded[i] == *c))?;
        s += 20 * i64::from(word_start(&chars, next)) + 10 * i64::from(next >= name_at);
        match spans.last_mut() {
            Some((_, end)) if *end == next => *end += 1,
            _ => spans.push((next, next + 1)),
        }
        at = next + 1;
    }
    Some((
        s - penalty - 5 * spans.len() as i64,
        utf16_spans(&chars, &spans),
    ))
}

/// The best `limit` of `candidates` (`(root, path)`), by regex match when
/// `matcher` is given (shorter paths first), else by [`score`].
pub(crate) fn rank(
    query: &str,
    matcher: Option<&RegexMatcher>,
    candidates: Vec<(String, String)>,
    case_sensitive: bool,
    limit: usize,
    cancel: &tokio_util::sync::CancellationToken,
) -> (Vec<SearchHit>, bool) {
    let mut scored: Vec<(i64, SearchHit)> = candidates
        .into_iter()
        .enumerate()
        .take_while(|(i, _)| i % 1024 != 0 || !cancel.is_cancelled())
        .filter_map(|(_, (root, path))| {
            let (s, ranges_utf16) = match matcher {
                Some(m) => {
                    let found = find_all(m, &path, MAX_RANGES);
                    let first = found.first()?;
                    let unit = |b: usize| path[..b].encode_utf16().count() as u32;
                    let ranges = found.iter().map(|r| (unit(r.start), unit(r.end))).collect();
                    (-(path.len() as i64) - first.start as i64, ranges)
                }
                None => score(query, &path, case_sensitive)?,
            };
            Some((
                s,
                SearchHit {
                    root,
                    snippet: path.clone(),
                    path,
                    title: None,
                    line: None,
                    ranges_utf16,
                },
            ))
        })
        .collect();
    scored.sort_by(|(a, x), (b, y)| {
        b.cmp(a)
            .then_with(|| x.path.cmp(&y.path))
            .then_with(|| x.root.cmp(&y.root))
    });
    if cancel.is_cancelled() {
        return (Vec::new(), false);
    }
    let truncated = scored.len() > limit;
    scored.truncate(limit);
    (scored.into_iter().map(|(_, h)| h).collect(), truncated)
}
