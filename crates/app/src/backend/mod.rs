//! Account-scoped mailbox boundary and its production EAS implementation.

mod eas_mailbox;
mod model;

use async_trait::async_trait;

use crate::Result;

pub use eas_mailbox::EasMailbox;
pub use model::{BackendAccount, BackendEvent, BackendMail, BackendSync, MailSource, OutgoingMail};

/// Network-backed operations for exactly one configured account.
#[async_trait]
pub trait AccountBackend: Send + Sync {
    /// Returns safe account metadata.
    fn account(&self) -> BackendAccount;

    /// Refreshes and returns the managed folder hierarchy.
    async fn folders(&self) -> Result<Vec<eas_mail_protocol::Folder>>;

    /// Refreshes selected collections into process-local memory.
    async fn sync(&self, mail: bool, calendar: bool) -> Result<BackendSync>;

    /// Performs a fresh mail synchronization and returns the resulting snapshot.
    async fn list_mail(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendMail>>;

    /// Performs EAS Search and returns server results.
    async fn search_mail(&self, query: &str, limit: usize) -> Result<Vec<BackendMail>>;

    /// Fetches a full message body from Exchange.
    async fn fetch_mail(&self, source: &MailSource, body_limit: usize) -> Result<BackendMail>;

    /// Downloads attachment bytes from Exchange.
    async fn fetch_attachment(&self, file_reference: &str) -> Result<Vec<u8>>;

    /// Performs a fresh calendar synchronization and returns the resulting snapshot.
    async fn list_calendar(&self, folder_ids: Option<&[String]>) -> Result<Vec<BackendEvent>>;

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
