use std::path::PathBuf;

use super::super::*;

#[test]
fn profile_paths_accept_plain_spaces_and_wrapping_quotes() {
    assert_eq!(
        profiles::input_path("/tmp/profile file.toml"),
        PathBuf::from("/tmp/profile file.toml")
    );
    assert_eq!(
        profiles::input_path("'/tmp/profile file.toml'"),
        PathBuf::from("/tmp/profile file.toml")
    );
}

#[test]
fn blank_arguments_keep_writes_and_client_setup_disabled() {
    let setup = blank_setup_args();
    assert!(!setup.enable_writes);
    assert!(setup.skip_clients);
    let profile = blank_profile_args();
    assert!(profile.identity_mode.is_none());
    assert!(profile.username_realm.is_none());
}
