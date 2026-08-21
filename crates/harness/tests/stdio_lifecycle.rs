use std::time::Duration;

use anyhow::{Context as _, Result};
use rmcp::ServiceExt as _;
use rmcp::transport::{ConfigureCommandExt as _, TokioChildProcess};
use tokio::process::Command;

const SESSION_CYCLES: usize = 24;
const EXIT_MARKER_ENV: &str = "EAS_MAIL_HARNESS_EXIT_MARKER";

#[tokio::test]
async fn black_box_stdio_sessions_do_not_accumulate_processes() -> Result<()> {
    for cycle in 0..SESSION_CYCLES {
        let temporary = tempfile::tempdir()?;
        let exit_marker = temporary.path().join("closed");
        let transport = TokioChildProcess::new(
            Command::new(env!("CARGO_BIN_EXE_harness-server")).configure(|command| {
                command.kill_on_drop(true);
                command.env(EXIT_MARKER_ENV, &exit_marker);
            }),
        )?;
        let client = tokio::time::timeout(Duration::from_secs(5), ().serve(transport))
            .await
            .context("MCP initialize timed out")??;

        if cycle % 2 == 0 {
            client.cancel().await?;
        } else {
            drop(client);
        }

        wait_until_server_closes(&exit_marker).await?;
    }
    Ok(())
}

async fn wait_until_server_closes(exit_marker: &std::path::Path) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !exit_marker.is_file() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("stdio server did not close gracefully after client shutdown")?;
    Ok(())
}
