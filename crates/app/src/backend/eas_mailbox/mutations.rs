use eas_mail_protocol::protocol::{ComposeSource, build_mime_message};
use eas_mail_protocol::{Command, EasError, Patch};

use super::super::{MailSource, OutgoingMail};
use super::session::EasMailbox;
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn change_read(&self, source: &MailSource, is_read: bool) -> Result<()> {
        let MailSource::Item { folder_id, server_id } = source else {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "read state requires a message returned by mail_list",
            ));
        };
        self.sync_mail_selected(false, Some(std::slice::from_ref(folder_id))).await?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(&state, Command::SendMail)?;
        let sync_key = state
            .collections
            .get(folder_id)
            .map(|collection| collection.sync_key.clone())
            .ok_or_else(|| {
                AppError::new(ErrorCode::SyncStale, "mail collection is not synchronized")
            })?;
        let result =
            self.client.mark_read(state.policy_key, folder_id, server_id, &sync_key, is_read).await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.mark_read(state.policy_key, folder_id, server_id, &sync_key, is_read).await
        } else {
            result
        }
        .map_err(self.mutation_error())?;
        require_success(result.status)?;
        let collection = state.collections.get_mut(folder_id).ok_or_else(|| {
            AppError::new(ErrorCode::SyncStale, "mail collection is not synchronized")
        })?;
        if let Some(sync_key) = result.sync_key {
            collection.sync_key = sync_key;
        }
        if let Some(fields) = collection.mail.get_mut(server_id) {
            fields.is_read = Patch::Value(is_read);
        }
        Ok(())
    }

    pub(super) async fn send_message(&self, client_id: &str, message: &OutgoingMail) -> Result<()> {
        let mime = self.mime(message)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(&state, Command::SendMail)?;
        let result = self.client.send(state.policy_key, client_id, mime.clone()).await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.send(state.policy_key, client_id, mime).await
        } else {
            result
        }
        .map_err(self.mutation_error())?;
        require_success(result.status)
    }

    pub(super) async fn compose(
        &self,
        forward: bool,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()> {
        let mime = self.mime(message)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(
            &state,
            if forward { Command::SmartForward } else { Command::SmartReply },
        )?;
        let source = compose_source(source);
        let result = self
            .client
            .smart_compose(state.policy_key, forward, client_id, source.clone(), mime.clone())
            .await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.smart_compose(state.policy_key, forward, client_id, source, mime).await
        } else {
            result
        }
        .map_err(self.mutation_error())?;
        require_success(result.status)
    }

    fn mime(&self, message: &OutgoingMail) -> Result<Vec<u8>> {
        build_mime_message(
            &self.account.email,
            &message.to,
            &message.cc,
            &message.bcc,
            &message.subject,
            &message.body,
        )
        .map_err(self.scoped_error())
    }

    fn mutation_error(&self) -> impl FnOnce(EasError) -> AppError + '_ {
        |error| AppError::from(error).account(&self.account.account_id)
    }
}

fn compose_source(source: &MailSource) -> ComposeSource<'_> {
    match source {
        MailSource::Item { folder_id, server_id } => {
            ComposeSource::Item { folder_id, item_id: server_id }
        }
        MailSource::LongId(long_id) => ComposeSource::LongId(long_id),
    }
}

fn require_success(status: u16) -> Result<()> {
    if status == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::ProtocolError,
            format!("Exchange rejected the mail mutation with status {status}"),
        ))
    }
}
