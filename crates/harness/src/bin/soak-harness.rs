use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use clap::Parser;
use rmcp::ServiceExt as _;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt as _, TokioChildProcess};
use serde::Serialize;
use serde_json::{Map, Value, json};

const CLIENTS: [(&str, &str); 3] =
    [("codex-mcp-client", "0.133.0"), ("claude-ai", "0.1.0"), ("opencode", "1.0.0")];
const MINIMUM_HOURS: u64 = 8;
const CALL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long)]
    application: PathBuf,
    #[arg(long, default_value_t = MINIMUM_HOURS)]
    hours: u64,
    #[arg(long, default_value_t = 300)]
    interval_seconds: u64,
}

#[derive(Debug, Serialize)]
struct Report {
    duration_hours: u64,
    clients: usize,
    cycles_per_client: usize,
    acceptance_passed: bool,
}

struct ClientSession {
    name: &'static str,
    service: RunningService<RoleClient, InitializeRequestParams>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    anyhow::ensure!(arguments.hours >= MINIMUM_HOURS, "acceptance soak requires at least 8 hours");
    anyhow::ensure!(arguments.interval_seconds > 0, "soak interval must be positive");
    let duration = Duration::from_secs(
        arguments.hours.checked_mul(3_600).context("soak duration is too large")?,
    );
    let deadline = Instant::now().checked_add(duration).context("soak deadline is invalid")?;
    let mut sessions = connect_clients(&arguments.application).await?;
    let mut cycles = 0;
    loop {
        for session in &sessions {
            check_cycle(&session.service)
                .await
                .with_context(|| format!("{} failed cycle {}", session.name, cycles + 1))?;
        }
        cycles += 1;
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        tokio::time::sleep(
            Duration::from_secs(arguments.interval_seconds).min(deadline.duration_since(now)),
        )
        .await;
    }
    for session in sessions.drain(..) {
        session.service.cancel().await?;
    }
    serde_json::to_writer_pretty(
        io::stdout().lock(),
        &Report {
            duration_hours: arguments.hours,
            clients: CLIENTS.len(),
            cycles_per_client: cycles,
            acceptance_passed: true,
        },
    )?;
    writeln!(io::stdout().lock())?;
    Ok(())
}

async fn connect_clients(application: &Path) -> Result<Vec<ClientSession>> {
    let mut sessions = Vec::with_capacity(CLIENTS.len());
    for (name, version) in CLIENTS {
        let command = tokio::process::Command::new(application).configure(|value| {
            value.arg("serve").kill_on_drop(true);
        });
        let transport = TokioChildProcess::new(command)?;
        let info = InitializeRequestParams::new(
            ClientCapabilities::default(),
            Implementation::new(name, version),
        );
        let session = tokio::time::timeout(Duration::from_secs(20), info.serve(transport))
            .await
            .with_context(|| format!("{name} MCP initialize timed out"))??;
        sessions.push(ClientSession { name, service: session });
    }
    Ok(sessions)
}

async fn check_cycle(session: &RunningService<RoleClient, InitializeRequestParams>) -> Result<()> {
    let peer = session.peer();
    let accounts = call(peer, "accounts_list", Map::new()).await?;
    let enabled = accounts.pointer("/data/accounts").and_then(Value::as_array).map_or(0, |items| {
        items.iter().filter(|item| item.get("enabled") == Some(&Value::Bool(true))).count()
    });
    anyhow::ensure!(enabled >= 2, "soak requires both enabled managed accounts");
    call(peer, "sync_now", Map::new()).await?;
    call(peer, "mail_list", arguments(json!({ "limit": 1 }))?).await?;
    let account = accounts
        .pointer("/data/accounts/0")
        .ok_or_else(|| anyhow::anyhow!("soak account metadata is missing"))?;
    let account_id = account
        .get("account_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("soak account id is missing"))?;
    let email = account
        .get("email")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("soak account email is missing"))?;
    call(
        peer,
        "calendar_search",
        arguments(json!({ "account_ids": [account_id], "query": email, "limit": 1 }))?,
    )
    .await?;
    let status = call(peer, "sync_status", Map::new()).await?;
    let reports = status.pointer("/data/reports").and_then(Value::as_array).map_or(0, Vec::len);
    anyhow::ensure!(reports >= 2, "both accounts must complete synchronization");
    Ok(())
}

async fn call(
    peer: &rmcp::service::Peer<RoleClient>,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<Value> {
    let request = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
    let response = tokio::time::timeout(CALL_TIMEOUT, peer.call_tool(request))
        .await
        .with_context(|| format!("{name} timed out"))??;
    anyhow::ensure!(!response.is_error.unwrap_or(false), "{name} returned an MCP error");
    let structured = response
        .structured_content
        .ok_or_else(|| anyhow::anyhow!("{name} returned no structured content"))?;
    anyhow::ensure!(structured.get("error").is_some_and(Value::is_null), "{name} failed");
    let warnings = structured
        .get("warnings")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("{name} returned malformed warnings"))?;
    if !warnings.is_empty() {
        anyhow::bail!("{name} returned warnings: {}", warning_codes(warnings));
    }
    Ok(structured)
}

fn warning_codes(warnings: &[Value]) -> String {
    warnings
        .iter()
        .map(|warning| {
            let account = warning.get("account_id").and_then(Value::as_str).unwrap_or("unknown");
            let code = warning.get("code").and_then(Value::as_str).unwrap_or("UNKNOWN");
            format!("{account}:{code}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn arguments(value: Value) -> Result<Map<String, Value>> {
    value.as_object().cloned().ok_or_else(|| anyhow::anyhow!("tool input must be an object"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_diagnostics_contain_only_account_and_code() -> Result<()> {
        let warnings = json!([{
            "account_id": "managed",
            "code": "NETWORK_ERROR",
            "message": "sensitive upstream detail"
        }]);
        let values =
            warnings.as_array().ok_or_else(|| anyhow::anyhow!("fixture is not an array"))?;
        assert_eq!(warning_codes(values), "managed:NETWORK_ERROR");
        Ok(())
    }
}
