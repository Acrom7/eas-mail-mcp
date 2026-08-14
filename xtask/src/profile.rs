use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use eas_mail_profile::{VerifiedBundle, load, require_release};

pub(crate) fn verify(root: &Path, path: &Path, release: bool) -> Result<VerifiedBundle> {
    let source = resolve(root, path);
    let bundle = load(&source).with_context(|| format!("cannot verify {}", path.display()))?;
    if release {
        require_release(&bundle)?;
    }
    let profiles = bundle
        .profiles
        .iter()
        .map(|profile| {
            serde_json::json!({
                "id": profile.spec.id,
                "trust": if profile.pem.is_some() { "exclusive_pem" } else { "system" },
                "device_id_length": profile.spec.device_id_length,
            })
        })
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "schema_version": bundle.manifest.schema_version,
        "bundle_version": bundle.manifest.bundle_version,
        "bundle_hash": bundle.hash,
        "development_only": bundle.manifest.development_only,
        "profiles": profiles,
        "release_eligible": !bundle.manifest.development_only,
    });
    writeln!(io::stdout().lock(), "{}", serde_json::to_string_pretty(&report)?)?;
    Ok(bundle)
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_owned() } else { root.join(path) }
}
