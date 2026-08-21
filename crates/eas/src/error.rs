use thiserror::Error;

/// Result returned by EAS operations.
pub type Result<T> = std::result::Result<T, EasError>;

/// Stable failures raised by the EAS layer.
#[derive(Debug, Error)]
pub enum EasError {
    /// The managed account requires credentials or rejected them.
    #[error("authentication failed")]
    Authentication,
    /// The endpoint authenticated the request path but denied EAS access.
    #[error("Exchange ActiveSync access is denied")]
    AccessDenied,
    /// The endpoint cannot be reached safely.
    #[error("managed Exchange endpoint is unreachable: {0}")]
    Network(String),
    /// A mutation may have reached Exchange before the connection failed.
    #[error("mutation outcome is unknown")]
    OutcomeUnknown,
    /// The configured endpoint or identity is invalid.
    #[error("invalid EAS configuration: {0}")]
    InvalidConfiguration(String),
    /// Exchange returned an invalid or unsupported protocol response.
    #[error("EAS protocol error: {0}")]
    Protocol(String),
    /// Exchange reported a retryable service-side failure.
    #[error("Exchange service is temporarily unavailable")]
    ServiceUnavailable,
    /// The collection SyncKey is no longer valid.
    #[error("collection SyncKey is stale")]
    InvalidSyncKey,
    /// The FolderSync key is no longer valid.
    #[error("FolderSync key is stale")]
    InvalidFolderSyncKey,
    /// Exchange requires a new Provision handshake before retrying.
    #[error("Exchange policy must be refreshed")]
    PolicyRefreshRequired,
    /// Exchange requested removal of this account's application data.
    #[error("Exchange requested an account-only remote wipe")]
    AccountRemoteWipe,
    /// Exchange requested a device-wide policy that this app cannot enforce.
    #[error("Exchange requested an unsupported device-wide policy: {0}")]
    UnsupportedDevicePolicy(String),
}
