use serde::Serialize;

use super::Runtime;
use super::convert::string;
use crate::backend::{BackendMail, OutgoingMail};
use crate::journal::payload_fingerprint;
use crate::model::{
    MailForwardInput, MailReplyInput, MailSendInput, MarkReadInput, OperationResult, OperationState,
};
use crate::sanitize::mailbox;
use crate::{
    ApiResponse, AppError, ErrorCode, JournalBegin, JournalRecord, OperationStatus, Result,
};

impl Runtime {
    /// Changes read state through an idempotent EAS Sync mutation.
    pub async fn mail_mark_read(&self, input: MarkReadInput) -> ApiResponse<OperationResult> {
        Self::response(self.mark_read_result(input).await)
    }

    /// Sends one plain-text message with durable idempotency metadata.
    pub async fn mail_send(&self, input: MailSendInput) -> ApiResponse<OperationResult> {
        Self::response(self.send_result(input).await)
    }

    /// Replies to a process-local source message.
    pub async fn mail_reply(&self, input: MailReplyInput) -> ApiResponse<OperationResult> {
        Self::response(self.reply_result(input).await)
    }

    /// Forwards a process-local source message.
    pub async fn mail_forward(&self, input: MailForwardInput) -> ApiResponse<OperationResult> {
        Self::response(self.forward_result(input).await)
    }

    async fn mark_read_result(
        &self,
        input: MarkReadInput,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        let mail = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&mail.account_id)?;
        let begin =
            self.begin_write(&mail.account_id, "mail_mark_read", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.mark_read(&mail.source, input.is_read).await;
        self.finish_write(&mail.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    async fn send_result(
        &self,
        input: MailSendInput,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        validate_message(&input.to, &input.cc, &input.bcc, &input.subject, &input.body)?;
        let backend = self.require_write(&input.account_id)?;
        let begin =
            self.begin_write(&input.account_id, "mail_send", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let message = OutgoingMail {
            to: input.to,
            cc: input.cc,
            bcc: input.bcc,
            subject: input.subject,
            body: input.body,
        };
        let result = backend.send(&begin.record.client_id, &message).await;
        self.finish_write(&input.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    async fn reply_result(
        &self,
        input: MailReplyInput,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        let mail = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&mail.account_id)?;
        let message = reply_message(&mail, &backend.account().email, &input)?;
        validate_message(&message.to, &message.cc, &message.bcc, &message.subject, &message.body)?;
        let begin =
            self.begin_write(&mail.account_id, "mail_reply", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.reply(&begin.record.client_id, &mail.source, &message).await;
        self.finish_write(&mail.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    async fn forward_result(
        &self,
        input: MailForwardInput,
    ) -> Result<(OperationResult, Vec<crate::Warning>)> {
        let mail = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&mail.account_id)?;
        let message = OutgoingMail {
            to: input.to.clone(),
            cc: input.cc.clone(),
            bcc: input.bcc.clone(),
            subject: prefix_subject("Fwd:", string(&mail.fields.subject)),
            body: input.body.clone(),
        };
        validate_message(&message.to, &message.cc, &message.bcc, &message.subject, &message.body)?;
        let begin =
            self.begin_write(&mail.account_id, "mail_forward", &input.idempotency_key, &input)?;
        if !begin.inserted {
            return Ok((existing_result(begin.record), Vec::new()));
        }
        let result = backend.forward(&begin.record.client_id, &mail.source, &message).await;
        self.finish_write(&mail.account_id, &begin.record.operation_id, result)
            .map(|value| (value, Vec::new()))
    }

    fn begin_write<T: Serialize>(
        &self,
        account_id: &str,
        kind: &str,
        operation_id: &str,
        payload: &T,
    ) -> Result<JournalBegin> {
        let parsed = uuid::Uuid::parse_str(operation_id).map_err(|_| {
            AppError::new(ErrorCode::ValidationFailed, "idempotency_key must be a UUID")
        })?;
        let canonical = serde_json::to_vec(payload).map_err(|_| {
            AppError::new(ErrorCode::ValidationFailed, "cannot canonicalize operation input")
        })?;
        let record = JournalRecord {
            operation_id: parsed.to_string(),
            account_id: account_id.to_owned(),
            kind: kind.to_owned(),
            payload_hmac: payload_fingerprint(&self.hmac_key, &canonical)?,
            client_id: parsed.to_string(),
            status: OperationStatus::Pending,
        };
        self.journal.begin(&record)
    }

    fn finish_write(
        &self,
        account_id: &str,
        operation_id: &str,
        result: Result<()>,
    ) -> Result<OperationResult> {
        match result {
            Ok(()) => {
                self.journal.finish(operation_id, OperationStatus::Succeeded)?;
                Ok(OperationResult {
                    operation_id: operation_id.into(),
                    status: OperationState::Succeeded,
                    message: "Exchange confirmed the operation".into(),
                })
            }
            Err(error) if error.envelope.code == ErrorCode::OutcomeUnknown => {
                self.journal.finish(operation_id, OperationStatus::Unknown)?;
                Err(error.operation(operation_id))
            }
            Err(error) if error.envelope.code == ErrorCode::RemoteWipe => {
                self.purge_account(account_id)?;
                Err(error.operation(operation_id))
            }
            Err(error) => {
                self.journal.finish(operation_id, OperationStatus::Failed)?;
                Err(error.operation(operation_id))
            }
        }
    }
}

fn existing_result(record: JournalRecord) -> OperationResult {
    let (status, message) = match record.status {
        OperationStatus::Succeeded => {
            (OperationState::Succeeded, "the prior operation was confirmed")
        }
        OperationStatus::Failed => (OperationState::Failed, "the prior operation failed safely"),
        OperationStatus::Pending | OperationStatus::Unknown => {
            (OperationState::Unknown, "the prior operation outcome is unknown")
        }
    };
    OperationResult { operation_id: record.operation_id, status, message: message.into() }
}

fn reply_message(
    mail: &BackendMail,
    own_email: &str,
    input: &MailReplyInput,
) -> Result<OutgoingMail> {
    let sender = mailbox(string(&mail.fields.sender));
    if sender.is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the source message has no reply recipient",
        ));
    }
    let mut to = vec![sender];
    let mut cc = Vec::new();
    if input.reply_all {
        let mut additional = addresses(string(&mail.fields.recipients));
        remove_own_and_duplicates(&mut additional, own_email);
        to.extend(additional);
        cc.extend(addresses(string(&mail.fields.cc)));
    }
    deduplicate(&mut to);
    remove_own_and_duplicates(&mut cc, own_email);
    Ok(OutgoingMail {
        to,
        cc,
        bcc: Vec::new(),
        subject: prefix_subject("Re:", string(&mail.fields.subject)),
        body: input.body.clone(),
    })
}

fn deduplicate(values: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| seen.insert(value.to_ascii_lowercase()));
}

fn addresses(value: &str) -> Vec<String> {
    value.split([',', ';']).map(mailbox).filter(|value| !value.is_empty()).collect()
}

fn remove_own_and_duplicates(values: &mut Vec<String>, own_email: &str) {
    let own_email = own_email.to_ascii_lowercase();
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| {
        let normalized = value.to_ascii_lowercase();
        normalized != own_email && seen.insert(normalized)
    });
}

fn prefix_subject(prefix: &str, subject: &str) -> String {
    if subject.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()) {
        subject.to_owned()
    } else {
        format!("{prefix} {subject}")
    }
}

fn validate_message(
    to: &[String],
    cc: &[String],
    bcc: &[String],
    subject: &str,
    body: &str,
) -> Result<()> {
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "at least one recipient is required",
        ));
    }
    if subject.chars().count() > 998 || body.len() > 1024 * 1024 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "subject or body exceeds the supported limit",
        ));
    }
    for address in to.iter().chain(cc).chain(bcc) {
        if !address.contains('@') || address.contains(['\r', '\n']) {
            return Err(AppError::new(ErrorCode::ValidationFailed, "recipient address is invalid"));
        }
    }
    Ok(())
}
