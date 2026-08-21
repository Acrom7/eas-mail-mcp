use std::sync::Arc;

use async_trait::async_trait;
use eas_mail_protocol::ProfileRegistry;

use super::super::terminal::Terminal;
use super::input::report_stage;
use crate::backend::{EasMailbox, VerificationStage};
use crate::{AccountConfig, Result, SecretStore};

#[async_trait(?Send)]
pub(super) trait AccountVerifier {
    async fn verify(
        &self,
        account_id: &str,
        account: &AccountConfig,
        secrets: Arc<dyn SecretStore>,
        profiles: &ProfileRegistry,
        terminal: Option<&mut dyn Terminal>,
    ) -> Result<(usize, bool)>;
}

pub(super) struct EasAccountVerifier;

#[async_trait(?Send)]
impl AccountVerifier for EasAccountVerifier {
    async fn verify(
        &self,
        account_id: &str,
        account: &AccountConfig,
        secrets: Arc<dyn SecretStore>,
        profiles: &ProfileRegistry,
        mut terminal: Option<&mut dyn Terminal>,
    ) -> Result<(usize, bool)> {
        let mailbox =
            EasMailbox::production(account_id.to_owned(), account.clone(), secrets, profiles)?;
        report_stage(&mut terminal, VerificationStage::Profile)?;
        let mut progress = |stage| report_stage(&mut terminal, stage);
        mailbox.verification_result_with_progress(&mut progress).await
    }
}
