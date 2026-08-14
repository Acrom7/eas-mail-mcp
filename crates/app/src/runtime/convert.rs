use eas_mail_protocol::{CalendarFields, Patch};

use crate::backend::{BackendEvent, BackendMail};
use crate::model::{CalendarEvent, MailDetail, MailSummary};
use crate::sanitize::{plain_text, truncate};
use crate::{Result, Runtime};

impl Runtime {
    pub(super) fn mail_summary(&self, mail: BackendMail) -> Result<MailSummary> {
        let mail_ref = self.references.insert_mail(mail.clone())?;
        Ok(mail_summary(mail_ref, &mail))
    }

    pub(super) fn mail_detail(
        &self,
        mail_ref: String,
        mail: &BackendMail,
        requested_limit: usize,
    ) -> MailDetail {
        let mut summary = mail_summary(mail_ref, mail);
        let body = plain_text(string(&mail.fields.body));
        let (body, application_truncated) = truncate(&body, requested_limit);
        summary.preview = truncate(&body, 500).0;
        MailDetail {
            summary,
            cc: string(&mail.fields.cc).to_owned(),
            body,
            body_truncated: boolean(&mail.fields.body_truncated) || application_truncated,
        }
    }

    pub(super) fn calendar_event(&self, event: BackendEvent) -> Result<CalendarEvent> {
        let event_ref = self.references.insert_event(event.clone())?;
        Ok(calendar_event(event_ref, &event.fields, &event))
    }
}

pub(super) fn calendar_event(
    event_ref: String,
    fields: &CalendarFields,
    event: &BackendEvent,
) -> CalendarEvent {
    CalendarEvent {
        event_ref,
        account_id: event.account_id.clone(),
        folder_id: event.folder_id.clone(),
        subject: string(&fields.subject).to_owned(),
        body: plain_text(string(&fields.body)),
        starts_at: optional_datetime(&fields.starts_at),
        ends_at: optional_datetime(&fields.ends_at),
        all_day: boolean(&fields.all_day),
        location: string(&fields.location).to_owned(),
        organizer: string(&fields.organizer).to_owned(),
        attendees: list(&fields.attendees),
        recurrence: map(&fields.recurrence),
        exceptions: nested_map(&fields.exceptions),
        untrusted_external_content: true,
    }
}

fn mail_summary(mail_ref: String, mail: &BackendMail) -> MailSummary {
    let preview = truncate(&plain_text(string(&mail.fields.body)), 500).0;
    MailSummary {
        mail_ref,
        account_id: mail.account_id.clone(),
        folder_id: mail.folder_id.clone(),
        subject: string(&mail.fields.subject).to_owned(),
        sender: string(&mail.fields.sender).to_owned(),
        recipients: string(&mail.fields.recipients).to_owned(),
        received_at: optional_datetime(&mail.fields.received_at),
        preview,
        is_read: boolean(&mail.fields.is_read),
        has_attachments: !list(&mail.fields.attachments).is_empty(),
        untrusted_external_content: true,
    }
}

pub(super) fn string(value: &Patch<String>) -> &str {
    match value {
        Patch::Value(value) => value,
        Patch::Missing => "",
    }
}

pub(super) fn boolean(value: &Patch<bool>) -> bool {
    matches!(value, Patch::Value(true))
}

pub(super) fn list<T: Clone>(value: &Patch<Vec<T>>) -> Vec<T> {
    match value {
        Patch::Value(value) => value.clone(),
        Patch::Missing => Vec::new(),
    }
}

pub(super) fn folder_role(folder_type: u16) -> &'static str {
    match folder_type {
        2 => "inbox",
        3 => "drafts",
        4 => "trash",
        5 => "sent",
        6 => "outbox",
        8 => "calendar",
        12 => "user_mail",
        13 => "user_calendar",
        _ => "other",
    }
}

fn map(
    value: &Patch<std::collections::BTreeMap<String, String>>,
) -> std::collections::BTreeMap<String, String> {
    match value {
        Patch::Value(value) => value.clone(),
        Patch::Missing => std::collections::BTreeMap::new(),
    }
}

fn nested_map(
    value: &Patch<Vec<std::collections::BTreeMap<String, String>>>,
) -> Vec<std::collections::BTreeMap<String, String>> {
    list(value)
}

fn optional_datetime(
    value: &Patch<Option<chrono::DateTime<chrono::Utc>>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match value {
        Patch::Value(value) => *value,
        Patch::Missing => None,
    }
}
