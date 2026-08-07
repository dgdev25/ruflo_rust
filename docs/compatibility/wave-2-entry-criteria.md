# Wave 2 Entry Criteria

Updated: 2026-08-07

Wave 2 capabilities may move from `unsupported` or `migrating` to `supported` only when the release manifest records all of the evidence below and `CapabilityManifest::validate_release(2)` passes.

- Named consumer fixtures for every promoted contract.
- Security coverage for the promoted transport and authentication boundaries.
- Native platform evidence tied to checked-in platform tests, smoke scripts, and recorded target-host runs.
- Migration and RVF regression tests proving persistence and interchange behavior.
- Supply-chain review evidence covering audit, SBOM generation, and maintained dependency policy.
- ADR records for each newly selected long-lived integration.
