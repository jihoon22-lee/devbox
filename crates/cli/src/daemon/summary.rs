//! `notes.insert_summary`: "오늘 한 일" for one local calendar day
//! `[00:00, next 00:00)`: the user's commits in registered projects, agent tasks
//! closed that day and failed runs/schedules. Only commit subjects, task titles,
//! PR links and run/schedule IDs are read; activity totals join in S3 Task 8.
use super::state::Services;
use chrono::{DateTime, NaiveDate, TimeZone};
use devbox_agents::state::CloseHow;
use devbox_notes::{InsertSummary, NotesError, SummaryInserted};
use devbox_protocol::Fail;
use devbox_runtime::RunKind;
use futures::StreamExt;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

type R<T> = Result<T, Fail<NotesError>>;
const HOUR_MS: i64 = 3_600_000;
/// Per project, both Git commands together.
const GIT_LIMIT: Duration = Duration::from_secs(3);
const COMMITS_PER_PROJECT: &str = "50";
/// Lines per section; the rest is counted.
const SECTION_LINES: usize = 100;
const TEXT_CHARS: usize = 200;

/// One local calendar day and the same bounds for `git log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Day {
    pub date: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub since: String,
    /// Inclusive in Git, so one second before the next midnight.
    pub until: String,
}
fn midnight<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Option<DateTime<Tz>> {
    let at = date.and_hms_opt(0, 0, 0)?;
    // A DST gap at midnight starts the day at the first valid local time.
    tz.from_local_datetime(&at).earliest().or_else(|| {
        tz.from_local_datetime(&(at + chrono::Duration::hours(1)))
            .earliest()
    })
}
impl Day {
    pub(super) fn new<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> Option<Self>
    where
        Tz::Offset: std::fmt::Display,
    {
        let start = midnight(date, tz)?;
        let end = midnight(date.succ_opt()?, tz)?;
        let git = "%Y-%m-%d %H:%M:%S %z";
        Some(Self {
            date: date.format("%Y-%m-%d").to_string(),
            start_ms: start.timestamp_millis(),
            end_ms: end.timestamp_millis(),
            since: start.format(git).to_string(),
            until: (end - chrono::Duration::seconds(1)).format(git).to_string(),
        })
    }
    fn contains(&self, ms: i64) -> bool {
        (self.start_ms..self.end_ms).contains(&ms)
    }
}

#[derive(Debug, Clone)]
pub(super) struct ClosedTask {
    pub title: String,
    pub how: Option<CloseHow>,
    pub pr_url: Option<String>,
    pub closed_ms: i64,
}
#[derive(Debug, Clone)]
pub(super) struct Commit {
    pub project: String,
    pub hash: String,
    pub subject: String,
}
#[derive(Debug, Clone, Default)]
pub(super) struct Commits {
    pub items: Vec<Commit>,
    /// Projects whose Git did not answer in time.
    pub unread: Vec<String>,
}
#[derive(Debug, Clone)]
pub(super) enum Failed {
    Run {
        kind: RunKind,
        id: String,
    },
    /// A schedule's last claim failed (its run may be listed as a `Scheduled` run).
    Schedule {
        id: String,
    },
}
#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub project: String,
    pub what: Failed,
    pub at_ms: i64,
}

/// Where the summary reads from (tests inject fakes).
pub(super) trait SummarySources: Sync {
    /// Tasks closed within the last `hours` (open tasks are left out).
    fn closed_tasks(&self, hours: u32) -> impl Future<Output = R<Vec<ClosedTask>>> + Send;
    fn commits(&self, day: &Day) -> impl Future<Output = R<Commits>> + Send;
    /// Recent failed runs and schedules whose last claim failed, any day.
    fn failures(&self) -> impl Future<Output = R<Vec<Failure>>> + Send;
}

/// One line of source text: control characters become spaces and comment
/// delimiters are escaped, so nothing can end or forge a summary block.
fn clean(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(TEXT_CHARS)
        .collect();
    flat.replace("<!--", "&lt;!--")
        .replace("-->", "--&gt;")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn section(out: &mut String, title: &str, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("- {title}\n"));
    let more = lines.len().saturating_sub(SECTION_LINES);
    for line in lines.into_iter().take(SECTION_LINES) {
        out.push_str(&format!("  - {line}\n"));
    }
    if more > 0 {
        out.push_str(&format!("  - 외 {more}개\n"));
    }
}
fn how(how: Option<CloseHow>) -> &'static str {
    match how {
        Some(CloseHow::Merged) => "병합됨",
        Some(CloseHow::PullRequest) => "PR",
        Some(CloseHow::Discarded) => "폐기",
        None => "닫힘",
    }
}
fn kind(kind: RunKind) -> &'static str {
    match kind {
        RunKind::Task => "작업",
        RunKind::Service => "서비스",
        RunKind::Scheduled => "예약 실행",
        RunKind::AgentTest => "에이전트 테스트",
    }
}

/// The block text for `day`, read at `now_ms`.
pub(super) async fn compose(src: &impl SummarySources, day: &Day, now_ms: i64) -> R<String> {
    // Back to that day's midnight (plus an hour of slack for the clock read
    // inside the source), never "the last 24 hours".
    let back = ((now_ms - day.start_ms).max(0) as u64).div_ceil(HOUR_MS as u64) + 1;
    let hours = u32::try_from(back).unwrap_or(u32::MAX);
    let mut tasks: Vec<ClosedTask> = src
        .closed_tasks(hours)
        .await?
        .into_iter()
        .filter(|t| day.contains(t.closed_ms))
        .collect();
    tasks.sort_by_key(|t| t.closed_ms);
    let commits = src.commits(day).await?;
    let failures: Vec<Failure> = src
        .failures()
        .await?
        .into_iter()
        .filter(|f| day.contains(f.at_ms))
        .collect();
    // A failed scheduled run already stands for its schedule's failed claim.
    let ran: Vec<&str> = failures
        .iter()
        .filter(|f| {
            matches!(
                f.what,
                Failed::Run {
                    kind: RunKind::Scheduled,
                    ..
                }
            )
        })
        .map(|f| f.project.as_str())
        .collect();
    let mut out = String::new();
    let commit_lines = commits
        .items
        .iter()
        .map(|c| {
            format!(
                "{}: {} {}",
                clean(&c.project),
                clean(&c.hash),
                clean(&c.subject)
            )
        })
        .collect();
    section(&mut out, "커밋", commit_lines);
    let task_lines = tasks
        .iter()
        .map(|t| {
            let mut line = format!("{} · {}", clean(&t.title), how(t.how));
            if let Some(url) = t.pr_url.as_deref().filter(|u| u.starts_with("https://")) {
                line.push_str(&format!(" · {}", clean(url)));
            }
            line
        })
        .collect();
    section(&mut out, "닫힌 에이전트 작업", task_lines);
    let failure_lines = failures
        .iter()
        .filter_map(|f| match &f.what {
            Failed::Run { kind: k, id } => Some(format!(
                "{}: {} ({})",
                clean(&f.project),
                clean(id),
                kind(*k)
            )),
            Failed::Schedule { .. } if ran.contains(&f.project.as_str()) => None,
            Failed::Schedule { id } => Some(format!("{}: 예약 {}", clean(&f.project), clean(id))),
        })
        .collect();
    section(&mut out, "실패한 실행·예약", failure_lines);
    if !commits.unread.is_empty() {
        let names: Vec<String> = commits.unread.iter().map(|p| clean(p)).collect();
        out.push_str(&format!(
            "- 커밋을 읽지 못한 프로젝트: {}\n",
            names.join(", ")
        ));
    }
    if out.is_empty() {
        out.push_str("- 기록된 일이 없습니다.\n");
    }
    Ok(format!("### 오늘 한 일\n{out}"))
}

/// `git log` arguments: the user's own commits (exact `<email>`) of `day`.
pub(super) fn log_args(day: &Day, email: &str) -> Vec<String> {
    vec![
        "log".into(),
        "--fixed-strings".into(),
        "--no-show-signature".into(),
        format!("--since={}", day.since),
        format!("--until={}", day.until),
        format!("--author=<{email}>"),
        "--format=%h%x1f%s".into(),
        "-n".into(),
        COMMITS_PER_PROJECT.into(),
    ]
}
/// `None` when the project has no `user.email` (nobody else's commits are listed).
pub(super) async fn project_commits(
    dir: &Path,
    day: &Day,
) -> Result<Option<Vec<(String, String)>>, devbox_git::GitError> {
    let (code, email, _) = devbox_git::run::output(dir, &["config", "user.email"]).await?;
    let email = email.trim();
    if code != 0 || email.is_empty() {
        return Ok(None);
    }
    let args = log_args(day, email);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, out, _) = devbox_git::run::output(dir, &args).await?;
    if code != 0 {
        // Not a repository yet, or no commits.
        return Ok(Some(Vec::new()));
    }
    Ok(Some(
        out.lines()
            .filter_map(|line| line.split_once('\x1f'))
            .map(|(hash, subject)| (hash.to_owned(), subject.to_owned()))
            .collect(),
    ))
}

struct Live {
    s: Arc<Services>,
}
impl Live {
    async fn projects(&self) -> R<Vec<devbox_projects::Project>> {
        let list = self.s.projects.list(devbox_projects::ListProjects {}).await;
        Ok(list
            .map_err(|_| Fail::Internal("list projects".into()))?
            .items)
    }
}
impl SummarySources for Live {
    async fn closed_tasks(&self, hours: u32) -> R<Vec<ClosedTask>> {
        let tasks = self.s.agents.all_tasks(Some(hours)).await;
        let tasks = tasks.map_err(|_| Fail::Internal("read agent tasks".into()))?;
        Ok(tasks
            .into_iter()
            .filter_map(|t| {
                Some(ClosedTask {
                    closed_ms: t.closed_ms?,
                    title: t.title,
                    how: t.closed_how,
                    pr_url: t.pr_url,
                })
            })
            .collect())
    }
    async fn commits(&self, day: &Day) -> R<Commits> {
        let projects = self.projects().await?;
        let results: Vec<_> = futures::stream::iter(projects)
            .map(|p| async move {
                let read =
                    tokio::time::timeout(GIT_LIMIT, project_commits(Path::new(&p.path), day));
                (p.name, read.await)
            })
            .buffered(4)
            .collect()
            .await;
        let mut out = Commits::default();
        for (project, read) in results {
            match read {
                Ok(Ok(Some(list))) => {
                    out.items
                        .extend(list.into_iter().map(|(hash, subject)| Commit {
                            project: project.clone(),
                            hash,
                            subject,
                        }))
                }
                Ok(Ok(None)) => {}
                // Timed out (git is killed on drop) or could not run.
                Ok(Err(_)) | Err(_) => out.unread.push(project),
            }
        }
        Ok(out)
    }
    async fn failures(&self) -> R<Vec<Failure>> {
        let projects = self.projects().await?;
        let name = |id: &str| {
            projects
                .iter()
                .find(|p| p.id == id)
                .map_or_else(|| id.to_owned(), |p| p.name.clone())
        };
        let runs = self
            .s
            .runs
            .runs(devbox_runtime::RunsQuery {
                project_id: None,
                limit: 256,
            })
            .await
            .map_err(|_| Fail::Internal("list runs".into()))?;
        let mut out = Vec::new();
        for run in runs.items {
            let ended = run.ended_ms.or(run.exit.as_ref().map(|e| e.ended_ms));
            if run.state != devbox_runtime::RunState::Failed {
                continue;
            }
            let Some(at) = ended.and_then(|ms| i64::try_from(ms).ok()) else {
                continue;
            };
            out.push(Failure {
                project: name(&run.project_id),
                what: Failed::Run {
                    kind: run.kind,
                    id: run.def_id,
                },
                at_ms: at,
            });
        }
        for project in &projects {
            let query = devbox_runtime::SchedulesQuery {
                project_id: project.id.clone(),
            };
            // No `devbox.toml`, untrusted or invalid: nothing to report here.
            let Ok(schedules) = self.s.runs.schedules(query).await else {
                continue;
            };
            for item in schedules.items {
                let Some(last) = item.last else { continue };
                let at = DateTime::parse_from_rfc3339(&last.at).map(|t| t.timestamp_millis());
                if last.result == devbox_runtime::ScheduleOutcome::Failed
                    && let Ok(at) = at
                {
                    out.push(Failure {
                        project: project.name.clone(),
                        what: Failed::Schedule { id: item.id },
                        at_ms: at,
                    });
                }
            }
        }
        Ok(out)
    }
}

fn parse_date(date: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .filter(|d| d.format("%Y-%m-%d").to_string() == date)
}

/// `notes.insert_summary` handler.
pub(super) async fn insert(s: Arc<Services>, p: InsertSummary) -> R<SummaryInserted> {
    let now = chrono::Local::now();
    let date = match p.date.as_deref() {
        None => now.date_naive(),
        Some(date) => parse_date(date).ok_or(NotesError::InvalidInput)?,
    };
    let day = Day::new(date, &chrono::Local).ok_or(NotesError::InvalidInput)?;
    let markdown = compose(&Live { s: s.clone() }, &day, now.timestamp_millis()).await?;
    let pc = s.notes.pc_name().await;
    s.notes.insert_summary(&day.date, &pc, &markdown).await
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
