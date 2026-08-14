# Security policy

## Supported versions

Security fixes are applied to the latest source release. This project does not
publish prebuilt public binaries in its initial release.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository. Do not place
credentials, mailbox content, private endpoint metadata, or internal trust
material in a public issue. Include the affected commit, reproduction steps,
and the expected security boundary.

## Trust boundary

The MCP server and every other process running as the same macOS user are in
one trusted local boundary. Credentials are stored in that user's Keychain and
runtime files use user-only permissions. The application does not attempt to
protect data from another process with the same UID.

MCP client name and version are self-reported protocol fields. They are a
compatibility guard used to keep write tools disabled for unknown clients; they
are not authentication. Likewise, a client's `ask` policy is a user-experience
confirmation and not an authorization boundary. Account-level writes remain
disabled by default, and every mutation requires an idempotency UUID.

## Network boundary

Profiles are validated and embedded at build time. Runtime account config stores
only a profile key, so it cannot select an arbitrary host, certificate, realm,
or protocol. The transport fixes HTTPS, the
`/Microsoft-Server-ActiveSync` path, Basic authentication over TLS, EAS 14.1,
disabled redirects, and response-origin validation.

Trust mode is either the macOS system store or one exclusive embedded PEM with
a build-verified SHA-256 fingerprint. TLS verification cannot be disabled by
configuration or environment variables.

## Secrets and content

Passwords, Device IDs, policy state, and the HMAC key are not `Debug` values and
are stored in macOS Keychain. SQLite contains only idempotency metadata and
keyed payload hashes. Mail and calendar data stays in process memory, except for
explicitly downloaded attachments in a private 24-hour cache.

Mailbox content is untrusted external input. HTML is converted to plain text,
external images are not fetched, file names are sanitized, and MCP responses
mark external content explicitly.
