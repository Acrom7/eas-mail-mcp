#![expect(
    clippy::indexing_slicing,
    reason = "fixed test fixtures use direct indexing for readable assertions"
)]

use eas_mail_protocol::ProfileKey;

use super::super::terminal::testing::ScriptedTerminal;
use super::super::{AddAccountArgs, SetupArgs};
use super::input::report_stage;
use super::*;
use crate::backend::VerificationStage;

mod operations;

#[test]
fn explicit_setup_arguments_do_not_prompt() -> anyhow::Result<()> {
    let profiles = crate::profiles::example_registry()?;
    let profile = ProfileKey::new("example")?;
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let mut terminal = ScriptedTerminal::new(&[], &["fixture-value"]);
    let request = collect_request(
        &paths,
        SetupArgs {
            profile_file: None,
            account_id: Some("work".into()),
            profile: Some(profile.clone()),
            email: Some("user@example.invalid".into()),
            username: Some("example_user".into()),
            password_stdin: false,
            enable_writes: false,
            skip_clients: true,
        },
        &profiles,
        &mut terminal,
    )?;
    assert_eq!(request.account_id, "work");
    assert_eq!(request.profile, profile);
    assert!(!request.write_enabled);
    let mut terminal = ScriptedTerminal::new(&[], &["fixture-value"]);
    assert!(
        collect_request(
            &paths,
            SetupArgs {
                profile_file: None,
                account_id: Some(" ".into()),
                profile: Some(ProfileKey::new("example")?),
                email: Some("user@example.invalid".into()),
                username: Some("example_user".into()),
                password_stdin: false,
                enable_writes: false,
                skip_clients: true,
            },
            &profiles,
            &mut terminal,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn add_arguments_map_profiles_and_flags() -> anyhow::Result<()> {
    let profile = ProfileKey::new("example")?;
    let request = SetupArgs::from(AddAccountArgs {
        account_id: Some("sample".into()),
        profile: Some(profile.clone()),
        email: Some("user@example.invalid".into()),
        username: Some("example_user".into()),
        password_stdin: false,
        enable_writes: true,
    });
    assert_eq!(request.profile, Some(profile));
    assert!(request.enable_writes);
    Ok(())
}

#[test]
fn realm_profile_collects_a_short_login_and_generates_account_ids() -> anyhow::Result<()> {
    let profiles = ProfileRegistry::from_toml(
        "schema_version = 2\nbundle_version = \"test\"\n\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Example\"\nhost = \"mail.example.invalid\"\nemail_domains = [\"example.invalid\"]\ndevice_id_length = 16\n\n[profiles.identity]\nmode = \"realm_username\"\nrealm = \"EXAMPLE\"\nusername_hint = \"Short login\"\n\n[profiles.trust]\nmode = \"system\"\n",
    )?;
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let mut config = crate::AppConfig::default();
    config.accounts.insert(
        "example".into(),
        AccountConfig {
            profile: ProfileKey::new("example")?,
            email: "first@example.invalid".into(),
            username: "EXAMPLE\\first".into(),
            enabled: true,
            write_enabled: false,
        },
    );
    save_config(&paths.config, &config)?;
    let mut terminal =
        ScriptedTerminal::new(&["second@example.invalid", "second"], &["fixture-value"]);
    let request = collect_request(&paths, blank_setup(), &profiles, &mut terminal)?;
    assert_eq!(request.account_id, "example-2");
    assert_eq!(request.username, "EXAMPLE\\second");
    assert!(terminal.transcript.iter().any(|line| line.contains("Username hint: Short login")));
    Ok(())
}

#[test]
fn passwords_and_required_values_fail_closed() -> anyhow::Result<()> {
    let mut terminal = ScriptedTerminal::new(&[], &[]);
    assert_eq!(required(Some(" value ".into()), "ignored", &mut terminal)?, " value ");
    assert!(required(Some(String::new()), "ignored", &mut terminal).is_err());
    for invalid in ["", "line\nfeed", "carriage\rreturn", "nul\0byte"] {
        assert!(validate_password(invalid).is_err());
    }
    validate_password("fixture-value")?;
    Ok(())
}

#[test]
fn verification_stages_are_human_readable_and_ordered() -> anyhow::Result<()> {
    let mut terminal = ScriptedTerminal::new(&[], &[]);
    let mut terminal_ref: Option<&mut dyn Terminal> = Some(&mut terminal);
    for stage in [
        VerificationStage::Profile,
        VerificationStage::Transport,
        VerificationStage::Capabilities,
        VerificationStage::Policy,
        VerificationStage::FolderSync,
    ] {
        report_stage(&mut terminal_ref, stage)?;
    }
    let messages = terminal
        .transcript
        .iter()
        .filter(|line| line.starts_with("message:["))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 5);
    assert!(messages[0].contains("[1/5]"));
    assert!(messages[4].contains("[5/5]"));
    Ok(())
}

#[test]
fn list_and_write_toggle_persist_only_non_secret_config() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let mut config = crate::AppConfig::default();
    config.accounts.insert(
        "work".into(),
        AccountConfig {
            profile: ProfileKey::new("example")?,
            email: "user@example.invalid".into(),
            username: "example_user".into(),
            enabled: true,
            write_enabled: false,
        },
    );
    save_config(&paths.config, &config)?;
    let listed = list(&paths)?;
    assert_eq!(listed["accounts"][0]["account_id"], "work");
    assert!(listed.to_string().find("example_user").is_none());

    let enabled = set_writes(&paths, "work", true)?;
    assert_eq!(enabled["write_enabled"], true);
    assert!(load_config(&paths.config)?.accounts["work"].write_enabled);
    assert_eq!(
        set_writes(&paths, "missing", true).map_err(|error| error.envelope.code),
        Err(ErrorCode::NotFound)
    );
    Ok(())
}

fn paths(root: &std::path::Path) -> Paths {
    Paths {
        support: root.join("support"),
        attachments: root.join("attachments"),
        config: root.join("support/config.toml"),
        profiles: root.join("support/profiles.toml"),
        journal: root.join("support/operations.sqlite"),
    }
}

fn blank_setup() -> SetupArgs {
    SetupArgs {
        profile_file: None,
        account_id: None,
        profile: None,
        email: None,
        username: None,
        password_stdin: false,
        enable_writes: false,
        skip_clients: true,
    }
}
