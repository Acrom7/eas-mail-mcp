use std::path::PathBuf;

use clap::{Args, ValueEnum};

#[derive(Debug, Args, Default)]
pub(super) struct InputSource {
    /// Read the command input as JSON from a file, or from stdin with `-`.
    #[arg(long, value_name = "FILE")]
    pub(super) input: Option<PathBuf>,
}

#[derive(Debug, Args, Default)]
pub(super) struct BodySource {
    /// Inline plain-text body.
    #[arg(long, conflicts_with_all = ["body_file", "body_stdin"])]
    pub(super) body: Option<String>,
    /// Read the plain-text body from a file.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["body", "body_stdin"])]
    pub(super) body_file: Option<PathBuf>,
    /// Read the plain-text body from stdin until EOF.
    #[arg(long, conflicts_with_all = ["body", "body_file"])]
    pub(super) body_stdin: bool,
}

#[derive(Debug, Args, Default)]
pub(super) struct CommentSource {
    /// Inline plain-text comment.
    #[arg(long, conflicts_with_all = ["comment_file", "comment_stdin"])]
    pub(super) comment: Option<String>,
    /// Read the plain-text comment from a file.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["comment", "comment_stdin"])]
    pub(super) comment_file: Option<PathBuf>,
    /// Read the plain-text comment from stdin until EOF.
    #[arg(long, conflicts_with_all = ["comment", "comment_file"])]
    pub(super) comment_stdin: bool,
}

#[derive(Debug, Args, Default)]
pub(super) struct WriteControl {
    /// Reuse a specific UUID for an idempotent retry; generated when omitted.
    #[arg(long)]
    pub(super) idempotency_key: Option<String>,
    /// Execute without an interactive confirmation prompt.
    #[arg(long)]
    pub(super) yes: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ReadStateArg {
    Read,
    Unread,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum BusyStatusArg {
    Free,
    Tentative,
    Busy,
    OutOfOffice,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ResponseArg {
    Accept,
    Tentative,
    Decline,
}
