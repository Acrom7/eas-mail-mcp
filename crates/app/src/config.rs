use eas_mail_protocol::{ProfileKey, ProfileRegistry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::{AppError, ErrorCode, Result};

/// Non-secret account configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    /// Managed endpoint profile.
    pub profile: ProfileKey,
    /// Mailbox email address.
    pub email: String,
    /// Exchange/AD username.
    pub username: String,
    /// Whether tools should include this account.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Whether externally visible mail mutations are enabled.
    #[serde(default)]
    pub write_enabled: bool,
}

impl AccountConfig {
    /// Validates identity against the fixed profile.
    pub fn validate(&self) -> Result<()> {
        let profile = ProfileRegistry::compiled().require(&self.profile).map_err(AppError::from)?;
        profile.validate_identity(&self.email, &self.username).map_err(AppError::from)
    }
}

/// Versioned non-secret application configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Configuration schema version.
    pub version: u8,
    /// Accounts keyed by stable local identifier.
    #[serde(default)]
    pub accounts: BTreeMap<String, AccountConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { version: 1, accounts: BTreeMap::new() }
    }
}

impl AppConfig {
    /// Validates schema version, account IDs, and managed identities.
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(AppError::new(ErrorCode::ConfigInvalid, "unsupported config version"));
        }
        for (account_id, account) in &self.accounts {
            if !valid_account_id(account_id) {
                return Err(AppError::new(ErrorCode::ConfigInvalid, "invalid account identifier"));
            }
            account.validate()?;
        }
        Ok(())
    }
}

/// User-local filesystem locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Application Support directory.
    pub support: PathBuf,
    /// Temporary attachment directory.
    pub attachments: PathBuf,
    /// TOML configuration path.
    pub config: PathBuf,
    /// Minimal operation journal path.
    pub journal: PathBuf,
}

impl Paths {
    /// Returns standard macOS per-user paths.
    pub fn standard() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| {
            AppError::new(ErrorCode::ConfigInvalid, "cannot determine the home directory")
        })?;
        let support = home.join("Library/Application Support/EAS Mail MCP");
        let cache = home.join("Library/Caches/EAS Mail MCP");
        Ok(Self {
            config: support.join("config.toml"),
            journal: support.join("operations.sqlite"),
            attachments: cache.join("attachments"),
            support,
        })
    }

    /// Creates private runtime directories.
    pub fn ensure(&self) -> Result<()> {
        for directory in [&self.support, &self.attachments] {
            fs::create_dir_all(directory).map_err(storage_error)?;
            set_private_directory(directory)?;
        }
        Ok(())
    }
}

/// Loads configuration, returning an empty version-one document when absent.
pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let input = fs::read_to_string(path).map_err(storage_error)?;
    let config: AppConfig = toml::from_str(&input)
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "configuration TOML is invalid"))?;
    config.validate()?;
    Ok(config)
}

/// Atomically saves non-secret configuration with mode 0600.
pub fn save_config(path: &Path, config: &AppConfig) -> Result<()> {
    config.validate()?;
    let parent = path.parent().ok_or_else(|| {
        AppError::new(ErrorCode::ConfigInvalid, "configuration path has no parent")
    })?;
    fs::create_dir_all(parent).map_err(storage_error)?;
    set_private_directory(parent)?;
    let temporary = parent.join(format!(".config-{}.tmp", std::process::id()));
    let document = toml::to_string_pretty(config)
        .map_err(|_| AppError::new(ErrorCode::StorageError, "cannot serialize configuration"))?;
    let mut file = private_file(&temporary)?;
    file.write_all(document.as_bytes()).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)?;
    fs::rename(&temporary, path).map_err(storage_error)
}

fn valid_account_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

const fn enabled_by_default() -> bool {
    true
}

fn storage_error(_: std::io::Error) -> AppError {
    AppError::new(ErrorCode::StorageError, "local application storage is unavailable")
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(storage_error)
}

#[cfg(not(unix))]
fn set_private_directory(_: &Path) -> Result<()> {
    Err(AppError::new(ErrorCode::ConfigInvalid, "only macOS is supported"))
}

#[cfg(unix)]
fn private_file(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(storage_error)
}

#[cfg(test)]
mod tests;
