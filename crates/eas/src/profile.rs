use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{EasError, Result};

/// Validated identifier of an endpoint profile embedded at build time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileKey(String);

impl ProfileKey {
    /// Parses a stable lowercase profile identifier.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if valid_profile_key(&value) {
            Ok(Self(value))
        } else {
            Err(EasError::InvalidConfiguration("profile key is invalid".into()))
        }
    }

    /// Returns the serialized profile identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProfileKey {
    type Err = EasError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl Serialize for ProfileKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProfileKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Immutable Exchange endpoint profile compiled into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    key: &'static str,
    display_name: &'static str,
    pub(crate) host: &'static str,
    email_domains: &'static [&'static str],
    username_realm: Option<&'static str>,
    device_id_length: u8,
    pub(crate) extra_ca_pem: Option<&'static [u8]>,
}

impl Profile {
    /// Returns this profile's stable identifier.
    #[must_use]
    pub const fn key(self) -> &'static str {
        self.key
    }

    /// Returns this profile's human-readable name.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        self.display_name
    }

    /// Returns the only allowed EAS URL for this profile.
    #[must_use]
    pub fn endpoint(self) -> String {
        format!("https://{}/Microsoft-Server-ActiveSync", self.host)
    }

    /// Returns the exact Device ID length required by this endpoint.
    #[must_use]
    pub const fn device_id_length(self) -> usize {
        self.device_id_length as usize
    }

    /// Reports whether this profile uses an exclusive embedded trust anchor.
    #[must_use]
    pub const fn has_extra_trust_anchor(self) -> bool {
        self.extra_ca_pem.is_some()
    }

    /// Validates account identity against the compiled profile.
    pub fn validate_identity(self, email: &str, username: &str) -> Result<()> {
        let domain = email.rsplit_once('@').map(|(_, domain)| domain);
        if domain.is_none_or(|domain| {
            !self.email_domains.iter().any(|allowed| domain.eq_ignore_ascii_case(allowed))
        }) {
            return Err(EasError::InvalidConfiguration(
                "email does not match the selected profile".into(),
            ));
        }
        if username.trim().is_empty() || username.chars().any(char::is_control) {
            return Err(EasError::InvalidConfiguration("username must not be empty".into()));
        }
        if let Some(required) = self.username_realm {
            let actual = username.split_once('\\').map(|(realm, _)| realm);
            if actual.is_none_or(|realm| !realm.eq_ignore_ascii_case(required)) {
                return Err(EasError::InvalidConfiguration(
                    "username does not match the selected profile realm".into(),
                ));
            }
        }
        Ok(())
    }

    /// Validates the stable EAS Device ID for this profile.
    pub fn validate_device_id(self, device_id: &str) -> Result<()> {
        if device_id.len() != self.device_id_length()
            || !device_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(EasError::InvalidConfiguration(
                "DeviceId does not match the selected profile".into(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) const fn localhost() -> Self {
        Self {
            key: "localhost",
            display_name: "Local test",
            host: "localhost",
            email_domains: &["example.invalid"],
            username_realm: None,
            device_id_length: 32,
            extra_ca_pem: None,
        }
    }
}

/// Registry of all immutable profiles embedded into this build.
#[derive(Debug, Clone, Copy)]
pub struct ProfileRegistry {
    bundle_version: &'static str,
    bundle_hash: &'static str,
    development_only: bool,
    profiles: &'static [Profile],
}

impl ProfileRegistry {
    /// Returns the compile-time profile registry.
    #[must_use]
    pub const fn compiled() -> &'static Self {
        &COMPILED_REGISTRY
    }

    /// Resolves a profile key without permitting an arbitrary endpoint.
    #[must_use]
    pub fn get(self, key: &ProfileKey) -> Option<Profile> {
        self.profiles.iter().copied().find(|profile| profile.key == key.as_str())
    }

    /// Resolves a profile or returns a redacted configuration error.
    pub fn require(self, key: &ProfileKey) -> Result<Profile> {
        self.get(key)
            .ok_or_else(|| EasError::InvalidConfiguration("profile is not compiled in".into()))
    }

    /// Returns all available profiles.
    #[must_use]
    pub const fn profiles(self) -> &'static [Profile] {
        self.profiles
    }

    /// Returns the first compiled profile key for deterministic harness defaults.
    #[must_use]
    pub fn default_key(self) -> ProfileKey {
        match self.profiles {
            [profile, ..] => ProfileKey(profile.key.to_owned()),
            [] => ProfileKey("unavailable".into()),
        }
    }

    /// Returns the operator-defined profile bundle version.
    #[must_use]
    pub const fn bundle_version(self) -> &'static str {
        self.bundle_version
    }

    /// Returns the SHA-256 hash of the profile manifest and trust material.
    #[must_use]
    pub const fn bundle_hash(self) -> &'static str {
        self.bundle_hash
    }

    /// Reports whether this build uses a development-only bundle.
    #[must_use]
    pub const fn development_only(self) -> bool {
        self.development_only
    }
}

fn valid_profile_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'-' | b'_'))
        })
}

include!(concat!(env!("OUT_DIR"), "/profiles.rs"));
