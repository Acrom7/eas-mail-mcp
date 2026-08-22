use eas_mail_protocol::{CalendarAttendee, MeetingResponseChoice, Patch};
use sha2::{Digest as _, Sha256};

use super::Runtime;
use super::calendar_mime::{self, CalendarMessageMethod};
use super::calendar_prepare::PreparedEvent;
use crate::backend::BackendEvent;
use crate::model::CalendarResponseChoice;
use crate::{AppError, ErrorCode, Result};

pub(super) fn notification(
    sender: &str,
    event: &PreparedEvent,
    recipients: &[CalendarAttendee],
    method: CalendarMessageMethod,
    comment: &str,
    client_id: &str,
) -> Result<Option<Vec<u8>>> {
    if recipients.is_empty() {
        Ok(None)
    } else {
        required_notification(sender, event, recipients, method, comment, client_id).map(Some)
    }
}

pub(super) fn required_notification(
    sender: &str,
    event: &PreparedEvent,
    recipients: &[CalendarAttendee],
    method: CalendarMessageMethod,
    comment: &str,
    client_id: &str,
) -> Result<Vec<u8>> {
    calendar_mime::build(
        sender,
        recipients,
        &event.mutation.application,
        event.all_day_dates,
        method,
        comment,
        client_id,
    )
}

pub(super) fn response_reference(
    runtime: &Runtime,
    mut source: BackendEvent,
    calendar_id: Option<String>,
    response: CalendarResponseChoice,
) -> Result<Option<String>> {
    if response == CalendarResponseChoice::Decline {
        return Ok(None);
    }
    if let Some(calendar_id) = calendar_id {
        source.server_id = Some(calendar_id);
    }
    source.fields.response_type = Patch::Value(response_type(response));
    runtime.references.insert_event(source).map(Some)
}

pub(super) fn organizer(source: &BackendEvent) -> Result<CalendarAttendee> {
    let email = match &source.fields.organizer_email {
        Patch::Value(value) if !value.is_empty() => value.clone(),
        _ => return Err(validation("received meeting has no organizer email")),
    };
    let name = match &source.fields.organizer {
        Patch::Value(value) => value.clone(),
        Patch::Missing => String::new(),
    };
    Ok(CalendarAttendee { email, name, attendee_type: 1, attendee_status: 0 })
}

pub(super) fn operation_uid(value: &str) -> Result<String> {
    uuid::Uuid::parse_str(value)
        .map(|value| format!("{}@eas-mail-mcp.local", value.hyphenated()))
        .map_err(|_| validation("idempotency_key must be a UUID"))
}

pub(super) fn step_client_id(operation_id: &str, step: &str) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(operation_id.as_bytes());
    digest.update([0]);
    digest.update(step.as_bytes());
    let digest = digest.finalize();
    let bytes = digest
        .get(..16)
        .ok_or_else(|| AppError::new(ErrorCode::StorageError, "cannot derive step ClientId"))?;
    uuid::Uuid::from_slice(bytes)
        .map(|value| value.to_string())
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot derive step ClientId"))
}

pub(super) const fn response_choice(value: CalendarResponseChoice) -> MeetingResponseChoice {
    match value {
        CalendarResponseChoice::Accept => MeetingResponseChoice::Accept,
        CalendarResponseChoice::Tentative => MeetingResponseChoice::Tentative,
        CalendarResponseChoice::Decline => MeetingResponseChoice::Decline,
    }
}

const fn response_type(value: CalendarResponseChoice) -> u8 {
    match value {
        CalendarResponseChoice::Tentative => 2,
        CalendarResponseChoice::Accept => 3,
        CalendarResponseChoice::Decline => 4,
    }
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
