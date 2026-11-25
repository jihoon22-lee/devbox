use crate::{units::*, *};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

const UNIT_DIR: &str = "/home/u/.config/systemd/user";
const HEX: &str = "0123456789abcdef0123456789abcdef";
// `systemctl --user list-units --type=service,timer --all --output=json`: systemd 249 also
// prints a `job` column, and a newer one may add keys.
const LIST: &str = r#"[{"unit":"app-backup.service","load":"loaded","active":"inactive","sub":"dead","description":"Back up the app data"},{"unit":"app-backup.timer","load":"loaded","active":"active","sub":"waiting","job":null,"description":"Nightly app backup"},{"unit":"app-web.service","load":"loaded","active":"active","sub":"running","job":"","description":"App web server"},{"unit":"dbus.service","load":"loaded","active":"active","sub":"running","description":"D-Bus User Message Bus"},{"unit":"devbox-tmux@prod.service","load":"loaded","active":"active","sub":"running","description":"own"},{"unit":"devbox@prod.service","load":"loaded","active":"active","sub":"running","description":"own"},{"unit":"devbox-prod-run-0123456789abcdef0123456789abcdef.service","load":"loaded","active":"active","sub":"running","description":"own run"},{"unit":"mydevbox-tool.service","load":"loaded","active":"failed","sub":"failed","description":"A tool"},{"unit":null},{"unit":"-o.service; rm.mount"},{"unit":"gone.service","load":"not-found","active":"inactive","sub":"dead","description":"gone.service"}]"#;
// `systemctl --user show --property=…` prints one block per unit in the manager's own property
// order and leaves out what a unit type does not have.
const SHOW: &str = "\
Id=app-backup.service
Triggers=
Description=Back up the app data
LoadState=loaded
ActiveState=inactive
SubState=dead
FragmentPath=/home/u/.config/systemd/user/app-backup.service
WorkingDirectory=!/home/u/projects/app

Id=app-backup.timer
Triggers=app-backup.service
Description=Nightly app backup
LoadState=loaded
ActiveState=active
SubState=waiting
FragmentPath=/home/u/.config/systemd/user/app-backup.timer
NextElapseUSecRealtime=Thu 2026-01-01 03:00:00 UTC

Id=app-web.service
Triggers=
Description=App web server
LoadState=loaded
ActiveState=active
SubState=running
FragmentPath=/home/u/.config/systemd/user/app-web.service
WorkingDirectory=/home/u/projects/app/web

Id=dbus.service
Description=D-Bus User Message Bus
LoadState=loaded
ActiveState=active
SubState=running
FragmentPath=/usr/lib/systemd/user/dbus.service
WorkingDirectory=

Id=mydevbox-tool.service
Description=A tool
LoadState=loaded
ActiveState=failed
SubState=failed
FragmentPath=/home/u/.config/systemd/user/mydevbox-tool.service
WorkingDirectory=/home/u/projects/app2

Id=gone.service
Description=gone.service
LoadState=not-found
ActiveState=inactive
SubState=dead
FragmentPath=
";

fn roots() -> Vec<ProjectRoot> {
    vec![ProjectRoot {
        id: "app".into(),
        path: "/home/u/projects/app".into(),
    }]
}
fn shown(text: &str) -> Vec<UserUnitInfo> {
    parse_show(text, Path::new(UNIT_DIR), &roots())
}
/// One block of a unit file in the user's own unit directory.
fn block(id: &str, lines: &str) -> String {
    format!(
        "Id={id}\nLoadState=loaded\nActiveState=active\nSubState=running\nFragmentPath={UNIT_DIR}/{id}\n{lines}\n"
    )
}
fn names(items: &[UserUnitInfo]) -> Vec<&str> {
    items.iter().map(|unit| unit.name.as_str()).collect()
}

#[test]
fn unit_names_follow_the_systemd_grammar_and_only_services_and_timers_count() {
    assert_eq!(unit_kind("app-web.service"), Some(UserUnitKind::Service));
    assert_eq!(unit_kind("app-backup.timer"), Some(UserUnitKind::Timer));
    assert_eq!(
        unit_kind("app@web-1.service"),
        Some(UserUnitKind::Service),
        "a template instance"
    );
    assert_eq!(unit_kind("a:b_c\\x2d.service"), Some(UserUnitKind::Service));
    let long = format!("{}.service", "a".repeat(248));
    assert_eq!(long.len(), 256);
    for refused in [
        "",
        ".service",
        "app.socket",
        "app.scope",
        "app",
        "app web.service",
        "app\n.service",
        "*.service",
        "app;rm.service",
        "앱.service",
        long.as_str(),
    ] {
        assert_eq!(unit_kind(refused), None, "{refused:?}");
    }
}

#[test]
fn devbox_units_of_every_channel_and_its_runs_are_recognized_by_their_naming_scheme() {
    for own in [
        "devbox@prod.service".to_owned(),
        "devbox@prod.socket".into(),
        "devbox-tmux@prod.service".into(),
        "devbox-accept@accept.service".into(),
        "devbox-accept-tmux@accept.service".into(),
        "devbox-dev@dev.service".into(),
        "devbox-dev-tmux@dev.service".into(),
        "devbox@test-ab.service".into(),
        format!("devbox-prod-run-{HEX}.service"),
        format!("devbox-test-ab-run-{HEX}.service"),
        format!("devbox-prod-agent-a{HEX}.scope"),
        format!("devbox-verify-{HEX}.scope"),
    ] {
        assert!(devbox_unit(&own), "{own}");
    }
    // "devbox" in another shape is somebody's own unit.
    for kept in [
        "mydevbox-tool.service".to_owned(),
        "devbox-notes.service".into(),
        "devbox-backup.timer".into(),
        "devbox-tmux.service".into(),
        "devbox-dev-proxy.service".into(),
        "devbox-my-run-1.service".into(),
        "app@devbox.service".into(),
        "mydevbox@prod.service".into(),
        format!("devbox-run-{HEX}.service"),
        format!("devbox-Prod-run-{HEX}.service"),
        format!("devbox-prod-run-{}.service", HEX.to_uppercase()),
        format!("devbox-prod-agent-{HEX}.scope"),
        format!("devbox-verify-{HEX}0.scope"),
    ] {
        assert!(!devbox_unit(&kept), "{kept}");
    }
}

#[test]
fn list_units_json_gives_the_names_to_show_without_devbox_units_and_odd_names() {
    let (names, truncated) = parse_list(LIST.as_bytes()).expect("fixture value");
    assert_eq!(
        names,
        [
            "app-backup.service",
            "app-backup.timer",
            "app-web.service",
            "dbus.service",
            "mydevbox-tool.service",
            "gone.service"
        ]
    );
    assert!(!truncated);
    assert_eq!(
        parse_list(b"[]").expect("no units"),
        (Vec::<String>::new(), false)
    );
    for malformed in [
        &b""[..],
        b"app-web.service loaded active running App",
        b"{\"unit\":\"app-web.service\"}",
        b"[{\"unit\":7}]",
        b"[{\"unit\":\"app-web.service\"}",
        b"\xff",
    ] {
        assert!(parse_list(malformed).is_none(), "{malformed:?}");
    }
}

#[test]
fn list_units_beyond_the_cap_are_cut_and_flagged() {
    let rows = |count: usize| {
        let rows: Vec<_> = (0..count)
            .map(|index| format!(r#"{{"unit":"app-{index}.service"}}"#))
            .collect();
        format!("[{}]", rows.join(","))
    };
    let (names, truncated) = parse_list(rows(MAX_UNITS).as_bytes()).expect("fixture value");
    assert_eq!((names.len(), truncated), (MAX_UNITS, false));
    let (names, truncated) = parse_list(rows(MAX_UNITS + 1).as_bytes()).expect("fixture value");
    assert_eq!((names.len(), truncated), (MAX_UNITS, true));
    assert_eq!(names[0], "app-0.service");
}

#[test]
fn show_keeps_only_unit_files_of_the_users_own_unit_directory() {
    let items = shown(SHOW);
    assert_eq!(
        names(&items),
        [
            "app-backup.service",
            "app-backup.timer",
            "app-web.service",
            "mydevbox-tool.service"
        ],
        "sorted by name; packaged, generated and not-found units are not the user's"
    );
    let web = &items[2];
    assert_eq!(web.kind, UserUnitKind::Service);
    assert_eq!(web.description, "App web server");
    assert_eq!(
        (
            web.load_state.as_str(),
            web.active_state.as_str(),
            web.sub_state.as_str()
        ),
        ("loaded", "active", "running")
    );
    assert_eq!((web.next_elapse_ms, web.triggers.as_deref()), (None, None));

    let other = [
        // A transient unit of `systemd-run --user`.
        "Id=run-r1.service\nLoadState=loaded\nActiveState=active\nSubState=running\nFragmentPath=/run/user/1000/systemd/transient/run-r1.service\n".to_owned(),
        // A subdirectory or a sibling of the unit directory is not the unit directory.
        format!("Id=nested.service\nFragmentPath={UNIT_DIR}/sub/nested.service\n"),
        format!("Id=sibling.service\nFragmentPath={UNIT_DIR}2/sibling.service\n"),
        // A mask is a link to /dev/null; an older manager reports the target as the file.
        "Id=masked.service\nLoadState=masked\nFragmentPath=/dev/null\n".into(),
        "Id=relative.service\nFragmentPath=relative.service\n".into(),
        // A Devbox unit file lives in the same directory and stays out by its name.
        format!("Id=devbox@prod.service\nFragmentPath={UNIT_DIR}/devbox@.service\n"),
        // A template instance is loaded from the template file.
        format!("Id=app@worker.service\nFragmentPath={UNIT_DIR}/app@.service\n"),
        // A linked unit file (`systemctl --user link`, a dotfiles symlink) keeps the link's path.
        block("linked.service", ""),
        "FragmentPath=/home/u/.config/systemd/user/nameless.service\n".into(),
        format!("Id=app.socket\nFragmentPath={UNIT_DIR}/app.socket\n"),
    ]
    .join("\n");
    assert_eq!(
        names(&shown(&other)),
        ["app@worker.service", "linked.service"]
    );
    // A mask the user put in the unit directory is listed as what it is.
    let masked = shown(&format!(
        "Id=app-old.service\nLoadState=masked\nActiveState=inactive\nSubState=dead\nFragmentPath={UNIT_DIR}/app-old.service\n"
    ));
    assert_eq!(
        (masked[0].name.as_str(), masked[0].load_state.as_str()),
        ("app-old.service", "masked")
    );
    // The same unit shown twice (an alias among the arguments) is one row.
    let twice = format!("{}\n{}", block("app.service", ""), block("app.service", ""));
    assert_eq!(names(&shown(&twice)), ["app.service"]);
    assert!(shown("").is_empty());
    assert!(shown("no key value lines\n\n=\n").is_empty());
}

#[test]
fn a_service_takes_the_project_of_its_working_directory() {
    let items = shown(SHOW);
    let project = |name: &str| {
        items
            .iter()
            .find(|unit| unit.name == name)
            .expect("fixture value")
            .project_id
            .as_deref()
    };
    assert_eq!(
        project("app-web.service"),
        Some("app"),
        "inside the project"
    );
    assert_eq!(
        project("app-backup.service"),
        Some("app"),
        "the project folder itself, written as WorkingDirectory=-…"
    );
    assert_eq!(
        project("mydevbox-tool.service"),
        None,
        "a sibling that shares a string prefix"
    );
    assert_eq!(
        project("app-backup.timer"),
        None,
        "a timer has no directory"
    );
    for none in [
        "WorkingDirectory=",
        "WorkingDirectory=~",
        "WorkingDirectory=!~",
        "WorkingDirectory=projects/app",
        "WorkingDirectory=/home/u/projects/app/../other",
        "ExecStart=/home/u/projects/app/bin/serve",
        "",
    ] {
        let items = shown(&block("app.service", none));
        assert_eq!(items[0].project_id, None, "{none:?}");
    }
    let long = format!("WorkingDirectory=/home/u/projects/app/{}", "a".repeat(4096));
    assert_eq!(shown(&block("app.service", &long))[0].project_id, None);
    // A timer never takes a project, whatever the block says.
    let timer = shown(&block("app.timer", "WorkingDirectory=/home/u/projects/app"));
    assert_eq!(timer[0].project_id, None);
}

#[test]
fn a_timer_reports_what_it_triggers_and_a_next_elapse_only_from_a_utc_timestamp() {
    let items = shown(SHOW);
    let timer = &items[1];
    assert_eq!(timer.kind, UserUnitKind::Timer);
    assert_eq!(timer.triggers.as_deref(), Some("app-backup.service"));
    // 2026-01-01T03:00:00Z
    assert_eq!(timer.next_elapse_ms, Some(1_767_236_400_000));
    let next = |value: &str| {
        shown(&block(
            "app.timer",
            &format!("NextElapseUSecRealtime={value}"),
        ))[0]
            .next_elapse_ms
    };
    assert_eq!(next("Thu 1970-01-01 00:00:01 UTC"), Some(1000));
    assert_eq!(
        next("Thu 2026-01-01 03:00:00.123456 UTC"),
        Some(1_767_236_400_123),
        "systemd prints microseconds when asked to"
    );
    for unknown in [
        // A timer of OnBootSec=/OnUnitActiveSec= has no wall-clock elapse.
        "",
        "n/a",
        "0",
        // Another zone cannot be converted without its rules.
        "Thu 2026-01-01 12:00:00 KST",
        "Thu 2026-01-01 03:00:00",
        "2026-01-01 03:00:00 UTC",
        "Thu 2026-13-01 03:00:00 UTC",
        "Thu 2026-01-01 25:00:00 UTC",
        "Thu 1969-12-31 23:59:59 UTC",
        "@1767236400",
        "Thu 2026-01-01 03:00:00 UTC extra",
    ] {
        assert_eq!(next(unknown), None, "{unknown:?}");
    }
    let triggers = |value: &str| {
        shown(&block("app.timer", &format!("Triggers={value}")))[0]
            .triggers
            .clone()
    };
    assert_eq!(triggers("").as_deref(), None);
    assert_eq!(
        triggers("app.service other.service").as_deref(),
        Some("app.service")
    );
    assert_eq!(triggers("not\u{1b}a-unit").as_deref(), None);
    // A service reports neither.
    let service = shown(&block(
        "app.service",
        "Triggers=x.service\nNextElapseUSecRealtime=Thu 2026-01-01 03:00:00 UTC",
    ));
    assert_eq!(
        (service[0].next_elapse_ms, service[0].triggers.as_deref()),
        (None, None)
    );
}

#[test]
fn untrusted_text_is_bounded_and_control_characters_do_not_pass() {
    let text = format!(
        "Description=  Serve\u{1b}[31m the\tapp\u{7f} {}\nSubState={}\nActiveState=act\u{1b}ive",
        "가".repeat(400),
        "s".repeat(100)
    );
    let unit = &shown(&format!(
        "Id=app.service\nFragmentPath={UNIT_DIR}/app.service\n{text}\n"
    ))[0];
    assert!(unit.description.starts_with("Serve [31m the app  가"));
    assert_eq!(unit.description.chars().count(), MAX_DESCRIPTION_CHARS);
    assert!(!unit.description.chars().any(char::is_control));
    assert_eq!(unit.sub_state.len(), MAX_STATE_CHARS);
    assert_eq!(unit.active_state, "act ive");
    assert_eq!(
        unit.load_state, "",
        "a missing state is empty, not invented"
    );
}

#[test]
fn units_wire_shape_leaves_out_what_is_not_known() {
    let result = UnitsResult {
        available: true,
        reason: None,
        items: shown(SHOW),
        truncated: false,
    };
    let json = serde_json::to_value(&result).expect("fixture value");
    assert_eq!(
        json["items"][2],
        serde_json::json!({
            "name": "app-web.service",
            "description": "App web server",
            "loadState": "loaded",
            "activeState": "active",
            "subState": "running",
            "kind": "service",
            "projectId": "app",
        })
    );
    assert_eq!(json["items"][1]["kind"], "timer");
    assert_eq!(json["items"][1]["nextElapseMs"], 1_767_236_400_000_u64);
    assert_eq!(json["items"][1]["triggers"], "app-backup.service");
    assert!(json.get("reason").is_none());
    let unavailable = serde_json::to_value(unavailable(UnitsUnavailableReason::ManagerUnavailable))
        .expect("fixture value");
    assert_eq!(
        unavailable,
        serde_json::json!({"available": false, "reason": "managerUnavailable", "items": [], "truncated": false})
    );
}

/// A fake `systemctl`: never the real one, so no test can reach this machine's units.
fn fake(dir: &Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("systemctl");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("fixture result");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("fixture result");
    path
}
async fn listed(program: &Path) -> R<UnitsResult> {
    snapshot(
        program,
        Duration::from_secs(5),
        Path::new(UNIT_DIR),
        &roots(),
    )
    .await
}

#[tokio::test]
async fn fake_systemctl_gets_exactly_two_read_only_calls_in_the_login_environment() {
    let dir = tempfile::tempdir().expect("fixture result");
    std::fs::write(dir.path().join("list"), LIST).expect("fixture result");
    std::fs::write(dir.path().join("show"), SHOW).expect("fixture result");
    let here = dir.path().display();
    // Any other argv — a verb that changes a unit included — fails the listing.
    let program = fake(
        dir.path(),
        &format!(
            r#"echo "$*" >> '{here}/calls'
[ "$TZ" = UTC ] && [ "$LC_ALL" = C ] && [ "$XDG_RUNTIME_DIR" = "/run/user/$(id -u)" ] || exit 8
case "$*" in
  '--user list-units --type=service,timer --all --no-pager --output=json') cat '{here}/list' ;;
  '--user show --no-pager --property=Id,Description,LoadState,ActiveState,SubState,FragmentPath,WorkingDirectory,NextElapseUSecRealtime,Triggers -- app-backup.service app-backup.timer app-web.service dbus.service mydevbox-tool.service gone.service') cat '{here}/show' ;;
  *) exit 9 ;;
esac"#
        ),
    );
    let result = listed(&program).await.expect("listing");
    assert!(result.available && result.reason.is_none() && !result.truncated);
    assert_eq!(
        names(&result.items),
        [
            "app-backup.service",
            "app-backup.timer",
            "app-web.service",
            "mydevbox-tool.service"
        ]
    );
    assert_eq!(result.items[2].project_id.as_deref(), Some("app"));
    let calls = std::fs::read_to_string(dir.path().join("calls")).expect("fixture result");
    assert_eq!(calls.lines().count(), 2, "{calls}");
}

#[tokio::test]
async fn nothing_to_show_is_one_call_and_an_empty_list() {
    let dir = tempfile::tempdir().expect("fixture result");
    let program = fake(
        dir.path(),
        r#"case "$2" in list-units) printf '%s' '[{"unit":"devbox@prod.service"}]' ;; *) exit 9 ;; esac"#,
    );
    let result = listed(&program).await.expect("listing");
    assert!(result.available && result.items.is_empty() && !result.truncated);
}

#[tokio::test]
async fn a_missing_systemctl_and_an_unreachable_manager_are_unavailable_with_a_reason() {
    let dir = tempfile::tempdir().expect("fixture result");
    let missing = listed(&dir.path().join("missing")).await.expect("typed");
    assert!(!missing.available && missing.items.is_empty());
    assert_eq!(missing.reason, Some(UnitsUnavailableReason::CliMissing));
    // `systemctl --user` without a user manager: a message on stderr and a failure status.
    let program = fake(
        dir.path(),
        "echo 'Failed to connect to bus: No such file or directory' >&2; exit 1",
    );
    let unreachable = listed(&program).await.expect("typed");
    assert!(!unreachable.available && unreachable.items.is_empty());
    assert_eq!(
        unreachable.reason,
        Some(UnitsUnavailableReason::ManagerUnavailable)
    );
}

fn code(result: R<UnitsResult>) -> ObserveError {
    match result {
        Err(devbox_protocol::Fail::Domain(error)) => error,
        other => panic!("expected a typed error, got {other:?}"),
    }
}

#[tokio::test]
async fn malformed_oversized_and_stalled_output_are_typed_errors() {
    let dir = tempfile::tempdir().expect("fixture result");
    let malformed = fake(dir.path(), "echo 'UNIT LOAD ACTIVE SUB DESCRIPTION'");
    assert!(matches!(
        code(listed(&malformed).await),
        ObserveError::SourceUnavailable
    ));
    let oversized = fake(
        dir.path(),
        &format!("head -c {} /dev/zero | tr '\\0' ' '", MAX_OUTPUT + 1),
    );
    assert!(matches!(
        code(listed(&oversized).await),
        ObserveError::LimitExceeded
    ));
    // The second call fails although the first one answered.
    let show_fails = fake(
        dir.path(),
        r#"case "$2" in list-units) printf '%s' '[{"unit":"app.service"}]' ;; *) exit 1 ;; esac"#,
    );
    assert!(matches!(
        code(listed(&show_fails).await),
        ObserveError::SourceUnavailable
    ));
    let stalled = fake(dir.path(), "exec sleep 30");
    let started = std::time::Instant::now();
    let result = snapshot(
        &stalled,
        Duration::from_millis(100),
        Path::new(UNIT_DIR),
        &roots(),
    )
    .await;
    assert!(matches!(code(result), ObserveError::SourceUnavailable));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the child is killed, not awaited"
    );
}

#[tokio::test]
async fn units_wait_for_a_host_slot_so_polling_cannot_pile_up_children() {
    let dir = tempfile::tempdir().expect("fixture result");
    let calls = dir.path().join("calls");
    let program = fake(
        dir.path(),
        &format!("echo call >> '{}'; printf '[]'", calls.display()),
    );
    let service = ObserveService::default().with_systemctl(program);
    let listing = || service.units(roots(), UNIT_DIR.into());
    // Both slots of the host queries are taken: nothing is spawned while waiting.
    let held = service
        .host_slots
        .clone()
        .acquire_many_owned(2)
        .await
        .expect("fixture result");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), listing())
            .await
            .is_err()
    );
    assert!(!calls.exists());
    drop(held);
    assert!(listing().await.expect("listing").available);
    assert!(calls.exists());
    assert_eq!(service.host_slots.available_permits(), 2);
}
