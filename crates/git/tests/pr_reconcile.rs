#![cfg(unix)]

use devbox_git::{GitError, mutate, run};
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

const CHILD: &str = "DEVBOX_PR_RECONCILE_CASE";
const URL: &str = "https://example.invalid/owned/pull/1";
const FAKE_GH: &str = r#"
import os, pathlib, sys, time
args = sys.argv[1:]
mode = os.environ['DEVBOX_PR_RECONCILE_CASE']
root = pathlib.Path(os.environ['DEVBOX_PR_FIXTURE'])
state = root / 'remote-pr'
if args[:2] == ['pr', 'list']:
    if mode == 'lookup-error' or (mode == 'retry' and state.exists() and not (root / 'query-failed').exists()):
        (root / 'query-failed').touch()
        print('owned lookup unavailable', file=sys.stderr)
        sys.exit(19)
    if state.exists():
        sys.stdout.write(state.read_text())
    sys.exit(0)
if args[:2] != ['pr', 'create']:
    sys.exit(99)
with (root / 'creates').open('a') as output:
    output.write('create\n')
if state.exists():
    print('owned PR already exists', file=sys.stderr)
    sys.exit(18)
body = pathlib.Path(args[args.index('--body-file') + 1]).read_bytes()
(root / 'body').write_bytes(body)
state.write_text((root / 'expected-record').read_text())
if mode in ['lost-error', 'retry']:
    print('owned create response lost', file=sys.stderr)
    sys.exit(17)
if mode == 'lost-empty':
    sys.exit(0)
if mode in ['lost-timeout', 'cancelled']:
    time.sleep(60)
print('https://example.invalid/owned/pull/1')
"#;

fn isolated_cases(cases: &[&str]) {
    let mut failures = Vec::new();
    for case in cases {
        let fixture = tempfile::tempdir().expect("owned fixture");
        let bin = fixture.path().join("bin");
        std::fs::create_dir(&bin).expect("owned fixture");
        std::os::unix::fs::symlink(
            which::which("git").expect("Git executable"),
            bin.join("git"),
        )
        .expect("owned fixture");
        let python = which::which("python3").expect("Python executable");
        let gh = bin.join("gh");
        std::fs::write(&gh, format!("#!{}\n{FAKE_GH}", python.display())).expect("owned fixture");
        std::fs::set_permissions(gh, std::fs::Permissions::from_mode(0o700))
            .expect("owned fixture");
        let config = fixture.path().join("global.gitconfig");
        std::fs::write(&config, "").expect("owned fixture");
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "reconcile_child", "--nocapture"])
            .env(CHILD, case)
            .env("DEVBOX_PR_FIXTURE", fixture.path())
            .env("GIT_CONFIG_GLOBAL", config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_COUNT", "0")
            .env("PATH", bin)
            .output()
            .expect("owned child");
        if !output.status.success() {
            failures.push(format!(
                "{case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn ambiguous_create_failures_and_retries_reconcile_without_duplicate_creation() {
    isolated_cases(&[
        "lost-error",
        "lost-empty",
        "lost-timeout",
        "cancelled",
        "retry",
        "existing",
    ]);
}

#[test]
fn existing_pr_requires_verified_metadata_and_lookup_failure_does_not_create() {
    isolated_cases(&[
        "wrong-base",
        "wrong-head",
        "wrong-oid",
        "cross-repository",
        "closed",
        "unsafe-url",
        "credential-url",
        "ambiguous",
        "lookup-error",
    ]);
}

fn git(directory: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command
        .current_dir(directory)
        .args(args)
        .env("GIT_AUTHOR_NAME", "owned fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@devbox.invalid")
        .env("GIT_COMMITTER_NAME", "owned fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@devbox.invalid");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
    ] {
        command.env_remove(key);
    }
    let output = command.output().expect("owned Git process");
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn reconcile_child() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    let root = std::env::var_os("DEVBOX_PR_FIXTURE").expect("owned fixture");
    let root = Path::new(&root);
    let repo = root.join("repository");
    std::fs::create_dir(&repo).expect("owned fixture");
    git(&repo, &["init", "-q", "--template=", "-b", "main"]);
    std::fs::write(repo.join("base"), "base").expect("owned fixture");
    git(&repo, &["add", "base"]);
    git(&repo, &["commit", "-qm", "base"]);
    let origin = root.join("origin.git");
    git(
        root,
        &[
            "init",
            "--bare",
            "-q",
            "--template=",
            origin.to_str().expect("owned fixture"),
        ],
    );
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("owned fixture"),
        ],
    );
    git(&repo, &["checkout", "-qb", "agent/reconcile"]);
    std::fs::write(repo.join("change"), "change").expect("owned fixture");
    git(&repo, &["add", "change"]);
    git(&repo, &["commit", "-qm", "owned change"]);
    let oid = run::resolve(&repo, "HEAD").await.expect("owned head");
    let expected = format!("{URL}\tmain\tagent/reconcile\t{oid}\tfalse\tOPEN\n");
    let record = match case.as_str() {
        "wrong-base" => expected.replace("\tmain\t", "\tother\t"),
        "wrong-head" => expected.replace("\tagent/reconcile\t", "\tagent/other\t"),
        "wrong-oid" => expected.replace(&oid, &"0".repeat(oid.len())),
        "cross-repository" => expected.replace("\tfalse\t", "\ttrue\t"),
        "closed" => expected.replace("\tOPEN", "\tCLOSED"),
        "unsafe-url" => expected.replace(URL, "javascript:alert(1)"),
        "credential-url" => expected.replace(URL, "https://fixture-secret@example.invalid/pull/1"),
        "ambiguous" => expected.repeat(2),
        _ => expected.clone(),
    };
    if !matches!(
        case.as_str(),
        "lost-error" | "lost-empty" | "lost-timeout" | "cancelled" | "retry" | "lookup-error"
    ) {
        std::fs::write(root.join("remote-pr"), &record).expect("owned fixture");
    }
    // The create stub reads owned metadata without mutating process environment.
    std::fs::write(root.join("expected-record"), &record).expect("owned fixture");
    let body = "owned line one\n\n$(literal) `literal`";
    let call =
        || mutate::push_and_pr_at(&repo, "agent/reconcile", &oid, "main", "Owned title", body);
    let first = if case == "cancelled" {
        let mut request = Box::pin(call());
        let created = async {
            while !root.join("remote-pr").exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        };
        tokio::select! {
            result = &mut request => panic!("create must remain in flight: {result:?}"),
            result = tokio::time::timeout(std::time::Duration::from_secs(5), created) => {
                result.expect("owned create became visible");
            }
        }
        drop(request);
        call().await
    } else {
        call().await
    };
    if case == "retry" {
        assert!(matches!(first, Err(GitError::GhFailed { .. })), "{first:?}");
        assert_eq!(call().await.expect("retry must recover existing PR"), URL);
    } else if matches!(
        case.as_str(),
        "lost-error" | "lost-empty" | "lost-timeout" | "cancelled" | "existing"
    ) {
        assert_eq!(
            first.expect("created or existing PR must be reconciled"),
            URL
        );
    } else {
        assert!(first.is_err(), "unverified PR accepted: {first:?}");
    }
    let creates = std::fs::read_to_string(root.join("creates")).unwrap_or_default();
    let expected_creates = usize::from(matches!(
        case.as_str(),
        "lost-error" | "lost-empty" | "lost-timeout" | "cancelled" | "retry"
    ));
    assert_eq!(
        creates.lines().count(),
        expected_creates,
        "duplicate or unverified creation"
    );
    assert_eq!(
        run::resolve(&origin, "agent/reconcile")
            .await
            .expect("owned pushed ref"),
        oid
    );
    if expected_creates > 0 {
        assert_eq!(
            std::fs::read_to_string(root.join("body")).expect("owned body"),
            body
        );
    }
}
