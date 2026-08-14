use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result};
use rmcp::ServiceExt as _;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientRequest, Implementation,
    InitializeRequestParams, Request,
};
use rmcp::service::{Peer, PeerRequestOptions, RoleClient, RunningService, ServiceError};
use rmcp::transport::{ConfigureCommandExt as _, TokioChildProcess};
use serde_json::{Value, json};

const CLOCK_FILE_ENV: &str = "EAS_MAIL_HARNESS_CLOCK_FILE";
const DELAY_ENV: &str = "EAS_MAIL_HARNESS_DELAY_MS";

#[tokio::test]
async fn black_box_cursor_expiry_is_reported_through_stdio() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let clock_file = directory.path().join("clock");
    std::fs::write(&clock_file, "1700000000")?;
    let client = start_server(None, Some(&clock_file)).await?;

    let first = successful_call(client.peer(), "mail_list", Some(json!({ "limit": 1 }))).await?;
    let cursor = first
        .pointer("/data/next_cursor")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("mail_list returned no cursor"))?;
    std::fs::write(&clock_file, "1700000960")?;

    let expired =
        tool_call(client.peer(), "mail_list", Some(json!({ "cursor": cursor, "limit": 1 })))
            .await?;
    let structured = expired
        .structured_content
        .ok_or_else(|| anyhow::anyhow!("expired cursor returned no structured error"))?;
    anyhow::ensure!(
        structured.pointer("/error/code").and_then(Value::as_str) == Some("REFERENCE_EXPIRED"),
        "unexpected cursor error: {structured}"
    );
    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn black_box_active_stdio_request_supports_timeout_and_explicit_cancellation() -> Result<()> {
    let client = start_server(Some(500), None).await?;
    let peer = client.peer().clone();

    let timed = peer
        .send_cancellable_request(
            request("sync_now", Some(json!({ "scope": "all" })))?,
            PeerRequestOptions::with_timeout(Duration::from_millis(50)),
        )
        .await?
        .await_response()
        .await;
    anyhow::ensure!(matches!(timed, Err(ServiceError::Timeout { .. })));

    let pending = peer
        .send_cancellable_request(
            request("sync_now", Some(json!({ "scope": "all" })))?,
            PeerRequestOptions::no_options(),
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    tokio::time::timeout(
        Duration::from_secs(1),
        pending.cancel(Some("black-box cancellation".into())),
    )
    .await
    .context("cancellation notification timed out")??;

    successful_call(&peer, "accounts_list", None).await?;
    client.cancel().await?;
    Ok(())
}

async fn start_server(
    delay_ms: Option<u64>,
    clock_file: Option<&Path>,
) -> Result<RunningService<RoleClient, InitializeRequestParams>> {
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_harness-server")).configure(|command| {
            command.kill_on_drop(true);
            if let Some(delay_ms) = delay_ms {
                command.env(DELAY_ENV, delay_ms.to_string());
            }
            if let Some(clock_file) = clock_file {
                command.env(CLOCK_FILE_ENV, clock_file);
            }
        }),
    )?;
    let info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("codex-mcp-client", "0.133.0"),
    );
    Ok(tokio::time::timeout(Duration::from_secs(10), info.serve(transport))
        .await
        .context("MCP initialize timed out")??)
}

async fn successful_call(
    peer: &Peer<RoleClient>,
    name: &str,
    input: Option<Value>,
) -> Result<Value> {
    let result = tool_call(peer, name, input).await?;
    let structured = result
        .structured_content
        .ok_or_else(|| anyhow::anyhow!("{name} returned no structured content"))?;
    anyhow::ensure!(structured.get("error").is_some_and(Value::is_null));
    Ok(structured)
}

async fn tool_call(
    peer: &Peer<RoleClient>,
    name: &str,
    input: Option<Value>,
) -> Result<rmcp::model::CallToolResult> {
    Ok(tokio::time::timeout(Duration::from_secs(10), peer.call_tool(tool_params(name, input)?))
        .await
        .with_context(|| format!("{name} timed out"))??)
}

fn request(name: &str, input: Option<Value>) -> Result<ClientRequest> {
    Ok(ClientRequest::CallToolRequest(Request::new(tool_params(name, input)?)))
}

fn tool_params(name: &str, input: Option<Value>) -> Result<CallToolRequestParams> {
    let mut params = CallToolRequestParams::new(name.to_owned());
    if let Some(input) = input {
        let arguments = input
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("tool arguments must be an object"))?;
        params = params.with_arguments(arguments);
    }
    Ok(params)
}
