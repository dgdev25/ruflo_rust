# Native Codex acceptance and rollout

The TypeScript `claude-flow-codex` executable is the behavioral oracle for
this phase. Fixture capture is limited to deterministic commands that neither
create project files nor launch a Codex or Claude worker.

## Acceptance ledger

- [x] Inventory the live Codex integration and identify its native boundaries.
- [x] Verify that the local oracle executable is available and capture its
  safe command surface: `--version`, `dual templates`, `dual run` with no
  worker configuration, and help output.
- [ ] Add a native `claude-flow-codex` façade that exactly replays the safe
  fixture contract.
- [ ] Capture redacted source-oracle fixtures for each safe command and add
  differential tests for the Rust façade.
- [ ] Implement the native dual-run scheduler behind explicit policy and
  worktree boundaries; it must launch `codex exec` only when a caller invokes
  a worker-running command.
- [ ] Add parity fixtures for invalid worker specifications, status, and loop
  lifecycle without executing a model provider.
- [ ] Pilot the native façade as an opt-in rUvNet consumer command/MCP child.
- [ ] Publish a release promotion report only after consumer evidence and all
  native runner checks are green.

## Observed oracle contract

The installed executable identifies as `claude-flow-codex` version `3.0.1`.
Its dual-mode command exposes the `feature`, `security`, and `refactor`
templates. A missing worker configuration prints guidance and exits without
starting a worker. Worker-bearing invocations use a distinct process boundary:
the TypeScript orchestrator ultimately calls `codex exec --sandbox
workspace-write --skip-git-repo-check <prompt>` (or read-only for explicitly
read-only workers).

This boundary keeps the pure-Rust rebuild free of provider credentials: the
native scheduler owns validation, policy, state, and cancellation; Codex CLI
remains the explicitly invoked local worker executable.
