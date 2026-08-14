# Threat model and release controls

The authoritative trust-boundary summary is in [`SECURITY.md`](../SECURITY.md).
This document records engineering controls used by release builds.

## Build inputs

- `profile.example.toml` is public, non-routable, and development-only.
- Deployment manifests and certificates stay under ignored `.private/`.
- `cargo xtask profile verify` validates schema, host and domain syntax,
  duplicate IDs, realm syntax, Device ID length, trust mode, traversal, symlink,
  PEM shape, and certificate fingerprint.
- `cargo xtask build-bundles` rejects development-only manifests.
- The profile version and content hash are embedded in the binary and written
  to `BUILD-METADATA.json` with source and artifact hashes.

## Publication controls

`cargo xtask public-audit` rejects tracked private-directory files, local user
paths, proprietary-license residue, and any operator denylist terms in the
tracked tree or Git history. Gitleaks scans the public tree and full history.
When present, `.private/` receives a separate credential/private-key scan while
allowed endpoint metadata is not treated as a secret.

Release builds remap workspace and Cargo source paths. A `strings` gate rejects
local build paths and confirms that profile version/hash metadata is present.
Source releases contain no binaries or ignored files.

## Runtime controls

- Redirects and changed response origins fail closed.
- HTTP and arbitrary runtime endpoints are not representable.
- EAS policy is acknowledged only when its requirements are supported.
- Remote wipe purges account credentials, process references, attachments, and
  journal rows.
- Ambiguous mutations are not retried and return `OUTCOME_UNKNOWN`.
- Write tools require account opt-in, a supported compatibility profile, client
  confirmation configuration, and an idempotency UUID.
