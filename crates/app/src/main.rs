use std::io::Write as _;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match eas_mail_mcp::cli::run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let payload = serde_json::to_string(&error.envelope)
                .unwrap_or_else(|_| "{\"code\":\"PROTOCOL_ERROR\"}".into());
            let _ = writeln!(std::io::stderr().lock(), "{payload}");
            ExitCode::FAILURE
        }
    }
}
