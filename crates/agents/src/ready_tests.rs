//! When a send may reach a pi launch, decided with explicit times instead of waiting for them.
use super::*;

/// When this daemon started.
const DAEMON: i64 = 1_000_000;

fn launch(id: &str, at_ms: i64) -> Binding {
    let mut binding = Binding::new(at_ms);
    binding.launch_id = Some(id.into());
    binding
}
fn seen(launch: &str, tool_started_ms: Option<i64>, ready: bool) -> LaunchSeen {
    LaunchSeen {
        launch: launch.into(),
        tool_started_ms,
        ready,
    }
}

#[test]
fn a_pi_launch_takes_sends_once_it_says_it_is_ready() {
    let current = launch("L2", DAEMON + 10);
    // Nothing heard from this launch yet.
    assert!(!may_send(&current, None, DAEMON + 11, DAEMON));
    // The previous launch said it was ready, which says nothing about this one.
    let stale = seen("L1", Some(DAEMON + 5), true);
    assert!(!may_send(&current, Some(&stale), DAEMON + 11, DAEMON));
    // This launch's tool started, but pi has not said it reads its input.
    let started = seen("L2", Some(DAEMON + 20), false);
    assert!(!may_send(&current, Some(&started), DAEMON + 21, DAEMON));
    let ready = seen("L2", Some(DAEMON + 20), true);
    assert!(may_send(&current, Some(&ready), DAEMON + 21, DAEMON));
}

#[test]
fn without_a_report_sends_are_taken_30_seconds_after_the_tool_started() {
    let current = launch("L1", DAEMON + 10);
    // Preparation and setup took 45 s, so pi starts long after the launch.
    let started = DAEMON + 10 + 45_000;
    let entry = seen("L1", Some(started), false);
    assert!(!may_send(
        &current,
        Some(&entry),
        DAEMON + 10 + READY_FALLBACK_MS,
        DAEMON
    ));
    assert!(!may_send(
        &current,
        Some(&entry),
        started + READY_FALLBACK_MS - 1,
        DAEMON
    ));
    assert!(may_send(
        &current,
        Some(&entry),
        started + READY_FALLBACK_MS,
        DAEMON
    ));
    // A launch whose tool start was not seen is timed from the launch itself.
    let launched = DAEMON + 10;
    assert!(!may_send(
        &current,
        None,
        launched + READY_FALLBACK_MS - 1,
        DAEMON
    ));
    assert!(may_send(
        &current,
        None,
        launched + READY_FALLBACK_MS,
        DAEMON
    ));
    // Another launch's tool start does not move it.
    let other = seen("L0", Some(started), false);
    assert!(may_send(
        &current,
        Some(&other),
        launched + READY_FALLBACK_MS,
        DAEMON
    ));
}

#[test]
fn a_launch_from_before_this_daemon_counts_as_ready() {
    // Its reports may have come while no daemon took them.
    assert!(may_send(&launch("L1", DAEMON - 1), None, DAEMON, DAEMON));
    assert!(!may_send(&launch("L1", DAEMON), None, DAEMON, DAEMON));
}

#[test]
fn a_report_for_another_launch_replaces_the_tasks_entry() {
    let launches = Launches::default();
    launches.tool_started("a", "L1", 5);
    launches.ready("a", "L1");
    assert_eq!(launches.get("a"), Some(seen("L1", Some(5), true)));
    launches.tool_started("a", "L2", 9);
    assert_eq!(launches.get("a"), Some(seen("L2", Some(9), false)));
    launches.ready("a", "L2");
    assert_eq!(launches.get("a"), Some(seen("L2", Some(9), true)));
    assert_eq!(launches.get("b"), None);
}
