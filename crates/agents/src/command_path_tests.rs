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
/// A distinct service `PATH` per lookup: results are cached per program and `PATH`.
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

/// Answers every lookup with the same stdout, or with no answer, and counts the lookups.
struct Shell {
    stdout: Option<Vec<u8>>,
    asked: AtomicUsize,
}
impl Shell {
    fn says(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            stdout: Some(stdout.into()),
            asked: AtomicUsize::new(0),
        }
    }
    fn fails() -> Self {
        Self {
            stdout: None,
            asked: AtomicUsize::new(0),
        }
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}
impl ShellProbe for Shell {
    fn command_v<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Option<Vec<u8>>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let stdout = self.stdout.clone();
        Box::pin(async move { stdout })
    }
}
/// Answers only after the probe's time is up.
struct Late(Vec<u8>);
impl ShellProbe for Late {
    fn command_v<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            Some(self.0.clone())
        })
    }
}

#[tokio::test]
async fn prefers_the_interactive_shell_and_reports_both_sources() {
    let root = tempfile::tempdir().expect("owned fixture");
    let interactive_dir = root.path().join("interactive bin");
    let interactive = cli(&interactive_dir);
    let service = root.path().join("service");
    let login = cli(&service);
    let shell = Shell::says(marked(&interactive));
    let resolved = resolve(
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
    let service = root.path().join("service");
    let login = cli(&service);
    let unrunnable = root.path().join("unrunnable/fixture-cli");
    std::fs::create_dir_all(unrunnable.parent().expect("parent")).expect("owned fixture");
    std::fs::write(&unrunnable, "#!/bin/sh\n").expect("owned fixture");
    std::fs::set_permissions(&unrunnable, std::fs::Permissions::from_mode(0o600))
        .expect("owned fixture mode");
    // `command -v` found nothing, the shell failed, or it named a file that cannot run.
    for (case, shell) in [
        Shell::says(b"\x1e\x1e".to_vec()),
        Shell::fails(),
        Shell::says(marked(&unrunnable)),
    ]
    .into_iter()
    .enumerate()
    {
        let path = path_with(&[root.path().join(format!("none-{case}")), service.clone()]);
        let resolved = resolve("fixture-cli", &path, &shell).await;
        assert_eq!(
            resolved,
            found(&login, CommandSource::ServicePath),
            "case {case}"
        );
        assert_eq!(shell.asked(), 1, "case {case}");
    }
}

#[tokio::test]
async fn reads_only_the_value_between_markers() {
    let root = tempfile::tempdir().expect("owned fixture");
    let ours = cli(&root.path().join("ours"));
    let decoy = cli(&root.path().join("decoy"));
    let mut noisy = b"Welcome!\nnvm: using node v24\n".to_vec();
    noisy.extend(marked(&ours));
    noisy.extend(b"\nbye from an EXIT trap\n");
    let mut decoyed = marked(&decoy);
    decoyed.extend(b" rc output ");
    decoyed.extend(marked(&ours));
    decoyed.extend(b"trailing");
    for (case, stdout) in [noisy, decoyed].into_iter().enumerate() {
        let path = path_with(&[root.path().join(format!("none-{case}"))]);
        let resolved = resolve("fixture-cli", &path, &Shell::says(stdout)).await;
        assert_eq!(
            resolved.map(|resolved| resolved.program),
            Some(ours.clone()),
            "case {case}"
        );
    }
    // Text inside the markers, or a single marker, is not a path.
    let mut extra = b"\x1e".to_vec();
    extra.extend(ours.to_string_lossy().as_bytes());
    extra.extend(b"\nnot part of it\x1e");
    let mut single = b"noise\x1e".to_vec();
    single.extend(ours.to_string_lossy().as_bytes());
    for (case, stdout) in [extra, single].into_iter().enumerate() {
        let path = path_with(&[root.path().join(format!("rejected-{case}"))]);
        assert_eq!(
            resolve("fixture-cli", &path, &Shell::says(stdout)).await,
            None,
            "case {case}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn timeout_or_relative_result_returns_none() {
    let root = tempfile::tempdir().expect("owned fixture");
    let shell_cli = cli(&root.path().join("shell"));
    let started = tokio::time::Instant::now();
    let path = path_with(&[root.path().join("late")]);
    assert_eq!(
        resolve("fixture-cli", &path, &Late(marked(&shell_cli))).await,
        None
    );
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "abandoned at 5 s"
    );
    for (case, stdout) in [
        "\x1erelative/fixture-cli\x1e",
        "\x1efixture-cli\x1e",
        "\x1e~/shell/fixture-cli\x1e",
    ]
    .into_iter()
    .enumerate()
    {
        let path = path_with(&[root.path().join(format!("relative-{case}"))]);
        let shell = Shell::says(stdout);
        assert_eq!(
            resolve("fixture-cli", &path, &shell).await,
            None,
            "case {case}"
        );
        assert_eq!(shell.asked(), 1);
    }
}

#[tokio::test]
async fn lookup_never_runs_the_program() {
    let root = tempfile::tempdir().expect("owned fixture");
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
            resolve(
                "fixture-cli",
                &path("interactive"),
                &Shell::says(marked(&interactive)),
            )
            .await,
            found(&interactive, CommandSource::InteractiveShell),
        ),
        (
            resolve("fixture-cli", &path("service"), &Shell::fails()).await,
            found(&login, CommandSource::ServicePath),
        ),
        (
            resolve(
                launcher.to_str().expect("fixture UTF-8"),
                &path("explicit"),
                &Shell::fails(),
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

#[tokio::test(start_paused = true)]
async fn results_are_cached_for_five_minutes() {
    let root = tempfile::tempdir().expect("owned fixture");
    let shell_cli = cli(&root.path().join("shell"));
    let path = path_with(&[root.path().join("service")]);
    let shell = Shell::says(marked(&shell_cli));
    let first = resolve("fixture-cli", &path, &shell).await;
    assert!(first.is_some());
    assert_eq!(resolve("fixture-cli", &path, &shell).await, first);
    assert_eq!(shell.asked(), 1);
    tokio::time::advance(Duration::from_secs(5 * 60)).await;
    assert!(resolve("fixture-cli", &path, &shell).await.is_some());
    assert_eq!(shell.asked(), 2);
    // A CLI removed since is looked up again rather than started.
    std::fs::remove_file(&shell_cli).expect("owned fixture");
    assert_eq!(resolve("fixture-cli", &path, &shell).await, None);
    assert_eq!(shell.asked(), 3);
    // A missing CLI is remembered too: one shell run per 5 minutes.
    let other = path_with(&[root.path().join("other")]);
    let nothing = Shell::says(b"\x1e\x1e".to_vec());
    assert_eq!(resolve("fixture-cli", &other, &nothing).await, None);
    assert_eq!(resolve("fixture-cli", &other, &nothing).await, None);
    assert_eq!(nothing.asked(), 1);
}

#[tokio::test]
async fn refresh_replaces_the_cached_lookup() {
    let root = tempfile::tempdir().expect("owned fixture");
    let service = root.path().join("service");
    let path = path_with(std::slice::from_ref(&service));
    let shell = Shell::says(b"\x1e\x1e".to_vec());
    // A start before the CLI is installed finds nothing, and that answer is kept.
    assert_eq!(resolve("fixture-cli", &path, &shell).await, None);
    let installed = cli(&service);
    assert_eq!(resolve("fixture-cli", &path, &shell).await, None);
    assert_eq!(shell.asked(), 1);
    // A fresh look finds it and replaces the kept answer, so the next start uses it at once.
    let fresh = refresh("fixture-cli", &path, &shell).await;
    assert_eq!(fresh, found(&installed, CommandSource::ServicePath));
    assert_eq!(shell.asked(), 2);
    assert_eq!(resolve("fixture-cli", &path, &shell).await, fresh);
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
