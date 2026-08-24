use super::Runtime;
use super::convert::{boolean, string};
use super::outgoing::{forward_message, reply_message, validate_message};
use super::write_preview::{PreparedWrite, WritePreview};
use super::writes::existing_result;
use crate::Result;
use crate::backend::{BackendMail, OutgoingMail};
use crate::model::{
    MailForwardInput, MailReplyInput, MailSendInput, MarkReadInput, OperationResult,
};

impl Runtime {
    pub(crate) async fn prepare_cli_mail_mark_read(
        &self,
        input: &MarkReadInput,
    ) -> Result<PreparedWrite<OperationResult>> {
        if let Some(record) = self.replay_write("mail_mark_read", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(existing_result(record)));
        }
        let reference = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&reference.account_id)?;
        let mail = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 1).await,
        )?;
        Ok(PreparedWrite::Ready(mark_read_preview(&mail, input.is_read)))
    }

    pub(crate) fn prepare_cli_mail_send(
        &self,
        input: &MailSendInput,
    ) -> Result<PreparedWrite<OperationResult>> {
        if let Some(record) = self.replay_write("mail_send", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(existing_result(record)));
        }
        let message = send_message(input);
        validate_message(&message)?;
        self.require_write(&input.account_id)?;
        Ok(PreparedWrite::Ready(message_preview("mail_send", &input.account_id, &message)))
    }

    pub(crate) async fn prepare_cli_mail_reply(
        &self,
        input: &MailReplyInput,
    ) -> Result<PreparedWrite<OperationResult>> {
        if let Some(record) = self.replay_write("mail_reply", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(existing_result(record)));
        }
        let reference = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&reference.account_id)?;
        let mail = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 1).await,
        )?;
        let message = reply_message(&mail, &backend.account().email, input)?;
        validate_message(&message)?;
        Ok(PreparedWrite::Ready(message_preview("mail_reply", &reference.account_id, &message)))
    }

    pub(crate) async fn prepare_cli_mail_forward(
        &self,
        input: &MailForwardInput,
    ) -> Result<PreparedWrite<OperationResult>> {
        if let Some(record) = self.replay_write("mail_forward", &input.idempotency_key, input)? {
            return Ok(PreparedWrite::Replay(existing_result(record)));
        }
        let reference = self.references.mail(&input.mail_ref)?;
        let backend = self.require_write(&reference.account_id)?;
        let mail = self.account_result(
            &reference.account_id,
            backend.fetch_mail(&reference.source, 1).await,
        )?;
        let message = forward(&mail, input);
        validate_message(&message)?;
        Ok(PreparedWrite::Ready(message_preview("mail_forward", &reference.account_id, &message)))
    }
}

pub(super) fn mark_read_preview(mail: &BackendMail, is_read: bool) -> WritePreview {
    WritePreview::new("mail_mark_read", mail.account_id.clone())
        .field("Sender", string(&mail.fields.sender))
        .field("Subject", string(&mail.fields.subject))
        .field("Current read state", boolean(&mail.fields.is_read).to_string())
        .field("New read state", is_read.to_string())
}

pub(super) fn send_message(input: &MailSendInput) -> OutgoingMail {
    OutgoingMail {
        to: input.to.clone(),
        cc: input.cc.clone(),
        bcc: input.bcc.clone(),
        subject: input.subject.clone(),
        body: input.body.clone(),
    }
}

pub(super) fn forward(mail: &BackendMail, input: &MailForwardInput) -> OutgoingMail {
    let mut message = forward_message(mail, &input.body);
    message.to.clone_from(&input.to);
    message.cc.clone_from(&input.cc);
    message.bcc.clone_from(&input.bcc);
    message
}

pub(super) fn message_preview(
    operation: &'static str,
    account_id: &str,
    message: &OutgoingMail,
) -> WritePreview {
    WritePreview::new(operation, account_id.to_owned())
        .field("To", message.to.join(", "))
        .field("Cc", message.cc.join(", "))
        .field("Bcc", message.bcc.join(", "))
        .field("Subject", &message.subject)
        .field("Body", &message.body)
}
