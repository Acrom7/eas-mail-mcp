mod input;
mod verification;

use std::sync::Arc;

use eas_mail_protocol::ProfileRegistry;

#[cfg(test)]
use self::input::required;
pub(super) use self::input::{AddRequest, collect_request};
use self::input::{read_password, select_profile_interactive, username_default, validate_password};
use self::verification::{AccountVerifier, EasAccountVerifier};
use super::account_secrets::replace_optional as replace_secret_optional;
use super::account_secrets::restore as restore_secret;
use super::account_secrets::{open as secret_store, replace as replace_secret};
use super::terminal::Terminal;
use super::{AccountCommand, AddAccountArgs, SetupArgs, Toggle};
use crate::{
    AccountConfig, AccountSecret, AppError, ErrorCode, MemorySecretStore, Paths, Result,
    SecretBundle, SecretStore, load_config, save_config,
};

pub(super) async fn run(
    paths: &Paths,
    command: AccountCommand,
    profiles: Option<&ProfileRegistry>,
    terminal: &mut dyn Terminal,
) -> Result<serde_json::Value> {
    match command {
        AccountCommand::List => list(paths),
        AccountCommand::Add(arguments) => {
            let profiles = require_profiles(profiles)?;
            let request = collect_request(paths, arguments.into(), profiles, terminal)?;
            add(paths, request, profiles, Some(terminal)).await
        }
        AccountCommand::UpdatePassword(arguments) => {
            update_password(
                paths,
                &arguments.account_id,
                arguments.password_stdin,
                require_profiles(profiles)?,
                terminal,
            )
            .await
        }
        AccountCommand::SetWrites(arguments) => {
            set_writes_checked(
                paths,
                &arguments.account_id,
                matches!(arguments.value, Toggle::On),
                require_profiles(profiles)?,
            )
            .await
        }
        AccountCommand::Remove(arguments) => remove(paths, &arguments.account_id),
    }
}

pub(super) async fn add(
    paths: &Paths,
    request: AddRequest,
    profiles: &ProfileRegistry,
    terminal: Option<&mut dyn Terminal>,
) -> Result<serde_json::Value> {
    let store = secret_store(paths);
    add_with_dependencies(paths, request, profiles, terminal, &store, &EasAccountVerifier).await
}

async fn add_with_dependencies(
    paths: &Paths,
    request: AddRequest,
    profiles: &ProfileRegistry,
    terminal: Option<&mut dyn Terminal>,
    store: &Arc<dyn SecretStore>,
    verifier: &dyn AccountVerifier,
) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    if config.accounts.contains_key(&request.account_id) {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "account identifier already exists",
        ));
    }
    let account = AccountConfig {
        profile: request.profile.clone(),
        email: request.email,
        username: request.username,
        enabled: true,
        write_enabled: request.write_enabled,
    };
    account.validate(profiles)?;
    let profile = profiles.require(&request.profile).map_err(AppError::from)?;
    let candidate = AccountSecret {
        password: request.password.to_string(),
        device_id: SecretBundle::device_id(profile.device_id_length())?,
        policy_key: 0,
        policy: None,
    };
    let (candidate, folders, writes_supported) =
        verify_candidate(&request.account_id, &account, candidate, profiles, terminal, verifier)
            .await?;
    if request.write_enabled && !writes_supported {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "Exchange does not advertise every required write command",
        ));
    }
    let original = replace_secret(store, &request.account_id, candidate.clone())?;
    config.accounts.insert(request.account_id.clone(), account);
    if let Err(error) = save_config(&paths.config, &config) {
        restore_secret(store, &request.account_id, Some(&candidate), original.as_ref())?;
        return Err(error);
    }
    Ok(serde_json::json!({
        "account_id": request.account_id,
        "configured": true,
        "write_enabled": request.write_enabled,
        "writes_supported": writes_supported,
        "folders_verified": folders,
    }))
}

pub(super) async fn update_password(
    paths: &Paths,
    account_id: &str,
    password_stdin: bool,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
) -> Result<serde_json::Value> {
    let store = secret_store(paths);
    update_password_with_dependencies(
        paths,
        account_id,
        password_stdin,
        profiles,
        terminal,
        &store,
        &EasAccountVerifier,
    )
    .await
}

async fn update_password_with_dependencies(
    paths: &Paths,
    account_id: &str,
    password_stdin: bool,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
    store: &Arc<dyn SecretStore>,
    verifier: &dyn AccountVerifier,
) -> Result<serde_json::Value> {
    let config = load_config(&paths.config)?;
    let account = config.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
    })?;
    let password = read_password(password_stdin, terminal)?;
    validate_password(&password)?;
    let original = store.load()?.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
            .account(account_id)
    })?;
    let mut candidate = original.clone();
    candidate.password = password.to_string();
    let (candidate, folders, writes_supported) =
        verify_candidate(account_id, &account, candidate, profiles, Some(terminal), verifier)
            .await?;
    replace_secret(store, account_id, candidate)?;
    Ok(serde_json::json!({
        "account_id": account_id,
        "password_updated": true,
        "writes_supported": writes_supported,
        "folders_verified": folders,
    }))
}

async fn verify_candidate(
    account_id: &str,
    account: &AccountConfig,
    candidate: AccountSecret,
    profiles: &ProfileRegistry,
    terminal: Option<&mut dyn Terminal>,
    verifier: &dyn AccountVerifier,
) -> Result<(AccountSecret, usize, bool)> {
    let mut bundle = SecretBundle::new();
    bundle.accounts.insert(account_id.to_owned(), candidate);
    let memory: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::with_bundle(bundle));
    let (folders, writes_supported) =
        verifier.verify(account_id, account, Arc::clone(&memory), profiles, terminal).await?;
    let verified = memory.load()?.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::RemoteWipe, "Exchange removed the temporary account state")
            .account(account_id)
    })?;
    Ok((verified, folders, writes_supported))
}

fn list(paths: &Paths) -> Result<serde_json::Value> {
    let config = load_config(&paths.config)?;
    let accounts = config
        .accounts
        .into_iter()
        .map(|(account_id, account)| {
            serde_json::json!({
                "account_id": account_id,
                "profile": account.profile.as_str(),
                "email": account.email,
                "enabled": account.enabled,
                "write_enabled": account.write_enabled,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "accounts": accounts }))
}

pub(super) fn set_writes(
    paths: &Paths,
    account_id: &str,
    enabled: bool,
) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    let account = config.accounts.get_mut(account_id).ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
    })?;
    account.write_enabled = enabled;
    save_config(&paths.config, &config)?;
    Ok(serde_json::json!({ "account_id": account_id, "write_enabled": enabled }))
}

pub(super) async fn set_writes_checked(
    paths: &Paths,
    account_id: &str,
    enabled: bool,
    profiles: &ProfileRegistry,
) -> Result<serde_json::Value> {
    let store = secret_store(paths);
    set_writes_checked_with_dependencies(
        paths,
        account_id,
        enabled,
        profiles,
        &store,
        &EasAccountVerifier,
    )
    .await
}

async fn set_writes_checked_with_dependencies(
    paths: &Paths,
    account_id: &str,
    enabled: bool,
    profiles: &ProfileRegistry,
    store: &Arc<dyn SecretStore>,
    verifier: &dyn AccountVerifier,
) -> Result<serde_json::Value> {
    if !enabled {
        return set_writes(paths, account_id, false);
    }
    let config = load_config(&paths.config)?;
    let account = config.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
    })?;
    let (_, writes_supported) =
        verifier.verify(account_id, &account, Arc::clone(store), profiles, None).await?;
    if !writes_supported {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "Exchange does not advertise every required write command",
        )
        .account(account_id));
    }
    set_writes(paths, account_id, true)
}

pub(super) async fn repair(
    paths: &Paths,
    account_id: &str,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
) -> Result<serde_json::Value> {
    let store = secret_store(paths);
    repair_with_dependencies(paths, account_id, profiles, terminal, &store, &EasAccountVerifier)
        .await
}

async fn repair_with_dependencies(
    paths: &Paths,
    account_id: &str,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
    store: &Arc<dyn SecretStore>,
    verifier: &dyn AccountVerifier,
) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    let original_account = config.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
    })?;
    let profile = select_profile_interactive(&original_account.profile, profiles, terminal)?;
    let selected = profiles.require(&profile).map_err(AppError::from)?;
    let email = terminal.input("Mailbox email", Some(&original_account.email))?;
    if let Some(hint) = selected.username_hint() {
        terminal.message(&format!("Username hint: {hint}"))?;
    }
    let username_default = username_default(&original_account.username, selected.username_realm());
    let username_input = match selected.identity_mode() {
        eas_mail_protocol::IdentityMode::Email => None,
        eas_mail_protocol::IdentityMode::Username => {
            Some(terminal.input("Exchange username", Some(&username_default))?)
        }
        eas_mail_protocol::IdentityMode::RealmUsername => Some(
            terminal.input("Username (realm is added automatically)", Some(&username_default))?,
        ),
    };
    let username =
        selected.canonical_username(&email, username_input.as_deref()).map_err(AppError::from)?;
    terminal.message(&format!("Authentication username: {username}"))?;
    let password = terminal.password("Exchange password")?;
    validate_password(&password)?;
    let account = AccountConfig {
        profile: profile.clone(),
        email,
        username,
        enabled: original_account.enabled,
        write_enabled: original_account.write_enabled,
    };
    account.validate(profiles)?;
    let original_secret = store.load()?.accounts.get(account_id).cloned().ok_or_else(|| {
        AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
            .account(account_id)
    })?;
    let device_id = if profile == original_account.profile {
        original_secret.device_id.clone()
    } else {
        SecretBundle::device_id(selected.device_id_length())?
    };
    let candidate =
        AccountSecret { password: password.to_string(), device_id, policy_key: 0, policy: None };
    let (candidate, folders, writes_supported) =
        verify_candidate(account_id, &account, candidate, profiles, Some(terminal), verifier)
            .await?;
    let mut account = account;
    if account.write_enabled && !writes_supported {
        account.write_enabled = false;
    }
    replace_secret(store, account_id, candidate.clone())?;
    config.accounts.insert(account_id.to_owned(), account.clone());
    if let Err(error) = save_config(&paths.config, &config) {
        restore_secret(store, account_id, Some(&candidate), Some(&original_secret))?;
        return Err(error);
    }
    Ok(serde_json::json!({
        "account_id": account_id,
        "repaired": true,
        "write_enabled": account.write_enabled,
        "writes_supported": writes_supported,
        "folders_verified": folders,
    }))
}

fn remove(paths: &Paths, account_id: &str) -> Result<serde_json::Value> {
    let store = secret_store(paths);
    remove_with_store(paths, account_id, &store)
}

fn remove_with_store(
    paths: &Paths,
    account_id: &str,
    store: &Arc<dyn SecretStore>,
) -> Result<serde_json::Value> {
    let mut config = load_config(&paths.config)?;
    if config.accounts.remove(account_id).is_none() {
        return Err(
            AppError::new(ErrorCode::NotFound, "account is not configured").account(account_id)
        );
    }
    let original = replace_secret_optional(store, account_id, None)?;
    if let Err(error) = save_config(&paths.config, &config) {
        restore_secret(store, account_id, None, original.as_ref())?;
        return Err(error);
    }
    Ok(serde_json::json!({ "account_id": account_id, "removed": true }))
}

impl From<AddAccountArgs> for SetupArgs {
    fn from(arguments: AddAccountArgs) -> Self {
        Self {
            profile_file: None,
            account_id: arguments.account_id,
            profile: arguments.profile,
            email: arguments.email,
            username: arguments.username,
            password_stdin: arguments.password_stdin,
            enable_writes: arguments.enable_writes,
            skip_clients: true,
        }
    }
}

fn require_profiles(profiles: Option<&ProfileRegistry>) -> Result<&ProfileRegistry> {
    profiles.ok_or_else(|| {
        AppError::new(ErrorCode::ConfigInvalid, "no EAS endpoint profiles are configured")
            .remediation("Run eas-mail-mcp setup or profile import")
    })
}

#[cfg(test)]
mod tests;
