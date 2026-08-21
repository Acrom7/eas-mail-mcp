use std::collections::BTreeMap;

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::model::{ScheduleWeekday, WorkingHoursInput};
use crate::{AppError, ErrorCode, Result};

const MAX_DAYS: i64 = 31;
const MAX_CHUNK: Duration = Duration::days(7);
const MIN_CHUNK: Duration = Duration::minutes(30);

type DailyInterval = (NaiveTime, NaiveTime);
type WorkingRules = BTreeMap<ScheduleWeekday, Vec<DailyInterval>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UtcInterval {
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: DateTime<Utc>,
}

pub(crate) struct SchedulePlan {
    pub(crate) date_from: NaiveDate,
    pub(crate) date_to: NaiveDate,
    pub(crate) time_zone: Tz,
    pub(crate) working: Vec<UtcInterval>,
    pub(crate) chunks: Vec<UtcInterval>,
}

pub(super) fn build_plan(
    date_from: &str,
    date_to: &str,
    time_zone: &str,
    working_hours: &[WorkingHoursInput],
) -> Result<SchedulePlan> {
    let date_from = parse_date(date_from)?;
    let date_to = parse_date(date_to)?;
    validate_range(date_from, date_to)?;
    let time_zone = time_zone.parse::<Tz>().map_err(|_| validation("invalid IANA timezone"))?;
    let rules = parse_rules(working_hours)?;
    let working = materialize_working(date_from, date_to, time_zone, &rules)?;
    if working.is_empty() {
        return Err(validation("working hours do not intersect the requested dates"));
    }
    let first = working.first().copied().ok_or_else(state_error)?;
    let last = working.last().copied().ok_or_else(state_error)?;
    let query_end = std::cmp::max(last.end, first.start + MIN_CHUNK);
    let chunks = split_chunks(first.start, query_end)?;
    Ok(SchedulePlan { date_from, date_to, time_zone, working, chunks })
}

pub(super) fn format_in_zone(value: DateTime<Utc>, time_zone: Tz) -> String {
    value.with_timezone(&time_zone).to_rfc3339()
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| validation("dates must use YYYY-MM-DD format"))
}

fn validate_range(date_from: NaiveDate, date_to: NaiveDate) -> Result<()> {
    let days = date_to.signed_duration_since(date_from).num_days().saturating_add(1);
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(validation("date range must contain from 1 through 31 days"));
    }
    Ok(())
}

fn parse_rules(input: &[WorkingHoursInput]) -> Result<WorkingRules> {
    if input.is_empty() {
        return Err(validation("working_hours must not be empty"));
    }
    let mut output = BTreeMap::new();
    for group in input {
        if group.weekdays.is_empty() {
            return Err(validation("each working-hours group needs at least one weekday"));
        }
        let start = parse_time(&group.start)?;
        let end = parse_time(&group.end)?;
        if start >= end {
            return Err(validation("working-hours intervals cannot be empty or overnight"));
        }
        for weekday in &group.weekdays {
            output.entry(*weekday).or_insert_with(Vec::new).push((start, end));
        }
    }
    Ok(output)
}

fn parse_time(value: &str) -> Result<NaiveTime> {
    if value.len() != 5 {
        return Err(validation("working-hours times must use HH:MM format"));
    }
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| validation("working-hours times must use HH:MM format"))
}

fn materialize_working(
    date_from: NaiveDate,
    date_to: NaiveDate,
    time_zone: Tz,
    rules: &WorkingRules,
) -> Result<Vec<UtcInterval>> {
    let mut output = Vec::new();
    let mut date = date_from;
    loop {
        if let Some(intervals) = rules.get(&weekday(date)) {
            for (start, end) in intervals {
                output.push(UtcInterval {
                    start: local_to_utc(time_zone, date.and_time(*start))?,
                    end: local_to_utc(time_zone, date.and_time(*end))?,
                });
            }
        }
        if date == date_to {
            break;
        }
        date = date.succ_opt().ok_or_else(|| validation("date range overflows"))?;
    }
    output.sort_by_key(|value| value.start);
    Ok(super::slots::merge_intervals(output))
}

fn local_to_utc(time_zone: Tz, value: NaiveDateTime) -> Result<DateTime<Utc>> {
    match time_zone.from_local_datetime(&value) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, _) => {
            Err(validation("working hours contain an ambiguous local time during DST"))
        }
        LocalResult::None => {
            Err(validation("working hours contain a nonexistent local time during DST"))
        }
    }
}

fn split_chunks(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<UtcInterval>> {
    let mut output = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let remaining = end.signed_duration_since(cursor);
        let duration = if remaining > MAX_CHUNK && remaining - MAX_CHUNK < MIN_CHUNK {
            remaining - MIN_CHUNK
        } else {
            remaining.min(MAX_CHUNK)
        };
        if duration < MIN_CHUNK {
            return Err(state_error());
        }
        let next = cursor + duration;
        output.push(UtcInterval { start: cursor, end: next });
        cursor = next;
    }
    Ok(output)
}

fn weekday(value: NaiveDate) -> ScheduleWeekday {
    use chrono::Datelike as _;
    match value.weekday() {
        chrono::Weekday::Mon => ScheduleWeekday::Mon,
        chrono::Weekday::Tue => ScheduleWeekday::Tue,
        chrono::Weekday::Wed => ScheduleWeekday::Wed,
        chrono::Weekday::Thu => ScheduleWeekday::Thu,
        chrono::Weekday::Fri => ScheduleWeekday::Fri,
        chrono::Weekday::Sat => ScheduleWeekday::Sat,
        chrono::Weekday::Sun => ScheduleWeekday::Sun,
    }
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::ProtocolError, "calendar schedule state is inconsistent")
}
