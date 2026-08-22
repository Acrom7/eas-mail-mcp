use std::fs::File;
use std::path::PathBuf;

use crate::config::valid_account_id;
use crate::{AppError, ErrorCode, Result, platform};

pub(crate) struct WriteLocks {
    directory: PathBuf,
}

impl WriteLocks {
    pub(crate) fn new(directory: PathBuf) -> Result<Self> {
        platform::ensure_private_directory(&directory).map_err(|_| lock_error())?;
        Ok(Self { directory })
    }

    pub(crate) async fn acquire(&self, account_id: &str) -> Result<WriteGuard> {
        if !valid_account_id(account_id) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "invalid account identifier for write lock",
            ));
        }
        let path = self.directory.join(format!("{account_id}.lock"));
        tokio::task::spawn_blocking(move || acquire_file(path)).await.map_err(|_| lock_error())?
    }
}

pub(crate) struct WriteGuard {
    _file: File,
}

fn acquire_file(path: PathBuf) -> Result<WriteGuard> {
    let file = platform::open_private_append(&path).map_err(|_| lock_error())?;
    file.lock().map_err(|_| lock_error())?;
    Ok(WriteGuard { _file: file })
}

fn lock_error() -> AppError {
    AppError::new(ErrorCode::StorageError, "per-account write lock is unavailable")
}

#[cfg(test)]
mod tests;
