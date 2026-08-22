use std::time::Duration;

use super::*;

#[tokio::test]
async fn independent_lock_instances_serialize_one_account() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let first = WriteLocks::new(directory.path().join("locks"))?;
    let second = WriteLocks::new(directory.path().join("locks"))?;
    let guard = first.acquire("work").await?;
    let waiting = tokio::spawn(async move { second.acquire("work").await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished());
    drop(guard);
    let second_guard = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .map_err(|_| anyhow::anyhow!("second lock did not unblock"))??;
    drop(second_guard?);
    Ok(())
}

#[tokio::test]
async fn different_accounts_do_not_block_each_other() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let locks = WriteLocks::new(directory.path().join("locks"))?;
    let first = locks.acquire("first").await?;
    let second = tokio::time::timeout(Duration::from_secs(1), locks.acquire("second")).await??;
    drop((first, second));
    Ok(())
}
