//! `devbox project list` and `devbox project add` through the real CLI and an owned daemon.
#![cfg(unix)]
mod support;
use devbox_projects::ProjectList;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use support::TestDaemon;

fn devbox(d: &TestDaemon, dir: &Path, args: &[&str]) -> Output {
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.output().expect("run the owned devbox CLI")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8 output")
}

/// An owned folder under the fixture home, by its canonical path (what the daemon stores).
fn folder(d: &TestDaemon, name: &str) -> PathBuf {
    let path = d.home.path().join("repos").join(name);
    std::fs::create_dir_all(&path).expect("owned project folder");
    std::fs::canonicalize(path).expect("canonical owned folder")
}

#[test]
fn list_prints_nothing_for_no_projects_then_name_and_path_as_text_and_json() {
    let mut d = TestDaemon::start();
    let home = d.home.path().to_owned();

    let empty = devbox(&d, &home, &["project", "list"]);
    assert!(empty.status.success(), "{empty:?}");
    assert_eq!(stdout(&empty), "");
    let empty_json = devbox(&d, &home, &["project", "list", "--json"]);
    assert!(empty_json.status.success(), "{empty_json:?}");
    let list: ProjectList = serde_json::from_slice(&empty_json.stdout).expect("project list JSON");
    assert!(list.items.is_empty());

    let (beta, alpha) = (folder(&d, "beta"), folder(&d, "alpha"));
    for path in [&beta, &alpha] {
        let added = devbox(&d, &home, &["project", "add", path.to_str().expect("path")]);
        assert!(added.status.success(), "{added:?}");
    }

    let text = devbox(&d, &home, &["project", "list"]);
    assert!(text.status.success(), "{text:?}");
    assert_eq!(
        stdout(&text),
        format!("alpha\t{}\nbeta\t{}\n", alpha.display(), beta.display())
    );
    let json = devbox(&d, &home, &["project", "list", "--json"]);
    assert!(json.status.success(), "{json:?}");
    let list: ProjectList = serde_json::from_slice(&json.stdout).expect("project list JSON");
    let seen: Vec<_> = list
        .items
        .iter()
        .map(|project| (project.name.as_str(), PathBuf::from(&project.path)))
        .collect();
    assert_eq!(seen, [("alpha", alpha), ("beta", beta)]);
    d.stop();
}

#[test]
fn add_reports_each_path_skips_registered_ones_and_goes_on_past_failures() {
    let mut d = TestDaemon::start();
    let home = d.home.path().to_owned();
    let (one, two) = (folder(&d, "one"), folder(&d, "two"));
    let one_text = one.to_str().expect("path");
    let two_text = two.to_str().expect("path");

    let first = devbox(&d, &home, &["project", "add", one_text]);
    assert!(first.status.success(), "{first:?}");
    assert_eq!(stdout(&first), format!("등록됨: {one_text}\n"));

    // A registered path is skipped, and skipping is not a failure.
    let again = devbox(&d, &home, &["project", "add", one_text]);
    assert!(again.status.success(), "{again:?}");
    assert_eq!(stdout(&again), format!("이미 등록됨: {one_text}\n"));

    let missing = home.join("repos/missing");
    let missing_text = missing.to_str().expect("path");
    let file = home.join("repos/file.txt");
    std::fs::write(&file, "fixture").expect("owned file");
    let file_text = file.to_str().expect("path");
    let mixed = devbox(
        &d,
        &home,
        &[
            "project",
            "add",
            one_text,
            missing_text,
            file_text,
            two_text,
        ],
    );
    assert_eq!(mixed.status.code(), Some(1), "{mixed:?}");
    // Results a script reads are on stdout; failures and the summary are on stderr.
    assert_eq!(
        stdout(&mixed),
        format!("이미 등록됨: {one_text}\n등록됨: {two_text}\n")
    );
    let stderr = String::from_utf8(mixed.stderr.clone()).expect("UTF-8 errors");
    assert_eq!(
        stderr,
        format!(
            "실패: {missing_text}: 경로를 열 수 없습니다. 경로가 맞는지, WSL 안의 폴더인지 확인하세요.\n실패: {file_text}: 폴더가 아닙니다. 프로젝트 폴더의 경로를 넣으세요.\nError: 등록하지 못한 경로가 2개 있습니다.\n"
        )
    );

    let listed = devbox(&d, &home, &["project", "list"]);
    assert_eq!(
        stdout(&listed),
        format!("one\t{one_text}\ntwo\t{two_text}\n")
    );
    d.stop();
}

#[test]
fn add_resolves_a_relative_path_against_the_current_directory() {
    let mut d = TestDaemon::start();
    let nested = folder(&d, "nested");
    let parent = nested.parent().expect("parent folder").to_owned();

    let relative = devbox(&d, &parent, &["project", "add", "nested"]);
    assert!(relative.status.success(), "{relative:?}");
    assert_eq!(stdout(&relative), format!("등록됨: {}\n", nested.display()));
    // The same folder by another relative spelling is the same project.
    let dot = devbox(&d, &nested, &["project", "add", "."]);
    assert!(dot.status.success(), "{dot:?}");
    assert_eq!(stdout(&dot), format!("이미 등록됨: {}\n", nested.display()));
    d.stop();
}

#[test]
fn control_characters_in_names_and_paths_never_reach_the_terminal_as_text() {
    let mut d = TestDaemon::start();
    let home = d.home.path().to_owned();
    let odd = folder(&d, "odd\u{1b}[31m\nname\tend");
    let odd_text = odd.to_str().expect("path");
    let parent = odd.parent().expect("parent folder").display().to_string();
    let shown = format!("{parent}/odd\\u{{1b}}[31m\\nname\\tend");

    let added = devbox(&d, &home, &["project", "add", odd_text]);
    assert!(added.status.success(), "{added:?}");
    assert_eq!(stdout(&added), format!("등록됨: {shown}\n"));
    let again = devbox(&d, &home, &["project", "add", odd_text]);
    assert_eq!(stdout(&again), format!("이미 등록됨: {shown}\n"));
    let missing = devbox(
        &d,
        &home,
        &["project", "add", "/owned\u{1b}]0;x\u{7}/missing"],
    );
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    let stderr = String::from_utf8(missing.stderr.clone()).expect("UTF-8 errors");
    assert!(
        stderr.starts_with("실패: /owned\\u{1b}]0;x\\u{7}/missing: "),
        "{stderr}"
    );

    // One line per project and two columns, whatever the name holds.
    let text = devbox(&d, &home, &["project", "list"]);
    assert_eq!(
        stdout(&text),
        format!("odd\\u{{1b}}[31m\\nname\\tend\t{shown}\n")
    );
    // JSON carries the real name and path; its own escaping keeps them off the terminal.
    let json = devbox(&d, &home, &["project", "list", "--json"]);
    assert!(!json.stdout.contains(&0x1b), "{json:?}");
    let list: ProjectList = serde_json::from_slice(&json.stdout).expect("project list JSON");
    assert_eq!(list.items[0].name, "odd\u{1b}[31m\nname\tend");
    assert_eq!(list.items[0].path, odd_text);
    d.stop();
}

const NOT_CONNECTED: &str = "데몬에 연결할 수 없습니다. devbox 앱을 켠 뒤 다시 실행하세요.";

#[test]
fn project_commands_say_what_to_do_when_devbox_is_not_running() {
    let d = TestDaemon::fixture();
    let home = d.home.path().to_owned();
    let check = |args: &[&str]| {
        let output = devbox(&d, &home, args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(stdout(&output), "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr, format!("Error: {NOT_CONNECTED}\n"));
    };
    for args in [&["project", "list"][..], &["project", "add", "."]] {
        check(args);
    }
    // Something that takes the connection and drops it before the greeting is no daemon either:
    // nothing was asked of it, so there is no result to be unsure of.
    std::fs::create_dir_all(d.socket.parent().expect("socket folder")).expect("owned folder");
    let listener = std::os::unix::net::UnixListener::bind(&d.socket).expect("owned socket");
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            drop(connection);
        }
    });
    for args in [&["project", "list"][..], &["project", "add", "."]] {
        check(args);
    }
}
