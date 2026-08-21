use std::sync::Arc;

use crate::{AccountSecret, KeychainStore, Paths, Result, SecretStore};

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

        let mut candidate = first.clone();
        candidate.password = "fixture".into();
        let previous = replace(&store, "work", candidate.clone())?
            .ok_or_else(|| anyhow::anyhow!("previous secret is missing"))?;
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
