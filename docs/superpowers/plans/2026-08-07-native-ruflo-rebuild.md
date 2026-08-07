# Native Ruflo Rebuild Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a pure-Rust Ruflo with proven, staged compatibility for rUvNet consumers without rebuilding existing rUv Rust components.

**Architecture:** A compatibility facade owns the CLI, MCP schema, policy, and diagnostics; a runtime owns agents, tasks, swarms, workflows, and explicit handles; persistence ports mediate legacy fixtures and existing rUv RVF adapters. One dispatcher serves stdio MCP in Wave 1 and stateless HTTP MCP in Wave 2.

**Tech Stack:** Rust 1.87+ · rmcp 3.1.1 · clap 4.6.6 · figment 0.10.19 · tokio 1.53.1 · optional petgraph 0.8.3 · rUv RVF/RuVector adapters · assert_cmd 2.2.2 · insta 1.48.0.

## Global Constraints

- Satisfy REQ-001 through REQ-017; add a test before each behavioural implementation.
- The runtime is pure Rust: no Node.js process, runtime, or JavaScript execution dependency.
- Never copy, fork, or reimplement existing rUv Rust applications, RVF, RuVector, AgentDB adapters, or Agentic Flow adapters.
- Use version-pinned upstream Rust dependencies, preserve unknown RVF data, and test real fixtures before enabling migration.
- Support Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64 with native-runner tests.
- Keep all source under `src/`, tests under `tests/`, fixtures under `tests/fixtures/`, and scripts under `scripts/`.
- MCP stdio stdout contains only JSON-RPC. All logs and diagnostics use stderr.
- HTTP MCP is disabled by default. When enabled, it requires server-side authenticated identity, capability authorization, and resource limits.
- New direct dependencies must be permissively licensed, version-specific OSV checked, lockfile-audited, licence/source-policy checked, and recorded in the SBOM.
- Any deferred capability returns a typed `unsupported_in_wave` error and appears in the capability manifest.

---

## Phase 1 — Establish the contract harness before product code

### Task 1: Create the native workspace and shared contract types

**Requirements:** REQ-001, REQ-004, REQ-008, REQ-017

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `src/crates/ruflo-types/Cargo.toml`
- Create: `src/crates/ruflo-types/src/lib.rs`
- Create: `src/crates/ruflo-types/src/capability.rs`
- Create: `src/crates/ruflo-types/src/error.rs`
- Create: `tests/types_contract.rs`

**Interfaces:**
- Produces `Capability { name, wave, status, migration }` and `RufloError` for all facades and runtime crates.

- [ ] **Step 1: Write the failing contract tests**

```rust
use ruflo_types::{Capability, CapabilityStatus, RufloError};

#[test]
fn unsupported_capability_has_stable_machine_fields() {
    let capability = Capability::unsupported("workflow.run", 2, "enable Wave 2");
    assert_eq!(capability.status, CapabilityStatus::Unsupported);
    assert_eq!(capability.wave, 2);
    assert!(matches!(RufloError::unsupported(capability), RufloError::UnsupportedInWave { .. }));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test types_contract`

Expected: FAIL because the workspace and `ruflo_types` crate do not exist.

- [ ] **Step 3: Create the workspace and minimal public types**

```rust
pub enum RufloError {
    InvalidInput { code: &'static str, message: String },
    Unauthenticated,
    Unauthorized { capability: String },
    UnsupportedInWave { capability: Capability },
    RateLimited { retry_after_ms: u64 },
    Timeout, Cancelled, LockConflict, MigrationFailed { message: String },
    UpstreamAdapter { message: String },
}
```

Set `rust-toolchain.toml` to `1.87.0` and make workspace members live under `src/crates/`.

- [ ] **Step 4: Run formatting and tests**

Run: `cargo fmt --check && cargo test --test types_contract`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml src/crates/ruflo-types tests/types_contract.rs
git commit -m "feat: establish native Ruflo contract types"
```

### Task 2: Build the differential compatibility fixture harness

**Requirements:** REQ-002, REQ-003, REQ-005, REQ-007, REQ-012

**Files:**
- Create: `tests/fixtures/cli/version.json`
- Create: `tests/fixtures/mcp/tools-list.json`
- Create: `tests/fixtures/persistence/README.md`
- Create: `tests/compat/fixture_schema.rs`
- Create: `tests/compat/differential_cli.rs`
- Create: `scripts/capture-reference-contract.sh`
- Create: `scripts/verify-fixtures.sh`

**Interfaces:**
- Consumes: a command invocation and captured JSON fixture.
- Produces: `assert_cli_fixture(binary, fixture)` and `assert_json_rpc_fixture(request, response)`.

- [ ] **Step 1: Write a failing fixture-schema test**

```rust
#[test]
fn cli_fixture_requires_exit_stdout_and_stderr() {
    let parsed = Fixture::parse(r#"{"argv":["--version"],"exit":0,"stdout":"ruflo vX\n","stderr":""}"#).unwrap();
    assert_eq!(parsed.exit, 0);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test differential_cli`

Expected: FAIL because `Fixture` and fixtures do not exist.

- [ ] **Step 3: Implement fixture parsing and reference capture**

Define fixture fields `argv`, `stdin`, `exit`, `stdout`, `stderr`, `environment`, and `platform`. Make the capture script write only explicitly approved fixtures, redacting environment values and refusing to overwrite an existing fixture without `--replace`.

- [ ] **Step 4: Add the first version and MCP discovery fixtures**

Capture source-Ruflo `--version`, `--help`, and `tools/list` contract records. Keep secrets, database contents, and machine-specific paths out of fixtures.

- [ ] **Step 5: Run validation**

Run: `cargo test --test differential_cli && bash scripts/verify-fixtures.sh`

Expected: PASS and no fixture contains an absolute home path or secret-like value.

- [ ] **Step 6: Commit**

```bash
git add tests scripts
git commit -m "test: add Ruflo compatibility fixture harness"
```

### Task 3: Inventory rUvNet consumers and map every P0 contract

**Requirements:** REQ-002, REQ-003, REQ-005, REQ-006, REQ-010, REQ-012, REQ-013

**Files:**
- Create: `docs/compatibility/consumer-inventory.md`
- Create: `docs/compatibility/contract-matrix.md`
- Create: `tests/fixtures/consumers/README.md`
- Create: `scripts/inventory-consumers.sh`
- Test: `tests/compat/fixture_schema.rs`

**Interfaces:**
- Produces a matrix with consumer, invocation type, contract, fixture path, wave, status, and owner.

- [ ] **Step 1: Write the failing inventory completeness test**

```rust
#[test]
fn every_p0_contract_has_a_consumer_fixture_or_explicit_blocker() {
    let matrix = ContractMatrix::load("docs/compatibility/contract-matrix.md").unwrap();
    assert!(matrix.p0_rows().all(|row| row.fixture.is_some() || row.blocker.is_some()));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test fixture_schema every_p0_contract`

Expected: FAIL because the matrix does not exist.

- [ ] **Step 3: Implement inventory script and matrix**

Search the source Ruflo and rUvNet trees for binary invocations, MCP configurations, plugin manifests, `.swarm/memory.db`, RVF/RVFA references, environment variables, and in-process imports. Assign each row to Wave 1, 2, or 3; do not classify unknown contracts as supported.

- [ ] **Step 4: Register P0 fixtures**

Add rows for binary aliases, version/help, stdio `tools/list`/`tools/call`, memory round trips, migration fixtures, policy denials, and platform hooks.

- [ ] **Step 5: Run validation**

Run: `cargo test --test fixture_schema && bash scripts/inventory-consumers.sh --check`

Expected: PASS with every P0 row mapped.

- [ ] **Step 6: Commit**

```bash
git add docs/compatibility tests scripts
git commit -m "docs: map native Ruflo consumer contracts"
```

## Phase 2 — Native Wave 1 façade and local MCP

### Task 4: Implement native CLI aliases and deterministic output

**Requirements:** REQ-001, REQ-002, REQ-007, REQ-008

**Files:**
- Create: `src/crates/ruflo-cli/Cargo.toml`
- Create: `src/crates/ruflo-cli/src/lib.rs`
- Create: `src/crates/ruflo-cli/src/command.rs`
- Create: `src/bin/ruflo/Cargo.toml`
- Create: `src/bin/ruflo/src/main.rs`
- Create: `src/bin/claude-flow/Cargo.toml`
- Create: `src/bin/claude-flow/src/main.rs`
- Test: `tests/compat/differential_cli.rs`

**Interfaces:**
- Consumes: `ruflo_types::RufloError`.
- Produces: `pub fn run(argv: impl IntoIterator<Item = OsString>) -> ExitCode`.

- [ ] **Step 1: Add failing alias/version/help fixtures**

```rust
#[test]
fn both_binaries_match_version_fixture() {
    assert_cli_fixture("ruflo", "tests/fixtures/cli/version.json");
    assert_cli_fixture("claude-flow", "tests/fixtures/cli/version.json");
}
```

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test --test differential_cli both_binaries_match_version_fixture`

Expected: FAIL because both binaries are absent.

- [ ] **Step 3: Implement a shared parser and two thin mains**

Implement `--version`, `--help`, and `mcp start` through one `ruflo_cli::run` path. Resolve version from compile-time package metadata before initializing adapters or models. Send parser errors to stderr and return a stable non-zero exit code.

- [ ] **Step 4: Run CLI contract tests**

Run: `cargo test --test differential_cli && cargo run -p ruflo -- --version && cargo run -p claude-flow -- --version`

Expected: PASS; both commands match the fixture exactly.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add native Ruflo CLI aliases"
```

### Task 5: Implement capability manifest, config precedence, and local policy

**Requirements:** REQ-002, REQ-008, REQ-009, REQ-012, REQ-016

**Files:**
- Create: `src/crates/ruflo-config/Cargo.toml`
- Create: `src/crates/ruflo-config/src/lib.rs`
- Create: `src/crates/ruflo-config/src/policy.rs`
- Create: `src/crates/ruflo-config/src/capability_manifest.rs`
- Create: `tests/config_precedence.rs`
- Create: `tests/policy_enforcement.rs`

**Interfaces:**
- Produces `EffectiveConfig::load()`, `CapabilityManifest::from_registry()`, and `ToolPolicy::authorize(&Caller, &str)`.

- [ ] **Step 1: Write failing precedence and denial tests**

```rust
#[test]
fn deny_overrides_profile_and_allowlist() {
    let policy = ToolPolicy::from_env([("RUFLO_MCP_ALLOW", "memory_search"), ("RUFLO_MCP_DENY", "memory_search")]);
    assert!(policy.authorize(&Caller::local(), "memory_search").is_err());
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test --test policy_enforcement`

Expected: FAIL because policy/config crates are absent.

- [ ] **Step 3: Implement typed layering and policy**

Define precedence `explicit CLI > environment > project config > defaults`. Validate all policy tokens against registered capabilities. Enforce maximum request bytes, concurrent executions, and execution duration before dispatch. Generate a capability manifest with `supported`, `migrating`, or `unsupported` status.

- [ ] **Step 4: Run tests**

Run: `cargo test --test config_precedence --test policy_enforcement`

Expected: PASS; denied tools are not discoverable or callable.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add capability policy and configuration"
```

### Task 6: Implement a clean stdio MCP server over the shared dispatcher

**Requirements:** REQ-003, REQ-004, REQ-008, REQ-009, REQ-012

**Files:**
- Create: `src/crates/ruflo-mcp/Cargo.toml`
- Create: `src/crates/ruflo-mcp/src/lib.rs`
- Create: `src/crates/ruflo-mcp/src/dispatcher.rs`
- Create: `src/crates/ruflo-mcp/src/stdio.rs`
- Create: `tests/mcp_stdio.rs`
- Modify: `src/crates/ruflo-cli/src/command.rs`

**Interfaces:**
- Consumes: `EffectiveConfig`, `ToolPolicy`, `RufloRuntime`.
- Produces: `Dispatcher::call(RequestContext, ToolCall) -> Result<ToolResult, RufloError>` and `serve_stdio(dispatcher)`.

- [ ] **Step 1: Write failing JSON-RPC tests**

```rust
#[tokio::test]
async fn tools_list_never_includes_denied_tool_and_logs_do_not_pollute_stdout() {
    let output = run_stdio_request(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    assert!(output.stdout.lines().all(|line| serde_json::from_str::<Value>(line).is_ok()));
    assert!(!tool_names(&output.stdout).contains(&"hooks_shell".to_string()));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test --test mcp_stdio`

Expected: FAIL because the MCP server does not exist.

- [ ] **Step 3: Implement stdio transport and dispatcher**

Use rmcp for protocol handling where its supported stdio API matches the fixture. Keep `tools/list` and `tools/call` schema generation in `Dispatcher`; route all operational logging through a stderr subscriber. Map every `RufloError` to a stable MCP error object with a correlation ID.

- [ ] **Step 4: Run round-trip and schema tests**

Run: `cargo test --test mcp_stdio && cargo test --test differential_cli`

Expected: PASS; each stdout line parses as JSON-RPC and source fixtures match.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add compatible stdio MCP dispatcher"
```

## Phase 3 — Runtime and rUv persistence integration

### Task 7: Define agent, task, swarm, and workflow domain handles

**Requirements:** REQ-004, REQ-008, REQ-010, REQ-013

**Files:**
- Create: `src/crates/ruflo-runtime/Cargo.toml`
- Create: `src/crates/ruflo-runtime/src/lib.rs`
- Create: `src/crates/ruflo-runtime/src/agent.rs`
- Create: `src/crates/ruflo-runtime/src/task.rs`
- Create: `src/crates/ruflo-runtime/src/swarm.rs`
- Create: `src/crates/ruflo-runtime/src/workflow.rs`
- Create: `tests/runtime_lifecycle.rs`

**Interfaces:**
- Produces `Runtime::{spawn_agent, create_task, init_swarm, cancel_task, get_handle}`.

- [ ] **Step 1: Write failing lifecycle and cancellation tests**

```rust
#[tokio::test]
async fn cancelled_task_is_terminal_and_retains_auditable_handle() {
    let task = Runtime::ephemeral().create_task(NewTask::named("fixture")).await.unwrap();
    Runtime::ephemeral().cancel_task(task.id).await.unwrap();
    assert_eq!(Runtime::ephemeral().get_task(task.id).await.unwrap().state, TaskState::Cancelled);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test runtime_lifecycle`

Expected: FAIL because the runtime crate is absent.

- [ ] **Step 3: Implement domain state machine**

Use explicit opaque IDs. Permit only defined transitions; reject duplicate cancel, unknown ID, and invalid transition with stable errors. Keep workflow graph analysis behind a trait; introduce petgraph only when Wave 0 fixtures require topology operations.

- [ ] **Step 4: Run lifecycle tests**

Run: `cargo test --test runtime_lifecycle`

Expected: PASS with deterministic terminal state and error mapping.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add Ruflo runtime lifecycle domain"
```

### Task 8: Implement persistence ports and legacy fixture safety

**Requirements:** REQ-005, REQ-008, REQ-017

**Files:**
- Create: `src/crates/ruflo-storage/Cargo.toml`
- Create: `src/crates/ruflo-storage/src/lib.rs`
- Create: `src/crates/ruflo-storage/src/port.rs`
- Create: `src/crates/ruflo-storage/src/migration.rs`
- Create: `tests/persistence_migration.rs`
- Create: `tests/fixtures/persistence/legacy-empty.db`

**Interfaces:**
- Produces `PersistencePort::{open, begin_migration, backup, commit, rollback}`.

- [ ] **Step 1: Write failing rollback and permission tests**

```rust
#[test]
fn failed_migration_preserves_original_and_creates_owner_only_backup() {
    let result = migrate_fixture("tests/fixtures/persistence/legacy-empty.db", MigrationPlan::failing());
    assert!(result.is_err());
    assert_original_matches_fixture();
    assert_backup_has_owner_only_permissions();
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test persistence_migration`

Expected: FAIL because the port and migration code are absent.

- [ ] **Step 3: Implement the transaction protocol**

Acquire a project-scoped lock, validate schema/ownership, create a same-filesystem backup, write a migration marker, validate the postcondition, and atomically commit. On any error, rollback and retain recovery metadata. Do not log database values, keys, or paths outside the configured project root.

- [ ] **Step 4: Run migration tests**

Run: `cargo test --test persistence_migration`

Expected: PASS for success, lock conflict, rollback, and permission cases.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add safe persistence migration ports"
```

### Task 9: Integrate existing rUv RVF adapters through a thin storage facade

**Requirements:** REQ-005, REQ-006, REQ-017

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/crates/ruflo-storage/Cargo.toml`
- Create: `src/crates/ruflo-storage/src/rvf_adapter.rs`
- Create: `tests/rvf_interop.rs`
- Create: `tests/fixtures/rvf/README.md`

**Interfaces:**
- Consumes: upstream `rvf-runtime`, `rvf-adapter-agentdb`, and `rvf-adapter-agentic-flow` through pinned revisions.
- Produces `RvfPersistencePort` implementing `PersistencePort` without encoding RVF bytes directly.

- [ ] **Step 1: Add failing interoperation tests**

```rust
#[test]
fn adapter_round_trip_preserves_unknown_segments_and_stable_search_order() {
    let store = RvfPersistencePort::open_fixture("tests/fixtures/rvf/agentdb-compatible.rvf").unwrap();
    assert_eq!(store.search(query(), 3).unwrap(), expected_fixture_results());
    assert_unknown_segments_survive_compaction(store).unwrap();
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test rvf_interop`

Expected: FAIL because upstream dependencies and facade are absent.

- [ ] **Step 3: Add exact upstream revision pins and facade**

Use published versions or immutable Git revisions after verifying the actual adapter API. Implement only object translation; do not hand-encode headers, indexes, vectors, quantisation, or witness data. Reject a migration when fixture validation cannot prove interoperability.

- [ ] **Step 4: Run interop tests**

Run: `cargo test --test rvf_interop`

Expected: PASS against AgentDB and Agentic Flow fixture corpus.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src tests Cargo.lock
git commit -m "feat: integrate native RVF persistence adapters"
```

## Phase 4 — Controlled execution, hooks, and Wave 2 transport

### Task 10: Add native hook and plugin action boundary

**Requirements:** REQ-009, REQ-010, REQ-015, REQ-016

**Files:**
- Create: `src/crates/ruflo-actions/Cargo.toml`
- Create: `src/crates/ruflo-actions/src/lib.rs`
- Create: `src/crates/ruflo-actions/src/manifest.rs`
- Create: `src/crates/ruflo-actions/src/executor.rs`
- Create: `tests/native_actions.rs`
- Create: `tests/fixtures/plugins/declarative-plugin.json`

**Interfaces:**
- Produces `ActionManifest::validate`, `NativeActionExecutor::execute(ActionRequest)`.

- [ ] **Step 1: Write failing injection and allowlist tests**

```rust
#[test]
fn shell_metacharacters_never_reach_an_executable() {
    let request = ActionRequest::from_untrusted("echo $(whoami)");
    assert!(NativeActionExecutor::default().execute(request).is_err());
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test native_actions`

Expected: FAIL because action validation is absent.

- [ ] **Step 3: Implement manifest and executor**

Accept only a versioned declarative manifest and enum-based native action names. Validate structured argument fields, canonicalize working directories beneath project root, construct bounded environments, and use direct process argument APIs only for approved binaries. Return `UnsupportedInWave` for JavaScript executable plugins.

- [ ] **Step 4: Run security tests**

Run: `cargo test --test native_actions`

Expected: PASS for allowlist, path escape, shell metacharacter, timeout, and concurrency cases.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add safe native hook action boundary"
```

### Task 11: Add stateless HTTP MCP as a feature-gated Wave 2 adapter

**Requirements:** REQ-003, REQ-004, REQ-009, REQ-011, REQ-014, REQ-016

**Files:**
- Modify: `src/crates/ruflo-mcp/Cargo.toml`
- Create: `src/crates/ruflo-mcp/src/stateless_http.rs`
- Create: `src/crates/ruflo-mcp/src/request_context.rs`
- Create: `tests/mcp_stateless_http.rs`
- Modify: `src/crates/ruflo-config/src/capability_manifest.rs`

**Interfaces:**
- Consumes: `Dispatcher::call`, authenticated `RequestContext`, and `ToolPolicy`.
- Produces: `serve_stateless_http(dispatcher, authn, limits)` behind the `stateless-http` feature.

- [ ] **Step 1: Write failing authorization and no-session tests**

```rust
#[tokio::test]
async fn remote_call_requires_identity_and_uses_explicit_handle() {
    assert_eq!(post_tool(None, "memory_search").await.status(), 401);
    let response = post_tool(valid_identity(), "task_get").await;
    assert!(response.body()["result"]["handle"].is_string());
    assert!(!response.headers().contains_key("Mcp-Session-Id"));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --features stateless-http --test mcp_stateless_http`

Expected: FAIL because the feature and server are absent.

- [ ] **Step 3: Implement only after SDK API verification**

Confirm the selected Rust MCP library supports the required stateless specification APIs. Validate issuer, audience, expiry, and per-tool capability context before dispatcher invocation. Enforce body, rate, timeout, and concurrency limits. Emit tool/method routing metadata and cache only deterministic discovery responses.

- [ ] **Step 4: Run stateless and shared-dispatcher tests**

Run: `cargo test --features stateless-http --test mcp_stateless_http --test mcp_stdio`

Expected: PASS; the same tool schema and policy denial occur over both transports.

- [ ] **Step 5: Commit**

```bash
git add src tests Cargo.lock
git commit -m "feat: add guarded stateless MCP transport"
```

## Phase 5 — Cross-platform release proof and deferred waves

### Task 12: Implement platform hook fixtures and native release matrix

**Requirements:** REQ-002, REQ-007, REQ-009, REQ-015, REQ-016

**Files:**
- Create: `.github/workflows/compatibility.yml`
- Create: `tests/platform_hooks.rs`
- Create: `scripts/release-smoke.sh`
- Create: `scripts/release-smoke.ps1`
- Create: `docs/release/platform-support.md`

**Interfaces:**
- Produces platform artifacts and a native-runner smoke report per target.

- [ ] **Step 1: Write failing platform behaviour tests**

```rust
#[test]
fn generated_hook_uses_tokenized_native_arguments_not_shell_pipelines() {
    let hook = generate_hook(&TargetPlatform::Windows);
    assert!(!hook.contains("/bin/bash"));
    assert!(!hook.contains("cmd.exe /c"));
    assert!(hook.contains("ruflo"));
}
```

- [ ] **Step 2: Run locally to verify failure**

Run: `cargo test --test platform_hooks`

Expected: FAIL because generator and fixtures are absent.

- [ ] **Step 3: Add matrix and smoke scripts**

Build all five target triples, execute native fixture tests on each supported native runner, check binary aliases, stdio protocol cleanliness, paths, locks, signal/cancellation, and hook rendering. Make release scripts fail closed when an expected artifact or signature/SBOM is absent.

- [ ] **Step 4: Run local checks**

Run: `cargo test --test platform_hooks && bash scripts/release-smoke.sh --local`

Expected: PASS locally; CI performs all platform-native checks.

- [ ] **Step 5: Commit**

```bash
git add .github tests scripts docs/release
git commit -m "ci: verify native Ruflo release matrix"
```

### Task 13: Add supply-chain, SBOM, and reproducibility gates

**Requirements:** REQ-006, REQ-007, REQ-009, REQ-017

**Files:**
- Create: `deny.toml`
- Create: `scripts/audit-supply-chain.sh`
- Create: `scripts/generate-sbom.sh`
- Create: `docs/security/supply-chain-policy.md`
- Modify: `.github/workflows/compatibility.yml`

**Interfaces:**
- Produces a passing `cargo audit`, `cargo deny`, and SBOM report for each release candidate.

- [ ] **Step 1: Write failing policy test script**

```bash
test -f Cargo.lock
cargo audit --deny warnings
cargo deny check advisories bans licenses sources
```

- [ ] **Step 2: Run it to verify failure**

Run: `bash scripts/audit-supply-chain.sh`

Expected: FAIL until lockfile and policy configuration exist.

- [ ] **Step 3: Configure allowlist and auditing**

Allow only the approved permissive licence set, deny unapproved registries, and fail on advisories without an explicitly documented, time-limited exception. Generate CycloneDX or SPDX SBOM from the locked dependency graph and record the artifact digest.

- [ ] **Step 4: Run supply-chain checks**

Run: `bash scripts/audit-supply-chain.sh && bash scripts/generate-sbom.sh --check`

Expected: PASS with an auditable dependency report.

- [ ] **Step 5: Commit**

```bash
git add deny.toml scripts docs/security .github Cargo.lock
git commit -m "ci: enforce Rust supply-chain policy"
```

### Task 14: Gate Wave 2 and Wave 3 expansion on evidence

**Requirements:** REQ-010, REQ-011, REQ-012, REQ-013

**Files:**
- Create: `docs/compatibility/wave-2-entry-criteria.md`
- Create: `docs/compatibility/wave-3-entry-criteria.md`
- Create: `tests/capability_manifest.rs`
- Modify: `src/crates/ruflo-config/src/capability_manifest.rs`

**Interfaces:**
- Produces `CapabilityManifest::validate_release(wave)` used by release CI.

- [ ] **Step 1: Write failing release-manifest test**

```rust
#[test]
fn wave_two_cannot_be_marked_supported_without_transport_and_auth_fixtures() {
    let manifest = CapabilityManifest::from_test_fixture("tests/fixtures/capabilities/wave-2-incomplete.json");
    assert!(manifest.validate_release(2).is_err());
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test capability_manifest`

Expected: FAIL because the release gate is absent.

- [ ] **Step 3: Define evidence gates**

Require named consumer fixtures, security tests, cross-platform results, migration tests, and maintained dependency review before a Wave 2 or Wave 3 contract can move from `unsupported` to `supported`. Require an ADR for each newly selected long-lived integration.

- [ ] **Step 4: Run release-gate tests**

Run: `cargo test --test capability_manifest`

Expected: PASS; incomplete evidence blocks release promotion.

- [ ] **Step 5: Commit**

```bash
git add docs/compatibility tests src
git commit -m "test: gate Ruflo compatibility wave promotion"
```

## Plan self-review

### Requirement coverage

- REQ-001: Tasks 1 and 4.
- REQ-002: Tasks 2, 3, 4, and 12.
- REQ-003: Tasks 2, 3, 6, and 11.
- REQ-004: Tasks 1, 6, 7, and 11.
- REQ-005: Tasks 2, 3, 8, and 9.
- REQ-006: Tasks 3, 9, and 13.
- REQ-007: Tasks 4, 12, and 13.
- REQ-008: Tasks 1, 4, 5, 6, 7, and 8.
- REQ-009: Tasks 5, 6, 10, 11, 12, and 13.
- REQ-010: Tasks 3, 7, 10, and 14.
- REQ-011: Tasks 6, 11, and 14.
- REQ-012: Tasks 3, 5, 6, and 14.
- REQ-013: Tasks 3, 7, and 14.
- REQ-014: Task 11.
- REQ-015: Tasks 10 and 12.
- REQ-016: Tasks 5, 10, and 11.
- REQ-017: Tasks 1, 8, 9, and 13.

### Placeholder scan

The task steps name their files, public interfaces, failing tests, verification commands, and commit boundaries. Wave 2 library API selection is deliberately gated by an explicit verification step rather than an invented API.

### Type consistency

`RufloError`, `Capability`, `ToolPolicy`, `Dispatcher`, `RequestContext`, `Runtime`, and `PersistencePort` originate in Tasks 1, 5, 6, 7, and 8 before later tasks consume them.
