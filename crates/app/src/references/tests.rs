use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, Utc};
use eas_mail_protocol::{CalendarFields, MailFields};

use super::*;
use crate::backend::MailSource;

#[derive(Debug)]
struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    fn advance(&self, duration: Duration) -> Result<()> {
        let mut now = self.0.lock().map_err(|_| state_error())?;
        *now += duration;
        Ok(())
    }

    fn set(&self, value: DateTime<Utc>) -> Result<()> {
        *self.0.lock().map_err(|_| state_error())? = value;
        Ok(())
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        self.0.lock().map_or(DateTime::UNIX_EPOCH, |value| *value)
    }
}

#[derive(Debug, Default)]
struct SequenceIds(AtomicU64);

impl IdGenerator for SequenceIds {
    fn next(&self) -> String {
        self.0.fetch_add(1, Ordering::Relaxed).to_string()
    }
}

#[test]
fn insertion_prunes_all_expired_reference_kinds() -> Result<()> {
    let clock = Arc::new(ManualClock(Mutex::new(DateTime::UNIX_EPOCH)));
    let references = References::new(clock.clone(), Arc::new(SequenceIds::default()));

    references.insert_mail(mail("old"))?;
    references.insert_event(event("old"))?;
    references.insert_attachment(attachment("old"))?;
    let (_, old_cursor) = references.first_mail_page(summaries(), 1)?;
    clock.advance(Duration::minutes(LIFETIME_MINUTES - 1))?;

    let unexpired = references.insert_mail(mail("unexpired"))?;
    let (_, unexpired_cursor) = references.first_mail_page(summaries(), 1)?;
    {
        let state = references.lock()?;
        assert_eq!(state.mail.len(), 2);
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.attachments.len(), 1);
        assert!(state.cursors.contains_key(&required(old_cursor)?));
    }
    clock.advance(Duration::minutes(1))?;

    let current = references.insert_mail(mail("current"))?;
    let state = references.lock()?;
    assert_eq!(state.mail.len(), 2);
    assert!(state.mail.contains_key(&unexpired));
    assert!(state.mail.contains_key(&current));
    assert!(state.events.is_empty());
    assert!(state.attachments.is_empty());
    assert_eq!(state.cursors.len(), 1);
    assert!(state.cursors.contains_key(&required(unexpired_cursor)?));
    Ok(())
}

#[test]
fn backward_clock_jump_restarts_the_prune_interval() -> Result<()> {
    let future = DateTime::UNIX_EPOCH + Duration::hours(1);
    let clock = Arc::new(ManualClock(Mutex::new(future)));
    let references = References::new(clock.clone(), Arc::new(SequenceIds::default()));
    references.insert_mail(mail("future"))?;

    clock.set(DateTime::UNIX_EPOCH)?;
    references.insert_mail(mail("after-jump"))?;
    assert_eq!(references.lock()?.last_pruned_at, Some(DateTime::UNIX_EPOCH));
    Ok(())
}

fn summaries() -> Vec<MailSummary> {
    ["first", "second"]
        .into_iter()
        .map(|mail_ref| MailSummary {
            mail_ref: mail_ref.into(),
            account_id: "account".into(),
            folder_id: "inbox".into(),
            subject: String::new(),
            sender: String::new(),
            recipients: String::new(),
            received_at: None,
            preview: String::new(),
            is_read: false,
            has_attachments: false,
            untrusted_external_content: true,
        })
        .collect()
}

fn required(value: Option<String>) -> Result<String> {
    value.ok_or_else(|| AppError::new(ErrorCode::ProtocolError, "test cursor is missing"))
}

fn mail(server_id: &str) -> BackendMail {
    BackendMail {
        account_id: "account".into(),
        folder_id: "inbox".into(),
        source: MailSource::Item { folder_id: "inbox".into(), server_id: server_id.into() },
        fields: MailFields::default(),
    }
}

fn event(server_id: &str) -> BackendEvent {
    BackendEvent {
        account_id: "account".into(),
        long_id: server_id.into(),
        collection_id: None,
        server_id: None,
        fields: CalendarFields::default(),
    }
}

fn attachment(file_reference: &str) -> AttachmentReference {
    AttachmentReference {
        account_id: "account".into(),
        file_reference: file_reference.into(),
        display_name: "file.txt".into(),
        content_type: "text/plain".into(),
        size: 1,
        is_inline: false,
    }
}

fn state_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "test clock is unavailable")
}
