use eas_mail_protocol::{Command, EasError, MeetingResponseChoice};

use super::calendar_write_model::require_status;
use super::session::EasMailbox;
use crate::Result;
use crate::backend::MailSource;

impl EasMailbox {
    pub(super) async fn respond_request(
        &self,
        source: &MailSource,
        response: MeetingResponseChoice,
    ) -> Result<Option<String>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_calendar_capability(&state, Command::MeetingResponse, "MeetingResponse")?;
        let result = match source {
            MailSource::Item { folder_id, server_id } => {
                self.client.meeting_response(state.policy_key, folder_id, server_id, response).await
            }
            MailSource::LongId(long_id) => {
                self.client.meeting_response_long_id(state.policy_key, long_id, response).await
            }
        };
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            match source {
                MailSource::Item { folder_id, server_id } => {
                    self.client
                        .meeting_response(state.policy_key, folder_id, server_id, response)
                        .await
                }
                MailSource::LongId(long_id) => {
                    self.client.meeting_response_long_id(state.policy_key, long_id, response).await
                }
            }
        } else {
            result
        }
        .map_err(self.scoped_error())?;
        require_status(result.status, "MeetingResponse")?;
        Ok(result.calendar_id)
    }
}
