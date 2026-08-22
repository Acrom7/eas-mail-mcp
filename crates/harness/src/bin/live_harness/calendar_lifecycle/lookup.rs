use std::time::Duration;

use chrono::{DateTime, Utc};
use eas_mail_mcp::{
    CalendarEvent, CalendarEventType, CalendarGetInput, CalendarSearchInput, MailSearchInput,
    Runtime,
};

use super::super::checks::required;

const DELIVERY_ATTEMPTS: usize = 45;
const DELIVERY_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
pub enum ExpectedEvent {
    Personal,
    Organizer,
    Attendee,
}

pub async fn wait_for_event(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    uid: Option<&str>,
    expected: ExpectedEvent,
) -> anyhow::Result<CalendarEvent> {
    wait_for_event_at(runtime, account_id, token, uid, expected, None).await
}

pub async fn wait_for_event_at(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    uid: Option<&str>,
    expected: ExpectedEvent,
    starts_at: Option<DateTime<Utc>>,
) -> anyhow::Result<CalendarEvent> {
    for _ in 0..DELIVERY_ATTEMPTS {
        if let Some(event) = find_event(runtime, account_id, token, uid, expected).await?
            && starts_at.is_none_or(|value| event_start(&event) == Some(value))
        {
            return Ok(event);
        }
        tokio::time::sleep(DELIVERY_DELAY).await;
    }
    anyhow::bail!("Calendar item did not reach the expected account within 90 seconds")
}

pub async fn find_event(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    uid: Option<&str>,
    expected: ExpectedEvent,
) -> anyhow::Result<Option<CalendarEvent>> {
    let search = required(
        runtime
            .calendar_search(CalendarSearchInput {
                query: token.to_owned(),
                account_ids: Some(vec![account_id.to_owned()]),
                limit: Some(20),
            })
            .await,
        "calendar_search lifecycle item",
    )?;
    for summary in search.items.into_iter().filter(|event| event.subject.contains(token)) {
        let response = runtime
            .calendar_get(CalendarGetInput {
                event_ref: summary.event_ref,
                body_limit: Some(12_000),
            })
            .await;
        let Some(event) = response.data else {
            continue;
        };
        if uid.is_none_or(|value| event.uid == value) && event_matches(&event, expected) {
            return Ok(Some(event));
        }
    }
    Ok(None)
}

pub async fn wait_for_event_absent(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
) -> anyhow::Result<()> {
    for _ in 0..DELIVERY_ATTEMPTS {
        let search = required(
            runtime
                .calendar_search(CalendarSearchInput {
                    query: token.to_owned(),
                    account_ids: Some(vec![account_id.to_owned()]),
                    limit: Some(20),
                })
                .await,
            "calendar_search after cleanup",
        )?;
        if search.items.iter().all(|event| !event.subject.contains(token)) {
            return Ok(());
        }
        tokio::time::sleep(DELIVERY_DELAY).await;
    }
    anyhow::bail!("Calendar item remained searchable after cleanup")
}

pub async fn mail_count(runtime: &Runtime, account_id: &str, token: &str) -> anyhow::Result<usize> {
    let result = required(
        runtime
            .mail_search(MailSearchInput {
                query: token.to_owned(),
                account_ids: Some(vec![account_id.to_owned()]),
                cursor: None,
                limit: Some(100),
            })
            .await,
        "mail_search meeting notification",
    )?;
    Ok(result.items.iter().filter(|message| message.subject.contains(token)).count())
}

pub async fn wait_for_mail_increase(
    runtime: &Runtime,
    account_id: &str,
    token: &str,
    baseline: usize,
) -> anyhow::Result<()> {
    for _ in 0..DELIVERY_ATTEMPTS {
        if mail_count(runtime, account_id, token).await? > baseline {
            return Ok(());
        }
        tokio::time::sleep(DELIVERY_DELAY).await;
    }
    anyhow::bail!("Calendar notification did not reach the expected mailbox within 90 seconds")
}

fn event_matches(event: &CalendarEvent, expected: ExpectedEvent) -> bool {
    match expected {
        ExpectedEvent::Personal => {
            event.event_type == CalendarEventType::Personal && event.can_update && event.can_delete
        }
        ExpectedEvent::Organizer => {
            event.event_type == CalendarEventType::OrganizerMeeting
                && event.can_update
                && event.can_cancel
        }
        ExpectedEvent::Attendee => {
            event.event_type == CalendarEventType::AttendeeMeeting && event.can_respond
        }
    }
}

fn event_start(event: &CalendarEvent) -> Option<DateTime<Utc>> {
    event.starts_at.as_deref()?.parse().ok()
}
