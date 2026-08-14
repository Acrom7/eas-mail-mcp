use std::process::{Command, Output, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt as _;

use super::ClientKind;
use crate::{AppError, ErrorCode, Result};

const CLIENT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn replace_cli_server(executable: &str, remove: &[&str], add: &[&str]) -> Result<()> {
    command(executable, remove, true)?;
    command(executable, add, false).map(|_| ())
}

pub(super) fn command(executable: &str, arguments: &[&str], allow_failure: bool) -> Result<bool> {
    let output = output_with_timeout(executable, arguments, CLIENT_COMMAND_TIMEOUT)?;
    let success = output.status.success();
    if success || allow_failure {
        Ok(success)
    } else {
        Err(AppError::new(ErrorCode::ConfigInvalid, "AI client rejected MCP configuration"))
    }
}

pub(super) fn detect_version(executable: &str) -> Result<String> {
    let output = output_with_timeout(executable, &["--version"], CLIENT_COMMAND_TIMEOUT)?;
    if !output.status.success() {
        return Err(AppError::new(ErrorCode::ConfigInvalid, "cannot determine AI client version"));
    }
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.split(|character: char| !character.is_ascii_digit() && character != '.')
        .find(|part| version_parts(part).is_some())
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::new(ErrorCode::ConfigInvalid, "cannot determine AI client version")
        })
}

pub(super) fn output_with_timeout(
    executable: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Output> {
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| AppError::new(ErrorCode::NotFound, "AI client executable is unavailable"))?;
    let status = child
        .wait_timeout(timeout)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "cannot monitor AI client command"))?;
    if status.is_none() {
        drop(child.kill());
        drop(child.wait());
        return Err(AppError::new(ErrorCode::ConfigInvalid, "AI client command timed out"));
    }
    child.wait_with_output().map_err(|_| {
        AppError::new(ErrorCode::ConfigInvalid, "cannot read AI client command output")
    })
}

pub(super) fn require_supported(client: ClientKind, version: &str) -> Result<()> {
    let Some((major, minor, _)) = version_parts(version) else {
        return Err(AppError::new(ErrorCode::ConfigInvalid, "AI client version is invalid"));
    };
    let supported = match client {
        ClientKind::Codex => major == 0 && minor == 133,
        ClientKind::Claude => major == 2 && minor == 1,
        ClientKind::Opencode => major == 1,
    };
    if supported {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "unknown AI client version; no configuration was changed",
        ))
    }
}

pub(super) fn version_parts(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.').map(|part| part.parse::<u64>().ok());
    match (parts.next()?, parts.next()?, parts.next()?, parts.next()) {
        (Some(major), Some(minor), Some(patch), None) => Some((major, minor, patch)),
        _ => None,
    }
}

pub(super) const fn client_name(client: ClientKind) -> &'static str {
    match client {
        ClientKind::Codex => "codex",
        ClientKind::Claude => "claude",
        ClientKind::Opencode => "opencode",
    }
}
