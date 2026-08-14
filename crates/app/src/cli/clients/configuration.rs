use std::path::{Path, PathBuf};

use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, value};

use super::files::{
    ClientFiles, array_entry, backup, exists, object_entry, path_text, paths_to_strings, read_json,
    read_text, restore, write_json, write_private,
};
use super::process::{command, replace_cli_server};
use super::{SERVER, WRITE_TOOLS};
use crate::{AppError, ErrorCode, Paths, Result};

pub(super) fn configure_codex(
    paths: &Paths,
    config: &Path,
    executable: &str,
    bridge: &Path,
) -> Result<Vec<String>> {
    let backup = backup(paths, config, "codex")?;
    let result = (|| {
        replace_cli_server(
            executable,
            &["mcp", "remove", SERVER],
            &["mcp", "add", SERVER, "--", path_text(bridge)?, "serve"],
        )?;
        configure_codex_approvals(config)?;
        command(executable, &["mcp", "get", SERVER], false).map(|_| ())
    })();
    if let Err(error) = result {
        restore(config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

pub(super) fn configure_codex_approvals(path: &Path) -> Result<()> {
    let content = read_text(path)?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "Codex configuration is invalid"))?;
    let servers = toml_table_entry(document.as_table_mut(), "mcp_servers")?;
    let server = toml_table_entry(servers, SERVER)?;
    let tools = toml_table_entry(server, "tools")?;
    for tool in WRITE_TOOLS {
        toml_table_entry(tools, tool)?.insert("approval_mode", value("prompt"));
    }
    write_private(path, document.to_string().as_bytes(), "Codex configuration")
}

pub(super) fn configure_claude(
    paths: &Paths,
    files: &ClientFiles,
    executable: &str,
    bridge: &Path,
) -> Result<Vec<String>> {
    let mcp_config = &files.claude_mcp;
    let settings = &files.claude_settings;
    let mcp_backup = backup(paths, mcp_config, "claude-mcp")?;
    let settings_backup = backup(paths, settings, "claude-settings")?;
    let result = (|| {
        replace_cli_server(
            executable,
            &["mcp", "remove", "--scope", "user", SERVER],
            &["mcp", "add", "--scope", "user", SERVER, "--", path_text(bridge)?, "serve"],
        )?;
        let mut document = read_json(settings, false)?;
        let permissions = object_entry(&mut document, "permissions")?;
        let ask = array_entry(permissions, "ask")?;
        for tool in WRITE_TOOLS {
            let rule = Value::String(format!("mcp__{SERVER}__{tool}"));
            if !ask.contains(&rule) {
                ask.push(rule);
            }
        }
        write_json(settings, &document)
    })();
    if let Err(error) = result {
        let mcp_restore = restore(mcp_config, mcp_backup.as_deref());
        let settings_restore = restore(settings, settings_backup.as_deref());
        mcp_restore?;
        settings_restore?;
        return Err(error);
    }
    Ok(paths_to_strings(mcp_backup.into_iter().chain(settings_backup)))
}

pub(super) fn configure_opencode(
    paths: &Paths,
    config: &Path,
    bridge: &Path,
) -> Result<Vec<String>> {
    let mut document = read_json(config, true)?;
    let backup = backup(paths, config, "opencode-1")?;
    let result = (|| {
        let mcp = object_entry(&mut document, "mcp")?;
        mcp.insert(
            SERVER.into(),
            serde_json::json!({
                "type": "local",
                "command": [path_text(bridge)?, "serve"],
                "enabled": true,
            }),
        );
        let permissions = object_entry(&mut document, "permission")?;
        for tool in WRITE_TOOLS {
            permissions.insert(format!("{SERVER}_{tool}"), Value::String("ask".into()));
        }
        write_json(config, &document)
    })();
    if let Err(error) = result {
        restore(config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

pub(super) fn unconfigure_cli(
    paths: &Paths,
    executable: &str,
    config: PathBuf,
    claude: bool,
) -> Result<Vec<String>> {
    let backup = backup(paths, &config, "client-remove")?;
    let args = if claude {
        vec!["mcp", "remove", "--scope", "user", SERVER]
    } else {
        vec!["mcp", "remove", SERVER]
    };
    let result = command(executable, &args, true);
    if let Err(error) = result {
        restore(&config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

pub(super) fn unconfigure_claude(
    paths: &Paths,
    files: &ClientFiles,
    executable: &str,
) -> Result<Vec<String>> {
    let settings = &files.claude_settings;
    let mut backups = unconfigure_cli(paths, executable, files.claude_mcp.clone(), true)?;
    let backup = backup(paths, settings, "claude-settings-remove")?;
    if exists(settings) {
        let mut document = read_json(settings, false)?;
        if let Some(ask) = document
            .get_mut("permissions")
            .and_then(Value::as_object_mut)
            .and_then(|value| value.get_mut("ask"))
            .and_then(Value::as_array_mut)
        {
            ask.retain(|rule| {
                rule.as_str().is_none_or(|value| {
                    !WRITE_TOOLS.iter().any(|tool| value == format!("mcp__{SERVER}__{tool}"))
                })
            });
            write_json(settings, &document)?;
        }
    }
    backups.extend(paths_to_strings(backup.into_iter()));
    Ok(backups)
}

pub(super) fn unconfigure_opencode(paths: &Paths, config: &Path) -> Result<Vec<String>> {
    if !exists(config) {
        return Ok(Vec::new());
    }
    let mut document = read_json(config, true)?;
    let backup = backup(paths, config, "opencode-1-remove")?;
    if let Some(mcp) = document.get_mut("mcp").and_then(Value::as_object_mut) {
        mcp.remove(SERVER);
    }
    if let Some(permissions) = document.get_mut("permission").and_then(Value::as_object_mut) {
        for tool in WRITE_TOOLS {
            permissions.remove(&format!("{SERVER}_{tool}"));
        }
    }
    if let Err(error) = write_json(config, &document) {
        restore(config, backup.as_deref())?;
        return Err(error);
    }
    Ok(paths_to_strings(backup.into_iter()))
}

fn toml_table_entry<'a>(table: &'a mut Table, key: &str) -> Result<&'a mut Table> {
    let item = table.entry(key).or_insert_with(|| Item::Table(Table::new()));
    item.as_table_mut().ok_or_else(|| {
        AppError::new(ErrorCode::ConfigInvalid, "Codex configuration has an unsupported shape")
    })
}
