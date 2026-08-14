use std::error::Error;
use std::fs;

use rcgen::generate_simple_self_signed;
use sha2::{Digest as _, Sha256};

use super::{ProfileError, load, require_release};

#[test]
fn system_profile_loads_and_development_bundle_cannot_release() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("profile.toml");
    fs::write(&path, system_manifest("mail.example.invalid", "example.invalid", None))?;
    let bundle = load(&path)?;
    assert_eq!(bundle.manifest.bundle_version, "fixture-1");
    assert_eq!(bundle.profiles.len(), 1);
    let profile = bundle
        .profiles
        .first()
        .ok_or_else(|| std::io::Error::other("verified profile is missing"))?;
    assert!(profile.pem.is_none());
    assert_eq!(bundle.hash.len(), 64);
    assert!(matches!(require_release(&bundle), Err(ProfileError::DevelopmentOnly)));
    Ok(())
}

#[test]
fn exclusive_pem_is_verified_and_loaded() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let certified = generate_simple_self_signed(vec!["mail.example.invalid".into()])?;
    let pem = certified.cert.pem();
    let fingerprint = Sha256::digest(certified.cert.der().as_ref())
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    fs::create_dir(directory.path().join("certs"))?;
    fs::write(directory.path().join("certs/root.pem"), &pem)?;
    let path = directory.path().join("profile.toml");
    fs::write(&path, pem_manifest("certs/root.pem", &fingerprint))?;
    let bundle = load(&path)?;
    let profile = bundle
        .profiles
        .first()
        .ok_or_else(|| std::io::Error::other("verified profile is missing"))?;
    assert_eq!(profile.pem.as_deref(), Some(pem.as_bytes()));
    assert!(require_release(&bundle).is_ok());
    Ok(())
}

#[test]
fn malformed_and_invalid_profile_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("profile.toml");
    fs::write(&path, "not = [valid")?;
    assert!(matches!(load(&path), Err(ProfileError::Toml)));

    let invalid = [
        system_manifest("localhost", "example.invalid", None),
        system_manifest("mail.example.invalid:443", "example.invalid", None),
        system_manifest("mail.example.invalid", "not a domain", None),
        system_manifest("mail.example.invalid", "example.invalid", Some("BAD\\REALM")),
        system_manifest("mail.example.invalid", "example.invalid", Some("")),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("id = \"example\"", "id = \"Bad Key\""),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("device_id_length = 16", "device_id_length = 15"),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("schema_version = 1", "schema_version = 2"),
        system_manifest("mail.example.invalid", "example.invalid", None)
            + "\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Duplicate\"\nhost = \"other.example.invalid\"\nemail_domains = [\"example.invalid\"]\ndevice_id_length = 16\n[profiles.trust]\nmode = \"system\"\n",
    ];
    for document in invalid {
        fs::write(&path, document)?;
        assert!(load(&path).is_err());
    }
    Ok(())
}

#[test]
fn trust_path_mode_and_fingerprint_fail_closed() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("profile.toml");
    for document in [
        pem_manifest("../root.pem", &"00".repeat(32)),
        pem_manifest("missing.pem", &"00".repeat(32)),
        pem_manifest("root.pem", "not-a-fingerprint"),
        system_manifest("mail.example.invalid", "example.invalid", None)
            .replace("mode = \"system\"", "mode = \"unsupported\""),
    ] {
        fs::write(&path, document)?;
        assert!(load(&path).is_err());
    }

    let certified = generate_simple_self_signed(vec!["mail.example.invalid".into()])?;
    fs::write(directory.path().join("root.pem"), certified.cert.pem())?;
    fs::write(&path, pem_manifest("root.pem", &"00".repeat(32)))?;
    assert!(matches!(load(&path), Err(ProfileError::Trust(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn trust_path_rejects_symlinks() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let certified = generate_simple_self_signed(vec!["mail.example.invalid".into()])?;
    let target = directory.path().join("actual.pem");
    fs::write(&target, certified.cert.pem())?;
    symlink(&target, directory.path().join("root.pem"))?;
    let fingerprint = Sha256::digest(certified.cert.der().as_ref())
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let path = directory.path().join("profile.toml");
    fs::write(&path, pem_manifest("root.pem", &fingerprint))?;
    assert!(matches!(load(&path), Err(ProfileError::Trust(_))));
    Ok(())
}

fn system_manifest(host: &str, domain: &str, realm: Option<&str>) -> String {
    let realm = realm.map_or_else(String::new, |value| format!("username_realm = {value:?}\n"));
    format!(
        "schema_version = 1\nbundle_version = \"fixture-1\"\ndevelopment_only = true\n\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Example EAS\"\nhost = {host:?}\nemail_domains = [{domain:?}]\n{realm}device_id_length = 16\n\n[profiles.trust]\nmode = \"system\"\n"
    )
}

fn pem_manifest(path: &str, fingerprint: &str) -> String {
    format!(
        "schema_version = 1\nbundle_version = \"fixture-1\"\ndevelopment_only = false\n\n[[profiles]]\nid = \"example\"\ndisplay_name = \"Example EAS\"\nhost = \"mail.example.invalid\"\nemail_domains = [\"example.invalid\"]\ndevice_id_length = 16\n\n[profiles.trust]\nmode = \"exclusive_pem\"\npem = {path:?}\nsha256 = {fingerprint:?}\n"
    )
}
