//! Validation for portable runtime EAS endpoint profile bundles.

#![deny(missing_docs)]

mod legacy;
mod validation;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

pub use validation::{certificate_fingerprint, normalize_fingerprint, valid_profile_key};

/// Current portable profile schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Versioned collection of endpoint profiles stored in one portable TOML file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileBundle {
    /// Canonical profile schema version.
    pub schema_version: u32,
    /// Operator-defined version shown in diagnostics.
    pub bundle_version: String,
    /// Endpoints available to the local application.
    pub profiles: Vec<ProfileSpec>,
}

/// One fixed Exchange ActiveSync endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSpec {
    /// Stable identifier persisted in account configuration.
    pub id: String,
    /// Human-readable profile name.
    pub display_name: String,
    /// DNS host without a scheme, port, path, or query.
    pub host: String,
    /// Allowed mailbox email domains.
    pub email_domains: Vec<String>,
    /// How an authentication username is collected and normalized.
    pub identity: IdentitySpec,
    /// Exact ASCII EAS Device ID length.
    pub device_id_length: u8,
    /// TLS trust configuration.
    pub trust: TrustSpec,
}

/// Authentication username rules supplied by a local endpoint profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdentitySpec {
    /// Username input mode.
    pub mode: IdentityMode,
    /// Required realm for `realm_username` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    /// Optional operator-provided hint shown by the setup wizard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username_hint: Option<String>,
}

/// Supported ways to construct the HTTP Basic authentication username.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityMode {
    /// Use the mailbox email address as the authentication username.
    Email,
    /// Accept one username verbatim after trimming surrounding whitespace.
    Username,
    /// Prefix a short username with the profile's fixed realm.
    RealmUsername,
}

/// Supported TLS trust modes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustSpec {
    /// Use the operating system trust store.
    System,
    /// Trust only the specified inline PEM certificate.
    ExclusivePem {
        /// Exactly one PEM-encoded certificate, stored inline.
        pem: String,
        /// SHA-256 fingerprint of the certificate DER bytes.
        sha256: String,
    },
}

/// A validated runtime profile bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBundle {
    /// Validated source manifest.
    pub manifest: ProfileBundle,
    /// Hash of the exact parsed TOML bytes.
    pub hash: String,
    /// Resolved source path when loaded from the filesystem.
    pub source: Option<PathBuf>,
    /// Schema version found in the source bytes before any in-memory migration.
    pub source_schema_version: u32,
}

/// A redacted profile validation error.
#[derive(Debug, Error)]
pub enum ProfileError {
    /// The bundle could not be read.
    #[error("cannot read profile bundle")]
    Read,
    /// The bundle is not valid TOML.
    #[error("profile bundle TOML is invalid")]
    Toml,
    /// One or more profile fields violate the schema constraints.
    #[error("invalid profile bundle: {0}")]
    Invalid(String),
    /// Inline trust material could not be verified.
    #[error("invalid profile trust material: {0}")]
    Trust(String),
}

/// Loads and validates a profile bundle without following a symlink or reparse point.
pub fn load(path: &Path) -> Result<VerifiedBundle, ProfileError> {
    validation::reject_link(path)?;
    let source = path.canonicalize().map_err(|_| ProfileError::Read)?;
    let input = fs::read(&source).map_err(|_| ProfileError::Read)?;
    let mut bundle = parse_bytes(&input)?;
    bundle.source = Some(source);
    Ok(bundle)
}

/// Parses and validates an in-memory profile bundle.
pub fn parse(input: &str) -> Result<VerifiedBundle, ProfileError> {
    parse_bytes(input.as_bytes())
}

/// Serializes a validated bundle into stable, human-readable TOML.
pub fn serialize(bundle: &ProfileBundle) -> Result<String, ProfileError> {
    validation::validate_manifest(bundle)?;
    toml::to_string_pretty(bundle).map_err(|_| ProfileError::Toml)
}

fn parse_bytes(input: &[u8]) -> Result<VerifiedBundle, ProfileError> {
    let source_schema_version = legacy::schema_version(input)?;
    let manifest = match source_schema_version {
        1 => legacy::parse_v1(input)?,
        CURRENT_SCHEMA_VERSION => {
            toml::from_slice::<ProfileBundle>(input).map_err(|_| ProfileError::Toml)?
        }
        _ => return Err(ProfileError::Invalid("unsupported schema_version".into())),
    };
    validation::validate_manifest(&manifest)?;
    let hash = Sha256::digest(input).iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(VerifiedBundle { manifest, hash, source: None, source_schema_version })
}

#[cfg(test)]
mod tests;
