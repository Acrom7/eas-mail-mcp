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
use serde_json::{Map, Value};

const STARTUP_SAMPLES: usize = 20;
const WARMUP_CALLS: usize = 20;
const LATENCY_SAMPLES: usize = 200;
const MAX_STARTUP_MS: f64 = 150.0;
const MAX_RSS_MIB: f64 = 20.0;
const MAX_BINARY_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PYTHON_RATIO: f64 = 1.15;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long)]
    application: PathBuf,
    #[arg(long)]
    python_baseline: PathBuf,
    #[arg(long, default_value = "python3")]
    python: String,
}

#[derive(Debug, Serialize)]
struct Report {
    startup_samples: usize,
    latency_samples: usize,
    rust_startup_p95_ms: f64,
    rust_mail_list_p95_ms: f64,
    python_mail_list_p95_ms: f64,
    rust_to_python_ratio: f64,
    rust_idle_rss_mib: f64,
    production_binary_bytes: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let server = sibling("perf-server")?;
    let startup = startup_samples(&server).await?;
    let rust = latency_samples(command(&server, None)).await?;
    let python =
        latency_samples(command(&arguments.python_baseline, Some(&arguments.python))).await?;
    let binary_bytes = std::fs::metadata(&arguments.application)
        .with_context(|| format!("cannot inspect {}", arguments.application.display()))?
        .len();
    let rust_p95 = p95(&rust.latencies)?;
    let python_p95 = p95(&python.latencies)?;
    let report = Report {
        startup_samples: startup.len(),
        latency_samples: rust.latencies.len(),
        rust_startup_p95_ms: rounded(p95(&startup)?),
        rust_mail_list_p95_ms: rounded(rust_p95),
        python_mail_list_p95_ms: rounded(python_p95),
        rust_to_python_ratio: rounded(rust_p95 / python_p95),
        rust_idle_rss_mib: rounded(rust.rss_mib),
        production_binary_bytes: binary_bytes,
    };
    serde_json::to_writer_pretty(io::stdout().lock(), &report)?;
    writeln!(io::stdout().lock())?;
    enforce(&report)
}

struct Measurements {
    latencies: Vec<f64>,
    rss_mib: f64,
}

async fn startup_samples(server: &Path) -> Result<Vec<f64>> {
    let mut samples = Vec::with_capacity(STARTUP_SAMPLES);
    for _ in 0..STARTUP_SAMPLES {
        let started = Instant::now();
        let (service, _) = connect(command(server, None)).await?;
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        service.cancel().await?;
    }
    Ok(samples)
}

async fn latency_samples(command: tokio::process::Command) -> Result<Measurements> {
    let (service, pid) = connect(command).await?;
    let peer = service.peer().clone();
    for _ in 0..WARMUP_CALLS {
        call_mail_list(&peer).await?;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    let rss_mib = rss_mib(pid)?;
    let mut latencies = Vec::with_capacity(LATENCY_SAMPLES);
    for _ in 0..LATENCY_SAMPLES {
        let started = Instant::now();
        call_mail_list(&peer).await?;
        latencies.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    service.cancel().await?;
    Ok(Measurements { latencies, rss_mib })
}

fn command(path: &Path, interpreter: Option<&str>) -> tokio::process::Command {
    let command = if let Some(interpreter) = interpreter {
        let mut command = tokio::process::Command::new(interpreter);
        command.arg(path);
        command
    } else {
        tokio::process::Command::new(path)
    };
    command.configure(|value| {
        value.kill_on_drop(true);
    })
}

async fn connect(
    command: tokio::process::Command,
) -> Result<(RunningService<RoleClient, InitializeRequestParams>, u32)> {
    let transport = TokioChildProcess::new(command)?;
    let pid = transport.id().ok_or_else(|| anyhow::anyhow!("child process has no PID"))?;
    let info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("codex-mcp-client", "0.133.0"),
    );
    let service = tokio::time::timeout(Duration::from_secs(5), info.serve(transport))
        .await
        .context("MCP initialize timed out")??;
    Ok((service, pid))
}

async fn call_mail_list(peer: &rmcp::service::Peer<RoleClient>) -> Result<()> {
    let mut arguments = Map::new();
    arguments.insert("limit".into(), Value::from(100));
    let request = CallToolRequestParams::new("mail_list").with_arguments(arguments);
    let response = tokio::time::timeout(Duration::from_secs(5), peer.call_tool(request))
        .await
        .context("mail_list timed out")??;
    anyhow::ensure!(response.structured_content.is_some(), "mail_list returned no structured data");
    anyhow::ensure!(!response.is_error.unwrap_or(false), "mail_list returned an MCP error");
    Ok(())
}

fn rss_mib(pid: u32) -> Result<f64> {
    let output =
        std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output()?;
    anyhow::ensure!(output.status.success(), "cannot inspect benchmark RSS");
    let kib = String::from_utf8(output.stdout)?.trim().parse::<f64>()?;
    Ok(kib / 1024.0)
}

fn p95(samples: &[f64]) -> Result<f64> {
    anyhow::ensure!(!samples.is_empty(), "performance sample is empty");
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    let index = (ordered.len() * 95).div_ceil(100).saturating_sub(1);
    ordered.get(index).copied().ok_or_else(|| anyhow::anyhow!("p95 index is invalid"))
}

fn sibling(name: &str) -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable.parent().ok_or_else(|| anyhow::anyhow!("binary path is invalid"))?;
    Ok(directory.join(name))
}

fn rounded(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

fn enforce(report: &Report) -> Result<()> {
    anyhow::ensure!(
        report.rust_startup_p95_ms <= MAX_STARTUP_MS,
        "cold MCP startup p95 exceeds {MAX_STARTUP_MS} ms"
    );
    anyhow::ensure!(report.rust_idle_rss_mib <= MAX_RSS_MIB, "idle RSS exceeds {MAX_RSS_MIB} MiB");
    anyhow::ensure!(
        report.production_binary_bytes <= MAX_BINARY_BYTES,
        "production binary exceeds {MAX_BINARY_BYTES} bytes"
    );
    anyhow::ensure!(
        report.rust_to_python_ratio <= MAX_PYTHON_RATIO,
        "fake-EAS p95 is more than 15% slower than the Python baseline"
    );
    Ok(())
}
