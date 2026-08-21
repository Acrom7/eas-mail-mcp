#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use chrono::{TimeZone as _, Utc};
use eas_mail_mcp::ErrorCode;
use eas_mail_mcp::backend::AccountBackend as _;
use eas_mail_protocol::protocol::{
    build_availability, build_calendar_search, build_initial_provision, build_item_fetch,
    build_policy_ack,
};
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, Patch, RequestSafety};

use support::{
    call, default_policy, mailbox, mailbox_unprovisioned, options, options_with_calendar,
    provision_response, read,
};

#[tokio::test]
async fn availability_uses_only_options_provision_and_resolve_recipients() -> anyhow::Result<()> {
    let start = instant(9, 0)?;
    let end = instant(10, 0)?;
    let participants = vec!["user@example.invalid".into()];
    let calls = vec![
        options_with_calendar(),
        call(
            Command::Provision,
            build_initial_provision()?,
            None,
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(700), None)?,
        ),
        call(
            Command::Provision,
            build_policy_ack(700, true)?,
            Some(0),
            RequestSafety::RetrySafe,
            200,
            provision_response(1, Some(701), Some(1))?,
        ),
        call(
            Command::ResolveRecipients,
            build_availability(&participants, start, end)?,
            Some(701),
            RequestSafety::RetrySafe,
            200,
            availability_response("00")?,
        ),
    ];
    let (mailbox, transport) = mailbox_unprovisioned(calls)?;
    let availability = mailbox.calendar_availability(&participants, start, end).await?;
    assert_eq!(availability.len(), 1);
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn missing_resolve_recipients_is_an_optional_feature_failure() -> anyhow::Result<()> {
    let (mailbox, transport) = mailbox(vec![options()], default_policy())?;
    let result = mailbox
        .calendar_availability(&["user@example.invalid".into()], instant(9, 0)?, instant(10, 0)?)
        .await;
    assert_eq!(result.err().map(|error| error.envelope.code), Some(ErrorCode::FeatureUnavailable));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn calendar_search_and_get_use_no_folder_or_calendar_sync() -> anyhow::Result<()> {
    let calls = vec![
        options_with_calendar(),
        read(
            Command::Search,
            build_calendar_search("planning", 0, 10)?,
            calendar_search_response()?,
        ),
        read(
            Command::ItemOperations,
            build_item_fetch(Some("calendar-long-1"), None, None, 12_000)?,
            calendar_item_response()?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let search = mailbox.search_calendar("planning", 10).await?;
    let event =
        search.events.first().ok_or_else(|| anyhow::anyhow!("calendar search fixture is empty"))?;
    assert_eq!(event.fields.subject, Patch::Value("Planning".into()));
    let fetched = mailbox.fetch_calendar(&event.long_id, 12_000).await?;
    assert_eq!(fetched.fields.body, Patch::Value("Agenda".into()));
    transport.verify_complete()?;
    Ok(())
}

fn instant(hour: u32, minute: u32) -> anyhow::Result<chrono::DateTime<Utc>> {
    Utc.with_ymd_and_hms(2026, 8, 3, hour, minute, 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("fixture time is invalid"))
}

fn availability_response(value: &str) -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ResolveRecipients", "ResolveRecipients");
    root.push(Element::text("ResolveRecipients", "Status", "1"));
    let mut response = Element::new("ResolveRecipients", "Response");
    response.push(Element::text("ResolveRecipients", "To", "user@example.invalid"));
    response.push(Element::text("ResolveRecipients", "Status", "1"));
    response.push(Element::text("ResolveRecipients", "RecipientCount", "1"));
    let mut recipient = Element::new("ResolveRecipients", "Recipient");
    recipient.push(Element::text("ResolveRecipients", "Type", "1"));
    recipient.push(Element::text("ResolveRecipients", "DisplayName", "Test User"));
    recipient.push(Element::text("ResolveRecipients", "EmailAddress", "user@example.invalid"));
    let mut availability = Element::new("ResolveRecipients", "Availability");
    availability.push(Element::text("ResolveRecipients", "Status", "1"));
    availability.push(Element::text("ResolveRecipients", "MergedFreeBusy", value));
    recipient.push(availability);
    response.push(recipient);
    root.push(response);
    encode(&root)
}

fn calendar_search_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    root.push(Element::text("Search", "Total", "1"));
    let mut result = Element::new("Search", "Result");
    result.push(Element::text("Search", "LongId", "calendar-long-1"));
    let mut properties = Element::new("Search", "Properties");
    properties.push(Element::text("Calendar", "Subject", "Planning"));
    properties.push(Element::text("Calendar", "StartTime", "20260803T090000Z"));
    properties.push(Element::text("Calendar", "EndTime", "20260803T100000Z"));
    result.push(properties);
    root.push(result);
    encode(&root)
}

fn calendar_item_response() -> eas_mail_protocol::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    let mut properties = Element::new("ItemOperations", "Properties");
    properties.push(Element::text("Calendar", "Subject", "Planning"));
    let mut body = Element::new("AirSyncBase", "Body");
    body.push(Element::text("AirSyncBase", "Data", "Agenda"));
    properties.push(body);
    fetch.push(properties);
    root.push(fetch);
    encode(&root)
}
