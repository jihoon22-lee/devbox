//! Cron validation and wall-clock occurrence generation.
//!
//! Croner is deliberately used as a parser and field-matching aid only.  Its
//! timezone-aware iterator is not used here because it can turn a nonexistent
//! local wall time into a shifted instant.  We first enumerate matching naive
//! wall tuples, then resolve each tuple with the caller's timezone policy.

use chrono::{
    DateTime, Datelike, FixedOffset, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone,
    Timelike, Utc,
};
use croner::{
    Cron,
    errors::CronError,
    parser::{CronParser, Seconds, Year},
};
use std::collections::BTreeSet;

/// The number of occurrences used by preview callers when no count is given.
pub const DEFAULT_OCCURRENCE_COUNT: usize = 5;

// Croner's ALL_BIT is intentionally not exported.  It is the flag used by
// CronComponent::get_set_values for ordinary field values.
const ALL_FIELD_VALUES: u8 = 1;

/// A parsed, validated schedule.
#[derive(Debug, Clone)]
pub struct CronSchedule {
    cron: Cron,
    normalized_expression: String,
}

impl CronSchedule {
    /// Parse a standard five- or six-field cron expression.
    ///
    /// The seconds field is optional and is normalized to `0` when omitted.
    /// POSIX day-of-month/day-of-week OR semantics are always used.  Croner
    /// supports several `@...` nicknames and an explicit `+` AND modifier;
    /// both are rejected so every accepted expression has the same semantics
    /// in preview and in the scheduler.
    pub fn parse(expression: &str) -> Result<Self, CronError> {
        let expression = expression.trim();
        reject_unsupported_syntax(expression)?;

        let parser = CronParser::builder()
            .seconds(Seconds::Optional)
            .year(Year::Disallowed)
            .dom_and_dow(false)
            .build();
        let cron = parser.parse(expression)?;
        let source_fields = expression.split_whitespace().count();
        let normalized_expression = if source_fields == 5 {
            format!("0 {}", cron.as_str())
        } else {
            cron.as_str().to_string()
        };

        Ok(Self {
            normalized_expression,
            cron,
        })
    }

    /// Return the expression after Croner's optional seconds normalization.
    pub fn normalized_expression(&self) -> &str {
        &self.normalized_expression
    }

    /// Return the original parsed Croner value for field matching helpers.
    ///
    /// Callers should not use Croner's timezone-aware occurrence search for
    /// execution; use [`Self::next_occurrences`] instead.
    pub fn parsed_cron(&self) -> &Cron {
        &self.cron
    }

    /// Generate the next `count` occurrences in the system-local timezone.
    ///
    /// The comparison with `after` is exclusive and uses its local wall tuple.
    /// A nonexistent wall time is discarded.  An ambiguous wall time is
    /// represented once, using its earliest instant.
    pub fn next_local_occurrences(
        &self,
        after: DateTime<Local>,
        count: usize,
    ) -> Result<Vec<CronOccurrence<Local>>, CronError> {
        self.next_occurrences(after, count)
    }

    /// Generate the default five local preview occurrences.
    pub fn preview_local(
        &self,
        after: DateTime<Local>,
    ) -> Result<Vec<CronOccurrence<Local>>, CronError> {
        self.next_local_occurrences(after, DEFAULT_OCCURRENCE_COUNT)
    }

    /// Generate the next `count` occurrences for any chrono timezone.
    ///
    /// This generic form is used by deterministic `chrono-tz` tests and keeps
    /// the production path identical: enumerate naive wall tuples first, then
    /// call `TimeZone::from_local_datetime` to resolve them.
    pub fn next_occurrences<Tz: TimeZone>(
        &self,
        after: DateTime<Tz>,
        count: usize,
    ) -> Result<Vec<CronOccurrence<Tz>>, CronError> {
        if count == 0 {
            return Ok(Vec::new());
        }

        let walls = self.next_wall_occurrences(after.naive_local(), count)?;
        let mut occurrences = Vec::with_capacity(count);
        let mut seen_wall_keys = BTreeSet::new();

        for wall in &walls {
            let wall_key = occurrence_wall_key(*wall);
            if !seen_wall_keys.insert(wall_key.clone()) {
                continue;
            }

            let Some(datetime) = resolve_wall_time(wall, &after.timezone()) else {
                // Spring-forward (or another timezone transition) can make a
                // syntactically valid local wall tuple nonexistent.
                continue;
            };
            if datetime <= after {
                // During a fall-back overlap, the canonical earliest instant
                // for a wall tuple can be before an `after` instant that is
                // already in the later offset. It is no longer a next run.
                continue;
            }

            occurrences.push(CronOccurrence {
                datetime,
                wall_time: *wall,
                wall_key,
            });

            if occurrences.len() == count {
                break;
            }
        }

        // If a run of wall tuples fell into a DST gap, the first `count`
        // generated tuples may not yield `count` instants. Continue after the
        // last generated wall tuple until the requested number is resolved.
        let mut cursor = walls.last().copied();
        while occurrences.len() < count {
            let Some(current_cursor) = cursor else {
                break;
            };
            let more_walls =
                self.next_wall_occurrences(current_cursor, count - occurrences.len())?;
            if more_walls.is_empty() {
                break;
            }
            let mut progressed = false;
            for wall in &more_walls {
                let wall_key = occurrence_wall_key(*wall);
                if !seen_wall_keys.insert(wall_key.clone()) {
                    continue;
                }
                progressed = true;
                if let Some(datetime) = resolve_wall_time(wall, &after.timezone()) {
                    if datetime <= after {
                        continue;
                    }
                    occurrences.push(CronOccurrence {
                        datetime,
                        wall_time: *wall,
                        wall_key,
                    });
                    if occurrences.len() == count {
                        break;
                    }
                }
            }
            cursor = more_walls.last().copied();
            if !progressed {
                break;
            }
        }

        Ok(occurrences)
    }

    /// Enumerate matching naive local wall tuples in strictly increasing order.
    ///
    /// This is intentionally separate from timezone resolution.  It lets the
    /// caller inspect DST gap candidates and guarantees that a fallback wall
    /// tuple is considered once before it is resolved to an instant.
    pub fn next_wall_occurrences(
        &self,
        after: NaiveDateTime,
        count: usize,
    ) -> Result<Vec<NaiveDateTime>, CronError> {
        if count == 0 {
            return Ok(Vec::new());
        }

        let years = self.cron.pattern.years.get_set_values(ALL_FIELD_VALUES);
        let months = self.cron.pattern.months.get_set_values(ALL_FIELD_VALUES);
        let hours = self.cron.pattern.hours.get_set_values(ALL_FIELD_VALUES);
        let minutes = self.cron.pattern.minutes.get_set_values(ALL_FIELD_VALUES);
        let seconds = self.cron.pattern.seconds.get_set_values(ALL_FIELD_VALUES);
        let mut result = Vec::with_capacity(count);

        'years: for year in years {
            let year = i32::from(year);
            if year < after.year() {
                continue;
            }

            for month in &months {
                let month = u32::from(*month);
                if year == after.year() && month < after.month() {
                    continue;
                }

                let Some(first_day) = NaiveDate::from_ymd_opt(year, month, 1) else {
                    continue;
                };
                let days_in_month = first_day
                    .with_month(month + 1)
                    .or_else(|| NaiveDate::from_ymd_opt(year + 1, 1, 1))
                    .and_then(|next_month| next_month.pred_opt())
                    .map_or(31, |date| date.day());

                for day in 1..=days_in_month {
                    if year == after.year() && month == after.month() && day < after.day() {
                        continue;
                    }
                    if !self.cron.pattern.day_match(year, month, day)? {
                        continue;
                    }

                    let date =
                        NaiveDate::from_ymd_opt(year, month, day).ok_or(CronError::InvalidDate)?;
                    for hour in &hours {
                        let hour = u32::from(*hour);
                        if year == after.year()
                            && month == after.month()
                            && day == after.day()
                            && hour < after.hour()
                        {
                            continue;
                        }
                        for minute in &minutes {
                            let minute = u32::from(*minute);
                            if year == after.year()
                                && month == after.month()
                                && day == after.day()
                                && hour == after.hour()
                                && minute < after.minute()
                            {
                                continue;
                            }
                            for second in &seconds {
                                let second = u32::from(*second);
                                let Some(wall) = date.and_hms_opt(hour, minute, second) else {
                                    return Err(CronError::InvalidTime);
                                };
                                if wall <= after {
                                    continue;
                                }
                                result.push(wall);
                                if result.len() == count {
                                    break 'years;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Test whether a naive wall tuple matches the validated expression.
    pub fn matches_wall(&self, wall: NaiveDateTime) -> Result<bool, CronError> {
        let timezone = FixedOffset::east_opt(0).ok_or(CronError::InvalidTime)?;
        let datetime = timezone
            .from_local_datetime(&wall)
            .single()
            .ok_or(CronError::InvalidTime)?;
        self.cron.is_time_matching(&datetime)
    }
}

/// A resolved cron occurrence and the wall tuple that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronOccurrence<Tz: TimeZone> {
    pub datetime: DateTime<Tz>,
    pub wall_time: NaiveDateTime,
    pub wall_key: String,
}

impl<Tz: TimeZone> CronOccurrence<Tz> {
    /// Canonical epoch milliseconds for DB `scheduled_at` storage.
    pub fn timestamp_millis(&self) -> i64 {
        self.datetime.timestamp_millis()
    }
}

/// Validate a cron expression without retaining the parsed schedule.
pub fn validate_cron(expression: &str) -> Result<(), CronError> {
    CronSchedule::parse(expression).map(|_| ())
}

/// Parse and generate local occurrences for preview/evaluation callers.
pub fn next_occurrences(
    expression: &str,
    after: DateTime<Local>,
    count: usize,
) -> Result<Vec<CronOccurrence<Local>>, CronError> {
    CronSchedule::parse(expression)?.next_local_occurrences(after, count)
}

/// Parse and generate the default five local preview occurrences.
pub fn preview_occurrences(
    expression: &str,
    after: DateTime<Local>,
) -> Result<Vec<CronOccurrence<Local>>, CronError> {
    CronSchedule::parse(expression)?.preview_local(after)
}

/// Parse and generate occurrences in a deterministic or caller-selected
/// timezone.  Production preview/evaluation should normally use `Local`.
pub fn next_occurrences_in_timezone<Tz: TimeZone>(
    expression: &str,
    after: DateTime<Tz>,
    count: usize,
) -> Result<Vec<CronOccurrence<Tz>>, CronError> {
    CronSchedule::parse(expression)?.next_occurrences(after, count)
}

/// Return the first occurrence strictly after a UTC instant in `timezone`.
///
/// This uses the same wall-clock and DST policy as preview generation and
/// preserves parse errors. `None` means the schedule has no future occurrence.
pub fn next_after<Tz: TimeZone>(
    expression: &str,
    timezone: &Tz,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, CronError> {
    Ok(
        next_occurrences_in_timezone(expression, after.with_timezone(timezone), 1)?
            .into_iter()
            .next()
            .map(|occurrence| occurrence.datetime.with_timezone(&Utc)),
    )
}

/// Parse and generate naive wall tuples without resolving a timezone.
pub fn next_wall_occurrences(
    expression: &str,
    after: NaiveDateTime,
    count: usize,
) -> Result<Vec<NaiveDateTime>, CronError> {
    CronSchedule::parse(expression)?.next_wall_occurrences(after, count)
}

/// Resolve a wall tuple with explicit DST behavior.
///
/// `None` means the wall time does not exist (spring-forward gap).  For an
/// overlap, the returned instant is whichever candidate is earlier on the UTC
/// timeline, so the wall key can be claimed only once.
pub fn resolve_wall_time<Tz: TimeZone>(
    wall: &NaiveDateTime,
    timezone: &Tz,
) -> Option<DateTime<Tz>> {
    match timezone.from_local_datetime(wall) {
        LocalResult::None => None,
        LocalResult::Single(datetime) => Some(datetime),
        LocalResult::Ambiguous(first, second) => {
            if first.naive_utc() <= second.naive_utc() {
                Some(first)
            } else {
                Some(second)
            }
        }
    }
}

/// Build the canonical local wall key stored with an automatic run.
pub fn occurrence_wall_key(wall: NaiveDateTime) -> String {
    wall.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn reject_unsupported_syntax(expression: &str) -> Result<(), CronError> {
    if expression.is_empty() {
        return Err(CronError::EmptyPattern);
    }
    if expression.starts_with('@') || expression.contains('@') {
        return Err(CronError::InvalidPattern(
            "cron macros (including @reboot) are not supported".to_string(),
        ));
    }

    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() == 5 || fields.len() == 6 {
        let dow = fields[fields.len() - 1];
        if dow.starts_with('+') {
            return Err(CronError::InvalidPattern(
                "the day-of-month/day-of-week relation must use POSIX OR semantics".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cron_tests.rs"]
mod tests;
