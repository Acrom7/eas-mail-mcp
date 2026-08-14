//! Validation for build-time EAS endpoint profile bundles.

#![deny(missing_docs)]

mod validation;

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

pub use validation::{normalize_fingerprint, valid_profile_key};

/// Versioned collection of endpoint profiles embedded at build time.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileBundle {
    /// Profile schema version. Version one is currently supported.
    pub schema_version: u32,
    /// Operator-defined version shown in diagnostics.
    pub bundle_version: String,
    /// Prevents an example profile from being used for release bundles.
    #[serde(default)]
    pub development_only: bool,
    /// Fixed endpoints available to the compiled application.
    pub profiles: Vec<ProfileSpec>,
}

/// One fixed Exchange ActiveSync endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    /// Required AD username realm, if the endpoint uses one.
    pub username_realm: Option<String>,
    /// Exact ASCII EAS Device ID length.
    pub device_id_length: u8,
    /// TLS trust configuration.
    pub trust: TrustSpec,
}

/// Supported TLS trust modes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustSpec {
    /// Use the operating system trust store.
    System,
    /// Trust only the specified embedded PEM certificate.
    ExclusivePem {
        /// PEM path relative to the profile bundle.
        pem: PathBuf,
        /// SHA-256 fingerprint of the certificate DER bytes.
        sha256: String,
    },
}

/// A validated profile and its optional trust anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedProfile {
    /// Validated source fields.
    pub spec: ProfileSpec,
    /// PEM bytes for exclusive trust, if configured.
    pub pem: Option<Vec<u8>>,
    /// Canonical PEM source path, if configured.
    pub pem_source: Option<PathBuf>,
}

/// A validated profile bundle ready for code generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBundle {
    /// Validated source manifest.
    pub manifest: ProfileBundle,
    /// Profiles with verified trust material.
    pub profiles: Vec<VerifiedProfile>,
    /// Hash of manifest bytes and referenced trust material.
    pub hash: String,
    /// Resolved manifest path.
    pub source: PathBuf,
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
    /// Referenced trust material could not be read or verified.
    #[error("invalid profile trust material: {0}")]
    Trust(String),
    /// A development-only profile cannot produce release artifacts.
    #[error("development-only profile bundles cannot produce release artifacts")]
    DevelopmentOnly,
}

/// Loads and validates a profile bundle and all referenced trust material.
pub fn load(path: &Path) -> Result<VerifiedBundle, ProfileError> {
    let source = path.canonicalize().map_err(|_| ProfileError::Read)?;
    let input = fs::read(&source).map_err(|_| ProfileError::Read)?;
    let manifest = toml::from_slice::<ProfileBundle>(&input).map_err(|_| ProfileError::Toml)?;
    validation::validate_manifest(&manifest)?;
    let parent = source.parent().ok_or(ProfileError::Read)?;
    let mut hasher = Sha256::new();
    hasher.update((input.len() as u64).to_le_bytes());
    hasher.update(&input);
    let mut profiles = Vec::with_capacity(manifest.profiles.len());
    for spec in &manifest.profiles {
        let trust = validation::load_trust(parent, &spec.trust)?;
        let pem = trust.pem;
        if let Some(bytes) = &pem {
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        profiles.push(VerifiedProfile { spec: spec.clone(), pem, pem_source: trust.source });
    }
    let hash = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(VerifiedBundle { manifest, profiles, hash, source })
}

/// Rejects development-only bundles before release artifact creation.
pub fn require_release(bundle: &VerifiedBundle) -> Result<(), ProfileError> {
    if bundle.manifest.development_only { Err(ProfileError::DevelopmentOnly) } else { Ok(()) }
}

#[cfg(test)]
mod tests;
