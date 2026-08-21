use chrono::{TimeZone as _, Utc};
use eas_mail_protocol::protocol::{build_availability, parse_availability};
use eas_mail_protocol::wbxml::{Element, decode, encode};
use eas_mail_protocol::{CandidateAvailability, EasError, FreeBusyStatus, RecipientResolution};

#[test]
fn availability_request_contains_recipients_and_utc_range() -> anyhow::Result<()> {
    let start = Utc.with_ymd_and_hms(2026, 8, 24, 7, 0, 0).single().ok_or_else(time_error)?;
    let end = Utc.with_ymd_and_hms(2026, 8, 24, 9, 0, 0).single().ok_or_else(time_error)?;
    let data = build_availability(&["Alice".into(), "bob@example.com".into()], start, end)?;
    let tree = decode(&data)?.ok_or_else(|| anyhow::anyhow!("request is empty"))?;
    let recipients = tree
        .children()
        .filter(|child| child.name == "To")
        .map(Element::text_content)
        .collect::<Vec<_>>();
    assert_eq!(recipients, ["Alice", "bob@example.com"]);
    assert_eq!(
        tree.descendant("ResolveRecipients", "StartTime").map(Element::text_content),
        Some("2026-08-24T07:00:00.000Z".into())
    );
    assert!(tree.descendant("ResolveRecipients", "MaxAmbiguousRecipients").is_some());
    Ok(())
}

#[test]
fn availability_response_parses_all_statuses() -> anyhow::Result<()> {
    let data = encode(&resolved_response("01234"))?;
    let results = parse_availability(&data, 5)?;
    let first = results.first().ok_or_else(|| anyhow::anyhow!("result is empty"))?;
    assert_eq!(first.input, "alice@example.com");
    assert_eq!(first.resolution, RecipientResolution::Resolved);
    let candidate = first.candidates.first().ok_or_else(|| anyhow::anyhow!("candidate missing"))?;
    assert_eq!(candidate.email, "alice@example.com");
    assert_eq!(
        candidate.availability,
        CandidateAvailability::Slots(vec![
            FreeBusyStatus::Free,
            FreeBusyStatus::Tentative,
            FreeBusyStatus::Busy,
            FreeBusyStatus::OutOfOffice,
            FreeBusyStatus::NoData,
        ])
    );
    Ok(())
}

#[test]
fn ambiguous_and_not_found_recipients_remain_typed() -> anyhow::Result<()> {
    let mut root = status_root(1);
    root.push(response("Alice", 2, 2, vec![candidate("Alice A", "a@example.com")]));
    root.push(response("Nobody", 4, 0, Vec::new()));
    let results = parse_availability(&encode(&root)?, 1)?;
    assert_eq!(results.len(), 2);
    let ambiguous = results.first().ok_or_else(|| anyhow::anyhow!("first result is missing"))?;
    let missing = results.get(1).ok_or_else(|| anyhow::anyhow!("second result is missing"))?;
    assert_eq!(ambiguous.resolution, RecipientResolution::Ambiguous);
    assert_eq!(ambiguous.total_candidates, 2);
    assert_eq!(missing.resolution, RecipientResolution::NotFound);
    Ok(())
}

#[test]
fn malformed_or_transient_availability_fails_safely() -> anyhow::Result<()> {
    let malformed = encode(&resolved_response("09"))?;
    assert!(matches!(parse_availability(&malformed, 2), Err(EasError::Protocol(_))));
    let wrong_length = encode(&resolved_response("0"))?;
    assert!(matches!(parse_availability(&wrong_length, 2), Err(EasError::Protocol(_))));
    let transient = encode(&status_root(6))?;
    assert!(matches!(parse_availability(&transient, 1), Err(EasError::ServiceUnavailable)));
    Ok(())
}

#[test]
fn availability_error_statuses_remain_typed() -> anyhow::Result<()> {
    let cases = [
        (160, CandidateAvailability::TooManyRecipients),
        (161, CandidateAvailability::DistributionListTooLarge),
        (162, CandidateAvailability::TransientFailure),
        (163, CandidateAvailability::Failure),
    ];
    for (status, expected) in cases {
        let results = parse_availability(&encode(&resolved_status_response(status))?, 1)?;
        let availability = results
            .first()
            .and_then(|result| result.candidates.first())
            .map(|candidate| &candidate.availability)
            .ok_or_else(|| anyhow::anyhow!("candidate availability is missing"))?;
        assert_eq!(availability, &expected);
    }
    Ok(())
}

fn resolved_response(free_busy: &str) -> Element {
    let mut root = status_root(1);
    let mut recipient = candidate("Alice", "alice@example.com");
    let mut availability = Element::new("ResolveRecipients", "Availability");
    availability.push(Element::text("ResolveRecipients", "Status", "1"));
    availability.push(Element::text("ResolveRecipients", "MergedFreeBusy", free_busy));
    recipient.push(availability);
    root.push(response("alice@example.com", 1, 1, vec![recipient]));
    root
}

fn resolved_status_response(status: u16) -> Element {
    let mut root = status_root(1);
    let mut recipient = candidate("Alice", "alice@example.com");
    let mut availability = Element::new("ResolveRecipients", "Availability");
    availability.push(Element::text("ResolveRecipients", "Status", status.to_string()));
    recipient.push(availability);
    root.push(response("alice@example.com", 1, 1, vec![recipient]));
    root
}

fn status_root(status: u16) -> Element {
    let mut root = Element::new("ResolveRecipients", "ResolveRecipients");
    root.push(Element::text("ResolveRecipients", "Status", status.to_string()));
    root
}

fn response(input: &str, status: u16, count: usize, recipients: Vec<Element>) -> Element {
    let mut response = Element::new("ResolveRecipients", "Response");
    response.push(Element::text("ResolveRecipients", "To", input));
    response.push(Element::text("ResolveRecipients", "Status", status.to_string()));
    response.push(Element::text("ResolveRecipients", "RecipientCount", count.to_string()));
    for recipient in recipients {
        response.push(recipient);
    }
    response
}

fn candidate(name: &str, email: &str) -> Element {
    let mut candidate = Element::new("ResolveRecipients", "Recipient");
    candidate.push(Element::text("ResolveRecipients", "Type", "1"));
    candidate.push(Element::text("ResolveRecipients", "DisplayName", name));
    candidate.push(Element::text("ResolveRecipients", "EmailAddress", email));
    candidate
}

fn time_error() -> anyhow::Error {
    anyhow::anyhow!("invalid test time")
}
