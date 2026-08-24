mod configuration;
mod files;
mod process;

use clap::ValueEnum;
use serde_json::Value;

use self::configuration::{
    configure_claude, configure_codex, configure_opencode, unconfigure_claude, unconfigure_cli,
    unconfigure_opencode,
};
use self::files::ClientFiles;
use self::process::{client_display_name, client_name, detect_version};
use super::terminal::Terminal;
use super::{ClientArgs, ClientCommand};
use crate::{AppError, ErrorCode, Paths, Result};

const SERVER: &str = "eas-mail";
const WRITE_TOOLS: [&str; 9] = [
    "mail_mark_read",
    "mail_send",
    "mail_reply",
    "mail_forward",
    "calendar_create",
    "calendar_update",
    "calendar_delete",
    "calendar_cancel",
    "calendar_respond",
];

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ClientKind {
    Codex,
    Claude,
    Opencode,
}

#[derive(Debug, Clone)]
struct DetectedClient {
    kind: ClientKind,
    executable: String,
    version: String,
}

pub(super) fn run(paths: &Paths, command: ClientCommand) -> Result<Value> {
    match command {
        ClientCommand::Configure(arguments) => configure(paths, arguments),
        ClientCommand::Unconfigure(arguments) => unconfigure(paths, arguments),
    }
}

pub(super) fn configure_detected_with_terminal(
    paths: &Paths,
    terminal: &mut dyn Terminal,
) -> Result<Vec<Value>> {
    let detected = detect_supported_clients(detect_version);
    configure_detected(terminal, &detected, |arguments| configure(paths, arguments))
}

fn detect_supported_clients(mut detect: impl FnMut(&str) -> Option<String>) -> Vec<DetectedClient> {
    [ClientKind::Codex, ClientKind::Claude, ClientKind::Opencode]
        .into_iter()
        .filter_map(|kind| {
            let executable = client_name(kind).to_owned();
            let version = detect(&executable)?;
            Some(DetectedClient { kind, executable, version })
        })
        .collect()
}

fn configure_detected(
    terminal: &mut dyn Terminal,
    detected: &[DetectedClient],
    mut configure_client: impl FnMut(ClientArgs) -> Result<Value>,
) -> Result<Vec<Value>> {
    terminal.message("AI client connection")?;
    if detected.is_empty() {
        terminal.message("No supported AI client commands were detected")?;
        terminal.message(
            "Connect one later with: eas-mail-mcp client configure <codex|claude|opencode>",
        )?;
        return Ok(Vec::new());
    }

    terminal.message(
        "EAS Mail MCP can register itself automatically; no manual MCP config is required.",
    )?;
    terminal.message("Detected AI clients:")?;
    for client in detected {
        terminal.message(&format!(
            "  {} ({})",
            client_display_name(client.kind),
            client.version
        ))?;
    }

    let mut results = Vec::new();
    for client in detected {
        let display_name = client_display_name(client.kind);
        if terminal
            .confirm(&format!("Connect EAS Mail MCP to {display_name} automatically"), true)?
        {
            let mut result = configure_client(ClientArgs {
                client: client.kind,
                executable: Some(client.executable.clone()),
            })?;
            add_client_result_metadata(&mut result, display_name, true)?;
            terminal.message(&format!("{display_name} configured successfully"))?;
            terminal.message(&format!("Restart {display_name} to activate EAS Mail MCP"))?;
            results.push(result);
        } else {
            terminal.message(&format!(
                "{display_name} skipped; connect it later with: eas-mail-mcp client configure {}",
                client_name(client.kind)
            ))?;
            results.push(serde_json::json!({
                "client": client_name(client.kind),
                "display_name": display_name,
                "version": client.version,
                "configured": false,
                "restart_required": false,
                "reason": "declined",
            }));
        }
    }
    Ok(results)
}

fn add_client_result_metadata(
    result: &mut Value,
    display_name: &str,
    restart_required: bool,
) -> Result<()> {
    let object = result.as_object_mut().ok_or_else(|| {
        AppError::new(ErrorCode::ProtocolError, "client configuration result is invalid")
    })?;
    object.insert("display_name".into(), Value::String(display_name.into()));
    object.insert("restart_required".into(), Value::Bool(restart_required));
    Ok(())
}

fn configure(paths: &Paths, arguments: ClientArgs) -> Result<Value> {
    let executable = arguments.executable.unwrap_or_else(|| client_name(arguments.client).into());
    let version = detect_version(&executable);
    let bridge = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "cannot resolve MCP executable"))?;
    let files = ClientFiles::discover()?;
    let backups = match arguments.client {
        ClientKind::Codex => configure_codex(paths, &files.codex, &executable, &bridge)?,
        ClientKind::Claude => configure_claude(paths, &files, &executable, &bridge)?,
        ClientKind::Opencode => configure_opencode(paths, &files.opencode, &bridge)?,
    };
    Ok(serde_json::json!({
        "client": client_name(arguments.client),
        "version": version,
        "configured": true,
        "write_execution": "direct_when_account_enabled",
        "backups": backups,
    }))
}

fn unconfigure(paths: &Paths, arguments: ClientArgs) -> Result<Value> {
    let executable = arguments.executable.unwrap_or_else(|| client_name(arguments.client).into());
    let version = detect_version(&executable);
    let files = ClientFiles::discover()?;
    let backups = match arguments.client {
        ClientKind::Codex => unconfigure_cli(paths, &executable, files.codex, false)?,
        ClientKind::Claude => unconfigure_claude(paths, &files, &executable)?,
        ClientKind::Opencode => unconfigure_opencode(paths, &files.opencode)?,
    };
    Ok(serde_json::json!({
        "client": client_name(arguments.client),
        "version": version,
        "configured": false,
        "backups": backups,
    }))
}

#[cfg(test)]
use self::configuration::remove_codex_generated_approvals;
#[cfg(test)]
use self::files::{
    array_entry, backup, object_entry, path_text, paths_to_strings, read_json, restore, write_json,
};
#[cfg(test)]
use self::process::{command, output_with_timeout, replace_cli_server};

#[cfg(test)]
mod tests;
