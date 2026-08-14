# Build-time profiles

Profiles bind a binary to a reviewed set of Exchange ActiveSync endpoints. They
are build inputs, not runtime configuration.

## Schema

```toml
schema_version = 1
bundle_version = "operator-release-1"
development_only = false

[[profiles]]
id = "work"
display_name = "Work Mail"
host = "mail.example.invalid"
email_domains = ["example.invalid"]
username_realm = "EXAMPLE"
device_id_length = 16

[profiles.trust]
mode = "system"
```

`id` is a stable lowercase key stored in account config. Host and domains are
lowercase DNS names without scheme, port, path, wildcard, or trailing dot.
`username_realm` is optional. Device ID length is 16 or 32 ASCII characters.

Trust mode is exactly one of:

```toml
[profiles.trust]
mode = "system"
```

or:

```toml
[profiles.trust]
mode = "exclusive_pem"
pem = "certs/root.pem"
sha256 = "00:11:...:FF"
```

Exclusive PEM paths are relative to the manifest, traversal-free, regular
non-symlink `.pem` files containing exactly one certificate and no private key.
The fingerprint is SHA-256 over DER bytes. Exclusive mode disables system roots
for that profile.

## Local layout

```text
.private/
  profile.toml
  certs/
    root.pem
  public-audit-denylist.txt
```

The whole directory is ignored. Never place credentials, usernames, personal
email addresses, private keys, or generated account config there.

## Commands

```bash
cargo xtask profile verify
cargo xtask profile verify --profile-bundle .private/profile.toml --release
cargo xtask build-bundles --profile-bundle .private/profile.toml
```

The first command validates the public development example. The second applies
the release eligibility gate. The third embeds the same verified manifest into
both architecture-specific artifacts and records its version/hash.

`EAS_MAIL_PROFILE_BUNDLE` is an internal Cargo build input set by `xtask`; it is
not read by the runtime. Direct operator builds may set it only for compilation:

```bash
EAS_MAIL_PROFILE_BUNDLE=.private/profile.toml cargo build --release --locked
```
