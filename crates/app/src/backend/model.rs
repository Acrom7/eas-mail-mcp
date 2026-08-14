use eas_mail_protocol::{CalendarFields, MailFields, ProfileKey};

/// Safe account metadata exposed by a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendAccount {
    /// Stable local identifier.
    pub account_id: String,
    /// Fixed managed endpoint profile.
    pub profile: ProfileKey,
    /// Mailbox address.
    pub email: String,
    /// Whether the account is enabled.
    pub enabled: bool,
    /// Whether mail mutations are enabled.
    pub write_enabled: bool,
}

/// Immutable Exchange reference for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailSource {
    /// Item returned by collection Sync.
    Item {
        /// Folder collection identifier.
        folder_id: String,
        /// Message server identifier.
        server_id: String,
    },
    /// Item returned by server-side Search.
    LongId(String),
}

/// Process-local mail record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendMail {
    /// Stable account identifier.
    pub account_id: String,
    /// Folder identifier, empty for Search LongId results.
    pub folder_id: String,
    /// Exchange source reference.
    pub source: MailSource,
    /// Parsed mail fields.
    pub fields: MailFields,
}

/// Process-local calendar record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendEvent {
    /// Stable account identifier.
    pub account_id: String,
    /// Calendar collection identifier.
    pub folder_id: String,
    /// Event server identifier.
    pub server_id: String,
    /// Parsed event fields.
    pub fields: CalendarFields,
}

/// One explicit synchronization result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSync {
    /// Number of collections synchronized.
    pub collections: usize,
    /// Ordered changes applied to process-local state.
    pub changes: usize,
}

/// Plain-text outgoing message accepted by EAS compose commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMail {
    /// To recipients.
    pub to: Vec<String>,
    /// Cc recipients.
    pub cc: Vec<String>,
    /// Bcc recipients.
    pub bcc: Vec<String>,
    /// Message subject.
    pub subject: String,
    /// Plain-text body.
    pub body: String,
}
