use eas_mail_protocol::CalendarAttendee;

use super::Runtime;
use super::calendar_prepare::{self, EventOwnership, PreparedEvent, PreparedUpdate};
use super::calendar_response_prepare;
use super::calendar_write_result;
use super::calendar_write_support::operation_uid;
use super::write_preview::{PreparedWrite, WritePreview};
use crate::model::{
    CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput, CalendarOperationResult,
    CalendarRespondInput, CalendarResponseChoice, CalendarUpdateInput,
};
use crate::references::MeetingReference;
use crate::{AppError, ErrorCode, Result};

impl Runtime {
    pub(crate) async fn prepare_cli_calendar_create(
        &self,
        input: &CalendarCreateInput,
    ) -> Result<PreparedWrite<CalendarOperationResult>> {
        if let Some(record) = self.replay_write("calendar_create", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(calendar_write_result::existing(record)));
        }
        let backend = self.require_write(&input.account_id)?;
        let account = backend.account();
        let uid = operation_uid(&input.idempotency_key)?;
        let prepared = calendar_prepare::create(input, self.clock.now(), uid, &account.email)?;
        self.require_calendar_capabilities(
            &backend,
            !prepared.mutation.application.attendees.is_empty(),
        )
        .await?;
        Ok(PreparedWrite::Ready(event_preview("calendar_create", &input.account_id, &prepared)))
    }

    pub(crate) async fn prepare_cli_calendar_update(
        &self,
        input: &CalendarUpdateInput,
    ) -> Result<PreparedWrite<CalendarOperationResult>> {
        if let Some(record) = self.replay_write("calendar_update", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(calendar_write_result::existing(record)));
        }
        let reference = self.references.event(&input.event_ref)?;
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        let source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        let old = calendar_prepare::existing(&source, self.clock.now())?;
        let prepared = calendar_prepare::update(input, &source, self.clock.now(), &account.email)?;
        let meeting = !old.mutation.application.attendees.is_empty()
            || !prepared.event.mutation.application.attendees.is_empty();
        self.require_calendar_capabilities(&backend, meeting).await?;
        Ok(PreparedWrite::Ready(update_preview(&reference.account_id, &prepared)))
    }

    pub(crate) async fn prepare_cli_calendar_delete(
        &self,
        input: &CalendarDeleteInput,
    ) -> Result<PreparedWrite<CalendarOperationResult>> {
        if let Some(record) = self.replay_write("calendar_delete", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(calendar_write_result::existing(record)));
        }
        let reference = self.references.event(&input.event_ref)?;
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        let source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        calendar_prepare::require_non_recurring(&source)?;
        if calendar_prepare::ownership(&source, &account.email) != EventOwnership::Personal {
            return Err(validation("calendar_delete requires a personal event"));
        }
        self.require_calendar_capabilities(&backend, false).await?;
        let prepared = calendar_prepare::existing(&source, self.clock.now())?;
        Ok(PreparedWrite::Ready(event_preview("calendar_delete", &reference.account_id, &prepared)))
    }

    pub(crate) async fn prepare_cli_calendar_cancel(
        &self,
        input: &CalendarCancelInput,
    ) -> Result<PreparedWrite<CalendarOperationResult>> {
        if let Some(record) = self.replay_write("calendar_cancel", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(calendar_write_result::existing(record)));
        }
        calendar_prepare::validate_comment(&input.comment)?;
        let reference = self.references.event(&input.event_ref)?;
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        let source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        calendar_prepare::require_non_recurring(&source)?;
        if calendar_prepare::ownership(&source, &account.email) != EventOwnership::Organizer {
            return Err(validation("calendar_cancel requires an organizer meeting"));
        }
        self.require_calendar_capabilities(&backend, true).await?;
        let prepared = calendar_prepare::existing(&source, self.clock.now())?;
        Ok(PreparedWrite::Ready(
            event_preview("calendar_cancel", &reference.account_id, &prepared)
                .field("Comment", &input.comment),
        ))
    }

    pub(crate) async fn prepare_cli_calendar_respond(
        &self,
        input: &CalendarRespondInput,
    ) -> Result<PreparedWrite<CalendarOperationResult>> {
        if let Some(record) =
            self.replay_write("calendar_respond", &input.idempotency_key, input)?
        {
            return Ok(PreparedWrite::Replay(calendar_write_result::existing(record)));
        }
        calendar_prepare::validate_comment(&input.comment)?;
        match self.references.meeting(&input.event_ref)? {
            MeetingReference::Event(reference) => {
                self.prepare_event_response(input, reference).await.map(PreparedWrite::Ready)
            }
            MeetingReference::Mail(reference) => {
                self.prepare_mail_response(input, reference).await.map(PreparedWrite::Ready)
            }
        }
    }

    async fn prepare_event_response(
        &self,
        input: &CalendarRespondInput,
        reference: crate::backend::BackendEvent,
    ) -> Result<WritePreview> {
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        let source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        calendar_prepare::require_non_recurring(&source)?;
        if calendar_prepare::ownership(&source, &account.email) != EventOwnership::Attendee {
            return Err(validation("calendar_respond requires a received meeting"));
        }
        self.require_calendar_capabilities(&backend, true).await?;
        let prepared = calendar_prepare::existing(&source, self.clock.now())?;
        Ok(response_preview(&reference.account_id, &prepared, input))
    }

    async fn prepare_mail_response(
        &self,
        input: &CalendarRespondInput,
        reference: crate::backend::BackendMail,
    ) -> Result<WritePreview> {
        let backend = self.require_write(&reference.account_id)?;
        self.require_calendar_capabilities(&backend, true).await?;
        let mail = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 50_000).await,
        )?;
        let prepared = calendar_response_prepare::prepare(&mail, self.clock.now())?;
        Ok(response_preview(&reference.account_id, &prepared.event, input))
    }
}

pub(super) fn update_preview(account_id: &str, prepared: &PreparedUpdate) -> WritePreview {
    let removed = attendee_list(&prepared.removed_attendees);
    event_preview("calendar_update", account_id, &prepared.event)
        .field("Removed attendees", removed)
}

pub(super) fn response_preview(
    account_id: &str,
    prepared: &PreparedEvent,
    input: &CalendarRespondInput,
) -> WritePreview {
    event_preview("calendar_respond", account_id, prepared)
        .field("Response", response_name(input.response))
        .field("Comment", &input.comment)
}

pub(super) fn event_preview(
    operation: &'static str,
    account_id: &str,
    prepared: &PreparedEvent,
) -> WritePreview {
    let event = &prepared.mutation.application;
    WritePreview::new(operation, account_id.to_owned())
        .field("Subject", &event.subject)
        .field("Starts at UTC", event.starts_at.to_rfc3339())
        .field("Ends at UTC", event.ends_at.to_rfc3339())
        .field("All day", event.all_day.to_string())
        .field("Location", &event.location)
        .field("Busy status", busy_name(event.busy_status))
        .field("Reminder minutes", reminder(event.reminder_minutes))
        .field("Attendees", attendee_list(&event.attendees))
        .field("Body", &event.body)
}

fn attendee_list(values: &[CalendarAttendee]) -> String {
    values
        .iter()
        .map(|value| {
            let role = match value.attendee_type {
                2 => "optional",
                3 => "resource",
                _ => "required",
            };
            if value.name.is_empty() {
                format!("{role}:{}", value.email)
            } else {
                format!("{role}:{} <{}>", value.name, value.email)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

const fn response_name(value: CalendarResponseChoice) -> &'static str {
    match value {
        CalendarResponseChoice::Accept => "accept",
        CalendarResponseChoice::Tentative => "tentative",
        CalendarResponseChoice::Decline => "decline",
    }
}

const fn busy_name(value: u8) -> &'static str {
    match value {
        0 => "free",
        1 => "tentative",
        3 => "out_of_office",
        _ => "busy",
    }
}

fn reminder(value: Option<u32>) -> String {
    value.map_or_else(|| "none".into(), |minutes| minutes.to_string())
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
