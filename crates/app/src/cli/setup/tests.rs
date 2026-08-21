#![expect(
    clippy::indexing_slicing,
    reason = "fixed setup fixtures use direct indexing for readable assertions"
)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use super::actions::SetupActions;
use super::*;
use crate::cli::terminal::testing::ScriptedTerminal;
use crate::{AccountConfig, AppConfig, load_profile_bundle, save_config};

mod basics;

enum AddOutcome {
    Success { writes_supported: bool },
    AuthRequired,
    AccessDenied,
}

struct ScriptedActions {
    outcomes: Mutex<VecDeque<AddOutcome>>,
    add_calls: AtomicUsize,
}

impl ScriptedActions {
    fn new(outcomes: impl IntoIterator<Item = AddOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            add_calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.add_calls.load(Ordering::SeqCst)
    }
}

#[async_trait(?Send)]
impl SetupActions for ScriptedActions {
    async fn add_account(
        &self,
        paths: &Paths,
        request: accounts::AddRequest,
        _: &ProfileRegistry,
        _: &mut dyn Terminal,
    ) -> Result<serde_json::Value> {
        self.add_calls.fetch_add(1, Ordering::SeqCst);
        let outcome = self
            .outcomes
            .lock()
            .map_err(|_| AppError::new(ErrorCode::StorageError, "scripted setup state failed"))?
            .pop_front()
            .unwrap_or(AddOutcome::Success { writes_supported: true });
        let writes_supported = match outcome {
            AddOutcome::AuthRequired => {
                return Err(AppError::new(
                    ErrorCode::AuthRequired,
                    "Exchange rejected the account credentials",
                ));
            }
            AddOutcome::AccessDenied => {
                return Err(AppError::new(
                    ErrorCode::AccessDenied,
                    "Exchange denied ActiveSync access for this account",
                ));
            }
            AddOutcome::Success { writes_supported } => writes_supported,
        };
        let mut config = load_config(&paths.config)?;
        config.accounts.insert(
            request.account_id.clone(),
            AccountConfig {
                profile: request.profile,
                email: request.email,
                username: request.username,
                enabled: true,
                write_enabled: request.write_enabled,
            },
        );
        save_config(&paths.config, &config)?;
        Ok(serde_json::json!({
            "account_id": request.account_id,
            "configured": true,
            "write_enabled": request.write_enabled,
            "writes_supported": writes_supported,
            "folders_verified": 3,
        }))
    }

    async fn repair_account(
        &self,
        _: &Paths,
        account_id: &str,
        _: &ProfileRegistry,
        _: &mut dyn Terminal,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "account_id": account_id, "repaired": true }))
    }

    async fn update_password(
        &self,
        _: &Paths,
        account_id: &str,
        _: &ProfileRegistry,
        _: &mut dyn Terminal,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "account_id": account_id, "password_updated": true }))
    }

    async fn set_writes_checked(
        &self,
        paths: &Paths,
        account_id: &str,
        enabled: bool,
        _: &ProfileRegistry,
    ) -> Result<serde_json::Value> {
        set_write_config(paths, account_id, enabled)
    }

    fn set_verified_writes(&self, paths: &Paths, account_id: &str) -> Result<serde_json::Value> {
        set_write_config(paths, account_id, true)
    }

    fn configure_clients(&self, _: &Paths, _: &mut dyn Terminal) -> Result<Vec<serde_json::Value>> {
        Ok(vec![serde_json::json!({ "client": "fixture", "configured": true })])
    }

    async fn doctor(&self, paths: &Paths, _: &ProfileRegistry) -> Result<serde_json::Value> {
        let accounts = load_config(&paths.config)?
            .accounts
            .keys()
            .map(|account_id| serde_json::json!({ "account_id": account_id, "status": "ok" }))
            .collect::<Vec<_>>();
        Ok(serde_json::json!({ "accounts": accounts }))
    }
}

#[tokio::test]
async fn first_run_creates_a_profile_and_multiple_realm_accounts() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    paths.ensure()?;
    let mut terminal = ScriptedTerminal::new(
        &[
            "2",
            "Example Mail",
            "mail.example.invalid",
            "example.invalid",
            "3",
            "EXAMPLE",
            "Short login",
            "",
            "1",
            "first@example.invalid",
            "first",
            "",
            "y",
            "second@example.invalid",
            "EXAMPLE\\second",
            "",
            "",
        ],
        &["first-password", "second-password"],
    );
    let actions = ScriptedActions::new([]);
    let result = run_with_actions(&paths, blank_setup_args(), &mut terminal, &actions).await?;

    assert_eq!(actions.calls(), 2);
    assert_eq!(result["accounts"].as_array().map(Vec::len), Some(2));
    let config = load_config(&paths.config)?;
    assert_eq!(config.accounts["mail"].username, "EXAMPLE\\first");
    assert_eq!(config.accounts["mail-2"].username, "EXAMPLE\\second");
    assert!(!config.accounts["mail"].write_enabled);
    let profiles = load_profile_bundle(&paths.profiles)?
        .ok_or_else(|| anyhow::anyhow!("profile store is missing"))?;
    assert_eq!(profiles.source_schema_version, 2);
    assert_eq!(
        profiles.manifest.profiles[0].identity.username_hint.as_deref(),
        Some("Short login")
    );
    Ok(())
}

#[tokio::test]
async fn imported_profile_retries_auth_and_access_errors_without_partial_accounts()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    paths.ensure()?;
    let profile_file = directory.path().join("team profile.toml");
    std::fs::write(&profile_file, include_str!("../../../../../profile.example.toml"))?;
    let mut arguments = blank_setup_args();
    arguments.profile_file = Some(profile_file);
    let mut terminal = ScriptedTerminal::new(
        &[
            "bad-one@example.invalid",
            "bad-one",
            "y",
            "bad-two@example.invalid",
            "bad-two",
            "y",
            "good@example.invalid",
            "good",
            "",
            "",
        ],
        &["bad-password", "bad-password", "good-password"],
    );
    let actions = ScriptedActions::new([
        AddOutcome::AuthRequired,
        AddOutcome::AccessDenied,
        AddOutcome::Success { writes_supported: true },
    ]);
    run_with_actions(&paths, arguments, &mut terminal, &actions).await?;

    assert_eq!(actions.calls(), 3);
    let config = load_config(&paths.config)?;
    assert_eq!(config.accounts.len(), 1);
    assert_eq!(config.accounts["example"].email, "good@example.invalid");
    assert!(terminal.transcript.iter().any(|line| line.contains("AUTH_REQUIRED")));
    assert!(terminal.transcript.iter().any(|line| line.contains("ACCESS_DENIED")));
    Ok(())
}

#[tokio::test]
async fn failed_account_can_be_cancelled_without_persisting_config() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    install_example_profile(&paths)?;
    let mut terminal =
        ScriptedTerminal::new(&["bad@example.invalid", "bad", "n"], &["bad-password"]);
    let actions = ScriptedActions::new([AddOutcome::AuthRequired]);
    let error = match run_with_actions(&paths, blank_setup_args(), &mut terminal, &actions).await {
        Err(error) => error,
        Ok(_) => return Err(anyhow::anyhow!("cancelled account setup unexpectedly succeeded")),
    };
    assert_eq!(error.envelope.code, ErrorCode::AuthRequired);
    assert!(load_config(&paths.config)?.accounts.is_empty());
    Ok(())
}

#[tokio::test]
async fn repeated_setup_opens_management_and_routes_password_updates() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    install_example_profile(&paths)?;
    let mut config = AppConfig::default();
    config.accounts.insert(
        "example".into(),
        AccountConfig {
            profile: eas_mail_protocol::ProfileKey::new("example")?,
            email: "user@example.invalid".into(),
            username: "example_user".into(),
            enabled: true,
            write_enabled: false,
        },
    );
    save_config(&paths.config, &config)?;
    let mut terminal = ScriptedTerminal::new(&["3", "", "8"], &[]);
    let actions = ScriptedActions::new([]);
    let result = run_with_actions(&paths, blank_setup_args(), &mut terminal, &actions).await?;
    assert_eq!(actions.calls(), 0);
    assert_eq!(result["accounts"][0]["password_updated"], true);
    Ok(())
}

#[tokio::test]
async fn repeated_setup_routes_every_management_action() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = test_paths(directory.path());
    install_example_profile(&paths)?;
    let mut config = AppConfig::default();
    config.accounts.insert(
        "example".into(),
        AccountConfig {
            profile: eas_mail_protocol::ProfileKey::new("example")?,
            email: "first@example.invalid".into(),
            username: "first".into(),
            enabled: true,
            write_enabled: false,
        },
    );
    save_config(&paths.config, &config)?;
    let mut terminal = ScriptedTerminal::new(
        &[
            "1",
            "second@example.invalid",
            "second",
            "",
            "2",
            "",
            "3",
            "",
            "4",
            "",
            "y",
            "5",
            "3",
            "6",
            "7",
            "8",
        ],
        &["fixture-value"],
    );
    let actions = ScriptedActions::new([]);
    let mut arguments = blank_setup_args();
    arguments.skip_clients = false;

    let result = run_with_actions(&paths, arguments, &mut terminal, &actions).await?;

    assert_eq!(actions.calls(), 1);
    assert_eq!(result["accounts"].as_array().map(Vec::len), Some(4));
    assert_eq!(result["clients"].as_array().map(Vec::len), Some(1));
    assert!(load_config(&paths.config)?.accounts["example"].write_enabled);
    assert!(terminal.transcript.iter().any(|line| line.contains("sha256")));
    assert!(terminal.transcript.iter().any(|line| line.contains("Diagnostics completed")));
    Ok(())
}

fn set_write_config(paths: &Paths, account_id: &str, enabled: bool) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    let account = config
        .accounts
        .get_mut(account_id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "scripted account is not configured"))?;
    account.write_enabled = enabled;
    save_config(&paths.config, &config)?;
    Ok(serde_json::json!({ "account_id": account_id, "write_enabled": enabled }))
}

fn install_example_profile(paths: &Paths) -> anyhow::Result<()> {
    paths.ensure()?;
    std::fs::write(&paths.profiles, include_str!("../../../../../profile.example.toml"))?;
    Ok(())
}

fn test_paths(root: &Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("cache/attachments"),
        config: root.join("support/config.toml"),
        profiles: root.join("support/profiles.toml"),
        journal: root.join("support/operations.sqlite"),
    }
}
