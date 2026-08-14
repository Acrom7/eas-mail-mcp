# ADR 0003: MCP contract and external writes

- Status: accepted
- Date: 2026-08-14

## Context

Agents need predictable tools across multiple accounts, while mail mutations can
be duplicated by retries or initiated without adequate user review.

## Decision

Expose 12 read tools and four mail write tools with structured schemas. Lists are
bounded to 100, previews to 500 characters, and bodies to 12,000 by default and
50,000 maximum. Calendar remains read-only in this version. Search always runs
on Exchange; cursors point to immutable 15-minute RAM snapshots.

Writes require account opt-in, a supported interactive client, client-side
`ask`, and a UUID. Persist a pending content-free journal record before the EAS
request and use the UUID as ClientId. Never retry an ambiguous mutation. Return
`OUTCOME_UNKNOWN` until a human reconciles it.

## Consequences

Callers must obtain fresh references after process restart or cursor expiry.
Unknown clients retain full read access but cannot mutate. A headless alias is
not shipped until a concrete client passes a real headless smoke test.
