use chrono::{DateTime, Duration, SecondsFormat, Utc};

use crate::wbxml::{Element, decode, encode};
use crate::{
    CandidateAvailability, EasError, FreeBusyStatus, RecipientAvailability, RecipientResolution,
    ResolvedRecipient, Result,
};

use super::tree::{direct_text, element, push_text};

const MAX_RECIPIENTS: usize = 100;
const MAX_AMBIGUOUS_RECIPIENTS: usize = 10;
const MAX_FREE_BUSY_BYTES: usize = 32 * 1024;

/// Builds a bounded ResolveRecipients Availability request.
pub fn build_availability(
    participants: &[String],
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
) -> Result<Vec<u8>> {
    validate_request(participants, starts_at, ends_at)?;
    let mut root = element("ResolveRecipients", "ResolveRecipients");
    for participant in participants {
        push_text(&mut root, "ResolveRecipients", "To", participant.trim());
    }
    let mut options = element("ResolveRecipients", "Options");
    push_text(
        &mut options,
        "ResolveRecipients",
        "MaxAmbiguousRecipients",
        MAX_AMBIGUOUS_RECIPIENTS.to_string(),
    );
    let mut availability = element("ResolveRecipients", "Availability");
    push_text(
        &mut availability,
        "ResolveRecipients",
        "StartTime",
        starts_at.to_rfc3339_opts(SecondsFormat::Millis, true),
    );
    push_text(
        &mut availability,
        "ResolveRecipients",
        "EndTime",
        ends_at.to_rfc3339_opts(SecondsFormat::Millis, true),
    );
    options.push(availability);
    root.push(options);
    encode(&root)
}

/// Parses one ResolveRecipients response and validates every free/busy stream.
pub fn parse_availability(
    data: &[u8],
    expected_slots: usize,
) -> Result<Vec<RecipientAvailability>> {
    let root = decode(data)?.ok_or_else(|| {
        EasError::Protocol("Exchange returned an empty ResolveRecipients response".into())
    })?;
    match number(direct_text(&root, "ResolveRecipients", "Status"), 0) {
        1 => {}
        6 => return Err(EasError::ServiceUnavailable),
        status => {
            return Err(EasError::Protocol(format!("ResolveRecipients status is {status}")));
        }
    }
    root.children()
        .filter(|child| child.namespace == "ResolveRecipients" && child.name == "Response")
        .map(|response| parse_response(response, expected_slots))
        .collect()
}

fn validate_request(
    participants: &[String],
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
) -> Result<()> {
    if participants.is_empty()
        || participants.len() > MAX_RECIPIENTS
        || participants.iter().any(|value| {
            value.trim().is_empty() || value.len() > 254 || value.chars().any(char::is_control)
        })
    {
        return Err(EasError::InvalidConfiguration(
            "availability requires 1-100 valid recipients".into(),
        ));
    }
    let duration = ends_at.signed_duration_since(starts_at);
    if duration < Duration::minutes(30) || duration > Duration::days(7) {
        return Err(EasError::InvalidConfiguration(
            "availability range must be between 30 minutes and 7 days".into(),
        ));
    }
    Ok(())
}

fn parse_response(response: &Element, expected_slots: usize) -> Result<RecipientAvailability> {
    let input = direct_text(response, "ResolveRecipients", "To").unwrap_or_default();
    let status = number(direct_text(response, "ResolveRecipients", "Status"), 0);
    let resolution = match status {
        1 => RecipientResolution::Resolved,
        2 => RecipientResolution::Ambiguous,
        3 => RecipientResolution::AmbiguousPartial,
        4 => RecipientResolution::NotFound,
        _ => {
            return Err(EasError::Protocol(format!("recipient resolution status is {status}")));
        }
    };
    let total_candidates = direct_text(response, "ResolveRecipients", "RecipientCount")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let candidates = response
        .children()
        .filter(|child| child.namespace == "ResolveRecipients" && child.name == "Recipient")
        .map(|candidate| parse_candidate(candidate, expected_slots))
        .collect::<Result<Vec<_>>>()?;
    if resolution == RecipientResolution::Resolved && candidates.len() != 1 {
        return Err(EasError::Protocol(
            "resolved recipient response must contain exactly one candidate".into(),
        ));
    }
    Ok(RecipientAvailability { input, resolution, total_candidates, candidates })
}

fn parse_candidate(candidate: &Element, expected_slots: usize) -> Result<ResolvedRecipient> {
    let recipient_type = number(direct_text(candidate, "ResolveRecipients", "Type"), 0);
    let display_name =
        direct_text(candidate, "ResolveRecipients", "DisplayName").unwrap_or_default();
    let email = direct_text(candidate, "ResolveRecipients", "EmailAddress").unwrap_or_default();
    let availability = candidate
        .child("ResolveRecipients", "Availability")
        .map_or(Ok(CandidateAvailability::Missing), |value| {
            parse_candidate_availability(value, expected_slots)
        })?;
    Ok(ResolvedRecipient { recipient_type, display_name, email, availability })
}

fn parse_candidate_availability(
    availability: &Element,
    expected_slots: usize,
) -> Result<CandidateAvailability> {
    let status = number(direct_text(availability, "ResolveRecipients", "Status"), 0);
    match status {
        1 => {
            let value = direct_text(availability, "ResolveRecipients", "MergedFreeBusy")
                .ok_or_else(|| EasError::Protocol("MergedFreeBusy is missing".into()))?;
            parse_slots(&value, expected_slots).map(CandidateAvailability::Slots)
        }
        160 => Ok(CandidateAvailability::TooManyRecipients),
        161 => Ok(CandidateAvailability::DistributionListTooLarge),
        162 => Ok(CandidateAvailability::TransientFailure),
        163 => Ok(CandidateAvailability::Failure),
        _ => Err(EasError::Protocol(format!("availability status is {status}"))),
    }
}

fn parse_slots(value: &str, expected_slots: usize) -> Result<Vec<FreeBusyStatus>> {
    if value.len() > MAX_FREE_BUSY_BYTES || value.len() != expected_slots {
        return Err(EasError::Protocol(
            "MergedFreeBusy length does not match the requested range".into(),
        ));
    }
    value
        .bytes()
        .map(|byte| match byte {
            b'0' => Ok(FreeBusyStatus::Free),
            b'1' => Ok(FreeBusyStatus::Tentative),
            b'2' => Ok(FreeBusyStatus::Busy),
            b'3' => Ok(FreeBusyStatus::OutOfOffice),
            b'4' => Ok(FreeBusyStatus::NoData),
            _ => Err(EasError::Protocol("MergedFreeBusy contains an invalid status".into())),
        })
        .collect()
}

fn number(value: Option<String>, default: u16) -> u16 {
    value.and_then(|value| value.parse().ok()).unwrap_or(default)
}
