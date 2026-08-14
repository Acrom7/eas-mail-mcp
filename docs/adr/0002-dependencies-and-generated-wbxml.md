# ADR 0002: Dependencies and generated specifications

- Status: accepted
- Date: 2026-08-14

## Context

The protocol implementation needs reviewed WBXML tables, reproducible endpoint
profiles, and an MCP implementation that follows the published protocol.

## Decision

Pin Rust 1.95.0 and the official Rust MCP SDK `rmcp` 3.0.1. Keep all EAS WBXML
code pages as declarative TOML under `spec/codepages`; `eas/build.rs` validates
and generates immutable tables only in Cargo `OUT_DIR`.

Use the shared `profile` crate from both `build.rs` and `xtask`. Profile source
and certificates remain build inputs, while only generated constants and
approved PEM bytes enter the runtime binary. `cargo-deny` controls advisories,
licenses, sources, and duplicate versions.

## Consequences

Generated protocol and profile tables cannot drift unnoticed in source control.
Toolchain, SDK, schema, and trust changes are explicit changes requiring golden,
compatibility, security, and cross-architecture checks.
