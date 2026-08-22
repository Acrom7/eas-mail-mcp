use base64::Engine as _;
use chrono::Duration;

use super::*;

#[test]
fn timed_schedule_validates_offset_and_emits_eas_timezone() -> anyhow::Result<()> {
    let prepared = prepare(&timed(
        "2026-08-24T11:00:00+02:00",
        "2026-08-24T12:00:00+02:00",
        "Europe/Belgrade",
    ))?;
    assert_eq!(prepared.starts_at.to_rfc3339(), "2026-08-24T09:00:00+00:00");
    assert_eq!(prepared.ends_at - prepared.starts_at, Duration::hours(1));
    let bytes = base64::engine::general_purpose::STANDARD.decode(prepared.time_zone)?;
    assert_eq!(bytes.len(), 172);
    assert_eq!(little_i32(&bytes, 0)?, -60);
    assert_eq!(little_i32(&bytes, 168)?, -60);
    Ok(())
}

#[test]
fn utc_timezone_has_no_bias_or_transitions() -> anyhow::Result<()> {
    let prepared = prepare(&timed("2026-01-15T09:00:00Z", "2026-01-15T10:00:00Z", "UTC"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(prepared.time_zone)?;
    assert_eq!(little_i32(&bytes, 0)?, 0);
    assert_eq!(little_i32(&bytes, 168)?, 0);
    assert!(bytes.get(68..84).is_some_and(|value| value.iter().all(|byte| *byte == 0)));
    Ok(())
}

#[test]
fn mismatched_and_nonexistent_local_times_are_rejected() {
    for input in [
        timed("2026-08-24T11:00:00+01:00", "2026-08-24T12:00:00+01:00", "Europe/Belgrade"),
        timed("2026-03-29T02:30:00+01:00", "2026-03-29T03:30:00+02:00", "Europe/Belgrade"),
        timed("2026-08-24T09:00:00Z", "2026-08-24T08:00:00Z", "UTC"),
    ] {
        assert!(prepare(&input).is_err());
    }
}

#[test]
fn all_day_schedule_uses_exclusive_dates_across_dst() -> anyhow::Result<()> {
    let prepared = prepare(&CalendarScheduleInput::AllDay {
        start_date: "2026-03-29".into(),
        end_date: "2026-03-30".into(),
        time_zone: "Europe/Belgrade".into(),
    })?;
    assert_eq!(prepared.ends_at - prepared.starts_at, Duration::hours(23));
    assert_eq!(
        prepared.all_day_dates.map(|(start, end)| (start.to_string(), end.to_string())),
        Some(("2026-03-29".into(), "2026-03-30".into()))
    );
    assert!(
        prepare(&CalendarScheduleInput::AllDay {
            start_date: "2026-03-30".into(),
            end_date: "2026-03-30".into(),
            time_zone: "Europe/Belgrade".into(),
        })
        .is_err()
    );
    Ok(())
}

fn timed(start: &str, end: &str, time_zone: &str) -> CalendarScheduleInput {
    CalendarScheduleInput::Timed {
        start: start.into(),
        end: end.into(),
        time_zone: time_zone.into(),
    }
}

fn little_i32(bytes: &[u8], start: usize) -> anyhow::Result<i32> {
    let value = bytes
        .get(start..start + 4)
        .ok_or_else(|| anyhow::anyhow!("timezone field is missing"))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("timezone field has an invalid length"))?;
    Ok(i32::from_le_bytes(value))
}
