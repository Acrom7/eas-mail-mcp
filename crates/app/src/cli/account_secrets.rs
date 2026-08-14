use std::sync::Arc;

use crate::{AccountSecret, AppError, ErrorCode, KeychainStore, Paths, Result, SecretStore};

pub(super) fn open(paths: &Paths) -> Arc<dyn SecretStore> {
    Arc::new(KeychainStore::new(paths.journal.clone()))
}

pub(super) fn replace(
    store: &Arc<dyn SecretStore>,
    account_id: &str,
    candidate: AccountSecret,
) -> Result<Option<AccountSecret>> {
    replace_optional(store, account_id, Some(candidate))
}

pub(super) fn replace_optional(
    store: &Arc<dyn SecretStore>,
    account_id: &str,
    candidate: Option<AccountSecret>,
) -> Result<Option<AccountSecret>> {
    let mut previous = None;
    store.update(&mut |bundle| {
        previous = match &candidate {
            Some(secret) => bundle.accounts.insert(account_id.to_owned(), secret.clone()),
            None => bundle.accounts.remove(account_id),
        };
        Ok(())
    })?;
    Ok(previous)
}

pub(super) fn replace_password(
    store: &Arc<dyn SecretStore>,
    account_id: &str,
    password: &str,
) -> Result<(AccountSecret, AccountSecret)> {
    let mut result = None;
    store.update(&mut |bundle| {
        let secret = bundle.accounts.get_mut(account_id).ok_or_else(|| {
            AppError::new(ErrorCode::AuthRequired, "account credentials are missing")
                .account(account_id)
        })?;
        let original = secret.clone();
        secret.password = password.to_owned();
        secret.policy_key = 0;
        secret.policy = None;
        result = Some((original, secret.clone()));
        Ok(())
    })?;
    result.ok_or_else(|| AppError::new(ErrorCode::StorageError, "secret update did not complete"))
}

pub(super) fn restore(
    store: &Arc<dyn SecretStore>,
    account_id: &str,
    expected: Option<&AccountSecret>,
    previous: Option<&AccountSecret>,
) -> Result<()> {
    let expected = expected.cloned();
    let previous = previous.cloned();
    store.update(&mut |bundle| {
        if bundle.accounts.get(account_id) != expected.as_ref() {
            return Ok(());
        }
        match &previous {
            Some(secret) => {
                bundle.accounts.insert(account_id.to_owned(), secret.clone());
            }
            None => {
                bundle.accounts.remove(account_id);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemorySecretStore;

    #[test]
    fn updates_and_conditional_rollbacks_preserve_newer_values() -> anyhow::Result<()> {
        let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        let first = secret("fixture-value");
        assert!(replace(&store, "work", first.clone())?.is_none());

        let (previous, candidate) = replace_password(&store, "work", "fixture")?;
        assert!(previous == first);
        restore(&store, "work", Some(&candidate), Some(&previous))?;
        assert!(store.load()?.accounts.get("work") == Some(&first));

        let newer = secret("redacted");
        let _ = replace(&store, "work", newer.clone())?;
        restore(&store, "work", Some(&candidate), Some(&previous))?;
        assert!(store.load()?.accounts.get("work") == Some(&newer));
        Ok(())
    }

    fn secret(password: &str) -> AccountSecret {
        AccountSecret {
            password: password.into(),
            device_id: "0011223344556677".into(),
            policy_key: 0,
            policy: None,
        }
    }
}
