use serde::Deserialize;

use crate::{
    CURRENT_SCHEMA_VERSION, IdentityMode, IdentitySpec, ProfileBundle, ProfileError, ProfileSpec,
    TrustSpec,
};

#[derive(Deserialize)]
struct VersionProbe {
    schema_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileBundleV1 {
    schema_version: u32,
    bundle_version: String,
    profiles: Vec<ProfileSpecV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileSpecV1 {
    id: String,
    display_name: String,
    host: String,
    email_domains: Vec<String>,
    username_realm: Option<String>,
    device_id_length: u8,
    trust: TrustSpec,
}

pub(crate) fn schema_version(input: &[u8]) -> Result<u32, ProfileError> {
    toml::from_slice::<VersionProbe>(input)
        .map(|probe| probe.schema_version)
        .map_err(|_| ProfileError::Toml)
}

pub(crate) fn parse_v1(input: &[u8]) -> Result<ProfileBundle, ProfileError> {
    let legacy = toml::from_slice::<ProfileBundleV1>(input).map_err(|_| ProfileError::Toml)?;
    if legacy.schema_version != 1 {
        return Err(ProfileError::Invalid("unsupported schema_version".into()));
    }
    let profiles = legacy.profiles.into_iter().map(ProfileSpec::from).collect();
    Ok(ProfileBundle {
        schema_version: CURRENT_SCHEMA_VERSION,
        bundle_version: legacy.bundle_version,
        profiles,
    })
}

impl From<ProfileSpecV1> for ProfileSpec {
    fn from(value: ProfileSpecV1) -> Self {
        let identity = match value.username_realm {
            Some(realm) => IdentitySpec {
                mode: IdentityMode::RealmUsername,
                realm: Some(realm),
                username_hint: None,
            },
            None => IdentitySpec { mode: IdentityMode::Username, realm: None, username_hint: None },
        };
        Self {
            id: value.id,
            display_name: value.display_name,
            host: value.host,
            email_domains: value.email_domains,
            identity,
            device_id_length: value.device_id_length,
            trust: value.trust,
        }
    }
}
