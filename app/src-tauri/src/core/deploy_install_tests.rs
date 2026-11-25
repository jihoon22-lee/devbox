//! The install script's two paths. `kept` is taken only for a regular file with the expected
//! SHA-256 in a directory that passed every check; everything else is the send path as before.
use super::*;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

fn command(home: &Path, sha: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.current_dir(home)
        .env("HOME", home)
        .env_remove("XDG_DATA_HOME")
        .args(["-c", &install_script_for(AppChannel::Prod), "sh", sha]);
    cmd
}
/// The app's own sender: the binary is written only after the script asks for it.
async fn offered(home: &Path, bytes: &[u8], sha: &str) -> Result<Transfer, ShellError> {
    offer(
        command(home, sha),
        bytes.to_vec(),
        Duration::from_secs(5),
        CancellationToken::new(),
    )
    .await
}
/// A sender that writes the binary whatever the script says, as the acceptance harness does.
async fn eager(home: &Path, bytes: &[u8], sha: &str) -> Result<std::process::Output, ShellError> {
    process::run(
        command(home, sha),
        bytes.to_vec(),
        Duration::from_secs(5),
        CancellationToken::new(),
    )
    .await
}
fn bin(home: &Path) -> PathBuf {
    home.join(".local/share/devbox/bin")
}
fn stored(home: &Path, bytes: &[u8]) -> PathBuf {
    bin(home).join(format!("devbox-{}", digest(bytes)))
}
fn linked(home: &Path) -> String {
    std::fs::read_link(bin(home).join("devbox"))
        .unwrap()
        .to_string_lossy()
        .into_owned()
}
fn temporary_left(home: &Path) -> bool {
    std::fs::read_dir(bin(home)).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".install-")
    })
}
/// The exit code and what the script had said on stdout before it refused.
fn refusal(result: Result<Transfer, ShellError>) -> (Option<i32>, String) {
    match result {
        Err(ShellError::Exit { code, stdout, .. }) => (code, stdout),
        other => panic!("expected the script to refuse, got {other:?}"),
    }
}
fn identity(path: &Path) -> (u64, i64, i64, u32) {
    let meta = std::fs::symlink_metadata(path).unwrap();
    (
        meta.ino(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.mode() & 0o7777,
    )
}

#[tokio::test]
async fn the_binary_is_sent_once_and_then_kept_without_reading_or_rewriting() {
    // Spaces and pattern characters in the path: every expansion in the script is quoted.
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("ho me *[x]");
    std::fs::create_dir(&home).unwrap();
    let a = b"first executable";
    assert_eq!(offered(&home, a, &digest(a)).await.unwrap(), Transfer::Sent);
    let (file, link) = (stored(&home, a), bin(&home).join("devbox"));
    assert_eq!(identity(&file).3, 0o755);
    let before = (identity(&file), identity(&link));
    assert_eq!(offered(&home, a, &digest(a)).await.unwrap(), Transfer::Kept);
    // Nothing offered at all still passes: the installed file itself was hashed.
    assert_eq!(
        offered(&home, b"", &digest(a)).await.unwrap(),
        Transfer::Kept
    );
    // A sender that writes anyway is not read: other bytes cannot replace the verified file.
    let output = eager(&home, b"not the expected bytes", &digest(a))
        .await
        .unwrap();
    assert_eq!(output.stdout, b"kept\n");
    assert_eq!((identity(&file), identity(&link)), before);
    assert_eq!(std::fs::read(&link).unwrap(), a);
    assert!(!temporary_left(&home));
    assert_eq!(std::fs::read_dir(bin(&home)).unwrap().count(), 3);
}

#[tokio::test]
async fn an_update_with_another_hash_is_always_sent_and_replaces_the_daemon_binary() {
    let home = tempfile::tempdir().unwrap();
    let (old, new) = (b"old daemon".as_slice(), b"new daemon".as_slice());
    assert_eq!(
        offered(home.path(), old, &digest(old)).await.unwrap(),
        Transfer::Sent
    );
    assert_eq!(
        offered(home.path(), new, &digest(new)).await.unwrap(),
        Transfer::Sent
    );
    assert_eq!(linked(home.path()), format!("devbox-{}", digest(new)));
    assert_eq!(std::fs::read(bin(home.path()).join("devbox")).unwrap(), new);
    assert_eq!(
        offered(home.path(), new, &digest(new)).await.unwrap(),
        Transfer::Kept
    );
    // The earlier binary is still stored (nothing prunes it, as before). Going back to it needs
    // no transfer once its own bytes pass the hash; only the link moves.
    assert_eq!(std::fs::read(stored(home.path(), old)).unwrap(), old);
    assert_eq!(
        offered(home.path(), b"", &digest(old)).await.unwrap(),
        Transfer::Kept
    );
    assert_eq!(linked(home.path()), format!("devbox-{}", digest(old)));
    assert_eq!(std::fs::read(bin(home.path()).join("devbox")).unwrap(), old);
    assert!(!temporary_left(home.path()));
}

#[tokio::test]
async fn a_stored_file_that_is_not_the_expected_regular_file_is_sent_for_and_refused_as_before() {
    let wanted = b"the bundled daemon".as_slice();
    type Damage = fn(&Path, &[u8]);
    let damages: [(&str, Damage); 4] = [
        ("truncated", |target, bytes| {
            std::fs::write(target, &bytes[..bytes.len() - 1]).unwrap()
        }),
        ("symlink to the right bytes", |target, bytes| {
            let real = target.with_file_name("elsewhere");
            std::fs::write(&real, bytes).unwrap();
            std::os::unix::fs::symlink(&real, target).unwrap();
        }),
        ("directory", |target, _| {
            std::fs::create_dir(target).unwrap()
        }),
        ("hash cannot be computed", |target, bytes| {
            std::fs::write(target, bytes).unwrap();
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o000)).unwrap();
        }),
    ];
    for (name, damage) in damages {
        let home = tempfile::tempdir().unwrap();
        let first = b"installed before";
        offered(home.path(), first, &digest(first)).await.unwrap();
        let target = stored(home.path(), wanted);
        damage(&target, wanted);
        // An unreadable file is readable to root, which then finds the right bytes.
        if name == "hash cannot be computed" && std::fs::read(&target).is_ok() {
            continue;
        }
        let (code, said) = refusal(offered(home.path(), wanted, &digest(wanted)).await);
        assert_eq!((code, said.as_str()), (Some(66), "send\n"), "{name}");
        assert_eq!(
            linked(home.path()),
            format!("devbox-{}", digest(first)),
            "{name}"
        );
        assert!(!temporary_left(home.path()), "{name}");
    }
}

#[tokio::test]
async fn a_damaged_linked_install_is_refused_before_any_answer() {
    let home = tempfile::tempdir().unwrap();
    let a = b"linked daemon".as_slice();
    offered(home.path(), a, &digest(a)).await.unwrap();
    std::fs::write(stored(home.path(), a), &a[..a.len() - 1]).unwrap();
    // Neither the same binary nor an update gets past a linked file that fails its own hash.
    for bytes in [a, b"an update".as_slice()] {
        let (code, said) = refusal(offered(home.path(), bytes, &digest(bytes)).await);
        assert_eq!((code, said.as_str()), (Some(64), ""));
    }
    std::fs::write(stored(home.path(), a), a).unwrap();
    assert_eq!(
        offered(home.path(), a, &digest(a)).await.unwrap(),
        Transfer::Kept
    );
    // A link that is not a link, or leads outside the stored names, is refused the same way.
    let link = bin(home.path()).join("devbox");
    std::fs::remove_file(&link).unwrap();
    std::fs::write(&link, a).unwrap();
    assert_eq!(
        refusal(offered(home.path(), a, &digest(a)).await).0,
        Some(64)
    );
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/bin/sh", &link).unwrap();
    assert_eq!(
        refusal(offered(home.path(), a, &digest(a)).await).0,
        Some(64)
    );
}

#[tokio::test]
async fn the_owner_marker_and_the_private_directory_are_checked_before_the_fast_path() {
    let home = tempfile::tempdir().unwrap();
    let a = b"owned daemon".as_slice();
    offered(home.path(), a, &digest(a)).await.unwrap();
    let directory = bin(home.path());
    let owner = directory.join(".owner");
    let marker = std::fs::read(&owner).unwrap();
    let refused = async || {
        let (code, said) = refusal(offered(home.path(), a, &digest(a)).await);
        assert_eq!((code, said.as_str()), (Some(63), ""));
    };
    std::fs::write(&owner, "someone-else\n").unwrap();
    refused().await;
    std::fs::remove_file(&owner).unwrap();
    refused().await;
    let elsewhere = home.path().join("marker");
    std::fs::write(&elsewhere, &marker).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &owner).unwrap();
    refused().await;
    std::fs::remove_file(&owner).unwrap();
    std::fs::write(&owner, &marker).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    refused().await;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        offered(home.path(), a, &digest(a)).await.unwrap(),
        Transfer::Kept
    );
    assert_eq!(std::fs::read(directory.join("devbox")).unwrap(), a);
}

#[tokio::test]
async fn bytes_that_do_not_match_a_hash_that_is_not_installed_are_refused() {
    let home = tempfile::tempdir().unwrap();
    let a = b"installed".as_slice();
    offered(home.path(), a, &digest(a)).await.unwrap();
    let wanted = digest(b"expected but never offered");
    for bytes in [b"something else".as_slice(), b""] {
        let (code, said) = refusal(offered(home.path(), bytes, &wanted).await);
        assert_eq!((code, said.as_str()), (Some(65), "send\n"));
    }
    assert_eq!(linked(home.path()), format!("devbox-{}", digest(a)));
    assert!(!bin(home.path()).join(format!("devbox-{wanted}")).exists());
    assert!(!temporary_left(home.path()));
}

#[tokio::test]
async fn only_the_scripts_own_answer_and_a_clean_exit_count_as_installed() {
    let run = async |script: &str| {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        offer(
            cmd,
            b"payload".to_vec(),
            Duration::from_secs(5),
            CancellationToken::new(),
        )
        .await
    };
    assert_eq!(run("printf 'kept\\n'").await.unwrap(), Transfer::Kept);
    // `wc` proves the payload arrived only after `send`.
    assert_eq!(
        run("printf 'send\\n'; [ \"$(cat | wc -c)\" = 7 ]")
            .await
            .unwrap(),
        Transfer::Sent
    );
    for script in [
        "exit 0",
        "printf 'ok\\n'",
        "printf 'kept and more\\n'",
        "printf ' kept\\n'",
        "printf 'KEPT\\n'",
        "printf '\\nkept\\n'",
    ] {
        assert!(
            matches!(run(script).await, Err(ShellError::Invalid(_))),
            "{script}"
        );
    }
    // Nothing is written for an unknown answer, and a failed exit wins over a good answer.
    assert!(matches!(
        run("printf 'ok\\n'; [ \"$(cat | wc -c)\" = 0 ] || exit 9").await,
        Err(ShellError::Invalid(_))
    ));
    for script in [
        "printf 'kept\\n'; exit 3",
        "printf 'send\\n'; cat >/dev/null; exit 65",
    ] {
        assert!(
            matches!(run(script).await, Err(ShellError::Exit { .. })),
            "{script}"
        );
    }
    assert_eq!(Transfer::Kept.phase(), StartPhase::InstallKept);
    assert_eq!(Transfer::Sent.phase(), StartPhase::InstallSent);
}

#[tokio::test]
async fn the_script_keeps_the_shape_the_acceptance_harness_extracts_and_runs() {
    // acceptance/harness/wsl_daemon.py reads exactly one raw string of each name from this
    // file, joins them as `install_script_for` does, and runs `sh -c <script> sh <sha256>` with
    // the whole binary on stdin.
    let source = include_str!("deploy.rs");
    for name in ["PATHS_SCRIPT", "INSTALL_SCRIPT"] {
        let opening = format!("const {name}: &str = r#\"");
        assert_eq!(source.matches(&opening).count(), 1, "{name}");
    }
    assert!(source.contains(r#"format!("{paths}\n{INSTALL_SCRIPT}")"#));
    assert!(source.contains(r#"format!("{paths}\nexec \"$d/devbox\" \"$@\"")"#));
    let script = install_script_for(AppChannel::Accept);
    assert!(script.starts_with("set -eu\n"));
    assert!(!INSTALL_SCRIPT.contains("\"#") && !PATHS_SCRIPT.contains("\"#"));
    // Stdin has one reader, after the answer: a sender that waits and one that does not both work.
    assert_eq!(INSTALL_SCRIPT.matches("cat >").count(), 1);
    let (asks, reads) = (
        INSTALL_SCRIPT.find("printf '%s\\n' send").unwrap(),
        INSTALL_SCRIPT.find("cat > \"$t/binary\"").unwrap(),
    );
    assert!(asks < reads);
    assert!(!INSTALL_SCRIPT.contains("read ") && !INSTALL_SCRIPT.contains("/dev/stdin"));
    // Its own exits stay the refusals the harness classifies (61-66).
    let exits: std::collections::BTreeSet<u32> = INSTALL_SCRIPT
        .split("exit ")
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(
        exits.into_iter().collect::<Vec<_>>(),
        [0, 61, 62, 63, 64, 65, 66]
    );
    let mut check = Command::new("sh");
    check.args(["-n", "-c", &script]);
    process::run(
        check,
        vec![],
        Duration::from_secs(5),
        CancellationToken::new(),
    )
    .await
    .unwrap();
}
