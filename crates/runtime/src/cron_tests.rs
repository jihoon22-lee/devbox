use super::*;
use chrono_tz::America::New_York;

fn naive(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("fixture value")
        .and_hms_opt(hour, minute, second)
        .expect("fixture value")
}

#[test]
fn optional_seconds_are_normalized_and_generated() {
    let schedule = CronSchedule::parse("*/15 * * * *").expect("fixture result");
    assert_eq!(schedule.normalized_expression(), "0 */15 * * * *");
    let result = schedule
        .next_wall_occurrences(naive(2026, 1, 1, 0, 0, 0), 4)
        .expect("fixture result");
    assert_eq!(
        result,
        vec![
            naive(2026, 1, 1, 0, 15, 0),
            naive(2026, 1, 1, 0, 30, 0),
            naive(2026, 1, 1, 0, 45, 0),
            naive(2026, 1, 1, 1, 0, 0),
        ]
    );

    let with_seconds = CronSchedule::parse("30 */15 * * * *").expect("fixture result");
    assert_eq!(
        with_seconds
            .next_wall_occurrences(naive(2026, 1, 1, 0, 0, 0), 1)
            .expect("fixture result"),
        vec![naive(2026, 1, 1, 0, 0, 30)]
    );
}

#[test]
fn dom_and_dow_use_posix_or_semantics() {
    let schedule = CronSchedule::parse("0 0 13 * 5").expect("fixture result");
    let result = schedule
        .next_wall_occurrences(naive(2024, 6, 12, 23, 59, 59), 4)
        .expect("fixture result");
    assert_eq!(
        result,
        vec![
            naive(2024, 6, 13, 0, 0, 0),
            naive(2024, 6, 14, 0, 0, 0),
            naive(2024, 6, 21, 0, 0, 0),
            naive(2024, 6, 28, 0, 0, 0),
        ]
    );
}

#[test]
fn multiple_daily_times_are_in_wall_order() {
    let schedule = CronSchedule::parse("0,30 9,17 * * *").expect("fixture result");
    let result = schedule
        .next_wall_occurrences(naive(2026, 8, 12, 9, 0, 0), 5)
        .expect("fixture result");
    assert_eq!(
        result,
        vec![
            naive(2026, 8, 12, 9, 30, 0),
            naive(2026, 8, 12, 17, 0, 0),
            naive(2026, 8, 12, 17, 30, 0),
            naive(2026, 8, 13, 9, 0, 0),
            naive(2026, 8, 13, 9, 30, 0),
        ]
    );
}

#[test]
fn invalid_expressions_and_macros_are_rejected() {
    for expression in [
        "",
        "@reboot",
        "@daily",
        "0 0 0 1 1 2026 1",
        "0 25 * * *",
        "not a cron expression",
    ] {
        assert!(
            validate_cron(expression).is_err(),
            "accepted {expression:?}"
        );
    }
    assert!(validate_cron("0 0 13 * +5").is_err());
}

#[test]
fn wall_boundary_is_strictly_after_reference() {
    let schedule = CronSchedule::parse("0 * * * *").expect("fixture result");
    let result = schedule
        .next_wall_occurrences(naive(2026, 8, 12, 9, 0, 0), 2)
        .expect("fixture result");
    assert_eq!(
        result,
        vec![naive(2026, 8, 12, 10, 0, 0), naive(2026, 8, 12, 11, 0, 0),]
    );
}

#[test]
fn dst_gap_is_kept_by_generator_but_skipped_by_resolver() {
    let schedule = CronSchedule::parse("30 2 * * *").expect("fixture result");
    let after = New_York
        .with_ymd_and_hms(2024, 3, 9, 0, 0, 0)
        .single()
        .expect("fixture value");
    let walls = schedule
        .next_wall_occurrences(after.naive_local(), 3)
        .expect("fixture result");
    assert_eq!(
        walls,
        vec![
            naive(2024, 3, 9, 2, 30, 0),
            naive(2024, 3, 10, 2, 30, 0),
            naive(2024, 3, 11, 2, 30, 0),
        ]
    );

    let occurrences = schedule.next_occurrences(after, 2).expect("fixture result");
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| occurrence.wall_time)
            .collect::<Vec<_>>(),
        vec![naive(2024, 3, 9, 2, 30, 0), naive(2024, 3, 11, 2, 30, 0)]
    );
    assert_eq!(occurrences[0].wall_key, "2024-03-09T02:30:00");
    assert_eq!(occurrences[1].wall_key, "2024-03-11T02:30:00");
}

#[test]
fn dst_overlap_uses_earliest_offset_and_one_wall_key() {
    let schedule = CronSchedule::parse("30 1 * * *").expect("fixture result");
    let after = New_York
        .with_ymd_and_hms(2024, 11, 2, 0, 0, 0)
        .single()
        .expect("fixture value");
    let occurrences = schedule.next_occurrences(after, 3).expect("fixture result");
    assert_eq!(occurrences.len(), 3);
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| occurrence.wall_time)
            .collect::<Vec<_>>(),
        vec![
            naive(2024, 11, 2, 1, 30, 0),
            naive(2024, 11, 3, 1, 30, 0),
            naive(2024, 11, 4, 1, 30, 0),
        ]
    );
    assert_eq!(occurrences[1].datetime.format("%:z").to_string(), "-04:00");
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| occurrence.wall_key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "2024-11-02T01:30:00",
            "2024-11-03T01:30:00",
            "2024-11-04T01:30:00"
        ]
    );

    let second_overlap = New_York
        .from_local_datetime(&naive(2024, 11, 3, 1, 30, 0))
        .latest()
        .expect("fixture value");
    let resolved =
        resolve_wall_time(&naive(2024, 11, 3, 1, 30, 0), &New_York).expect("fixture value");
    assert!(resolved < second_overlap);
}

#[test]
fn local_preview_uses_default_count() {
    let schedule = CronSchedule::parse("0 0 * * *").expect("fixture result");
    let after = Local
        .with_ymd_and_hms(2026, 8, 12, 12, 0, 0)
        .single()
        .expect("fixture value");
    assert_eq!(
        schedule.preview_local(after).expect("fixture result").len(),
        DEFAULT_OCCURRENCE_COUNT
    );
    assert_eq!(
        next_occurrences("0 0 * * *", after, 1)
            .expect("fixture result")
            .first()
            .expect("fixture value")
            .wall_time,
        naive(2026, 8, 13, 0, 0, 0)
    );
}

#[test]
fn ambiguous_resolution_is_stable_when_after_is_before_overlap() {
    let wall = naive(2024, 11, 3, 1, 30, 0);
    let first = resolve_wall_time(&wall, &New_York).expect("fixture value");
    let second = resolve_wall_time(&wall, &New_York).expect("fixture value");
    assert_eq!(first, second);
    let latest = New_York
        .from_local_datetime(&wall)
        .latest()
        .expect("fixture value");
    assert!(first < latest);
    assert_eq!(first.offset().to_string(), "EDT");
}

#[test]
fn canonical_overlap_occurrence_is_not_returned_after_later_offset() {
    let schedule = CronSchedule::parse("30 1 * * *").expect("fixture result");
    let after = New_York
        .from_local_datetime(&naive(2024, 11, 3, 1, 15, 0))
        .latest()
        .expect("fixture value");
    let occurrence = schedule.next_occurrences(after, 1).expect("fixture result")[0].clone();
    assert_eq!(occurrence.wall_time, naive(2024, 11, 4, 1, 30, 0));
}

#[test]
fn next_after_returns_exclusive_utc_instant_for_named_timezone() {
    let after = New_York
        .with_ymd_and_hms(2026, 1, 1, 9, 0, 0)
        .single()
        .expect("fixture value")
        .with_timezone(&chrono::Utc);
    assert_eq!(
        next_after("0 9 * * *", &New_York, after).expect("fixture result"),
        Some(chrono::Utc.with_ymd_and_hms(2026, 1, 2, 14, 0, 0).unwrap())
    );
}

#[test]
fn next_after_accepts_local_timezone_and_preserves_parse_errors() {
    let after = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    assert_eq!(
        next_after("* * * * * *", &Local, after).expect("fixture result"),
        Some(after + chrono::Duration::seconds(1))
    );
    assert!(next_after("@daily", &Local, after).is_err());
}

#[test]
fn next_after_skips_gap_and_does_not_repeat_overlap() {
    let before_gap = chrono::Utc.with_ymd_and_hms(2024, 3, 10, 5, 0, 0).unwrap();
    assert_eq!(
        next_after("30 2 * * *", &New_York, before_gap).expect("fixture result"),
        Some(chrono::Utc.with_ymd_and_hms(2024, 3, 11, 6, 30, 0).unwrap())
    );
    let later_overlap = chrono::Utc.with_ymd_and_hms(2024, 11, 3, 6, 15, 0).unwrap();
    assert_eq!(
        next_after("30 1 * * *", &New_York, later_overlap).expect("fixture result"),
        Some(chrono::Utc.with_ymd_and_hms(2024, 11, 4, 6, 30, 0).unwrap())
    );
}

#[test]
fn next_after_reports_exhausted_calendar_as_none() {
    let after = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    assert_eq!(
        next_after("0 0 30 2 *", &chrono::Utc, after).expect("fixture result"),
        None
    );
}
