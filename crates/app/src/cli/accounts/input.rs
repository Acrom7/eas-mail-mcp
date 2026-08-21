use std::io::BufRead as _;

use eas_mail_protocol::{ProfileKey, ProfileRegistry};
use zeroize::Zeroizing;

use super::super::SetupArgs;
use super::super::terminal::Terminal;
use crate::backend::VerificationStage;
use crate::{AppConfig, AppError, ErrorCode, Paths, Result, load_config};

pub(in crate::cli) struct AddRequest {
    pub(in crate::cli) account_id: String,
    pub(in crate::cli) profile: ProfileKey,
    pub(in crate::cli) email: String,
    pub(in crate::cli) username: String,
    pub(super) password: Zeroizing<String>,
    pub(in crate::cli) write_enabled: bool,
}

pub(in crate::cli) fn collect_request(
    paths: &Paths,
    arguments: SetupArgs,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
) -> Result<AddRequest> {
    let profile = select_profile(arguments.profile, profiles, terminal)?;
    let selected = profiles.require(&profile).map_err(AppError::from)?;
    let config = load_config(&paths.config)?;
    let account_id =
        arguments.account_id.unwrap_or_else(|| next_account_id(profile.as_str(), &config));
    if !crate::config::valid_account_id(&account_id) {
        return Err(AppError::new(ErrorCode::ValidationFailed, "account identifier is invalid"));
    }
    let email = required(arguments.email, "Mailbox email", terminal)?;
    if let Some(hint) = selected.username_hint() {
        terminal.message(&format!("Username hint: {hint}"))?;
    }
    let username_input = match selected.identity_mode() {
        eas_mail_protocol::IdentityMode::Email => arguments.username,
        eas_mail_protocol::IdentityMode::Username => {
            Some(required(arguments.username, "Exchange username", terminal)?)
        }
        eas_mail_protocol::IdentityMode::RealmUsername => {
            Some(required(arguments.username, "Username (realm is added automatically)", terminal)?)
        }
    };
    let username =
        selected.canonical_username(&email, username_input.as_deref()).map_err(AppError::from)?;
    terminal.message(&format!("Authentication username: {username}"))?;
    let password = read_password(arguments.password_stdin, terminal)?;
    validate_password(&password)?;
    Ok(AddRequest {
        account_id,
        profile,
        email,
        username,
        password,
        write_enabled: arguments.enable_writes,
    })
}

pub(super) fn select_profile_interactive(
    current: &ProfileKey,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
) -> Result<ProfileKey> {
    let available = profiles.profiles();
    let options = available
        .iter()
        .map(|profile| format!("{} ({})", profile.display_name(), profile.key()))
        .collect::<Vec<_>>();
    let default =
        available.iter().position(|profile| profile.key() == current.as_str()).unwrap_or(0);
    let index = terminal.select("Endpoint profile", &options, default)?;
    selected_profile(available, index)
}

pub(super) fn read_password(
    from_stdin: bool,
    terminal: &mut dyn Terminal,
) -> Result<Zeroizing<String>> {
    let value = if from_stdin {
        let mut value = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut value)
            .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot read password"))?;
        value.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        return terminal.password("Exchange password");
    };
    Ok(Zeroizing::new(value))
}

pub(super) fn username_default(username: &str, realm: Option<&str>) -> String {
    realm
        .and_then(|required| {
            username
                .split_once('\\')
                .filter(|(actual, _)| actual.eq_ignore_ascii_case(required))
                .map(|(_, local)| local.to_owned())
        })
        .unwrap_or_else(|| username.to_owned())
}

pub(super) fn validate_password(password: &str) -> Result<()> {
    if password.is_empty() || password.contains(['\0', '\r', '\n']) {
        Err(AppError::new(ErrorCode::ValidationFailed, "password is empty or malformed"))
    } else {
        Ok(())
    }
}

pub(super) fn report_stage(
    terminal: &mut Option<&mut dyn Terminal>,
    stage: VerificationStage,
) -> Result<()> {
    if let Some(terminal) = terminal.as_deref_mut()
        && terminal.is_interactive()
    {
        terminal.message(stage.message())?;
    }
    Ok(())
}

pub(super) fn required(
    value: Option<String>,
    label: &str,
    terminal: &mut dyn Terminal,
) -> Result<String> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => Err(AppError::new(ErrorCode::ValidationFailed, "required value is empty")),
        None => terminal.input(label, None),
    }
}

fn select_profile(
    selected: Option<ProfileKey>,
    profiles: &ProfileRegistry,
    terminal: &mut dyn Terminal,
) -> Result<ProfileKey> {
    if let Some(selected) = selected {
        profiles.require(&selected).map_err(AppError::from)?;
        return Ok(selected);
    }
    let available = profiles.profiles();
    if let [profile] = available {
        return ProfileKey::new(profile.key()).map_err(AppError::from);
    }
    let options = available
        .iter()
        .map(|profile| format!("{} ({})", profile.display_name(), profile.key()))
        .collect::<Vec<_>>();
    let index = terminal.select("Select an endpoint profile", &options, 0)?;
    selected_profile(available, index)
}

fn selected_profile(profiles: &[eas_mail_protocol::Profile], index: usize) -> Result<ProfileKey> {
    let profile = profiles.get(index).ok_or_else(|| {
        AppError::new(ErrorCode::ValidationFailed, "selected profile is unavailable")
    })?;
    ProfileKey::new(profile.key()).map_err(AppError::from)
}

fn next_account_id(profile: &str, config: &AppConfig) -> String {
    if !config.accounts.contains_key(profile) {
        return profile.to_owned();
    }
    (2..)
        .map(|suffix| format!("{profile}-{suffix}"))
        .find(|candidate| !config.accounts.contains_key(candidate))
        .unwrap_or_else(|| format!("{profile}-new"))
}
