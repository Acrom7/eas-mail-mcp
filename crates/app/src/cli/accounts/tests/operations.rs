#![expect(
    clippy::indexing_slicing,
    reason = "fixed account fixtures use direct indexing for readable assertions"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use eas_mail_protocol::{ProfileKey, ProfileRegistry};

use super::super::*;
use super::paths;
use crate::cli::terminal::testing::ScriptedTerminal;

#[derive(Clone, Copy)]
enum FakeOutcome {
    Success { writes_supported: bool },
    Error(ErrorCode),
    RemovedByServer,
}

struct FakeVerifier {
    outcome: FakeOutcome,
    calls: AtomicUsize,
}

impl FakeVerifier {
    fn new(outcome: FakeOutcome) -> Self {
        Self { outcome, calls: AtomicUsize::new(0) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait(?Send)]
impl AccountVerifier for FakeVerifier {
    async fn verify(
        &self,
        account_id: &str,
        _: &AccountConfig,
        secrets: Arc<dyn SecretStore>,
        _: &ProfileRegistry,
        _: Option<&mut dyn Terminal>,
    ) -> Result<(usize, bool)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.outcome {
            FakeOutcome::Success { writes_supported } => {
                secrets.update(&mut |bundle| {
                    let account = bundle.accounts.get_mut(account_id).ok_or_else(|| {
                        AppError::new(ErrorCode::AuthRequired, "temporary credentials are missing")
                    })?;
                    account.policy_key = 7;
                    Ok(())
                })?;
                Ok((4, writes_supported))
            }
            FakeOutcome::Error(code) => {
                Err(AppError::new(code, "scripted verification failure").account(account_id))
            }
            FakeOutcome::RemovedByServer => {
                secrets.update(&mut |bundle| {
                    bundle.accounts.remove(account_id);
                    Ok(())
                })?;
                Ok((0, false))
            }
        }
    }
}

#[tokio::test]
async fn verified_add_persists_config_and_updated_temporary_secret() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let profiles = crate::profiles::example_registry()?;
    let store = memory_store();
    let verifier = FakeVerifier::new(FakeOutcome::Success { writes_supported: true });

    let result =
        add_with_dependencies(&paths, request("work", false)?, &profiles, None, &store, &verifier)
            .await?;

    assert_eq!(result["folders_verified"], 4);
    assert_eq!(verifier.calls(), 1);
    assert_eq!(load_config(&paths.config)?.accounts["work"].username, "example_user");
    let secret = store
        .load()?
        .accounts
        .get("work")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("verified secret was not persisted"))?;
    assert_eq!(secret.password, "fixture-value");
    assert_eq!(secret.policy_key, 7);

    let duplicate =
        add_with_dependencies(&paths, request("work", false)?, &profiles, None, &store, &verifier)
            .await
            .map_err(|error| error.envelope.code);
    assert_eq!(duplicate, Err(ErrorCode::ValidationFailed));
    assert_eq!(verifier.calls(), 1);
    Ok(())
}

#[tokio::test]
async fn failed_or_unsupported_add_leaves_no_credentials_or_config() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let profiles = crate::profiles::example_registry()?;
    let store = memory_store();
    let denied = FakeVerifier::new(FakeOutcome::Error(ErrorCode::AccessDenied));

    let error =
        add_with_dependencies(&paths, request("denied", false)?, &profiles, None, &store, &denied)
            .await
            .map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::AccessDenied));
    assert!(load_config(&paths.config)?.accounts.is_empty());
    assert!(store.load()?.accounts.is_empty());

    let unsupported = FakeVerifier::new(FakeOutcome::Success { writes_supported: false });
    let error = add_with_dependencies(
        &paths,
        request("unsupported", true)?,
        &profiles,
        None,
        &store,
        &unsupported,
    )
    .await
    .map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::ValidationFailed));
    assert!(load_config(&paths.config)?.accounts.is_empty());
    assert!(store.load()?.accounts.is_empty());

    let wiped = FakeVerifier::new(FakeOutcome::RemovedByServer);
    let error =
        add_with_dependencies(&paths, request("wiped", false)?, &profiles, None, &store, &wiped)
            .await
            .map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::RemoteWipe));
    assert!(store.load()?.accounts.is_empty());
    Ok(())
}

#[tokio::test]
async fn add_rolls_back_secret_when_config_cannot_be_saved() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let blocked_parent = directory.path().join("not-a-directory");
    std::fs::write(&blocked_parent, "fixture")?;
    let mut paths = paths(directory.path());
    paths.config = blocked_parent.join("config.toml");
    let profiles = crate::profiles::example_registry()?;
    let store = memory_store();
    let verifier = FakeVerifier::new(FakeOutcome::Success { writes_supported: true });

    let error =
        add_with_dependencies(&paths, request("work", false)?, &profiles, None, &store, &verifier)
            .await
            .map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::StorageError));
    assert!(store.load()?.accounts.is_empty());
    Ok(())
}

#[tokio::test]
async fn password_update_and_repair_commit_only_verified_values() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let profiles = crate::profiles::example_registry()?;
    let store = configured_store(&paths)?;
    let accepted = FakeVerifier::new(FakeOutcome::Success { writes_supported: true });
    let mut terminal = ScriptedTerminal::new(&[], &["updated-value"]);

    update_password_with_dependencies(
        &paths,
        "work",
        false,
        &profiles,
        &mut terminal,
        &store,
        &accepted,
    )
    .await?;
    assert_eq!(store.load()?.accounts["work"].password, "updated-value");

    let rejected = FakeVerifier::new(FakeOutcome::Error(ErrorCode::AuthRequired));
    let mut terminal = ScriptedTerminal::new(&[], &["rejected-value"]);
    let error = update_password_with_dependencies(
        &paths,
        "work",
        false,
        &profiles,
        &mut terminal,
        &store,
        &rejected,
    )
    .await
    .map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::AuthRequired));
    assert_eq!(store.load()?.accounts["work"].password, "updated-value");

    set_writes(&paths, "work", true)?;
    let mut terminal = ScriptedTerminal::new(
        &["", "repaired@example.invalid", "repaired_user"],
        &["repaired-value"],
    );
    let no_writes = FakeVerifier::new(FakeOutcome::Success { writes_supported: false });
    let result =
        repair_with_dependencies(&paths, "work", &profiles, &mut terminal, &store, &no_writes)
            .await?;
    assert_eq!(result["write_enabled"], false);
    let account = &load_config(&paths.config)?.accounts["work"];
    assert_eq!(account.email, "repaired@example.invalid");
    assert_eq!(account.username, "repaired_user");
    let secret = &store.load()?.accounts["work"];
    assert_eq!(secret.password, "repaired-value");
    assert_eq!(secret.device_id, "0011223344556677");
    Ok(())
}

#[tokio::test]
async fn write_capabilities_and_remove_are_enforced_with_shared_state() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let paths = paths(directory.path());
    let profiles = crate::profiles::example_registry()?;
    let store = configured_store(&paths)?;
    let unsupported = FakeVerifier::new(FakeOutcome::Success { writes_supported: false });

    let error =
        set_writes_checked_with_dependencies(&paths, "work", true, &profiles, &store, &unsupported)
            .await
            .map_err(|error| error.envelope.code);
    assert_eq!(error, Err(ErrorCode::ValidationFailed));
    assert!(!load_config(&paths.config)?.accounts["work"].write_enabled);

    let supported = FakeVerifier::new(FakeOutcome::Success { writes_supported: true });
    set_writes_checked_with_dependencies(&paths, "work", true, &profiles, &store, &supported)
        .await?;
    assert!(load_config(&paths.config)?.accounts["work"].write_enabled);
    assert_eq!(store.load()?.accounts["work"].policy_key, 7);

    set_writes_checked_with_dependencies(&paths, "work", false, &profiles, &store, &unsupported)
        .await?;
    assert_eq!(unsupported.calls(), 1);
    assert!(!load_config(&paths.config)?.accounts["work"].write_enabled);

    let removed = remove_with_store(&paths, "work", &store)?;
    assert_eq!(removed["removed"], true);
    assert!(load_config(&paths.config)?.accounts.is_empty());
    assert!(store.load()?.accounts.is_empty());
    assert_eq!(
        remove_with_store(&paths, "work", &store).map_err(|error| error.envelope.code),
        Err(ErrorCode::NotFound)
    );
    Ok(())
}

fn request(account_id: &str, write_enabled: bool) -> anyhow::Result<AddRequest> {
    Ok(AddRequest {
        account_id: account_id.into(),
        profile: ProfileKey::new("example")?,
        email: "user@example.invalid".into(),
        username: "example_user".into(),
        password: zeroize::Zeroizing::new("fixture-value".into()),
        write_enabled,
    })
}

fn memory_store() -> Arc<dyn SecretStore> {
    Arc::new(MemorySecretStore::default())
}

fn configured_store(paths: &Paths) -> anyhow::Result<Arc<dyn SecretStore>> {
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
    let mut bundle = SecretBundle::new();
    bundle.accounts.insert(
        "work".into(),
        AccountSecret {
            password: "fixture-value".into(),
            device_id: "0011223344556677".into(),
            policy_key: 0,
            policy: None,
        },
    );
    Ok(Arc::new(MemorySecretStore::with_bundle(bundle)))
}
