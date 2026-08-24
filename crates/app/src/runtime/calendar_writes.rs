use std::sync::Arc;

use super::Runtime;
use super::calendar_mime::CalendarMessageMethod;
use super::calendar_prepare::{self, EventOwnership};
use super::calendar_write_result::{self, STEP_ITEM, STEP_NOTIFY_CURRENT, STEP_NOTIFY_REMOVED};
use super::calendar_write_support::{
    notification, operation_uid, required_notification, step_client_id,
};
use crate::backend::AccountBackend;
use crate::model::{
    ApiResponse, CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput,
    CalendarOperationResult, CalendarOperationState, CalendarRespondInput, CalendarUpdateInput,
};
use crate::{AppError, ErrorCode, JournalRecord, OperationStatus, Result, Warning};

impl Runtime {
    /// Creates one non-recurring personal event or organizer meeting.
    pub async fn calendar_create(
        &self,
        input: CalendarCreateInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_create_result(input).await)
    }

    /// Applies a patch to one non-recurring personal event or organizer meeting.
    pub async fn calendar_update(
        &self,
        input: CalendarUpdateInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_update_result(input).await)
    }

    /// Deletes one non-recurring personal event.
    pub async fn calendar_delete(
        &self,
        input: CalendarDeleteInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_delete_result(input).await)
    }

    /// Cancels one non-recurring organizer meeting and notifies attendees.
    pub async fn calendar_cancel(
        &self,
        input: CalendarCancelInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_cancel_result(input).await)
    }

    /// Accepts, tentatively accepts, or declines one non-recurring received meeting.
    pub async fn calendar_respond(
        &self,
        input: CalendarRespondInput,
    ) -> ApiResponse<CalendarOperationResult> {
        Self::response(self.calendar_respond_result(input).await)
    }

    async fn calendar_create_result(
        &self,
        input: CalendarCreateInput,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) =
            self.replay_write("calendar_create", &input.idempotency_key, &input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let backend = self.require_write(&input.account_id)?;
        let account = backend.account();
        let uid = operation_uid(&input.idempotency_key)?;
        let prepared = calendar_prepare::create(&input, self.clock.now(), uid, &account.email)?;
        self.require_calendar_capabilities(
            &backend,
            !prepared.mutation.application.attendees.is_empty(),
        )
        .await?;
        let request_id = step_client_id(&input.idempotency_key, "request")?;
        let request_mime = notification(
            &account.email,
            &prepared,
            &prepared.mutation.application.attendees,
            CalendarMessageMethod::Request,
            "",
        )?;
        let _guard = self.write_locks.acquire(&input.account_id).await?;
        let begin =
            self.begin_write(&input.account_id, "calendar_create", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        let created =
            match backend.create_calendar_item(&begin.record.client_id, &prepared.mutation).await {
                Ok(value) => value,
                Err(error) => return self.calendar_failure(&begin.record, 0, error, None),
            };
        let mut steps = STEP_ITEM;
        self.journal.checkpoint(&begin.record.operation_id, steps)?;
        let event_ref = self.references.insert_event(created)?;
        if let Some(mime) = request_mime {
            if let Err(error) = backend.send_calendar_message(&request_id, mime).await {
                return self.calendar_failure(&begin.record, steps, error, Some(event_ref));
            }
            steps |= STEP_NOTIFY_CURRENT;
            self.journal.checkpoint(&begin.record.operation_id, steps)?;
        }
        self.calendar_success(&begin.record, steps, Some(event_ref))
    }

    async fn calendar_update_result(
        &self,
        input: CalendarUpdateInput,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) =
            self.replay_write("calendar_update", &input.idempotency_key, &input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let reference = self.references.event(&input.event_ref)?;
        calendar_prepare::require_non_recurring(&reference)?;
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let source = self.account_result(
            &reference.account_id,
            backend.resolve_calendar_source(&reference).await,
        )?;
        let old = calendar_prepare::existing(&source, self.clock.now())?;
        let prepared = calendar_prepare::update(&input, &source, self.clock.now(), &account.email)?;
        let meeting = !old.mutation.application.attendees.is_empty()
            || !prepared.event.mutation.application.attendees.is_empty();
        self.require_calendar_capabilities(&backend, meeting).await?;
        let request_id = step_client_id(&input.idempotency_key, "request")?;
        let cancel_id = step_client_id(&input.idempotency_key, "cancel-removed")?;
        let request_mime = notification(
            &account.email,
            &prepared.event,
            &prepared.event.mutation.application.attendees,
            CalendarMessageMethod::Request,
            "",
        )?;
        let cancel_mime = notification(
            &account.email,
            &old,
            &prepared.removed_attendees,
            CalendarMessageMethod::Cancel,
            "",
        )?;
        let begin = self.begin_write(
            &reference.account_id,
            "calendar_update",
            &input.idempotency_key,
            &input,
        )?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        let updated = match backend.update_calendar_item(&source, &prepared.event.mutation).await {
            Ok(value) => value,
            Err(error) => return self.calendar_failure(&begin.record, 0, error, None),
        };
        let mut steps = STEP_ITEM;
        self.journal.checkpoint(&begin.record.operation_id, steps)?;
        let event_ref = self.references.insert_event(updated)?;
        if let Some(mime) = request_mime {
            if let Err(error) = backend.send_calendar_message(&request_id, mime).await {
                return self.calendar_failure(&begin.record, steps, error, Some(event_ref));
            }
            steps |= STEP_NOTIFY_CURRENT;
            self.journal.checkpoint(&begin.record.operation_id, steps)?;
        }
        if let Some(mime) = cancel_mime {
            if let Err(error) = backend.send_calendar_message(&cancel_id, mime).await {
                return self.calendar_failure(&begin.record, steps, error, Some(event_ref));
            }
            steps |= STEP_NOTIFY_REMOVED;
            self.journal.checkpoint(&begin.record.operation_id, steps)?;
        }
        self.calendar_success(&begin.record, steps, Some(event_ref))
    }

    async fn calendar_delete_result(
        &self,
        input: CalendarDeleteInput,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) =
            self.replay_write("calendar_delete", &input.idempotency_key, &input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        let reference = self.references.event(&input.event_ref)?;
        calendar_prepare::require_non_recurring(&reference)?;
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        self.require_calendar_capabilities(&backend, false).await?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let source = backend.resolve_calendar_source(&reference).await?;
        calendar_prepare::require_non_recurring(&source)?;
        if calendar_prepare::ownership(&source, &account.email) != EventOwnership::Personal {
            return Err(validation("calendar_delete only accepts personal events"));
        }
        let begin = self.begin_write(
            &reference.account_id,
            "calendar_delete",
            &input.idempotency_key,
            &input,
        )?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        if let Err(error) = backend.delete_calendar_item(&source).await {
            return self.calendar_failure(&begin.record, 0, error, None);
        }
        self.journal.checkpoint(&begin.record.operation_id, STEP_ITEM)?;
        self.calendar_success(&begin.record, STEP_ITEM, None)
    }

    async fn calendar_cancel_result(
        &self,
        input: CalendarCancelInput,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if let Some(record) =
            self.replay_write("calendar_cancel", &input.idempotency_key, &input)?
        {
            return Ok((calendar_write_result::existing(record), Vec::new()));
        }
        calendar_prepare::validate_comment(&input.comment)?;
        let reference = self.references.event(&input.event_ref)?;
        calendar_prepare::require_non_recurring(&reference)?;
        let backend = self.require_write(&reference.account_id)?;
        let account = backend.account();
        self.require_calendar_capabilities(&backend, true).await?;
        let _guard = self.write_locks.acquire(&reference.account_id).await?;
        let source = backend.resolve_calendar_source(&reference).await?;
        calendar_prepare::require_non_recurring(&source)?;
        if calendar_prepare::ownership(&source, &account.email) != EventOwnership::Organizer {
            return Err(validation("calendar_cancel requires an organizer meeting"));
        }
        let prepared = calendar_prepare::existing(&source, self.clock.now())?;
        let cancel_id = step_client_id(&input.idempotency_key, "cancel")?;
        let mime = required_notification(
            &account.email,
            &prepared,
            &prepared.mutation.application.attendees,
            CalendarMessageMethod::Cancel,
            &input.comment,
        )?;
        let begin = self.begin_write(
            &reference.account_id,
            "calendar_cancel",
            &input.idempotency_key,
            &input,
        )?;
        if !begin.inserted {
            return Ok((calendar_write_result::existing(begin.record), Vec::new()));
        }
        if let Err(error) = backend.delete_calendar_item(&source).await {
            return self.calendar_failure(&begin.record, 0, error, None);
        }
        let mut steps = STEP_ITEM;
        self.journal.checkpoint(&begin.record.operation_id, steps)?;
        if let Err(error) = backend.send_calendar_message(&cancel_id, mime).await {
            return self.calendar_failure(&begin.record, steps, error, None);
        }
        steps |= STEP_NOTIFY_CURRENT;
        self.journal.checkpoint(&begin.record.operation_id, steps)?;
        self.calendar_success(&begin.record, steps, None)
    }

    pub(super) fn calendar_success(
        &self,
        record: &JournalRecord,
        steps: u32,
        event_ref: Option<String>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        self.journal.finish(&record.operation_id, OperationStatus::Succeeded, steps)?;
        Ok((
            calendar_write_result::result(
                &record.operation_id,
                CalendarOperationState::Succeeded,
                steps,
                "Exchange confirmed every Calendar operation step",
                event_ref,
            ),
            Vec::new(),
        ))
    }

    pub(super) fn calendar_failure(
        &self,
        record: &JournalRecord,
        steps: u32,
        error: AppError,
        event_ref: Option<String>,
    ) -> Result<(CalendarOperationResult, Vec<Warning>)> {
        if error.envelope.code == ErrorCode::RemoteWipe {
            self.purge_account(&record.account_id)?;
            return Err(error.operation(&record.operation_id));
        }
        let (journal_status, result_status, message) = if error.envelope.code
            == ErrorCode::OutcomeUnknown
        {
            (
                OperationStatus::Unknown,
                CalendarOperationState::Unknown,
                "A Calendar operation step may have reached Exchange; do not retry with a new UUID",
            )
        } else if steps == 0 {
            (
                OperationStatus::Failed,
                CalendarOperationState::Failed,
                "Exchange safely rejected the Calendar operation",
            )
        } else {
            (
                OperationStatus::Partial,
                CalendarOperationState::Partial,
                "Some Calendar steps succeeded; do not retry with a new UUID",
            )
        };
        self.journal.finish(&record.operation_id, journal_status, steps)?;
        Ok((
            calendar_write_result::result(
                &record.operation_id,
                result_status,
                steps,
                message,
                event_ref,
            ),
            Vec::new(),
        ))
    }

    pub(super) async fn require_calendar_capabilities(
        &self,
        backend: &Arc<dyn AccountBackend>,
        meeting: bool,
    ) -> Result<()> {
        let account_id = backend.account().account_id;
        let capabilities = self.account_result(&account_id, backend.capabilities().await)?;
        let supported =
            capabilities.personal_calendar_writes && (!meeting || capabilities.meeting_lifecycle);
        if supported {
            Ok(())
        } else {
            let feature = if meeting { "meeting lifecycle" } else { "personal Calendar writes" };
            Err(AppError::new(
                ErrorCode::FeatureUnavailable,
                format!("Exchange does not advertise {feature}"),
            )
            .account(account_id))
        }
    }
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}
