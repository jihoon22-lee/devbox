//! Day and range summaries and file exports, read from stored sessions. Days are
//! local calendar days (DST days are 23 or 25 hours long); sessions are clipped.
use crate::store::{Session, overlapping};
use crate::*;
use chrono::{Local, NaiveDate, TimeZone, Timelike};
use devbox_core::Db;
use devbox_protocol::Internal;
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

fn parse_date(text: &str) -> Result<NaiveDate, ActivityError> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| ActivityError::InvalidRange)
}

/// Epoch ms of local midnight starting `date` (the first valid instant if a DST
/// gap swallows midnight).
fn midnight(date: NaiveDate) -> Result<i64, ActivityError> {
    let start = date
        .and_hms_opt(0, 0, 0)
        .ok_or(ActivityError::InvalidRange)?;
    (0..=3)
        .find_map(|h| {
            Local
                .from_local_datetime(&(start + chrono::Duration::hours(h)))
                .earliest()
        })
        .map(|t| t.timestamp_millis())
        .ok_or(ActivityError::InvalidRange)
}

/// `[start, end)` of each date from `from` to `to` inclusive, at most `max` days.
fn days(from: &str, to: &str, max: i64) -> Result<Vec<(NaiveDate, i64, i64)>, ActivityError> {
    let (from, to) = (parse_date(from)?, parse_date(to)?);
    let span = (to - from).num_days();
    if !(0..max).contains(&span) {
        return Err(ActivityError::InvalidRange);
    }
    from.iter_days()
        .take(span as usize + 1)
        .map(|d| {
            let next = d.succ_opt().ok_or(ActivityError::InvalidRange)?;
            Ok((d, midnight(d)?, midnight(next)?))
        })
        .collect()
}

fn clip(s: &Session, from: i64, to: i64) -> i64 {
    (s.end_ms.min(to) - s.start_ms.max(from)).max(0)
}

fn sorted<K: Clone + Ord>(map: HashMap<K, i64>) -> Vec<(K, i64)> {
    let mut items: Vec<(K, i64)> = map.into_iter().filter(|(_, ms)| *ms > 0).collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
}

#[derive(Default)]
struct Totals {
    total: i64,
    apps: HashMap<String, i64>,
    projects: HashMap<String, i64>,
}
impl Totals {
    fn add(&mut self, s: &Session, ms: i64) {
        self.total += ms;
        *self.apps.entry(s.app.clone()).or_default() += ms;
        if let Some(p) = &s.project_id {
            *self.projects.entry(p.clone()).or_default() += ms;
        }
    }
    fn apps(&mut self) -> Vec<AppMs> {
        sorted(std::mem::take(&mut self.apps))
            .into_iter()
            .map(|(app, ms)| AppMs { app, ms })
            .collect()
    }
    fn projects(&mut self) -> Vec<ProjectMs> {
        sorted(std::mem::take(&mut self.projects))
            .into_iter()
            .map(|(project_id, ms)| ProjectMs { project_id, ms })
            .collect()
    }
}

/// Splits `[from, to)` at local hour boundaries into the 24 hour slots.
fn add_hours(hours: &mut [i64; 24], from: i64, to: i64) {
    let mut t = from;
    while t < to {
        let Some(local) = Local.timestamp_millis_opt(t).earliest() else {
            return;
        };
        let into = i64::from(local.minute()) * 60_000
            + i64::from(local.second()) * 1_000
            + i64::from(local.timestamp_subsec_millis());
        let next = (t - into + 3_600_000).min(to).max(t + 1);
        hours[local.hour() as usize] += next - t;
        t = next;
    }
}

pub(crate) async fn day(db: &Db, p: DayQuery) -> R<DayActivity> {
    let [(_, from, to)] = days(&p.date, &p.date, 1)?[..] else {
        return Err(ActivityError::InvalidRange.into());
    };
    let rev = db.rev();
    let sessions = overlapping(db, from, to).await.internal("read activity")?;
    let mut totals = Totals::default();
    let mut titles: HashMap<(String, String), i64> = HashMap::new();
    let mut hours = [0i64; 24];
    for s in &sessions {
        let ms = clip(s, from, to);
        totals.add(s, ms);
        if !s.title.is_empty() {
            *titles.entry((s.app.clone(), s.title.clone())).or_default() += ms;
        }
        add_hours(&mut hours, s.start_ms.max(from), s.end_ms.min(to));
    }
    Ok(DayActivity {
        rev: rev.to_string(),
        date: p.date,
        total_ms: totals.total,
        apps: totals.apps(),
        projects: totals.projects(),
        titles: sorted(titles)
            .into_iter()
            .take(MAX_DAY_TITLES)
            .map(|((app, title), ms)| TitleMs { app, title, ms })
            .collect(),
        hours: hours
            .iter()
            .enumerate()
            .map(|(hour, ms)| HourMs {
                hour: hour as u32,
                ms: *ms,
            })
            .collect(),
    })
}

pub(crate) async fn range(db: &Db, p: RangeQuery) -> R<RangeActivity> {
    let days = days(&p.from, &p.to, MAX_RANGE_DAYS)?;
    let (from, to) = (days[0].1, days[days.len() - 1].2);
    let rev = db.rev();
    let sessions = overlapping(db, from, to).await.internal("read activity")?;
    let mut totals = Totals::default();
    let per_day = days
        .iter()
        .map(|(date, start, end)| DayTotal {
            date: date.format("%Y-%m-%d").to_string(),
            total_ms: sessions.iter().map(|s| clip(s, *start, *end)).sum(),
        })
        .collect();
    for s in &sessions {
        totals.add(s, clip(s, from, to));
    }
    Ok(RangeActivity {
        rev: rev.to_string(),
        days: per_day,
        apps: totals.apps(),
        projects: totals.projects(),
    })
}

fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .earliest()
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        .unwrap_or_else(|| ms.to_string())
}

/// Every field quoted; a leading `= + - @` (spreadsheet formula) gets a `'`.
fn csv_field(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    format!("\"{}\"", guarded.replace('"', "\"\""))
}

fn render(sessions: &[Session], format: ExportFormat) -> String {
    match format {
        ExportFormat::Csv => {
            let mut out = String::from("start,end,app,title,project\n");
            for s in sessions {
                let row = [
                    local_time(s.start_ms),
                    local_time(s.end_ms),
                    s.app.clone(),
                    s.title.clone(),
                    s.project_id.clone().unwrap_or_default(),
                ];
                let row: Vec<String> = row.iter().map(|f| csv_field(f)).collect();
                out.push_str(&row.join(","));
                out.push('\n');
            }
            out
        }
        ExportFormat::Json => {
            let rows: Vec<serde_json::Value> = sessions
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "start": local_time(s.start_ms),
                        "end": local_time(s.end_ms),
                        "app": s.app,
                        "title": s.title,
                        "project": s.project_id,
                    })
                })
                .collect();
            serde_json::Value::Array(rows).to_string()
        }
    }
}

/// `<exports>/activity-<from>-<to>.<csv|json>`: whole sessions overlapping the
/// range, written to a temp file then renamed (a second export replaces it).
pub(crate) async fn export(db: &Db, dir: &Path, p: ExportQuery) -> R<Exported> {
    let days = days(&p.from, &p.to, MAX_EXPORT_DAYS)?;
    let (from, to) = (days[0].1, days[days.len() - 1].2);
    let sessions = overlapping(db, from, to).await.internal("read activity")?;
    let text = render(&sessions, p.format);
    let ext = match p.format {
        ExportFormat::Csv => "csv",
        ExportFormat::Json => "json",
    };
    let name = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let path = dir.join(format!(
        "activity-{}-{}.{ext}",
        name(days[0].0),
        name(days[days.len() - 1].0)
    ));
    let (dir, target) = (dir.to_path_buf(), path.clone());
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        let temp = dir.join(format!(
            ".activity-export-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp, &target)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    })
    .await
    .internal("export activity")?
    .internal("export activity")?;
    Ok(Exported {
        path: path.display().to_string(),
    })
}
