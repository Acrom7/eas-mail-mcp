use rcgen::generate_simple_self_signed;

use super::*;
use crate::cli::terminal::testing::ScriptedTerminal;
use crate::cli::{ProfileCommand, ProfileIdentityMode};

#[test]
fn setup_imports_then_reuses_the_same_runtime_registry() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    let source = directory.path().join("setup.toml");
    fs::write(&source, include_str!("../../../../../../profile.example.toml"))?;

    let mut terminal = ScriptedTerminal::new(&[], &[]);
    let (imported, first) = ensure_for_setup_with_terminal(&paths, Some(&source), &mut terminal)?;
    assert_eq!(imported.profiles().len(), 1);
    assert_eq!(first.get("action").and_then(serde_json::Value::as_str), Some("imported"));
    let (reused, second) = ensure_for_setup_with_terminal(&paths, None, &mut terminal)?;
    assert_eq!(reused.bundle_hash(), imported.bundle_hash());
    assert_eq!(second.get("action").and_then(serde_json::Value::as_str), Some("reused"));

    let mut additional = stored_manifest(&paths)?;
    let profile = additional
        .profiles
        .first_mut()
        .ok_or_else(|| anyhow::anyhow!("profile fixture is empty"))?;
    profile.id = "second".into();
    let additional_path = write_manifest(directory.path(), "additional.toml", &additional)?;
    let (merged, third) =
        ensure_for_setup_with_terminal(&paths, Some(&additional_path), &mut terminal)?;
    assert_eq!(merged.profiles().len(), 2);
    assert_eq!(third.get("action").and_then(serde_json::Value::as_str), Some("imported"));
    Ok(())
}

#[test]
fn command_dispatch_covers_empty_store_and_selected_export() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    let empty = run(&paths, ProfileCommand::List)?;
    assert_eq!(empty.get("profiles").and_then(serde_json::Value::as_array).map(Vec::len), Some(0));
    assert!(
        run(
            &paths,
            ProfileCommand::Export(ProfileExportArgs {
                file: directory.path().join("none.toml"),
                id: None,
            }),
        )
        .is_err()
    );

    let source = directory.path().join("incoming.toml");
    fs::write(&source, include_str!("../../../../../../profile.example.toml"))?;
    run(
        &paths,
        ProfileCommand::Import(ProfileImportArgs {
            file: source.clone(),
            replace: false,
            yes: false,
        }),
    )?;
    run(&paths, ProfileCommand::Validate(ProfileValidateArgs { file: Some(source) }))?;
    let exported = directory.path().join("selected.toml");
    run(
        &paths,
        ProfileCommand::Export(ProfileExportArgs {
            file: exported.clone(),
            id: Some(ProfileKey::new("example")?),
        }),
    )?;
    assert_eq!(eas_mail_profile::load(&exported)?.manifest.profiles.len(), 1);
    assert!(
        selected_export(&stored_manifest(&paths)?, Some(&ProfileKey::new("missing")?)).is_err()
    );
    Ok(())
}

#[test]
fn interactive_import_retries_a_missing_profile_path() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    let missing = directory.path().join("missing.toml");
    let source = directory.path().join("profile.toml");
    fs::write(&source, include_str!("../../../../../../profile.example.toml"))?;
    let missing_text = missing.to_string_lossy().into_owned();
    let source_text = source.to_string_lossy().into_owned();
    let mut terminal = ScriptedTerminal::new(&[&missing_text, &source_text], &[]);

    import_from_prompt(&paths, &mut terminal)?;

    assert!(terminal.transcript.iter().any(|line| line.contains("profile file does not exist")));
    assert_eq!(stored_manifest(&paths)?.profiles.len(), 1);
    Ok(())
}

#[test]
fn multiple_profiles_and_exclusive_pem_are_managed_without_leaking_pem() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    import_fixture(&paths, directory.path())?;
    let certificate = generate_simple_self_signed(vec!["mail.example.invalid".into()])?.cert.pem();
    let certificate_path = directory.path().join("root.pem");
    fs::write(&certificate_path, &certificate)?;

    add(
        &paths,
        ProfileAddArgs {
            id: Some("exclusive".into()),
            display_name: Some("Exclusive trust".into()),
            host: Some("MAIL.EXAMPLE.INVALID".into()),
            email_domains: vec![" example.invalid ".into(), "".into()],
            identity_mode: Some(ProfileIdentityMode::RealmUsername),
            username_realm: Some("EXAMPLE".into()),
            username_hint: Some("short login".into()),
            device_id_length: None,
            pem: Some(certificate_path),
        },
    )?;
    let visible = list(&paths)?;
    assert_eq!(
        visible.get("profiles").and_then(serde_json::Value::as_array).map(Vec::len),
        Some(2)
    );
    assert!(!visible.to_string().contains("BEGIN CERTIFICATE"));
    run(
        &paths,
        ProfileCommand::Remove(ProfileRemoveArgs { id: ProfileKey::new("example")?, yes: true }),
    )?;
    assert_eq!(first_profile(&paths)?.id, "exclusive");
    assert!(
        remove(&paths, ProfileRemoveArgs { id: ProfileKey::new("missing")?, yes: true },).is_err()
    );
    Ok(())
}

#[test]
fn invalid_manual_inputs_and_duplicate_ids_fail_closed() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    paths.ensure()?;
    let arguments = ProfileAddArgs {
        id: Some("manual".into()),
        display_name: Some("Manual".into()),
        host: Some("mail.example.invalid".into()),
        email_domains: vec!["example.invalid".into()],
        identity_mode: Some(ProfileIdentityMode::Username),
        username_realm: None,
        username_hint: None,
        device_id_length: Some(16),
        pem: None,
    };
    run(&paths, ProfileCommand::Add(arguments))?;
    assert!(
        add(
            &paths,
            ProfileAddArgs {
                id: Some("manual".into()),
                display_name: Some("Duplicate".into()),
                host: Some("mail.example.invalid".into()),
                email_domains: vec!["example.invalid".into()],
                identity_mode: Some(ProfileIdentityMode::Username),
                username_realm: None,
                username_hint: None,
                device_id_length: Some(16),
                pem: None,
            },
        )
        .is_err()
    );
    assert!(
        add(
            &paths,
            ProfileAddArgs {
                id: Some(" ".into()),
                display_name: Some("Invalid".into()),
                host: Some("mail.example.invalid".into()),
                email_domains: vec!["example.invalid".into()],
                identity_mode: Some(ProfileIdentityMode::Username),
                username_realm: None,
                username_hint: None,
                device_id_length: None,
                pem: None,
            },
        )
        .is_err()
    );
    Ok(())
}
