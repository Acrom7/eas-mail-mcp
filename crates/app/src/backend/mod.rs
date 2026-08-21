//! Account-scoped mailbox boundary and its production EAS implementation.

mod eas_mailbox;
mod model;

use async_trait::async_trait;

use crate::Result;

pub use eas_mailbox::EasMailbox;
pub(crate) use eas_mailbox::VerificationStage;
pub use model::{
    BackendAccount, BackendCalendarSearch, BackendCapabilities, BackendEvent, BackendMail,
    BackendSync, MailSource, OutgoingMail,
};

/// Network-backed operations for exactly one configured account.
#[async_trait]
pub trait AccountBackend: Send + Sync {
    /// Returns safe account metadata.
    fn account(&self) -> BackendAccount;

    /// Negotiates and returns optional server capabilities.
    async fn capabilities(&self) -> Result<BackendCapabilities>;

    /// Refreshes and returns the managed folder hierarchy.
    async fn folders(&self) -> Result<Vec<eas_mail_protocol::Folder>>;

    /// Refreshes all mail collections into process-local memory.
    async fn sync_mail(&self) -> Result<BackendSync>;

    /// Performs a fresh mail synchronization and returns the resulting snapshot.
    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>>;

    /// Performs EAS Search and returns server results.
    async fn search_mail(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>>;

    /// Fetches a full message body from Exchange.
    async fn fetch_mail(&self, source: &MailSource, body_limit: usize) -> Result<BackendMail>;

    /// Downloads attachment bytes from Exchange.
    async fn fetch_attachment(&self, file_reference: &str) -> Result<Vec<u8>>;

    /// Resolves directory recipients and returns one free/busy range.
    async fn calendar_availability(
        &self,
        participants: &[String],
        starts_at: chrono::DateTime<chrono::Utc>,
        ends_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<eas_mail_protocol::RecipientAvailability>>;

    /// Performs bounded server-side Calendar Search.
    async fn search_calendar(&self, query: &str, limit: usize) -> Result<BackendCalendarSearch>;

    /// Fetches one full Calendar item from a Search LongId.
    async fn fetch_calendar(&self, long_id: &str, body_limit: usize) -> Result<BackendEvent>;

    /// Changes one message's read state.
    async fn mark_read(&self, source: &MailSource, is_read: bool) -> Result<()>;

    /// Sends a new message.
    async fn send(&self, client_id: &str, message: &OutgoingMail) -> Result<()>;

    /// Replies to an existing message.
    async fn reply(
        &self,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()>;

    /// Forwards an existing message.
    async fn forward(
        &self,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()>;
}
