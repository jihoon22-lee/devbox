//! GitHub CLI uses the selected native repository, with no renderer remote override.
use super::*;
use crate::core::pull_requests::{classify_gh_failure, parse_list, parse_view, valid_url};
pub use crate::core::pull_requests::{PrListItem, PullRequest};
use process_tree::ProcessTree;
use std::process::Stdio;
const FAILED: &str = "pr_failed";
const LIMIT: usize = 1024 * 1024;
const VIEW_FIELDS: &str =
    "number,title,state,url,isDraft,headRefName,baseRefName,reviewDecision,statusCheckRollup";
const LIST_FIELDS: &str = "number,title,headRefName,author,updatedAt,isDraft,url";
#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrStatus {
    pub available: bool,
    pub reason: Option<String>,
    pub pr: Option<PullRequest>,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrListRequest {
    pub path: String,
    pub limit: u32,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrCreateRequest {
    pub path: String,
    pub title: String,
    pub body: String,
    pub base: Option<String>,
    pub draft: bool,
    pub operation_id: String,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrCreated {
    pub url: String,
}
#[derive(Debug)]
pub struct GhOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum GhSpawnError {
    Missing,
    Timeout,
    Io,
}
pub trait GhRunner {
    fn run(&self, cwd: &Path, args: &[&str]) -> Result<GhOutput, GhSpawnError>;
}
pub struct SystemGh {
    cancel: Arc<AtomicBool>,
}
impl GhRunner for SystemGh {
    fn run(&self, cwd: &Path, args: &[&str]) -> Result<GhOutput, GhSpawnError> {
        self.run_program(cwd, "gh", args, Duration::from_secs(30))
    }
}
impl SystemGh {
    fn run_program(
        &self,
        cwd: &Path,
        program: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<GhOutput, GhSpawnError> {
        let policy = devbox_git::execution::current();
        if let Some(policy) = &policy {
            let target = git_target_for_path(cwd, FAILED).map_err(|_| GhSpawnError::Io)?;
            policy.admit(&target).map_err(|_| GhSpawnError::Io)?;
        }
        let timeout = if let Some(policy) = &policy {
            policy
                .remaining(timeout)
                .map_err(|_| GhSpawnError::Timeout)?
        } else {
            timeout
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| GhSpawnError::Io)?;
        runtime.block_on(async {
            let mut command = tokio::process::Command::new(program);
            command.args(args).current_dir(cwd).env("GH_PROMPT_DISABLED", "1").env("GH_NO_UPDATE_NOTIFIER", "1")
                .env("NO_COLOR", "1").env("GH_PAGER", "cat").env("LC_ALL", "C").env("LANGUAGE", "C")
                .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
            // cwd is the only repository selector; inherited Git/GH overrides
            // must not redirect an operation away from the admitted repository.
            for (key, _) in std::env::vars_os() {
                let folded = key.to_string_lossy().to_ascii_uppercase();
                if folded.starts_with("GIT_") || folded == "GH_REPO" { command.env_remove(key); }
            }
            ProcessTree::prepare_tokio(&mut command);
            if self.cancel.load(Ordering::Acquire) || policy.as_ref().is_some_and(|p| p.boundary().is_err()) { return Err(GhSpawnError::Timeout); }
            let mut child = command.spawn().map_err(|error| if error.kind() == ErrorKind::NotFound { GhSpawnError::Missing } else { GhSpawnError::Io })?;
            let mut tree = match ProcessTree::assign(&child) {
                Ok(tree) => tree,
                Err(()) => { ProcessTree::terminate_unassigned(&mut child).await; return Err(GhSpawnError::Io); }
            };
            let stdout = child.stdout.take().ok_or(GhSpawnError::Io)?;
            let stderr = child.stderr.take().ok_or(GhSpawnError::Io)?;
            let result = {
                let capture = async {
                    let (stdout, stderr, status) = tokio::try_join!(
                        read_pipe(stdout), read_pipe(stderr),
                        async { child.wait().await.map_err(|_| GhSpawnError::Io) }
                    )?;
                    Ok(GhOutput { status: status.code().unwrap_or(-1), stdout, stderr })
                };
                tokio::select! {
                    result = tokio::time::timeout(timeout, capture) => result.map_err(|_| GhSpawnError::Timeout).and_then(|result| result),
                    _ = async {
                        loop {
                            if self.cancel.load(Ordering::Acquire) || policy.as_ref().is_some_and(|p| p.boundary().is_err()) { break; }
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    } => Err(GhSpawnError::Timeout),
                }
            };
            if !tree.terminate(&mut child).await { return Err(GhSpawnError::Io); }
            result
        })
    }
}
async fn read_pipe(reader: impl tokio::io::AsyncRead + Unpin) -> Result<String, GhSpawnError> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    reader
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| GhSpawnError::Io)?;
    if bytes.len() > LIMIT {
        return Err(GhSpawnError::Io);
    }
    String::from_utf8(bytes).map_err(|_| GhSpawnError::Io)
}
fn unavailable(reason: &str) -> PrStatus {
    PrStatus {
        available: false,
        reason: Some(reason.into()),
        pr: None,
    }
}
fn pr_status_with(gh: &impl GhRunner, cwd: &Path) -> Result<PrStatus, String> {
    let auth = match gh.run(cwd, &["auth", "status"]) {
        Ok(auth) => auth,
        Err(GhSpawnError::Missing) => return Ok(unavailable("gh_missing")),
        Err(_) => return Err(FAILED.into()),
    };
    if auth.status != 0 {
        return Ok(unavailable("gh_unauthenticated"));
    }
    let result = gh
        .run(cwd, &["pr", "view", "--json", VIEW_FIELDS])
        .map_err(|_| FAILED)?;
    let pr = if result.status == 0 {
        Some(parse_view(&result.stdout)?)
    } else if result.stderr.contains("no pull requests found") {
        None
    } else {
        return Err(FAILED.into());
    };
    Ok(PrStatus {
        available: true,
        reason: None,
        pr,
    })
}
fn pr_create_with(
    gh: &impl GhRunner,
    cwd: &Path,
    title: &str,
    body: &str,
    base: Option<&str>,
    draft: bool,
) -> Result<PrCreated, String> {
    if title.trim().is_empty()
        || title.chars().count() > 256
        || title.chars().any(char::is_control)
        || body.len() > 64 * 1024
        || body.contains('\0')
        || base.is_some_and(|base| !valid_worktree_branch(base))
    {
        return Err("pr_input_invalid".into());
    }
    let mut args = vec!["pr", "create", "--title", title, "--body", body];
    if let Some(base) = base {
        args.extend(["--base", base]);
    }
    if draft {
        args.push("--draft");
    }
    let result = gh.run(cwd, &args).map_err(|_| FAILED)?;
    if result.status != 0 {
        return Err(classify_gh_failure(&result.stderr).into());
    }
    let url = result.stdout.lines().last().unwrap_or("").trim();
    if !valid_url(url) {
        return Err(FAILED.into());
    }
    Ok(PrCreated { url: url.into() })
}
pub async fn repo_pr_status(request: PathRequest) -> Result<PrStatus, String> {
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let result = pr_status_with(
            &SystemGh {
                cancel: Arc::new(AtomicBool::new(false)),
            },
            &context.worktree,
        )?;
        revalidate_repository_context(&context, FAILED)?;
        Ok(result)
    })
    .await
}
pub async fn repo_pr_list(request: PrListRequest) -> Result<Vec<PrListItem>, String> {
    if !(1..=30).contains(&request.limit) {
        return Err("pr_input_invalid".into());
    }
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let result = SystemGh {
            cancel: Arc::new(AtomicBool::new(false)),
        }
        .run(
            &context.worktree,
            &[
                "pr",
                "list",
                "--json",
                LIST_FIELDS,
                "--limit",
                &request.limit.to_string(),
            ],
        )
        .map_err(|_| FAILED)?;
        if result.status != 0 {
            return Err(classify_gh_failure(&result.stderr).into());
        }
        revalidate_repository_context(&context, FAILED)?;
        parse_list(&result.stdout)
    })
    .await
}
pub async fn repo_pr_create(request: PrCreateRequest) -> Result<PrCreated, String> {
    let operation = begin_git_operation(&request.operation_id, FAILED, FAILED)?;
    spawn_git_task(FAILED, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, FAILED)?;
        operation.bind_repository(context.common_git_identity, FAILED, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let result = pr_create_with(
            &SystemGh {
                cancel: operation.cancellation.clone(),
            },
            &context.worktree,
            &request.title,
            &request.body,
            request.base.as_deref(),
            request.draft,
        )?;
        revalidate_repository_context(&context, FAILED)?;
        Ok(result)
    })
    .await
}

// commands/pull_requests.rs
#[cfg(test)]
mod tests {
    use super::*;

    struct FakeGh {
        rows: Vec<(Vec<String>, i32, String, String)>,
        missing: bool,
        args: std::cell::RefCell<Vec<String>>,
    }
    impl FakeGh {
        fn missing() -> Self {
            Self {
                missing: true,
                ..Self::with(vec![])
            }
        }
        fn with(rows: Vec<(Vec<&str>, i32, &str, &str)>) -> Self {
            Self {
                rows: rows
                    .into_iter()
                    .map(|(args, code, out, err)| {
                        (
                            args.into_iter().map(str::to_owned).collect(),
                            code,
                            out.into(),
                            err.into(),
                        )
                    })
                    .collect(),
                missing: false,
                args: Default::default(),
            }
        }
        fn last_args(&self) -> Vec<String> {
            self.args.borrow().clone()
        }
    }
    impl GhRunner for FakeGh {
        fn run(&self, _cwd: &Path, args: &[&str]) -> Result<GhOutput, GhSpawnError> {
            if self.missing {
                return Err(GhSpawnError::Missing);
            }
            *self.args.borrow_mut() = args.iter().map(|s| (*s).into()).collect();
            let (_, status, stdout, stderr) = self
                .rows
                .iter()
                .find(|(prefix, ..)| {
                    args.starts_with(&prefix.iter().map(String::as_str).collect::<Vec<_>>())
                })
                .expect("unexpected gh command");
            Ok(GhOutput {
                status: *status,
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            })
        }
    }
    #[test]
    fn missing_or_logged_out_gh_is_reported_as_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let missing = FakeGh::missing();
        let status = pr_status_with(&missing, dir.path()).unwrap();
        assert!(!status.available && status.pr.is_none());
        assert_eq!(status.reason.as_deref(), Some("gh_missing"));
        let logged_out = FakeGh::with(vec![(
            vec!["auth", "status"],
            1,
            "",
            "You are not logged into any GitHub hosts.",
        )]);
        assert_eq!(
            pr_status_with(&logged_out, dir.path())
                .unwrap()
                .reason
                .as_deref(),
            Some("gh_unauthenticated")
        );
    }

    #[test]
    fn a_branch_without_a_pr_is_available_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let gh = FakeGh::with(vec![
            (vec!["auth", "status"], 0, "", ""),
            (
                vec!["pr", "view"],
                1,
                "",
                "no pull requests found for branch \"agent/fix\"",
            ),
        ]);
        let status = pr_status_with(&gh, dir.path()).unwrap();
        assert!(status.available && status.reason.is_none() && status.pr.is_none());
    }

    #[test]
    fn create_passes_only_title_body_base_and_draft() {
        let dir = tempfile::tempdir().unwrap();
        let gh = FakeGh::with(vec![(
            vec!["pr", "create"],
            0,
            "https://github.com/me/devbox/pull/43\n",
            "",
        )]);
        let created =
            pr_create_with(&gh, dir.path(), "Fix login", "Body", Some("main"), true).unwrap();
        assert_eq!(created.url, "https://github.com/me/devbox/pull/43");
        assert_eq!(
            gh.last_args(),
            vec![
                "pr",
                "create",
                "--title",
                "Fix login",
                "--body",
                "Body",
                "--base",
                "main",
                "--draft"
            ]
        );
        assert_eq!(
            pr_create_with(&gh, dir.path(), "", "Body", None, false).unwrap_err(),
            "pr_input_invalid"
        );
    }
    #[test]
    fn system_runner_collects_separate_streams_and_missing_programs() {
        let root = tempfile::tempdir().unwrap();
        let gh = SystemGh {
            cancel: Arc::new(AtomicBool::new(false)),
        };
        #[cfg(unix)]
        let (program, args) = (
            "/bin/sh",
            vec!["-c", "printf output; printf diagnostic >&2; exit 7"],
        );
        #[cfg(windows)]
        let (program, args) = (
            "cmd.exe",
            vec!["/d", "/c", "echo output& echo diagnostic 1>&2& exit /b 7"],
        );
        let output = gh
            .run_program(root.path(), program, &args, Duration::from_secs(3))
            .unwrap();
        assert_eq!(output.status, 7);
        assert_eq!(output.stdout.trim(), "output");
        assert_eq!(output.stderr.trim(), "diagnostic");
        assert!(matches!(
            gh.run_program(
                root.path(),
                root.path().join("missing-tool").to_str().unwrap(),
                &[],
                Duration::from_secs(1)
            ),
            Err(GhSpawnError::Missing)
        ));
    }
    #[test]
    #[cfg(unix)]
    fn runner_limits_output_deadlines_and_cancelled_work() {
        let root = tempfile::tempdir().unwrap();
        let gh = SystemGh {
            cancel: Arc::new(AtomicBool::new(false)),
        };
        assert!(matches!(
            gh.run_program(
                root.path(),
                "/bin/sh",
                &["-c", "head -c 1048577 /dev/zero"],
                Duration::from_secs(3)
            ),
            Err(GhSpawnError::Io)
        ));
        let began = Instant::now();
        assert!(gh
            .run_program(
                root.path(),
                "/bin/sh",
                &["-c", "sleep 5 & wait"],
                Duration::from_millis(100)
            )
            .is_err());
        assert!(began.elapsed() < Duration::from_secs(2));
        gh.cancel.store(true, Ordering::Release);
        assert!(matches!(
            gh.run_program(
                root.path(),
                "/bin/sh",
                &["-c", "touch forbidden"],
                Duration::from_secs(1)
            ),
            Err(GhSpawnError::Timeout)
        ));
        assert!(!root.path().join("forbidden").exists());
    }
    #[test]
    fn invalid_input_never_executes_and_remote_errors_are_fixed_codes() {
        let root = tempfile::tempdir().unwrap();
        let gh = FakeGh::with(vec![]);
        for (title, body, base) in [
            ("bad\nline", "", None),
            ("title", "nul\0", None),
            ("title", "", Some("--remote")),
        ] {
            assert_eq!(
                pr_create_with(&gh, root.path(), title, body, base, false).unwrap_err(),
                "pr_input_invalid"
            );
        }
        let gh = FakeGh::with(vec![(
            vec!["pr", "create"],
            1,
            "",
            "must first push the current branch private detail",
        )]);
        assert_eq!(
            pr_create_with(&gh, root.path(), "title", "", None, false).unwrap_err(),
            "pr_branch_not_pushed"
        );
        let gh = FakeGh::with(vec![(
            vec!["pr", "create"],
            0,
            "http://example.test/pr/1",
            "",
        )]);
        assert_eq!(
            pr_create_with(&gh, root.path(), "title", "", None, false).unwrap_err(),
            "pr_failed"
        );
    }
}
