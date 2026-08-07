# Persistence Compatibility Fixtures

Task 2 creates the harness and schema for persistence compatibility without checking in live database contents.

Rules for fixtures in this directory:

- Add only curated, approved fixture files.
- Never commit secrets, tokens, private keys, or raw database snapshots containing user data.
- Never commit absolute machine paths, hostnames, or other environment-specific values.
- Prefer synthetic or sanitized fixtures that prove locking, backup, rollback, and migration semantics.
- Mark every fixture with provenance and recording metadata so source-oracle captures stay distinct from reduced-schema synthetic fixtures.
- Record the legacy source, reduction note, and redactions applied in the fixture metadata and any follow-up task.
