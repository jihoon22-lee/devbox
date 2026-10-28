use super::*;
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

/// An owned executable.
fn tool(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("owned fixture folder");
    let path = dir.join(name);
    std::fs::write(&path, body).expect("owned fixture tool");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("owned fixture mode");
    path
}
/// A CLI that leaves `<path>.invoked` behind if anything ever runs it.
fn cli(dir: &Path) -> PathBuf {
    tool(dir, "fixture-cli", "#!/bin/sh\n: > \"$0.invoked\"\n")
}
fn invoked(cli: &Path) -> bool {
    cli.with_extension("invoked").exists()
}
fn marked(path: &Path) -> Vec<u8> {
    format!("\x1e{}\x1e", path.display()).into_bytes()
}
/// A service `PATH` of owned folders.
fn path_with(dirs: &[PathBuf]) -> OsString {
    std::env::join_paths(dirs).expect("owned fixture PATH")
}
fn found(program: &Path, source: CommandSource) -> Option<ResolvedCommand> {
    Some(ResolvedCommand {
        program: program.to_path_buf(),
        source,
        version: None,
    })
}

/// Gives every lookup the same answer and counts the lookups.
struct Shell {
    answer: ShellAnswer,
    asked: AtomicUsize,
}
impl Shell {
    /// The shell answered: this path, or nothing found.
    fn answers(path: Option<&Path>) -> Self {
        Self {
            answer: ShellAnswer::Answered(path.map(Path::to_path_buf)),
            asked: AtomicUsize::new(0),
        }
    }
    /// The shell failed without an answer.
    fn silent() -> Self {
        Self {
            answer: ShellAnswer::NoAnswer,
            asked: AtomicUsize::new(0),
        }
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}
impl ShellProbe for Shell {
    fn command_v<'a>(&'a self, _: &'a str) -> BoxFuture<'a, ShellAnswer> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let answer = self.answer.clone();
        Box::pin(async move { answer })
    }
}
/// Answers only after the probe's time is up.
struct Late(PathBuf);
impl ShellProbe for Late {
    fn command_v<'a>(&'a self, _: &'a str) -> BoxFuture<'a, ShellAnswer> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            ShellAnswer::Answered(Some(self.0.clone()))
        })
    }
}

#[tokio::test]
async fn prefers_the_interactive_shell_and_reports_both_sources() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let interactive_dir = root.path().join("interactive bin");
    let interactive = cli(&interactive_dir);
    let service = root.path().join("service");
    let login = cli(&service);
    let shell = Shell::answers(Some(&interactive));
    let resolved = cache
        .resolve(
            "fixture-cli",
            &path_with(std::slice::from_ref(&service)),
            &shell,
        )
        .await;
    assert_eq!(
        resolved,
        found(&interactive, CommandSource::InteractiveShell)
    );
    assert_eq!(
        resolved.as_ref().and_then(ResolvedCommand::dir),
        interactive_dir.to_str()
    );
    assert_eq!(shell.asked(), 1);
    // The login shell's PATH still reports its own CLI, for the environment check.
    assert_eq!(
        search_path("fixture-cli", &path_with(&[service])),
        Some(login)
    );
}

#[tokio::test]
async fn falls_back_to_the_service_path() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let service = root.path().join("service");
    let login = cli(&service);
    let unrunnable = root.path().join("unrunnable/fixture-cli");
    std::fs::create_dir_all(unrunnable.parent().expect("parent")).expect("owned fixture");
    std::fs::write(&unrunnable, "#!/bin/sh\n").expect("owned fixture");
    std::fs::set_permissions(&unrunnable, std::fs::Permissions::from_mode(0o600))
        .expect("owned fixture mode");
    // `command -v` found nothing, the shell gave no answer, or it named a file that cannot run.
    for (case, shell) in [
        Shell::answers(None),
        Shell::silent(),
        Shell::answers(Some(&unrunnable)),
    ]
    .into_iter()
    .enumerate()
    {
        let path = path_with(&[root.path().join(format!("none-{case}")), service.clone()]);
        let resolved = cache.resolve("fixture-cli", &path, &shell).await;
        assert_eq!(
            resolved,
            found(&login, CommandSource::ServicePath),
            "case {case}"
        );
        assert_eq!(shell.asked(), 1, "case {case}");
    }
}

#[test]
fn reads_only_the_value_between_markers() {
    let root = tempfile::tempdir().expect("owned fixture");
    let ours = root.path().join("ours/fixture-cli");
    let decoy = root.path().join("decoy/fixture-cli");
    let mut noisy = b"Welcome!\nnvm: using node v24\n".to_vec();
    noisy.extend(marked(&ours));
    noisy.extend(b"\nbye from an EXIT trap\n");
    let mut decoyed = marked(&decoy);
    decoyed.extend(b" rc output ");
    decoyed.extend(marked(&ours));
    decoyed.extend(b"trailing");
    for (case, stdout) in [noisy, decoyed].into_iter().enumerate() {
        assert_eq!(
            answer(Some(stdout)),
            ShellAnswer::Answered(Some(ours.clone())),
            "case {case}"
        );
    }
    // Between the markers but not an absolute path (an alias, a function, a builtin, extra
    // text): the shell answered, with nothing usable.
    let mut extra = b"\x1e".to_vec();
    extra.extend(ours.to_string_lossy().as_bytes());
    extra.extend(b"\nnot part of it\x1e");
    for stdout in [
        extra,
        b"\x1erelative/fixture-cli\x1e".to_vec(),
        b"\x1efixture-cli\x1e".to_vec(),
        b"\x1e~/shell/fixture-cli\x1e".to_vec(),
    ] {
        assert_eq!(answer(Some(stdout)), ShellAnswer::Answered(None));
    }
    // A clean empty answer: `command -v` found nothing.
    assert_eq!(
        answer(Some(b"rc noise\x1e\x1e".to_vec())),
        ShellAnswer::Answered(None)
    );
    // A single marker, no markers, no output or too much output is no answer.
    let mut single = b"noise\x1e".to_vec();
    single.extend(ours.to_string_lossy().as_bytes());
    let mut flood = marked(&ours);
    flood.resize(OUTPUT_LIMIT + 1, b'x');
    for (case, stdout) in [
        Some(single),
        Some(b"exec zsh happened".to_vec()),
        None,
        Some(flood),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(answer(stdout), ShellAnswer::NoAnswer, "case {case}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_late_shell_is_abandoned_after_five_seconds() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let shell_cli = cli(&root.path().join("shell"));
    let started = tokio::time::Instant::now();
    let path = path_with(&[root.path().join("late")]);
    assert_eq!(
        cache.resolve("fixture-cli", &path, &Late(shell_cli)).await,
        None
    );
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "abandoned at 5 s"
    );
}

#[tokio::test]
async fn lookup_never_runs_the_program() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let interactive = cli(&root.path().join("interactive"));
    let service = root.path().join("service");
    let login = cli(&service);
    let launcher = tool(
        &root.path().join("launchers"),
        "owned-launcher",
        "#!/bin/sh\n: > \"$0.invoked\"\n",
    );
    let path = |case: &str| path_with(&[service.clone(), root.path().join(case)]);
    // Found by the interactive shell, by the service PATH, and as an explicit path.
    let lookups = [
        (
            cache
                .resolve(
                    "fixture-cli",
                    &path("interactive"),
                    &Shell::answers(Some(&interactive)),
                )
                .await,
            found(&interactive, CommandSource::InteractiveShell),
        ),
        (
            cache
                .resolve("fixture-cli", &path("service"), &Shell::silent())
                .await,
            found(&login, CommandSource::ServicePath),
        ),
        (
            cache
                .resolve(
                    launcher.to_str().expect("fixture UTF-8"),
                    &path("explicit"),
                    &Shell::silent(),
                )
                .await,
            found(&launcher, CommandSource::ServicePath),
        ),
    ];
    for (case, (resolved, expected)) in lookups.into_iter().enumerate() {
        assert_eq!(resolved, expected, "case {case}");
    }
    for program in [&interactive, &login, &launcher] {
        assert!(!invoked(program), "{} was started", program.display());
    }
}

#[tokio::test]
async fn results_are_cached_for_five_minutes() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let shell_cli = cli(&root.path().join("shell"));
    let path = path_with(&[root.path().join("service")]);
    let shell = Shell::answers(Some(&shell_cli));
    let first = cache.resolve("fixture-cli", &path, &shell).await;
    assert!(first.is_some());
    assert_eq!(cache.resolve("fixture-cli", &path, &shell).await, first);
    assert_eq!(shell.asked(), 1);
    cache.advance(Duration::from_secs(5 * 60));
    assert!(cache.resolve("fixture-cli", &path, &shell).await.is_some());
    assert_eq!(shell.asked(), 2);
    // A CLI removed since is looked up again rather than started.
    std::fs::remove_file(&shell_cli).expect("owned fixture");
    assert_eq!(cache.resolve("fixture-cli", &path, &shell).await, None);
    assert_eq!(shell.asked(), 3);
}

#[tokio::test]
async fn each_cache_keeps_its_own_clock() {
    let root = tempfile::tempdir().expect("owned fixture");
    let shell_cli = cli(&root.path().join("shell"));
    let path = path_with(&[root.path().join("service")]);
    let shell = Shell::answers(Some(&shell_cli));
    let (moved, still) = (LookupCache::stopped(), LookupCache::stopped());
    for cache in [&moved, &still] {
        assert!(cache.resolve("fixture-cli", &path, &shell).await.is_some());
    }
    assert_eq!(shell.asked(), 2);
    // Five minutes pass on one cache's clock: its lookup expires, and keeping the new one
    // prunes only its own entries...
    moved.advance(Duration::from_secs(5 * 60));
    assert!(moved.resolve("fixture-cli", &path, &shell).await.is_some());
    assert_eq!(shell.asked(), 3);
    // ...while the other still answers from its own, without asking the shell.
    assert!(still.resolve("fixture-cli", &path, &shell).await.is_some());
    assert_eq!(shell.asked(), 3);
}

#[tokio::test]
async fn a_clean_empty_answer_is_cached_as_not_found() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let path = path_with(&[root.path().join("service")]);
    let nothing = Shell::answers(None);
    assert_eq!(cache.resolve("fixture-cli", &path, &nothing).await, None);
    assert_eq!(cache.resolve("fixture-cli", &path, &nothing).await, None);
    assert_eq!(nothing.asked(), 1, "one shell run per 5 minutes");
}

#[tokio::test]
async fn a_start_without_an_answer_uses_the_service_path_once() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let interactive = cli(&root.path().join("interactive"));
    let service = root.path().join("service");
    let login = cli(&service);
    let path = path_with(std::slice::from_ref(&service));
    // The shell gave no answer: this start runs the service PATH's CLI...
    assert_eq!(
        cache.resolve("fixture-cli", &path, &Shell::silent()).await,
        found(&login, CommandSource::ServicePath)
    );
    // ...and that is not kept: the next start asks the shell again.
    let shell = Shell::answers(Some(&interactive));
    assert_eq!(
        cache.resolve("fixture-cli", &path, &shell).await,
        found(&interactive, CommandSource::InteractiveShell)
    );
    assert_eq!(shell.asked(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_check_without_an_answer_keeps_the_cached_interactive_path() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let interactive = cli(&root.path().join("interactive"));
    let service = root.path().join("service");
    let login = cli(&service);
    let path = path_with(std::slice::from_ref(&service));
    // Both shells see the CLI; a start keeps the interactive shell's.
    let shell = Shell::answers(Some(&interactive));
    let kept = cache.resolve("fixture-cli", &path, &shell).await;
    assert_eq!(kept, found(&interactive, CommandSource::InteractiveShell));
    // The check's shell times out: its row shows this run's service PATH lookup...
    assert_eq!(
        cache
            .refresh("fixture-cli", &path, &Late(interactive.clone()))
            .await,
        found(&login, CommandSource::ServicePath)
    );
    // ...and the next start still runs the interactive shell's CLI, without asking again.
    assert_eq!(cache.resolve("fixture-cli", &path, &shell).await, kept);
    assert_eq!(shell.asked(), 1);
}

#[tokio::test]
async fn a_check_without_an_answer_stores_nothing_new() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let interactive = cli(&root.path().join("interactive"));
    let service = root.path().join("service");
    let login = cli(&service);
    let path = path_with(std::slice::from_ref(&service));
    // Nothing is kept yet and the check's shell gives no answer: its row shows the service
    // PATH's CLI...
    assert_eq!(
        cache.refresh("fixture-cli", &path, &Shell::silent()).await,
        found(&login, CommandSource::ServicePath)
    );
    // ...and the next start asks the shell instead of reusing that row.
    let shell = Shell::answers(Some(&interactive));
    assert_eq!(
        cache.resolve("fixture-cli", &path, &shell).await,
        found(&interactive, CommandSource::InteractiveShell)
    );
    assert_eq!(shell.asked(), 1);
}

#[tokio::test]
async fn refresh_replaces_the_cached_lookup() {
    let root = tempfile::tempdir().expect("owned fixture");
    let cache = LookupCache::stopped();
    let service = root.path().join("service");
    let path = path_with(std::slice::from_ref(&service));
    let shell = Shell::answers(None);
    // A start before the CLI is installed finds nothing, and that answer is kept.
    assert_eq!(cache.resolve("fixture-cli", &path, &shell).await, None);
    let installed = cli(&service);
    assert_eq!(cache.resolve("fixture-cli", &path, &shell).await, None);
    assert_eq!(shell.asked(), 1);
    // A fresh look finds it and replaces the kept answer, so the next start uses it at once.
    let fresh = cache.refresh("fixture-cli", &path, &shell).await;
    assert_eq!(fresh, found(&installed, CommandSource::ServicePath));
    assert_eq!(shell.asked(), 2);
    assert_eq!(cache.resolve("fixture-cli", &path, &shell).await, fresh);
    assert_eq!(shell.asked(), 2);
}

#[test]
fn only_a_plain_first_word_is_resolved() {
    for (command, program) in [
        ("claude", Some("claude")),
        ("codex --search", Some("codex")),
        ("  opencode\t--port 0", Some("opencode")),
        ("/opt/pi/bin/pi -e x", Some("/opt/pi/bin/pi")),
        ("node_modules/.bin/pi", None),
        ("./run-agent.sh", None),
        ("FOO=1 claude", None),
        ("\"claude\" --verbose", None),
        ("$HOME/bin/claude", None),
        ("~/bin/claude", None),
        ("source ~/.nvm/nvm.sh && claude", None),
        (". ~/.profile; claude", None),
        ("exec claude", None),
        ("cd sub && claude", None),
        ("(claude)", None),
        ("claude;true", None),
        ("-x", None),
        ("", None),
    ] {
        assert_eq!(program_of(command), program, "{command:?}");
    }
}

/// The environment check asks a CLI for its version with the `PATH` its start script gives it,
/// so a check cannot pass where the start would fail.
#[test]
fn the_version_check_puts_the_cli_folder_where_the_start_script_does() {
    use std::os::unix::ffi::OsStrExt;
    let service = "/owned/service/bin:/owned/other/bin";
    for (dir, expected) in [
        // A shared folder goes last; any other goes first.
        ("/usr/bin", format!("{service}:/usr/bin")),
        (
            "/home/u/.nvm/versions/node/v24.1.0/bin",
            format!("/home/u/.nvm/versions/node/v24.1.0/bin:{service}"),
        ),
    ] {
        let check = version_command(&Path::new(dir).join("claude"), OsStr::new(service));
        let checked = check
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("PATH"))
            .and_then(|(_, value)| value)
            .expect("the check sets PATH");
        let script = crate::start::render_start_script(&crate::start::StartPlan {
            devbox: "/b/devbox".into(),
            instance: "test".into(),
            agent_id: "a1".into(),
            launch_id: "launch-1".into(),
            phase_file: "/d/phase".into(),
            port: 5200,
            workdir: "/w".into(),
            setup: vec![],
            tool_command: "claude".into(),
            tool_dir: Some(dir.into()),
            hooks: crate::profile::Hooks::None,
            hook_file: None,
            prompt_file: None,
            resume_session: None,
            session_id: None,
            use_scope: true,
        });
        // The script's own PATH lines, run by a shell that starts with the service PATH.
        let lines: Vec<&str> = script
            .lines()
            .filter(|line| line.starts_with("export PATH="))
            .collect();
        let shell = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("{}\nprintf '%s' \"$PATH\"", lines.join("\n")))
            .env_clear()
            .env("PATH", service)
            .output()
            .expect("owned shell");
        assert!(shell.status.success(), "{dir}: {shell:?}");
        assert_eq!(OsStr::from_bytes(&shell.stdout), checked, "{dir}");
        assert_eq!(checked, OsStr::new(&expected), "{dir}");
    }
}
