use std::fs;
use std::path::Path;

use eas_mail_profile::{IdentityMode, IdentitySpec, ProfileSpec, TrustSpec};
use eas_mail_protocol::ProfileKey;

use super::super::terminal::Terminal;
use super::super::{ProfileAddArgs, ProfileIdentityMode};
use super::profile_error;
use crate::platform;
use crate::{AppError, ErrorCode, Result};

pub(super) fn interactive_profile(
    arguments: ProfileAddArgs,
    terminal: &mut dyn Terminal,
) -> Result<ProfileSpec> {
    let interactive = arguments.id.is_none()
        && arguments.display_name.is_none()
        && arguments.host.is_none()
        && arguments.email_domains.is_empty()
        && arguments.identity_mode.is_none()
        && arguments.username_realm.is_none()
        && arguments.username_hint.is_none()
        && arguments.device_id_length.is_none()
        && arguments.pem.is_none();
    let display_name = required(arguments.display_name, "Display name", terminal)?;
    let host = required(arguments.host, "Exchange host", terminal)?.to_ascii_lowercase();
    let id = arguments.id.unwrap_or_else(|| profile_id(&host));
    ProfileKey::new(id.clone()).map_err(AppError::from)?;
    let email_domains = domains(arguments.email_domains, terminal)?;
    let identity = identity(
        arguments.identity_mode,
        arguments.username_realm,
        arguments.username_hint,
        interactive,
        terminal,
    )?;
    let device_id_length = device_id_length(arguments.device_id_length, interactive, terminal)?;
    let pem = match arguments.pem {
        Some(path) => Some(path),
        None if interactive => {
            let trust = terminal.select(
                "TLS trust",
                &["System trust store".into(), "Exclusive PEM certificate".into()],
                0,
            )?;
            if trust == 1 {
                Some(terminal.input("PEM certificate path", None)?.into())
            } else {
                None
            }
        }
        None => None,
    };
    Ok(ProfileSpec {
        id,
        display_name,
        host,
        email_domains,
        identity,
        device_id_length,
        trust: trust(pem.as_deref())?,
    })
}

fn identity(
    mode: Option<ProfileIdentityMode>,
    legacy_realm: Option<String>,
    hint: Option<String>,
    interactive: bool,
    terminal: &mut dyn Terminal,
) -> Result<IdentitySpec> {
    let mode = match (mode, legacy_realm.as_ref()) {
        (Some(ProfileIdentityMode::Email), Some(_))
        | (Some(ProfileIdentityMode::Username), Some(_)) => return Err(invalid_value()),
        (Some(value), _) => value,
        (None, Some(_)) => ProfileIdentityMode::RealmUsername,
        (None, None) if interactive => match terminal.select(
            "Authentication username format",
            &["Username".into(), "Email address".into(), "Realm + username".into()],
            0,
        )? {
            0 => ProfileIdentityMode::Username,
            1 => ProfileIdentityMode::Email,
            _ => ProfileIdentityMode::RealmUsername,
        },
        (None, None) => ProfileIdentityMode::Username,
    };
    let realm = match mode {
        ProfileIdentityMode::RealmUsername => match legacy_realm {
            Some(value) => Some(value),
            None => optional(terminal.input("Username realm", None)?),
        },
        ProfileIdentityMode::Email | ProfileIdentityMode::Username => None,
    };
    let username_hint = match hint {
        Some(value) => optional(value),
        None if interactive && !matches!(mode, ProfileIdentityMode::Email) => {
            optional(terminal.input("Username hint (optional)", None)?)
        }
        None => None,
    };
    Ok(IdentitySpec { mode: mode.into(), realm, username_hint })
}

impl From<ProfileIdentityMode> for IdentityMode {
    fn from(value: ProfileIdentityMode) -> Self {
        match value {
            ProfileIdentityMode::Email => Self::Email,
            ProfileIdentityMode::Username => Self::Username,
            ProfileIdentityMode::RealmUsername => Self::RealmUsername,
        }
    }
}

fn domains(values: Vec<String>, terminal: &mut dyn Terminal) -> Result<Vec<String>> {
    let values = if values.is_empty() {
        terminal
            .input("Email domains (comma-separated)", None)?
            .split(',')
            .map(str::to_owned)
            .collect()
    } else {
        values
    };
    Ok(values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect())
}

fn device_id_length(
    value: Option<u8>,
    interactive: bool,
    terminal: &mut dyn Terminal,
) -> Result<u8> {
    match value {
        Some(value) => Ok(value),
        None if interactive
            && terminal.confirm("Configure advanced Device ID length", false)? =>
        {
            Ok(if terminal.select("Device ID length", &["16".into(), "32".into()], 0)? == 0 {
                16
            } else {
                32
            })
        }
        None => Ok(16),
    }
}

fn trust(path: Option<&Path>) -> Result<TrustSpec> {
    let Some(path) = path else { return Ok(TrustSpec::System) };
    platform::reject_existing_link(path)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "PEM path is not safe"))?;
    let bytes = fs::read(path)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "PEM certificate cannot be read"))?;
    let sha256 = eas_mail_profile::certificate_fingerprint(&bytes).map_err(profile_error)?;
    let pem = String::from_utf8(bytes)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "PEM certificate is not UTF-8"))?;
    Ok(TrustSpec::ExclusivePem { pem, sha256 })
}

fn required(value: Option<String>, label: &str, terminal: &mut dyn Terminal) -> Result<String> {
    let value = value.map_or_else(|| terminal.input(label, None), Ok)?;
    if value.trim().is_empty() {
        Err(AppError::new(ErrorCode::ValidationFailed, "required value is empty"))
    } else {
        Ok(value)
    }
}

fn profile_id(host: &str) -> String {
    host.split('.').next().filter(|value| !value.is_empty()).unwrap_or("mail").to_owned()
}

fn optional(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn invalid_value() -> AppError {
    AppError::new(ErrorCode::ValidationFailed, "interactive profile value is invalid")
}

#[cfg(test)]
mod tests {
    use super::{device_id_length, domains, optional, profile_id, required};
    use crate::cli::terminal::testing::ScriptedTerminal;

    #[test]
    fn explicit_values_are_normalized_and_empty_required_values_fail() -> anyhow::Result<()> {
        let mut terminal = ScriptedTerminal::new(&[], &[]);
        assert_eq!(
            domains(vec![" Example.Invalid ".into(), "".into()], &mut terminal)?,
            ["example.invalid"]
        );
        assert_eq!(device_id_length(Some(32), false, &mut terminal)?, 32);
        assert_eq!(device_id_length(None, false, &mut terminal)?, 16);
        assert_eq!(optional(" value ".into()).as_deref(), Some("value"));
        assert!(optional("  ".into()).is_none());
        assert_eq!(required(Some("value".into()), "unused", &mut terminal)?, "value");
        assert!(required(Some("  ".into()), "unused", &mut terminal).is_err());
        assert_eq!(profile_id("mail.example.invalid"), "mail");
        Ok(())
    }
}
