use crate::fixture::*;
use crate::model::attribution::ProjectMatch;
use crate::model::privacy::{InvalidRule, PrivacyRules, RuleField, RuleProblem};
use crate::*;
use devbox_protocol::Fail;
use std::time::Duration;

fn rules(redact: &[&str]) -> SetRules {
    SetRules {
        rules: PrivacyRules {
            redact_title_patterns: redact.iter().map(|s| s.to_string()).collect(),
            ..PrivacyRules::default()
        },
        idle_threshold_ms: 300_000,
    }
}

/// AU-K2 through the daemon path: the idle tail after the last input is not activity.
#[tokio::test]
async fn ingest_with_idle_tail_stores_end_at_last_input() -> T {
    let a = fixture();
    for (at, title) in [
        (900_000, "first"),
        (1_000_000, "first"),
        (1_100_000, "tail1"),
        (1_200_000, "tail2"),
        (1_298_000, "tail2"),
    ] {
        let last = if at <= 1_000_000 { at } else { 1_000_000 };
        a.ingest(a.fg("owned", title, at, last))
            .await
            .map_err(fail)?;
    }
    a.ingest(a.idle(1_300_000, 1_000_000)).await.map_err(fail)?;
    a.flush().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].start_ms, rows[0].end_ms), (900_000, 1_000_000));
    assert_eq!(rows[0].title, "first");
    Ok(())
}

/// `input` extends the open session (same app and title), whatever it carries.
#[tokio::test]
async fn input_heartbeat_extends_the_open_session() -> T {
    let a = fixture();
    a.ingest(a.fg("code.exe", "main.rs", 1_000, 1_000))
        .await
        .map_err(fail)?;
    for at in [31_000, 61_000] {
        a.ingest(a.input("code.exe", at)).await.map_err(fail)?;
    }
    a.ingest(a.fg("chrome.exe", "docs", 70_000, 70_000))
        .await
        .map_err(fail)?;
    a.flush().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].title.as_str(), rows[0].start_ms, rows[0].end_ms),
        ("main.rs", 1_000, 70_000)
    );
    Ok(())
}

#[tokio::test]
async fn ingest_reapplies_rules_server_side() -> T {
    let a = fixture();
    a.svc.set_rules(rules(&["secret"])).await.map_err(fail)?;
    a.ingest(a.fg("code.exe", "secret doc", 1_000, 1_000))
        .await
        .map_err(fail)?;
    a.ingest(a.fg("code.exe", "other", 2_000, 2_000))
        .await
        .map_err(fail)?;
    a.flush().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "[redacted] doc");
    Ok(())
}

#[tokio::test]
async fn broken_rules_store_no_titles() -> T {
    for broken in [
        "excluded_title_patterns = ['(unclosed']",
        "redact_title_patterns = 'not a list'",
        "unknown_key = 1",
    ] {
        let a = fixture_with(broken);
        *a.projects.lock().expect("projects") = vec![ProjectMatch {
            project_id: "p1".into(),
            basenames: vec!["devbox".into()],
        }];
        a.ingest(a.fg("code.exe", "devbox — private.md", 1_000, 1_000))
            .await
            .map_err(fail)?;
        a.ingest(a.pause(2_000)).await.map_err(fail)?;
        a.flush().await;
        let rows = a.sessions().await;
        assert_eq!(rows.len(), 1, "{broken}");
        assert_eq!(rows[0].app, "code.exe");
        assert_eq!(rows[0].title, "", "{broken}: fail-closed stores no title");
        assert_eq!(
            rows[0].project_id, None,
            "no title, so nothing to attribute"
        );
        // The app receives rules that compile to "no titles" or do not compile.
        let view = a.svc.rules(NoParams {}).await.map_err(fail)?;
        let compiled = model::privacy::CompiledRules::compile(&view.rules);
        assert!(
            compiled.is_err() || view.rules.mask_all_titles,
            "{broken}: {view:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn invalid_rule_regex_is_rejected_with_position() -> T {
    let a = fixture();
    let text = "# owned comment\n[pc]\nname = \"x\"\n";
    std::fs::write(a.config(), text)?;
    let mut bad = rules(&[]);
    bad.rules.excluded_title_patterns = vec!["ok".into(), "(unclosed".into()];
    let Err(Fail::Domain(error)) = a.svc.set_rules(bad).await else {
        panic!("broken rules must be refused");
    };
    assert_eq!(
        error,
        ActivityError::InvalidRules {
            problems: vec![InvalidRule {
                field: RuleField::ExcludedTitlePatterns,
                index: 1,
                problem: RuleProblem::Syntax,
            }]
        }
    );
    assert_eq!(
        serde_json::to_value(&error)?,
        serde_json::json!({"code": "activity.invalid_rules", "detail": {"problems": [
            {"field": "excludedTitlePatterns", "index": 1, "problem": "syntax"}
        ]}})
    );
    assert_eq!(
        std::fs::read_to_string(a.config())?,
        text,
        "config untouched"
    );
    let mut idle = rules(&[]);
    idle.idle_threshold_ms = 1_000;
    assert!(matches!(
        a.svc.set_rules(idle).await,
        Err(Fail::Domain(ActivityError::InvalidInput))
    ));
    assert_eq!(std::fs::read_to_string(a.config())?, text);
    Ok(())
}

fn hold_lock(dir: &std::path::Path) -> rustix::fd::OwnedFd {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    let fd = rustix::fs::open(
        dir.join(".config.toml.lock"),
        OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .expect("test lock file");
    rustix::fs::flock(&fd, FlockOperation::LockExclusive).expect("test holds the lock");
    fd
}

#[tokio::test]
async fn rules_write_waits_for_the_config_lock_and_keeps_other_sections() -> T {
    let a = fixture();
    std::fs::write(a.config(), "# top comment\n[pc]\nname = \"x\" # keep me\n")?;
    let held = hold_lock(&a.config_dir);
    let svc = a.svc.clone();
    let write = tokio::spawn(async move { svc.set_rules(rules(&["secret"])).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!write.is_finished(), "set_rules waits for the config lock");
    let mut text = std::fs::read_to_string(a.config())?;
    text.push_str("[secrets]\nnames = [\"A\"]\n");
    std::fs::write(a.config(), &text)?;
    drop(held);
    tokio::time::timeout(Duration::from_secs(5), write)
        .await??
        .map_err(fail)?;
    let after = std::fs::read_to_string(a.config())?;
    for kept in ["# top comment", "# keep me", "[secrets]", "names = [\"A\"]"] {
        assert!(after.contains(kept), "{kept} kept in {after}");
    }
    let table: toml::Table = after.parse()?;
    assert_eq!(
        table["activity"]["redact_title_patterns"],
        toml::Value::Array(vec!["secret".into()])
    );
    assert_eq!(
        table["activity"]["idle_threshold_ms"],
        toml::Value::Integer(300_000)
    );
    // The settings parser reads back what set_rules wrote.
    let back = ActivitySettings::from_table(table["activity"].as_table().expect("table"));
    assert_eq!(back.rules, rules(&["secret"]).rules);
    // A symlinked config.toml is refused and its target left alone.
    let target = a.config_dir.join("elsewhere.toml");
    std::fs::write(&target, "# not ours\n")?;
    std::fs::remove_file(a.config())?;
    std::os::unix::fs::symlink(&target, a.config())?;
    assert!(matches!(
        a.svc.set_rules(rules(&["x"])).await,
        Err(Fail::Domain(ActivityError::ConfigUnavailable))
    ));
    assert_eq!(std::fs::read_to_string(&target)?, "# not ours\n");
    assert!(
        std::fs::symlink_metadata(a.config())?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

/// Same batch again after a restart (its `upto` forgotten): same session ids, no duplicates.
#[tokio::test]
async fn replayed_batch_after_restart_keeps_session_ids() -> T {
    let mut a = fixture();
    let batch = vec![
        a.fg("a.exe", "one", 1_000, 1_000),
        a.fg("b.exe", "two", 2_000, 2_000),
        a.fg("c.exe", "three", 3_000, 3_000),
    ];
    a.send(batch.clone()).await.map_err(fail)?;
    assert_eq!(a.send(batch.clone()).await.map_err(fail)?.accepted, 0);
    a.flush().await;
    let first = a.sessions().await;
    assert_eq!(first.len(), 2);
    a.restart();
    assert_eq!(a.send(batch).await.map_err(fail)?.accepted, 3);
    a.flush().await;
    assert_eq!(a.sessions().await, first);
    Ok(())
}

#[tokio::test]
async fn day_sums_clipped_sessions_by_app_project_title_and_hour() -> T {
    use chrono::TimeZone;
    let a = fixture();
    *a.projects.lock().expect("projects") = vec![ProjectMatch {
        project_id: "p1".into(),
        basenames: vec!["devbox".into()],
    }];
    let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).expect("date");
    let at = |h: u32, m: u32| {
        chrono::Local
            .from_local_datetime(&date.and_hms_opt(h, m, 0).expect("time"))
            .earliest()
            .expect("local time")
            .timestamp_millis()
    };
    let before = at(0, 0) - 30 * 60_000;
    a.ingest(a.fg("code.exe", "devbox — a.rs", before, before))
        .await
        .map_err(fail)?;
    a.ingest(a.fg("chrome.exe", "news", at(0, 30), at(0, 30)))
        .await
        .map_err(fail)?;
    a.ingest(a.fg("chrome.exe", "news", at(1, 30), at(1, 30)))
        .await
        .map_err(fail)?;
    a.ingest(a.pause(at(1, 30))).await.map_err(fail)?;
    a.flush().await;
    let day = a
        .svc
        .day(DayQuery {
            date: "2026-03-10".into(),
        })
        .await
        .map_err(fail)?;
    let (half, hour) = (30 * 60_000, 60 * 60_000);
    assert_eq!(day.total_ms, half + hour);
    assert_eq!(
        day.apps,
        vec![
            AppMs {
                app: "chrome.exe".into(),
                ms: hour
            },
            AppMs {
                app: "code.exe".into(),
                ms: half
            }
        ]
    );
    assert_eq!(
        day.projects,
        vec![ProjectMs {
            project_id: "p1".into(),
            ms: half
        }]
    );
    assert_eq!(day.titles[0].title, "news");
    assert_eq!(day.hours.len(), 24);
    assert_eq!((day.hours[0].ms, day.hours[1].ms), (hour, half));
    let range = a
        .svc
        .range(RangeQuery {
            from: "2026-03-09".into(),
            to: "2026-03-10".into(),
        })
        .await
        .map_err(fail)?;
    assert_eq!(
        range.days.iter().map(|d| d.total_ms).collect::<Vec<_>>(),
        [half, half + hour]
    );
    for (from, to) in [("2026-03-10", "2026-03-09"), ("2026-01-01", "2026-03-10")] {
        assert!(matches!(
            a.svc
                .range(RangeQuery {
                    from: from.into(),
                    to: to.into()
                })
                .await,
            Err(Fail::Domain(ActivityError::InvalidRange))
        ));
    }
    let out = a
        .svc
        .export(ExportQuery {
            from: "2026-03-10".into(),
            to: "2026-03-10".into(),
            format: ExportFormat::Csv,
        })
        .await
        .map_err(fail)?;
    assert_eq!(
        out.path,
        a.exports_dir
            .join("activity-2026-03-10-2026-03-10.csv")
            .display()
            .to_string()
    );
    let csv = std::fs::read_to_string(&out.path)?;
    let mut lines = csv.lines();
    assert_eq!(lines.next(), Some("start,end,app,title,project"));
    assert_eq!(lines.count(), 2);
    assert!(csv.contains("\"devbox — a.rs\",\"p1\""));
    Ok(())
}

/// Spreadsheet formulas in titles are neutralized; exports cover at most 366 days.
#[tokio::test]
async fn export_guards_formulas_and_caps_the_range_at_366_days() -> T {
    let a = fixture();
    let start = 1_773_100_800_000; // 2026-03-10 (any zone: the range below covers it)
    a.send(vec![
        a.fg("x.exe", "=HYPERLINK(\"http://x\")", start, start),
        a.fg("y.exe", "@SUM(1)", start + 1_000, start + 1_000),
        a.pause(start + 2_000),
    ])
    .await
    .map_err(fail)?;
    a.flush().await;
    let query = |from: &str, to: &str| ExportQuery {
        from: from.into(),
        to: to.into(),
        format: ExportFormat::Csv,
    };
    let out = a
        .svc
        .export(query("2026-03-09", "2026-03-11"))
        .await
        .map_err(fail)?;
    let csv = std::fs::read_to_string(&out.path)?;
    assert!(csv.contains("\"'=HYPERLINK(\"\"http://x\"\")\""), "{csv}");
    assert!(csv.contains("\"'@SUM(1)\""), "{csv}");
    assert!(!csv.contains(",\"=") && !csv.contains(",\"@"), "{csv}");
    a.svc
        .export(query("2025-03-11", "2026-03-11"))
        .await
        .map_err(fail)?; // 366 days
    assert!(matches!(
        a.svc.export(query("2025-03-10", "2026-03-11")).await, // 367 days
        Err(Fail::Domain(ActivityError::InvalidRange))
    ));
    Ok(())
}

/// A symlinked lock file is refused like a symlinked `config.toml`.
#[tokio::test]
async fn symlinked_config_lock_is_refused() -> T {
    let a = fixture();
    let target = a.config_dir.join("elsewhere.lock");
    std::fs::write(&target, "")?;
    std::os::unix::fs::symlink(&target, a.config_dir.join(".config.toml.lock"))?;
    assert!(matches!(
        a.svc.set_rules(rules(&["x"])).await,
        Err(Fail::Domain(ActivityError::ConfigUnavailable))
    ));
    assert!(!a.config().exists());
    Ok(())
}

pub fn fail(e: Fail<ActivityError>) -> Box<dyn std::error::Error> {
    format!("{e:?}").into()
}
