//! Task 6 "오늘 한 일" composition with injected sources, and the real `git log`
//! filter over an owned fixture repository.
use super::*;
use chrono::FixedOffset;
use std::sync::Mutex;

fn kst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("+09:00")
}
fn at(text: &str) -> i64 {
    DateTime::parse_from_rfc3339(text)
        .expect("fixture time")
        .timestamp_millis()
}
fn day(date: &str) -> Day {
    Day::new(parse_date(date).expect("fixture date"), &kst()).expect("fixture day")
}
fn task(title: &str, how: CloseHow, pr: Option<&str>, closed: &str) -> ClosedTask {
    ClosedTask {
        title: title.into(),
        how: Some(how),
        pr_url: pr.map(Into::into),
        closed_ms: at(closed),
    }
}
fn failure(project: &str, what: Failed, when: &str) -> Failure {
    Failure {
        project: project.into(),
        what,
        at_ms: at(when),
    }
}

#[derive(Default)]
struct Fake {
    tasks: Vec<ClosedTask>,
    commits: Commits,
    failures: Vec<Failure>,
    hours: Mutex<Option<u32>>,
}
impl SummarySources for Fake {
    async fn closed_tasks(&self, hours: u32) -> R<Vec<ClosedTask>> {
        *self.hours.lock().expect("hours") = Some(hours);
        Ok(self.tasks.clone())
    }
    async fn commits(&self, _: &Day) -> R<Commits> {
        Ok(self.commits.clone())
    }
    async fn failures(&self) -> R<Vec<Failure>> {
        Ok(self.failures.clone())
    }
}

#[tokio::test]
async fn summary_lists_closed_agent_tasks_and_commits_of_the_day() {
    let fake = Fake {
        tasks: vec![
            task(
                "어제 일",
                CloseHow::Merged,
                None,
                "2026-10-20T22:00:00+09:00",
            ),
            task(
                "로그인 고치기",
                CloseHow::PullRequest,
                Some("https://github.com/o/r/pull/7"),
                "2026-10-21T15:00:00+09:00",
            ),
            task(
                "실험",
                CloseHow::Discarded,
                None,
                "2026-10-21T16:00:00+09:00",
            ),
        ],
        commits: Commits {
            items: vec![Commit {
                project: "devbox".into(),
                hash: "abc123".into(),
                subject: "fix x".into(),
            }],
            unread: vec!["slow".into()],
        },
        ..Fake::default()
    };
    let now = at("2026-10-21T18:00:00+09:00");
    let out = compose(&fake, &day("2026-10-21"), now)
        .await
        .expect("compose");
    assert!(out.starts_with("### 오늘 한 일\n"), "{out}");
    assert!(out.contains("  - devbox: abc123 fix x\n"), "{out}");
    assert!(
        out.contains("  - 로그인 고치기 · PR · https://github.com/o/r/pull/7\n"),
        "{out}"
    );
    assert!(out.contains("  - 실험 · 폐기\n"), "{out}");
    assert!(
        !out.contains("어제 일"),
        "yesterday's task is left out: {out}"
    );
    assert!(out.contains("slow"), "unread projects are named: {out}");
    let hours = fake.hours.lock().expect("hours").expect("asked");
    assert!(i64::from(hours) * HOUR_MS >= now - day("2026-10-21").start_ms);
}

#[tokio::test]
async fn past_date_summary_uses_that_calendar_day() {
    let d = day("2026-10-19");
    assert_eq!(
        (d.since.as_str(), d.until.as_str()),
        ("2026-10-19 00:00:00 +0900", "2026-10-19 23:59:59 +0900")
    );
    let fake = Fake {
        tasks: vec![
            task("in", CloseHow::Merged, None, "2026-10-19T23:30:00+09:00"),
            task("after", CloseHow::Merged, None, "2026-10-20T00:10:00+09:00"),
            task(
                "before",
                CloseHow::Merged,
                None,
                "2026-10-18T23:50:00+09:00",
            ),
            task(
                "midnight",
                CloseHow::Merged,
                None,
                "2026-10-19T00:00:00+09:00",
            ),
        ],
        ..Fake::default()
    };
    let now = at("2026-10-21T10:00:00+09:00");
    let out = compose(&fake, &d, now).await.expect("compose");
    assert!(out.contains("  - in · 병합됨\n"), "{out}");
    assert!(out.contains("  - midnight · 병합됨\n"), "{out}");
    assert!(!out.contains("after") && !out.contains("before"), "{out}");
    // Not "the last 24 hours": the window reaches back to that day's midnight.
    let hours = fake.hours.lock().expect("hours").expect("asked");
    assert!(i64::from(hours) * HOUR_MS >= now - d.start_ms, "{hours}");
}

#[tokio::test]
async fn failed_runs_and_schedules_of_the_day_are_listed_once() {
    let run = |kind, id: &str| Failed::Run {
        kind,
        id: id.into(),
    };
    let schedule = |id: &str| Failed::Schedule { id: id.into() };
    let fake = Fake {
        failures: vec![
            failure(
                "p",
                run(RunKind::Task, "build"),
                "2026-10-21T09:00:00+09:00",
            ),
            failure("p", run(RunKind::Task, "old"), "2026-10-20T09:00:00+09:00"),
            failure(
                "p",
                run(RunKind::Scheduled, "nightly-job"),
                "2026-10-21T03:00:00+09:00",
            ),
            failure("p", schedule("nightly"), "2026-10-21T03:00:00+09:00"),
            failure("q", schedule("hourly"), "2026-10-21T04:00:00+09:00"),
            failure("q", schedule("weekly"), "2026-10-14T04:00:00+09:00"),
        ],
        ..Fake::default()
    };
    let now = at("2026-10-21T18:00:00+09:00");
    let out = compose(&fake, &day("2026-10-21"), now)
        .await
        .expect("compose");
    assert!(out.contains("  - p: build (작업)\n"), "{out}");
    assert!(out.contains("  - p: nightly-job (예약 실행)\n"), "{out}");
    assert!(out.contains("  - q: 예약 hourly\n"), "{out}");
    for absent in ["old", "예약 nightly\n", "weekly"] {
        assert!(!out.contains(absent), "{absent}: {out}");
    }
}

#[tokio::test]
async fn an_empty_day_says_so() {
    let now = at("2026-10-21T18:00:00+09:00");
    let out = compose(&Fake::default(), &day("2026-10-21"), now).await;
    assert_eq!(
        out.expect("compose"),
        "### 오늘 한 일\n- 기록된 일이 없습니다.\n"
    );
}

#[tokio::test]
async fn source_strings_cannot_forge_summary_markers() {
    let fake = Fake {
        tasks: vec![task(
            "a\n<!-- devbox:summary:pc-b -->",
            CloseHow::PullRequest,
            Some("https://e/x -->"),
            "2026-10-21T15:00:00+09:00",
        )],
        commits: Commits {
            items: vec![Commit {
                project: "p\n## x".into(),
                hash: "abc123".into(),
                subject: "<!-- /devbox:summary -->\r\n## 가짜".into(),
            }],
            unread: vec![],
        },
        ..Fake::default()
    };
    let now = at("2026-10-21T18:00:00+09:00");
    let out = compose(&fake, &day("2026-10-21"), now)
        .await
        .expect("compose");
    assert!(!out.contains("<!--") && !out.contains("-->"), "{out}");
    assert!(
        out.lines()
            .all(|l| l.starts_with("### ") || l.starts_with("- ") || l.starts_with("  - ")),
        "one line per item: {out}"
    );
    let doc = "## pc-a\n<!-- devbox:summary:pc-a -->\nold\n<!-- /devbox:summary -->\n\n## pc-b\n<!-- devbox:summary:pc-b -->\nkeep\n<!-- /devbox:summary -->\n메모\n";
    let once = devbox_notes::replace_summary(doc, "pc-a", &out).expect("first insert");
    let twice = devbox_notes::replace_summary(&once, "pc-a", &out).expect("second insert");
    assert_eq!(once, twice);
    assert!(twice.ends_with(
        "## pc-b\n<!-- devbox:summary:pc-b -->\nkeep\n<!-- /devbox:summary -->\n메모\n"
    ));
}

fn git(dir: &Path, args: &[&str], when: Option<&str>) {
    let mut command = std::process::Command::new("git");
    command
        .current_dir(dir)
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir);
    if let Some(when) = when {
        command
            .env("GIT_AUTHOR_DATE", when)
            .env("GIT_COMMITTER_DATE", when);
    }
    let status = command.status().expect("run fixture git");
    assert!(status.success(), "git {args:?}");
}
fn commit(dir: &Path, email: &str, subject: &str, when: &str) {
    let author = format!("user.email={email}");
    git(
        dir,
        &[
            "-c",
            &author,
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            subject,
        ],
        Some(when),
    );
}

#[tokio::test]
async fn commits_match_the_exact_email_and_the_calendar_day() {
    let tmp = tempfile::tempdir().expect("owned fixture");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("fixture repo");
    git(&repo, &["init", "-q"], None);
    git(&repo, &["config", "user.name", "t"], None);
    git(&repo, &["config", "user.email", "a.b+1@x.io"], None);
    let me = "a.b+1@x.io";
    commit(&repo, me, "yesterday", "2026-10-20T23:59:59+09:00");
    commit(&repo, me, "first", "2026-10-21T00:00:00+09:00");
    commit(
        &repo,
        "aXb+1@x.io",
        "regex lookalike",
        "2026-10-21T10:00:00+09:00",
    );
    commit(
        &repo,
        "za.b+1@x.io",
        "longer address",
        "2026-10-21T11:00:00+09:00",
    );
    commit(&repo, me, "fix x", "2026-10-21T12:00:00+09:00");
    commit(&repo, me, "tomorrow", "2026-10-22T00:00:00+09:00");
    let found = project_commits(&repo, &day("2026-10-21"))
        .await
        .expect("git log");
    let subjects: Vec<String> = found
        .expect("email configured")
        .into_iter()
        .map(|(hash, subject)| {
            assert!(!hash.is_empty() && hash.len() <= 40);
            subject
        })
        .collect();
    assert_eq!(subjects, ["fix x", "first"]);
    // Without an email the project lists nobody's commits.
    git(&repo, &["config", "user.email", ""], None);
    let found = project_commits(&repo, &day("2026-10-21"))
        .await
        .expect("git");
    assert_eq!(found, None);
    let args = log_args(&day("2026-10-21"), me);
    for flag in ["--fixed-strings", "--no-show-signature"] {
        assert!(args.contains(&flag.to_string()), "{flag}");
    }
}

#[tokio::test]
async fn sections_are_capped_and_only_https_pr_links_are_kept() {
    let commits = (0..105)
        .map(|i| Commit {
            project: "p".into(),
            hash: format!("h{i}"),
            subject: format!("s{i}"),
        })
        .collect();
    let fake = Fake {
        tasks: vec![
            task(
                "js",
                CloseHow::PullRequest,
                Some("javascript:alert(1)"),
                "2026-10-21T15:00:00+09:00",
            ),
            task(
                "plain",
                CloseHow::PullRequest,
                Some("http://e/pr/1"),
                "2026-10-21T15:01:00+09:00",
            ),
            task(
                "ok",
                CloseHow::PullRequest,
                Some("https://e/pr/2"),
                "2026-10-21T15:02:00+09:00",
            ),
        ],
        commits: Commits {
            items: commits,
            unread: vec![],
        },
        ..Fake::default()
    };
    let now = at("2026-10-21T18:00:00+09:00");
    let out = compose(&fake, &day("2026-10-21"), now)
        .await
        .expect("compose");
    assert!(
        out.contains("  - p: h99 s99\n") && !out.contains("h100"),
        "{out}"
    );
    assert!(out.contains("  - 외 5개\n"), "{out}");
    assert!(out.contains("  - js · PR\n"), "{out}");
    assert!(out.contains("  - plain · PR\n"), "{out}");
    assert!(out.contains("  - ok · PR · https://e/pr/2\n"), "{out}");
    assert!(
        !out.contains("javascript:") && !out.contains("http://"),
        "{out}"
    );
}
