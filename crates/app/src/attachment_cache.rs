use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::references::Clock;
use crate::sanitize::safe_filename;
use crate::{AppError, ErrorCode, Result};

const RETENTION_HOURS: i64 = 24;

pub(super) struct AttachmentCache {
    root: PathBuf,
    clock: Arc<dyn Clock>,
}

impl AttachmentCache {
    pub(super) fn new(root: PathBuf, clock: Arc<dyn Clock>) -> Result<Self> {
        private_directory(&root)?;
        let cache = Self { root, clock };
        cache.prune()?;
        Ok(cache)
    }

    pub(super) fn store(
        &self,
        account_id: &str,
        token: &str,
        display_name: &str,
        bytes: &[u8],
    ) -> Result<(PathBuf, DateTime<Utc>)> {
        self.prune()?;
        let directory = self.account_directory(account_id);
        private_directory(&directory)?;
        let path = directory.join(format!("{token}_{}", safe_filename(display_name)));
        private_file(&path, bytes)?;
        Ok((path, self.clock.now() + Duration::hours(RETENTION_HOURS)))
    }

    pub(super) fn purge_account(&self, account_id: &str) -> Result<()> {
        let path = self.account_directory(account_id);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
                fs::remove_file(path).map_err(storage_error)
            }
            Ok(_) => fs::remove_dir_all(path).map_err(storage_error),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn prune(&self) -> Result<()> {
        let entries = fs::read_dir(&self.root).map_err(storage_error)?;
        for entry in entries {
            let path = entry.map_err(storage_error)?.path();
            let metadata = fs::symlink_metadata(&path).map_err(storage_error)?;
            if metadata.file_type().is_symlink() || metadata.is_file() {
                fs::remove_file(path).map_err(storage_error)?;
            } else if metadata.is_dir() {
                self.prune_directory(&path)?;
            }
        }
        Ok(())
    }

    fn prune_directory(&self, directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory).map_err(storage_error)? {
            let path = entry.map_err(storage_error)?.path();
            let metadata = fs::symlink_metadata(&path).map_err(storage_error)?;
            let expired = metadata.modified().map(DateTime::<Utc>::from).map_or(true, |modified| {
                modified + Duration::hours(RETENTION_HOURS) <= self.clock.now()
            });
            if metadata.file_type().is_symlink() || !metadata.is_file() || expired {
                remove_entry(&path, &metadata)?;
            }
        }
        Ok(())
    }

    fn account_directory(&self, account_id: &str) -> PathBuf {
        self.root.join(safe_filename(account_id))
    }
}

fn remove_entry(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(storage_error)
    } else {
        fs::remove_dir_all(path).map_err(storage_error)
    }
}

#[cfg(unix)]
fn private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::create_dir_all(path).map_err(storage_error)?;
    let metadata = fs::symlink_metadata(path).map_err(storage_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(storage_error(std::io::Error::other("cache path is not a directory")));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(storage_error)
}

#[cfg(unix)]
fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(storage_error)?;
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)
}

#[cfg(not(unix))]
fn private_directory(_: &Path) -> Result<()> {
    Err(AppError::new(ErrorCode::ConfigInvalid, "only macOS is supported"))
}

#[cfg(not(unix))]
fn private_file(_: &Path, _: &[u8]) -> Result<()> {
    Err(AppError::new(ErrorCode::ConfigInvalid, "only macOS is supported"))
}

fn storage_error(_: std::io::Error) -> AppError {
    AppError::new(ErrorCode::StorageError, "managed attachment cache is unavailable")
}

#[cfg(test)]
mod tests;
