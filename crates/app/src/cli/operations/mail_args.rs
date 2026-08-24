use clap::{Args, Subcommand};

use super::common::{BodySource, InputSource, ReadStateArg, WriteControl};

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum MailCommand {
    /// List a fresh bounded mail snapshot.
    List(MailListArgs),
    /// Search Exchange mail directly.
    Search(MailSearchArgs),
    /// Fetch one full message body.
    Get(MailGetArgs),
    /// List attachment metadata for one message.
    Attachments(MailReferenceArgs),
    /// Download one attachment to the managed cache.
    Download(AttachmentReferenceArgs),
    /// Change one message's read state.
    MarkRead(MailMarkReadArgs),
    /// Send a plain-text message.
    Send(MailSendArgs),
    /// Reply to a referenced message.
    Reply(MailReplyArgs),
    /// Forward a referenced message.
    Forward(MailForwardArgs),
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailListArgs {
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Account ID; repeat to select multiple accounts.
    #[arg(long = "account")]
    pub(super) accounts: Vec<String>,
    /// Exchange folder ID; repeat to select multiple folders.
    #[arg(long = "folder")]
    pub(super) folders: Vec<String>,
    /// Maximum total results, default 50 and maximum 10,000.
    #[arg(long, conflicts_with = "all")]
    pub(super) limit: Option<usize>,
    /// Consume every page in the bounded runtime snapshot.
    #[arg(long)]
    pub(super) all: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailSearchArgs {
    /// Search text; omit only when using --input.
    pub(super) query: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Account ID; repeat to select multiple accounts.
    #[arg(long = "account")]
    pub(super) accounts: Vec<String>,
    /// Maximum total results, default 50 and maximum 10,000.
    #[arg(long, conflicts_with = "all")]
    pub(super) limit: Option<usize>,
    /// Consume every page in the bounded runtime snapshot.
    #[arg(long)]
    pub(super) all: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailGetArgs {
    /// Portable mail reference; omit only when using --input.
    pub(super) mail_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Maximum body characters, default 12,000 and maximum 50,000.
    #[arg(long)]
    pub(super) body_limit: Option<u32>,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailReferenceArgs {
    /// Portable mail reference; omit only when using --input.
    pub(super) mail_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct AttachmentReferenceArgs {
    /// Portable attachment reference; omit only when using --input.
    pub(super) attachment_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailMarkReadArgs {
    /// Portable mail reference; omit only when using --input.
    pub(super) mail_ref: Option<String>,
    /// New message state; omit only when using --input.
    #[arg(value_enum)]
    pub(super) state: Option<ReadStateArg>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailSendArgs {
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Sending account ID.
    #[arg(long)]
    pub(super) account: Option<String>,
    /// To recipient; repeat for multiple recipients.
    #[arg(long)]
    pub(super) to: Vec<String>,
    /// Cc recipient; repeat for multiple recipients.
    #[arg(long)]
    pub(super) cc: Vec<String>,
    /// Bcc recipient; repeat for multiple recipients.
    #[arg(long)]
    pub(super) bcc: Vec<String>,
    /// Message subject.
    #[arg(long)]
    pub(super) subject: Option<String>,
    #[command(flatten)]
    pub(super) content: BodySource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailReplyArgs {
    /// Portable mail reference; omit only when using --input.
    pub(super) mail_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    #[command(flatten)]
    pub(super) content: BodySource,
    /// Include original To and Cc recipients.
    #[arg(long)]
    pub(super) reply_all: bool,
    #[command(flatten)]
    pub(super) control: WriteControl,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct MailForwardArgs {
    /// Portable mail reference; omit only when using --input.
    pub(super) mail_ref: Option<String>,
    #[command(flatten)]
    pub(super) source: InputSource,
    /// Forward recipient; repeat for multiple recipients.
    #[arg(long)]
    pub(super) to: Vec<String>,
    /// Cc recipient; repeat for multiple recipients.
    #[arg(long)]
    pub(super) cc: Vec<String>,
    /// Bcc recipient; repeat for multiple recipients.
    #[arg(long)]
    pub(super) bcc: Vec<String>,
    #[command(flatten)]
    pub(super) content: BodySource,
    #[command(flatten)]
    pub(super) control: WriteControl,
}
