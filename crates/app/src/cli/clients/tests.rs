#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use toml_edit::DocumentMut;

use super::*;

#[test]
fn supported_versions_are_exactly_bounded() {
    for (kind, accepted, rejected) in [
        (ClientKind::Codex, "0.133.0", "0.134.0"),
        (ClientKind::Claude, "2.1.160", "2.2.0"),
        (ClientKind::Opencode, "1.14.23", "2.0.0"),
    ] {
        assert!(require_supported(kind, accepted).is_ok());
        assert!(require_supported(kind, rejected).is_err());
    }
    assert_eq!(version_parts("1.2.3"), Some((1, 2, 3)));
    assert_eq!(version_parts("1.2"), None);
    assert_eq!(version_parts("1.2.3.4"), None);
    assert!(require_supported(ClientKind::Codex, "invalid").is_err());
}

#[test]
fn black_box_executable_version_and_command_fail_closed() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let success = script(directory.path(), "success", "echo 'tool 0.133.7'\nexit 0")?;
    let failure = script(directory.path(), "failure", "echo 'broken' >&2\nexit 7")?;
    assert_eq!(detect_version(path_text(&success)?)?, "0.133.7");
    assert!(detect_version(path_text(&failure)?).is_err());
    assert!(detect_version("/missing/client").is_err());
    assert!(command(path_text(&success)?, &["ignored"], false)?);
    assert!(!command(path_text(&failure)?, &["ignored"], true)?);
    assert!(command(path_text(&failure)?, &["ignored"], false).is_err());
    assert!(command("/missing/client", &[], false).is_err());
    Ok(())
}

#[test]
fn black_box_client_command_has_a_hard_timeout() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let hanging = script(directory.path(), "hanging", "while :; do :; done")?;
    let started = Instant::now();
    assert!(output_with_timeout(path_text(&hanging)?, &[], Duration::from_millis(50)).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn codex_approval_edit_preserves_document_and_prompts_all_writes() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    fs::write(&path, "# keep this comment\n[mcp_servers.eas-mail]\ncommand = \"/tmp/server\"\n")?;
    configure_codex_approvals(&path)?;
    let content = fs::read_to_string(&path)?;
    assert!(content.contains("# keep this comment"));
    let document = content.parse::<DocumentMut>()?;
    for tool in WRITE_TOOLS {
        assert_eq!(
            document["mcp_servers"][SERVER]["tools"][tool]["approval_mode"].as_str(),
            Some("prompt")
        );
    }
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    fs::write(&path, "not = [toml")?;
    assert!(configure_codex_approvals(&path).is_err());
    Ok(())
}

#[test]
fn json_and_jsonc_round_trip_validate_shape() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/config.json");
    assert_eq!(read_json(&path, false)?, serde_json::json!({}));
    let value = serde_json::json!({"existing": true});
    write_json(&path, &value)?;
    assert_eq!(read_json(&path, false)?, value);
    assert!(fs::read_to_string(&path)?.ends_with('\n'));

    fs::write(&path, "{ // comment\n existing: true,\n}")?;
    assert_eq!(read_json(&path, true)?["existing"], true);
    assert!(read_json(&path, false).is_err());
    fs::write(&path, "[]")?;
    assert!(read_json(&path, false).is_err());
    Ok(())
}

#[test]
fn config_shape_helpers_reject_existing_wrong_types() -> anyhow::Result<()> {
    let mut document = serde_json::json!({});
    let object = object_entry(&mut document, "permissions")?;
    let ask = array_entry(object, "ask")?;
    ask.push(Value::String("rule".into()));
    assert_eq!(document["permissions"]["ask"][0], "rule");

    let mut wrong_root = Value::Array(Vec::new());
    assert!(object_entry(&mut wrong_root, "x").is_err());
    let mut wrong_child = serde_json::json!({"x": []});
    assert!(object_entry(&mut wrong_child, "x").is_err());
    let mut map = Map::from_iter([("ask".into(), Value::Object(Map::new()))]);
    assert!(array_entry(&mut map, "ask").is_err());
    Ok(())
}

#[test]
fn backups_are_private_and_restore_exact_or_absent_state() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    let source = directory.path().join("client.json");
    assert_eq!(backup(&paths, &source, "missing")?, None);
    fs::write(&source, "before")?;
    let saved = backup(&paths, &source, "client")?
        .ok_or_else(|| anyhow::anyhow!("backup was not created"))?;
    fs::write(&source, "after")?;
    restore(&source, Some(&saved))?;
    assert_eq!(fs::read_to_string(&source)?, "before");
    assert_eq!(fs::metadata(&saved)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(saved.parent().ok_or_else(|| anyhow::anyhow!("backup parent missing"))?)?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    restore(&source, None)?;
    assert!(!source.exists());
    assert_eq!(paths_to_strings([saved].into_iter()).len(), 1);
    Ok(())
}

#[test]
fn black_box_replace_server_does_not_launch_the_existing_server() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let calls = directory.path().join("calls.log");
    let executable = script(
        directory.path(),
        "client",
        &format!(
            "printf '%s\\n' \"$*\" >> '{}'\n\
             if [ \"$1\" = mcp ] && [ \"$2\" = remove ]; then exit 9; fi\n\
             exit 0",
            calls.display()
        ),
    )?;
    replace_cli_server(path_text(&executable)?, &["mcp", "remove"], &["mcp", "add"])?;
    assert_eq!(fs::read_to_string(calls)?, "mcp remove\nmcp add\n");
    assert_eq!(client_name(ClientKind::Codex), "codex");
    assert_eq!(client_name(ClientKind::Claude), "claude");
    assert_eq!(client_name(ClientKind::Opencode), "opencode");
    Ok(())
}

fn script(directory: &Path, name: &str, body: &str) -> anyhow::Result<PathBuf> {
    let path = directory.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn test_paths(root: &Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("attachments"),
        config: root.join("config.toml"),
        journal: root.join("operations.sqlite"),
    }
}

mod configuration;
