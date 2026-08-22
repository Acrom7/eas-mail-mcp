use base64::Engine as _;
use chrono::{
    DateTime, Datelike as _, Duration, LocalResult, NaiveDate, NaiveDateTime, Offset as _,
    TimeZone as _, Timelike as _, Utc, Weekday,
};
use chrono_tz::{OffsetComponents as _, OffsetName as _, Tz};

use crate::model::CalendarScheduleInput;
use crate::{AppError, ErrorCode, Result};

pub(super) struct PreparedSchedule {
    pub(super) starts_at: DateTime<Utc>,
    pub(super) ends_at: DateTime<Utc>,
    pub(super) time_zone: String,
    pub(super) all_day_dates: Option<(NaiveDate, NaiveDate)>,
}

pub(super) fn prepare(input: &CalendarScheduleInput) -> Result<PreparedSchedule> {
    match input {
        CalendarScheduleInput::Timed { start, end, time_zone } => {
            prepare_timed(start, end, time_zone)
        }
        CalendarScheduleInput::AllDay { start_date, end_date, time_zone } => {
            prepare_all_day(start_date, end_date, time_zone)
        }
    }
}

fn prepare_timed(start: &str, end: &str, time_zone: &str) -> Result<PreparedSchedule> {
    let zone = parse_zone(time_zone)?;
    let start = parse_instant(start, zone)?;
    let end = parse_instant(end, zone)?;
    if end <= start {
        return Err(validation("calendar end must be after start"));
    }
    let year = start.with_timezone(&zone).year();
    Ok(PreparedSchedule {
        starts_at: start,
        ends_at: end,
        time_zone: encode_time_zone(zone, year)?,
        all_day_dates: None,
    })
}

fn prepare_all_day(start: &str, end: &str, time_zone: &str) -> Result<PreparedSchedule> {
    let zone = parse_zone(time_zone)?;
    let start_date = parse_date(start)?;
    let end_date = parse_date(end)?;
    if end_date <= start_date {
        return Err(validation("all-day end_date must be after start_date"));
    }
    let starts_at = local_midnight(zone, start_date)?;
    let ends_at = local_midnight(zone, end_date)?;
    Ok(PreparedSchedule {
        starts_at,
        ends_at,
        time_zone: encode_time_zone(zone, start_date.year())?,
        all_day_dates: Some((start_date, end_date)),
    })
}

fn parse_zone(value: &str) -> Result<Tz> {
    value.parse().map_err(|_| validation("time_zone must be a valid IANA timezone"))
}

fn parse_instant(value: &str, zone: Tz) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| validation("timed schedule values must be RFC3339 timestamps"))?;
    let instant = parsed.with_timezone(&Utc);
    let expected = parsed.offset().local_minus_utc();
    let actual = zone.offset_from_utc_datetime(&instant.naive_utc()).fix().local_minus_utc();
    if expected != actual {
        return Err(validation("RFC3339 offset does not match time_zone at that instant"));
    }
    Ok(instant)
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| validation("all-day dates must use YYYY-MM-DD"))
}

fn local_midnight(zone: Tz, date: NaiveDate) -> Result<DateTime<Utc>> {
    let local =
        date.and_hms_opt(0, 0, 0).ok_or_else(|| validation("all-day local midnight is invalid"))?;
    match zone.from_local_datetime(&local) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, _) => Err(validation("all-day local midnight is ambiguous")),
        LocalResult::None => Err(validation("all-day local midnight does not exist")),
    }
}

fn encode_time_zone(zone: Tz, year: i32) -> Result<String> {
    let transitions = transitions(zone, year)?;
    if !matches!(transitions.len(), 0 | 2) {
        return Err(validation("time_zone has an unsupported transition pattern for this year"));
    }
    let sample = utc_datetime(year, 1, 15)?;
    let sample_offset = zone.offset_from_utc_datetime(&sample.naive_utc());
    let base_seconds = sample_offset.base_utc_offset().num_seconds();
    let base_minutes = exact_minutes(base_seconds)?;
    let daylight = transitions.iter().find(|value| value.new_dst_seconds != 0);
    let standard = transitions.iter().find(|value| value.new_dst_seconds == 0);
    if transitions.len() == 2 && (daylight.is_none() || standard.is_none()) {
        return Err(validation("time_zone transitions are not a standard DST pair"));
    }
    let daylight_delta = daylight.map_or(0, |value| value.new_dst_seconds);
    let mut bytes = Vec::with_capacity(172);
    push_i32(&mut bytes, -base_minutes);
    push_name(&mut bytes, standard.map_or("Standard", |value| value.new_name.as_str()));
    push_system_time(&mut bytes, standard.map(|value| value.local_before))?;
    push_i32(&mut bytes, 0);
    push_name(&mut bytes, daylight.map_or("Daylight", |value| value.new_name.as_str()));
    push_system_time(&mut bytes, daylight.map(|value| value.local_before))?;
    push_i32(&mut bytes, -exact_minutes(daylight_delta)?);
    if bytes.len() != 172 {
        return Err(validation("EAS timezone encoding produced an invalid length"));
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

struct Transition {
    local_before: NaiveDateTime,
    new_dst_seconds: i64,
    new_name: String,
}

fn transitions(zone: Tz, year: i32) -> Result<Vec<Transition>> {
    let start = utc_datetime(year, 1, 1)? - Duration::days(2);
    let end = utc_datetime(year + 1, 1, 1)? + Duration::days(2);
    let mut cursor = start;
    let mut previous = offset_snapshot(zone, cursor);
    let mut output = Vec::new();
    while cursor < end {
        let next = (cursor + Duration::hours(6)).min(end);
        let current = offset_snapshot(zone, next);
        if current.total_seconds != previous.total_seconds {
            let transition = locate_transition(zone, cursor, next, previous.total_seconds);
            let after = offset_snapshot(zone, transition);
            let local_before = transition.naive_utc() + Duration::seconds(previous.total_seconds);
            if local_before.year() == year {
                if after.base_seconds != previous.base_seconds {
                    return Err(validation("time_zone changes its base UTC offset in this year"));
                }
                output.push(Transition {
                    local_before,
                    new_dst_seconds: after.dst_seconds,
                    new_name: after.name.clone(),
                });
            }
            previous = after;
        } else {
            previous = current;
        }
        cursor = next;
    }
    Ok(output)
}

struct OffsetSnapshot {
    total_seconds: i64,
    base_seconds: i64,
    dst_seconds: i64,
    name: String,
}

fn offset_snapshot(zone: Tz, instant: DateTime<Utc>) -> OffsetSnapshot {
    let offset = zone.offset_from_utc_datetime(&instant.naive_utc());
    OffsetSnapshot {
        total_seconds: i64::from(offset.fix().local_minus_utc()),
        base_seconds: offset.base_utc_offset().num_seconds(),
        dst_seconds: offset.dst_offset().num_seconds(),
        name: offset.abbreviation().unwrap_or("Time zone").to_owned(),
    }
}

fn locate_transition(
    zone: Tz,
    mut low: DateTime<Utc>,
    mut high: DateTime<Utc>,
    old_offset: i64,
) -> DateTime<Utc> {
    while high.signed_duration_since(low) > Duration::seconds(1) {
        let seconds = high.signed_duration_since(low).num_seconds() / 2;
        let middle = low + Duration::seconds(seconds);
        if offset_snapshot(zone, middle).total_seconds == old_offset {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

fn push_name(output: &mut Vec<u8>, value: &str) {
    let mut units = value.encode_utf16().take(31).collect::<Vec<_>>();
    units.resize(32, 0);
    for unit in units {
        output.extend_from_slice(&unit.to_le_bytes());
    }
}

fn push_system_time(output: &mut Vec<u8>, value: Option<NaiveDateTime>) -> Result<()> {
    let Some(value) = value else {
        output.extend_from_slice(&[0; 16]);
        return Ok(());
    };
    let date = value.date();
    let week = transition_week(date)?;
    for field in [
        0,
        u16::try_from(date.month()).map_err(|_| validation("invalid timezone month"))?,
        weekday(date.weekday()),
        week,
        u16::try_from(value.hour()).map_err(|_| validation("invalid timezone hour"))?,
        u16::try_from(value.minute()).map_err(|_| validation("invalid timezone minute"))?,
        u16::try_from(value.second()).map_err(|_| validation("invalid timezone second"))?,
        0,
    ] {
        output.extend_from_slice(&field.to_le_bytes());
    }
    Ok(())
}

fn transition_week(date: NaiveDate) -> Result<u16> {
    let week = ((date.day() - 1) / 7) + 1;
    let next = date.checked_add_signed(Duration::days(7));
    let value = if next.is_none_or(|next| next.month() != date.month()) { 5 } else { week };
    u16::try_from(value).map_err(|_| validation("invalid timezone transition week"))
}

const fn weekday(value: Weekday) -> u16 {
    match value {
        Weekday::Sun => 0,
        Weekday::Mon => 1,
        Weekday::Tue => 2,
        Weekday::Wed => 3,
        Weekday::Thu => 4,
        Weekday::Fri => 5,
        Weekday::Sat => 6,
    }
}

fn exact_minutes(seconds: i64) -> Result<i32> {
    if seconds % 60 != 0 {
        return Err(validation("time_zone offset is not minute-aligned"));
    }
    i32::try_from(seconds / 60).map_err(|_| validation("time_zone offset is out of range"))
}

fn utc_datetime(year: i32, month: u32, day: u32) -> Result<DateTime<Utc>> {
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| validation("time_zone year is out of range"))?;
    let value =
        date.and_hms_opt(0, 0, 0).ok_or_else(|| validation("time_zone boundary is invalid"))?;
    Ok(DateTime::from_naive_utc_and_offset(value, Utc))
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

#[cfg(test)]
mod tests;
