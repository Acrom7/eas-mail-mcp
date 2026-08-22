use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use eas_mail_mcp::backend::{
    AccountBackend, BackendAccount, BackendCalendarMutation, BackendCalendarSearch,
    BackendCapabilities, BackendEvent, BackendMail, BackendSync, MailSource, OutgoingMail,
};
use eas_mail_mcp::{AppError, ErrorCode, Result};
use eas_mail_protocol::{
    Attachment, CalendarApplication, CalendarAttendee, CalendarFields, CandidateAvailability,
    CollectionKind, Folder, FreeBusyStatus, MailFields, MeetingResponseChoice, Patch, ProfileKey,
    RecipientAvailability, RecipientResolution, ResolvedRecipient,
};

/// Deterministic high-level backend used by MCP black-box tests.
#[derive(Debug)]
pub struct FakeBackend {
    account: BackendAccount,
    failure: Mutex<Option<ErrorCode>>,
    operation_failure: Mutex<Option<(String, ErrorCode)>>,
    mail_count: usize,
    operations: Mutex<Vec<String>>,
    source_resolutions: AtomicUsize,
    capabilities: BackendCapabilities,
    delay: Duration,
}

impl FakeBackend {
    /// Creates a successful backend with write tools enabled.
    #[must_use]
    pub fn new(account_id: &str) -> Self {
        Self {
            account: BackendAccount {
                account_id: account_id.into(),
                profile: ProfileKey::default(),
                email: format!("{account_id}@example.invalid"),
                email_domains: vec!["example.invalid".into()],
                enabled: true,
                write_enabled: true,
            },
            failure: Mutex::new(None),
            operation_failure: Mutex::new(None),
            mail_count: 1,
            operations: Mutex::new(Vec::new()),
            source_resolutions: AtomicUsize::new(0),
            capabilities: BackendCapabilities {
                calendar_availability: true,
                mail_writes: true,
                personal_calendar_writes: true,
                meeting_lifecycle: true,
            },
            delay: Duration::ZERO,
        }
    }

    /// Creates a backend that returns a retryable network error.
    #[must_use]
    pub fn failing(account_id: &str) -> Self {
        Self { failure: Mutex::new(Some(ErrorCode::NetworkUnreachable)), ..Self::new(account_id) }
    }

    /// Configures the number of deterministic messages returned by list and search.
    #[must_use]
    pub const fn with_mail_count(mut self, count: usize) -> Self {
        self.mail_count = count;
        self
    }

    /// Enables or disables account-level write tools.
    #[must_use]
    pub const fn with_writes_enabled(mut self, enabled: bool) -> Self {
        self.account.write_enabled = enabled;
        self
    }

    /// Replaces safe account identity metadata for account-selection tests.
    #[must_use]
    pub fn with_identity(mut self, email: &str, domains: &[&str]) -> Self {
        self.account.email = email.into();
        self.account.email_domains = domains.iter().map(|value| (*value).into()).collect();
        self
    }

    /// Adds deterministic latency to each asynchronous backend operation.
    #[must_use]
    pub const fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Replaces Calendar write capability flags for preflight tests.
    #[must_use]
    pub const fn with_calendar_capabilities(mut self, personal: bool, meeting: bool) -> Self {
        self.capabilities.personal_calendar_writes = personal;
        self.capabilities.meeting_lifecycle = meeting;
        self
    }

    /// Selects a deterministic account failure or restores normal operation.
    pub fn set_failure(&self, value: Option<ErrorCode>) -> Result<()> {
        *self.failure.lock().map_err(|_| failure(ErrorCode::StorageError))? = value;
        Ok(())
    }

    /// Fails one named operation until the failure is explicitly cleared.
    pub fn set_operation_failure(&self, name: Option<&str>, code: ErrorCode) -> Result<()> {
        *self.operation_failure.lock().map_err(|_| failure(ErrorCode::StorageError))? =
            name.map(|value| (value.to_owned(), code));
        Ok(())
    }

    /// Returns mutation names recorded by the fake backend.
    pub fn operations(&self) -> Result<Vec<String>> {
        self.operations
            .lock()
            .map(|values| values.clone())
            .map_err(|_| failure(ErrorCode::StorageError))
    }

    /// Returns how many mutable-source resolutions were attempted.
    #[must_use]
    pub fn source_resolutions(&self) -> usize {
        self.source_resolutions.load(Ordering::Relaxed)
    }

    async fn check(&self) -> Result<()> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.failure
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .map_or(Ok(()), |code| Err(failure(code)))
    }

    async fn check_operation(&self, name: &str) -> Result<()> {
        self.check().await?;
        let scripted =
            self.operation_failure.lock().map_err(|_| failure(ErrorCode::StorageError))?.clone();
        match scripted {
            Some((expected, code)) if expected == name => Err(failure(code)),
            _ => Ok(()),
        }
    }

    fn record(&self, value: &str) -> Result<()> {
        self.operations.lock().map_err(|_| failure(ErrorCode::StorageError))?.push(value.into());
        Ok(())
    }
}

#[async_trait]
impl AccountBackend for FakeBackend {
    fn account(&self) -> BackendAccount {
        self.account.clone()
    }

    async fn capabilities(&self) -> Result<BackendCapabilities> {
        self.check().await?;
        Ok(self.capabilities)
    }

    async fn folders(&self) -> Result<Vec<Folder>> {
        self.check().await?;
        Ok(folders())
    }

    async fn sync_mail(&self) -> Result<BackendSync> {
        self.check().await?;
        Ok(BackendSync { collections: 1, changes: 1 })
    }

    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>> {
        self.check().await?;
        Ok(if folder_ids.is_none_or(|ids| ids.iter().any(|id| id == "inbox")) {
            (0..self.mail_count)
                .map(|index| {
                    mail(
                        &self.account.account_id,
                        MailSource::Item {
                            folder_id: "inbox".into(),
                            server_id: format!("message-{index}"),
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        })
    }

    async fn search_mail(&self, _: &str, _: usize) -> Result<Vec<BackendMail>> {
        self.check().await?;
        Ok((0..self.mail_count)
            .map(|index| {
                mail(&self.account.account_id, MailSource::LongId(format!("long-message-{index}")))
            })
            .collect())
    }

    async fn fetch_mail(&self, source: &MailSource, _: usize) -> Result<BackendMail> {
        self.check().await?;
        Ok(mail(&self.account.account_id, source.clone()))
    }

    async fn fetch_attachment(&self, _: &str) -> Result<Vec<u8>> {
        self.check().await?;
        Ok(b"attachment payload".to_vec())
    }

    async fn calendar_availability(
        &self,
        participants: &[String],
        starts_at: chrono::DateTime<chrono::Utc>,
        ends_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<RecipientAvailability>> {
        self.check().await?;
        let milliseconds = ends_at.signed_duration_since(starts_at).num_milliseconds();
        let slots = milliseconds
            .saturating_add(1_799_999)
            .checked_div(1_800_000)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| failure(ErrorCode::ProtocolError))?;
        Ok(participants
            .iter()
            .map(|input| RecipientAvailability {
                input: input.clone(),
                resolution: RecipientResolution::Resolved,
                total_candidates: 1,
                candidates: vec![ResolvedRecipient {
                    recipient_type: 1,
                    display_name: "Test User".into(),
                    email: input.clone(),
                    availability: CandidateAvailability::Slots(vec![FreeBusyStatus::Free; slots]),
                }],
            })
            .collect())
    }

    async fn search_calendar(&self, query: &str, limit: usize) -> Result<BackendCalendarSearch> {
        self.check().await?;
        let events = (limit > 0)
            .then(|| {
                if query == "received" {
                    received_event(&self.account.account_id)
                } else if query == "recurring" {
                    recurring_event(&self.account.account_id)
                } else {
                    event(&self.account.account_id)
                }
            })
            .into_iter()
            .collect();
        Ok(BackendCalendarSearch { events, total: 1 })
    }

    async fn fetch_calendar(&self, source: &BackendEvent, _: usize) -> Result<BackendEvent> {
        self.check().await?;
        Ok(source.clone())
    }

    async fn resolve_calendar_source(&self, source: &BackendEvent) -> Result<BackendEvent> {
        self.source_resolutions.fetch_add(1, Ordering::Relaxed);
        self.check().await?;
        let mut output = source.clone();
        output.collection_id.get_or_insert_with(|| "calendar".into());
        output.server_id.get_or_insert_with(|| "event-1".into());
        Ok(output)
    }

    async fn create_calendar_item(
        &self,
        _: &str,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.check_operation("calendar_create_item").await?;
        self.record("calendar_create_item")?;
        Ok(event_from_application(&self.account.account_id, &item.application))
    }

    async fn update_calendar_item(
        &self,
        source: &BackendEvent,
        item: &BackendCalendarMutation,
    ) -> Result<BackendEvent> {
        self.check_operation("calendar_update_item").await?;
        self.record("calendar_update_item")?;
        let mut output = event_from_application(&self.account.account_id, &item.application);
        output.collection_id.clone_from(&source.collection_id);
        output.server_id.clone_from(&source.server_id);
        Ok(output)
    }

    async fn delete_calendar_item(&self, _: &BackendEvent) -> Result<()> {
        self.check_operation("calendar_delete_item").await?;
        self.record("calendar_delete_item")
    }

    async fn respond_calendar_item(
        &self,
        _: &BackendEvent,
        _: MeetingResponseChoice,
    ) -> Result<Option<String>> {
        self.check_operation("calendar_respond_item").await?;
        self.record("calendar_respond_item")?;
        Ok(Some("responded-event".into()))
    }

    async fn send_calendar_message(&self, _: &str, _: Vec<u8>) -> Result<()> {
        self.check_operation("calendar_send").await?;
        self.record("calendar_send")
    }

    async fn mark_read(&self, _: &MailSource, _: bool) -> Result<()> {
        self.check().await?;
        self.record("mail_mark_read")
    }

    async fn send(&self, _: &str, _: &OutgoingMail) -> Result<()> {
        self.check().await?;
        self.record("mail_send")
    }

    async fn reply(&self, _: &str, _: &MailSource, _: &OutgoingMail) -> Result<()> {
        self.check().await?;
        self.record("mail_reply")
    }

    async fn forward(&self, _: &str, _: &MailSource, _: &OutgoingMail) -> Result<()> {
        self.check().await?;
        self.record("mail_forward")
    }
}

fn folders() -> Vec<Folder> {
    vec![
        Folder {
            server_id: "inbox".into(),
            parent_id: "0".into(),
            display_name: "Inbox".into(),
            folder_type: 2,
            kind: Some(CollectionKind::Mail),
        },
        Folder {
            server_id: "calendar".into(),
            parent_id: "0".into(),
            display_name: "Calendar".into(),
            folder_type: 8,
            kind: Some(CollectionKind::Calendar),
        },
    ]
}

fn mail(account_id: &str, source: MailSource) -> BackendMail {
    BackendMail {
        account_id: account_id.into(),
        folder_id: match &source {
            MailSource::Item { folder_id, .. } => folder_id.clone(),
            MailSource::LongId(_) => String::new(),
        },
        source,
        fields: MailFields {
            subject: Patch::Value("Quarterly update".into()),
            sender: Patch::Value("Sender <sender@example.invalid>".into()),
            recipients: Patch::Value(format!("{account_id}@example.invalid")),
            cc: Patch::Value(String::new()),
            received_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_000_000, 0)),
            body: Patch::Value("<p>Safe <strong>plain</strong> body</p>".into()),
            body_truncated: Patch::Value(false),
            is_read: Patch::Value(false),
            importance: Patch::Value(1),
            attachments: Patch::Value(vec![Attachment {
                display_name: "report.txt".into(),
                file_reference: "attachment-1".into(),
                size: 18,
                content_type: "text/plain".into(),
                is_inline: false,
                content_id: String::new(),
            }]),
        },
    }
}

fn event(account_id: &str) -> BackendEvent {
    BackendEvent {
        account_id: account_id.into(),
        long_id: "event-1".into(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-1".into()),
        fields: CalendarFields {
            subject: Patch::Value("Planning".into()),
            body: Patch::Value("<p>Agenda</p>".into()),
            body_truncated: Patch::Value(false),
            starts_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_010_000, 0)),
            ends_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_013_600, 0)),
            all_day: Patch::Value(false),
            location: Patch::Value("Room 1".into()),
            organizer: Patch::Value("owner@example.invalid".into()),
            organizer_email: Patch::Value(format!("{account_id}@example.invalid")),
            attendees: Patch::Value(vec![CalendarAttendee {
                email: "guest@example.invalid".into(),
                name: "Guest".into(),
                attendee_type: 1,
                attendee_status: 0,
            }]),
            reminder_minutes: Patch::Value(15),
            recurrence: Patch::Value(BTreeMap::new()),
            exceptions: Patch::Value(Vec::new()),
            meeting_status: Patch::Value(1),
            uid: Patch::Value("event-uid@example.invalid".into()),
            dt_stamp: Patch::Value(chrono::DateTime::from_timestamp(1_700_000_000, 0)),
            time_zone: Patch::Value("AAAA".into()),
            busy_status: Patch::Value(2),
            response_requested: Patch::Value(true),
            response_type: Patch::Value(5),
        },
    }
}

fn event_from_application(account_id: &str, item: &CalendarApplication) -> BackendEvent {
    BackendEvent {
        account_id: account_id.into(),
        long_id: String::new(),
        collection_id: Some("calendar".into()),
        server_id: Some("event-created".into()),
        fields: CalendarFields {
            subject: Patch::Value(item.subject.clone()),
            body: Patch::Value(item.body.clone()),
            body_truncated: Patch::Value(false),
            starts_at: Patch::Value(Some(item.starts_at)),
            ends_at: Patch::Value(Some(item.ends_at)),
            all_day: Patch::Value(item.all_day),
            location: Patch::Value(item.location.clone()),
            organizer_email: Patch::Value(format!("{account_id}@example.invalid")),
            attendees: Patch::Value(item.attendees.clone()),
            reminder_minutes: item.reminder_minutes.map_or(Patch::Missing, Patch::Value),
            recurrence: Patch::Value(BTreeMap::new()),
            exceptions: Patch::Value(Vec::new()),
            meeting_status: Patch::Value(item.meeting_status),
            uid: Patch::Value(item.uid.clone()),
            dt_stamp: Patch::Value(Some(item.dt_stamp)),
            time_zone: Patch::Value(item.time_zone.clone()),
            busy_status: Patch::Value(item.busy_status),
            response_requested: Patch::Value(item.response_requested),
            ..CalendarFields::default()
        },
    }
}

fn received_event(account_id: &str) -> BackendEvent {
    let mut value = event(account_id);
    value.long_id = "received-event".into();
    value.server_id = Some("received-event".into());
    value.fields.organizer = Patch::Value("External Organizer".into());
    value.fields.organizer_email = Patch::Value("organizer@example.invalid".into());
    value.fields.meeting_status = Patch::Value(3);
    value
}

fn recurring_event(account_id: &str) -> BackendEvent {
    let mut value = event(account_id);
    value.long_id = "recurring-event".into();
    value.server_id = Some("recurring-event".into());
    value.fields.recurrence = Patch::Value(BTreeMap::from([("type".into(), "1".into())]));
    value
}

fn failure(code: ErrorCode) -> AppError {
    let error = AppError::new(code, "scripted backend is unavailable");
    if code == ErrorCode::NetworkUnreachable { error.retryable() } else { error }
}
