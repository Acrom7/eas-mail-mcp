use chrono::{TimeZone as _, Utc};
use eas_mail_protocol::{
    CandidateAvailability, FreeBusyStatus, RecipientAvailability, RecipientResolution,
    ResolvedRecipient,
};

use super::*;
use crate::model::{ScheduleWeekday, WorkingHoursInput};

fn hours(start: &str, end: &str) -> Vec<WorkingHoursInput> {
    vec![WorkingHoursInput {
        weekdays: vec![
            ScheduleWeekday::Mon,
            ScheduleWeekday::Tue,
            ScheduleWeekday::Wed,
            ScheduleWeekday::Thu,
            ScheduleWeekday::Fri,
        ],
        start: start.into(),
        end: end.into(),
    }]
}

fn exact(input: &str, statuses: Vec<FreeBusyStatus>) -> RecipientAvailability {
    RecipientAvailability {
        input: input.into(),
        resolution: RecipientResolution::Resolved,
        total_candidates: 1,
        candidates: vec![ResolvedRecipient {
            recipient_type: 1,
            display_name: "Test User".into(),
            email: input.into(),
            availability: CandidateAvailability::Slots(statuses),
        }],
    }
}

fn unresolved(input: &str, resolution: RecipientResolution) -> RecipientAvailability {
    RecipientAvailability {
        input: input.into(),
        resolution,
        total_candidates: usize::from(resolution != RecipientResolution::NotFound),
        candidates: if resolution == RecipientResolution::NotFound {
            Vec::new()
        } else {
            vec![ResolvedRecipient {
                recipient_type: 1,
                display_name: "Candidate".into(),
                email: "candidate@example.com".into(),
                availability: CandidateAvailability::Missing,
            }]
        },
    }
}

#[test]
fn splits_a_31_day_query_into_bounded_pages() -> Result<()> {
    let participants = vec!["user@example.com".into()];
    let plan = plan(
        &participants,
        "2026-08-03",
        "2026-09-02",
        "Europe/Belgrade",
        &hours("09:00", "18:00"),
    )?;
    assert!(plan.chunks.len() >= 4);
    assert!(
        plan.chunks
            .iter()
            .all(|chunk| chunk.end.signed_duration_since(chunk.start) <= Duration::days(7))
    );
    Ok(())
}

#[test]
fn rejects_nonexistent_dst_working_time() {
    let participants = vec!["user@example.com".into()];
    let result = plan(
        &participants,
        "2026-03-29",
        "2026-03-29",
        "Europe/Belgrade",
        &[WorkingHoursInput {
            weekdays: vec![ScheduleWeekday::Sun],
            start: "02:30".into(),
            end: "04:00".into(),
        }],
    );
    assert_eq!(result.err().map(|error| error.envelope.code), Some(ErrorCode::ValidationFailed));
}

#[test]
fn tentative_only_becomes_a_slot_when_allowed() -> Result<()> {
    let participants = vec!["user@example.com".into()];
    let plan = plan(&participants, "2026-08-03", "2026-08-03", "UTC", &hours("09:00", "10:00"))?;
    let range = plan.chunks.first().copied().ok_or_else(state_error)?;
    let statuses = vec![FreeBusyStatus::Tentative, FreeBusyStatus::Free];
    let prepared = prepare(
        "account".into(),
        &participants,
        &plan,
        vec![AvailabilityPage {
            range,
            participants: vec![exact("user@example.com", statuses.clone())],
        }],
    )?;
    assert!(find_slots(prepared, 60, false, 20)?.windows.is_empty());
    let prepared = prepare(
        "account".into(),
        &participants,
        &plan,
        vec![AvailabilityPage { range, participants: vec![exact("user@example.com", statuses)] }],
    )?;
    let slots = find_slots(prepared, 60, true, 20)?;
    assert_eq!(slots.windows.len(), 1);
    assert_eq!(
        slots.windows.first().map(|value| value.window_start.as_str()),
        Some("2026-08-03T09:00:00+00:00")
    );
    Ok(())
}

#[test]
fn rejects_more_than_31_days() {
    let participants = vec!["user@example.com".into()];
    let result = plan(&participants, "2026-08-01", "2026-09-01", "UTC", &hours("09:00", "18:00"));
    assert_eq!(result.err().map(|error| error.envelope.code), Some(ErrorCode::ValidationFailed));
}

#[test]
fn utc_fixture_is_stable() {
    let value = Utc.with_ymd_and_hms(2026, 8, 3, 9, 0, 0).single();
    assert_eq!(value.map(|item| item.to_rfc3339()).as_deref(), Some("2026-08-03T09:00:00+00:00"));
}

#[test]
fn ambiguous_and_not_found_participants_never_produce_slots() -> Result<()> {
    let participants = vec!["ambiguous".into(), "missing".into()];
    let plan = plan(&participants, "2026-08-03", "2026-08-03", "UTC", &hours("09:00", "10:00"))?;
    let range = plan.chunks.first().copied().ok_or_else(state_error)?;
    let prepared = prepare(
        "account".into(),
        &participants,
        &plan,
        vec![AvailabilityPage {
            range,
            participants: vec![
                unresolved("ambiguous", RecipientResolution::Ambiguous),
                unresolved("missing", RecipientResolution::NotFound),
            ],
        }],
    )?;
    assert!(!prepared.data.resolution_complete);
    assert!(find_slots(prepared, 30, false, 20)?.windows.is_empty());
    Ok(())
}

#[test]
fn no_data_and_busy_intervals_block_common_windows() -> Result<()> {
    let participants = vec!["first@example.com".into(), "second@example.com".into()];
    let plan = plan(&participants, "2026-08-03", "2026-08-03", "UTC", &hours("09:00", "11:00"))?;
    let range = plan.chunks.first().copied().ok_or_else(state_error)?;
    let prepared = prepare(
        "account".into(),
        &participants,
        &plan,
        vec![AvailabilityPage {
            range,
            participants: vec![
                exact(
                    "first@example.com",
                    vec![
                        FreeBusyStatus::Free,
                        FreeBusyStatus::Busy,
                        FreeBusyStatus::Free,
                        FreeBusyStatus::NoData,
                    ],
                ),
                exact(
                    "second@example.com",
                    vec![
                        FreeBusyStatus::Free,
                        FreeBusyStatus::Free,
                        FreeBusyStatus::Free,
                        FreeBusyStatus::Free,
                    ],
                ),
            ],
        }],
    )?;
    let slots = find_slots(prepared, 30, false, 20)?;
    assert_eq!(slots.windows.len(), 2);
    assert!(slots.participants.first().is_some_and(|value| value.has_no_data));
    Ok(())
}

#[test]
fn invalid_duration_is_rejected_before_slot_output() -> Result<()> {
    let participants = vec!["user@example.com".into()];
    let plan = plan(&participants, "2026-08-03", "2026-08-03", "UTC", &hours("09:00", "10:00"))?;
    let range = plan.chunks.first().copied().ok_or_else(state_error)?;
    let prepared = prepare(
        "account".into(),
        &participants,
        &plan,
        vec![AvailabilityPage {
            range,
            participants: vec![exact(
                "user@example.com",
                vec![FreeBusyStatus::Free, FreeBusyStatus::Free],
            )],
        }],
    )?;
    assert_eq!(
        find_slots(prepared, 17, false, 20).err().map(|error| error.envelope.code),
        Some(ErrorCode::ValidationFailed)
    );
    Ok(())
}

#[test]
fn rejects_ambiguous_dst_working_time() {
    let participants = vec!["user@example.com".into()];
    let result = plan(
        &participants,
        "2026-10-25",
        "2026-10-25",
        "Europe/Belgrade",
        &[WorkingHoursInput {
            weekdays: vec![ScheduleWeekday::Sun],
            start: "02:30".into(),
            end: "04:00".into(),
        }],
    );
    assert_eq!(result.err().map(|error| error.envelope.code), Some(ErrorCode::ValidationFailed));
}

#[test]
fn oversized_complete_availability_is_rejected_without_truncation() -> Result<()> {
    let participants = (0..20).map(|index| format!("user{index}@example.com")).collect::<Vec<_>>();
    let all_days = vec![WorkingHoursInput {
        weekdays: vec![
            ScheduleWeekday::Mon,
            ScheduleWeekday::Tue,
            ScheduleWeekday::Wed,
            ScheduleWeekday::Thu,
            ScheduleWeekday::Fri,
            ScheduleWeekday::Sat,
            ScheduleWeekday::Sun,
        ],
        start: "00:00".into(),
        end: "23:59".into(),
    }];
    let plan = plan(&participants, "2026-08-01", "2026-08-31", "UTC", &all_days)?;
    let pages =
        plan.chunks
            .iter()
            .map(|range| {
                let slots = range.end.signed_duration_since(range.start).num_minutes() / 30;
                let statuses =
                    (0..slots)
                        .map(|index| {
                            if index % 2 == 0 { FreeBusyStatus::Free } else { FreeBusyStatus::Busy }
                        })
                        .collect::<Vec<_>>();
                AvailabilityPage {
                    range: *range,
                    participants: participants
                        .iter()
                        .map(|input| exact(input, statuses.clone()))
                        .collect(),
                }
            })
            .collect();
    assert_eq!(
        prepare("account".into(), &participants, &plan, pages)
            .err()
            .map(|error| error.envelope.code),
        Some(ErrorCode::ResultTooLarge)
    );
    Ok(())
}
