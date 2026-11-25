use crate::{
    units::*,
    units_tests::{CLOCK, SHOW, UNIT_DIR, block, roots, shown},
    *,
};
use std::path::Path;

#[test]
fn a_timer_reports_what_it_triggers_and_a_wall_clock_elapse_only_from_a_utc_timestamp() {
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
fn a_time_span_is_read_exactly_as_systemd_prints_it() {
    // `show` prints NextElapseUSecMonotonic with systemd's format_timespan.
    for (text, us) in [
        ("1w 5d 12min 13.049512s", 1_037_533_049_512_u64),
        ("1month 9min 2.5s", 2_629_800_000_000 + 542_500_000),
        ("1y", 31_557_600_000_000),
        ("2h", 7_200_000_000),
        ("1h 5min 30.500000s", 3_930_500_000),
        ("45s", 45_000_000),
        ("500ms", 500_000),
        ("1.5ms", 1_500),
        ("7us", 7),
        ("7μs", 7),
    ] {
        assert_eq!(timespan_us(text), Some(us), "{text:?}");
    }
    for unknown in [
        "",
        "0",
        "infinity",
        "n/a",
        "5",
        "1h5min",
        "1 h",
        "-1h",
        "1.2.3s",
        ".5s",
        "1.0000001s",
        "1parsec",
        "1H",
        "1h  5min",
        "99999999999999y",
        "1us 1us 1us 1us 1us 1us 1us 1us 1us 1us",
        "Thu 2026-01-01 03:00:00 UTC",
    ] {
        assert_eq!(timespan_us(unknown), None, "{unknown:?}");
    }
}

#[test]
fn a_timer_counting_from_boot_or_its_last_run_gets_a_wall_clock_elapse_from_the_clocks() {
    let timer = |lines: &str, clock| {
        parse_show(
            &block("app-health.timer", lines),
            Path::new(UNIT_DIR),
            &roots(),
            clock,
        )[0]
        .next_elapse_ms
    };
    // OnUnitActiveSec=: no wall-clock elapse, and a point on the monotonic clock.
    let monotonic = "NextElapseUSecRealtime=\nNextElapseUSecMonotonic=1h 5min 30.500000s";
    assert_eq!(
        timer(monotonic, Some(CLOCK)),
        Some(CLOCK.realtime_ms + 330_500),
        "the wall clock plus what is left on the monotonic clock, as list-timers computes it"
    );
    assert_eq!(timer(monotonic, None), None, "no clock, no guess");
    // Already due: the moment it was due.
    assert_eq!(
        timer("NextElapseUSecMonotonic=30min", Some(CLOCK)),
        Some(CLOCK.realtime_ms - 1_800_000)
    );
    // A calendar timer prints 0; an elapsed one prints 0 for both.
    let calendar = "NextElapseUSecRealtime=Thu 2026-01-01 03:00:00 UTC\nNextElapseUSecMonotonic=0";
    assert_eq!(timer(calendar, Some(CLOCK)), Some(1_767_236_400_000));
    assert_eq!(timer(calendar, None), Some(1_767_236_400_000));
    assert_eq!(
        timer(
            "NextElapseUSecRealtime=\nNextElapseUSecMonotonic=0",
            Some(CLOCK)
        ),
        None
    );
    // Both schedules: the earlier one.
    let both = |span: &str| {
        timer(
            &format!(
                "NextElapseUSecRealtime=Thu 2026-01-01 03:00:00 UTC\nNextElapseUSecMonotonic={span}"
            ),
            Some(CLOCK),
        )
    };
    assert_eq!(both("1h 10min"), Some(CLOCK.realtime_ms + 600_000));
    assert_eq!(both("1w"), Some(1_767_236_400_000));
    // Not a date a screen can show.
    assert_eq!(timer("NextElapseUSecMonotonic=300000y", Some(CLOCK)), None);
    assert_eq!(timer("NextElapseUSecMonotonic=infinity", Some(CLOCK)), None);
    // A service has neither.
    let service = parse_show(
        &block("app.service", "NextElapseUSecMonotonic=2h"),
        Path::new(UNIT_DIR),
        &roots(),
        Some(CLOCK),
    );
    assert_eq!(service[0].next_elapse_ms, None);
}

#[test]
fn the_process_clocks_are_readable_and_plausible() {
    let clock = Clock::now().expect("clocks of this process");
    assert!(clock.monotonic_us > 0);
    // After 2020-01-01, in milliseconds.
    assert!(clock.realtime_ms > 1_577_836_800_000);
}

#[test]
fn a_timer_takes_the_project_of_the_listed_unit_it_triggers() {
    let project = |items: &[UserUnitInfo], name: &str| {
        items
            .iter()
            .find(|unit| unit.name == name)
            .expect("fixture value")
            .project_id
            .clone()
    };
    let items = shown(SHOW);
    assert_eq!(
        project(&items, "app-backup.timer").as_deref(),
        Some("app"),
        "app-backup.service runs in the project folder"
    );
    let inside = "WorkingDirectory=/home/u/projects/app";
    let listing = [
        block("app-web.service", inside),
        block("plain.service", "WorkingDirectory=/srv/elsewhere"),
        // A packaged unit is not listed, whatever its directory is.
        format!(
            "Id=packaged.service\nFragmentPath=/usr/lib/systemd/user/packaged.service\n{inside}\n"
        ),
        block("to-project.timer", "Triggers=app-web.service"),
        block("to-plain.timer", "Triggers=plain.service"),
        block("to-packaged.timer", "Triggers=packaged.service"),
        block("to-missing.timer", "Triggers=missing.service"),
        block("to-nothing.timer", "Triggers="),
        // The first triggered unit that is listed with a project.
        block(
            "to-several.timer",
            "Triggers=missing.service plain.service app-web.service",
        ),
        // A timer's own directory is not a schedule's project.
        block(
            "own-directory.timer",
            &format!("{inside}\nTriggers=plain.service"),
        ),
        // Only a timer inherits.
        block("chained.service", "Triggers=app-web.service"),
    ]
    .join("\n");
    let items = shown(&listing);
    assert_eq!(project(&items, "to-project.timer").as_deref(), Some("app"));
    assert_eq!(project(&items, "to-several.timer").as_deref(), Some("app"));
    for none in [
        "to-plain.timer",
        "to-packaged.timer",
        "to-missing.timer",
        "to-nothing.timer",
        "own-directory.timer",
        "chained.service",
        "plain.service",
    ] {
        assert_eq!(project(&items, none), None, "{none}");
    }
    // The shown `triggers` stays the first triggered unit.
    let several = items
        .iter()
        .find(|unit| unit.name == "to-several.timer")
        .expect("fixture value");
    assert_eq!(several.triggers.as_deref(), Some("missing.service"));
    // The order of the blocks does not matter.
    let reversed = [
        block("early.timer", "Triggers=late.service"),
        block("late.service", inside),
    ]
    .join("\n");
    assert_eq!(
        project(&shown(&reversed), "early.timer").as_deref(),
        Some("app")
    );
}
