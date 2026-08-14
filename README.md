# EAS Mail MCP

`eas-mail-mcp` is a local Rust MCP server for Exchange ActiveSync 14.1. It gives
supported AI clients structured mail tools and read-only calendar tools without
a daemon, GUI, mailbox database, or runtime endpoint configuration.

Endpoint profiles are validated and embedded at build time. The public tree
ships only a non-routable `example.invalid` development profile. Operators can
build deployments from a local ignored profile bundle without adding server
names, trust anchors, or realm information to Git.

## MCP tools

Read tools:

- `accounts_list`, `folders_list`, `sync_status`, `sync_now`
- `mail_list`, `mail_search`, `mail_get`
- `mail_list_attachments`, `mail_download_attachment`
- `calendar_list`, `calendar_search`, `calendar_get`

Write tools:

- `mail_mark_read`, `mail_send`, `mail_reply`, `mail_forward`

Writes are disabled per account by default. Every write requires a UUID
`idempotency_key`; a content-free SQLite journal prevents blind replay after an
ambiguous network result. Passwords, Device IDs, policy state, and the journal
HMAC key are stored in macOS Keychain.

## Build the example

Requirements are macOS 14+ and the Rust toolchain pinned in
`rust-toolchain.toml`.

```bash
cargo xtask profile verify
cargo build --locked --release --package eas-mail-mcp
cargo xtask test
```

The example binary is functional but cannot connect to a real endpoint. Release
bundles deliberately reject `development_only` profiles.

## Build a deployment

Create a local `.private/profile.toml` from
[`profile.example.toml`](profile.example.toml), then verify and build it:

```bash
cargo xtask profile verify --profile-bundle .private/profile.toml --release
cargo xtask build-bundles --profile-bundle .private/profile.toml
```

`.private/` is ignored in full. Profiles are compile-time input only: the
runtime does not read `.env`, profile TOML, certificate files, or endpoint
variables. See [Build-time profiles](docs/build-time-profiles.md) for the schema
and [Security](SECURITY.md) for the trust boundary.

## Engineering gates

```bash
./scripts/bootstrap-tools.sh
cargo xtask check
cargo xtask public-audit
cargo xtask goldens verify
```

The harness covers WBXML, EAS pagination and policy handling, TLS failures,
idempotent writes, MCP stdio framing, cursor expiry, and subprocess resilience.
See [CONTRIBUTING.md](CONTRIBUTING.md) and
[architecture.md](docs/architecture.md).

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
