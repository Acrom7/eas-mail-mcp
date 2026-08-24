use std::io::Write as _;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match eas_mail_mcp::cli::run().await {
        Ok(status) => ExitCode::from(status.code()),
        Err(error) => {
            let payload = serde_json::to_string(&error.envelope)
                .unwrap_or_else(|_| "{\"code\":\"PROTOCOL_ERROR\"}".into());
            let _ = writeln!(std::io::stderr().lock(), "{payload}");
            let usage = matches!(
                error.envelope.code,
                eas_mail_mcp::ErrorCode::InteractiveRequired
                    | eas_mail_mcp::ErrorCode::ValidationFailed
            );
            let write_failed = error.envelope.operation_id.is_some()
                || error.envelope.code == eas_mail_mcp::ErrorCode::OutcomeUnknown;
            ExitCode::from(if write_failed {
                3
            } else if usage {
                2
            } else {
                1
            })
        }
    }
}
