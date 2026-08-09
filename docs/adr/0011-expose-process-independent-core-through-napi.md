# 0011 — Expose process-independent core through N-API

## Status

Accepted (2026-08-09)

## Context

Ruflo needs a real Node-native distribution channel without treating CLI
subprocesses as in-process native work. The former CLI-private calculations
combined argv parsing, output, current-directory state, and compute, making
them unsuitable for an ABI boundary.

## Decision

Create `ruflo-core` as the typed, process-independent owner of exported
operations, and keep `ruflo-napi` a thin napi-rs `cdylib` adapter. The initial
API is deliberately bounded to deterministic embedding, vector similarity, and
task routing. Its provider name explicitly distinguishes the deterministic
vectorizer from BGE/MiniLM semantic-model parity.

## Consequences

- Node receives a genuine in-process native addon and direct Rust/N-API
  equivalence tests can share contracts.
- CLI and N-API can evolve independently from process state.
- Stateful semantic memory is withheld until it has an explicit confined
  context and source-derived interoperability contract.

## Implementation note — 2026-08-09

Implemented by `ruflo-core`, `ruflo-napi`, and `packages/ruflo-native`.
`native-release.yml` builds matching Linux x86_64/aarch64, macOS x86_64/aarch64,
and Windows x86_64 addon artifacts beside both native CLI aliases. The public
API remains intentionally bounded; it must not be used to imply BGE semantic
memory parity or NPM publication until those contracts have their own evidence.

## Alternatives

- Wrap the CLI through Node child processes — rejected because it is not N-API.
- Expose every CLI command — rejected because interactive and filesystem side
  effects do not make a safe stable addon API.
