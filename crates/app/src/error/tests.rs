use super::*;

#[test]
fn builder_populates_only_explicit_public_fields() {
    let error = AppError::new(ErrorCode::NotFound, "missing")
        .account("work")
        .retryable()
        .remediation("refresh")
        .operation("operation-1");
    assert_eq!(error.envelope.code, ErrorCode::NotFound);
    assert_eq!(error.envelope.account_id.as_deref(), Some("work"));
    assert_eq!(error.envelope.operation_id.as_deref(), Some("operation-1"));
    assert_eq!(error.envelope.remediation.as_deref(), Some("refresh"));
    assert!(error.envelope.retryable);
}

#[test]
fn every_eas_error_maps_to_a_stable_safe_code() {
    let cases = [
        (EasError::Authentication, ErrorCode::AuthRequired, false),
        (EasError::AccessDenied, ErrorCode::AccessDenied, false),
        (EasError::Network("private detail".into()), ErrorCode::NetworkUnreachable, true),
        (EasError::ServiceUnavailable, ErrorCode::ProtocolError, true),
        (EasError::OutcomeUnknown, ErrorCode::OutcomeUnknown, false),
        (EasError::InvalidConfiguration("private detail".into()), ErrorCode::ConfigInvalid, false),
        (EasError::InvalidSyncKey, ErrorCode::SyncStale, true),
        (EasError::InvalidFolderSyncKey, ErrorCode::SyncStale, true),
        (EasError::PolicyRefreshRequired, ErrorCode::SyncStale, true),
        (EasError::AccountRemoteWipe, ErrorCode::RemoteWipe, false),
        (
            EasError::UnsupportedDevicePolicy("private detail".into()),
            ErrorCode::PolicyBlocked,
            false,
        ),
        (EasError::Protocol("private detail".into()), ErrorCode::ProtocolError, false),
    ];
    for (source, code, retryable) in cases {
        let mapped = AppError::from(source);
        assert_eq!(mapped.envelope.code, code);
        assert_eq!(mapped.envelope.retryable, retryable);
        assert!(!mapped.envelope.message.contains("private detail"));
    }
}
